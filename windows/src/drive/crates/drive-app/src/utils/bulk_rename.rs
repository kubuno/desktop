//! Port of the bulk rename ENGINE (BulkRename).
//!
//! Strict fidelity to `Files.App/ViewModels/Dialogs/BulkRenameDialogViewModel.cs`,
//! `Files.App/Dialogs/BulkRenameDialog.xaml(.cs)` and `Files.App/Actions/FileSystem/
//! RenameAction.cs`.
//!
//! ## What the C# actually does
//!
//! The bulk rename dialog is deliberately minimalistic. The user enters ONE
//! name (`FileName`) — without extension or tokens — and each selected item
//! is renamed to `FileName + original extension`:
//!
//! ```csharp
//! // BulkRenameDialogViewModel.DoCommitRenameAsync()
//! foreach (ListedItem item in context.SelectedItems)
//! {
//!     await context.ShellPage.FilesystemHelpers.RenameAsync(
//!         StorageHelpers.FromPathAndType(item.ItemPath, itemType),
//!         FileName + item.FileExtension,          // ← literal name + extension kept
//!         NameCollisionOption.GenerateUniqueName, // ← suffix-based disambiguation
//!         true, false);
//! }
//! ```
//!
//! The entered name must be valid and must NOT contain a dot:
//!
//! ```csharp
//! // BulkRenameDialogViewModel
//! public bool IsNameValid =>
//!     FilesystemHelpers.IsValidForFilename(_FileName) && !_FileName.Contains('.');
//! ```
//!
//! ### The "counter" = the `GenerateUniqueName` collision suffix
//!
//! Since ALL items get the same `FileName`, disambiguation relies entirely
//! on `NameCollisionOption.GenerateUniqueName`. The exact suffix format
//! added by the C# is `name (n)` inserted before the extension:
//!
//! ```csharp
//! // FilesystemOperations.cs (manual fallback for GenerateUniqueName)
//! string nameWithoutExt = Path.GetFileNameWithoutExtension(desiredNewName);
//! string extension = Path.GetExtension(desiredNewName);
//! ushort attempt = 1;
//! do { ... } while (... && ++attempt < 1024);
//! desiredNewName = $"{nameWithoutExt} ({attempt}){extension}";
//! ```
//!
//! ```csharp
//! // FtpStorageFolder.CreateFileAsync — same format, same 1024 cap
//! desiredName = $"{nameWithoutExt} ({attempt}){extension}";
//! ```
//!
//! The port already has its own canonical implementation of
//! `GenerateUniqueName` (`storage::unique_path`), which starts the counter
//! at `2` (Windows Explorer convention: `name.ext`, `name (2).ext`,
//! `name (3).ext`…):
//!
//! ```rust,ignore
//! // utils/storage.rs
//! for n in 2.. {
//!     let candidate = parent.join(format!("{stem} ({n}){ext}"));
//!     if !candidate.exists() { return candidate; }
//! }
//! ```
//!
//! This module reproduces that same suffix format (` (n)` starting at 2)
//! for the preview, to stay consistent with `storage::unique_path`.
//!
//! ## Pattern tokens
//!
//! The C# does NOT expose a `{}` token language in this dialog: the only
//! dynamic element is the collision counter described above. To satisfy
//! the need for an explicit incremental counter (with width/format)
//! requested by the port, this module adds — as a clearly identified
//! EXTENSION — an optional `{n}` token. Without a token, the behavior is
//! rigorously that of the C# (literal name + extension + collision suffix).
//!
//! Supported tokens:
//! - literal text — any character outside braces.
//! - `{n}` — incremental counter, no padding. Reproduces the `attempt`
//!   value of the C# `({attempt})` suffix.
//! - `{n:0N}` — incremental counter with fixed width `N`, left-padded with
//!   zeros (e.g. `{n:000}` → `001`, `002`…). Port extension: the C# never
//!   pads the collision suffix.
//!
//! The original extension of each item is ALWAYS kept and appended at the
//! end (mirrors `FileName + item.FileExtension`).

// NOT WIRED YET. Ported from Files' bulk rename (pattern + counter); waits for the bulk-rename dialog.
// The allow is scoped to this file and must go once it is wired.
#![allow(dead_code)]

use std::collections::HashSet;

/// A segment of the parsed pattern.
#[derive(Debug, Clone, PartialEq)]
enum Segment {
    /// Literal text copied as-is.
    Literal(String),
    /// Counter token `{n}` / `{n:0N}`; `width` = 0 means "no padding"
    /// (reproduces the C# `({attempt})`).
    Counter { width: usize },
}

/// Rename pattern parsed from a string, faithful to the C# (literal name +
/// extension kept) with an optional counter token as a port extension.
#[derive(Debug, Clone, PartialEq)]
pub struct RenamePattern {
    segments: Vec<Segment>,
    /// Whether the original extension is kept (mirrors `FileName +
    /// item.FileExtension`). True by default, like the C#.
    pub keep_extension: bool,
}

