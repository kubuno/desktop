//! Reading the files a request looks at, preferring the text of the editors that have them open
//! (`vskubuno/docs/EVENTS.md` §5.5, EVT-5).
//!
//! The server only tracks `.kbview` documents; the Rust code-behind lives in rust-analyzer's
//! world. A request that edits the code-behind (create, rename, remove a handler…) must compute
//! its offsets against the text the client will apply the edits to — the editor buffer, which may
//! hold unsaved changes — not against the file on disk. So such requests may carry `openFiles`, a
//! map `uri → text` of the client's open documents, installed for the duration of the request with
//! [`with_overlays`]; every read of a code-behind or sibling view goes through [`read`], which
//! answers from that overlay first, then from the disk.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;

use lsp_types::Uri;

use crate::fs_uri;

thread_local! {
    static OVERLAYS: RefCell<HashMap<String, String>> = RefCell::new(HashMap::new());
}

/// A path as an overlay key: separators unified and lower-cased (Windows paths are
/// case-insensitive, and the client's URIs do not always spell the drive letter like `read_dir`).
pub(crate) fn key(path: &Path) -> String {
    path.to_string_lossy().replace('/', "\\").to_lowercase()
}

/// Runs `f` with `open_files` (client URI → text) readable through [`read`]; the previous overlay
/// is restored afterwards, even if `f` panics.
pub fn with_overlays<R>(open_files: &HashMap<String, String>, f: impl FnOnce() -> R) -> R {
    let map: HashMap<String, String> = open_files
        .iter()
        .filter_map(|(uri, text)| {
            let uri: Uri = uri.parse().ok()?;
            Some((key(&fs_uri::to_path(&uri)?), text.clone()))
        })
        .collect();
    struct Restore(Option<HashMap<String, String>>);
    impl Drop for Restore {
        fn drop(&mut self) {
            if let Some(previous) = self.0.take() {
                OVERLAYS.with(|o| *o.borrow_mut() = previous);
            }
        }
    }
    let _restore = Restore(Some(OVERLAYS.with(|o| std::mem::replace(&mut *o.borrow_mut(), map))));
    f()
}

/// The client's open buffer of `path`, when the current request sent it (never the file).
pub fn read_overlay(path: &Path) -> Option<String> {
    OVERLAYS.with(|o| o.borrow().get(&key(path)).cloned())
}

/// The text of `path`: the client's open buffer when the current request sent it, else the file.
pub fn read(path: &Path) -> Option<String> {
    if let Some(text) = OVERLAYS.with(|o| o.borrow().get(&key(path)).cloned()) {
        return Some(text);
    }
    std::fs::read_to_string(path).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_overlay_wins_over_the_disk_and_is_scoped() {
        let dir = std::env::temp_dir().join(format!("kubuno-views-ls-sources-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("view.rs");
        std::fs::write(&file, "disk").unwrap();
        let uri = fs_uri::from_path(&file).unwrap().as_str().to_string();
        // The client spells the path differently (case): still the same file.
        let open = HashMap::from([(uri.to_uppercase().replace("FILE:///", "file:///"), "buffer".to_string())]);
        assert_eq!(with_overlays(&open, || read(&file)).as_deref(), Some("buffer"));
        assert_eq!(read(&file).as_deref(), Some("disk"));
        std::fs::remove_dir_all(&dir).ok();
    }
}
