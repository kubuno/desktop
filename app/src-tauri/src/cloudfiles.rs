//! Cloud Files API (CfApi) integration — native Windows sync-status overlays.
//!
//! Registering each sync folder as a CfApi *sync root* makes Explorer show the
//! standard cloud overlays (✓ in-sync, ⟳ syncing) and a "Status" column, the
//! same way OneDrive does — no admin rights, no COM overlay DLL. Our files are
//! always fully local (population = always-full), so we never field on-demand
//! hydration; we just register the root, keep a connection while running, and
//! mark files in-sync.

#![cfg(windows)]

use std::path::{Path, PathBuf};

use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Foundation::{CloseHandle, GENERIC_READ, GENERIC_WRITE};
use windows::Win32::Storage::CloudFilters::*;
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAGS_AND_ATTRIBUTES, FILE_FLAG_BACKUP_SEMANTICS, FILE_SHARE_READ,
    FILE_SHARE_WRITE, OPEN_EXISTING,
};

/// Register `folder` as a sync root via the **WinRT** StorageProviderSyncRootManager.
///
/// This is the API that wires up the shell integration (display name, icon and,
/// crucially, the status overlays) — the low-level Win32 `CfRegisterSyncRoot`
/// only sets up the file-system filter and does *not* render overlays.
/// Returns true if the folder was registered (fails on non-local volumes —
/// CfApi/StorageProvider require a local NTFS path). `name` is the label shown
/// in the Explorer navigation pane.
pub fn register(id: &str, name: &str, folder: &Path) -> bool {
    match register_winrt(id, name, folder) {
        Ok(()) => {
            eprintln!("[cloudfiles] sync root (WinRT) enregistré : {}", folder.display());
            true
        }
        Err(e) => {
            eprintln!("[cloudfiles] register WinRT {} échoué : {e:?}", folder.display());
            false
        }
    }
}

fn register_winrt(id: &str, name: &str, folder: &Path) -> windows::core::Result<()> {
    use windows::Security::Cryptography::{BinaryStringEncoding, CryptographicBuffer};
    use windows::Storage::Provider::*;
    use windows::Storage::StorageFolder;

    let info = StorageProviderSyncRootInfo::new()?;
    info.SetId(&HSTRING::from(id))?;

    // Resolve a WinRT StorageFolder from the path (blocking on the async op).
    let folder_h = HSTRING::from(folder.as_os_str());
    let sf = StorageFolder::GetFolderFromPathAsync(&folder_h)?.get()?;
    info.SetPath(&sf)?;

    info.SetDisplayNameResource(&HSTRING::from(name))?;
    let exe = std::env::current_exe()
        .ok()
        .and_then(|p| p.to_str().map(str::to_string))
        .unwrap_or_default();
    info.SetIconResource(&HSTRING::from(format!("{exe},0")))?;
    info.SetVersion(&HSTRING::from("1.0.0"))?;

    // Population AlwaysFull keeps every file VISIBLE (the namespace is always
    // fully present); Partial hydration lets a file's *content* be dehydrated
    // (online-only) and re-fetched on access. Changing population to Full instead
    // would make Windows expect provider-driven population and purge the files.
    info.SetHydrationPolicy(StorageProviderHydrationPolicy::Partial)?;
    info.SetPopulationPolicy(StorageProviderPopulationPolicy::AlwaysFull)?;
    info.SetInSyncPolicy(StorageProviderInSyncPolicy::Default)?;
    info.SetHardlinkPolicy(StorageProviderHardlinkPolicy::None)?;
    info.SetProtectionMode(StorageProviderProtectionMode::Unknown)?;
    info.SetShowSiblingsAsGroup(false)?;
    info.SetAllowPinning(true)?;

    // The context must be a non-empty buffer (Register fails otherwise).
    let ctx = CryptographicBuffer::ConvertStringToBinary(&HSTRING::from(id), BinaryStringEncoding::Utf8)?;
    info.SetContext(&ctx)?;

    StorageProviderSyncRootManager::Register(&info)
}

