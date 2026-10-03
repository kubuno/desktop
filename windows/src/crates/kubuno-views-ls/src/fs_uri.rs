//! `lsp_types::Uri` ⇄ filesystem path conversion.
//!
//! `lsp-types` 0.97 represents a document URI as its own minimal RFC 3986
//! `Uri` (backed by `fluent-uri`), not `url::Url` — it has no
//! `to_file_path`/`from_file_path` of its own (unlike the `url::Url` older
//! `lsp-types` releases, and most other LSP server crates in the ecosystem,
//! use). Rather than hand-roll percent-decoding and Windows drive-letter
//! handling (`file:///C:/foo` → `C:\foo`) against `fluent-uri`'s lower-level
//! API, this module round-trips through `url::Url`'s already-correct,
//! well-tested implementation via the string form both crates agree on.

use std::path::{Path, PathBuf};

use lsp_types::Uri;

/// `file://` URI → filesystem path. `None` for a non-`file` scheme or a URI
/// `url::Url` cannot turn into a path (e.g. one with a `host` component on a
/// platform where that is not meaningful).
pub fn to_path(uri: &Uri) -> Option<PathBuf> {
    let url = url::Url::parse(uri.as_str()).ok()?;
    url.to_file_path().ok()
}

/// Filesystem path → `file://` URI. `None` only when the path is not
/// absolute (`url::Url::from_file_path`'s own requirement) or not valid
/// UTF-8 once turned into a URI string.
pub fn from_path(path: &Path) -> Option<Uri> {
    let url = url::Url::from_file_path(path).ok()?;
    url.as_str().parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_a_windows_path() {
        let path = Path::new(r"C:\projects\kubuno\settings_view.kbview");
        let uri = from_path(path).expect("a valid file URI");
        assert!(uri.as_str().starts_with("file:///C:/"), "{}", uri.as_str());
        let back = to_path(&uri).expect("a valid path");
        assert_eq!(back, path);
    }

    #[test]
    fn non_file_scheme_is_not_a_path() {
        let uri: Uri = "https://example.com/a.kbview".parse().unwrap();
        assert!(to_path(&uri).is_none());
    }
}
