//! Windows Explorer navigation-pane integration.
//!
//! Registers each instance's sync folder as a shell *namespace extension* so it
//! shows up as a root node in Explorer's left navigation pane — the same
//! per-user registry technique Nextcloud / ownCloud / OneDrive use. No admin
//! rights are needed (everything lives under `HKEY_CURRENT_USER`).
//!
//! Each instance gets a stable CLSID derived from its id; a `KubunoInstanceId`
//! marker value under the CLSID key lets us find and prune our own entries
//! (e.g. when an instance was removed while the app was not running).

use std::path::{Path, PathBuf};

use winreg::enums::*;
use winreg::RegKey;

/// Built-in "delegate folder" shell extension that renders a real directory.
const FOLDER_SHELLEXT: &str = "{0E5AAE11-A475-4C5B-AB00-C66DE400274E}";
const NAMESPACE: &str =
    r"Software\Microsoft\Windows\CurrentVersion\Explorer\Desktop\NameSpace";
const HIDE_NEW: &str =
    r"Software\Microsoft\Windows\CurrentVersion\Explorer\HideDesktopIcons\NewStartPanel";
const MARKER: &str = "KubunoInstanceId";

/// Stable per-instance CLSID (uuid v5 over the instance id) → `{GUID}` string.
fn clsid_for(id: &str) -> String {
    let u = uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_URL, format!("kubuno-nav:{id}").as_bytes());
    format!("{{{}}}", u.hyphenated())
}

/// Register (or update) the navigation-pane entry for one instance.
fn register(id: &str, name: &str, folder: &Path) -> std::io::Result<()> {
    let clsid = clsid_for(id);
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let base = format!(r"Software\Classes\CLSID\{clsid}");

    let (k, _) = hkcu.create_subkey(&base)?;
    k.set_value("", &name.to_string())?;
    k.set_value(MARKER, &id.to_string())?; // ownership marker for pruning
    k.set_value("System.IsPinnedToNameSpaceTree", &1u32)?;
    k.set_value("SortOrderIndex", &0x42u32)?;

    let exe = std::env::current_exe()
        .ok()
        .and_then(|p| p.to_str().map(str::to_string))
        .unwrap_or_default();
    let (icon, _) = hkcu.create_subkey(format!(r"{base}\DefaultIcon"))?;
    icon.set_value("", &format!("{exe},0"))?;

    let (inproc, _) = hkcu.create_subkey(format!(r"{base}\InProcServer32"))?;
    inproc.set_value("", &r"%SystemRoot%\system32\shell32.dll".to_string())?;
    inproc.set_value("ThreadingModel", &"Both".to_string())?;

    let (inst, _) = hkcu.create_subkey(format!(r"{base}\Instance"))?;
    inst.set_value("CLSID", &FOLDER_SHELLEXT.to_string())?;

    let (bag, _) = hkcu.create_subkey(format!(r"{base}\Instance\InitPropertyBag"))?;
    bag.set_value("Attributes", &0x11u32)?;
    // The shell needs a native Windows path (backslashes) to resolve the target.
    let target = folder.to_string_lossy().replace('/', "\\");
    bag.set_value("TargetFolderPath", &target)?;

    let (sf, _) = hkcu.create_subkey(format!(r"{base}\ShellFolder"))?;
    sf.set_value("FolderValueFlags", &0x28u32)?;
    sf.set_value("Attributes", &0xF080004Du32)?;

    // Show it in the navigation pane …
    let (ns, _) = hkcu.create_subkey(format!(r"{NAMESPACE}\{clsid}"))?;
    ns.set_value("", &name.to_string())?;

    // … but not on the Desktop.
    let (hide, _) = hkcu.create_subkey(HIDE_NEW)?;
    hide.set_value(&clsid, &1u32)?;
    Ok(())
}

/// Remove an instance's navigation-pane entry (CLSID + namespace + desktop-hide).
fn unregister(clsid: &str) {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let _ = hkcu.delete_subkey_all(format!(r"Software\Classes\CLSID\{clsid}"));
    let _ = hkcu.delete_subkey_all(format!(r"{NAMESPACE}\{clsid}"));
    if let Ok(hide) = hkcu.open_subkey_with_flags(HIDE_NEW, KEY_ALL_ACCESS) {
        let _ = hide.delete_value(clsid);
    }
}

/// Reconcile the registry with the current instance set: register/update an
/// entry for each, and prune any of *our* entries whose instance is gone.
pub fn sync(instances: &[(String, String, PathBuf)]) {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);

    // Prune orphans: scan the namespace, keep only entries we own whose marker
    // still matches a current instance id.
    let current: std::collections::HashSet<&str> =
        instances.iter().map(|(id, _, _)| id.as_str()).collect();
    if let Ok(nsk) = hkcu.open_subkey(NAMESPACE) {
        for guid in nsk.enum_keys().flatten() {
            let cls = format!(r"Software\Classes\CLSID\{guid}");
            let owned = hkcu
                .open_subkey(&cls)
                .ok()
                .and_then(|k| k.get_value::<String, _>(MARKER).ok());
            if let Some(marker) = owned {
                if !current.contains(marker.as_str()) {
                    unregister(&guid);
                }
            }
        }
    }

    for (id, name, folder) in instances {
        let _ = register(id, name, folder);
    }

    notify_shell();
}

/// Ask Explorer to refresh so changes show without a restart.
fn notify_shell() {
    // SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, NULL, NULL)
    #[link(name = "shell32")]
    extern "system" {
        fn SHChangeNotify(
            event: i32,
            flags: u32,
            item1: *const std::ffi::c_void,
            item2: *const std::ffi::c_void,
        );
    }
    const SHCNE_ASSOCCHANGED: i32 = 0x0800_0000;
    const SHCNF_IDLIST: u32 = 0x0000;
    unsafe { SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, std::ptr::null(), std::ptr::null()) };
}
