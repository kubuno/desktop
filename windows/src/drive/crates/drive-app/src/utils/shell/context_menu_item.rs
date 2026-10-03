//! ContextMenuItem (mirrors ContextMenuItem.cs)
//!
//! One row of the enumerated shell menu (`Utils/Shell/ContextMenuItem.cs`),
//! as we redraw it in our own flyout.

/// One row of the shell menu, as we redraw it.
#[derive(Clone)]
pub struct ShellEntry {
    pub label: String,
    pub separator: bool,
    pub enabled: bool,
    /// The row's icon. Shell extensions hand it over as an `HBITMAP` on
    /// `MIIM_BITMAP`; we copy its pixels out right away, because the bitmap
    /// belongs to the menu and dies with it.
    pub bitmap: Option<std::sync::Arc<drive_app_storage::ShellBitmap>>,
    /// Command id, already offset from `CMD_FIRST`.
    pub(super) id: u32,
}
