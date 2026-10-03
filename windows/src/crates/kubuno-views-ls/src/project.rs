//! The project's own controls (EVT-7b of `vskubuno/docs/EVENTS.md`, §4.3 "Metadata without
//! building"): the `#[derive(Component)]` / `#[derive(UserControl)]` classes of the package a view
//! belongs to — and of its path dependencies — read from their sources with `syn`
//! (`kubuno_views_meta::scan_source`, the very grammar the derive macros compile), and registered
//! with the view registry as **declared** classes (`kubuno_views::registry::set_declared`). From
//! then on completion, validation, hover, go-to-definition, the Properties window and the Toolbox
//! see a custom control as soon as it is typed and saved, before any build.
//!
//! The scan is incremental: each `.rs` file is re-parsed only when its (modified time, length)
//! changed; a file that does not parse keeps its previous result (a class being typed does not
//! flicker out of the registry).

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use kubuno_views::registry::{DeclaredComponent, DeclaredEvent, DeclaredKind, DeclaredProperty};
use kubuno_views_meta::{ComponentDecl, ScanResult, ValueKind};

/// One scanned source file.
struct FileScan {
    stamp: (Option<SystemTime>, u64),
    result: ScanResult,
    /// The 1-based line of each class's `struct` item.
    lines: HashMap<String, u32>,
}

/// One package: its crate name, where its sources are, and its scanned files.
struct Package {
    crate_name: String,
    manifest_stamp: (Option<SystemTime>, u64),
    path_dependencies: Vec<PathBuf>,
    files: HashMap<PathBuf, FileScan>,
}

/// The packages of the open views (and their path dependencies), scanned incrementally.
#[derive(Default)]
pub struct ProjectScanner {
    packages: HashMap<PathBuf, Package>,
    last: Vec<DeclaredComponent>,
}

fn stamp(path: &Path) -> (Option<SystemTime>, u64) {
    let meta = fs::metadata(path).ok();
    (meta.as_ref().and_then(|m| m.modified().ok()), meta.map_or(0, |m| m.len()))
}

/// The editor a property type asks for (`kubuno_views::component::PropertyValue::EDITOR`), read
/// from the type's name and its text (`ty`, as written): `"list"` for `Rows`, `"object"` for
/// `Shared<T>`, `"color"` for `Color` / `ColorValue` / `Option<ColorValue>`, `"lines"` for
/// `Vec<String>`.
fn value_editor(kind: &ValueKind, ty: &str) -> Option<&'static str> {
    let last = |t: &str| t.rsplit("::").next().unwrap_or(t).to_string();
    let inner = ty.strip_prefix("Option<").and_then(|t| t.strip_suffix('>')).map(last);
    match kind {
        ValueKind::Other(t) if t == "Rows" => Some("list"),
        ValueKind::Other(t) if t == "Shared" || t.starts_with("Shared<") => Some("object"),
        ValueKind::Other(t) if t == "Color" || t == "ColorValue" || inner.as_deref() == Some("ColorValue") => Some("color"),
        ValueKind::Other(t) if t == "Vec" && ty.ends_with("<String>") => Some("lines"),
        _ => None,
    }
}

/// Whether a type's editor makes the property bound only (and so bindable): a list or an object.
fn bound_only(editor: Option<&str>) -> bool {
    matches!(editor, Some("list") | Some("object"))
}

/// The folder of the `Cargo.toml` declaring a `[package]` that contains `file`.
pub fn package_root(file: &Path) -> Option<PathBuf> {
    let mut dir = file.parent();
    while let Some(d) = dir {
        let manifest = d.join("Cargo.toml");
        if manifest.is_file() && fs::read_to_string(&manifest).is_ok_and(|t| t.lines().any(|l| l.trim() == "[package]")) {
            return Some(d.to_path_buf());
        }
        dir = d.parent();
    }
    None
}

/// The `[package] name` of a manifest, as a crate name (`my-app` → `my_app`).
pub fn crate_name(manifest: &str) -> Option<String> {
    let mut in_package = false;
    for line in manifest.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_package = line == "[package]";
            continue;
        }
        if in_package {
            if let Some(rest) = line.strip_prefix("name") {
                let value = rest.trim_start().strip_prefix('=')?.trim();
                return Some(value.trim_matches('"').replace('-', "_"));
            }
        }
    }
    None
}

