//! Port of the drive-side "storage security" actions: disk image mounting
//! (.iso/.img) and BitLocker unlocking.
//!
//! ## What the original C# does
//!
//! - **ISO / disk image.** `Files.App/Helpers/Win32/Win32Helper.Storage.cs`,
//!   `MountVhdDisk` (lines 421-425):
//!   ```csharp
//!   public static Task<bool> MountVhdDisk(string vhdPath)
//!   {
//!       // Mounting requires elevation
//!       return RunPowershellCommandAsync(
//!           $"-command \"Mount-DiskImage -ImagePath '{vhdPath}'\"",
//!           PowerShellExecutionOptions.Elevated | PowerShellExecutionOptions.Hidden);
//!   }
//!   ```
//!   So the C# delegates to PowerShell (`Mount-DiskImage`, elevation required).
//!   Here we reproduce the SAME result but WITHOUT PowerShell or a UAC pop-up
//!   by going directly through the VirtualDisk API (`virtdisk.dll`) that
//!   `Mount-DiskImage` calls internally: `OpenVirtualDisk` +
//!   `AttachVirtualDisk` (mount), `DetachVirtualDisk` (unmount). This is the
//!   approach the task calls for and the only one available in a pure Win32
//!   port. `TextPreviewViewModel.cs:72` confirms that ".iso" is indeed the
//!   extension convention on the Files side (`extension is ".iso"`).
//!
//! - **BitLocker.** Files does NOT have (un)locking code for BitLocker: it
//!   relies on the shell context menu verbs. See
//!   `Views/Layouts/BaseLayoutPage.cs:833-834`:
//!   ```csharp
//!   var turnOnBitLockerMenuItem = shellMenuItems.FirstOrDefault(x =>
//!       x.Tag is Win32ContextMenuItem menuItem && menuItem.CommandString is not null
//!       && menuItem.CommandString.StartsWith("encrypt-bde"));
//!   var manageBitLockerMenuItem = shellMenuItems.FirstOrDefault(x =>
//!       x.Tag is Win32ContextMenuItem { CommandString: "manage-bde" });
//!   ```
//!   and the "TurnOnBitLocker"/"ManageBitLocker" entries of the drive widgets
//!   (`DrivesWidgetViewModel.cs:172-179`). So the C# never unlocks with a
//!   password itself. Here, as the task allows, we implement
//!   `is_bitlocker_locked` (robust detection) and `unlock_bitlocker`
//!   (via the system tool `manage-bde`, best-effort — see the TODO).
//!
//! ## Robustness
//!
//! Any error (API, insufficient rights, invalid path) results in a silent
//! `false`. None of these functions can panic.

// NOT WIRED YET. Ported ISO mount/unmount and BitLocker unlock; waits for their entries in the drive context menu.
// The allow is scoped to this file and must go once it is wired.
#![allow(dead_code)]

use std::iter::once;

use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, ERROR_SUCCESS, HANDLE};
use windows::Win32::Storage::FileSystem::GetVolumeInformationW;
use windows::Win32::Storage::Vhd::{
    AttachVirtualDisk, DetachVirtualDisk, OpenVirtualDisk, ATTACH_VIRTUAL_DISK_FLAG_PERMANENT_LIFETIME,
    ATTACH_VIRTUAL_DISK_FLAG_READ_ONLY, DETACH_VIRTUAL_DISK_FLAG_NONE, OPEN_VIRTUAL_DISK_FLAG_NONE,
    VIRTUAL_DISK_ACCESS_ATTACH_RO, VIRTUAL_DISK_ACCESS_DETACH, VIRTUAL_DISK_ACCESS_GET_INFO,
    VIRTUAL_STORAGE_TYPE, VIRTUAL_STORAGE_TYPE_DEVICE_ISO, VIRTUAL_STORAGE_TYPE_VENDOR_MICROSOFT,
};

/// Encodes a Rust string as UTF-16 terminated by a `\0` (for `PCWSTR`).
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(once(0)).collect()
}

