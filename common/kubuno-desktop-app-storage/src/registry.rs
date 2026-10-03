//! The Windows Registry (`Microsoft.Win32.Registry` / `RegistryKey` of .NET): hives, 32/64-bit views, typed
//! values, sub-keys — what the `<RegistryKey>` component and [`crate::backend::RegistryBackend`] stand on.
//!
//! ```no_run
//! # fn f() -> Result<(), kubuno_desktop_app_storage::StorageError> {
//! use kubuno_desktop_app_storage::registry::{Access, Hive, RegValue, RegistryRoot, RegistryView};
//! let root = RegistryRoot::current();          // redirected inside a sandbox (below)
//! let key = root.create(Hive::CurrentUser, r"Software\Kubuno\Apps\notes\Integration", RegistryView::Default)?;
//! key.set_value("LastRun", &RegValue::QWord(1))?;
//! if let Some(k) = root.open(Hive::LocalMachine, r"SOFTWARE\Microsoft\Windows NT\CurrentVersion", RegistryView::Registry64, Access::Read)? {
//!     let build = k.get_value("CurrentBuild")?;
//! }
//! # Ok(()) }
//! ```
//!
//! **Views** (WOW64): a 32-bit process reading `HKLM\SOFTWARE` sees `HKLM\SOFTWARE\WOW6432Node` unless it asks for
//! [`RegistryView::Registry64`]; `Registry32` does the opposite for a 64-bit process. `HKCU\Software` is shared by
//! both views since Windows 7, so user keys use [`RegistryView::Default`].
//!
//! **Roaming**: `HKCU` roams with a roaming profile, except below `HKCU\Software\Classes\Local Settings`
//! ([`LOCAL_SETTINGS`]), which is what the local layer of the settings uses.
//!
//! **Safety of tests and sandboxes**: every access goes through a [`RegistryRoot`]. [`RegistryRoot::current`] is
//! the real Registry, except in a sandboxed profile (`KUBUNO_SANDBOX_DIR`), where every hive is mapped below
//! `HKCU\Software\Kubuno\Sandbox\<tag>\<HKLM|HKCU|…>`; tests use [`RegistryRoot::under`] with a throw-away key
//! (`HKCU\Software\Kubuno\Tests\<id>`) deleted afterwards. Neither can write the real `Run` key, file
//! associations or machine keys.
//!
//! Errors name the key and the value, never the data.

use std::fmt;

use windows_sys::Win32::Foundation::{ERROR_ACCESS_DENIED, ERROR_FILE_NOT_FOUND, ERROR_MORE_DATA, ERROR_NO_MORE_ITEMS, ERROR_SUCCESS, FILETIME, WIN32_ERROR};
use windows_sys::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegDeleteKeyExW, RegDeleteTreeW, RegDeleteValueW, RegEnumKeyExW, RegEnumValueW, RegOpenKeyExW, RegQueryInfoKeyW, RegQueryValueExW, RegSetValueExW,
    HKEY, HKEY_CLASSES_ROOT, HKEY_CURRENT_CONFIG, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, HKEY_USERS, KEY_ALL_ACCESS, KEY_READ, KEY_WOW64_32KEY, KEY_WOW64_64KEY,
    REG_BINARY, REG_DWORD, REG_EXPAND_SZ, REG_MULTI_SZ, REG_OPTION_NON_VOLATILE, REG_QWORD, REG_SAM_FLAGS, REG_SZ, REG_VALUE_TYPE,
};

use crate::{paths, StorageError};

/// The key below `HKCU` whose content does not roam (`HKCU\Software\Classes\Local Settings`).
pub const LOCAL_SETTINGS: &str = r"Software\Classes\Local Settings";
/// The largest value read or written (the Registry is not a file store: Microsoft advises under 2 KB).
pub const MAX_VALUE_BYTES: usize = 1024 * 1024;
/// The longest value name (Win32 limit).
const MAX_VALUE_NAME: usize = 16_383;

/// A root key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Hive {
    /// `HKEY_CURRENT_USER` (`HKCU`).
    CurrentUser,
    /// `HKEY_LOCAL_MACHINE` (`HKLM`): readable by everyone, writable by administrators.
    LocalMachine,
    /// `HKEY_CLASSES_ROOT` (`HKCR`): the merged view of file associations.
    ClassesRoot,
    /// `HKEY_USERS` (`HKU`).
    Users,
    /// `HKEY_CURRENT_CONFIG` (`HKCC`).
    CurrentConfig,
}

