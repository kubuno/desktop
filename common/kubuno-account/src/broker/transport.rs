//! Local transport of the broker and its access control (vskubuno `docs/DESKTOP-OFFLINE-SYNC.md` §9):
//!
//! - **Windows**: named pipe `\\.\pipe\kubuno-auth-<user SID>`, created with a DACL granting access to the current
//!   user only, `PIPE_REJECT_REMOTE_CLIENTS`, `FILE_FLAG_FIRST_PIPE_INSTANCE` on the first instance (a second shell,
//!   or a squatter, makes `bind` fail instead of sharing the name). Each client's process id is read with
//!   `GetNamedPipeClientProcessId` and, under [`ClientPolicy::ImagesUnder`], its image path checked. In the other
//!   direction the **client checks the server** ([`ServerPolicy`]): `GetNamedPipeServerProcessId`, the server
//!   process must run as the same user and, for an app, be the installed shell executable. This closes the
//!   squatting window: while the shell is not running, any process of the same user could create the pipe first
//!   and answer the apps (hand them a database key of its choosing, or collect the account list).
//! - **Linux / macOS**: Unix domain socket `<runtime dir>/auth.sock`, directory `0700`, socket `0600`, and the
//!   peer's uid (`SO_PEERCRED` / `getpeereid`) must be the effective uid, on both sides; the image of the peer
//!   process (`/proc/<pid>/exe`, `proc_pidpath`) is checked against the policies like on Windows.

use std::io;
use std::path::{Path, PathBuf};

/// Who may talk to the broker (checked by the server).
#[derive(Debug, Clone, Default)]
pub enum ClientPolicy {
    /// Any process of the same user (what the OS access control already guarantees).
    #[default]
    SameUser,
    /// Same user **and** an executable under one of these directories (the installation directory). On macOS the
    /// image path comes from `proc_pidpath`, on Linux from `/proc/<pid>/exe`.
    ImagesUnder(Vec<PathBuf>),
}

impl ClientPolicy {
    #[cfg(feature = "server")]
    pub(crate) fn allows_image(&self, image: Option<&Path>) -> bool {
        match self {
            ClientPolicy::SameUser => true,
            ClientPolicy::ImagesUnder(dirs) => image.is_some_and(|img| is_under_any(img, dirs)),
        }
    }
}

/// What an app requires of the process that serves the broker (checked by the client, before it sends anything).
#[derive(Debug, Clone, Default)]
pub enum ServerPolicy {
    /// Any process of the same user. Only for tests and tools: an app uses [`ServerPolicy::Images`].
    #[default]
    SameUser,
    /// Same user **and** the server's executable is one of these files (the installed shell,
    /// `<install dir>/kubuno-desktop.exe`).
    Images(Vec<PathBuf>),
    /// Same user **and** the server's executable is under one of these directories.
    ImagesUnder(Vec<PathBuf>),
}

impl ServerPolicy {
    #[cfg(feature = "client")]
    pub(crate) fn allows_image(&self, image: Option<&Path>) -> bool {
        match self {
            ServerPolicy::SameUser => true,
            ServerPolicy::Images(files) => image.is_some_and(|img| {
                let img = comparable(img);
                files.iter().any(|f| comparable(f) == img)
            }),
            ServerPolicy::ImagesUnder(dirs) => image.is_some_and(|img| is_under_any(img, dirs)),
        }
    }
}

#[cfg(any(feature = "server", feature = "client"))]
fn is_under_any(image: &Path, dirs: &[PathBuf]) -> bool {
    let img = comparable(image);
    dirs.iter().any(|d| {
        let d = comparable(d);
        img.len() > d.len() && img.starts_with(&d) && img[d.len()..].starts_with(['/', '\\'])
    })
}

