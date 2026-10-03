//! UnpinFromStart (mirrors UnpinFromStartAction.cs)

// NOT WIRED YET. See actions::start: waits for the packaged (MSIX) build.
// The allow is scoped to this file and must go once it is wired.
#![allow(dead_code)]

use crate::actions::Action;
use crate::main_window::MainWindow;

use super::{native_tile_id, pin_targets};

/// `UnpinFromStartAction.cs`.
///
/// - `Label`: `Strings.UnpinItemFromStart_Text` (.resw `UnpinItemFromStart.Text`)
///   → "Unpin from the Start Menu".
/// - `Description`: `Strings.UnpinFromStartDescription`.
/// - `Glyph`: `themedIconStyle: "App.ThemedIcons.FavoritePinRemove"`.
/// - `IsExecutable`: not overridden in the C# (always executable); we require
///   at least one target, as with `PinToStart`.
///
/// Same deviation: `StartMenuService.UnpinAsync` calls
/// `StartScreenManager.GetDefault().TryRemoveSecondaryTileAsync(tileId)`
/// (WinRT `Windows.UI.StartScreen`), not reproducible here.
pub struct UnpinFromStart;
impl Action for UnpinFromStart {
    fn label(&self) -> &'static str {
        "UnpinItemFromStart.Text"
    }
    fn description(&self) -> &'static str {
        "UnpinFromStartDescription"
    }
    fn glyph(&self) -> Option<&'static str> {
        Some("FavoritePinRemove")
    }
    fn is_executable(&self, w: &MainWindow) -> bool {
        !pin_targets(w).is_empty()
    }
    fn execute(&self, w: &mut MainWindow, _parameter: Option<&str>) {
        for (path, _name) in pin_targets(w) {
            // `StartMenuService.UnpinAsync` → TryRemoveSecondaryTileAsync(tileId).
            // Not reproducible without a package identity.
            let _tile_id = native_tile_id(&path);
        }
    }
}
