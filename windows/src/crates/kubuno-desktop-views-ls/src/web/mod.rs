//! The web profile of the views language server (`vskubuno/docs/WEB-VIEWS.md` §5, lot WV-7).
//!
//! One server, two target profiles. A view belongs to the web target when, walking up from its folder, the first
//! project marker found says so (`vskubuno/docs/VIEWS-SPEC.md` §1):
//!
//! 1. `kubuno.views.json` — its `target` (`"web"` / `"desktop"`; a file without `target` is the web compiler's
//!    configuration, so web);
//! 2. a `package.json` depending on `@kubuno/views-compiler`, or a Visual Studio JavaScript project (`*.esproj`) → web;
//! 3. a `Cargo.toml` depending on `kubuno-desktop` / `kubuno-desktop-views` → desktop.
//!
//! Nothing found means desktop: every existing behaviour of the server is unchanged for desktop views.
//!
//! | Module | What |
//! |---|---|
//! | [`project`] | the project: registries (host JSON + project registries), user controls, the web compiler, `.d.ts` generation |
//! | [`ts`] | the TypeScript code-behind, read with oxc |
//! | [`handlers`] | `kubuno/compatibleHandlers`, `createHandler`, `renameHandler`, `removeHandler`, F2 — editing the TS by insertion |
//! | [`language`] | diagnostics, completion, hover, go-to-definition, code actions, binding paths |
//! | [`res`] | `{Res key}`: the web i18n bundles (`locales/<lang>/<ns>.json`) and `.kbres` files |

pub mod handlers;
pub mod language;
pub mod project;
pub mod res;
pub mod ts;

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use lsp_types::Uri;

/// The target of a view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Profile {
    Desktop,
    Web,
}

/// What the profile detection found for a folder: the profile and, for the web, the project root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Detection {
    pub profile: Profile,
    pub root: Option<PathBuf>,
}

/// How long a folder's detection is trusted before the markers are read again.
const DETECTION_TTL: Duration = Duration::from_secs(5);

thread_local! {
    static DETECTIONS: RefCell<HashMap<PathBuf, (Detection, Instant)>> = RefCell::new(HashMap::new());
}

fn read(path: &Path) -> Option<String> {
    std::fs::read_to_string(path).ok()
}

/// The marker of one folder, if it has one.
fn marker(dir: &Path) -> Option<Profile> {
    let config = dir.join("kubuno.views.json");
    if config.is_file() {
        let target = read(&config).and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok()).and_then(|v| v.get("target").and_then(|t| t.as_str()).map(str::to_string));
        return Some(if target.as_deref() == Some("desktop") { Profile::Desktop } else { Profile::Web });
    }
    let package = dir.join("package.json");
    if package.is_file() && read(&package).is_some_and(|t| t.contains("\"@kubuno/views-compiler\"")) {
        return Some(Profile::Web);
    }
    let esproj = std::fs::read_dir(dir).ok().is_some_and(|entries| {
        entries.flatten().any(|e| e.path().extension().is_some_and(|x| x.eq_ignore_ascii_case("esproj")))
    });
    if esproj {
        return Some(Profile::Web);
    }
    let cargo = dir.join("Cargo.toml");
    if cargo.is_file() && read(&cargo).is_some_and(|t| t.contains("kubuno-desktop") || t.contains("kubuno_desktop")) {
        return Some(Profile::Desktop);
    }
    None
}

/// Detects the profile of the view file `view` (see the module doc).
pub fn detect(view: &Path) -> Detection {
    let Some(start) = view.parent() else { return Detection { profile: Profile::Desktop, root: None } };
    if let Some(hit) = DETECTIONS.with(|d| d.borrow().get(start).filter(|(_, at)| at.elapsed() < DETECTION_TTL).map(|(det, _)| det.clone())) {
        return hit;
    }
    let mut found = Detection { profile: Profile::Desktop, root: None };
    let mut dir = Some(start);
    while let Some(d) = dir {
        if let Some(profile) = marker(d) {
            found = Detection { profile, root: (profile == Profile::Web).then(|| d.to_path_buf()) };
            break;
        }
        dir = d.parent();
    }
    DETECTIONS.with(|d| d.borrow_mut().insert(start.to_path_buf(), (found.clone(), Instant::now())));
    found
}

