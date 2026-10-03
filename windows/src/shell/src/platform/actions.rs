//! What the shell opens outside itself: a URL in the user's browser — where every Kubuno module
//! lives, the shell hosting no web view at all — and a folder in Explorer.
//!
//! The Tauri build routed every action through an IPC command invoked from JavaScript; the window
//! (`shell_window`) now calls the engine directly, and these two are what is left of the shell's
//! own actions.

/// Opens a URL with the user's default browser.
pub fn open_in_browser(url: &str) {
    open_path(url);
}

/// Opens `target` (a URL, a folder) with its default handler.
pub fn open_path(target: &str) {
    use windows::core::HSTRING;
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    if target.is_empty() || crate::services::backend::is_sample() || !kubuno_account::paths::system_integration_allowed() {
        // The sample opens nothing: its server and folders do not exist. Nor does a sandboxed run
        // (`KUBUNO_SANDBOX_DIR`: tests, captures), which must never reach the user's browser.
        kubuno::tracing::info!("[shell] open {target}");
        return;
    }
    let verb = HSTRING::from("open");
    let file = HSTRING::from(target);
    // SAFETY: the strings outlive the call; ShellExecuteW has no other requirement.
    unsafe {
        ShellExecuteW(
            None,
            windows::core::PCWSTR(verb.as_ptr()),
            windows::core::PCWSTR(file.as_ptr()),
            windows::core::PCWSTR::null(),
            windows::core::PCWSTR::null(),
            SW_SHOWNORMAL,
        );
    }
}
