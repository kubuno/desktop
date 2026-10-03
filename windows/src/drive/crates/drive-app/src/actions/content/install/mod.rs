//! Port of `Files.App/Actions/Content/Install/` (`InstallFontAction`,
//! `InstallCertificateAction`, `InstallInfDriverAction`).
//!
//! (`Files.App/Actions/Start/` — Pin/Unpin from the Start menu — now lives
//! in its own mirror `crate::actions::start`.)

// NOT WIRED YET. Ported install actions (font, certificate, INF driver); wait for their registration in the action registry and the context menu.
// The allow is scoped to this file and must go once it is wired.
#![allow(dead_code, unused_imports)]

pub mod install_certificate_action;
pub mod install_font_action;
pub mod install_inf_driver_action;

pub use install_certificate_action::InstallCertificate;
pub use install_font_action::InstallFont;
pub use install_inf_driver_action::InstallInfDriver;

use crate::main_window::MainWindow;
use crate::view_models::shell_view_model::Location;

/// The selection's paths (context-menu `parameter`, otherwise the current
/// selection), like `IContentPageContext.SelectedItems`.
pub(super) fn selected(w: &MainWindow, parameter: Option<&str>) -> Vec<String> {
    match parameter {
        Some(p) => vec![p.to_owned()],
        None => w.state.active().selected_paths(),
    }
}

/// True outside the Recycle Bin (the port has no separate "zip folder"
/// location, whereas the C# also excludes `ContentPageTypes.ZipFolder`).
pub(super) fn not_recycle_bin(w: &MainWindow) -> bool {
    w.state.active().location != Location::RecycleBin
}
