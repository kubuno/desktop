//! A web project as the language server sees it: its root (the folder of `kubuno.views.json` /
//! `package.json` / `.esproj`), its element registries (the host's `kbview-registry.web.json` and the
//! project's own control registries, re-read when they change), its views and user controls, and the
//! web compiler loaded with all of them — the same inputs, rules and outputs as `@kubuno/views-compiler`'s
//! `ViewProject` (`core/frontend/packages/views-compiler/src/project.ts`).

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use kubuno_web_views_compiler_core::{CompileOptions, CompileOutput, RegistryInput, Session, UserControlRef};

/// Folders never scanned for views (`SKIP_DIRS` of the compiler package).
const SKIP_DIRS: &[&str] = &["node_modules", ".kubuno", ".git", "dist", "obj", "bin", "target", "coverage", ".vs"];

/// Where the generated declarations go, under the project root (`GENERATED_DIR`).
pub const GENERATED_DIR: &str = ".kubuno/views";

/// How often the project's view list (its user controls) is re-scanned at most.
const RESCAN_EVERY: Duration = Duration::from_secs(8);

type Stamp = (Option<SystemTime>, u64);

fn stamp(path: &Path) -> Stamp {
    std::fs::metadata(path).map(|m| (m.modified().ok(), m.len())).unwrap_or((None, 0))
}

/// `kubuno.views.json`.
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Config {
    pub target: Option<String>,
    pub registries: Vec<String>,
    pub sources: Vec<String>,
    pub host_registry: Option<String>,
}

/// `/`-separated path of `file` relative to `root` (`src/core/shell/menus/AccountMenu.kbcontrol`).
pub fn project_path(root: &Path, file: &Path) -> String {
    let rel = file.strip_prefix(root).unwrap_or(file);
    rel.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect::<Vec<_>>().join("/")
}

/// The same-stem code-behind of a view (`X.ts`, else `X.tsx`), on disk or in an open buffer.
pub fn code_behind_of(view: &Path) -> Option<PathBuf> {
    let stem = view.file_stem()?.to_str()?;
    let dir = view.parent()?;
    ["ts", "tsx"].iter().map(|ext| dir.join(format!("{stem}.{ext}"))).find(|p| p.is_file() || crate::sources::read_overlay(p).is_some())
}

/// Whether `path` is a view file (`.kbview` / `.kbcontrol`).
pub fn is_view_file(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("kbview") || e.eq_ignore_ascii_case("kbcontrol"))
}

fn scan(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    if depth > 16 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let path = entry.path();
        match entry.file_type() {
            Ok(t) if t.is_dir() => {
                if !SKIP_DIRS.contains(&name.as_ref()) {
                    scan(&path, depth + 1, out);
                }
            }
            Ok(_) if is_view_file(&path) => out.push(path),
            _ => {}
        }
    }
}

/// A project registry with its project-local modules rewritten to project-root-relative specifiers
/// (`./controls` next to `src/kbview-controls.json` → `/src/controls`), as `projectRegistryJson` does.
pub fn project_registry_json(root: &Path, file: &Path, text: &str) -> Result<String, String> {
    let mut doc: serde_json::Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    let dir = file.parent().map(|d| project_path(root, d)).unwrap_or_default();
    let rewrite = |m: &mut serde_json::Value| {
        if let Some(s) = m.as_str() {
            if s.starts_with("./") || s.starts_with("../") {
                *m = serde_json::Value::String(format!("/{}", normalize_posix(&format!("{dir}/{s}"))));
            }
        }
    };
    if let Some(components) = doc.get_mut("components").and_then(|c| c.as_array_mut()) {
        for c in components {
            let Some(web) = c.get_mut("web").filter(|w| w.is_object()) else { continue };
            if let Some(m) = web.get_mut("module") {
                rewrite(m);
            }
            if let Some(alts) = web.get_mut("alternates").and_then(|a| a.as_array_mut()) {
                for a in alts {
                    if let Some(m) = a.get_mut("module") {
                        rewrite(m);
                    }
                }
            }
        }
    }
    serde_json::to_string(&doc).map_err(|e| e.to_string())
}

/// `a/./b/../c` → `a/c` (posix, like `path.posix.normalize`).
fn normalize_posix(p: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    for part in p.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out.join("/")
}

