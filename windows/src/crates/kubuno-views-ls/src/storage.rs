//! The storage components in views (`vskubuno/docs/STORAGE-COMPONENTS.md`):
//!
//! - the **binding members of a `<Settings>`**: the settings its `.kbsettings` file declares (`Schema`, default
//!   `settings`, looked up in the view's package), with their shapes and whether a two-way binding may write them
//!   (an `Application` setting may not) — completion, hover, unknown-name diagnostics and go-to-definition of
//!   `{Binding Theme, Source=settings}` come from them through [`crate::binding_sources`];
//! - **platform availability**: an element that exists on some platforms only
//!   (`kubuno_views_meta::kbview::PLATFORM_ELEMENTS`: `<RegistryKey>` is Windows only) is a warning in a view of a
//!   project that also targets another platform (`[package.metadata.kubuno] targets = ["windows", "linux"]` in the
//!   package's `Cargo.toml`; a desktop project targets `windows` alone by default);
//! - an information when a `<Settings>` names a set with no `.kbsettings` file: the set is then open (any name, no
//!   default, no typed class).

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use kubuno_resources_model::settings::{SettingEntry, SettingsFile};
use kubuno_views::ast::{AstNode, Element};
use kubuno_views::messages::tr;
use lsp_types::{Diagnostic, DiagnosticSeverity, Location, NumberOrString, Range, Uri};

use crate::binding_sources::{position_in, Member, MemberKind, Shape};
use crate::documents::Document;

const SOURCE: &str = "kubuno-storage";
const SKIPPED: &[&str] = &["target", "bin", "obj", ".git", ".vs", "node_modules"];

type Stamp = (Option<SystemTime>, u64);

thread_local! {
    /// Parsed `.kbsettings` files by path, with the stamp they were parsed at.
    static FILES: RefCell<HashMap<PathBuf, (Stamp, SettingsFile, String)>> = RefCell::new(HashMap::new());
}

fn stamp(path: &Path) -> Stamp {
    std::fs::metadata(path).map(|m| (m.modified().ok(), m.len())).unwrap_or((None, 0))
}

fn kbsettings_files(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    if depth > 8 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        match entry.file_type() {
            Ok(t) if t.is_dir() => {
                if !entry.file_name().to_str().is_some_and(|n| SKIPPED.contains(&n)) && !path.join("Cargo.toml").is_file() {
                    kbsettings_files(&path, depth + 1, out);
                }
            }
            Ok(_) if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("kbsettings")) => out.push(path),
            _ => {}
        }
    }
}

/// The `.kbsettings` file of set `set` in the package of `view` (its stem, compared ignoring case), parsed, with
/// its text.
pub fn settings_file(view: &Path, set: &str) -> Option<(PathBuf, SettingsFile, String)> {
    let root = crate::project::package_root(view)?;
    let mut files = Vec::new();
    kbsettings_files(&root, 0, &mut files);
    files.sort();
    let path = files.into_iter().find(|p| p.file_stem().and_then(|s| s.to_str()).is_some_and(|s| s.eq_ignore_ascii_case(set)))?;
    let s = stamp(&path);
    if let Some(hit) = FILES.with(|f| f.borrow().get(&path).filter(|(st, _, _)| *st == s).map(|(_, file, text)| (file.clone(), text.clone()))) {
        return Some((path, hit.0, hit.1));
    }
    let text = crate::sources::read(&path).or_else(|| std::fs::read_to_string(&path).ok())?;
    let (file, _) = SettingsFile::read(&text);
    FILES.with(|f| f.borrow_mut().insert(path.clone(), (s, file.clone(), text.clone())));
    Some((path, file, text))
}

fn shape_of(e: &SettingEntry) -> Shape {
    match e.ty.as_str() {
        "Bool" => Shape::Bool,
        "Int" | "Float" => Shape::Number,
        "StringList" => Shape::List,
        _ => Shape::Text,
    }
}

/// The set a `<Settings>` element names (`Schema`, default `settings`).
pub fn set_of(element: &Element) -> String {
    element.attribute("Schema").and_then(|a| a.value()).map(|v| v.trim().to_string()).filter(|v| !v.is_empty()).unwrap_or_else(|| "settings".to_string())
}

