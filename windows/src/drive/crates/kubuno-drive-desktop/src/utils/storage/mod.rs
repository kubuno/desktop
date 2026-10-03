//! File operations: Explorer-compatible clipboard (CF_HDROP + Preferred
//! DropEffect) and copy/move/delete through `WindowsBulkOperations`
//! (IFileOperation), mirroring `ShellFilesystemOperations.cs`.
//!
//! The IFileOperation group (delete/paste/rename/create + `OpsMonitor`) was
//! extracted into `operations/shell_filesystem_operations.rs` (mirrors
//! `Utils/Storage/Operations/ShellFilesystemOperations.cs`). What remains
//! here, for lack of a mirror home INSIDE `utils/` (cross-folder
//! relocations deferred):
//! - the shortcuts `create_shortcut`/`resolve_shortcut`/`shortcut_name` →
//!   `IWindowsShortcutService` (Services);
//! - the archives `is_archive`/`compress`/`decompress`/… →
//!   `Services/Archive/StorageArchiveService.cs`;
//! - `flatten_folder` → `Actions/FileSystem/FlattenFolderAction`;
//! - the clipboard (`clipboard_*`) and `sendto_entries`: immediate Win32
//!   rendering infra, with no C# counterpart.

use windows::core::w;
use windows::Win32::Foundation::{HANDLE, HGLOBAL, HWND};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard,
    RegisterClipboardFormatW, SetClipboardData,
};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::System::Ole::CF_HDROP;
use windows::Win32::UI::Shell::{DragQueryFileW, DROPFILES, HDROP};

pub mod operations;

pub use operations::shell_filesystem_operations::*;

fn preferred_drop_effect_format() -> u32 {
    unsafe { RegisterClipboardFormatW(w!("Preferred DropEffect")) }
}

const DROPEFFECT_COPY: u32 = 1;
const DROPEFFECT_MOVE: u32 = 2;

/// Puts file paths on the clipboard as CF_HDROP (`cut` sets the move effect),
/// interoperable with File Explorer.
pub fn clipboard_set_files(hwnd: HWND, paths: &[String], cut: bool) -> bool {
    unsafe {
        if OpenClipboard(Some(hwnd)).is_err() {
            return false;
        }
        let result = (|| {
            EmptyClipboard().ok()?;

            // DROPFILES header followed by a double-null-terminated UTF-16 list.
            let mut list: Vec<u16> = Vec::new();
            for path in paths {
                list.extend(path.encode_utf16());
                list.push(0);
            }
            list.push(0);

            let header_size = std::mem::size_of::<DROPFILES>();
            let bytes = header_size + list.len() * 2;
            let hglobal = GlobalAlloc(GMEM_MOVEABLE, bytes).ok()?;
            let ptr = GlobalLock(hglobal) as *mut u8;
            if ptr.is_null() {
                return None;
            }
            let dropfiles = ptr as *mut DROPFILES;
            (*dropfiles) = DROPFILES {
                pFiles: header_size as u32,
                fWide: true.into(),
                ..Default::default()
            };
            std::ptr::copy_nonoverlapping(list.as_ptr() as *const u8, ptr.add(header_size), list.len() * 2);
            let _ = GlobalUnlock(hglobal);
            SetClipboardData(CF_HDROP.0 as u32, Some(HANDLE(hglobal.0))).ok()?;

            // Preferred DropEffect: copy vs move (cut).
            let effect = GlobalAlloc(GMEM_MOVEABLE, 4).ok()?;
            let ptr = GlobalLock(effect) as *mut u32;
            if ptr.is_null() {
                return None;
            }
            *ptr = if cut { DROPEFFECT_MOVE } else { DROPEFFECT_COPY };
            let _ = GlobalUnlock(effect);
            SetClipboardData(preferred_drop_effect_format(), Some(HANDLE(effect.0))).ok()?;
            Some(())
        })();
        let _ = CloseClipboard();
        result.is_some()
    }
}