impl Hive {
    pub const ALL: [Hive; 5] = [Hive::CurrentUser, Hive::LocalMachine, Hive::ClassesRoot, Hive::Users, Hive::CurrentConfig];

    fn hkey(self) -> HKEY {
        match self {
            Hive::CurrentUser => HKEY_CURRENT_USER,
            Hive::LocalMachine => HKEY_LOCAL_MACHINE,
            Hive::ClassesRoot => HKEY_CLASSES_ROOT,
            Hive::Users => HKEY_USERS,
            Hive::CurrentConfig => HKEY_CURRENT_CONFIG,
        }
    }

    /// The short name (`HKCU`).
    pub fn short_name(self) -> &'static str {
        match self {
            Hive::CurrentUser => "HKCU",
            Hive::LocalMachine => "HKLM",
            Hive::ClassesRoot => "HKCR",
            Hive::Users => "HKU",
            Hive::CurrentConfig => "HKCC",
        }
    }

    /// `HKCU`, `HKEY_CURRENT_USER`, `CurrentUser` (the component's `Hive=` values)…
    pub fn parse(name: &str) -> Option<Self> {
        let n = name.trim().to_ascii_uppercase().replace('_', "");
        Hive::ALL.into_iter().find(|h| n == h.short_name() || n == format!("{h:?}").to_ascii_uppercase() || n == format!("HKEY{}", format!("{h:?}").to_ascii_uppercase()))
    }
}

impl fmt::Display for Hive {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.short_name())
    }
}

/// The WOW64 view a key is opened in (see the module doc).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum RegistryView {
    /// The process's own view.
    #[default]
    Default,
    /// The 32-bit view (`WOW6432Node`), from a 64-bit process.
    Registry32,
    /// The 64-bit view, from a 32-bit process.
    Registry64,
}

impl RegistryView {
    fn flag(self) -> REG_SAM_FLAGS {
        match self {
            RegistryView::Default => 0,
            RegistryView::Registry32 => KEY_WOW64_32KEY,
            RegistryView::Registry64 => KEY_WOW64_64KEY,
        }
    }
}

/// How a key is opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    Read,
    ReadWrite,
}

/// A typed Registry value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegValue {
    /// `REG_SZ`.
    String(String),
    /// `REG_EXPAND_SZ` (unexpanded: `%LOCALAPPDATA%\x`).
    ExpandString(String),
    /// `REG_MULTI_SZ` (no empty item: the format cannot hold one).
    MultiString(Vec<String>),
    /// `REG_DWORD`.
    DWord(u32),
    /// `REG_QWORD`.
    QWord(u64),
    /// `REG_BINARY` (and any other type, as its bytes).
    Binary(Vec<u8>),
}