/// Fills the members of the `<Settings>` component `c` (named `name`) of the view at `view`: one per declared
/// setting. Without a `.kbsettings` file the component has none, and any name is accepted.
pub fn settings_members(view: &Path, element: &Element, name: &str, c: &mut Member) {
    let set = set_of(element);
    c.doc = format!("Settings · {set}.kbsettings");
    let Some((path, file, text)) = settings_file(view, &set) else { return };
    let uri = crate::fs_uri::from_path(&path);
    for e in &file.entries {
        let member_path = format!("{name}.{}", e.name);
        let mut m = Member::new(&e.name, &member_path, MemberKind::Column, shape_of(e));
        m.expression = format!("{{Binding {}, Source={name}{}}}", e.name, if e.is_application() { "" } else { ", Mode=TwoWay" });
        m.writable = !e.is_application();
        m.rust_type = Some(e.ty.clone());
        let scope = if e.is_application() {
            tr("application, read-only", "application, lecture seule")
        } else if e.roaming {
            tr("user", "utilisateur")
        } else {
            tr("user, this machine", "utilisateur, cette machine")
        };
        let default = if e.ty == "StringList" { e.default_items().join(", ") } else { e.default.clone() };
        m.doc = format!("{} ({scope}) · {} `{default}`{}", e.ty, tr("default", "défaut"), if e.description.is_empty() { String::new() } else { format!("\n\n{}", e.description) });
        m.location = uri.clone().map(|uri| Location { uri, range: Range { start: position_in(&text, e.name_range.start), end: position_in(&text, e.name_range.end) } });
        c.children.push(m);
    }
}

/// The platforms the package of `view` targets: `[package.metadata.kubuno] targets = [...]`, else `windows` (the
/// desktop views run on Windows today).
pub fn project_targets(view: &Path) -> Vec<String> {
    let manifest = crate::project::package_root(view).and_then(|root| std::fs::read_to_string(root.join("Cargo.toml")).ok()).unwrap_or_default();
    targets_of(&manifest)
}

fn targets_of(manifest: &str) -> Vec<String> {
    let mut in_section = false;
    for line in manifest.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_section = line == "[package.metadata.kubuno]";
            continue;
        }
        if !in_section {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else { continue };
        if key.trim() != "targets" {
            continue;
        }
        let list: Vec<String> = value
            .trim()
            .trim_start_matches('[')
            .split(']')
            .next()
            .unwrap_or_default()
            .split(',')
            .map(|s| s.trim().trim_matches('"').trim().to_ascii_lowercase())
            .filter(|s| !s.is_empty())
            .collect();
        if !list.is_empty() {
            return list;
        }
    }
    vec!["windows".to_string()]
}