/// Live CfApi connections (sync-root path → connection key), kept so the
/// provider stays "online" while the app runs.
static CONNECTIONS: std::sync::Mutex<Vec<(String, i64)>> = std::sync::Mutex::new(Vec::new());

/// FETCH_DATA callback — invoked when an online-only file is opened. Reads the
/// `"instanceId|serverFileId"` stored in the placeholder's FileIdentity,
/// downloads the content from Kubuno and hands it back to the OS via CfExecute.
unsafe extern "system" fn on_fetch_data(
    info: *const CF_CALLBACK_INFO,
    _params: *const CF_CALLBACK_PARAMETERS,
) {
    use windows::Win32::Foundation::NTSTATUS;
    const STATUS_SUCCESS: NTSTATUS = NTSTATUS(0);
    const STATUS_UNSUCCESSFUL: NTSTATUS = NTSTATUS(0xC000_0001u32 as i32);

    let info = &*info;
    let bytes =
        std::slice::from_raw_parts(info.FileIdentity as *const u8, info.FileIdentityLength as usize);
    let ident = String::from_utf8_lossy(bytes);

    let mut data: Vec<u8> = Vec::new();
    let mut status = STATUS_SUCCESS;
    match ident.split_once('|') {
        Some((instance, file_id)) => match kubuno_sync::download_for(instance, file_id) {
            Ok(d) => data = d,
            Err(e) => {
                eprintln!("[cloudfiles] hydratation échouée ({ident}) : {e}");
                status = STATUS_UNSUCCESSFUL;
            }
        },
        None => status = STATUS_UNSUCCESSFUL,
    }

    let op_info = CF_OPERATION_INFO {
        StructSize: std::mem::size_of::<CF_OPERATION_INFO>() as u32,
        Type: CF_OPERATION_TYPE_TRANSFER_DATA,
        ConnectionKey: info.ConnectionKey,
        TransferKey: info.TransferKey,
        CorrelationVector: std::ptr::null(),
        SyncStatus: std::ptr::null(),
        RequestKey: info.RequestKey,
    };
    let mut op_params = CF_OPERATION_PARAMETERS {
        ParamSize: std::mem::size_of::<CF_OPERATION_PARAMETERS>() as u32,
        Anonymous: CF_OPERATION_PARAMETERS_0 {
            TransferData: CF_OPERATION_PARAMETERS_0_0 {
                Flags: CF_OPERATION_TRANSFER_DATA_FLAG_NONE,
                CompletionStatus: status,
                Buffer: if data.is_empty() {
                    std::ptr::null()
                } else {
                    data.as_ptr() as *const core::ffi::c_void
                },
                Offset: 0,
                Length: data.len() as i64,
            },
        },
    };
    let _ = CfExecute(&op_info, &mut op_params);
}

/// Turn one already-present file into an online-only ("virtual") placeholder:
/// record `"instanceId|serverFileId"` in its FileIdentity (for the fetch
/// callback), unpin it, then dehydrate it (free the local bytes → cloud icon).
fn dehydrate_one(instance: &str, path: &Path, server_id: &str) -> bool {
    let wpath = HSTRING::from(path.as_os_str());
    let ident = format!("{instance}|{server_id}");
    let id_bytes = ident.as_bytes();
    let id_ptr = id_bytes.as_ptr() as *const core::ffi::c_void;
    unsafe {
        let handle = CreateFileW(
            PCWSTR(wpath.as_ptr()),
            GENERIC_READ.0 | GENERIC_WRITE.0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            OPEN_EXISTING,
            FILE_FLAGS_AND_ATTRIBUTES(0),
            None,
        );
        let Ok(handle) = handle else { return false };
        let _ = CfUpdatePlaceholder(handle, None, Some(id_ptr), id_bytes.len() as u32, None, CF_UPDATE_FLAG_NONE, None, None);
        let _ = CfSetPinState(handle, CF_PIN_STATE_UNPINNED, CF_SET_PIN_FLAG_NONE, None);
        let ok = CfDehydratePlaceholder(handle, 0, -1, CF_DEHYDRATE_FLAG_NONE, None).is_ok();
        let _ = CloseHandle(handle);
        ok
    }
}