impl RenamePattern {
    /// Parses a pattern string. Never panics: any malformed brace is
    /// treated as literal text.
    pub fn parse(pattern: &str) -> Self {
        let mut segments = Vec::new();
        let mut literal = String::new();
        let chars: Vec<char> = pattern.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            if chars[i] == '{' {
                // Looks for the matching closing brace.
                if let Some(close) = chars[i + 1..].iter().position(|&c| c == '}') {
                    let inner: String = chars[i + 1..i + 1 + close].iter().collect();
                    if let Some(token) = parse_counter_token(&inner) {
                        if !literal.is_empty() {
                            segments.push(Segment::Literal(std::mem::take(&mut literal)));
                        }
                        segments.push(token);
                        i += 1 + close + 1;
                        continue;
                    }
                }
                // Unrecognized brace → literal.
                literal.push('{');
                i += 1;
            } else {
                literal.push(chars[i]);
                i += 1;
            }
        }
        if !literal.is_empty() {
            segments.push(Segment::Literal(literal));
        }
        Self { segments, keep_extension: true }
    }

    /// True if the pattern contains a counter token (the name will vary
    /// from one item to the next without relying on the collision suffix).
    pub fn has_counter(&self) -> bool {
        self.segments.iter().any(|s| matches!(s, Segment::Counter { .. }))
    }

    /// Renders the base name (WITHOUT extension) for the given index.
    fn render_base(&self, index: usize) -> String {
        let mut out = String::new();
        for seg in &self.segments {
            match seg {
                Segment::Literal(text) => out.push_str(text),
                Segment::Counter { width } => {
                    out.push_str(&format!("{:0width$}", index, width = *width));
                }
            }
        }
        out
    }
}

/// Parses the content of a brace into a counter token, or `None` if not
/// recognized. Accepts `n`, `n:0N` (N digits).
fn parse_counter_token(inner: &str) -> Option<Segment> {
    let inner = inner.trim();
    if inner == "n" {
        return Some(Segment::Counter { width: 0 });
    }
    // Form `n:0N`: width = number of zeros in the specifier.
    if let Some(spec) = inner.strip_prefix("n:") {
        if !spec.is_empty() && spec.chars().all(|c| c == '0') {
            return Some(Segment::Counter { width: spec.len() });
        }
    }
    None
}

/// Splits a file name into `(base, extension)` following the semantics of
/// the C#'s `Path.GetFileNameWithoutExtension` / `Path.GetExtension`: the
/// extension includes the dot; a leading dot (`.gitignore` files) is not
/// an extension.
fn split_extension(name: &str) -> (&str, &str) {
    match name.rfind('.') {
        // Dot at position 0 → no extension (e.g. ".gitignore").
        Some(0) | None => (name, ""),
        Some(pos) => (&name[..pos], &name[pos..]),
    }
}

/// Renders a name unique within the current batch by adding the C#
/// collision suffix `name (n){ext}` (n starting at 2, cf.
/// `storage::unique_path`). Compares case-insensitively (Windows file
/// systems).
fn make_unique(base: &str, ext: &str, used: &mut HashSet<String>) -> String {
    let candidate = format!("{base}{ext}");
    if used.insert(candidate.to_lowercase()) {
        return candidate;
    }
    for n in 2.. {
        let candidate = format!("{base} ({n}){ext}");
        if used.insert(candidate.to_lowercase()) {
            return candidate;
        }
    }
    unreachable!()
}

/// Previews the rename: returns the list of `(old name → new name)`
/// WITHOUT touching disk (for the dialog's preview list).
///
/// - `pattern`   : pattern string (see tokens above).
/// - `names`     : original file names (name only, not the full path).
/// - `start_index` : first value of the `{n}` counter (the C# starts
///   `attempt` at 1; the caller chooses).
///
/// Collisions (several items producing the same name, typical when the
/// pattern is a simple literal name) are disambiguated by the
/// `GenerateUniqueName` ` (n)` suffix, faithful to the C#.
pub fn preview(pattern: &str, names: &[String], start_index: usize) -> Vec<(String, String)> {
    let pat = RenamePattern::parse(pattern);
    let mut used: HashSet<String> = HashSet::new();
    let mut out = Vec::with_capacity(names.len());
    for (i, name) in names.iter().enumerate() {
        let (_, ext) = split_extension(name);
        let ext = if pat.keep_extension { ext } else { "" };
        let base = pat.render_base(start_index + i);
        let new_name = make_unique(&base, ext, &mut used);
        out.push((name.clone(), new_name));
    }
    out
}