/// One web project.
pub struct WebProject {
    pub root: PathBuf,
    pub config: Config,
    pub session: Session,
    /// The registry files and their stamps when loaded (host first).
    registry_files: Vec<(PathBuf, Stamp)>,
    /// Problems loading the registries (no host registry found, invalid JSON).
    pub load_problems: Vec<String>,
    pub views: Vec<PathBuf>,
    scanned: Option<Instant>,
}

impl WebProject {
    fn open(root: &Path) -> Self {
        let config = std::fs::read_to_string(root.join("kubuno.views.json")).ok().and_then(|t| serde_json::from_str::<Config>(&t).ok()).unwrap_or_default();
        let mut project = WebProject {
            root: root.to_path_buf(),
            config,
            session: Session::new(),
            registry_files: Vec::new(),
            load_problems: Vec::new(),
            views: Vec::new(),
            scanned: None,
        };
        project.load_registries();
        project
    }

    /// The host registry: `hostRegistry` of `kubuno.views.json`, else `node_modules/@kubuno/ui/kbview-registry.web.json`
    /// searched upwards from the root.
    fn host_registry(&self) -> Option<PathBuf> {
        if let Some(h) = &self.config.host_registry {
            let p = Path::new(h);
            return Some(if p.is_absolute() { p.to_path_buf() } else { self.root.join(p) });
        }
        let mut dir = Some(self.root.as_path());
        while let Some(d) = dir {
            let candidate = d.join("node_modules").join("@kubuno").join("ui").join("kbview-registry.web.json");
            if candidate.is_file() {
                return Some(candidate);
            }
            dir = d.parent();
        }
        None
    }

    fn load_registries(&mut self) {
        let mut session = Session::new();
        let mut files = Vec::new();
        let mut problems = Vec::new();
        match self.host_registry() {
            Some(host) => match std::fs::read_to_string(&host) {
                Ok(json) => {
                    let label = project_path(&self.root, &host);
                    if let Err(e) = session.add_registry(&RegistryInput { json, label, host: true }) {
                        problems.push(e);
                    }
                    files.push((host.clone(), stamp(&host)));
                }
                Err(e) => problems.push(format!("the element registry `{}` cannot be read: {e}", host.display())),
            },
            None => problems.push(
                "no element registry found (expected node_modules/@kubuno/ui/kbview-registry.web.json, or \"hostRegistry\" in kubuno.views.json)".into(),
            ),
        }
        for reg in &self.config.registries {
            let file = self.root.join(reg);
            files.push((file.clone(), stamp(&file)));
            let result = std::fs::read_to_string(&file)
                .map_err(|e| e.to_string())
                .and_then(|t| project_registry_json(&self.root, &file, &t))
                .and_then(|json| session.add_registry(&RegistryInput { json, label: project_path(&self.root, &file), host: false }));
            if let Err(e) = result {
                problems.push(format!("the project registry `{reg}` cannot be read: {e}"));
            }
        }
        self.session = session;
        self.registry_files = files;
        self.load_problems = problems;
        self.scanned = None;
    }

    /// Reloads the registries when one of their files changed, and re-scans the views (their user
    /// controls) when the last scan is old or `force`. Returns whether anything changed.
    pub fn refresh(&mut self, force: bool) -> bool {
        let mut changed = false;
        if self.registry_files.iter().any(|(p, s)| stamp(p) != *s) {
            tracing::info!("web registry changed under {}: reloading", self.root.display());
            self.load_registries();
            changed = true;
        }
        if force || self.scanned.is_none_or(|t| t.elapsed() >= RESCAN_EVERY) {
            let mut views = Vec::new();
            let sources: Vec<String> = if self.config.sources.is_empty() { vec!["src".into()] } else { self.config.sources.clone() };
            for s in &sources {
                scan(&self.root.join(s), 0, &mut views);
            }
            views.sort();
            if views != self.views || self.scanned.is_none() {
                let controls: Vec<UserControlRef> = views
                    .iter()
                    .filter(|v| v.extension().is_some_and(|e| e.eq_ignore_ascii_case("kbcontrol")))
                    .filter_map(|v| {
                        let name = v.file_stem()?.to_str()?.to_string();
                        let module_file = code_behind_of(v).map(|cb| cb.with_extension("")).unwrap_or_else(|| v.clone());
                        Some(UserControlRef { name, module: format!("/{}", project_path(&self.root, &module_file)) })
                    })
                    .collect();
                self.session.set_user_controls(&controls);
                changed |= views != self.views;
                self.views = views;
            }
            self.scanned = Some(Instant::now());
        }
        changed
    }

