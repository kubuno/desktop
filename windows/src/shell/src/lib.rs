//! Kubuno Desktop — the native shell, written like a Windows Forms application.
//!
//! It carries the sync engine, the Explorer integration and a launcher. The library holds
//! everything the Visual Studio designer links to render the views (the design build compiles
//! this crate): the main form [`ShellWindow`] (`views/shell_window.kbview` + `views/shell_window.rs`),
//! one user control per page ([`LauncherPage`], [`SettingsPage`], [`AccountsPage`], [`ActivityPage`],
//! [`LoginPage`], [`LabelsPage`]), their rows, the [`StatusPresenter`], the confirmation dialog and
//! the custom controls ([`StorageGauge`], [`StatusDot`]). `main.rs` only starts it.
//!
//! The sources are grouped by role (vskubuno `docs/DESKTOP-MIGRATION.md`, "Source layout"), each
//! view next to its code-behind of the same name:
//!
//! - `views/` — the top-level views (`.kbview`): the window, its flyouts and its dialogs;
//! - `pages/` — the window's pages (`.kbcontrol`) and the item templates their lists repeat;
//! - `admin/` — the administration console, a feature big enough for a folder of its own;
//! - `controls/` — the custom-drawn controls and the user controls several views place;
//! - `model/` — what the views show and raise, pure (`view_model`, `events`);
//! - `services/` — the work behind the views: `backend` (the sync engine, or the offline sample),
//!   `session`, `sync`, `apps`, `activity`, `settings`, `favorites`, `options`;
//! - `platform/` — the parts that make this a Windows sync client: `cloudfiles`, `explorer`,
//!   `tray`, the folder picker, the browser — which the views do not touch;
//! - `resources/` — the strings and icons (`resources.kbres`, `resources.fr.kbres`).
//!
//! There is no web view anywhere: a module opens in the user's browser.

pub mod admin;
pub mod controls;
pub mod model;
pub mod pages;
pub mod platform;
pub mod services;
pub mod views;

pub use controls::status_dot::StatusDot;
pub use controls::status_presenter::StatusPresenter;
pub use controls::storage_gauge::StorageGauge;
pub use pages::account_row::AccountRow;
pub use pages::accounts_page::AccountsPage;
pub use pages::activity_page::ActivityPage;
pub use pages::activity_row::ActivityRow;
pub use pages::label_row::LabelRow;
pub use pages::labels_page::LabelsPage;
pub use pages::launcher_page::LauncherPage;
pub use pages::login_page::LoginPage;
pub use pages::settings_page::SettingsPage;
pub use services::options::Options;
pub use views::confirm_dialog::ConfirmDialog;
pub use views::shell_window::{Page, ShellWindow};
pub use views::signout_dialog::SignOutDialog;

// `Resources::app_title()`, `Resources::nav_home()`… — the strings of `resources/resources.kbres`
// (neutral English) and `resources/resources.fr.kbres`, in the current UI culture; `{Res key}` in the views.
kubuno::resources!(pub Resources, "resources/resources.kbres");

/// The name the Run key and the tray use.
pub const APP_NAME: &str = "Kubuno";
/// Our own toast identity, so notifications read "Kubuno" and not "PowerShell".
pub const AUMID: &str = "com.kubuno.desktop";

/// Posted by the background sync thread once a cycle ends (`WM_APP + 2`).
pub const WM_SYNC_DONE: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 2;

/// Summary handed over by the sync thread — a message only carries integers, so the text is
/// parked here for the UI thread to pick up.
static LAST_SUMMARY: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());

/// Tells the window (`hwnd`) that a sync cycle ended, with its summary.
pub fn post_sync_done(hwnd: isize, summary: String) {
    use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
    if let Ok(mut slot) = LAST_SUMMARY.lock() {
        *slot = summary;
    }
    // SAFETY: posting to a window handle (possibly stale, which then fails harmlessly) has no
    // memory-safety requirement.
    unsafe {
        let _ = windows::Win32::UI::WindowsAndMessaging::PostMessageW(Some(HWND(hwnd as *mut _)), WM_SYNC_DONE, WPARAM(0), LPARAM(0));
    }
}

/// The summary the last [`post_sync_done`] parked.
pub fn take_summary() -> String {
    LAST_SUMMARY.lock().map(|s| s.clone()).unwrap_or_default()
}

/// The splash screen of this start (inert when there is none), for the steps that run once the
/// window exists.
static SPLASH: std::sync::OnceLock<kubuno::Splash> = std::sync::OnceLock::new();

/// Keeps the splash screen for [`splash_step`].
pub fn set_splash(splash: kubuno::Splash) {
    let _ = SPLASH.set(splash);
}

/// Shows `status` (and `progress`, `0..=1`) on the splash screen, if one is up.
pub fn splash_step(status: &str, progress: f32) {
    if let Some(splash) = SPLASH.get() {
        splash.step(status, progress);
    }
}
