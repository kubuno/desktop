//! Local "documents-core" backend, run as a WebAssembly module (SQLite-in-WASI)
//! embedded in the desktop daemon. It lets the office *documents* API work fully
//! offline — create/list/open/rename/delete + edit/save + export — with no core
//! round-trip. The core is only used for the OPTIONAL sync (handled elsewhere).
//!
//! ABI (manual marshalling, agreed with the office side — no component model):
//!   • `alloc(len: u32) -> u32`  — allocate `len` bytes in linear memory, return ptr
//!   • `handle(req_ptr: u32, req_len: u32) -> u64`
//!       request buffer (one alloc): [method_len u32 LE][method][path_len u32 LE][path][body_len u32 LE][body]
//!       return = (res_ptr as u64) << 32 | (res_len as u32) ;
//!       result buffer at res_ptr: [status u16 LE][ctype_len u16 LE][ctype UTF-8][body bytes...]
//!       (res_len = total length; the module sets ctype per response — JSON for metadata,
//!        application/octet-stream for .kbdoc content and DOCX/ODT exports)
//!       The guest reuses a static result buffer → the host COPIES it before the next `handle` call
//!       (guaranteed here: calls are serialized per instance and the result is read fully under the lock).
//!
//! WASI: the daemon preopens `<instance>/office/` as `/data`; the module does its
//! own SQLite + .kbdoc I/O there. Identity is passed as the `KUBUNO_USER_ID` env
//! var so the module scopes data per user.
//!
//! Lifecycle: the wasmtime `Store` (and its warm SQLite connection) is created
//! ONCE per instance and kept alive; each request is one `handle` call. The
//! module artifact is loaded at runtime — if it isn't present yet, the whole
//! feature is disabled and office requests fall through to the core proxy.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

use wasmtime::{Engine, Instance, Linker, Memory, Module, Store, TypedFunc};
use wasmtime_wasi::preview1::{self, WasiP1Ctx};
use wasmtime_wasi::{DirPerms, FilePerms, WasiCtxBuilder};

type AllocFn = TypedFunc<u32, u32>;
type HandleFn = TypedFunc<(u32, u32), u64>;

/// One live instance of the documents-core module (warm SQLite via WASI).
struct OfficeWasm {
    store:  Store<WasiP1Ctx>,
    alloc:  AllocFn,
    handle: HandleFn,
    memory: Memory,
}

fn engine() -> &'static Engine {
    static E: OnceLock<Engine> = OnceLock::new();
    E.get_or_init(Engine::default)
}

/// The compiled module (None if the artifact isn't shipped yet → feature off).
fn module() -> &'static Option<Module> {
    static M: OnceLock<Option<Module>> = OnceLock::new();
    M.get_or_init(|| {
        let path = wasm_path()?;
        match Module::from_file(engine(), &path) {
            Ok(m) => {
                eprintln!("[wasmoffice] module chargé : {}", path.display());
                Some(m)
            }
            Err(e) => {
                eprintln!("[wasmoffice] échec de chargement ({}) : {e}", path.display());
                None
            }
        }
    })
}

/// Resolve the `documents-core.wasm` path: explicit env override, then next to
/// the executable, then the config dir. Returns None if none exists.
fn wasm_path() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("KUBUNO_OFFICE_WASM") {
        let p = PathBuf::from(p);
        if p.is_file() {
            return Some(p);
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let p = dir.join("documents-core.wasm");
            if p.is_file() {
                return Some(p);
            }
        }
    }
    if let Ok(dir) = kubuno_sync::config::config_dir() {
        let p = dir.join("documents-core.wasm");
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

/// True if the local office backend is available (artifact present + compiled).
pub fn enabled() -> bool {
    module().is_some()
}

fn registry() -> &'static Mutex<HashMap<String, Arc<Mutex<OfficeWasm>>>> {
    static R: OnceLock<Mutex<HashMap<String, Arc<Mutex<OfficeWasm>>>>> = OnceLock::new();
    R.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Get (or lazily create + cache) the live module for `instance_id`.
fn instance_for(instance_id: &str) -> Result<Arc<Mutex<OfficeWasm>>, String> {
    {
        let reg = registry().lock().unwrap_or_else(|p| p.into_inner());
        if let Some(ow) = reg.get(instance_id) {
            return Ok(ow.clone());
        }
    }
    let ow = Arc::new(Mutex::new(create(instance_id)?));
    registry()
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .insert(instance_id.to_string(), ow.clone());
    Ok(ow)
}

/// Instantiate the module for an instance: preopen its `office/` data dir as
/// `/data`, pass the user id via env, and resolve the ABI exports.
fn create(instance_id: &str) -> Result<OfficeWasm, String> {
    let module = module().as_ref().ok_or("module office indisponible")?;

    let data_dir = office_dir(instance_id)?;
    std::fs::create_dir_all(&data_dir).map_err(|e| e.to_string())?;
    let user_id = user_id_for(instance_id, &data_dir);

    let wasi = WasiCtxBuilder::new()
        .preopened_dir(&data_dir, "/data", DirPerms::all(), FilePerms::all())
        .map_err(|e| e.to_string())?
        .env("KUBUNO_USER_ID", &user_id)
        .build_p1();

    let mut store = Store::new(engine(), wasi);
    let mut linker: Linker<WasiP1Ctx> = Linker::new(engine());
    preview1::add_to_linker_sync(&mut linker, |t| t).map_err(|e| e.to_string())?;
    let instance: Instance = linker
        .instantiate(&mut store, module)
        .map_err(|e| e.to_string())?;

    // WASI reactor modules (no `main`) export `_initialize`; call it once after
    // instantiation to set up their runtime before any `handle` call.
    if let Ok(init) = instance.get_typed_func::<(), ()>(&mut store, "_initialize") {
        init.call(&mut store, ()).map_err(|e| e.to_string())?;
    }

    let memory = instance
        .get_memory(&mut store, "memory")
        .ok_or("export `memory` absent")?;
    let alloc = instance
        .get_typed_func::<u32, u32>(&mut store, "alloc")
        .map_err(|e| e.to_string())?;
    let handle = instance
        .get_typed_func::<(u32, u32), u64>(&mut store, "handle")
        .map_err(|e| e.to_string())?;

    Ok(OfficeWasm { store, alloc, handle, memory })
}

/// `<instance>/office/` — holds `docs.db` + `<doc_id>.kbdoc`, written by the wasm.
fn office_dir(instance_id: &str) -> Result<PathBuf, String> {
    kubuno_sync::db_path(instance_id)
        .map_err(|e| e.to_string())?
        .parent()
        .map(|p| p.join("office"))
        .ok_or_else(|| "chemin office introuvable".to_string())
}

/// The user id to scope data by: the cached server UUID if known, else the
/// instance id (one account per instance, so it's a stable per-user scope).
fn user_id_for(instance_id: &str, data_dir: &std::path::Path) -> String {
    std::fs::read_to_string(data_dir.join("user_id"))
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| instance_id.to_string())
}

