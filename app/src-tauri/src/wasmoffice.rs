//! Generic local backend host, running a WebAssembly module (SQLite-in-WASI)
//! embedded in the desktop daemon. It lets a module's read/CRUD API work fully
//! offline with no core round-trip; the core is only used for the optional sync
//! (handled elsewhere). Two modules are hosted, sharing one ABI:
//!   • `office`  — `documents-core.wasm` (office documents: CRUD + content + export)
//!   • `drive`   — `drive-core.wasm`     (Drive tree + file metadata, ingest via sync)
//!
//! ABI (manual marshalling, agreed with the core side — no component model):
//!   • `alloc(len: u32) -> u32`  — allocate `len` bytes in linear memory, return ptr
//!   • `handle(req_ptr: u32, req_len: u32) -> u64`
//!       request buffer (one alloc): [method_len u32 LE][method][path_len u32 LE][path][body_len u32 LE][body]
//!       return = (res_ptr as u64) << 32 | (res_len as u32) ;
//!       result buffer at res_ptr: [status u16 LE][ctype_len u16 LE][ctype UTF-8][body bytes...]
//!       (the module sets ctype per response; `status == 0` means "not mine" → the
//!        caller falls through to the core proxy)
//!       The guest reuses a static result buffer → the host COPIES it before the next `handle` call
//!       (guaranteed here: calls are serialized per (module, instance) and read fully under the lock).
//!
//! WASI: the daemon preopens `<instance>/<module>/` as `/data`; the module does
//! its own SQLite + file I/O there. Identity is passed as `KUBUNO_USER_ID`.
//!
//! Lifecycle: the wasmtime `Store` (warm SQLite) is created ONCE per (module,
//! instance) and kept alive; each request is one `handle` call. Each module's
//! artifact is loaded at runtime — absent → that module is disabled and its
//! requests fall through to the core proxy.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

use wasmtime::{Engine, Instance, Linker, Memory, Module, Store, TypedFunc};
use wasmtime_wasi::preview1::{self, WasiP1Ctx};
use wasmtime_wasi::{DirPerms, FilePerms, WasiCtxBuilder};

type AllocFn = TypedFunc<u32, u32>;
type HandleFn = TypedFunc<(u32, u32), u64>;

/// Static description of a hosted module: its name, the env var that overrides its
/// artifact path, the default artifact filename, and its per-instance data subdir.
#[derive(Clone, Copy)]
pub struct Spec {
    pub name:   &'static str,
    pub env:    &'static str,
    pub file:   &'static str,
    pub subdir: &'static str,
}

/// Office documents backend (local-first CRUD + content + export).
pub const OFFICE: Spec = Spec {
    name:   "office",
    env:    "KUBUNO_OFFICE_WASM",
    file:   "documents-core.wasm",
    subdir: "office",
};

/// Drive backend (local-first tree + file metadata, fed by the drive delta sync).
pub const DRIVE: Spec = Spec {
    name:   "drive",
    env:    "KUBUNO_DRIVE_WASM",
    file:   "drive-core.wasm",
    subdir: "drive",
};

/// One live instance of a hosted module (warm SQLite via WASI).
struct WasmInst {
    store:  Store<WasiP1Ctx>,
    alloc:  AllocFn,
    handle: HandleFn,
    memory: Memory,
}

fn engine() -> &'static Engine {
    static E: OnceLock<Engine> = OnceLock::new();
    E.get_or_init(Engine::default)
}

