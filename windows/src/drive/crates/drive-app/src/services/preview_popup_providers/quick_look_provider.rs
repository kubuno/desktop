//! QuickLookProvider (mirror of `QuickLookProvider.cs`).
//!
//! Controls QuickLook via its named pipe (`\\.\pipe\QuickLook.App.Pipe.{SID}`):
//! availability detection and sending Toggle/Switch messages. The current
//! user's SID is resolved via local Win32 interop (advapi32/kernel32)
//! so as not to modify the shared Cargo.toml.

// The public API (`toggle_preview`/`switch_preview`/`is_available`) isn't
// called yet: the main window's keyboard hook still needs to be wired up.
#![allow(dead_code)]

use std::sync::OnceLock;

use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, WriteFile, FILE_FLAGS_AND_ATTRIBUTES, FILE_SHARE_MODE, OPEN_EXISTING,
};

/// `private const int TIMEOUT = 500;` (QuickLookProvider.cs) — maximum
/// wait for a free pipe server instance.
const TIMEOUT: u32 = 500;

// `GENERIC_WRITE` (0x40000000): the pipe is opened write-only, like the
// `PipeDirection.Out` of `NamedPipeClientStream`.
const GENERIC_WRITE: u32 = 0x4000_0000;

/// `pipeMessageSwitch` — switches the content of the already-open popup.
pub(super) const MSG_SWITCH: &str = "QuickLook.App.PipeMessages.Switch";
/// `pipeMessageToggle` — opens/closes the popup.
pub(super) const MSG_TOGGLE: &str = "QuickLook.App.PipeMessages.Toggle";

// ---------------------------------------------------------------------------
// Win32 interop not covered by the crate's enabled `windows` features:
// current process token (for the SID) and waiting for a pipe instance.
// Local FFI declarations so as NOT to modify the shared Cargo.toml.
// ---------------------------------------------------------------------------

// `TOKEN_QUERY`.
const TOKEN_QUERY: u32 = 0x0008;
// `TOKEN_INFORMATION_CLASS.TokenUser`.
const TOKEN_USER: i32 = 1;
// Pseudo-handle returned by `GetCurrentProcess()` (constant value -1).
const CURRENT_PROCESS: isize = -1;

#[link(name = "advapi32")]
extern "system" {
    fn OpenProcessToken(process: isize, desired: u32, token: *mut isize) -> i32;
    fn GetTokenInformation(
        token: isize,
        class: i32,
        info: *mut core::ffi::c_void,
        len: u32,
        ret: *mut u32,
    ) -> i32;
    fn ConvertSidToStringSidW(sid: *mut core::ffi::c_void, out: *mut *mut u16) -> i32;
}

// The pipe HANDLE is closed via the `windows` crate's `CloseHandle`; the
// token (raw isize) is closed via this local FFI on the same kernel32 symbol.
#[link(name = "kernel32")]
extern "system" {
    fn WaitNamedPipeW(name: *const u16, timeout: u32) -> i32;
    fn LocalFree(mem: *mut core::ffi::c_void) -> *mut core::ffi::c_void;
    #[link_name = "CloseHandle"]
    fn CloseTokenHandle(handle: isize) -> i32;
}

/// Current user's SID, as a string (`S-1-5-21-…`).
///
/// Reproduces `WindowsIdentity.GetCurrent().User?.Value`:
/// `OpenProcessToken(TOKEN_QUERY)` → `GetTokenInformation(TokenUser)` →
/// `ConvertSidToStringSidW`. Memoized (the SID doesn't change within the session).
fn current_user_sid() -> Option<&'static str> {
    static SID: OnceLock<Option<String>> = OnceLock::new();
    SID.get_or_init(|| unsafe {
        let mut token: isize = 0;
        if OpenProcessToken(CURRENT_PROCESS, TOKEN_QUERY, &mut token) == 0 {
            return None;
        }
        // First call: size of the TOKEN_USER buffer.
        let mut len = 0u32;
        GetTokenInformation(token, TOKEN_USER, std::ptr::null_mut(), 0, &mut len);
        if len == 0 {
            CloseTokenHandle(token);
            return None;
        }
        let mut buf = vec![0u8; len as usize];
        let ok = GetTokenInformation(
            token,
            TOKEN_USER,
            buf.as_mut_ptr() as *mut core::ffi::c_void,
            len,
            &mut len,
        );
        CloseTokenHandle(token);
        if ok == 0 {
            return None;
        }
        // TOKEN_USER starts with SID_AND_ATTRIBUTES { PSID Sid; … }: the first
        // field is the PSID pointer (unaligned read, byte buffer).
        let psid = (buf.as_ptr() as *const *mut core::ffi::c_void).read_unaligned();
        if psid.is_null() {
            return None;
        }
        let mut str_ptr: *mut u16 = std::ptr::null_mut();
        if ConvertSidToStringSidW(psid, &mut str_ptr) == 0 || str_ptr.is_null() {
            return None;
        }
        let mut n = 0usize;
        while *str_ptr.add(n) != 0 {
            n += 1;
        }
        let s = String::from_utf16_lossy(std::slice::from_raw_parts(str_ptr, n));
        LocalFree(str_ptr as *mut core::ffi::c_void);
        Some(s)
    })
    .as_deref()
}