/// Actually applies the rename on disk. Returns the number of successes.
///
/// `renames` is the output of [`preview`] (`(old, new)` pairs, names
/// only). `dir` is the common parent folder. Robust: never calls
/// `panic!`, ignores unchanged pairs, and continues despite individual
/// failures (mirrors the C#'s `foreach` loop, which doesn't stop the
/// batch on error).
pub fn apply(dir: &str, renames: &[(String, String)]) -> usize {
    let mut ok = 0usize;
    for (old, new) in renames {
        if old == new {
            continue;
        }
        let path = std::path::Path::new(dir).join(old);
        let path_str = path.to_string_lossy().into_owned();
        // `catch_unwind` as a precaution: guarantees that any panic from
        // the interop layer doesn't stop the batch.
        let renamed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            super::storage::rename_item(&path_str, new)
        }))
        .unwrap_or(false);
        if renamed {
            ok += 1;
        }
    }
    ok
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn nom_litteral_conserve_extension() {
        // Mirrors the C#: `FileName + item.FileExtension`.
        let r = preview("Photo", &names(&["a.jpg"]), 1);
        assert_eq!(r, vec![("a.jpg".to_string(), "Photo.jpg".to_string())]);
    }

    #[test]
    fn collision_suffixe_generate_unique_name() {
        // Three items, same literal name, identical extensions → suffix (n)
        // starting at 2, faithful to storage::unique_path.
        let r = preview("Photo", &names(&["a.jpg", "b.jpg", "c.jpg"]), 1);
        assert_eq!(
            r,
            vec![
                ("a.jpg".to_string(), "Photo.jpg".to_string()),
                ("b.jpg".to_string(), "Photo (2).jpg".to_string()),
                ("c.jpg".to_string(), "Photo (3).jpg".to_string()),
            ]
        );
    }

    #[test]
    fn extensions_differentes_pas_de_collision() {
        // Identical bases but distinct extensions → distinct full names.
        let r = preview("Photo", &names(&["a.jpg", "b.png"]), 1);
        assert_eq!(
            r,
            vec![
                ("a.jpg".to_string(), "Photo.jpg".to_string()),
                ("b.png".to_string(), "Photo.png".to_string()),
            ]
        );
    }

    #[test]
    fn compteur_simple_sans_remplissage() {
        let r = preview("IMG_{n}", &names(&["x.png", "y.png", "z.png"]), 1);
        assert_eq!(
            r,
            vec![
                ("x.png".to_string(), "IMG_1.png".to_string()),
                ("y.png".to_string(), "IMG_2.png".to_string()),
                ("z.png".to_string(), "IMG_3.png".to_string()),
            ]
        );
    }

    #[test]
    fn compteur_largeur_fixe_zeros() {
        let r = preview("IMG_{n:000}", &names(&["x.png", "y.png"]), 9);
        assert_eq!(
            r,
            vec![
                ("x.png".to_string(), "IMG_009.png".to_string()),
                ("y.png".to_string(), "IMG_010.png".to_string()),
            ]
        );
    }

    #[test]
    fn compteur_evite_les_collisions() {
        // The counter makes each name unique: no (n) suffix added.
        let r = preview("File_{n}", &names(&["a.txt", "b.txt"]), 1);
        assert_eq!(
            r,
            vec![
                ("a.txt".to_string(), "File_1.txt".to_string()),
                ("b.txt".to_string(), "File_2.txt".to_string()),
            ]
        );
    }

    #[test]
    fn fichier_sans_extension() {
        let r = preview("doc", &names(&["readme"]), 1);
        assert_eq!(r, vec![("readme".to_string(), "doc".to_string())]);
    }

    #[test]
    fn point_en_tete_non_traite_comme_extension() {
        // ".gitignore": no extension (Path.GetExtension semantics).
        let r = preview("config", &names(&[".gitignore"]), 1);
        assert_eq!(r, vec![(".gitignore".to_string(), "config".to_string())]);
    }

    #[test]
    fn double_extension_seule_la_derniere() {
        let r = preview("arch", &names(&["data.tar.gz"]), 1);
        assert_eq!(r, vec![("data.tar.gz".to_string(), "arch.gz".to_string())]);
    }

    #[test]
    fn accolade_mal_formee_est_litterale() {
        let r = preview("a{b", &names(&["x.txt"]), 1);
        assert_eq!(r, vec![("x.txt".to_string(), "a{b.txt".to_string())]);
    }

    #[test]
    fn insensible_a_la_casse_pour_collision() {
        let r = preview("Photo", &names(&["a.JPG", "b.jpg"]), 1);
        // "Photo.JPG" then "Photo.jpg" collide (case ignored).
        assert_eq!(
            r,
            vec![
                ("a.JPG".to_string(), "Photo.JPG".to_string()),
                ("b.jpg".to_string(), "Photo (2).jpg".to_string()),
            ]
        );
    }

    #[test]
    fn parse_jetons() {
        assert!(RenamePattern::parse("IMG_{n}").has_counter());
        assert!(RenamePattern::parse("{n:0000}").has_counter());
        assert!(!RenamePattern::parse("Photo").has_counter());
        assert!(!RenamePattern::parse("a{b}c").has_counter());
    }
}
