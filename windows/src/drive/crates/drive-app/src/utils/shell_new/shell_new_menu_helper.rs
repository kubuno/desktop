//! ShellNewMenuHelper (mirrors ShellNewMenuHelper.cs)
//!
//! Port of `Files.App/Utils/Shell/ShellNewMenuHelper.cs`: the "New" menu
//! templates beyond Folder/File/Shortcut.
//!
//! The original enumerates `Registry.ClassesRoot` (HKEY_CLASSES_ROOT), and
//! for each `.ext` extension looks for a `ShellNew` subkey:
//!
//! ```csharp
//! foreach (var keyName in Registry.ClassesRoot.GetSubKeyNames()
//!     .Where(x => x.StartsWith('.') && !shortcutExtensions.Contains(x, ...)))
//! {
//!     using var key = Registry.ClassesRoot.OpenSubKeySafe(keyName);
//!     if (key is not null) { var ret = await GetShellNewRegistryEntries(key, key); ... }
//! }
//! ```
//!
//! `ShellNew` carries one of the four forms read by
//! `ParseShellNewRegistryEntry`: `NullFile` (empty file), `FileName`
//! (copy of a template), `Data` (raw bytes) or `Command` (command to
//! execute — not handled here, cf. `create_from_template`).
//!
//! NOTE: `Key` (RAII registry helper) has no C# analog (the original
//! uses `Registry.ClassesRoot`) — it stays private to this module.

use windows::core::{PCWSTR, PWSTR};
use windows::Win32::System::Registry::{
    RegCloseKey, RegEnumKeyExW, RegEnumValueW, RegGetValueW, RegOpenKeyExW, RegQueryValueExW, HKEY,
    HKEY_CLASSES_ROOT, KEY_READ, REG_EXPAND_SZ, REG_SZ, REG_VALUE_TYPE, RRF_RT_REG_SZ,
};

use crate::data::items::shell_new_entry::{ShellNewEntry, ShellNewKind};

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain([0]).collect()
}

/// A registry key opened for reading, closed on drop (like `cloud.rs`).
struct Key(HKEY);

impl Key {
    /// Opens a subkey under a root; an empty `path` duplicates the root
    /// itself into a closable handle.
    fn open(root: HKEY, path: &str) -> Option<Self> {
        let mut key = HKEY::default();
        let path = wide(path);
        unsafe { RegOpenKeyExW(root, PCWSTR(path.as_ptr()), None, KEY_READ, &mut key) }
            .is_ok()
            .then_some(Self(key))
    }

    /// Opens a subkey relative to this key (the original's `OpenSubKeySafe`).
    fn open_sub(&self, name: &str) -> Option<Self> {
        Self::open(self.0, name)
    }

    /// The subkey names (`GetSubKeyNames`).
    fn subkeys(&self) -> Vec<String> {
        let mut names = Vec::new();
        for i in 0.. {
            let mut buf = [0u16; 256];
            let mut len = buf.len() as u32;
            let ok = unsafe {
                RegEnumKeyExW(
                    self.0,
                    i,
                    Some(PWSTR(buf.as_mut_ptr())),
                    &mut len,
                    None,
                    None,
                    None,
                    None,
                )
            };
            if ok.is_err() {
                break;
            }
            names.push(String::from_utf16_lossy(&buf[..len as usize]));
        }
        names
    }

    /// The key's value names (`GetValueNames`).
    fn value_names(&self) -> Vec<String> {
        let mut names = Vec::new();
        for i in 0.. {
            let mut buf = [0u16; 260];
            let mut len = buf.len() as u32;
            let ok = unsafe {
                RegEnumValueW(
                    self.0,
                    i,
                    Some(PWSTR(buf.as_mut_ptr())),
                    &mut len,
                    None,
                    None,
                    None,
                    None,
                )
            };
            if ok.is_err() {
                break;
            }
            names.push(String::from_utf16_lossy(&buf[..len as usize]));
        }
        names
    }

    /// The string value `name` (`""` = default value). `RRF_RT_REG_SZ`
    /// automatically expands `REG_EXPAND_SZ`.
    fn string(&self, name: &str) -> Option<String> {
        let name = wide(name);
        let mut len = 0u32;
        unsafe {
            RegGetValueW(self.0, None, PCWSTR(name.as_ptr()), RRF_RT_REG_SZ, None, None, Some(&mut len))
        }
        .ok()
        .ok()?;
        if len == 0 {
            return None;
        }
        let mut buf = vec![0u16; (len as usize).div_ceil(2)];
        unsafe {
            RegGetValueW(
                self.0,
                None,
                PCWSTR(name.as_ptr()),
                RRF_RT_REG_SZ,
                None,
                Some(buf.as_mut_ptr() as *mut _),
                Some(&mut len),
            )
        }
        .ok()
        .ok()?;
        Some(String::from_utf16_lossy(&buf).trim_end_matches('\0').to_string())
    }

    /// The raw value `name` with its type (`GetValue` + `GetValueKind` for
    /// "Data"). Two calls: sizing then reading.
    fn value_raw(&self, name: &str) -> Option<(REG_VALUE_TYPE, Vec<u8>)> {
        let wname = wide(name);
        let mut ty = REG_VALUE_TYPE::default();
        let mut size = 0u32;
        unsafe {
            RegQueryValueExW(
                self.0,
                PCWSTR(wname.as_ptr()),
                None,
                Some(&mut ty),
                None,
                Some(&mut size),
            )
        }
        .ok()
        .ok()?;
        let mut buf = vec![0u8; size as usize];
        unsafe {
            RegQueryValueExW(
                self.0,
                PCWSTR(wname.as_ptr()),
                None,
                Some(&mut ty),
                Some(buf.as_mut_ptr()),
                Some(&mut size),
            )
        }
        .ok()
        .ok()?;
        buf.truncate(size as usize);
        Some((ty, buf))
    }
}