impl RegValue {
    /// The type's name (`REG_SZ`…).
    pub fn type_name(&self) -> &'static str {
        match self {
            RegValue::String(_) => "REG_SZ",
            RegValue::ExpandString(_) => "REG_EXPAND_SZ",
            RegValue::MultiString(_) => "REG_MULTI_SZ",
            RegValue::DWord(_) => "REG_DWORD",
            RegValue::QWord(_) => "REG_QWORD",
            RegValue::Binary(_) => "REG_BINARY",
        }
    }

    /// The value as text (numbers in decimal, lists one item per line, bytes in hex): what a binding shows.
    pub fn to_text(&self) -> String {
        match self {
            RegValue::String(s) | RegValue::ExpandString(s) => s.clone(),
            RegValue::MultiString(l) => l.join("\n"),
            RegValue::DWord(v) => v.to_string(),
            RegValue::QWord(v) => v.to_string(),
            RegValue::Binary(b) => hex::encode(b),
        }
    }

    fn encode(&self, name: &str) -> Result<(REG_VALUE_TYPE, Vec<u8>), StorageError> {
        fn utf16z(s: &str) -> Vec<u8> {
            s.encode_utf16().chain(std::iter::once(0)).flat_map(u16::to_le_bytes).collect()
        }
        Ok(match self {
            RegValue::String(s) => (REG_SZ, utf16z(s)),
            RegValue::ExpandString(s) => (REG_EXPAND_SZ, utf16z(s)),
            RegValue::MultiString(items) => {
                if items.iter().any(|i| i.is_empty() || i.contains('\0')) {
                    return Err(StorageError::Setting { name: name.to_string(), message: "a REG_MULTI_SZ item cannot be empty".into() });
                }
                let mut bytes: Vec<u8> = items.iter().flat_map(|i| utf16z(i)).collect();
                bytes.extend_from_slice(&[0, 0]);
                (REG_MULTI_SZ, bytes)
            }
            RegValue::DWord(v) => (REG_DWORD, v.to_le_bytes().to_vec()),
            RegValue::QWord(v) => (REG_QWORD, v.to_le_bytes().to_vec()),
            RegValue::Binary(b) => (REG_BINARY, b.clone()),
        })
    }

    fn decode(ty: REG_VALUE_TYPE, data: &[u8]) -> RegValue {
        let wide = || -> Vec<u16> { data.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect() };
        let text = |w: &[u16]| String::from_utf16_lossy(w.split(|&c| c == 0).next().unwrap_or(&[]));
        match ty {
            REG_SZ => RegValue::String(text(&wide())),
            REG_EXPAND_SZ => RegValue::ExpandString(text(&wide())),
            REG_MULTI_SZ => RegValue::MultiString(wide().split(|&c| c == 0).filter(|s| !s.is_empty()).map(String::from_utf16_lossy).collect()),
            REG_DWORD if data.len() >= 4 => RegValue::DWord(u32::from_le_bytes([data[0], data[1], data[2], data[3]])),
            REG_QWORD if data.len() >= 8 => RegValue::QWord(u64::from_le_bytes([data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7]])),
            _ => RegValue::Binary(data.to_vec()),
        }
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// A key path relative to its hive: no empty segment, no leading or trailing `\`, segments of at most 255
/// characters, no NUL.
fn check_path(path: &str) -> Result<(), StorageError> {
    let ok = path.len() < 32_767 && !path.contains('\0') && (path.is_empty() || path.split('\\').all(|s| !s.is_empty() && s.encode_utf16().count() <= 255));
    if ok {
        Ok(())
    } else {
        Err(StorageError::InvalidName(format!("Registry path '{path}'")))
    }
}

fn check_value_name(name: &str) -> Result<(), StorageError> {
    if name.contains('\0') || name.encode_utf16().count() > MAX_VALUE_NAME {
        return Err(StorageError::InvalidName(format!("Registry value name '{name}'")));
    }
    Ok(())
}

/// The error of a failed call on `what` (a key's display name).
fn fail(what: &str, code: WIN32_ERROR) -> StorageError {
    if code == ERROR_ACCESS_DENIED {
        return StorageError::AccessDenied(what.to_string());
    }
    StorageError::Backend { backend: "registry", message: format!("{what}: {}", std::io::Error::from_raw_os_error(code as i32)) }
}

/// Where Registry accesses go: the real Registry or a redirection (see the module doc).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistryRoot {
    /// `HKCU\<redirect>\<HIVE>\<path>` instead of `<HIVE>\<path>`.
    redirect: Option<String>,
}

impl RegistryRoot {
    /// The key a sandboxed profile's accesses are mapped below.
    pub const SANDBOX_BASE: &'static str = r"Software\Kubuno\Sandbox";

    /// The real Registry, or the sandbox's redirection when `KUBUNO_SANDBOX_DIR` is set.
    pub fn current() -> Self {
        Self { redirect: paths::sandbox_tag().map(|t| format!(r"{}\{t}", Self::SANDBOX_BASE)) }
    }

    /// Every hive mapped below `HKCU\<base>` (tests: `Software\Kubuno\Tests\<id>`; delete it with
    /// [`RegistryRoot::delete_redirect`] afterwards).
    pub fn under(base: &str) -> Result<Self, StorageError> {
        check_path(base)?;
        if base.is_empty() {
            return Err(StorageError::InvalidName("an empty redirection".into()));
        }
        Ok(Self { redirect: Some(base.to_string()) })
    }

    /// Whether accesses are redirected (a sandbox or a test).
    pub fn is_redirected(&self) -> bool {
        self.redirect.is_some()
    }

