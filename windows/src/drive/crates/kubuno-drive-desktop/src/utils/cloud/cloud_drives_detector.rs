//! CloudDrivesDetector (mirrors CloudDrivesDetector.cs)
//!
//! Port of `Files.App/Utils/Cloud/CloudDrivesDetector.cs`: cloud drive
//! detection via the registry. Two detectors ported — OneDrive (the
//! `Accounts` key) and the generic one (pinned Explorer namespace
//! extensions: iCloud, Nextcloud, MEGA, ownCloud, Proton…); Sharepoint,
//! Yandex, pCloud, Nutstore, Seadrive and Autodesk remain to be ported.

use windows::core::PCWSTR;
use windows::Win32::System::Registry::{
    RegCloseKey, RegEnumKeyExW, RegGetValueW, RegOpenKeyExW, HKEY, HKEY_CLASSES_ROOT,
    HKEY_CURRENT_USER, KEY_READ, RRF_RT_REG_DWORD, RRF_RT_REG_SZ,
};

use super::cloud_provider::CloudProvider;

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain([0]).collect()
}

/// A key opened for reading, closed on drop.
struct Key(HKEY);

impl Key {
    fn open(root: HKEY, path: &str) -> Option<Self> {
        let mut key = HKEY::default();
        let path = wide(path);
        unsafe { RegOpenKeyExW(root, PCWSTR(path.as_ptr()), None, KEY_READ, &mut key) }
            .is_ok()
            .then_some(Self(key))
    }

    fn subkeys(&self) -> Vec<String> {
        let mut names = Vec::new();
        for i in 0.. {
            let mut buf = [0u16; 256];
            let mut len = buf.len() as u32;
            let ok = unsafe {
                RegEnumKeyExW(self.0, i, Some(windows::core::PWSTR(buf.as_mut_ptr())), &mut len, None, None, None, None)
            };
            if ok.is_err() {
                break;
            }
            names.push(String::from_utf16_lossy(&buf[..len as usize]));
        }
        names
    }

    /// The string value `name` (`""` for the default value).
    fn string(&self, name: &str) -> Option<String> {
        let name = wide(name);
        let mut len = 0u32;
        unsafe {
            RegGetValueW(self.0, None, PCWSTR(name.as_ptr()), RRF_RT_REG_SZ, None, None, Some(&mut len))
        }
        .ok().ok()?;
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
        .ok().ok()?;
        Some(String::from_utf16_lossy(&buf).trim_end_matches('\0').to_string())
    }

    fn dword(&self, name: &str) -> Option<u32> {
        let name = wide(name);
        let mut value = 0u32;
        let mut len = 4u32;
        unsafe {
            RegGetValueW(
                self.0,
                None,
                PCWSTR(name.as_ptr()),
                RRF_RT_REG_DWORD,
                None,
                Some(&mut value as *mut _ as *mut _),
                Some(&mut len),
            )
        }
        .ok().ok()?;
        Some(value)
    }
}

impl Drop for Key {
    fn drop(&mut self) {
        unsafe {
            let _ = RegCloseKey(self.0);
        }
    }
}

/// `DetectCloudDrives`: the union of the detectors, sorted by name, deduped.
pub fn detect_cloud_drives() -> Vec<CloudProvider> {
    let mut out = Vec::new();
    out.extend(detect_onedrive());
    out.extend(detect_generic());
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out.dedup();
    out
}

/// `DetectOneDrive`: `HKCU\SOFTWARE\Microsoft\OneDrive\Accounts\*`.
fn detect_onedrive() -> Vec<CloudProvider> {
    let mut out = Vec::new();
    let Some(accounts) = Key::open(HKEY_CURRENT_USER, r"SOFTWARE\Microsoft\OneDrive\Accounts")
    else {
        return out;
    };
    for account in accounts.subkeys() {
        let Some(key) =
            Key::open(HKEY_CURRENT_USER, &format!(r"SOFTWARE\Microsoft\OneDrive\Accounts\{account}"))
        else {
            continue;
        };
        let display = key.string("DisplayName").unwrap_or_default();
        let name = if display.trim().is_empty() {
            "OneDrive".to_string()
        } else {
            format!("OneDrive - {display}")
        };
        if let Some(folder) = key.string("UserFolder").filter(|f| !f.trim().is_empty()) {
            if !out.iter().any(|p: &CloudProvider| p.name == name) {
                out.push(CloudProvider { name, sync_folder: folder });
            }
        }
    }
    out
}