/// The `path = "…"` dependencies of a manifest (`[dependencies]`, one line per dependency), as
/// folders relative to `root`.
pub fn path_dependencies(manifest: &str, root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut in_deps = false;
    for line in manifest.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_deps = line == "[dependencies]";
            continue;
        }
        if !in_deps {
            continue;
        }
        let Some(pos) = line.find("path") else { continue };
        let rest = line[pos + 4..].trim_start();
        let Some(rest) = rest.strip_prefix('=') else { continue };
        let rest = rest.trim_start();
        let Some(rest) = rest.strip_prefix('"') else { continue };
        let Some(end) = rest.find('"') else { continue };
        out.push(root.join(&rest[..end]));
    }
    out
}

/// The `name = { workspace = true … }` (or `name.workspace = true`) dependencies of a manifest
/// (`[dependencies]`), resolved to folders through the `[workspace.dependencies]` of the Cargo
/// workspace above `root`: how the `kubuno` facade reaches `kubuno-print` and `kubuno-data`, whose
/// components an application of that facade uses (`<PrintDocument>`…) without naming those crates.
pub fn workspace_dependencies(manifest: &str, root: &Path) -> Vec<PathBuf> {
    let mut names = Vec::new();
    let mut in_deps = false;
    for line in manifest.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_deps = line == "[dependencies]";
            continue;
        }
        if !in_deps {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else { continue };
        let (key, value) = (key.trim(), value.trim());
        let name = match key.strip_suffix(".workspace") {
            Some(name) if value.starts_with("true") => name,
            Some(_) => continue,
            None if value.starts_with('{') && value.replace(' ', "").contains("workspace=true") => key,
            None => continue,
        };
        names.push(name.trim_matches('"').to_string());
    }
    if names.is_empty() {
        return Vec::new();
    }
    // The workspace root: the nearest folder above the package whose manifest has `[workspace]`.
    let Some((ws_root, ws_manifest)) = root.ancestors().skip(1).find_map(|dir| {
        let text = fs::read_to_string(dir.join("Cargo.toml")).ok()?;
        text.lines().any(|l| l.trim() == "[workspace]").then(|| (dir.to_path_buf(), text))
    }) else {
        return Vec::new();
    };
    let mut table = HashMap::new();
    let mut in_ws = false;
    for line in ws_manifest.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_ws = line == "[workspace.dependencies]";
            continue;
        }
        if !in_ws {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else { continue };
        if let Some(path) = path_dependencies(&format!("[dependencies]\nx = {}", value.trim()), &ws_root).into_iter().next() {
            table.insert(key.trim().trim_matches('"').to_string(), path);
        }
    }
    names.iter().filter_map(|n| table.get(n).cloned()).collect()
}

/// Whether the package at `root` can hold controls of its own: it depends on `kubuno-views` or on the `kubuno`
/// facade (a control library of an application: `uclib = { path = "../UcLib" }`, `kubuno-shell-controls`). The
/// framework's own crates ([`kubuno_views_meta::FRAMEWORK_CRATES`], an explicit list, never a `kubuno` prefix) are
/// not scanned: their classes are the built-in ones — except the facade and the crates of library components
/// ([`kubuno_views_meta::LIBRARY_COMPONENT_CRATES`]: `kubuno-print`, `kubuno-data`…, reached through the facade's
/// workspace dependencies), whose components the language server learns from their sources.
fn is_control_library(root: &Path) -> bool {
    let Ok(manifest) = fs::read_to_string(root.join("Cargo.toml")) else { return false };
    manifest_is_control_library(&manifest)
}

fn manifest_is_control_library(manifest: &str) -> bool {
    let scanned = |n: &str| !kubuno_views_meta::is_framework_crate(n) || kubuno_views_meta::is_library_component_crate(n);
    if !crate_name(manifest).is_some_and(|n| scanned(&n)) {
        return false;
    }
    let mut in_deps = false;
    manifest.lines().any(|line| {
        let line = line.trim();
        if line.starts_with('[') {
            in_deps = line == "[dependencies]";
            return false;
        }
        let key = line.split(['=', '.']).next().unwrap_or_default().trim().trim_matches('"');
        in_deps && (key == "kubuno" || key == "kubuno-views")
    })
}

