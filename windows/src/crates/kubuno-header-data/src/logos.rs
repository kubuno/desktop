//! The header's pictures — the modules' brand logos and the account's photo — kept on disk so a header
//! looks the same offline as online.
//!
//! Moved here from the shell (`services/logos.rs`) with its I/O made explicit: a [`PictureCache`] is a
//! directory (the caller says which: the shell keeps one per file-sync instance, the other apps one per
//! account), and downloading is the caller's (the shell fetches synchronously through `kubuno-sync`, the
//! apps asynchronously through `kubuno-api-client`): [`PictureCache::lookup`] before, [`PictureCache::store`]
//! after.
//!
//! The source of truth is the web's: the core answers `/api/v1/modules` with each app's `logo_url`, the very
//! file the web launcher shows. A downloaded picture is stored under a name derived from its CONTENT
//! (`<key>-<hash>.<ext>`): a logo or a photo that changes on the server gets a new path, so no path-keyed
//! decode cache of the renderer keeps showing the old one, and an unchanged one is not rewritten. A small
//! index remembers which file answers which URL, for the next start offline.
//!
//! Before the first download (a first start offline, a sample), the fallback is the same web files again,
//! embedded at BUILD time by `build.rs` from `core/frontend/public` — never a copy kept by hand — and written
//! out once to a directory (the user's cache directory by default), because the painters decode from a path.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

mod generated {
    include!(concat!(env!("OUT_DIR"), "/web_logos.rs"));
}

// ── Formats and names ────────────────────────────────────────────────────────────────────────────

/// The extension of an image the painters can draw, from its first bytes: PNG, JPEG, GIF, WebP, ICO, BMP, or
/// an SVG document. `None` for anything else (an HTML error page, an empty answer…), which is then never
/// cached.
pub fn image_extension(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("png")
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("jpg")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("gif")
    } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some("webp")
    } else if bytes.starts_with(&[0, 0, 1, 0]) {
        Some("ico")
    } else if bytes.starts_with(b"BM") {
        Some("bmp")
    } else if is_svg(bytes) {
        Some("svg")
    } else {
        None
    }
}

/// An SVG document: text (a byte-order mark, an XML declaration and comments allowed) whose root element is
/// `<svg`.
fn is_svg(bytes: &[u8]) -> bool {
    let head = &bytes[..bytes.len().min(4096)];
    let Ok(text) = std::str::from_utf8(head).or_else(|e| std::str::from_utf8(&head[..e.valid_up_to()])) else {
        return false;
    };
    let text = text.trim_start_matches('\u{feff}').trim_start();
    text.starts_with('<') && text.contains("<svg")
}

/// FNV-1a, 64 bits: names a file after its content (a cache key, not a security measure).
fn content_hash(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325_u64, |h, &b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3))
}

/// `key` as a file name part: ASCII letters, digits, `-` and `_` only.
fn file_safe(key: &str) -> String {
    key.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '_' }).collect()
}

/// The content-addressed name of `bytes` stored under `key`.
pub fn file_name(key: &str, bytes: &[u8], ext: &str) -> String {
    format!("{}-{:016x}.{ext}", file_safe(key), content_hash(bytes))
}

/// Writes `bytes` to `dir/name` unless that file is already there (its name is its content), through a
/// temporary file so that a reader never sees half a picture.
fn write_once(dir: &Path, name: &str, bytes: &[u8]) -> Option<PathBuf> {
    let dest = dir.join(name);
    if dest.metadata().is_ok_and(|m| m.len() == bytes.len() as u64) {
        return Some(dest);
    }
    std::fs::create_dir_all(dir).ok()?;
    let tmp = dir.join(format!("{name}.{}.tmp", std::process::id()));
    std::fs::write(&tmp, bytes).ok()?;
    if std::fs::rename(&tmp, &dest).is_err() {
        let _ = std::fs::remove_file(&tmp);
        // Another thread (or another Kubuno app sharing the account's cache) may have written the same
        // content meanwhile.
        return dest.is_file().then_some(dest);
    }
    Some(dest)
}

/// Writes `bytes` to `dest` through a temporary file in the same directory (a reader never sees half of
/// it; two apps writing the same file each rename a whole one).
pub fn write_atomic(dest: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = dest.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = dest.with_extension(format!("{}.tmp", std::process::id()));
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, dest).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

/// Whether the launcher's tile grid (`kubuno_shell_controls::AppTileGrid`) paints the logo file at `path`:
/// it paints every format [`image_extension`] accepts, SVG included (`Canvas::vector_icon`).
pub fn tile_grid_draws(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "png" | "jpg" | "gif" | "webp" | "ico" | "bmp" | "svg"))
}

// ── The build-time fallback ──────────────────────────────────────────────────────────────────────

/// The web logo file named `name` (`drive-logo.png`), as embedded at build time.
fn embedded(name: &str) -> Option<&'static [u8]> {
    generated::WEB_LOGOS.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, b)| *b)
}

