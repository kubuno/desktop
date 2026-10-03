//! The `kubuno://` URL protocol — the web-to-desktop hand-off, the way Zoom's
//! `zoommtg://` links open its app — and the single-instance forwarding.
//!
//! When the web wants a meeting to run in the desktop app it navigates to a
//! `kubuno://meet/<conversation_id>` link. Windows looks the scheme up under
//! `HKCU\Software\Classes\kubuno`, which [`register`] points at this executable,
//! and launches it with the URL as the first argument. [`from_args`] parses it;
//! `main` hands it to an already-running instance ([`forward_to_running`], a
//! `WM_COPYDATA`), which foregrounds and opens the conversation.

/// The scheme this app owns.
pub const SCHEME: &str = "kubuno";

/// The named mutex that tells a second launch an instance is already running.
pub const SINGLETON_MUTEX: &str = "Local\\KubunoChatSingletonMutex";

/// `COPYDATASTRUCT::dwData` of a forwarded link (anything else is not ours).
pub const COPYDATA_LINK: usize = 1;

/// A parsed hand-off target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeepLink {
    /// Join/open a meeting — a conversation flagged `is_meeting`.
    Meet(String),
    /// Open a conversation.
    Chat(String),
}

impl DeepLink {
    /// The conversation id the link points at (a meeting is a conversation too).
    pub fn conversation_id(&self) -> &str {
        match self {
            DeepLink::Meet(id) | DeepLink::Chat(id) => id,
        }
    }

    pub fn is_meeting(&self) -> bool {
        matches!(self, DeepLink::Meet(_))
    }

    /// The link back as a URL (what "Copy the conversation link" puts on the clipboard).
    pub fn to_url(&self) -> String {
        match self {
            DeepLink::Meet(id) => format!("{SCHEME}://meet/{id}"),
            DeepLink::Chat(id) => format!("{SCHEME}://chat/{id}"),
        }
    }
}

/// Parses `kubuno://meet/<id>` or `kubuno://chat/<id>`. Anything else is `None`,
/// so a stray argument can never be mistaken for a link.
pub fn parse(url: &str) -> Option<DeepLink> {
    let rest = url.strip_prefix("kubuno://").or_else(|| url.strip_prefix("kubuno:"))?;
    let rest = rest.trim_matches('/');
    if let Some(id) = rest.strip_prefix("meet/") {
        return Some(DeepLink::Meet(clean(id)));
    }
    if let Some(id) = rest.strip_prefix("chat/") {
        return Some(DeepLink::Chat(clean(id)));
    }
    None
}

/// The id, trimmed of any query string or trailing slash.
fn clean(id: &str) -> String {
    id.split(['?', '#', '/']).next().unwrap_or(id).to_string()
}

/// The raw `kubuno:` argument among `args`, if any.
pub fn raw_arg_in(args: impl IntoIterator<Item = String>) -> Option<String> {
    args.into_iter().skip(1).find(|a| a.starts_with("kubuno:"))
}

/// The raw `kubuno:` argument this process was launched with, if any.
pub fn raw_arg() -> Option<String> {
    raw_arg_in(std::env::args())
}

/// The parsed deep link this process was launched with, if any.
pub fn from_args() -> Option<DeepLink> {
    raw_arg().as_deref().and_then(parse)
}

/// The link a `WM_COPYDATA` payload carries (`dwData` = [`COPYDATA_LINK`], UTF-8 URL bytes).
pub fn link_from_copydata(kind: usize, bytes: &[u8]) -> Option<DeepLink> {
    if kind != COPYDATA_LINK {
        return None;
    }
    std::str::from_utf8(bytes).ok().and_then(parse)
}

/// Registers `kubuno://` under the current user, pointing at this executable.
/// Idempotent — run at every startup so a moved binary re-points itself. Never
/// fatal: a machine where the registry write is refused simply cannot receive
/// hand-offs, which must not stop the app from opening.
#[cfg(windows)]
pub fn register() {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    let Ok(exe) = std::env::current_exe() else { return };
    let command = format!("\"{}\" \"%1\"", exe.display());
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let result = (|| -> std::io::Result<()> {
        let (root, _) = hkcu.create_subkey(format!("Software\\Classes\\{SCHEME}"))?;
        root.set_value("", &"URL:Kubuno Protocol")?;
        // The marker Windows requires for a URL-protocol class.
        root.set_value("URL Protocol", &"")?;
        let (cmd, _) = hkcu.create_subkey(format!("Software\\Classes\\{SCHEME}\\shell\\open\\command"))?;
        cmd.set_value("", &command)?;
        Ok(())
    })();
    if let Err(e) = result {
        kubuno_desktop::tracing::warn!("[protocol] registering kubuno:// failed: {e}");
    }
}

