//! Port of `Files.App/UserControls/Widgets/` (QuickAccessWidget.xaml,
//! DrivesWidget.xaml, RecentFilesWidget.xaml): the Home page sections —
//! quick access cards, drive cards with usage gauge, and recent files rows.
//!
//! `draw_home` orchestrates the Home page; each section is ported in its
//! own file, mirroring a distinct C# UserControl:
//! - `quick_access_widget` ↔ `QuickAccessWidget.xaml.cs`
//! - `drives_widget` ↔ `DrivesWidget.xaml.cs`
//! - `recent_files_widget` ↔ `RecentFilesWidget.xaml.cs`

pub mod quick_access_widget;
pub mod drives_widget;
pub mod recent_files_widget;

use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;

use crate::services::storage::IconCache;
use crate::data::items::HomeModel;
use crate::styles::theme::Theme;
use crate::ui::{Layout, Painter, UiState};

/// Fill colour of a storage gauge, from its fill ratio.
///
/// Thresholds and colours are the drive web's, not `@ui/ProgressBar.tsx`'s:
/// the shared component defaults to 75 % / 90 % (`ProgressBar.tsx:73`), but
/// every storage bar in drive overrides them to `> 90 %` danger, `> 70 %`
/// warning, else primary — see `FilesStorageGaugeHeader.tsx:21-24` and
/// `FilesTreeSidebar.tsx:109`. The web compares the ROUNDED percentage, so we
/// do too, otherwise 90.4 % would already read as red here and not there.
pub(crate) fn gauge_color(theme: &Theme, fraction: f32) -> D2D1_COLOR_F {
    let pct = (fraction * 100.0).round();
    if pct > 90.0 {
        theme.danger
    } else if pct > 70.0 {
        theme.warning
    } else {
        theme.accent
    }
}

impl Painter<'_> {
    pub(crate) fn draw_home(&self, layout: &Layout, state: &UiState, model: &HomeModel, icons: &IconCache, scale: f32) {
        self.draw_quick_access(layout, state, model, icons, scale);
        self.draw_drives(layout, state, model, icons, scale);
        self.draw_recent_files(layout, state, model, icons, scale);
    }
}
