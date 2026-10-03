//! Port of `Files.App/Actions/Start/` (`PinToStartAction`,
//! `UnpinFromStartAction`).
//!
//! DEVIATION: the original relies on `IStartMenuService`
//! (`Services/StartMenuService.cs`), which creates/removes a `SecondaryTile`
//! (`Windows.UI.StartScreen`). This WinRT API requires a **package identity**
//! (`SecondaryTile.RequestCreateAsync` otherwise throws `COMException`). The
//! Win32 port isn't packaged, so it cannot reproduce it: the faithful logic
//! (`tileId` computation) is ported, the WinRT call is documented as
//! non-reproducible.

// NOT WIRED YET. Pin/Unpin to Start: the WinRT SecondaryTile call needs a package identity (see above), so these wait for the packaged (MSIX) build.
// The allow is scoped to this file and must go once it is wired.
#![allow(dead_code, unused_imports)]

use crate::main_window::MainWindow;
use crate::view_models::shell_view_model::Location;

pub mod pin_to_start_action;
pub mod unpin_from_start_action;

pub use pin_to_start_action::PinToStart;
pub use unpin_from_start_action::UnpinFromStart;

/// `StartMenuService.GetTileId`: "Remove symbols because windows doesn't like
/// them in the ID"; truncates to every other character beyond 64.
pub(super) fn native_tile_id(id: &str) -> String {
    let mut str: String =
        format!("folder-{}", id.chars().filter(|c| c.is_alphanumeric()).collect::<String>());
    if str.len() > 64 {
        str = str.chars().enumerate().filter(|(i, _)| i % 2 == 0).map(|(_, c)| c).collect();
    }
    str
}

/// Pin targets: the selection if any, otherwise the current folder
/// (`CurrentFolder`), like `PinToStartAction`/`UnpinFromStartAction`. Returns
/// (path, display name) pairs.
pub(super) fn pin_targets(w: &MainWindow) -> Vec<(String, String)> {
    let tab = w.state.active();
    let sel = tab.selected_paths();
    if !sel.is_empty() {
        return sel
            .into_iter()
            .map(|p| {
                let name = std::path::Path::new(&p)
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or(&p)
                    .to_owned();
                (p, name)
            })
            .collect();
    }
    if let Location::Dir(dir) = &tab.location {
        let path = dir.to_string_lossy().into_owned();
        let name = dir
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(&path)
            .to_owned();
        return vec![(path, name)];
    }
    Vec::new()
}