/// Every `.rs` file under `dir`, recursively, skipping `target`, hidden folders and nested packages (a folder with a
/// `Cargo.toml` of its own): the package's sources, wherever they are (a control added next to `Cargo.toml` is
/// declared from the crate root with a `#[path]`).
fn rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if path.is_dir() {
            if name != "target" && name != "obj" && name != "bin" && !name.starts_with('.') && !path.join("Cargo.toml").is_file() {
                rs_files(&path, out);
            }
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}

/// The 1-based line of `struct <name>` in `text`.
fn struct_line(text: &str, name: &str) -> Option<u32> {
    let needle = format!("struct {name}");
    let mut from = 0;
    while let Some(rel) = text[from..].find(&needle) {
        let at = from + rel;
        let after = text[at + needle.len()..].chars().next();
        if !after.is_some_and(|c| c.is_alphanumeric() || c == '_') {
            return Some(text[..at].matches('\n').count() as u32 + 1);
        }
        from = at + needle.len();
    }
    None
}

impl ProjectScanner {
    pub fn new() -> Self {
        Self::default()
    }

    /// Rescans the packages of `view_files` (and their path dependencies); the packages of views
    /// no longer open are forgotten. Returns the declared classes when they changed since the last
    /// call, `None` otherwise.
    pub fn refresh(&mut self, view_files: &[PathBuf]) -> Option<Vec<DeclaredComponent>> {
        let mut wanted: Vec<PathBuf> = Vec::new();
        for file in view_files {
            if let Some(root) = package_root(file) {
                if !wanted.contains(&root) {
                    wanted.push(root);
                }
            }
        }
        // Path dependencies, one level (their controls are the project's too).
        let mut i = 0;
        while i < wanted.len() {
            let root = wanted[i].clone();
            self.refresh_package(&root);
            if let Some(p) = self.packages.get(&root) {
                for dep in p.path_dependencies.clone() {
                    let dep = fs::canonicalize(&dep).map(|d| strip_verbatim(&d)).unwrap_or(dep);
                    if !wanted.contains(&dep) && is_control_library(&dep) && wanted.len() < 64 {
                        wanted.push(dep);
                    }
                }
            }
            i += 1;
        }
        self.packages.retain(|root, _| wanted.contains(root));
        let declared = self.declared();
        if declared != self.last {
            self.last = declared.clone();
            Some(declared)
        } else {
            None
        }
    }

    fn refresh_package(&mut self, root: &Path) {
        let manifest_path = root.join("Cargo.toml");
        let manifest_stamp = stamp(&manifest_path);
        let package = self.packages.entry(root.to_path_buf()).or_insert_with(|| Package {
            crate_name: String::new(),
            manifest_stamp: (None, u64::MAX),
            path_dependencies: Vec::new(),
            files: HashMap::new(),
        });
        if package.manifest_stamp != manifest_stamp {
            package.manifest_stamp = manifest_stamp;
            let manifest = fs::read_to_string(&manifest_path).unwrap_or_default();
            package.crate_name = crate_name(&manifest).unwrap_or_default();
            let mut deps = path_dependencies(&manifest, root);
            deps.extend(workspace_dependencies(&manifest, root));
            package.path_dependencies = deps;
        }
        let mut files = Vec::new();
        rs_files(root, &mut files);
        package.files.retain(|path, _| files.contains(path));
        for path in files {
            let now = stamp(&path);
            if package.files.get(&path).is_some_and(|f| f.stamp == now) {
                continue;
            }
            let Ok(text) = fs::read_to_string(&path) else { continue };
            match kubuno_views_meta::try_scan_source(&text) {
                Some(result) => {
                    let lines = result.components.iter().filter_map(|c| struct_line(&text, &c.name).map(|l| (c.name.clone(), l))).collect();
                    package.files.insert(path, FileScan { stamp: now, result, lines });
                }
                None => {
                    // Keep the previous result of a file being edited; remember the stamp only once
                    // there is one.
                    if let Some(previous) = package.files.get_mut(&path) {
                        previous.stamp = now;
                    }
                }
            }
        }
    }