    /// The compile options of the view `path` (`ViewProject.compile`).
    pub fn options(&self, path: &Path) -> CompileOptions {
        let code_behind = code_behind_of(path).and_then(|cb| cb.file_stem().and_then(|s| s.to_str()).map(|s| format!("./{s}")));
        CompileOptions { file: project_path(&self.root, path), code_behind, class_name: None, design: false }
    }

    /// Compiles the view `path` whose text is `text`.
    pub fn compile(&self, path: &Path, text: &str) -> CompileOutput {
        let text = text.strip_prefix('\u{feff}').unwrap_or(text);
        kubuno_web_views_compiler_core::compile(text, &self.session.registry, &self.options(path))
    }

    /// The generated files of a view: `(d.ts, check.ts, check.json)`.
    /// Always under `<root>/.kubuno/views`: a view outside the root (or reached through `..`) goes to
    /// `.kubuno/views/_external/<segments of its absolute path>` (see [`generated_base`]), the same rule as
    /// `generatedPaths` of `@kubuno/views-compiler` (core `packages/views-compiler/src/project.ts`).
    pub fn generated_paths(&self, view: &Path) -> (PathBuf, PathBuf, PathBuf) {
        let base = generated_base(&self.root, view);
        let with = |suffix: &str| PathBuf::from(format!("{}{suffix}", base.display()));
        (with(".d.ts"), with(".check.ts"), with(".check.json"))
    }

    /// Writes the generated declarations, check file and span map of `view` (`writeGenerated`), each only when
    /// its content changed. Returns how many files were written.
    pub fn write_generated(&self, view: &Path, out: &CompileOutput) -> usize {
        #[derive(serde::Serialize)]
        struct Map<'a> {
            view: String,
            class_name: &'a str,
            spans: &'a [kubuno_web_views_compiler_core::CheckSpan],
            handlers: &'a [kubuno_web_views_compiler_core::HandlerUse],
        }
        let (dts, check, map) = self.generated_paths(view);
        let map_text = serde_json::to_string(&Map { view: project_path(&self.root, view), class_name: &out.class_name, spans: &out.check_map, handlers: &out.handlers })
            .map(|t| t + "\n")
            .unwrap_or_default();
        [(dts, out.dts.as_str()), (check, out.check.as_str()), (map, map_text.as_str())].into_iter().filter(|(p, t)| write_if_changed(p, t)).count()
    }
}

