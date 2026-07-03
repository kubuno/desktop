//! Claims-driven map of the local-first WASM components.
//!
//! The core's component manifest (`GET /api/v1/desktop/wasm`) declares, for each
//! component, its owning `module` and the API route prefixes it `claims`. We
//! persist that mapping to `components.json` beside the artifacts, so routing
//! works offline and across restarts, and the proxy needs NO hardcoded prefixes:
//! a new `<module>-core.wasm` published by the core starts routing without a
//! desktop change.
//!
//! A claimed prefix is actually routed only when:
//!   • its component artifact is installed (user opt-in, `enabled_for`), and
//!   • the prefix is PRIMED for the instance — its local store has been fed by a
//!     first successful pull. Routing an unprimed prefix would serve empty local
//!     listings that mask the server data (the trap documented in
//!     COORDINATION_WASM.md). The push-capable prefixes (documents, drive) are
//!     primed by design: their sync loops run since v1.
//!   • mutations only pass for PUSHABLE prefixes (a replay loop exists); other
//!     claims are GET-only so offline edits can't get trapped in a local outbox.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::wasmoffice::{self, Spec};

/// One hosted component, as declared by the core manifest.
#[derive(Clone)]
pub struct Component {
    pub name:   String,
    pub module: String,
    pub claims: Vec<String>,
}

/// Prefixes whose local mutations are replayed to the core by a push loop
/// (office_sync for documents, drive_push for drive, office_entities for the
/// office sub-modules). Everything else is GET-only until its push loop lands.
const PUSHABLE: [&str; 6] = [
    "/api/v1/office/documents",
    "/api/v1/drive",
    "/api/v1/office/spreadsheets",
    "/api/v1/office/presentations",
    "/api/v1/office/diagrams",
    "/api/v1/office/whiteboard/boards",
];

/// Builtin fallback for installs that predate the `module`/`claims` manifest
/// fields (or before the first successful manifest fetch).
fn builtin() -> Vec<Component> {
    vec![
        Component {
            name:   "documents-core.wasm".into(),
            module: "office".into(),
            claims: vec!["/api/v1/office/documents".into()],
        },
        Component {
            name:   "drive-core.wasm".into(),
            module: "drive".into(),
            claims: vec!["/api/v1/drive".into()],
        },
    ]
}

fn store_path() -> Option<PathBuf> {
    kubuno_sync::config::config_dir().ok().map(|d| d.join("components.json"))
}

/// Persist the component map from a fetched core manifest (name/module/claims per
/// entry; entries without the new fields fall back to the builtin defaults).
/// Called on every successful manifest fetch so the map tracks the core.
pub fn persist_manifest(manifest: &serde_json::Value) {
    let Some(entries) = manifest["components"].as_array() else { return };
    let defaults = builtin();
    let mut out = Vec::new();
    for c in entries {
        let Some(name) = c["name"].as_str() else { continue };
        let fallback = defaults.iter().find(|d| d.name == name);
        let module = c["module"]
            .as_str()
            .map(String::from)
            .or_else(|| fallback.map(|d| d.module.clone()));
        let claims: Vec<String> = match c["claims"].as_array() {
            Some(a) => a.iter().filter_map(|v| v.as_str().map(String::from)).collect(),
            None => fallback.map(|d| d.claims.clone()).unwrap_or_default(),
        };
        let Some(module) = module else { continue };
        if claims.is_empty() {
            continue;
        }
        out.push(serde_json::json!({ "name": name, "module": module, "claims": claims }));
    }
    if out.is_empty() {
        return;
    }
    if let Some(p) = store_path() {
        let _ = std::fs::write(p, serde_json::json!({ "components": out }).to_string());
        cache().lock().unwrap_or_else(|e| e.into_inner()).take();
    }
}

type CacheSlot = Option<(Instant, Vec<Component>)>;

fn cache() -> &'static Mutex<CacheSlot> {
    static C: OnceLock<Mutex<CacheSlot>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(None))
}

/// The known component map: persisted manifest if present, builtin fallback
/// otherwise. Cached for a few seconds (it's consulted per proxied request).
pub fn all() -> Vec<Component> {
    let mut slot = cache().lock().unwrap_or_else(|e| e.into_inner());
    if let Some((at, list)) = slot.as_ref() {
        if at.elapsed() < Duration::from_secs(10) {
            return list.clone();
        }
    }
    let list = load_disk().unwrap_or_else(builtin);
    *slot = Some((Instant::now(), list.clone()));
    list
}