/// Compiled-module cache, one entry per module name (None = artifact absent).
fn modules() -> &'static Mutex<HashMap<&'static str, Option<Module>>> {
    static M: OnceLock<Mutex<HashMap<&'static str, Option<Module>>>> = OnceLock::new();
    M.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Compile (once) and return the module for `spec`, or None if its artifact isn't
/// shipped yet. `Module` is Arc-backed, so the clone is cheap.
fn compiled(spec: Spec) -> Option<Module> {
    let mut m = modules().lock().unwrap_or_else(|p| p.into_inner());
    if !m.contains_key(spec.name) {
        let c = wasm_path(spec).and_then(|path| match Module::from_file(engine(), &path) {
            Ok(md) => {
                eprintln!("[wasmhost] {} chargé : {}", spec.name, path.display());
                Some(md)
            }
            Err(e) => {
                eprintln!("[wasmhost] {} échec de chargement ({}) : {e}", spec.name, path.display());
                None
            }
        });
        m.insert(spec.name, c);
    }
    m.get(spec.name).cloned().flatten()
}

/// Resolve a module's artifact path: explicit env override, then next to the
/// executable, then the config dir. Returns None if none exists.
fn wasm_path(spec: Spec) -> Option<PathBuf> {
    if let Ok(p) = std::env::var(spec.env) {
        let p = PathBuf::from(p);
        if p.is_file() {
            return Some(p);
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let p = dir.join(spec.file);
            if p.is_file() {
                return Some(p);
            }
        }
    }
    if let Ok(dir) = kubuno_sync::config::config_dir() {
        let p = dir.join(spec.file);
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

/// True if a given module's backend is available (artifact present + compiled).
pub fn enabled_for(spec: Spec) -> bool {
    compiled(spec).is_some()
}

/// Drop the compiled-module cache so the next `enabled_for`/`handle_for` re-checks
/// the artifact paths. Called after the user installs/removes the WASM backends so
/// the change takes effect without a restart.
pub fn invalidate() {
    modules().lock().unwrap_or_else(|p| p.into_inner()).clear();
}

/// Back-compat: the office backend (used by the office document proxy + sync).
pub fn enabled() -> bool {
    enabled_for(OFFICE)
}

fn registry() -> &'static Mutex<HashMap<String, Arc<Mutex<WasmInst>>>> {
    static R: OnceLock<Mutex<HashMap<String, Arc<Mutex<WasmInst>>>>> = OnceLock::new();
    R.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Get (or lazily create + cache) the live `spec` module for `instance_id`.
fn instance_for(spec: Spec, instance_id: &str) -> Result<Arc<Mutex<WasmInst>>, String> {
    let key = format!("{}:{}", spec.name, instance_id);
    {
        let reg = registry().lock().unwrap_or_else(|p| p.into_inner());
        if let Some(w) = reg.get(&key) {
            return Ok(w.clone());
        }
    }
    let w = Arc::new(Mutex::new(create(spec, instance_id)?));
    registry()
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .insert(key, w.clone());
    Ok(w)
}

/// Instantiate a module for an instance: preopen its data dir as `/data`, pass the
/// user id via env, and resolve the ABI exports.
fn create(spec: Spec, instance_id: &str) -> Result<WasmInst, String> {
    let module = compiled(spec).ok_or_else(|| format!("module {} indisponible", spec.name))?;

    let data_dir = data_dir(spec, instance_id)?;
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
        .instantiate(&mut store, &module)
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

    Ok(WasmInst { store, alloc, handle, memory })
}

/// `<instance>/<module>/` — holds the module's SQLite + files, written by the wasm.
fn data_dir(spec: Spec, instance_id: &str) -> Result<PathBuf, String> {
    kubuno_sync::db_path(instance_id)
        .map_err(|e| e.to_string())?
        .parent()
        .map(|p| p.join(spec.subdir))
        .ok_or_else(|| format!("chemin {} introuvable", spec.name))
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

/// Route one request to a module's local backend. Returns `(status, content_type,
/// body)`, or `None` if that module is disabled (caller falls through to proxy).
pub fn handle_for(
    spec: Spec,
    instance_id: &str,
    method: &str,
    path: &str,
    body: &[u8],
) -> Option<(u16, String, Vec<u8>)> {
    if !enabled_for(spec) {
        return None;
    }
    let w = match instance_for(spec, instance_id) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("[wasmhost] init {} {instance_id} : {e}", spec.name);
            return Some((500, json_ct(), err_json(&e)));
        }
    };
    let mut guard = w.lock().unwrap_or_else(|p| p.into_inner());
    match call(&mut guard, method, path, body) {
        Ok(r) => Some(r),
        Err(e) => {
            eprintln!("[wasmhost] handle {} {method} {path} : {e}", spec.name);
            Some((500, json_ct(), err_json(&e.to_string())))
        }
    }
}

/// Back-compat: route an office request to the office backend.
pub fn handle(
    instance_id: &str,
    method: &str,
    path: &str,
    body: &[u8],
) -> Option<(u16, String, Vec<u8>)> {
    handle_for(OFFICE, instance_id, method, path, body)
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
    w: &mut WasmInst,
    method: &str,
    path: &str,
    body: &[u8],
) -> wasmtime::Result<(u16, String, Vec<u8>)> {
    let alloc = w.alloc.clone();
    let handle = w.handle.clone();
    let memory = w.memory;
    let store = &mut w.store;

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