/// Route one office request to the local backend. Returns `(status, content_type,
/// body)`, or `None` if the feature is disabled (caller falls through to proxy).
pub fn handle(
    instance_id: &str,
    method: &str,
    path: &str,
    body: &[u8],
) -> Option<(u16, String, Vec<u8>)> {
    if !enabled() {
        return None;
    }
    let ow = match instance_for(instance_id) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("[wasmoffice] init {instance_id} : {e}");
            return Some((500, json_ct(), err_json(&e)));
        }
    };
    let mut guard = ow.lock().unwrap_or_else(|p| p.into_inner());
    match call(&mut guard, method, path, body) {
        Ok(r) => Some(r),
        Err(e) => {
            eprintln!("[wasmoffice] handle {method} {path} : {e}");
            Some((500, json_ct(), err_json(&e.to_string())))
        }
    }
}

fn json_ct() -> String {
    "application/json".to_string()
}

fn err_json(msg: &str) -> Vec<u8> {
    format!("{{\"error\":{}}}", serde_json::Value::String(msg.into())).into_bytes()
}

/// One `handle` round-trip: build the framed request, copy it into linear memory
/// in a single `alloc`, call, then read+copy the
/// `[status u16][ctype_len u16][ctype][body]` result.
fn call(
    ow: &mut OfficeWasm,
    method: &str,
    path: &str,
    body: &[u8],
) -> wasmtime::Result<(u16, String, Vec<u8>)> {
    let alloc = ow.alloc.clone();
    let handle = ow.handle.clone();
    let memory = ow.memory;
    let store = &mut ow.store;

    // Frame: [method_len u32 LE][method][path_len u32 LE][path][body_len u32 LE][body]
    let mut req = Vec::with_capacity(12 + method.len() + path.len() + body.len());
    req.extend_from_slice(&(method.len() as u32).to_le_bytes());
    req.extend_from_slice(method.as_bytes());
    req.extend_from_slice(&(path.len() as u32).to_le_bytes());
    req.extend_from_slice(path.as_bytes());
    req.extend_from_slice(&(body.len() as u32).to_le_bytes());
    req.extend_from_slice(body);

    let req_ptr = alloc.call(&mut *store, req.len() as u32)?;
    memory.write(&mut *store, req_ptr as usize, &req)?;

    let ret = handle.call(&mut *store, (req_ptr, req.len() as u32))?;
    let res_ptr = (ret >> 32) as usize;
    let res_len = (ret & 0xffff_ffff) as usize;

    // result = [status u16][ctype_len u16][ctype][body]; copy out before any
    // further call (the guest reuses a static buffer).
    if res_len < 4 {
        return Ok((502, json_ct(), Vec::new()));
    }
    let mut buf = vec![0u8; res_len];
    memory.read(&mut *store, res_ptr, &mut buf)?;
    let status = u16::from_le_bytes([buf[0], buf[1]]);
    let ctype_len = u16::from_le_bytes([buf[2], buf[3]]) as usize;
    let ctype_end = (4 + ctype_len).min(buf.len());
    let ctype = String::from_utf8_lossy(&buf[4..ctype_end]).into_owned();
    let body = buf[ctype_end..].to_vec();
    Ok((status, ctype, body))
}