/// A web view: its file, project root and stem.
#[derive(Debug, Clone)]
pub struct WebView {
    pub path: PathBuf,
    pub root: PathBuf,
    pub stem: String,
}

impl WebView {
    pub fn dir(&self) -> &Path {
        self.path.parent().unwrap_or(&self.root)
    }
}

/// The web view `uri` names, `None` for a desktop view (or a URI that is not a file).
pub fn web_view(uri: &Uri) -> Option<WebView> {
    let path = crate::fs_uri::to_path(uri)?;
    let detection = detect(&path);
    let root = detection.root.filter(|_| detection.profile == Profile::Web)?;
    let stem = path.file_stem()?.to_str()?.to_string();
    Some(WebView { path, root, stem })
}

/// Whether `uri` is a view of a web project.
pub fn is_web(uri: &Uri) -> bool {
    web_view(uri).is_some()
}

/// The LSP range of the byte range `start..end` of `text`.
pub fn lsp_range(text: &str, start: usize, end: usize) -> lsp_types::Range {
    let index = crate::position::PositionIndex::new(text);
    let at = |o: usize| index.offset_to_position(text, rowan::TextSize::from(o.min(text.len()) as u32));
    lsp_types::Range { start: at(start), end: at(end) }
}

/// `edits` (byte ranges of `text` and their replacement) as LSP text edits.
pub fn text_edits(text: &str, edits: impl IntoIterator<Item = (usize, usize, String)>) -> Vec<lsp_types::TextEdit> {
    edits.into_iter().map(|(s, e, new_text)| lsp_types::TextEdit { range: lsp_range(text, s, e), new_text }).collect()
}

/// A location in the file `path` at the byte range `start..end` of its `text`.
pub fn location(path: &Path, text: &str, start: usize, end: usize) -> Option<lsp_types::Location> {
    Some(lsp_types::Location { uri: crate::fs_uri::from_path(path)?, range: lsp_range(text, start, end) })
}

/// Forgets every cached detection and project (tests that build projects on disk).
pub fn reset_caches() {
    DETECTIONS.with(|d| d.borrow_mut().clear());
    project::reset();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("kubuno-webls-profile-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("frontend/src/views")).expect("dirs");
        root
    }

    #[test]
    fn the_first_marker_walking_up_decides() {
        reset_caches();
        let root = tree("markers");
        let view = root.join("frontend/src/views/A.kbview");
        // Nothing: desktop (the unchanged default).
        assert_eq!(detect(&view).profile, Profile::Desktop);

        // A module repository: Cargo.toml (backend) at the root, package.json with the compiler in frontend/.
        std::fs::write(root.join("Cargo.toml"), "[package]\nname = \"notes\"\n").expect("cargo");
        std::fs::write(root.join("frontend/package.json"), r#"{"devDependencies":{"@kubuno/views-compiler":"^0.1.0"}}"#).expect("package");
        reset_caches();
        let d = detect(&view);
        assert_eq!(d.profile, Profile::Web);
        assert_eq!(d.root.as_deref(), Some(root.join("frontend").as_path()));

        // kubuno.views.json wins, in either direction.
        std::fs::write(root.join("frontend/src/kubuno.views.json"), r#"{"target":"desktop"}"#).expect("config");
        reset_caches();
        assert_eq!(detect(&view).profile, Profile::Desktop);
        std::fs::write(root.join("frontend/src/kubuno.views.json"), r#"{"sources":["views"]}"#).expect("config");
        reset_caches();
        assert_eq!(detect(&view), Detection { profile: Profile::Web, root: Some(root.join("frontend/src")) });
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn an_esproj_is_web_and_a_desktop_crate_is_desktop() {
        reset_caches();
        let root = tree("esproj");
        std::fs::write(root.join("frontend/Kubuno.Notes.Frontend.esproj"), "<Project/>").expect("esproj");
        assert_eq!(detect(&root.join("frontend/src/views/A.kbcontrol")).profile, Profile::Web);
        std::fs::write(root.join("frontend/src/Cargo.toml"), "[dependencies]\nkubuno-desktop = { path = \"..\" }\n").expect("cargo");
        reset_caches();
        assert_eq!(detect(&root.join("frontend/src/views/A.kbview")).profile, Profile::Desktop);
        let _ = std::fs::remove_dir_all(&root);
    }
}