/// Make every given file online-only ("virtual"). `files` = (local path, server
/// file id). Already-dehydrated files are skipped (cheap, no hydration).
pub fn make_ondemand(instance: &str, files: &[(PathBuf, String)]) {
    let mut done = 0usize;
    for (path, server_id) in files {
        // Skip files that are already online-only (reading attributes only).
        if is_online_only(path) {
            continue;
        }
        if dehydrate_one(instance, path, server_id) {
            done += 1;
        }
    }
    eprintln!("[cloudfiles] {done} fichier(s) passés en on-demand pour {instance}");
}

/// True if `path` is already an online-only placeholder (attributes only — no
/// hydration triggered).
fn is_online_only(path: &Path) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS: u32 = 0x0040_0000;
    std::fs::metadata(path)
        .map(|m| m.file_attributes() & FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS != 0)
        .unwrap_or(false)
}

/// Connect to a registered sync root and report it "online/idle" so Explorer
/// renders the in-sync overlays. Idempotent per path (skips if already linked).
pub fn connect(folder: &Path) {
    let key_path: String = folder.to_string_lossy().into();
    if CONNECTIONS.lock().map(|c| c.iter().any(|(p, _)| *p == key_path)).unwrap_or(false) {
        return;
    }
    let path = HSTRING::from(folder.as_os_str());
    let table = [
        CF_CALLBACK_REGISTRATION { Type: CF_CALLBACK_TYPE_FETCH_DATA, Callback: Some(on_fetch_data) },
        CF_CALLBACK_REGISTRATION { Type: CF_CALLBACK_TYPE_NONE, Callback: None },
    ];
    unsafe {
        match CfConnectSyncRoot(PCWSTR(path.as_ptr()), table.as_ptr(), None, CF_CONNECT_FLAG_NONE) {
            Ok(key) => {
                let _ = CfUpdateSyncProviderStatus(key, CF_PROVIDER_STATUS_IDLE);
                eprintln!("[cloudfiles] connecté : {}", folder.display());
                if let Ok(mut c) = CONNECTIONS.lock() {
                    c.push((key_path, key.0));
                }
            }
            Err(e) => eprintln!("[cloudfiles] connexion échouée {} : {e:?}", folder.display()),
        }
    }
}

/// Drop the sync-root registration for an instance (e.g. on disconnect).
pub fn unregister(id: &str) {
    use windows::Storage::Provider::StorageProviderSyncRootManager;
    let _ = StorageProviderSyncRootManager::Unregister(&HSTRING::from(id));
}

/// Convert a single file/dir into a placeholder marked *in-sync* → green check.
fn mark_one(path: &Path, is_dir: bool) {
    let wpath = HSTRING::from(path.as_os_str());
    let flags = if is_dir { FILE_FLAG_BACKUP_SEMANTICS } else { FILE_FLAGS_AND_ATTRIBUTES(0) };
    unsafe {
        let handle = CreateFileW(
            PCWSTR(wpath.as_ptr()),
            GENERIC_READ.0 | GENERIC_WRITE.0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            OPEN_EXISTING,
            flags,
            None,
        );
        let Ok(handle) = handle else { return };
        // MARK_IN_SYNC converts to a placeholder *and* flags it in-sync in one go.
        let _ = CfConvertToPlaceholder(handle, None, 0, CF_CONVERT_FLAG_MARK_IN_SYNC, None, None);
        // Pin it ("always keep on this device") → green-check overlay.
        let _ = CfSetPinState(handle, CF_PIN_STATE_PINNED, CF_SET_PIN_FLAG_NONE, None);
        let _ = CloseHandle(handle);
    }
}

/// Walk `folder` and mark every entry (and the folder) as in-sync placeholders.
pub fn mark_tree_in_sync(folder: &Path) {
    mark_one(folder, true);
    let mut stack = vec![folder.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for entry in rd.flatten() {
            let p = entry.path();
            let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
            mark_one(&p, is_dir);
            if is_dir {
                stack.push(p);
            }
        }
    }
}
