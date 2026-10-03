//! PinToStart (mirrors PinToStartAction.cs)

// NOT WIRED YET. See actions::start: waits for the packaged (MSIX) build.
// The allow is scoped to this file and must go once it is wired.
#![allow(dead_code)]

use crate::actions::Action;
use crate::main_window::MainWindow;

use super::{native_tile_id, pin_targets};

/// `PinToStartAction.cs`.
///
/// - `Label`: `Strings.PinItemToStart_Text` (.resw `PinItemToStart.Text`) →
///   "Pin to the Start Menu".
/// - `Description`: `Strings.PinToStartDescription`.
/// - `Glyph`: `themedIconStyle: "App.ThemedIcons.FavoritePin"`.
/// - `IsExecutable`: `context.ShellPage is not null` (true as soon as a tab is
///   open on a folder — we require at least one pinnable target).
///
/// DEVIATION: `ExecuteAsync` calls `StartMenuService.PinAsync(storable, name)`,
/// which creates a WinRT `SecondaryTile` (package identity required). Not
/// reproducible in the unpackaged Win32 port: the faithful `tileId` is
/// computed but the actual pinning isn't performed (see module header).
pub struct PinToStart;
impl Action for PinToStart {
    fn label(&self) -> &'static str {
        "PinItemToStart.Text"
    }
    fn description(&self) -> &'static str {
        "PinToStartDescription"
    }
    fn glyph(&self) -> Option<&'static str> {
        Some("FavoritePin")
    }
    fn is_executable(&self, w: &MainWindow) -> bool {
        !pin_targets(w).is_empty()
    }
    fn execute(&self, w: &mut MainWindow, _parameter: Option<&str>) {
        for (path, _name) in pin_targets(w) {
            // `StartMenuService.PinAsync(storable, displayName)`: creates a
            // SecondaryTile with id `native_tile_id(path)`. WinRT API
            // `Windows.UI.StartScreen` unavailable without a package
            // identity — pinning not performed.
            let _tile_id = native_tile_id(&path);
        }
    }
}
