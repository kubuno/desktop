//! Port of `Files.Shared/Helpers/FileExtensionHelpers.cs`.
//! Classification of files by extension. Extension lists are load-bearing
//! and mirror the C# source exactly.

use std::path::Path;

/// Extensions of file types that can be digitally signed (mirrors `_signableTypes`).
const SIGNABLE_TYPES: &[&str] = &[
    ".aab", ".apk", ".application", ".appx", ".appxbundle", ".arx", ".cab", ".cat", ".cbx",
    ".cpl", ".crx", ".dbx", ".deploy", ".dll", ".doc", ".docm", ".dot", ".dotm", ".drx",
    ".ear", ".efi", ".exe", ".jar", ".js", ".manifest", ".mpp", ".mpt", ".msi", ".msix",
    ".msixbundle", ".msm", ".msp", ".nupkg", ".ocx", ".pot", ".potm", ".ppa", ".ppam", ".pps",
    ".ppsm", ".ppt", ".pptm", ".ps1", ".psm1", ".psi", ".pub", ".sar", ".stl", ".sys", ".vbs",
    ".vdw", ".vdx", ".vsd", ".vsdm", ".vss", ".vssm", ".vst", ".vstm", ".vsto", ".vsix", ".vsx",
    ".vtx", ".vxd", ".war", ".wiz", ".wsf", ".xap", ".xla", ".xlam", ".xls", ".xlsb", ".xlsm",
    ".xlt", ".xltm", ".xsn",
];

/// Returns the extension of a path including the leading dot, like C# `Path.GetExtension`.
fn get_extension(path: &str) -> &str {
    match path.rfind('.') {
        Some(idx) if !path[idx..].contains(['\\', '/']) => &path[idx..],
        _ => "",
    }
}

/// Check if the file extension matches one of the specified extensions.
///
/// Folder paths that exist on disk are never matched, mirroring the C# guard
/// (files-community/Files#17094).
pub fn has_extension(file_path_to_check: Option<&str>, extensions: &[&str]) -> bool {
    let Some(path) = file_path_to_check else {
        return false;
    };
    if path.trim().is_empty() {
        return false;
    }
    if Path::new(path).is_dir() {
        return false;
    }

    let path_extension = get_extension(path);
    extensions
        .iter()
        .any(|ext| path_extension.eq_ignore_ascii_case(ext))
}

pub fn is_image_file(ext: Option<&str>) -> bool {
    has_extension(
        ext,
        &[".png", ".bmp", ".jpg", ".jpeg", ".jfif", ".gif", ".tiff", ".tif", ".webp", ".jxr"],
    )
}

pub fn is_compatible_to_set_as_windows_wallpaper(ext: Option<&str>) -> bool {
    has_extension(
        ext,
        &[".png", ".bmp", ".jpg", ".jpeg", ".jfif", ".gif", ".tiff", ".tif", ".jxr"],
    )
}

pub fn is_audio_file(ext: Option<&str>) -> bool {
    has_extension(
        ext,
        &[".mp3", ".m4a", ".ogg", ".oga", ".wav", ".wma", ".aac", ".adt", ".adts", ".cda", ".flac"],
    )
}

pub fn is_video_file(ext: Option<&str>) -> bool {
    has_extension(
        ext,
        &[".avi", ".mp4", ".webm", ".ogg", ".mov", ".qt", ".m4v", ".mp4v", ".3g2", ".3gp2", ".3gp", ".3gpp", ".mkv"],
    )
}

pub fn is_power_shell_file(ext: &str) -> bool {
    has_extension(Some(ext), &[".ps1"])
}

pub fn is_batch_file(ext: &str) -> bool {
    has_extension(Some(ext), &[".bat"])
}

pub fn is_zip_file(ext: Option<&str>) -> bool {
    has_extension(
        ext,
        &[".zip", ".msix", ".appx", ".msixbundle", ".appxbundle", ".7z", ".rar", ".tar", ".mcpack", ".mcworld", ".mrpack", ".jar", ".gz", ".lzh"],
    )
}

/// Returns the browsable-archive extension contained in `file_path`, if any.
pub fn browsable_zip_extension(file_path: Option<&str>) -> Option<&'static str> {
    let path = file_path?;
    if path.trim().is_empty() {
        return None;
    }

    const BROWSABLE: &[&str] = &[".zip", ".7z", ".rar", ".tar", ".gz", ".lzh", ".mrpack", ".jar"];
    let lower = path.to_lowercase();
    BROWSABLE.iter().copied().find(|ext| lower.contains(ext))
}

pub fn is_inf_file(ext: Option<&str>) -> bool {
    has_extension(ext, &[".inf"])
}