/// `DetectGenericCloudDrive`: pinned Explorer namespaces
/// (`Desktop\NameSpace` + `CLSID\{...}\Instance\InitPropertyBag`).
fn detect_generic() -> Vec<CloudProvider> {
    let mut out = Vec::new();
    let Some(namespace) = Key::open(
        HKEY_CURRENT_USER,
        r"SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\Desktop\NameSpace",
    ) else {
        return out;
    };
    for clsid in namespace.subkeys() {
        let Some(clsid_key) = Key::open(HKEY_CLASSES_ROOT, &format!(r"CLSID\{clsid}")) else {
            continue;
        };
        if clsid_key.dword("System.IsPinnedToNameSpaceTree") != Some(1) {
            continue;
        }
        let ns_path = format!(
            r"SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\Desktop\NameSpace\{clsid}"
        );
        let ns_key = Key::open(HKEY_CURRENT_USER, &ns_path);
        let Some(identifier) = ns_key.as_ref().and_then(|k| k.string("")) else { continue };
        let Some(bag) =
            Key::open(HKEY_CLASSES_ROOT, &format!(r"CLSID\{clsid}\Instance\InitPropertyBag"))
        else {
            continue;
        };
        let Some(folder) = bag.string("TargetFolderPath") else { continue };

        // `GetDriveType`: specific prefixes, then `ApplicationName`.
        let app_name = ns_key.as_ref().and_then(|k| k.string("ApplicationName"));
        let drive_type = if identifier.starts_with("iCloudDrive") {
            "iCloudDrive".to_string()
        } else if identifier.starts_with("iCloudPhotos") {
            "iCloudPhotos".to_string()
        } else if identifier.starts_with("ownCloud") {
            "ownCloud".to_string()
        } else if identifier.starts_with("ProtonDrive") {
            "ProtonDrive".to_string()
        } else if identifier.starts_with("SyncCom") {
            "SyncCom".to_string()
        } else if app_name.as_deref() == Some("Nextcloud") || app_name.as_deref() == Some("kDrive")
        {
            app_name.clone().unwrap()
        } else {
            identifier.clone()
        };

        let clsid_default = clsid_key.string("");
        let or_default = |fallback: &str| {
            clsid_default.clone().filter(|v| !v.is_empty()).unwrap_or_else(|| fallback.to_string())
        };
        // The `DetectGenericCloudDrive` table, verbatim.
        let name = match drive_type.as_str() {
            "MEGA" => {
                let leaf = std::path::Path::new(folder.trim_end_matches('\\'))
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                format!("MEGA ({leaf})")
            }
            "Nextcloud" => {
                if identifier.is_empty() { "Nextcloud".to_string() } else { identifier.clone() }
            }
            "Jottacloud" => "Jottacloud".to_string(),
            "iCloudDrive" => "iCloud Drive".to_string(),
            "iCloudPhotos" => "iCloud Photos".to_string(),
            "Creative Cloud Files" => "Creative Cloud Files".to_string(),
            "ownCloud" => or_default("ownCloud"),
            "ProtonDrive" => "Proton Drive".to_string(),
            "kDrive" => or_default("kDrive"),
            "Lucid" => or_default("lucidLink"),
            "SyncCom" => or_default("Sync"),
            "MagentaCLOUD" => or_default("MagentaCLOUD"),
            _ => continue,
        };
        out.push(CloudProvider { name, sync_folder: folder });
    }
    out
}