    /// The real location of `<hive>\<path>`.
    fn map(&self, hive: Hive, path: &str) -> (Hive, String) {
        match &self.redirect {
            None => (hive, path.to_string()),
            Some(base) if path.is_empty() => (Hive::CurrentUser, format!(r"{base}\{}", hive.short_name())),
            Some(base) => (Hive::CurrentUser, format!(r"{base}\{}\{path}", hive.short_name())),
        }
    }

    /// `HKCU\Software\…` as shown to the user (the real location).
    pub fn display(&self, hive: Hive, path: &str) -> String {
        let (h, p) = self.map(hive, path);
        if p.is_empty() {
            h.short_name().to_string()
        } else {
            format!(r"{}\{p}", h.short_name())
        }
    }

    /// Opens an existing key; `Ok(None)` when it does not exist.
    pub fn open(&self, hive: Hive, path: &str, view: RegistryView, access: Access) -> Result<Option<RegistryKey>, StorageError> {
        check_path(path)?;
        let (h, p) = self.map(hive, path);
        let display = self.display(hive, path);
        let sam = match access {
            Access::Read => KEY_READ,
            Access::ReadWrite => KEY_ALL_ACCESS,
        } | view.flag();
        let mut out: HKEY = std::ptr::null_mut();
        let p16 = wide(&p);
        // SAFETY: valid predefined root, NUL-terminated path, `out` receives a handle owned by the returned key.
        let code = unsafe { RegOpenKeyExW(h.hkey(), p16.as_ptr(), 0, sam, &mut out) };
        match code {
            ERROR_SUCCESS => Ok(Some(RegistryKey { hkey: out, display, view, writable: access == Access::ReadWrite })),
            ERROR_FILE_NOT_FOUND => Ok(None),
            c => Err(fail(&display, c)),
        }
    }

    /// Opens a key for writing, creating it (and its parents) when missing.
    pub fn create(&self, hive: Hive, path: &str, view: RegistryView) -> Result<RegistryKey, StorageError> {
        check_path(path)?;
        let (h, p) = self.map(hive, path);
        let display = self.display(hive, path);
        let mut out: HKEY = std::ptr::null_mut();
        let p16 = wide(&p);
        // SAFETY: as in `open`; no class, default security, the disposition is not needed.
        let code = unsafe {
            RegCreateKeyExW(h.hkey(), p16.as_ptr(), 0, std::ptr::null(), REG_OPTION_NON_VOLATILE, KEY_ALL_ACCESS | view.flag(), std::ptr::null(), &mut out, std::ptr::null_mut())
        };
        if code != ERROR_SUCCESS {
            return Err(fail(&display, code));
        }
        Ok(RegistryKey { hkey: out, display, view, writable: true })
    }

    /// Deletes a key and everything below it; `Ok(false)` when it did not exist.
    pub fn delete_tree(&self, hive: Hive, path: &str, view: RegistryView) -> Result<bool, StorageError> {
        check_path(path)?;
        let Some((parent, leaf)) = path.rsplit_once('\\') else {
            return match self.open(hive, "", view, Access::ReadWrite)? {
                Some(root) => root.delete_subkey_tree(path),
                None => Ok(false),
            };
        };
        match self.open(hive, parent, view, Access::ReadWrite)? {
            Some(k) => k.delete_subkey_tree(leaf),
            None => Ok(false),
        }
    }

    /// Deletes the redirection's own key (the end of a test), and its parents below `Software\Kubuno` that it left
    /// empty (`Tests`, `Sandbox`); nothing for the real Registry.
    pub fn delete_redirect(&self) -> Result<bool, StorageError> {
        let Some(base) = &self.redirect else { return Ok(false) };
        let real = RegistryRoot { redirect: None };
        let deleted = real.delete_tree(Hive::CurrentUser, base, RegistryView::Default)?;
        let mut path = base.as_str();
        while let Some((parent, _)) = path.rsplit_once('\\') {
            if !parent.to_ascii_lowercase().starts_with(r"software\kubuno\") {
                break;
            }
            let Some((grand, leaf)) = parent.rsplit_once('\\') else { break };
            let Some(g) = real.open(Hive::CurrentUser, grand, RegistryView::Default, Access::ReadWrite)? else { break };
            let has_values = real.open(Hive::CurrentUser, parent, RegistryView::Default, Access::Read)?.map(|k| k.value_names()).transpose()?.is_some_and(|v| !v.is_empty());
            // `RegDeleteKeyExW` refuses a key that has sub-keys: a concurrent test's key is never removed.
            if has_values || !g.delete_empty_subkey(leaf)? {
                break;
            }
            path = parent;
        }
        Ok(deleted)
    }
}