/// The names of every embedded web logo (tests and diagnostics).
pub fn builtin_names() -> impl Iterator<Item = &'static str> {
    generated::WEB_LOGOS.iter().map(|(n, _)| *n)
}

/// Where the embedded logos are written out by default: the user's cache directory (moved by a sandboxed
/// profile).
pub fn default_builtin_dir() -> Option<PathBuf> {
    Some(kubuno_account::paths::user_cache_dir().ok()?.join("launcher-logos"))
}

/// The embedded web logo named `name` as a file under `dir`, written out on first use.
fn builtin_file(dir: &Path, name: &str) -> Option<PathBuf> {
    let bytes = embedded(name)?;
    let ext = image_extension(bytes)?;
    let stem = name.rsplit_once('.').map_or(name, |(s, _)| s);
    write_once(dir, &file_name(stem, bytes, ext), bytes)
}

/// The embedded copy of the logo a server serves at `url` (`/drive-logo.png`, or a full URL ending so),
/// written out under `dir`.
pub fn builtin_for_url(dir: &Path, url: &str) -> Option<PathBuf> {
    let path = url.split(['?', '#']).next().unwrap_or(url);
    builtin_file(dir, path.rsplit('/').next()?)
}

/// The embedded web logo of app or module `id`, found the way the core finds the one it serves
/// (`logo_url_for`): `<id>-logo.png`, then `<id>-logo.svg`, and the media module's historical
/// `media-listen-logo.png`; written out under `dir`.
pub fn builtin_for_id(dir: &Path, id: &str) -> Option<PathBuf> {
    let mut names = vec![format!("{id}-logo.png"), format!("{id}-logo.svg")];
    if id == "media" {
        names.push("media-listen-logo.png".into());
    }
    names.iter().find_map(|n| builtin_file(dir, n))
}

// ── The per-account (or per-instance) cache ──────────────────────────────────────────────────────

/// Which file answers which URL, per key (an app id, `avatar`): `index.json`.
#[derive(Serialize, Deserialize, Default)]
struct Index {
    #[serde(default)]
    entries: BTreeMap<String, Entry>,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
struct Entry {
    url: String,
    file: String,
}

/// Serialises the read-modify-write of the indexes within the process (refreshes run on background
/// threads; another app sharing the directory only ever replaces the index as a whole).
static INDEX_LOCK: Mutex<()> = Mutex::new(());

/// A directory of downloaded pictures, content-addressed, with the index of the URL each one answers (see
/// the module doc).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PictureCache {
    dir: PathBuf,
}