/// A path in a form two spellings of the same file agree on: resolved (symlinks, junctions), without the
/// Windows verbatim prefix, trailing separators removed, case-folded on Windows and macOS (case-insensitive
/// file systems by default).
#[cfg(any(feature = "server", feature = "client"))]
fn comparable(p: &Path) -> String {
    let resolved = std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let s = resolved.to_string_lossy();
    let s = s.strip_prefix(r"\\?\UNC\").map(|r| format!(r"\\{r}")).unwrap_or_else(|| s.strip_prefix(r"\\?\").unwrap_or(&s).to_string());
    let s = s.trim_end_matches(['/', '\\']).to_string();
    if cfg!(any(windows, target_os = "macos")) {
        s.to_lowercase()
    } else {
        s
    }
}

/// Where the broker listens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrokerEndpoint {
    /// A named pipe (`\\.\pipe\…`), Windows.
    Pipe(String),
    /// A Unix domain socket path.
    Socket(PathBuf),
}

impl BrokerEndpoint {
    /// The endpoint of the current user's shell. `runtime_dir` is ignored on Windows and is the per-user runtime
    /// directory elsewhere (`crate::paths::user_runtime_dir()`). A sandboxed profile (`KUBUNO_SANDBOX_DIR`) gets
    /// its own pipe name, so a sandboxed shell and the real one never answer each other's apps.
    pub fn for_current_user(runtime_dir: &Path) -> io::Result<Self> {
        #[cfg(windows)]
        {
            let _ = runtime_dir;
            let sandbox = crate::paths::sandbox_tag().map(|t| format!("-sandbox-{t}")).unwrap_or_default();
            Ok(BrokerEndpoint::Pipe(format!(r"\\.\pipe\kubuno-auth-{}{sandbox}", sys::current_user_sid()?)))
        }
        #[cfg(unix)]
        {
            Ok(BrokerEndpoint::Socket(runtime_dir.join("auth.sock")))
        }
    }

    /// A private endpoint for tests (unique name, still protected like the real one).
    pub fn for_test(name: &str, dir: &Path) -> Self {
        #[cfg(windows)]
        {
            let _ = dir;
            BrokerEndpoint::Pipe(format!(r"\\.\pipe\kubuno-auth-test-{name}-{}", std::process::id()))
        }
        #[cfg(unix)]
        {
            BrokerEndpoint::Socket(dir.join(format!("{name}.sock")))
        }
    }

    /// The textual form (pipe name or socket path), to pass to a child process.
    pub fn to_arg(&self) -> String {
        match self {
            BrokerEndpoint::Pipe(p) => p.clone(),
            BrokerEndpoint::Socket(p) => p.display().to_string(),
        }
    }

    pub fn from_arg(s: &str) -> Self {
        if s.starts_with(r"\\.\pipe\") {
            BrokerEndpoint::Pipe(s.to_string())
        } else {
            BrokerEndpoint::Socket(PathBuf::from(s))
        }
    }
}

#[cfg(windows)]
pub(crate) mod sys {
    //! Win32 pieces of the named pipe transport.

    use std::io;
    use std::path::PathBuf;