pub fn is_font_file(ext: Option<&str>) -> bool {
    has_extension(ext, &[".fon", ".otf", ".ttc", ".ttf"])
}

pub fn is_shortcut_file(path: Option<&str>) -> bool {
    has_extension(path, &[".lnk"])
}

pub fn is_web_link_file(path: Option<&str>) -> bool {
    has_extension(path, &[".url"])
}

pub fn is_shortcut_or_url_file(path: Option<&str>) -> bool {
    has_extension(path, &[".lnk", ".url"])
}

pub fn is_executable_file(path: Option<&str>, exe_only: bool) -> bool {
    if exe_only {
        has_extension(path, &[".exe"])
    } else {
        has_extension(path, &[".exe", ".bat", ".cmd", ".ahk"])
    }
}

pub fn is_ahk_file(path: Option<&str>) -> bool {
    has_extension(path, &[".ahk"])
}

pub fn is_cmd_file(path: Option<&str>) -> bool {
    has_extension(path, &[".cmd"])
}

pub fn is_msi_file(path: Option<&str>) -> bool {
    has_extension(path, &[".msi"])
}

pub fn is_vhd_file(ext: Option<&str>) -> bool {
    has_extension(ext, &[".vhd", ".vhdx"])
}

pub fn is_screen_saver_file(ext: Option<&str>) -> bool {
    has_extension(ext, &[".scr"])
}

pub fn is_media_file(path: Option<&str>) -> bool {
    has_extension(
        path,
        &[
            ".mp4", ".m4v", ".mp4v", ".3g2", ".3gp2", ".3gp", ".3gpp",
            ".mpg", ".mp2", ".mpeg", ".mpe", ".mpv", ".mkv", ".ogg", ".avi", ".wmv", ".mov", ".qt",
            ".mp3", ".m4a", ".oga", ".wav", ".wma", ".aac", ".flac",
        ],
    )
}

pub fn is_certificate_file(path: Option<&str>) -> bool {
    has_extension(path, &[".cer", ".crt", ".der", ".pfx"])
}

pub fn is_script_file(path: Option<&str>) -> bool {
    has_extension(path, &[".py", ".ahk", ".bat", ".cmd", ".ps1"])
}

pub fn is_system_file(path: Option<&str>) -> bool {
    has_extension(path, &[".dll", ".exe", ".sys", ".inf"])
}

pub fn is_signable_file(path: Option<&str>, is_extension: bool) -> bool {
    let Some(path) = path else {
        return false;
    };
    if path.trim().is_empty() {
        return false;
    }

    let ext = if is_extension { path } else { get_extension(path) };
    SIGNABLE_TYPES.iter().any(|s| s.eq_ignore_ascii_case(ext))
}

pub fn is_markdown_file(ext: Option<&str>) -> bool {
    has_extension(ext, &[".md", ".markdown"])
}

pub fn is_text_file(ext: Option<&str>) -> bool {
    has_extension(ext, &[".txt"])
}

pub fn is_rich_text_file(ext: Option<&str>) -> bool {
    has_extension(ext, &[".rtf"])
}

pub fn is_pdf_file(ext: Option<&str>) -> bool {
    has_extension(ext, &[".pdf"])
}

pub fn is_html_file(ext: Option<&str>) -> bool {
    has_extension(ext, &[".htm", ".html", ".svg"])
}

pub fn is_image_preview_file(ext: Option<&str>) -> bool {
    has_extension(
        ext,
        &[".png", ".jpg", ".jpeg", ".bmp", ".gif", ".tiff", ".ico", ".webp", ".jxr"],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extension_matching_is_case_insensitive() {
        assert!(is_image_file(Some("photo.PNG")));
        assert!(is_image_file(Some(r"C:\nonexistent\photo.jpeg")));
        assert!(!is_image_file(Some("archive.zip")));
        assert!(!is_image_file(None));
        assert!(!is_image_file(Some("  ")));
    }

    #[test]
    fn get_extension_matches_dotnet_semantics() {
        assert_eq!(get_extension("foo.txt"), ".txt");
        assert_eq!(get_extension("foo"), "");
        assert_eq!(get_extension(r"dir.d\file"), "");
        assert_eq!(get_extension("archive.tar.gz"), ".gz");
    }

    #[test]
    fn signable_lookup() {
        assert!(is_signable_file(Some("setup.exe"), false));
        assert!(is_signable_file(Some(".DLL"), true));
        assert!(!is_signable_file(Some("notes.txt"), false));
    }

    #[test]
    fn browsable_zip() {
        assert_eq!(browsable_zip_extension(Some("a.ZIP")), Some(".zip"));
        assert_eq!(browsable_zip_extension(Some("a.png")), None);
    }
}