/// The generated files' path of `view` without their suffix, never outside `<root>/.kubuno/views`:
/// - a view inside `root` (no `..` in its relative path): `.kubuno/views/<relative path>`;
/// - otherwise `.kubuno/views/_external/<segments>`, the segments being the view's path (a leading `\\?\`
///   removed) split on `/` and `\`, empty ones dropped, `:` removed from each (`C:` → `C`; a segment left
///   empty is dropped), `..` → `_up`, `.` dropped (`D:\shared\A.kbview` → `_external\D\shared\A.kbview`).
pub fn generated_base(root: &Path, view: &Path) -> PathBuf {
    let generated = root.join(GENERATED_DIR);
    if let Ok(rel) = view.strip_prefix(root) {
        if rel.components().all(|c| matches!(c, std::path::Component::Normal(_) | std::path::Component::CurDir)) {
            let rel = project_path(root, view);
            return generated.join(rel.replace('/', std::path::MAIN_SEPARATOR_STR));
        }
    }
    let full = view.to_string_lossy();
    let full = full.strip_prefix(r"\\?\").unwrap_or(&full);
    let mut out = generated.join("_external");
    for segment in full.split(['/', '\\']).filter(|s| !s.is_empty()) {
        match segment {
            "." => {}
            ".." => out.push("_up"),
            s => {
                let s = s.replace(':', "");
                if !s.is_empty() {
                    out.push(s);
                }
            }
        }
    }
    out
}

/// Writes `text` unless the file already holds it (keeps tsc's incremental state and watchers quiet).
pub fn write_if_changed(file: &Path, text: &str) -> bool {
    if std::fs::read_to_string(file).is_ok_and(|t| t == text) {
        return false;
    }
    if let Some(dir) = file.parent() {
        if std::fs::create_dir_all(dir).is_err() {
            return false;
        }
    }
    match std::fs::write(file, text) {
        Ok(()) => true,
        Err(e) => {
            tracing::warn!("cannot write {}: {e}", file.display());
            false
        }
    }
}

thread_local! {
    static PROJECTS: RefCell<HashMap<PathBuf, WebProject>> = RefCell::new(HashMap::new());
}

/// Runs `f` with the (refreshed) project rooted at `root`.
pub fn with_project<R>(root: &Path, f: impl FnOnce(&WebProject) -> R) -> R {
    PROJECTS.with(|p| {
        let mut map = p.borrow_mut();
        let project = map.entry(root.to_path_buf()).or_insert_with(|| WebProject::open(root));
        project.refresh(false);
        f(project)
    })
}

/// Refreshes every known project (the main loop's poll); returns whether one of them changed.
pub fn refresh_all(force: bool) -> bool {
    PROJECTS.with(|p| p.borrow_mut().values_mut().fold(false, |changed, project| project.refresh(force) | changed))
}

/// Forgets every cached project (tests).
pub fn reset() {
    PROJECTS.with(|p| p.borrow_mut().clear());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_registry_modules_become_root_relative() {
        let root = Path::new("/p");
        let json = r#"{"components":[{"name":"A","web":{"module":"./AppTileGrid","alternates":[{"module":"../x/B"}]}},{"name":"C","web":{"module":"@ui"}}]}"#;
        let out = project_registry_json(root, Path::new("/p/src/menus/controls.json"), json).expect("json");
        assert!(out.contains(r#""module":"/src/menus/AppTileGrid""#), "{out}");
        assert!(out.contains(r#""module":"/src/x/B""#), "{out}");
        assert!(out.contains(r#""module":"@ui""#), "{out}");
    }

    /// The generated files never leave `<root>/.kubuno/views`, whatever the view's path.
    #[test]
    fn generated_paths_stay_under_the_generated_folder() {
        let root = std::env::temp_dir().join("p");
        let generated = root.join(GENERATED_DIR);
        let inside = root.join("src").join("A.kbview");
        assert_eq!(generated_base(&root, &inside), generated.join("src").join("A.kbview"), "unchanged inside the root");
        let parent = root.parent().expect("parent");
        let outside = parent.join("q").join("B.kbview");
        let dotted = root.join("src").join("..").join("..").join("r").join("C.kbview");
        for view in [&outside, &dotted] {
            let base = generated_base(&root, view);
            assert!(base.starts_with(generated.join("_external")), "{}", base.display());
            assert!(base.components().all(|c| !matches!(c, std::path::Component::ParentDir)), "{}", base.display());
            let full = format!("{}.d.ts", base.display());
            assert!(Path::new(&full).starts_with(&generated), "{full}");
        }
        assert!(generated_base(&root, &dotted).ends_with(Path::new("src").join("_up").join("_up").join("r").join("C.kbview")));
        #[cfg(windows)]
        {
            let root = Path::new(r"C:\p");
            assert_eq!(generated_base(root, Path::new(r"D:\shared\A.kbview")), Path::new(r"C:\p\.kubuno\views\_external\D\shared\A.kbview"));
            assert_eq!(generated_base(root, Path::new(r"\\?\D:\shared\A.kbview")), Path::new(r"C:\p\.kubuno\views\_external\D\shared\A.kbview"));
            assert_eq!(generated_base(root, Path::new(r"C:\p\src\..\..\x\.\A.kbview")), Path::new(r"C:\p\.kubuno\views\_external\C\p\src\_up\_up\x\A.kbview"));
        }
        #[cfg(not(windows))]
        assert_eq!(generated_base(Path::new("/p"), Path::new("/q/A.kbview")), Path::new("/p/.kubuno/views/_external/q/A.kbview"));
    }

    #[test]
    fn project_paths_are_posix() {
        let root = std::env::temp_dir().join("p");
        assert_eq!(project_path(&root, &root.join("src").join("A.kbview")), "src/A.kbview");
        assert_eq!(normalize_posix("src/./a/../b"), "src/b");
    }
}