/// Full path of the QuickLook pipe for the current user, NUL-terminated
/// (for `CreateFileW`/`WaitNamedPipeW`).
///
/// `$"QuickLook.App.Pipe.{WindowsIdentity.GetCurrent().User?.Value}"` on
/// `NamedPipeClientStream(".", …)` → `\\.\pipe\QuickLook.App.Pipe.{SID}`.
fn quicklook_pipe_wide() -> Option<Vec<u16>> {
    let sid = current_user_sid()?;
    let path = format!(r"\\.\pipe\QuickLook.App.Pipe.{sid}");
    Some(path.encode_utf16().chain(std::iter::once(0)).collect())
}

/// Opens the pipe for writing (waiting for a free instance, like
/// `ConnectAsync(TIMEOUT)`). `None` if QuickLook isn't listening.
unsafe fn open_quicklook_pipe(wide: &[u16]) -> Option<HANDLE> {
    let name = PCWSTR(wide.as_ptr());
    // Direct attempt.
    if let Ok(h) = CreateFileW(
        name,
        GENERIC_WRITE,
        FILE_SHARE_MODE(0),
        None,
        OPEN_EXISTING,
        FILE_FLAGS_AND_ATTRIBUTES(0),
        None,
    ) {
        if !h.is_invalid() {
            return Some(h);
        }
    }
    // All instances busy: wait for one to free up (ERROR_PIPE_BUSY).
    if WaitNamedPipeW(wide.as_ptr(), TIMEOUT) != 0 {
        if let Ok(h) = CreateFileW(
            name,
            GENERIC_WRITE,
            FILE_SHARE_MODE(0),
            None,
            OPEN_EXISTING,
            FILE_FLAGS_AND_ATTRIBUTES(0),
            None,
        ) {
            if !h.is_invalid() {
                return Some(h);
            }
        }
    }
    None
}

/// Writes a line to the pipe, identically to
/// `StreamWriter.WriteLineAsync($"{message}|{path}")`.
///
/// ENCODING: `QuickLookProvider.cs` line 40 does `new StreamWriter(client)`
/// WITHOUT an explicit encoder. .NET's default `StreamWriter` writes
/// **UTF-8 without BOM** (`UTF8NoBOM`), NOT UTF-16. `WriteLineAsync` appends
/// `Environment.NewLine`, i.e. `"\r\n"` on Windows. So we reproduce
/// exactly: UTF-8 bytes of `"{message}|{path}\r\n"`.
unsafe fn write_pipe_line(handle: HANDLE, message: &str, path: &str) {
    let line = format!("{message}|{path}\r\n");
    let bytes = line.as_bytes();
    let _ = WriteFile(handle, Some(bytes), None, None);
}

/// QuickLook's `DetectAvailability()`: opens the pipe, sends
/// `"{switch}|"` (empty path) and considers QuickLook available if opening
/// succeeds (the original tests `NumberOfServerInstances != 0`, which is
/// equivalent to a successful connection).
pub(super) fn quicklook_available() -> bool {
    let Some(wide) = quicklook_pipe_wide() else {
        return false;
    };
    unsafe {
        let Some(handle) = open_quicklook_pipe(&wide) else {
            return false;
        };
        write_pipe_line(handle, MSG_SWITCH, "");
        let _ = CloseHandle(handle);
        true
    }
}

/// Sends a QuickLook message (`DoPreviewAsync`): opens the pipe, writes,
/// closes. No-op if QuickLook isn't listening.
pub(super) fn quicklook_send(message: &str, path: &str) {
    let Some(wide) = quicklook_pipe_wide() else {
        return;
    };
    unsafe {
        let Some(handle) = open_quicklook_pipe(&wide) else {
            return;
        };
        write_pipe_line(handle, message, path);
        let _ = CloseHandle(handle);
    }
}