/// The storage diagnostics of a view (see the module doc).
pub fn diagnostics(doc: &Document, uri: &Uri) -> Vec<Diagnostic> {
    let Some(view) = crate::fs_uri::to_path(uri) else { return Vec::new() };
    let root = doc.parse.syntax();
    let elements: Vec<Element> = root.descendants().filter_map(Element::cast).collect();
    let mut out = Vec::new();
    let range_of = |r: rowan::TextRange| Range {
        start: doc.position_index.offset_to_position(&doc.text, r.start()),
        end: doc.position_index.offset_to_position(&doc.text, r.end()),
    };
    let diag = |range: Range, severity: DiagnosticSeverity, code: &str, message: String| Diagnostic {
        range,
        severity: Some(severity),
        code: Some(NumberOrString::String(code.to_string())),
        source: Some(SOURCE.to_string()),
        message,
        ..Default::default()
    };
    let mut targets: Option<Vec<String>> = None;
    for e in &elements {
        let Some(name) = e.name() else { continue };
        let Some(r) = e.name_range() else { continue };
        if let Some((_, platforms)) = kubuno_views_meta::kbview::PLATFORM_ELEMENTS.iter().find(|(n, _)| *n == name) {
            let targets = targets.get_or_insert_with(|| project_targets(&view));
            let missing: Vec<&str> = targets.iter().map(String::as_str).filter(|t| !platforms.contains(t)).collect();
            if !missing.is_empty() {
                let only = platforms.join(", ");
                let others = missing.join(", ");
                out.push(diag(
                    range_of(r),
                    DiagnosticSeverity::WARNING,
                    "platform-only",
                    tr(
                        &format!("`<{name}>` exists on {only} only, and this project also targets {others} ([package.metadata.kubuno] targets): there it reads nothing and its calls fail — keep portable data in a `<Settings>`, or this view to {only}"),
                        &format!("`<{name}>` n'existe que sous {only}, et ce projet cible aussi {others} ([package.metadata.kubuno] targets) : là, il ne lit rien et ses appels échouent — gardez les données portables dans un `<Settings>`, ou réservez cette vue à {only}"),
                    ),
                ));
            }
        }
        if name == "Settings" {
            let set = set_of(e);
            if settings_file(&view, &set).is_none() {
                let r = e.attribute("Schema").and_then(|a| a.value_range()).unwrap_or(r);
                out.push(diag(
                    range_of(r),
                    DiagnosticSeverity::INFORMATION,
                    "open-settings",
                    tr(
                        &format!("no `{set}.kbsettings` in this project: the set is open (any name, no default, no typed class) — add one with Add > New Item > Kubuno Settings File"),
                        &format!("aucun `{set}.kbsettings` dans ce projet : le jeu est ouvert (tout nom, sans défaut ni classe typée) — ajoutez-en un avec Ajouter > Nouvel élément > Fichier de paramètres Kubuno"),
                    ),
                ));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn targets_are_read_from_the_package_metadata() {
        assert_eq!(targets_of("[package]\nname = \"a\"\n"), vec!["windows"]);
        assert_eq!(targets_of("[package]\nname = \"a\"\n\n[package.metadata.kubuno]\nuser-secrets-id = \"x\"\ntargets = [\"Windows\", \"linux\", \"macos\"]\n"), vec!["windows", "linux", "macos"]);
        assert_eq!(targets_of("[package.metadata.kubuno]\ntargets = []\n"), vec!["windows"]);
    }

    fn temp_package(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("kubuno-ls-storage-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src")).expect("mkdir");
        dir
    }

    fn document(text: &str) -> Document {
        Document { parse: kubuno_views::syntax::parse(text), position_index: crate::position::PositionIndex::new(text), text: text.to_string(), version: 1 }
    }

    #[test]
    fn settings_members_come_from_the_kbsettings_file() {
        let dir = temp_package("members");
        std::fs::write(dir.join("Cargo.toml"), "[package]\nname = \"app\"\n").expect("manifest");
        std::fs::write(
            dir.join("src/settings.kbsettings"),
            "<Settings>\n  <Setting Name=\"Theme\" Default=\"System\" Description=\"The theme.\"/>\n  <Setting Name=\"Zoom\" Type=\"Float\" Default=\"1\"/>\n  <Setting Name=\"Channel\" Scope=\"Application\" Default=\"stable\"/>\n</Settings>\n",
        )
        .expect("settings");
        let view = dir.join("src/main_view.kbview");
        let text = "<Panel><Settings x:Name=\"settings\"/></Panel>";
        std::fs::write(&view, text).expect("view");
        let parse = kubuno_views::syntax::parse(text);
        let element = parse.syntax().descendants().filter_map(Element::cast).find(|e| e.name().as_deref() == Some("Settings")).expect("element");
        let mut c = Member::new("settings", "settings", MemberKind::Component, Shape::Any);
        settings_members(&view, &element, "settings", &mut c);
        let names: Vec<&str> = c.children.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, ["Theme", "Zoom", "Channel"]);
        assert_eq!(c.children[1].shape, Shape::Number);
        assert!(!c.children[2].writable, "an Application setting is read-only");
        assert_eq!(c.children[0].expression, "{Binding Theme, Source=settings, Mode=TwoWay}");
        assert!(c.children[0].doc.contains("The theme."));
        assert!(c.children[0].location.is_some());
        // No diagnostic: the set exists, and a desktop project targets Windows.
        let uri = crate::fs_uri::from_path(&view).expect("uri");
        assert!(diagnostics(&document(text), &uri).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The language server knows the storage components from their sources (the scan of `kubuno-app-storage-components`
    /// the `kubuno` facade reaches through its workspace dependencies): kind, Toolbox category, enum properties.
    #[test]
    fn the_storage_components_are_scanned_from_their_crate() {
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("../kubuno-app-storage-components/Cargo.toml");
        let (name, declared) = crate::project::crate_components(&manifest);
        assert_eq!(name.as_deref(), Some("kubuno_app_storage_components"));
        let mut names: Vec<&str> = declared.iter().map(|d| d.name.as_str()).collect();
        names.sort_unstable();
        assert_eq!(names, kubuno_views_meta::kbview::STORAGE_ELEMENTS);
        let settings = declared.iter().find(|d| d.name == "Settings").expect("Settings");
        assert_eq!((settings.kind.as_str(), settings.toolbox_category.as_deref()), ("component", Some("Storage")));
        let backend = settings.properties.iter().find(|p| p.name == "Backend").expect("Backend");
        let kind = format!("{:?}", backend.kind);
        assert!(kind.contains("Registry") && kind.contains("Memory"), "{kind}");
        assert_eq!(settings.default_event.as_deref(), Some("OnSettingChanged"));
    }

    /// Writes the `kubuno/registry` entries of the storage components for a view of a project linking the `kubuno`
    /// crate: the fixture of vskubuno's `StorageToolboxTests` (`tests/Kubuno.Views.Tests/Fixtures/storage.registry.json`).
    /// `KUBUNO_STORAGE_SAMPLE_VIEW=<samples/storage-desktop/src/main_view.kbview> KUBUNO_STORAGE_FIXTURE=<out.json>
    /// cargo test -p kubuno-views-ls storage_registry_fixture -- --ignored`.
    #[test]
    #[ignore = "writes the Visual Studio test fixture; run with --ignored and the two variables"]
    fn storage_registry_fixture() {
        let (Some(view), Some(out)) = (std::env::var_os("KUBUNO_STORAGE_SAMPLE_VIEW"), std::env::var_os("KUBUNO_STORAGE_FIXTURE")) else { return };
        let mut scanner = crate::project::ProjectScanner::new();
        let declared = scanner.refresh(&[PathBuf::from(view)]).expect("scanned");
        kubuno_views::registry::set_declared(declared);
        let export = serde_json::to_value(kubuno_views::registry::export::export()).expect("json");
        let lines: Vec<String> = export["components"]
            .as_array()
            .expect("components")
            .iter()
            .filter(|c| c["crate_name"] == "kubuno_app_storage_components")
            .map(|c| format!("  {c}"))
            .collect();
        assert_eq!(lines.len(), kubuno_views_meta::kbview::STORAGE_ELEMENTS.len());
        std::fs::write(out, format!("[\n{}\n]\n", lines.join(",\n"))).expect("write");
    }

    #[test]
    fn platform_and_open_set_diagnostics() {
        let dir = temp_package("diags");
        std::fs::write(dir.join("Cargo.toml"), "[package]\nname = \"app\"\n\n[package.metadata.kubuno]\ntargets = [\"windows\", \"linux\"]\n").expect("manifest");
        let view = dir.join("src/v.kbview");
        let text = "<Panel>\n  <RegistryKey x:Name=\"k\" Path=\"Software\\X\"/>\n  <Settings x:Name=\"s\" Schema=\"prefs\"/>\n</Panel>";
        std::fs::write(&view, text).expect("view");
        let uri = crate::fs_uri::from_path(&view).expect("uri");
        let d = diagnostics(&document(text), &uri);
        assert_eq!(d.len(), 2, "{d:#?}");
        assert_eq!(d[0].severity, Some(DiagnosticSeverity::WARNING));
        assert!(d[0].message.contains("linux"), "{}", d[0].message);
        assert_eq!(d[0].range.start.line, 1);
        assert_eq!(d[1].severity, Some(DiagnosticSeverity::INFORMATION));
        assert!(d[1].message.contains("prefs.kbsettings"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
