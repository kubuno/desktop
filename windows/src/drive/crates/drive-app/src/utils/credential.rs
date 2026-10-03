//! Network credential entry — port of Files' credential prompt when
//! connecting to a protected `\\server` share.
//!
//! ## What the original C# does
//!
//! The trigger is `NetworkService.AuthenticateNetworkShare(string path)`
//! (`Files.App/Services/Storage/StorageNetworkService.cs`, lines 224-312):
//!
//! 1. It tries to open the share without credentials via
//!    `WNetAddConnection3W(NETRESOURCEW { RESOURCETYPE_DISK, lpRemoteName = path }, null, null, 0)`.
//!    If credentials are already stored in the Windows Credential Manager,
//!    the call returns `NO_ERROR` and nothing else happens.
//! 2. If the call fails with `ERROR_LOGON_FAILURE` **or** `ERROR_ACCESS_DENIED`
//!    (i.e. "access denied" while enumerating/opening the `\\…` path),
//!    Files shows a credential entry dialog
//!    (`DynamicDialogFactory.GetFor_CredentialEntryDialog(path)`) that
//!    collects:
//!    - a username (`userAndPass[0]`),
//!    - a password (`userAndPass[1]`),
//!    - a "remember" checkbox (`userAndPass[2]` == "y"/"n"),
//!
//!    with title `NetworkAuthenticationDialogTitle` ("Enter network
//!    credentials") and message `NetworkAuthenticationDialogMessage`
//!    ("Enter your credentials to connect to: {0}", `{0}` = `path[2..]`,
//!    i.e. the path without the `\\` prefix).
//! 3. If the user confirms, Files calls
//!    `WNetAddConnection3W(netRes, password, username, 0)` again. On
//!    success **and** if the checkbox is checked, it persists the
//!    credential via `CredWrite` (type `CRED_TYPE_DOMAIN_PASSWORD`,
//!    persistence `CRED_PERSIST_ENTERPRISE`, `TargetName = path[2..]`).
//!    Cancellation → `AuthenticateNetworkShare` returns `false`.
//!
//! Files' C# draws its own XAML box (`CredentialDialog`). Since this port
//! is a pure Win32 UI, we use here the system's native equivalent:
//! `CredUIPromptForWindowsCredentialsW` (`credui.dll`), Windows' standard
//! "Enter network credentials" box, with the "remember" checkbox via
//! `CREDUIWIN_CHECKBOX`. The result is an opaque credential buffer that we
//! decode into `username`/`password` with
//! `CredUnPackAuthenticationBufferW`.
//!
//! ## Where to wire it up (port side)
//!
//! To be connected in the future `NetworkService::authenticate_network_share(path)`:
//! when opening a `\\server\share` path fails with access denied
//! (`WNetAddConnection2W`/`WNetAddConnection3W` → `ERROR_LOGON_FAILURE` /
//! `ERROR_ACCESS_DENIED`), call [`prompt_credentials`], then retry
//! `WNetAddConnection2W`/`WNetAddConnection3W` with the obtained
//! `username`/`password`. If `save` is true and the connection succeeds,
//! persist via `CredWriteW` (`CRED_TYPE_DOMAIN_PASSWORD` /
//! `CRED_PERSIST_ENTERPRISE`), exactly like the C#. `prompt_credentials`
//! returns `None` on user cancellation or error.
//!
//! ## Robustness
//!
//! None of these functions can panic: any API error, null buffer or zero
//! size, any cancellation results in `None`. The credential buffer
//! returned by Windows (`CoTaskMemAlloc` memory) is zeroed then freed
//! (`CoTaskMemFree`) in all success cases.

// NOT WIRED YET. Ported credential prompt (CredUI) for network locations; waits for the connect-to-network-share flow.
// The allow is scoped to this file and must go once it is wired.
#![allow(dead_code)]

use std::iter::once;

use windows::core::{BOOL, PCWSTR, PWSTR};
use windows::Win32::Foundation::{ERROR_SUCCESS, HWND};
use windows::Win32::Security::Credentials::{
    CredUIPromptForWindowsCredentialsW, CredUnPackAuthenticationBufferW, CREDUIWIN_CHECKBOX,
    CREDUI_INFOW, CRED_PACK_FLAGS,
};
use windows::Win32::System::Com::CoTaskMemFree;

/// Network credentials entered by the user.
///
/// Mirrors the three elements collected by the C#
/// (`DynamicDialogFactory.GetFor_CredentialEntryDialog` → `userAndPass[0..2]`).
pub struct Credential {
    /// Username (possibly in `domain\user` format).
    pub username: String,
    /// Password in plain text.
    pub password: String,
    /// True if the user checked "Remember my credentials".
    pub save: bool,
}

/// Encodes a Rust string as UTF-16 terminated by `\0` (for `PCWSTR`).
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(once(0)).collect()
}

