//! FileTagsHelper (mirrors FileTagsHelper.cs)
//!
//! Port of `Files.App/Utils/FileTags/FileTagsHelper.cs`: a file's tags
//! live in its NTFS alternate data stream `:files`, as comma-separated
//! uids. Writing preserves the modification date and works around the
//! read-only attribute (#7534), like the original. (The FRN-based
//! resilience to moves is not ported.)

use windows::core::HSTRING;
use windows::Win32::Storage::FileSystem::{
    GetFileAttributesW, SetFileAttributesW, FILE_ATTRIBUTE_READONLY,
    FILE_FLAGS_AND_ATTRIBUTES, INVALID_FILE_ATTRIBUTES,
};

/// `ReadFileTag`: the tag uids from the `:files` stream.
pub fn read_file_tags(path: &str) -> Vec<String> {
    std::fs::read_to_string(format!("{path}:files"))
        .map(|s| {
            s.split(',')
                .filter(|p| !p.is_empty())
                .map(|p| p.trim().to_string())
                .collect()
        })
        .unwrap_or_default()
}

/// `WriteFileTag`: writes (or removes) the stream, without touching either
/// the modification date or the read-only attribute.
pub fn write_file_tags(path: &str, tags: &[String]) {
    let modified = std::fs::metadata(path).and_then(|m| m.modified()).ok();

    let wide = HSTRING::from(path);
    let attributes = unsafe { GetFileAttributesW(&wide) };
    let read_only =
        attributes != INVALID_FILE_ATTRIBUTES && attributes & FILE_ATTRIBUTE_READONLY.0 != 0;
    if read_only {
        unsafe {
            let _ = SetFileAttributesW(
                &wide,
                FILE_FLAGS_AND_ATTRIBUTES(attributes & !FILE_ATTRIBUTE_READONLY.0),
            );
        }
    }

    let stream = format!("{path}:files");
    if tags.is_empty() {
        let _ = std::fs::remove_file(&stream);
    } else {
        let _ = std::fs::write(&stream, tags.join(","));
    }

    if read_only {
        unsafe {
            let _ = SetFileAttributesW(&wide, FILE_FLAGS_AND_ATTRIBUTES(attributes));
        }
    }
    if let Some(modified) = modified {
        restore_modified(path, modified);
    }
}

/// `SetFileDateModified`: rewrites LastWriteTime after writing the stream
/// (creating/writing an ADS updates the carrier file's date).
fn restore_modified(path: &str, modified: std::time::SystemTime) {
    use windows::Win32::Foundation::FILETIME;
    use windows::Win32::Storage::FileSystem::{
        CreateFileW, SetFileTime, FILE_FLAG_BACKUP_SEMANTICS, FILE_SHARE_READ,
        FILE_SHARE_WRITE, FILE_WRITE_ATTRIBUTES, OPEN_EXISTING,
    };
    let Ok(elapsed) = modified.duration_since(std::time::UNIX_EPOCH) else { return };
    // SystemTime (Unix epoch) → FILETIME (100 ns intervals since 1601).
    let intervals = elapsed.as_nanos() as u64 / 100 + 116_444_736_000_000_000;
    let ft = FILETIME {
        dwLowDateTime: intervals as u32,
        dwHighDateTime: (intervals >> 32) as u32,
    };
    unsafe {
        if let Ok(handle) = CreateFileW(
            &HSTRING::from(path),
            FILE_WRITE_ATTRIBUTES.0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS,
            None,
        ) {
            let _ = SetFileTime(handle, None, None, Some(&ft));
            let _ = windows::Win32::Foundation::CloseHandle(handle);
        }
    }
}

/// Toggles a uid in `path`'s tags (checking it in the "Edit tags" flyout).
pub fn toggle_file_tag(path: &str, uid: &str) {
    let mut tags = read_file_tags(path);
    if let Some(i) = tags.iter().position(|t| t == uid) {
        tags.remove(i);
    } else {
        tags.push(uid.to_string());
    }
    write_file_tags(path, &tags);
}
