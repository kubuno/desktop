//! ShellNewEntryExtensions (mirrors ShellNewEntryExtensions.cs)
//!
//! Port of `Files.App/Extensions/ShellNewEntryExtensions.cs`.
//!
//! NOTE: the C# counterpart lives under `Extensions/`; a strict mirror would
//! place it in `extensions/shell_new_entry_extensions.rs`, a new top-level
//! module that would require declaring `mod extensions;` in `main.rs`. To
//! avoid touching `main.rs`, the type is temporarily housed under
//! `data/items/` (next to its `ShellNewEntry`) — to move to `extensions/`
//! at Time B.

use std::path::{Path, PathBuf};

use super::shell_new_entry::{ShellNewEntry, ShellNewKind};

/// Resolves a `FileName` template's path. Absolute and existing → as is;
/// otherwise tries the classic template folders (like Explorer).
fn resolve_template_path(template: &str) -> Option<PathBuf> {
    let p = Path::new(template);
    if p.is_absolute() {
        return p.exists().then(|| p.to_path_buf());
    }
    // %APPDATA%\Microsoft\Windows\Templates (user templates folder).
    if let Ok(appdata) = std::env::var("APPDATA") {
        let cand =
            Path::new(&appdata).join("Microsoft").join("Windows").join("Templates").join(template);
        if cand.exists() {
            return Some(cand);
        }
    }
    // %SystemRoot%\ShellNew (legacy system templates).
    if let Ok(win) = std::env::var("SystemRoot") {
        let cand = Path::new(&win).join("ShellNew").join(template);
        if cand.exists() {
            return Some(cand);
        }
    }
    None
}

/// `ShellNewEntryExtensions.Create`: creates the file according to the
/// template's form. `dir` is the target folder, `file_name` the name
/// (extension included). Returns `true` on success. Never panics (any
/// error → `false`).
pub fn create_from_template(entry: &ShellNewEntry, dir: &str, file_name: &str) -> bool {
    let dest = Path::new(dir).join(file_name);
    match &entry.kind {
        // Null template → `CreateFileAsync` of an empty file.
        ShellNewKind::NullFile => std::fs::write(&dest, b"").is_ok(),
        // Data → `WriteBytesAsync(shellEntry.Data)`.
        ShellNewKind::Data(bytes) => std::fs::write(&dest, bytes).is_ok(),
        // FileName → `CopyAsync` of the template to the target folder.
        ShellNewKind::FileName(template) => match resolve_template_path(template) {
            Some(src) => std::fs::copy(&src, &dest).is_ok(),
            None => false,
        },
        // Command → placeholder substitution not ported (TODO), ignored.
        ShellNewKind::Command(_) => false,
    }
}