/// Puts plain text on the clipboard (CF_UNICODETEXT) — AboutViewModel's
/// Copy version / Windows version / user ID commands.
pub fn clipboard_set_text(hwnd: HWND, text: &str) -> bool {
    use windows::Win32::System::Ole::CF_UNICODETEXT;
    unsafe {
        if OpenClipboard(Some(hwnd)).is_err() {
            return false;
        }
        let result = (|| {
            EmptyClipboard().ok()?;
            let wide: Vec<u16> = text.encode_utf16().chain([0]).collect();
            let hglobal = GlobalAlloc(GMEM_MOVEABLE, wide.len() * 2).ok()?;
            let ptr = GlobalLock(hglobal) as *mut u16;
            if ptr.is_null() {
                return None;
            }
            std::ptr::copy_nonoverlapping(wide.as_ptr(), ptr, wide.len());
            let _ = GlobalUnlock(hglobal);
            SetClipboardData(CF_UNICODETEXT.0 as u32, Some(HANDLE(hglobal.0))).ok()?;
            Some(())
        })();
        let _ = CloseClipboard();
        result.is_some()
    }
}

/// Whether the clipboard holds files — `PasteItemAction.IsExecutable`, which is
/// what greys out the command bar's Paste button.
pub fn clipboard_has_files() -> bool {
    unsafe { IsClipboardFormatAvailable(CF_HDROP.0 as u32).is_ok() }
}

/// Reads file paths (+ whether the source requested a move) from the clipboard.
pub fn clipboard_get_files(hwnd: HWND) -> Option<(Vec<String>, bool)> {
    unsafe {
        OpenClipboard(Some(hwnd)).ok()?;
        let result = (|| {
            let handle = GetClipboardData(CF_HDROP.0 as u32).ok()?;
            let hdrop = HDROP(handle.0 as *mut _);
            let count = DragQueryFileW(hdrop, u32::MAX, None);
            let mut paths = Vec::with_capacity(count as usize);
            for i in 0..count {
                let mut buf = [0u16; 1024];
                let len = DragQueryFileW(hdrop, i, Some(&mut buf));
                if len > 0 {
                    paths.push(String::from_utf16_lossy(&buf[..len as usize]));
                }
            }

            let mut is_move = false;
            if let Ok(effect_handle) = GetClipboardData(preferred_drop_effect_format()) {
                let ptr = GlobalLock(HGLOBAL(effect_handle.0)) as *const u32;
                if !ptr.is_null() {
                    is_move = *ptr & DROPEFFECT_MOVE != 0;
                    let _ = GlobalUnlock(HGLOBAL(effect_handle.0));
                }
            }
            Some((paths, is_move))
        })();
        let _ = CloseClipboard();
        result
    }
}

// ---------------------------------------------------------------------------
// Shortcuts (.lnk) — `CreateShortcutAction`, `PasteItemAsShortcutAction`.
//
// The original service goes through `IWindowsShortcutService`, which
// relies on IShellLink: that's exactly what we do here.
// ---------------------------------------------------------------------------

/// Creates `link_path` (.lnk) pointing to `target`.
pub fn create_shortcut(target: &str, link_path: &str) -> bool {
    use windows::core::{Interface, HSTRING};
    use windows::Win32::System::Com::{CoCreateInstance, IPersistFile, CLSCTX_INPROC_SERVER};
    use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};

    // `IPersistFile::Save` requires Windows separators — a path coming
    // from the command line may carry "/".
    let link_path = link_path.replace('/', "\\");

    unsafe {
        let Ok(link) = CoCreateInstance::<_, IShellLinkW>(&ShellLink, None, CLSCTX_INPROC_SERVER)
        else {
            return false;
        };
        if link.SetPath(&HSTRING::from(target)).is_err() {
            return false;
        }
        // Shortcut's working directory = target's folder, like Explorer.
        if let Some(parent) = std::path::Path::new(target).parent() {
            let _ = link.SetWorkingDirectory(&HSTRING::from(parent.to_string_lossy().as_ref()));
        }
        let Ok(persist) = link.cast::<IPersistFile>() else {
            return false;
        };
        persist.Save(&HSTRING::from(link_path.as_str()), true).is_ok()
    }
}