// ---------------------------------------------------------------------------
// ISO / disk images
// ---------------------------------------------------------------------------

/// True if the path designates a disk image mountable by Files (".iso" is
/// the convention on the C# side, cf. `TextPreviewViewModel.cs:72`; we also
/// accept ".img", equivalent for `Mount-DiskImage`/VirtualDisk).
pub fn is_iso(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower.ends_with(".iso") || lower.ends_with(".img")
}

/// Microsoft's "ISO" virtual storage type, shared by open and detach.
fn iso_storage_type() -> VIRTUAL_STORAGE_TYPE {
    VIRTUAL_STORAGE_TYPE {
        DeviceId: VIRTUAL_STORAGE_TYPE_DEVICE_ISO,
        VendorId: VIRTUAL_STORAGE_TYPE_VENDOR_MICROSOFT,
    }
}

/// Mounts a `.iso`/`.img` image (equivalent to `Mount-DiskImage`).
///
/// `OpenVirtualDisk` (ISO type, read-only attach + info access) then
/// `AttachVirtualDisk` with `READ_ONLY | PERMANENT_LIFETIME`: read-only
/// (an ISO is never writable) and permanent lifetime (the drive stays
/// mounted after the handle is closed, as Windows does). Returns `false`
/// on the slightest error (rights, corrupted image, missing path).
pub fn mount_iso(path: &str) -> bool {
    if !is_iso(path) {
        return false;
    }

    let storage_type = iso_storage_type();
    let path_w = wide(path);
    let mut handle = HANDLE::default();

    unsafe {
        // Opens the virtual disk.
        let opened = OpenVirtualDisk(
            &storage_type,
            PCWSTR(path_w.as_ptr()),
            VIRTUAL_DISK_ACCESS_ATTACH_RO | VIRTUAL_DISK_ACCESS_GET_INFO,
            OPEN_VIRTUAL_DISK_FLAG_NONE,
            None,
            &mut handle,
        );
        if opened != ERROR_SUCCESS || handle.is_invalid() {
            return false;
        }

        // Attach (mount). READ_ONLY + PERMANENT_LIFETIME: identical to the
        // way Windows mounts an ISO via Explorer.
        let attached = AttachVirtualDisk(
            handle,
            None,
            ATTACH_VIRTUAL_DISK_FLAG_READ_ONLY | ATTACH_VIRTUAL_DISK_FLAG_PERMANENT_LIFETIME,
            0,
            None,
            None,
        );

        // The handle can be closed: PERMANENT_LIFETIME keeps the mount.
        let _ = CloseHandle(handle);

        attached == ERROR_SUCCESS
    }
}

/// Unmounts a previously mounted `.iso`/`.img` image.
///
/// `OpenVirtualDisk` (`DETACH` access) then `DetachVirtualDisk`. Returns
/// `false` if the image is not mounted or on error.
pub fn unmount_iso(path: &str) -> bool {
    if !is_iso(path) {
        return false;
    }

    let storage_type = iso_storage_type();
    let path_w = wide(path);
    let mut handle = HANDLE::default();

    unsafe {
        let opened = OpenVirtualDisk(
            &storage_type,
            PCWSTR(path_w.as_ptr()),
            VIRTUAL_DISK_ACCESS_DETACH,
            OPEN_VIRTUAL_DISK_FLAG_NONE,
            None,
            &mut handle,
        );
        if opened != ERROR_SUCCESS || handle.is_invalid() {
            return false;
        }

        let detached = DetachVirtualDisk(handle, DETACH_VIRTUAL_DISK_FLAG_NONE, 0);

        let _ = CloseHandle(handle);

        detached == ERROR_SUCCESS
    }
}

// ---------------------------------------------------------------------------
// BitLocker
// ---------------------------------------------------------------------------

/// Normalizes a drive letter to `X:` (or `None` if invalid).
fn drive_root_colon(drive: char) -> Option<String> {
    let c = drive.to_ascii_uppercase();
    c.is_ascii_alphabetic().then(|| format!("{c}:"))
}