impl PictureCache {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn read_index(&self) -> Index {
        std::fs::read(self.dir.join("index.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }

    fn write_index(&self, index: &Index) {
        if let Ok(bytes) = serde_json::to_vec_pretty(index) {
            let _ = write_atomic(&self.dir.join("index.json"), &bytes);
        }
    }

    /// The file last stored for `key` from that same `url`, if it is still there.
    pub fn lookup(&self, key: &str, url: &str) -> Option<PathBuf> {
        let _guard = INDEX_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let entry = self.read_index().entries.get(key).cloned()?;
        let path = self.dir.join(&entry.file);
        (entry.url == url && path.is_file()).then_some(path)
    }

    /// Stores `bytes` downloaded from `url` for `key`: under its content's name, recorded in the index; the
    /// previous file of that key goes (unless another key still uses it). `None` when `bytes` is not a picture
    /// (an error page is never cached: the previous picture stays) or the directory is not writable.
    pub fn store(&self, key: &str, url: &str, bytes: &[u8]) -> Option<PathBuf> {
        let ext = image_extension(bytes)?;
        let name = file_name(key, bytes, ext);
        let written = write_once(&self.dir, &name, bytes)?;
        let _guard = INDEX_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let mut index = self.read_index();
        let entry = Entry { url: url.to_string(), file: name.clone() };
        if index.entries.get(key) != Some(&entry) {
            if let Some(old) = index.entries.insert(key.to_string(), entry) {
                if old.file != name && !index.entries.values().any(|e| e.file == old.file) {
                    let _ = std::fs::remove_file(self.dir.join(&old.file));
                }
            }
            self.write_index(&index);
        }
        Some(written)
    }

    /// Forgets the picture of `key` (the account no longer has a photo).
    pub fn forget(&self, key: &str) {
        let _guard = INDEX_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let mut index = self.read_index();
        if let Some(old) = index.entries.remove(key) {
            if !index.entries.values().any(|e| e.file == old.file) {
                let _ = std::fs::remove_file(self.dir.join(&old.file));
            }
            self.write_index(&index);
        }
    }

    /// Keeps a JSON document (`modules.json`, `me.json`) next to the pictures.
    pub fn save_json(&self, name: &str, value: &serde_json::Value) {
        if let Ok(bytes) = serde_json::to_vec(value) {
            if let Err(e) = write_atomic(&self.dir.join(name), &bytes) {
                tracing::debug!("[header] {name} not cached: {e}");
            }
        }
    }

    /// A JSON document kept by [`Self::save_json`].
    pub fn load_json(&self, name: &str) -> Option<serde_json::Value> {
        let bytes = std::fs::read(self.dir.join(name)).ok()?;
        serde_json::from_slice(&bytes).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_are_read_from_the_bytes_not_the_name() {
        assert_eq!(image_extension(b"\x89PNG\r\n\x1a\n...."), Some("png"));
        assert_eq!(image_extension(&[0xFF, 0xD8, 0xFF, 0xE0]), Some("jpg"));
        assert_eq!(image_extension(b"RIFF\0\0\0\0WEBPVP8 "), Some("webp"));
        assert_eq!(image_extension(b"\xEF\xBB\xBF<?xml version=\"1.0\"?>\n<svg viewBox=\"0 0 1 1\"/>"), Some("svg"));
        assert_eq!(image_extension(b"  <svg xmlns=\"http://www.w3.org/2000/svg\"></svg>"), Some("svg"));
        assert_eq!(image_extension(b"<!doctype html><html><body>404</body></html>"), None);
        assert_eq!(image_extension(b""), None);
    }

    #[test]
    fn a_file_is_named_after_its_content() {
        let a = file_name("office/documents", b"one", "png");
        assert!(a.starts_with("office_documents-") && a.ends_with(".png"), "{a}");
        assert_eq!(a, file_name("office/documents", b"one", "png"));
        assert_ne!(a, file_name("office/documents", b"two", "png"), "a changed picture gets a new path");
        assert!(tile_grid_draws(Path::new(r"C:\c\drive-logo-1.png")));
        assert!(tile_grid_draws(Path::new(r"C:\c\office-logo-1.SVG")));
        assert!(!tile_grid_draws(Path::new(r"C:\c\x.html")));
    }

    /// A stored picture answers its URL until another one replaces it; an error page never replaces it; a
    /// forgotten one is gone, file included.
    #[test]
    fn the_cache_keeps_the_last_good_picture_per_url() {
        let tmp = tempfile::tempdir().expect("tmp");
        let cache = PictureCache::new(tmp.path().join("header"));
        assert_eq!(cache.lookup("drive", "/drive-logo.png"), None);
        let png1 = b"\x89PNG\r\n\x1a\nfirst";
        let first = cache.store("drive", "/drive-logo.png", png1).expect("stored");
        assert_eq!(cache.lookup("drive", "/drive-logo.png").as_deref(), Some(first.as_path()));
        assert_eq!(cache.lookup("drive", "/other.png"), None, "another URL is another picture");
        assert_eq!(cache.store("drive", "/drive-logo.png", b"<html>500</html>"), None);
        assert_eq!(cache.lookup("drive", "/drive-logo.png").as_deref(), Some(first.as_path()), "the error page kept the picture");
        let second = cache.store("drive", "/drive-logo.png", b"\x89PNG\r\n\x1a\nsecond").expect("stored");
        assert_ne!(first, second);
        assert!(!first.exists(), "the replaced file went");
        cache.forget("drive");
        assert!(!second.exists());
        assert_eq!(cache.lookup("drive", "/drive-logo.png"), None);
        cache.save_json("modules.json", &serde_json::json!({ "modules": [] }));
        assert_eq!(cache.load_json("modules.json"), Some(serde_json::json!({ "modules": [] })));
        assert_eq!(cache.load_json("me.json"), None);
    }

    /// The fallback is generated from the web host's files: every one is a picture, named as the core serves
    /// it, and the core's own lookup rule finds them.
    #[test]
    fn the_embedded_fallback_is_the_web_hosts_logos() {
        let names: Vec<&str> = builtin_names().collect();
        if names.is_empty() {
            return; // built without `core/frontend/public` next to the checkout
        }
        for (name, bytes) in generated::WEB_LOGOS {
            assert!(name.ends_with("-logo.png") || name.ends_with("-logo.svg"), "{name}");
            assert!(image_extension(bytes).is_some(), "{name} is not an image");
        }
        assert!(!names.iter().any(|n| n.starts_with("kubuno-")), "the product's own mark is not a module");
        let tmp = tempfile::tempdir().expect("tmp");
        let drive = builtin_for_url(tmp.path(), "/drive-logo.png?v=2").expect("drive's web logo is embedded");
        assert_eq!(std::fs::read(&drive).ok().as_deref().and_then(image_extension), Some("png"));
        assert!(builtin_for_id(tmp.path(), "office").is_some_and(|p| p.extension().is_some_and(|e| e == "svg")));
        assert_eq!(builtin_for_id(tmp.path(), "no-such-module"), None);
    }
}