/// Decodes a UTF-16 buffer, stopping at the first `\0`.
fn from_wide_nul(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

/// Shows the system "Enter network credentials" box for the `target` share
/// (server/share name, e.g. `server\share` — the path without the `\\`
/// prefix, like the C# message).
///
/// - `parent` : owner window (can be `HWND(0)` if none).
/// - `target` : target shown in the prompt message.
///
/// Returns `Some(Credential)` if the user confirms, `None` if they cancel
/// (`ERROR_CANCELLED`) or on error. Never panics.
pub fn prompt_credentials(parent: HWND, target: &str) -> Option<Credential> {
    // Title and message modeled after the C# resources
    // `NetworkAuthenticationDialogTitle` and
    // `NetworkAuthenticationDialogMessage` (fr-FR). `{0}` = the `target`.
    let caption = wide("Entrez les identifiants réseau");
    let message = wide(&format!(
        "Entrez vos identifiants afin de vous connecter à : {target}"
    ));

    unsafe {
        let info = CREDUI_INFOW {
            cbSize: std::mem::size_of::<CREDUI_INFOW>() as u32,
            hwndParent: parent,
            pszMessageText: PCWSTR(message.as_ptr()),
            pszCaptionText: PCWSTR(caption.as_ptr()),
            hbmBanner: Default::default(),
        };

        let mut auth_package: u32 = 0;
        let mut out_buffer: *mut core::ffi::c_void = std::ptr::null_mut();
        let mut out_size: u32 = 0;
        // Initial (and returned) state of the "remember" checkbox.
        let mut save = BOOL(0);

        // Shows the native box. Return value: 0 (ERROR_SUCCESS) = confirmed,
        // 1223 (ERROR_CANCELLED) = cancelled, other = error.
        let result = CredUIPromptForWindowsCredentialsW(
            Some(&info),
            0,
            &mut auth_package,
            None,
            0,
            &mut out_buffer,
            &mut out_size,
            Some(&mut save),
            CREDUIWIN_CHECKBOX,
        );

        // Cancellation or error → no credential.
        if result != ERROR_SUCCESS.0 {
            return None;
        }
        if out_buffer.is_null() || out_size == 0 {
            return None;
        }

        // Decodes the opaque buffer into username/domain/password. Pass 1:
        // empty buffers to obtain the required sizes (in characters,
        // `\0` terminator included).
        let mut user_len: u32 = 0;
        let mut domain_len: u32 = 0;
        let mut pass_len: u32 = 0;
        let _ = CredUnPackAuthenticationBufferW(
            CRED_PACK_FLAGS(0),
            out_buffer,
            out_size,
            None,
            &mut user_len,
            None,
            Some(&mut domain_len),
            None,
            &mut pass_len,
        );

        // If Windows requests no size at all, nothing can be extracted.
        if user_len == 0 && pass_len == 0 {
            secure_free(out_buffer, out_size);
            return None;
        }

        // Pass 2: allocation then actual decoding.
        let mut user = vec![0u16; user_len.max(1) as usize];
        let mut domain = vec![0u16; domain_len.max(1) as usize];
        let mut pass = vec![0u16; pass_len.max(1) as usize];

        let decoded = CredUnPackAuthenticationBufferW(
            CRED_PACK_FLAGS(0),
            out_buffer,
            out_size,
            Some(PWSTR(user.as_mut_ptr())),
            &mut user_len,
            Some(PWSTR(domain.as_mut_ptr())),
            Some(&mut domain_len),
            Some(PWSTR(pass.as_mut_ptr())),
            &mut pass_len,
        )
        .is_ok();

        // The credential buffer is no longer needed: zero it and free it.
        secure_free(out_buffer, out_size);

        if !decoded {
            // Zeroes the intermediate buffers for hygiene before giving up.
            zero_u16(&mut user);
            zero_u16(&mut domain);
            zero_u16(&mut pass);
            return None;
        }

        let username_part = from_wide_nul(&user);
        let domain_part = from_wide_nul(&domain);
        let password = from_wide_nul(&pass);

        // Zeroes the UTF-16 buffers once the strings have been extracted.
        zero_u16(&mut user);
        zero_u16(&mut domain);
        zero_u16(&mut pass);

        // Rebuilds `domain\user` if a distinct domain was provided and the
        // username doesn't already contain it.
        let username = if !domain_part.is_empty()
            && !username_part.contains('\\')
            && !username_part.contains('@')
        {
            format!("{domain_part}\\{username_part}")
        } else {
            username_part
        };

        Some(Credential {
            username,
            password,
            save: save.as_bool(),
        })
    }
}

/// Zeroes then frees (`CoTaskMemFree`) the credential buffer returned by
/// `CredUIPromptForWindowsCredentialsW`.
///
/// # Safety
/// `buffer` must be a valid `CoTaskMemAlloc` pointer of `size` bytes (which
/// `CredUIPromptForWindowsCredentialsW` guarantees on success), or null.
unsafe fn secure_free(buffer: *mut core::ffi::c_void, size: u32) {
    if buffer.is_null() {
        return;
    }
    unsafe {
        std::ptr::write_bytes(buffer as *mut u8, 0, size as usize);
        CoTaskMemFree(Some(buffer));
    }
}

/// Zeroes a UTF-16 buffer (hygiene: avoids leaving a password lying around).
fn zero_u16(buf: &mut [u16]) {
    for c in buf.iter_mut() {
        *c = 0;
    }
}