    /// The declared classes of every scanned package, in a stable order (package, file, item).
    pub fn declared(&self) -> Vec<DeclaredComponent> {
        let mut roots: Vec<&PathBuf> = self.packages.keys().collect();
        roots.sort();
        let mut all: Vec<(String, &ComponentDecl, PathBuf, Option<u32>)> = Vec::new();
        let mut enums: Vec<(String, Vec<String>)> = Vec::new();
        for root in roots {
            let package = &self.packages[root];
            let mut paths: Vec<&PathBuf> = package.files.keys().collect();
            paths.sort();
            for path in paths {
                let scan = &package.files[path];
                enums.extend(scan.result.enums.iter().cloned());
                for c in &scan.result.components {
                    all.push((package.crate_name.clone(), c, path.clone(), scan.lines.get(&c.name).copied()));
                }
            }
        }
        let decls: Vec<ComponentDecl> = all.iter().map(|(_, c, _, _)| {
            let mut c = (*c).clone();
            kubuno_views_meta::resolve_enums(std::slice::from_mut(&mut c), &enums);
            c
        }).collect();
        decls
            .iter()
            .zip(all.iter())
            .map(|(decl, (crate_name, _, path, line))| to_declared(decl, &decls, Some(crate_name.as_str()), Some(path), *line))
            .collect()
    }
}

/// `\\?\C:\x` → `C:\x` (what `fs::canonicalize` returns on Windows).
fn strip_verbatim(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    match text.strip_prefix(r"\\?\") {
        Some(rest) if !rest.starts_with("UNC") => PathBuf::from(rest),
        _ => path.to_path_buf(),
    }
}

/// The chain of `decl`, resolving a base that is another class of `all`.
pub fn chain_of(decl: &ComponentDecl, all: &[ComponentDecl]) -> Vec<String> {
    fn go(decl: &ComponentDecl, all: &[ComponentDecl], depth: usize) -> Vec<String> {
        let lookup = |name: &str| -> Option<Vec<String>> {
            if depth > 16 {
                return None;
            }
            all.iter().find(|c| c.name == name && c.name != decl.name).map(|base| go(base, all, depth + 1))
        };
        decl.chain(&lookup)
    }
    go(decl, all, 0)
}

/// A scanned class as a [`DeclaredComponent`] (what the registry, and the export Visual Studio
/// reads, take).
pub fn to_declared(decl: &ComponentDecl, all: &[ComponentDecl], crate_name: Option<&str>, file: Option<&Path>, line: Option<u32>) -> DeclaredComponent {
    DeclaredComponent {
        name: decl.name.clone(),
        kind: decl.kind.as_str().to_string(),
        doc: decl.description.clone(),
        crate_name: crate_name.filter(|c| !c.is_empty()).map(str::to_string),
        extends: decl.extends.clone(),
        base_chain: chain_of(decl, all),
        properties: decl
            .properties
            .iter()
            .map(|p| DeclaredProperty {
                name: p.name.clone(),
                kind: match &p.kind {
                    ValueKind::Bool => DeclaredKind::Bool,
                    ValueKind::Number => DeclaredKind::F32,
                    ValueKind::Enum(v) => DeclaredKind::Enum(v.clone()),
                    ValueKind::String | ValueKind::Other(_) => DeclaredKind::String,
                },
                default: p.default_value.clone(),
                doc: p.description.clone(),
                // The category the export gives a property that declares none (so a class read from
                // source and the same class linked into the server list it alike).
                category: Some(p.category.clone().unwrap_or_else(|| {
                    let kind = if matches!(p.kind, ValueKind::Bool) { kubuno_views::registry::PropKind::Bool } else { kubuno_views::registry::PropKind::String };
                    kubuno_views::registry::common::default_category(&p.name, kind).to_string()
                })),
                browsable: p.browsable,
                bindable: p.bindable || bound_only(value_editor(&p.kind, &p.ty)),
                localizable: p.localizable,
                serialization: p.serialization.clone(),
                // A list (`Rows`) or any Rust value (`Shared<T>`): bound only, like the derive says.
                editor: p.editor.clone().or_else(|| value_editor(&p.kind, &p.ty).map(str::to_string)),
                type_converter: p.type_converter.clone(),
            })
            .collect(),
        events: decl
            .events
            .iter()
            .map(|e| DeclaredEvent {
                name: e.name.clone(),
                doc: e.description.clone(),
                category: e.category.to_string(),
                args_type: e.args.clone(),
                browsable: e.browsable,
                inherited_from: None,
                root_only: false,
            })
            .collect(),
        default_event: decl.default_event.clone(),
        default_property: decl.default_property.clone(),
        toolbox_category: decl.toolbox_category.clone(),
        // A toolbox bitmap is named by its absolute path, as the derive registers it.
        toolbox_icon: decl.toolbox_icon.as_deref().map(|icon| match file.and_then(|f| kubuno_views_meta::resolve_toolbox_bitmap(icon, f)) {
            Some(path) => path.to_string_lossy().into_owned(),
            None => icon.to_string(),
        }),
        browsable: decl.browsable,
        view_path: decl.view.clone(),
        source_file: file.map(|f| f.to_string_lossy().to_string()),
        source_line: line,
    }
}