/// Resolves the target of a `.lnk` (`OpenFileLocationAction`).
pub fn resolve_shortcut(link_path: &str) -> Option<String> {
    use windows::core::{Interface, HSTRING};
    use windows::Win32::System::Com::{
        CoCreateInstance, IPersistFile, CLSCTX_INPROC_SERVER, STGM_READ,
    };
    use windows::Win32::UI::Shell::{IShellLinkW, ShellLink, SLGP_RAWPATH};

    unsafe {
        let link =
            CoCreateInstance::<_, IShellLinkW>(&ShellLink, None, CLSCTX_INPROC_SERVER).ok()?;
        let persist = link.cast::<IPersistFile>().ok()?;
        persist.Load(&HSTRING::from(link_path), STGM_READ).ok()?;
        let mut buf = [0u16; 1024];
        link.GetPath(&mut buf, std::ptr::null_mut(), SLGP_RAWPATH.0 as u32).ok()?;
        let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        (len > 0).then(|| String::from_utf16_lossy(&buf[..len]))
    }
}

/// Free name for a new shortcut in `dir`.
pub fn shortcut_name(dir: &str, target: &str) -> String {
    let stem = std::path::Path::new(target)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let base = format!("{stem} - {}", kubuno_drive_desktop_localization::tr("Shortcut"));
    let mut candidate = std::path::Path::new(dir).join(format!("{base}.lnk"));
    let mut n = 2;
    while candidate.exists() {
        candidate = std::path::Path::new(dir).join(format!("{base} ({n}).lnk"));
        n += 1;
    }
    candidate.to_string_lossy().into_owned()
}

// ---------------------------------------------------------------------------
// Archives — `StorageArchiveService`.
//
// The original bundles SevenZipSharp; we rely on the tools shipped with
// Windows: `tar.exe` (bsdtar, present since Windows 10 1803) can create
// and read a .zip, and 7z.exe takes over for .7z / .rar when available.
// ---------------------------------------------------------------------------

/// `FileExtensionHelpers.IsZipFile`.
const ARCHIVE_EXTENSIONS: [&str; 13] = [
    "zip", "msix", "appx", "msixbundle", "appxbundle", "7z", "rar", "tar", "mcpack", "mcworld",
    "mrpack", "jar", "gz",
];

pub fn is_archive(path: &str) -> bool {
    std::path::Path::new(path)
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .is_some_and(|e| ARCHIVE_EXTENSIONS.contains(&e.as_str()))
}

