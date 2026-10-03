//! `customization_view_model` (mirrors
//! `ViewModels/Properties/CustomizationViewModel.cs`).
//!
//! The original only offers this tab for a FOLDER (non-archive) or a
//! SHORTCUT (`customizationItemEnabled = !isLibrary && (isFolder && !isArchive
//! || isShortcut)`). It lets you pick a custom icon: a DLL/executable path
//! field (default `SHELL32.dll`), a "Browse" button, a "Restore default"
//! button, and a grid of icons extracted from the file
//! (`ExtractIconsFromDLL`).
//!
//! The port additionally READS the item's CURRENT icon path (read from
//! `desktop.ini` for a folder, or from `IShellLinkW.GetIconLocation` for a
//! `.lnk`) instead of only the `SHELL32.dll` default. The icon grid
//! (`ExtractIconsFromDLL`) and APPLYING the choice (`UpdateIcon` →
//! `SetCustomDirectoryIcon` / `SetCustomFileIcon`) are TODOs.

use crate::views::properties::PropertiesTarget;

/// The Customization tab's data.
pub struct CustomizationModel {
    /// Whether the item accepts a custom icon (folder/shortcut).
    pub loaded: bool,
    /// Whether the target is a shortcut (`IsShortcut`) — drives the write mode
    /// (`SetCustomFileIcon` vs `SetCustomDirectoryIcon`) — TODO: read once the
    /// icon write is ported.
    #[allow(dead_code)]
    pub is_shortcut: bool,
    /// Displayed icon resource path (`IconResourceItemPath`): the current
    /// custom icon if one exists, else the `SHELL32.dll` default.
    pub icon_resource_path: String,
}

impl CustomizationModel {
    pub fn gather(target: &PropertiesTarget) -> Self {
        let empty = || CustomizationModel {
            loaded: false,
            is_shortcut: false,
            icon_resource_path: default_icon_dll_path(),
        };
        let path = match target {
            PropertiesTarget::Path(p) => p.clone(),
            _ => return empty(),
        };
        let pp = std::path::Path::new(&path);
        let is_dir = pp.is_dir();
        let ext = pp
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        let is_shortcut = matches!(ext.as_str(), "lnk" | "url");

        // The tab only exists for a folder (non-archive) or a shortcut.
        if !is_dir && !is_shortcut {
            return empty();
        }

        // Current icon: `desktop.ini` (folder) or `GetIconLocation` (.lnk).
        let current = if is_dir {
            read_desktop_ini_icon(&path)
        } else if ext == "lnk" {
            read_lnk_icon(&path)
        } else {
            None
        };

        CustomizationModel {
            loaded: true,
            is_shortcut,
            icon_resource_path: current.unwrap_or_else(default_icon_dll_path),
        }
    }
}

/// `DefaultIconDllFilePath`: `%SystemRoot%\System32\SHELL32.dll`.
fn default_icon_dll_path() -> String {
    let root = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".to_string());
    format!("{root}\\System32\\SHELL32.dll")
}

/// Reads `IconResource=path,index` (or `IconFile=`) from the `[.ShellClassInfo]`
/// of a folder's `desktop.ini`. Returns the path alone (without the index).
fn read_desktop_ini_icon(folder: &str) -> Option<String> {
    let ini = std::path::Path::new(folder).join("desktop.ini");
    let content = std::fs::read_to_string(&ini).ok()?;
    let mut in_section = false;
    let mut icon_file: Option<String> = None;
    for line in content.lines() {
        let t = line.trim();
        if t.starts_with('[') {
            in_section = t.eq_ignore_ascii_case("[.ShellClassInfo]");
            continue;
        }
        if !in_section {
            continue;
        }
        let lower = t.to_ascii_lowercase();
        if let Some(rest) = lower.strip_prefix("iconresource=") {
            // `path,index`: keep only the path part.
            let raw = &t[t.len() - rest.len()..];
            let path = raw.split(',').next().unwrap_or(raw).trim();
            if !path.is_empty() {
                return Some(expand_env(path));
            }
        } else if let Some(rest) = lower.strip_prefix("iconfile=") {
            let raw = &t[t.len() - rest.len()..];
            icon_file = Some(expand_env(raw.trim()));
        }
    }
    icon_file
}

/// `IShellLinkW.GetIconLocation` of a `.lnk`.
fn read_lnk_icon(path: &str) -> Option<String> {
    use windows::core::{Interface, HSTRING};
    use windows::Win32::System::Com::{
        CoCreateInstance, IPersistFile, CLSCTX_INPROC_SERVER, STGM_READ,
    };
    use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};

    unsafe {
        let link =
            CoCreateInstance::<_, IShellLinkW>(&ShellLink, None, CLSCTX_INPROC_SERVER).ok()?;
        let persist = link.cast::<IPersistFile>().ok()?;
        persist.Load(&HSTRING::from(path), STGM_READ).ok()?;
        let mut buf = [0u16; 1024];
        let mut idx = 0i32;
        link.GetIconLocation(&mut buf, &mut idx).ok()?;
        let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        (len > 0).then(|| String::from_utf16_lossy(&buf[..len]))
    }
}

/// Expands the environment variables in a path (`%SystemRoot%`…).
fn expand_env(path: &str) -> String {
    if !path.contains('%') {
        return path.to_string();
    }
    let mut out = String::new();
    let mut rest = path;
    while let Some(start) = rest.find('%') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        if let Some(end) = after.find('%') {
            let var = &after[..end];
            match std::env::var(var) {
                Ok(v) => out.push_str(&v),
                Err(_) => {
                    out.push('%');
                    out.push_str(var);
                    out.push('%');
                }
            }
            rest = &after[end + 1..];
        } else {
            out.push('%');
            rest = after;
        }
    }
    out.push_str(rest);
    out
}