fn load_disk() -> Option<Vec<Component>> {
    let raw = std::fs::read_to_string(store_path()?).ok()?;
    let v: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let mut out = Vec::new();
    for c in v["components"].as_array()? {
        let (Some(name), Some(module)) = (c["name"].as_str(), c["module"].as_str()) else {
            continue;
        };
        let claims: Vec<String> = c["claims"]
            .as_array()
            .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
            .unwrap_or_default();
        if !claims.is_empty() {
            out.push(Component { name: name.into(), module: module.into(), claims });
        }
    }
    if out.is_empty() { None } else { Some(out) }
}

/// True when `path` sits under `prefix` (exact or a `/`-separated descendant).
fn covers(prefix: &str, path: &str) -> bool {
    path == prefix
        || (path.len() > prefix.len() && path.starts_with(prefix) && path.as_bytes()[prefix.len()] == b'/')
}

/// Marker dropped by a prefix's pull loop after its first successful full pull.
fn primed_marker(instance_id: &str, spec: Spec, prefix: &str) -> Option<PathBuf> {
    let sane: String = prefix
        .trim_start_matches("/api/v1/")
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    kubuno_sync::db_path(instance_id)
        .ok()?
        .parent()
        .map(|p| p.join(spec.subdir).join(format!("primed_{sane}")))
}

/// Prefixes primed by design: their sync loops predate the primed markers and
/// keep the store fed from the very first cycle.
const ALWAYS_PRIMED: [&str; 2] = ["/api/v1/office/documents", "/api/v1/drive"];

fn primed(instance_id: &str, spec: Spec, prefix: &str) -> bool {
    if ALWAYS_PRIMED.contains(&prefix) {
        return true;
    }
    primed_marker(instance_id, spec, prefix).is_some_and(|p| p.is_file())
}

/// Record that a prefix completed its first full pull: from now on the proxy
/// serves it from the local store. Called by the prefix's pull loop.
pub fn mark_primed(instance_id: &str, spec: Spec, prefix: &str) {
    if let Some(p) = primed_marker(instance_id, spec, prefix) {
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(p, "1");
    }
}

/// Full component status for the launcher UI: per component, whether its
/// artifact is installed and, per claimed prefix, whether the prefix is primed
/// (first pull done → truly offline-capable) and pushable (offline mutations
/// replay to the core).
pub fn status(instance_id: &str) -> Vec<serde_json::Value> {
    all()
        .iter()
        .map(|c| {
            let spec = wasmoffice::spec_for(&c.name, &c.module);
            let installed = wasmoffice::enabled_for(spec);
            serde_json::json!({
                "name": c.name,
                "module": c.module,
                "installed": installed,
                "claims": c.claims.iter().map(|p| serde_json::json!({
                    "prefix": p,
                    "primed": primed(instance_id, spec, p),
                    "pushable": PUSHABLE.contains(&p.as_str()),
                })).collect::<Vec<_>>(),
            })
        })
        .collect()
}

/// The component serving `path` locally for this instance, if any: longest
/// installed + primed claim. Returns the wasm spec and whether mutations may be
/// routed (a push loop exists for the prefix).
pub fn route_for(instance_id: &str, path: &str) -> Option<(Spec, bool)> {
    let mut best: Option<(&str, Spec)> = None;
    let comps = all();
    // Longest-prefix wins so a specific claim overrides a broader one.
    let mut claim_of: HashMap<&str, &Component> = HashMap::new();
    for c in &comps {
        for p in &c.claims {
            claim_of.insert(p.as_str(), c);
        }
    }
    for (prefix, c) in claim_of {
        if !covers(prefix, path) {
            continue;
        }
        if best.as_ref().is_some_and(|(b, _)| b.len() >= prefix.len()) {
            continue;
        }
        let spec = wasmoffice::spec_for(&c.name, &c.module);
        if wasmoffice::enabled_for(spec) && primed(instance_id, spec, prefix) {
            best = Some((prefix, spec));
        }
    }
    best.map(|(prefix, spec)| (spec, PUSHABLE.contains(&prefix)))
}