/// An open key (`Microsoft.Win32.RegistryKey`). Closed when dropped.
pub struct RegistryKey {
    hkey: HKEY,
    display: String,
    view: RegistryView,
    writable: bool,
}

// SAFETY: a Registry handle may be used from any thread (the API is thread-safe); the key is not `Sync` because
// nothing here needs it.
unsafe impl Send for RegistryKey {}

impl fmt::Debug for RegistryKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RegistryKey").field("name", &self.display).field("view", &self.view).field("writable", &self.writable).finish()
    }
}

impl Drop for RegistryKey {
    fn drop(&mut self) {
        // SAFETY: the handle was opened by this key and is closed once.
        unsafe { RegCloseKey(self.hkey) };
    }
}

impl RegistryKey {
    /// The full name of the key (`HKCU\Software\…`, the real location when redirected).
    pub fn name(&self) -> &str {
        &self.display
    }

    pub fn is_writable(&self) -> bool {
        self.writable
    }

    fn child(&self, name: &str) -> String {
        format!(r"{}\{name}", self.display)
    }

    fn require_writable(&self) -> Result<(), StorageError> {
        if self.writable {
            Ok(())
        } else {
            Err(StorageError::ReadOnly(self.display.clone()))
        }
    }

    /// Opens a sub-key; `Ok(None)` when it does not exist.
    pub fn open_subkey(&self, name: &str, access: Access) -> Result<Option<RegistryKey>, StorageError> {
        check_path(name)?;
        let sam = match access {
            Access::Read => KEY_READ,
            Access::ReadWrite => KEY_ALL_ACCESS,
        } | self.view.flag();
        let mut out: HKEY = std::ptr::null_mut();
        let n = wide(name);
        // SAFETY: an open key, a NUL-terminated name, `out` owned by the returned key.
        let code = unsafe { RegOpenKeyExW(self.hkey, n.as_ptr(), 0, sam, &mut out) };
        match code {
            ERROR_SUCCESS => Ok(Some(RegistryKey { hkey: out, display: self.child(name), view: self.view, writable: access == Access::ReadWrite })),
            ERROR_FILE_NOT_FOUND => Ok(None),
            c => Err(fail(&self.child(name), c)),
        }
    }

    /// Opens a sub-key for writing, creating it when missing.
    pub fn create_subkey(&self, name: &str) -> Result<RegistryKey, StorageError> {
        check_path(name)?;
        self.require_writable()?;
        let mut out: HKEY = std::ptr::null_mut();
        let n = wide(name);
        // SAFETY: as in `RegistryRoot::create`.
        let code = unsafe {
            RegCreateKeyExW(self.hkey, n.as_ptr(), 0, std::ptr::null(), REG_OPTION_NON_VOLATILE, KEY_ALL_ACCESS | self.view.flag(), std::ptr::null(), &mut out, std::ptr::null_mut())
        };
        if code != ERROR_SUCCESS {
            return Err(fail(&self.child(name), code));
        }
        Ok(RegistryKey { hkey: out, display: self.child(name), view: self.view, writable: true })
    }

    /// Deletes a sub-key and everything below it; `Ok(false)` when it did not exist.
    pub fn delete_subkey_tree(&self, name: &str) -> Result<bool, StorageError> {
        check_path(name)?;
        if name.is_empty() {
            return Err(StorageError::InvalidName("an empty sub-key name (the key itself)".into()));
        }
        self.require_writable()?;
        let n = wide(name);
        // SAFETY: an open key opened with KEY_ALL_ACCESS, a NUL-terminated sub-key name.
        match unsafe { RegDeleteTreeW(self.hkey, n.as_ptr()) } {
            ERROR_SUCCESS => Ok(true),
            ERROR_FILE_NOT_FOUND => Ok(false),
            c => Err(fail(&self.child(name), c)),
        }
    }