/// `StorageArchiveService.GenerateArchiveNameFromItems`: the item's name
/// without its extension when it's alone.
pub fn archive_name(path: &str) -> String {
    std::path::Path::new(path)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn seven_zip() -> Option<String> {
    [r"C:\Program Files\7-Zip\7z.exe", r"C:\Program Files (x86)\7-Zip\7z.exe"]
        .into_iter()
        .find(|p| std::path::Path::new(p).exists())
        .map(str::to_string)
}

/// Compresses `paths` into `archive` (.zip via tar, .7z via 7z.exe).
pub fn compress(paths: &[String], archive: &str) -> bool {
    let Some(dir) = std::path::Path::new(archive).parent().map(|p| p.to_path_buf()) else {
        return false;
    };
    let names: Vec<String> = paths
        .iter()
        .filter_map(|p| {
            std::path::Path::new(p).file_name().map(|n| n.to_string_lossy().into_owned())
        })
        .collect();

    if archive.to_ascii_lowercase().ends_with(".7z") {
        let Some(exe) = seven_zip() else {
            tracing::warn!("compress: 7z.exe introuvable");
            return false;
        };
        return std::process::Command::new(exe)
            .arg("a")
            .arg(archive)
            .args(&names)
            .current_dir(&dir)
            .status()
            .is_ok_and(|s| s.success());
    }
    // bsdtar: -a infers the format from the extension, so .zip produces a zip.
    std::process::Command::new("tar.exe")
        .args(["-a", "-c", "-f", archive])
        .args(&names)
        .current_dir(&dir)
        .status()
        .is_ok_and(|s| s.success())
}

/// Decompresses `archive` into `dest` (created if needed).
pub fn decompress(archive: &str, dest: &str) -> bool {
    if std::fs::create_dir_all(dest).is_err() {
        return false;
    }
    let lower = archive.to_ascii_lowercase();
    if lower.ends_with(".7z") || lower.ends_with(".rar") {
        let Some(exe) = seven_zip() else {
            tracing::warn!("decompress: 7z.exe introuvable");
            return false;
        };
        return std::process::Command::new(exe)
            .args(["x", archive, &format!("-o{dest}"), "-y"])
            .status()
            .is_ok_and(|s| s.success());
    }
    std::process::Command::new("tar.exe")
        .args(["-x", "-f", archive, "-C", dest])
        .status()
        .is_ok_and(|s| s.success())
}

/// `BaseDecompressArchiveAction` (ExtractHereSmart): true when the archive
/// has several top-level entries — it then deserves its own folder.
pub fn archive_has_multiple_roots(archive: &str) -> bool {
    let lower = archive.to_ascii_lowercase();
    let listing = if lower.ends_with(".7z") || lower.ends_with(".rar") {
        let Some(exe) = seven_zip() else { return false };
        std::process::Command::new(exe)
            .args(["l", "-ba", "-slt", archive])
            .output()
            .ok()
            .map(|o| {
                String::from_utf8_lossy(&o.stdout)
                    .lines()
                    .filter_map(|l| l.strip_prefix("Path = ").map(str::to_owned))
                    .collect::<Vec<_>>()
            })
    } else {
        std::process::Command::new("tar.exe")
            .args(["-t", "-f", archive])
            .output()
            .ok()
            .map(|o| {
                String::from_utf8_lossy(&o.stdout).lines().map(str::to_owned).collect()
            })
    };
    let Some(listing) = listing else { return false };
    let mut first_root: Option<String> = None;
    for entry in listing {
        let root = entry
            .trim_start_matches(['/', '\\'])
            .split(['/', '\\'])
            .next()
            .unwrap_or("")
            .to_owned();
        if root.is_empty() {
            continue;
        }
        match &first_root {
            None => first_root = Some(root),
            Some(f) if *f != root => return true,
            _ => {}
        }
    }
    false
}

// ---------------------------------------------------------------------------
// FlattenFolderAction: raises subfolder contents up to the root, then
// removes the emptied folders.
// ---------------------------------------------------------------------------

pub fn flatten_folder(dir: &str) -> bool {
    fn walk(
        dir: &std::path::Path,
        files: &mut Vec<std::path::PathBuf>,
        dirs: &mut Vec<std::path::PathBuf>,
    ) {
        for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                dirs.push(path.clone());
                walk(&path, files, dirs);
            } else {
                files.push(path);
            }
        }
    }
    let root = std::path::Path::new(dir);
    let (mut files, mut dirs) = (Vec::new(), Vec::new());
    for entry in std::fs::read_dir(root).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            dirs.push(path.clone());
            walk(&path, &mut files, &mut dirs);
        }
    }
    for file in &files {
        if let Some(name) = file.file_name() {
            let _ = std::fs::rename(file, root.join(name));
        }
    }
    // Deepest first, otherwise the parent isn't empty yet.
    dirs.sort_by_key(|p| std::cmp::Reverse(p.components().count()));
    for d in dirs {
        let _ = std::fs::remove_dir(d);
    }
    true
}

// ---------------------------------------------------------------------------
// "Send to": the original lets the shell fill this submenu. We enumerate
// the user's SendTo folder, as Explorer does.
// ---------------------------------------------------------------------------

pub fn sendto_entries() -> Vec<(String, String)> {
    let Ok(appdata) = std::env::var("APPDATA") else {
        return Vec::new();
    };
    let dir = std::path::Path::new(&appdata).join(r"Microsoft\Windows\SendTo");
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("lnk")) {
            if let Some(stem) = path.file_stem() {
                out.push((
                    stem.to_string_lossy().into_owned(),
                    path.to_string_lossy().into_owned(),
                ));
            }
        }
    }
    out.sort_by_key(|a| a.0.to_lowercase());
    out
}
