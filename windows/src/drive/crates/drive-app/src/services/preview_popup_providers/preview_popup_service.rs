//! PreviewPopupService (mirror of `PreviewPopupService.cs` + `IPreviewPopupProvider`).
//!
//! `GetProviderAsync()` keeps the FIRST available provider: QuickLook
//! takes priority, Seer as fallback, no-op if none (no internal preview, like
//! the original).

#![allow(dead_code)]

use super::quick_look_provider::{quicklook_available, quicklook_send, MSG_SWITCH, MSG_TOGGLE};
use super::seer_pro_provider::{seer_available, seer_switch, seer_toggle};

/// `true` if an external preview provider is available (QuickLook or Seer).
/// Equivalent to `GetProviderAsync() is not null` — used to gray out/enable
/// the preview command and decide whether to relay the keyboard.
pub fn is_available() -> bool {
    quicklook_available() || seer_available()
}

/// Toggles (opens/closes) the quick preview for `path`.
///
/// `LaunchPreviewPopupAction.ExecuteAsync`: first available provider →
/// `TogglePreviewPopupAsync`. QuickLook takes priority, Seer as fallback, no-op
/// if none (no internal preview, like the original).
pub fn toggle_preview(path: &str) {
    if quicklook_available() {
        quicklook_send(MSG_TOGGLE, path);
    } else if seer_available() {
        seer_toggle(path);
    }
    // Otherwise: nothing (no provider installed).
}

/// Updates the already-open preview when the selection changes (arrow keys).
///
/// `LaunchPreviewPopupAction.SwitchPopupPreviewAsync`: first provider →
/// `SwitchPreviewAsync`. Only call this when a popup is open.
pub fn switch_preview(path: &str) {
    if quicklook_available() {
        quicklook_send(MSG_SWITCH, path);
    } else if seer_available() {
        seer_switch(path);
    }
}