/// True if the volume responds to `GetVolumeInformationW` (so mounted and
/// readable). A LOCKED BitLocker drive fails here (access denied / not
/// ready), which serves as a quick, elevation-free test in
/// `is_bitlocker_locked`.
fn volume_accessible(drive: char) -> bool {
    let Some(colon) = drive_root_colon(drive) else {
        return false;
    };
    let root = wide(&format!("{colon}\\"));
    unsafe {
        GetVolumeInformationW(PCWSTR(root.as_ptr()), None, None, None, None, None).is_ok()
    }
}

/// True if the drive is a currently LOCKED BitLocker volume.
///
/// The C# doesn't provide this information (it goes through the shell
/// menu). We reconstruct it robustly:
/// 1. fast path: if the volume responds to `GetVolumeInformationW`, it is
///    accessible so NOT locked → `false` (common case, no process needed);
/// 2. otherwise we confirm via `manage-bde -status X:` by looking for the
///    "Locked" state. `manage-bde` requires elevation: without rights, we
///    fall back to `false` (silent failure, per the spec).
///
/// Note: parsing `manage-bde` depends on the Windows display language
/// (token "Locked" in English). Any ambiguity = `false`.
pub fn is_bitlocker_locked(drive: char) -> bool {
    // 1. Readable volume => not locked.
    if volume_accessible(drive) {
        return false;
    }

    // 2. Confirmation via manage-bde -status.
    let Some(colon) = drive_root_colon(drive) else {
        return false;
    };
    let output = std::process::Command::new("manage-bde")
        .args(["-status", &colon])
        .output();

    match output {
        Ok(out) => {
            let text = String::from_utf8_lossy(&out.stdout);
            // "Lock Status:  Locked" line on a locked encrypted volume.
            text.lines()
                .filter(|l| l.to_ascii_lowercase().contains("lock status"))
                .any(|l| l.to_ascii_lowercase().contains("locked"))
        }
        Err(_) => false,
    }
}

/// Unlocks a BitLocker drive with a user password.
///
/// The C# does NOT do this (shell menu "manage-bde"). Best-effort as
/// requested by the task: we invoke the system tool
/// `manage-bde -unlock X: -password`.
///
/// TODO(limitation): `manage-bde -unlock -Password` reads the password
/// interactively on the console; here we supply it via standard input.
/// Depending on the Windows version this may not be accepted silently.
/// Requires elevation. A fully reliable implementation would go through
/// the WMI API `Win32_EncryptableVolume::UnlockWithPassphrase` (COM), not
/// ported here to keep things lightweight. Any error => `false`.
pub fn unlock_bitlocker(drive: char, password: &str) -> bool {
    use std::io::Write;
    use std::process::Stdio;

    let Some(colon) = drive_root_colon(drive) else {
        return false;
    };

    let child = std::process::Command::new("manage-bde")
        .args(["-unlock", &colon, "-password"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();

    let mut child = match child {
        Ok(c) => c,
        Err(_) => return false,
    };

    // The password is requested on stdin; we write it there (twice is not
    // needed for -unlock, unlike -changepassword).
    if let Some(stdin) = child.stdin.as_mut() {
        let _ = writeln!(stdin, "{password}");
    }

    match child.wait() {
        Ok(status) => status.success(),
        Err(_) => false,
    }
}

// ---------------------------------------------------------------------------
// Non-destructive tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_iso_reconnait_les_extensions() {
        assert!(is_iso("D:\\images\\ubuntu.iso"));
        assert!(is_iso("D:\\images\\disk.IMG"));
        assert!(!is_iso("D:\\images\\photo.png"));
        assert!(!is_iso(""));
    }

    #[test]
    fn is_bitlocker_locked_ne_panique_pas_sur_c() {
        // C: is the system disk: normally accessible so not locked.
        // We mainly check that it does not panic.
        let _ = is_bitlocker_locked('C');
    }
}