/// The classes of the package whose manifest is `manifest` (a folder or its `Cargo.toml`):
/// `kubuno/crateComponents`, what Visual Studio's "Choisir des éléments…" lists for another crate.
pub fn crate_components(manifest: &Path) -> (Option<String>, Vec<DeclaredComponent>) {
    let root = if manifest.is_dir() { manifest.to_path_buf() } else { manifest.parent().map(Path::to_path_buf).unwrap_or_default() };
    let mut scanner = ProjectScanner::new();
    scanner.refresh_package(&root);
    let name = scanner.packages.get(&root).map(|p| p.crate_name.clone()).filter(|n| !n.is_empty());
    (name, scanner.declared())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_package(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("kubuno-ls-project-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("src/controls")).unwrap();
        dir
    }

    #[test]
    fn manifests_are_read() {
        let m = "[package]\nname = \"my-app\"\nversion = \"0.1.0\"\n\n[dependencies]\nkubuno-views = { path = \"../desktop/kubuno-views\" }\ntracing = \"0.1\"\n";
        assert_eq!(crate_name(m).as_deref(), Some("my_app"));
        let deps = path_dependencies(m, Path::new("C:/p"));
        assert_eq!(deps, [Path::new("C:/p").join("../desktop/kubuno-views")]);
        assert_eq!(struct_line("a\nstruct Foo2 {}\nstruct Foo {}\n", "Foo"), Some(3));
    }

    /// Found live: a user control in a library crate of a facade application (`kubuno = { path = … }` only) had an
    /// empty Properties window — the library was not scanned.
    #[test]
    fn a_library_of_the_facade_holds_controls() {
        assert!(manifest_is_control_library("[package]\nname = \"uclib\"\n\n[dependencies]\nkubuno = { path = \"../kubuno\" }\n"));
        assert!(manifest_is_control_library("[package]\nname = \"uclib\"\n\n[dependencies]\nkubuno.workspace = true\n"));
        assert!(manifest_is_control_library("[package]\nname = \"lib\"\n\n[dependencies]\nkubuno-views = { path = \"x\" }\n"));
        assert!(!manifest_is_control_library("[package]\nname = \"tool\"\n\n[dependencies]\nkubuno-ui = { path = \"x\" }\n"));
        assert!(!manifest_is_control_library("[package]\nname = \"kubuno_views\"\n\n[dependencies]\nkubuno-views-meta = { path = \"x\" }\n"));
        assert!(!manifest_is_control_library("[package]\nname = \"t\"\n\n[dev-dependencies]\nkubuno = { path = \"x\" }\n"));
    }

    /// The framework is an explicit list, not a `kubuno` prefix: a control library named `kubuno-…` is scanned.
    #[test]
    fn kubuno_named_control_libraries_are_scanned_the_framework_is_not() {
        let manifest = |name: &str| format!("[package]\nname = \"{name}\"\n\n[dependencies]\nkubuno-views = {{ path = \"x\" }}\n");
        for name in ["kubuno-shell-controls", "kubuno-acme-widgets", "kubuno_acme_widgets"] {
            assert!(manifest_is_control_library(&manifest(name)), "{name} is a control library");
        }
        for name in ["kubuno-ui", "kubuno_controls", "kubuno-views", "kubuno-views-ls", "kubuno-resources"] {
            assert!(!manifest_is_control_library(&manifest(name)), "{name} is the framework");
        }
        // The facade and the library components' crates are read (their components are not built in).
        for name in ["kubuno", "kubuno-print", "kubuno_data", "kubuno-app-storage-components"] {
            assert!(manifest_is_control_library(&manifest(name)), "{name} declares library components");
        }
    }

    #[test]
    fn workspace_dependencies_are_resolved_through_the_workspace_root() {
        let ws = temp_package("workspace");
        fs::write(
            ws.join("Cargo.toml"),
            "[workspace]\nmembers = [\"crates/facade\"]\n\n[workspace.dependencies]\nkubuno-print = { path = \"crates/kubuno-print\" }\nserde = { version = \"1\" }\n\"quoted\" = { path = \"crates/q\" }\n",
        )
        .unwrap();
        let facade = ws.join("crates/facade");
        fs::create_dir_all(&facade).unwrap();
        let manifest = "[package]\nname = \"kubuno\"\n\n[dependencies]\nkubuno-print = { workspace = true }\nserde.workspace = true\nquoted = { workspace = true, optional = true }\nlocal = { path = \"../local\" }\n\n[dev-dependencies]\nkubuno-print = { workspace = true }\n";
        assert_eq!(workspace_dependencies(manifest, &facade), [ws.join("crates/kubuno-print"), ws.join("crates/q")]);
        assert!(workspace_dependencies("[dependencies]\nserde = \"1\"\n", &facade).is_empty());
        let _ = fs::remove_dir_all(&ws);
    }

    #[test]
    fn a_package_is_scanned_incrementally_and_keeps_a_file_being_typed() {
        let root = temp_package("scan");
        fs::write(root.join("Cargo.toml"), "[package]\nname = \"round-app\"\n").unwrap();
        fs::write(root.join("src/main.rs"), "mod controls;\nfn main() {}\n").unwrap();
        fs::write(root.join("src/main_view.kbview"), "<Panel/>").unwrap();
        let control = root.join("src/controls/round.rs");
        fs::write(
            &control,
            "/// Pill.\n#[derive(Component, Default)]\n#[kubuno(extends = Button, overrides(Control))]\npub struct RoundButton {\n    base: Button,\n    #[property] #[default_value(Shape::Pill)] pub shape: Shape,\n}\n#[derive(PropertyValue)] pub enum Shape { Pill, Square }\n#[derive(Component, Default)] #[kubuno(extends = RoundButton, levels(ButtonBase))] pub struct BigRound { base: RoundButton }\n",
        )
        .unwrap();
        let mut scanner = ProjectScanner::new();
        let declared = scanner.refresh(&[root.join("src/main_view.kbview")]).expect("first scan");
        assert_eq!(declared.len(), 2);
        let round = &declared[0];
        assert_eq!((round.name.as_str(), round.crate_name.as_deref(), round.source_line), ("RoundButton", Some("round_app"), Some(4)));
        assert_eq!(round.doc, "Pill.");
        assert_eq!(round.properties[0].kind, DeclaredKind::Enum(vec!["Pill".into(), "Square".into()]));
        assert_eq!(declared[1].base_chain, ["BigRound", "RoundButton", "Button", "ButtonBase", "Control", "Component"]);
        assert!(scanner.refresh(&[root.join("src/main_view.kbview")]).is_none(), "nothing changed");

        // A file that does not parse (being typed) keeps its classes.
        fs::write(&control, "pub struct RoundButton {").unwrap();
        assert!(scanner.refresh(&[root.join("src/main_view.kbview")]).is_none());
        // Saved again with the class removed: gone.
        fs::write(&control, "pub struct Nothing;\n").unwrap();
        assert_eq!(scanner.refresh(&[root.join("src/main_view.kbview")]).map(|d| d.len()), Some(0));
        let _ = fs::remove_dir_all(&root);
    }
}