    use windows_sys::Win32::Foundation::{CloseHandle, LocalFree, HANDLE};
    use windows_sys::Win32::Security::Authorization::ConvertSidToStringSidW;
    use windows_sys::Win32::Security::{GetTokenInformation, TokenUser, TOKEN_QUERY, TOKEN_USER};
    use windows_sys::Win32::System::Threading::{
        GetCurrentProcess, OpenProcess, OpenProcessToken, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };
    #[cfg(feature = "server")]
    use {
        windows_sys::Win32::Security::Authorization::{ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1},
        windows_sys::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES},
        windows_sys::Win32::System::Pipes::GetNamedPipeClientProcessId,
    };

    #[cfg(feature = "server")]
    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// The user SID (string form) of the process whose handle is `process`.
    fn process_user_sid(process: HANDLE) -> io::Result<String> {
        let mut token: HANDLE = std::ptr::null_mut();
        // SAFETY: `process` is a valid process handle (or the current-process pseudo-handle); `token` receives a
        // handle closed below.
        if unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let result = (|| {
            let mut len = 0u32;
            // SAFETY: size query with a null buffer, as documented.
            unsafe { GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &mut len) };
            if len == 0 {
                return Err(io::Error::last_os_error());
            }
            // u64 elements keep the buffer aligned for TOKEN_USER.
            let mut buf = vec![0u64; (len as usize).div_ceil(8)];
            // SAFETY: `buf` holds at least `len` bytes.
            if unsafe { GetTokenInformation(token, TokenUser, buf.as_mut_ptr().cast(), len, &mut len) } == 0 {
                return Err(io::Error::last_os_error());
            }
            // SAFETY: GetTokenInformation(TokenUser) filled a TOKEN_USER at the start of `buf`.
            let sid = unsafe { (*(buf.as_ptr() as *const TOKEN_USER)).User.Sid };
            let mut text: *mut u16 = std::ptr::null_mut();
            // SAFETY: `sid` points into `buf`, alive for the call; `text` is LocalAlloc'ed by the API.
            if unsafe { ConvertSidToStringSidW(sid, &mut text) } == 0 || text.is_null() {
                return Err(io::Error::last_os_error());
            }
            // SAFETY: `text` is a NUL-terminated UTF-16 string returned by the API, freed right after.
            let s = unsafe {
                let mut n = 0usize;
                while *text.add(n) != 0 {
                    n += 1;
                }
                let s = String::from_utf16_lossy(std::slice::from_raw_parts(text, n));
                LocalFree(text.cast());
                s
            };
            Ok(s)
        })();
        // SAFETY: `token` was opened above.
        unsafe { CloseHandle(token) };
        result
    }

    /// The current user's SID in string form (`S-1-5-21-…`).
    pub(crate) fn current_user_sid() -> io::Result<String> {
        // SAFETY: the pseudo-handle of the current process needs no closing.
        process_user_sid(unsafe { GetCurrentProcess() })
    }

    /// What can be learnt about another process: its image path and its user.
    #[cfg(any(feature = "server", feature = "client"))]
    #[derive(Debug, Clone, Default)]
    pub(crate) struct ProcessInfo {
        pub(crate) image: Option<PathBuf>,
        #[cfg_attr(not(feature = "client"), allow(dead_code))]
        pub(crate) user_sid: Option<String>,
    }

    /// The image path and the user of process `pid`, as far as they can be queried (another user's process
    /// usually refuses the token query: `user_sid` is then `None`).
    #[cfg(any(feature = "server", feature = "client"))]
    pub(crate) fn process_info(pid: u32) -> ProcessInfo {
        // SAFETY: plain handle query; closed below.
        let h = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if h.is_null() {
            return ProcessInfo::default();
        }
        let mut buf = vec![0u16; 32768];
        let mut len = buf.len() as u32;
        // SAFETY: `buf` holds `len` UTF-16 units.
        let ok = unsafe { QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, buf.as_mut_ptr(), &mut len) };
        let image = (ok != 0).then(|| PathBuf::from(String::from_utf16_lossy(&buf[..len as usize])));
        let user_sid = process_user_sid(h).ok();
        // SAFETY: `h` was opened above.
        unsafe { CloseHandle(h) };
        ProcessInfo { image, user_sid }
    }

    /// The full image path of a process, when it can be queried.
    #[cfg(feature = "server")]
    pub(crate) fn process_image(pid: u32) -> Option<PathBuf> {
        process_info(pid).image
    }

    /// The process id of the server of a connected client pipe handle.
    #[cfg(feature = "client")]
    pub(crate) fn pipe_server_pid(handle: HANDLE) -> io::Result<u32> {
        use windows_sys::Win32::System::Pipes::GetNamedPipeServerProcessId;
        let mut pid = 0u32;
        // SAFETY: `handle` is a connected client end of a named pipe owned by the caller.
        if unsafe { GetNamedPipeServerProcessId(handle, &mut pid) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(pid)
    }

    /// A security descriptor granting full access to the current user only (no inheritance from the default DACL,
    /// so neither Administrators nor other users of a terminal server).
    #[cfg(feature = "server")]
    pub(crate) struct UserOnlySecurity {
        descriptor: PSECURITY_DESCRIPTOR,
        attributes: SECURITY_ATTRIBUTES,
    }

    // SAFETY: the descriptor is an immutable LocalAlloc'ed block only read by the pipe API.
    #[cfg(feature = "server")]
    unsafe impl Send for UserOnlySecurity {}
    // SAFETY: as above.
    #[cfg(feature = "server")]
    unsafe impl Sync for UserOnlySecurity {}

    #[cfg(feature = "server")]
    impl UserOnlySecurity {
        pub(crate) fn new() -> io::Result<Self> {
            let sddl = wide(&format!("D:P(A;;GA;;;{})", current_user_sid()?));
            let mut descriptor: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
            // SAFETY: `sddl` is NUL-terminated; `descriptor` receives a LocalAlloc'ed block freed in Drop.
            let ok = unsafe {
                ConvertStringSecurityDescriptorToSecurityDescriptorW(sddl.as_ptr(), SDDL_REVISION_1, &mut descriptor, std::ptr::null_mut())
            };
            if ok == 0 || descriptor.is_null() {
                return Err(io::Error::last_os_error());
            }
            let attributes = SECURITY_ATTRIBUTES {
                nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
                lpSecurityDescriptor: descriptor,
                bInheritHandle: 0,
            };
            Ok(Self { descriptor, attributes })
        }

        /// The `SECURITY_ATTRIBUTES` to pass to `CreateNamedPipeW`, valid while `self` lives.
        pub(crate) fn as_ptr(&self) -> *mut std::ffi::c_void {
            (&self.attributes as *const SECURITY_ATTRIBUTES).cast_mut().cast()
        }
    }

    #[cfg(feature = "server")]
    impl Drop for UserOnlySecurity {
        fn drop(&mut self) {
            // SAFETY: allocated by ConvertStringSecurityDescriptorToSecurityDescriptorW.
            unsafe { LocalFree(self.descriptor) };
        }
    }

    /// The process id of the client connected to a server pipe handle.
    #[cfg(feature = "server")]
    pub(crate) fn pipe_client_pid(handle: HANDLE) -> io::Result<u32> {
        let mut pid = 0u32;
        // SAFETY: `handle` is a connected server end of a named pipe owned by the caller.
        if unsafe { GetNamedPipeClientProcessId(handle, &mut pid) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(pid)
    }
}

#[cfg(all(unix, any(feature = "server", feature = "client")))]
pub(crate) mod sys {
    //! Unix pieces of the socket transport.

    use std::path::PathBuf;

    pub(crate) fn effective_uid() -> u32 {
        // SAFETY: geteuid has no preconditions and cannot fail.
        unsafe { libc::geteuid() }
    }

    /// The executable of a process, when it can be found.
    pub(crate) fn process_image(pid: i32) -> Option<PathBuf> {
        #[cfg(target_os = "linux")]
        {
            std::fs::read_link(format!("/proc/{pid}/exe")).ok()
        }
        #[cfg(target_os = "macos")]
        {
            let mut buf = vec![0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
            // SAFETY: `buf` holds PROC_PIDPATHINFO_MAXSIZE bytes, the size passed.
            let n = unsafe { libc::proc_pidpath(pid, buf.as_mut_ptr().cast(), buf.len() as u32) };
            if n <= 0 {
                return None;
            }
            buf.truncate(n as usize);
            String::from_utf8(buf).ok().map(PathBuf::from)
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            let _ = pid;
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "client")]
    #[test]
    fn server_policy_matches_exact_images_only() {
        let dir = tempfile::tempdir().expect("tmp");
        let shell = dir.path().join("kubuno-desktop.exe");
        std::fs::write(&shell, b"x").expect("write");
        let other = dir.path().join("squatter.exe");
        std::fs::write(&other, b"x").expect("write");
        let policy = ServerPolicy::Images(vec![shell.clone()]);
        assert!(policy.allows_image(Some(&shell)));
        assert!(!policy.allows_image(Some(&other)), "another program of the same directory is refused");
        assert!(!policy.allows_image(None), "an unknown image is refused");
        assert!(ServerPolicy::SameUser.allows_image(None));
        let under = ServerPolicy::ImagesUnder(vec![dir.path().to_path_buf()]);
        assert!(under.allows_image(Some(&other)));
        assert!(!under.allows_image(Some(dir.path())), "the directory itself is not an image under it");
    }
}
