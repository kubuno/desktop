//! Port of `Files.Shared/Helpers/PathHelpers.cs`.

use std::path::{Path, PathBuf, MAIN_SEPARATOR};
use std::process::Command;

use super::file_extension_helpers;

/// Combines a folder and a name, preserving forward-slash style when the
/// folder already uses `/` separators (e.g. FTP paths).
pub fn combine(folder: &str, name: &str) -> String {
    if folder.is_empty() {
        return name.to_string();
    }

    let combined = Path::new(folder).join(name).to_string_lossy().into_owned();
    if folder.contains('/') {
        combined.replace('\\', "/")
    } else {
        combined
    }
}

/// Resolves a command name to a full executable path using `where.exe`.
pub fn try_get_full_path(command_name: &str) -> Option<String> {
    let output = Command::new("where.exe").arg(command_name).output().ok()?;
    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout
        .lines()
        .find(|line| file_extension_helpers::is_executable_file(Some(line), false))
        .map(|line| line.to_string())
}

/// Whether `path` points inside the Windows fonts folder.
pub fn is_in_system_fonts_folder(path: &str) -> bool {
    let Ok(full_path) = std::path::absolute(path) else {
        return false;
    };
    let windir = std::env::var_os("WINDIR").unwrap_or_else(|| "C:\\Windows".into());
    let mut fonts = PathBuf::from(windir).join("Fonts").into_os_string();
    fonts.push(MAIN_SEPARATOR.to_string());

    full_path
        .to_string_lossy()
        .to_lowercase()
        .starts_with(&fonts.to_string_lossy().to_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn combine_preserves_forward_slashes() {
        assert_eq!(combine("ftp://host/dir", "file.txt"), "ftp://host/dir/file.txt");
        assert_eq!(combine(r"C:\dir", "file.txt"), r"C:\dir\file.txt");
        assert_eq!(combine("", "file.txt"), "file.txt");
    }

    #[test]
    fn fonts_folder_detection() {
        assert!(is_in_system_fonts_folder(r"C:\Windows\Fonts\arial.ttf"));
        assert!(!is_in_system_fonts_folder(r"C:\Users\someone\arial.ttf"));
    }
}