/// Whether another instance already runs (the named mutex existed). The handle is
/// kept for the life of the process (it marks this instance as the running one).
#[cfg(windows)]
pub fn another_instance_runs() -> bool {
    use windows::core::HSTRING;
    use windows::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS};
    use windows::Win32::System::Threading::CreateMutexW;
    let name = HSTRING::from(SINGLETON_MUTEX);
    // SAFETY: a plain named-mutex creation; the handle is never closed, so the mutex
    // lives as long as the process.
    unsafe {
        match CreateMutexW(None, false, &name) {
            Ok(_handle) => {
                GetLastError() == ERROR_ALREADY_EXISTS
            }
            Err(_) => false,
        }
    }
}

/// Hands a `kubuno://` link to the already-running instance (or just raises it).
/// The running window is the Kubuno host window titled like the app.
#[cfg(windows)]
pub fn forward_to_running(url: Option<&str>, title: &str) {
    use windows::core::{w, HSTRING};
    use windows::Win32::Foundation::{LPARAM, WPARAM};
    use windows::Win32::System::DataExchange::COPYDATASTRUCT;
    use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, SendMessageW, SetForegroundWindow, ShowWindow, SW_RESTORE, WM_COPYDATA};
    let title = HSTRING::from(title);
    // SAFETY: Win32 calls on a window handle we just looked up; the COPYDATASTRUCT and its
    // bytes outlive the synchronous SendMessage.
    unsafe {
        let Ok(hwnd) = FindWindowW(w!("KubunoControlsHost"), &title) else { return };
        if let Some(url) = url {
            let bytes = url.as_bytes();
            let cds = COPYDATASTRUCT { dwData: COPYDATA_LINK, cbData: bytes.len() as u32, lpData: bytes.as_ptr() as *mut core::ffi::c_void };
            SendMessageW(hwnd, WM_COPYDATA, Some(WPARAM(0)), Some(LPARAM(&cds as *const _ as isize)));
        }
        let _ = ShowWindow(hwnd, SW_RESTORE);
        let _ = SetForegroundWindow(hwnd);
    }
}

/// Reads the link of a `WM_COPYDATA` the window received (its `lparam`).
#[cfg(windows)]
pub fn link_from_copydata_lparam(lparam: isize) -> Option<DeepLink> {
    use windows::Win32::System::DataExchange::COPYDATASTRUCT;
    let cds = lparam as *const COPYDATASTRUCT;
    if cds.is_null() {
        return None;
    }
    // SAFETY: WM_COPYDATA's lparam points at a COPYDATASTRUCT valid for the duration of the
    // message; its bytes are copied out before returning.
    unsafe {
        let (kind, ptr, len) = ((*cds).dwData, (*cds).lpData as *const u8, (*cds).cbData as usize);
        if ptr.is_null() || len == 0 {
            return None;
        }
        link_from_copydata(kind, std::slice::from_raw_parts(ptr, len))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_parse() {
        assert_eq!(parse("kubuno://meet/abc"), Some(DeepLink::Meet("abc".into())));
        assert_eq!(parse("kubuno://chat/abc/"), Some(DeepLink::Chat("abc".into())));
        assert_eq!(parse("kubuno:chat/abc?x=1#y"), Some(DeepLink::Chat("abc".into())));
        assert_eq!(parse("kubuno://files/abc"), None);
        assert_eq!(parse("https://chat/abc"), None);
        let link = DeepLink::Meet("r1".into());
        assert!(link.is_meeting());
        assert_eq!(link.conversation_id(), "r1");
        assert_eq!(parse(&link.to_url()), Some(link));
    }

    #[test]
    fn arguments_and_copydata() {
        let args = ["kubuno-chat.exe", "--sample", "kubuno://chat/42"].map(String::from);
        assert_eq!(raw_arg_in(args), Some("kubuno://chat/42".into()));
        assert_eq!(raw_arg_in(["kubuno:x".to_string()]), None, "the program name is not an argument");
        assert_eq!(link_from_copydata(COPYDATA_LINK, b"kubuno://chat/7"), Some(DeepLink::Chat("7".into())));
        assert_eq!(link_from_copydata(2, b"kubuno://chat/7"), None);
        assert_eq!(link_from_copydata(COPYDATA_LINK, &[0xFF, 0xFE]), None);
    }
}
