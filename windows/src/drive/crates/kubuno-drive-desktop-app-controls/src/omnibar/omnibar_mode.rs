//! `OmnibarMode` (mirror of `Files.App.Controls/Omnibar/OmnibarMode.cs`).
//! Also holds the PORTABLE dimensions of the mode button (constants from
//! `Omnibar.xaml`, describing each mode cell in the row).

// ── Mode button (Omnibar.xaml) ──────────────────────────────────────────────
/// `OmnibarModeDefaultHeight`.
pub const MODE_HEIGHT: f32 = 34.0;
/// `OmnibarModeDefaultClickAreaWidth`.
pub const MODE_CLICK_AREA_WIDTH: f32 = 46.0;
/// `OmnibarModeDefaultCornerRadius` (mode pill).
pub const MODE_CORNER_RADIUS: f32 = 17.0;
/// `PART_ModeButton` `Margin="1"`.
pub const MODE_BUTTON_MARGIN: f32 = 1.0;
/// A mode's cell in the row: click area + margins.
pub const MODE_SLOT_WIDTH: f32 = MODE_CLICK_AREA_WIDTH + 2.0 * MODE_BUTTON_MARGIN; // 48

/// The Omnibar's neutral mode. On the C# side this isn't an enum but a
/// `Modes` collection of `OmnibarMode` instances; this is Files' canonical
/// order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OmnibarMode {
    /// Breadcrumb / path editing (default mode, `IsDefault`).
    Path,
    /// Command palette.
    CommandPalette,
    /// Search.
    Search,
}

impl OmnibarMode {
    /// Left-to-right display order in the mode row.
    pub const ALL: [OmnibarMode; 3] =
        [OmnibarMode::Path, OmnibarMode::CommandPalette, OmnibarMode::Search];

    pub fn index(self) -> usize {
        match self {
            OmnibarMode::Path => 0,
            OmnibarMode::CommandPalette => 1,
            OmnibarMode::Search => 2,
        }
    }
}