    /// Deletes the sub-key `name` only if it has no sub-key of its own; `Ok(false)` when it has some or does not
    /// exist.
    pub fn delete_empty_subkey(&self, name: &str) -> Result<bool, StorageError> {
        check_path(name)?;
        self.require_writable()?;
        let n = wide(name);
        // SAFETY: an open writable key, a NUL-terminated name; the view flag is the key's own.
        match unsafe { RegDeleteKeyExW(self.hkey, n.as_ptr(), self.view.flag(), 0) } {
            ERROR_SUCCESS => Ok(true),
            ERROR_FILE_NOT_FOUND | ERROR_ACCESS_DENIED => Ok(false),
            c => Err(fail(&self.child(name), c)),
        }
    }

    /// `(sub-keys, values, longest sub-key name, longest value name, longest value data, last write time)`.
    fn info(&self) -> Result<(u32, u32, u32, u32, u32, u64), StorageError> {
        let (mut subkeys, mut max_subkey, mut values, mut max_name, mut max_data) = (0u32, 0u32, 0u32, 0u32, 0u32);
        let mut time = FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 };
        // SAFETY: an open key; every out pointer is valid or null as the API allows.
        let code = unsafe {
            RegQueryInfoKeyW(
                self.hkey,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null(),
                &mut subkeys,
                &mut max_subkey,
                std::ptr::null_mut(),
                &mut values,
                &mut max_name,
                &mut max_data,
                std::ptr::null_mut(),
                &mut time,
            )
        };
        if code != ERROR_SUCCESS {
            return Err(fail(&self.display, code));
        }
        Ok((subkeys, values, max_subkey, max_name, max_data, (u64::from(time.dwHighDateTime) << 32) | u64::from(time.dwLowDateTime)))
    }

    /// When the key (or one of its values) last changed, as a `FILETIME` (100 ns since 1601): what the settings
    /// engine compares to notice a change made by another process.
    pub fn last_write_time(&self) -> Result<u64, StorageError> {
        Ok(self.info()?.5)
    }

    /// The names of the sub-keys.
    pub fn subkey_names(&self) -> Result<Vec<String>, StorageError> {
        let (count, _, max, _, _, _) = self.info()?;
        let mut out = Vec::with_capacity(count as usize);
        let mut buf = vec![0u16; max as usize + 1];
        for i in 0.. {
            let mut len = buf.len() as u32;
            // SAFETY: `buf` holds `len` UTF-16 units; the class and time outputs are not requested.
            let code = unsafe {
                RegEnumKeyExW(self.hkey, i, buf.as_mut_ptr(), &mut len, std::ptr::null(), std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut())
            };
            match code {
                ERROR_SUCCESS => out.push(String::from_utf16_lossy(&buf[..len as usize])),
                ERROR_NO_MORE_ITEMS => break,
                ERROR_MORE_DATA => buf.resize(buf.len() * 2, 0),
                c => return Err(fail(&self.display, c)),
            }
        }
        Ok(out)
    }

    /// The names of the values (`""` is the key's default value).
    pub fn value_names(&self) -> Result<Vec<String>, StorageError> {
        let (_, count, _, max, _, _) = self.info()?;
        let mut out = Vec::with_capacity(count as usize);
        let mut buf = vec![0u16; max as usize + 1];
        for i in 0.. {
            let mut len = buf.len() as u32;
            // SAFETY: `buf` holds `len` UTF-16 units; the type and data are not requested.
            let code = unsafe {
                RegEnumValueW(self.hkey, i, buf.as_mut_ptr(), &mut len, std::ptr::null(), std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut())
            };
            match code {
                ERROR_SUCCESS => out.push(String::from_utf16_lossy(&buf[..len as usize])),
                ERROR_NO_MORE_ITEMS => break,
                ERROR_MORE_DATA => buf.resize(buf.len() * 2, 0),
                c => return Err(fail(&self.display, c)),
            }
        }
        Ok(out)
    }

    /// The value `name` (`""`: the default value); `Ok(None)` when it does not exist.
    pub fn get_value(&self, name: &str) -> Result<Option<RegValue>, StorageError> {
        check_value_name(name)?;
        let n = wide(name);
        let what = || format!(r"{}\{name}", self.display);
        let mut ty: REG_VALUE_TYPE = 0;
        let mut len: u32 = 0;
        // SAFETY: size query (null data pointer), as documented.
        let code = unsafe { RegQueryValueExW(self.hkey, n.as_ptr(), std::ptr::null(), &mut ty, std::ptr::null_mut(), &mut len) };
        match code {
            ERROR_SUCCESS => {}
            ERROR_FILE_NOT_FOUND => return Ok(None),
            c => return Err(fail(&what(), c)),
        }
        loop {
            if len as usize > MAX_VALUE_BYTES {
                return Err(StorageError::TooLarge { name: what(), len: len as usize, max: MAX_VALUE_BYTES });
            }
            let mut data = vec![0u8; len as usize];
            let mut got = len;
            // SAFETY: `data` holds `got` bytes.
            let code = unsafe { RegQueryValueExW(self.hkey, n.as_ptr(), std::ptr::null(), &mut ty, data.as_mut_ptr(), &mut got) };
            match code {
                ERROR_SUCCESS => {
                    data.truncate(got as usize);
                    return Ok(Some(RegValue::decode(ty, &data)));
                }
                // The value grew between the two calls.
                ERROR_MORE_DATA => len = got.max(len.saturating_mul(2)),
                ERROR_FILE_NOT_FOUND => return Ok(None),
                c => return Err(fail(&what(), c)),
            }
        }
    }

    /// Creates or replaces the value `name`.
    pub fn set_value(&self, name: &str, value: &RegValue) -> Result<(), StorageError> {
        check_value_name(name)?;
        self.require_writable()?;
        let what = format!(r"{}\{name}", self.display);
        let (ty, data) = value.encode(&what)?;
        if data.len() > MAX_VALUE_BYTES {
            return Err(StorageError::TooLarge { name: what, len: data.len(), max: MAX_VALUE_BYTES });
        }
        let n = wide(name);
        // SAFETY: an open writable key, `data` holds `data.len()` bytes of the declared type.
        let code = unsafe { RegSetValueExW(self.hkey, n.as_ptr(), 0, ty, data.as_ptr(), data.len() as u32) };
        if code != ERROR_SUCCESS {
            return Err(fail(&what, code));
        }
        Ok(())
    }

    /// Removes the value `name`; `Ok(false)` when it did not exist.
    pub fn delete_value(&self, name: &str) -> Result<bool, StorageError> {
        check_value_name(name)?;
        self.require_writable()?;
        let n = wide(name);
        // SAFETY: an open writable key and a NUL-terminated name.
        match unsafe { RegDeleteValueW(self.hkey, n.as_ptr()) } {
            ERROR_SUCCESS => Ok(true),
            ERROR_FILE_NOT_FOUND => Ok(false),
            c => Err(fail(&format!(r"{}\{name}", self.display), c)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hives_parse_every_spelling() {
        for (s, h) in [("HKCU", Hive::CurrentUser), ("HKEY_LOCAL_MACHINE", Hive::LocalMachine), ("CurrentUser", Hive::CurrentUser), ("hkcr", Hive::ClassesRoot)] {
            assert_eq!(Hive::parse(s), Some(h), "{s}");
        }
        assert_eq!(Hive::parse("HKXX"), None);
    }

    #[test]
    fn values_encode_and_decode() {
        for v in [
            RegValue::String("é\u{1F600}".into()),
            RegValue::ExpandString("%TEMP%\\x".into()),
            RegValue::MultiString(vec!["a".into(), "b c".into()]),
            RegValue::DWord(7),
            RegValue::QWord(u64::MAX),
            RegValue::Binary(vec![0, 1, 255]),
        ] {
            let (ty, data) = v.encode("t").expect("encode");
            assert_eq!(RegValue::decode(ty, &data), v);
        }
        assert!(RegValue::MultiString(vec!["".into()]).encode("t").is_err());
    }

    #[test]
    fn paths_are_checked_and_redirected() {
        assert!(check_path(r"Software\Kubuno").is_ok());
        for bad in [r"\Software", r"Software\", r"a\\b", "a\0b"] {
            assert!(check_path(bad).is_err(), "{bad:?}");
        }
        let root = RegistryRoot::under(r"Software\Kubuno\Tests\x").expect("root");
        assert_eq!(root.display(Hive::LocalMachine, r"SOFTWARE\Microsoft"), r"HKCU\Software\Kubuno\Tests\x\HKLM\SOFTWARE\Microsoft");
        assert!(RegistryRoot::under("").is_err());
    }
}
