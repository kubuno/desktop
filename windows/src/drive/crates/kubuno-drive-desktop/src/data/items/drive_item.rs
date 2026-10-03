//! DriveItem (mirrors DriveItem.cs)

use windows::Win32::Foundation::MAX_PATH;
use windows::Win32::Storage::FileSystem::{
    GetDiskFreeSpaceExW, GetDriveTypeW, GetLogicalDrives, GetVolumeInformationW,
};

use super::format_bytes_fr;

#[derive(Debug, Clone)]
pub struct DriveItem {
    pub letter: char,
    pub label: String,
    pub total_bytes: u64,
    pub free_bytes: u64,
    /// `DRIVE_REMOTE`: filed under the Network section, not Drives.
    pub is_network: bool,
}

impl DriveItem {
    pub fn display_name(&self) -> String {
        // A mapped network drive also shows its UNC, like the original:
        // "html (\servershare) (N:)".
        if self.is_network {
            if let Some(unc) = network_drive_unc(self.letter) {
                return format!("{} ({}) ({}:)", self.label, unc, self.letter);
            }
        }
        format!("{} ({}:)", self.label, self.letter)
    }

    pub fn usage_text(&self) -> String {
        format!(
            "{} libre(s) sur {}",
            format_bytes_fr(self.free_bytes),
            format_bytes_fr(self.total_bytes)
        )
    }

    pub fn used_fraction(&self) -> f32 {
        if self.total_bytes == 0 {
            return 0.0;
        }
        1.0 - (self.free_bytes as f64 / self.total_bytes as f64) as f32
    }
}

pub fn load_drives() -> Vec<DriveItem> {
    const DRIVE_FIXED: u32 = 3;
    const DRIVE_REMOTE: u32 = 4;
    let mask = unsafe { GetLogicalDrives() };
    let mut drives = Vec::new();
    for i in 0..26u32 {
        if mask & (1 << i) == 0 {
            continue;
        }
        let letter = (b'A' + i as u8) as char;
        let root: Vec<u16> = format!("{letter}:\\").encode_utf16().chain([0]).collect();
        let root_pcwstr = windows::core::PCWSTR(root.as_ptr());

        let drive_type = unsafe { GetDriveTypeW(root_pcwstr) };
        if drive_type != DRIVE_FIXED && drive_type != DRIVE_REMOTE {
            continue;
        }

        let mut free = 0u64;
        let mut total = 0u64;
        if unsafe { GetDiskFreeSpaceExW(root_pcwstr, None, Some(&mut total), Some(&mut free)) }
            .is_err()
        {
            // A disconnected network drive has no measurable space but
            // stays listed, like in the original.
            if drive_type != DRIVE_REMOTE {
                continue;
            }
        }

        let mut label_buf = [0u16; MAX_PATH as usize + 1];
        let label = unsafe {
            GetVolumeInformationW(root_pcwstr, Some(&mut label_buf), None, None, None, None)
        }
        .ok()
        .map(|_| String::from_utf16_lossy(&label_buf).trim_end_matches('\0').to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "Disque local".to_string());

        drives.push(DriveItem {
            letter,
            label,
            total_bytes: total,
            free_bytes: free,
            is_network: drive_type == DRIVE_REMOTE,
        });
    }
    drives
}

/// The UNC of a mapped network drive (`WNetGetConnectionW`).
fn network_drive_unc(letter: char) -> Option<String> {
    use windows::core::PCWSTR;
    use windows::Win32::NetworkManagement::WNet::WNetGetConnectionW;
    let local: Vec<u16> = format!("{letter}:").encode_utf16().chain([0]).collect();
    let mut buf = [0u16; 512];
    let mut len = buf.len() as u32;
    let ok = unsafe {
        WNetGetConnectionW(PCWSTR(local.as_ptr()), Some(windows::core::PWSTR(buf.as_mut_ptr())), &mut len)
    };
    (ok == windows::Win32::Foundation::NO_ERROR)
        .then(|| String::from_utf16_lossy(&buf).trim_end_matches('\0').to_string())
        .filter(|s| !s.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drives_enumerate() {
        let drives = load_drives();
        assert!(!drives.is_empty(), "at least one drive should be present");
        for d in &drives {
            assert!(d.total_bytes > 0);
        }
    }
}
