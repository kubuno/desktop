//! Port of the "New" menu: `Utils/Shell/ShellNewMenuHelper.cs` +
//! `Data/Items/ShellNewEntry.cs` + `Extensions/ShellNewEntryExtensions.cs`.

pub mod shell_new_menu_helper;

pub use shell_new_menu_helper::*;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::items::create_from_template;
    use crate::data::items::shell_new_entry::{ShellNewEntry, ShellNewKind};

    #[test]
    fn lists_entries_without_panic() {
        // The real registry always contains at least ".txt".
        let entries = list_shell_new_entries();
        assert!(
            entries.iter().any(|e| e.extension.eq_ignore_ascii_case(".txt")),
            "« .txt » doit toujours être présent"
        );
        // Sorted by display name: each name is non-empty.
        for e in &entries {
            assert!(!e.display_name.is_empty());
            assert!(e.extension.starts_with('.'));
        }
    }

    #[test]
    fn null_file_creates_empty_file() {
        let dir = std::env::temp_dir();
        let name = format!("files_shell_new_test_{}.txt", std::process::id());
        let entry = ShellNewEntry {
            extension: ".txt".to_string(),
            display_name: "Document texte".to_string(),
            icon: None,
            kind: ShellNewKind::NullFile,
        };
        let ok = create_from_template(&entry, &dir.to_string_lossy(), &name);
        let path = dir.join(&name);
        assert!(ok);
        assert!(path.exists());
        let _ = std::fs::remove_file(path);
    }
}