impl Drop for Key {
    fn drop(&mut self) {
        unsafe {
            let _ = RegCloseKey(self.0);
        }
    }
}

/// `GetShellNewRegistryEntries`: recursive descent into the extension's
/// subkey tree to find the first subkey named "ShellNew". The original
/// doesn't bound the depth; we limit it to never loop forever.
fn find_shell_new(key: &Key, depth: u32) -> Option<Key> {
    if depth > 8 {
        return None;
    }
    for name in key.subkeys() {
        let Some(sub) = key.open_sub(&name) else {
            continue;
        };
        if name.eq_ignore_ascii_case("ShellNew") {
            return Some(sub);
        }
        if let Some(found) = find_shell_new(&sub, depth + 1) {
            return Some(found);
        }
    }
    None
}

/// `Data`: `Binary` → raw bytes; `String`/`ExpandString` →
/// `Encoding.UTF8.GetBytes` of the string (registry bytes being UTF-16LE).
fn decode_data(ty: REG_VALUE_TYPE, bytes: Vec<u8>) -> Vec<u8> {
    if ty == REG_SZ || ty == REG_EXPAND_SZ {
        let u16s: Vec<u16> =
            bytes.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        let s = String::from_utf16_lossy(&u16s);
        s.trim_end_matches('\0').as_bytes().to_vec()
    } else {
        bytes
    }
}

/// The display name of an extension: `HKCR\.ext` (default) gives the
/// ProgID, then `HKCR\<ProgID>` (default) gives the friendly label.
/// Fallback: `None`.
fn display_name_for(ext: &str) -> Option<String> {
    let ext_key = Key::open(HKEY_CLASSES_ROOT, ext)?;
    let progid = ext_key.string("")?;
    if progid.is_empty() {
        return None;
    }
    let class_key = Key::open(HKEY_CLASSES_ROOT, &progid)?;
    class_key.string("").filter(|s| !s.is_empty())
}

/// The extension's ProgID's `DefaultIcon` reference, if present.
fn default_icon_for(ext: &str) -> Option<String> {
    let ext_key = Key::open(HKEY_CLASSES_ROOT, ext)?;
    let progid = ext_key.string("")?;
    if progid.is_empty() {
        return None;
    }
    let icon_key = Key::open(HKEY_CLASSES_ROOT, &format!(r"{progid}\DefaultIcon"))?;
    icon_key.string("").filter(|s| !s.is_empty())
}

/// `ParseShellNewRegistryEntry`: reads the `ShellNew` form for extension
/// `ext`. `None` if none of the recognized values are present.
fn parse_shell_new(key: &Key, ext: &str) -> Option<ShellNewEntry> {
    let names = key.value_names();
    let has = |n: &str| names.iter().any(|v| v.eq_ignore_ascii_case(n));

    // The original requires at least one of these values, otherwise
    // returns null.
    if !(has("NullFile")
        || has("Name")
        || has("FileName")
        || has("Command")
        || has("ItemName")
        || has("Data"))
    {
        return None;
    }

    let file_name = key.string("FileName").filter(|s| !s.is_empty());
    let command = key.string("Command").filter(|s| !s.is_empty());
    let data = key.value_raw("Data").map(|(ty, bytes)| decode_data(ty, bytes));

    // Explorer-style priority: a template (FileName) wins, then bytes
    // (Data), then a command (Command), otherwise empty file (NullFile).
    let kind = if let Some(f) = file_name {
        ShellNewKind::FileName(f)
    } else if let Some(d) = data {
        ShellNewKind::Data(d)
    } else if let Some(c) = command {
        ShellNewKind::Command(c)
    } else {
        ShellNewKind::NullFile
    };

    Some(ShellNewEntry {
        extension: ext.to_string(),
        display_name: display_name_for(ext).unwrap_or_else(|| format!("file {ext}")),
        icon: default_icon_for(ext),
        kind,
    })
}

/// `GetNewContextMenuEntries`: walks HKCR, collects `ShellNew` entries,
/// guarantees the presence of ".txt", and sorts by display name.
pub fn list_shell_new_entries() -> Vec<ShellNewEntry> {
    let mut entries: Vec<ShellNewEntry> = Vec::new();

    // The original's `shortcutExtensions`: excluded because handled
    // separately (Shortcut).
    let shortcut_exts = [".library-ms", ".url", ".lnk"];

    if let Some(hkcr) = Key::open(HKEY_CLASSES_ROOT, "") {
        for name in hkcr.subkeys() {
            if !name.starts_with('.') {
                continue;
            }
            if shortcut_exts.iter().any(|e| e.eq_ignore_ascii_case(&name)) {
                continue;
            }
            let Some(ext_key) = hkcr.open_sub(&name) else {
                continue;
            };
            if let Some(shell_new) = find_shell_new(&ext_key, 0) {
                if let Some(entry) = parse_shell_new(&shell_new, &name) {
                    entries.push(entry);
                }
            }
        }
    }

    // `if (!newMenuItems.Any(x => ".txt".Equals(...)))`: "Document texte"
    // always present, even without a ShellNew key for ".txt".
    if !entries.iter().any(|e| e.extension.eq_ignore_ascii_case(".txt")) {
        entries.push(ShellNewEntry {
            extension: ".txt".to_string(),
            display_name: display_name_for(".txt").unwrap_or_else(|| "file .txt".to_string()),
            icon: default_icon_for(".txt"),
            kind: ShellNewKind::NullFile,
        });
    }

    // `OrderBy(item => item.Name)`: alphabetical sort by display name.
    entries.sort_by_key(|a| a.display_name.to_lowercase());
    entries
}
