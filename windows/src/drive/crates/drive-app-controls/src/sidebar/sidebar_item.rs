//! `SidebarItem` (mirrors `Files.App.Controls/Sidebar/SidebarItem.cs` +
//! `SidebarItem.Properties.cs` + `SidebarStyles.xaml`).
//!
//! The WinUI control (VisualStateManager, template children, event wiring)
//! is IGNORED machinery. This file houses the row's PORTABLE content:
//! - row geometry/dimensions (extracted from `SidebarStyles.xaml`);
//! - the drop position algorithm (`DetermineDropTargetPosition`);
//! - per-level indentation (`OnNestingLevelChanged`);
//! - the port's rendering primitives (`RowIcon`, `SidebarRowView`), invented
//!   by the port (WinUI renders via the DataTemplate) but kept at row level.

use crate::geometry::Rect;
use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;
use windows::Win32::Graphics::Direct2D::ID2D1Bitmap1;

use super::sidebar_item_drop_position::SidebarItemDropPosition;

// Row dimensions — Kubuno design system (web sidebar nav row), no longer
// `SidebarStyles.xaml`.
/// Height of a nav row (`shape::height::SIDEBAR_ROW`).
pub const SIDEBAR_ROW_HEIGHT: f32 = crate::themes::shape::height::SIDEBAR_ROW;
/// The row background is a full pill (web `rounded-full`): the radius is half
/// the height, i.e. `shape::pill(SIDEBAR_ROW_HEIGHT)`. Kept as a constant here
/// because `pill()` is not `const`.
pub const SIDEBAR_ROW_CORNER: f32 = SIDEBAR_ROW_HEIGHT / 2.0;

/// `SidebarItem.Properties.cs` `OnNestingLevelChanged`: `IndentWidth = level * 16d`.
pub const INDENT_PER_LEVEL: f32 = 16.0;

/// `SidebarItem.DROP_REPOSITION_THRESHOLD`: fraction of the top/bottom beyond
/// which a drop is considered a reinsertion (instead of a child).
pub const DROP_REPOSITION_THRESHOLD: f64 = 0.2;

/// `DetermineDropTargetPosition`: `Y < h*0.2` → Top, `Y > h*(1-0.2)` → Bottom,
/// else Center. PURE: receives the position and height measured by the caller.
pub fn determine_drop_target_position(y: f64, height: f64) -> SidebarItemDropPosition {
    if y < height * DROP_REPOSITION_THRESHOLD {
        SidebarItemDropPosition::Top
    } else if y > height * (1.0 - DROP_REPOSITION_THRESHOLD) {
        SidebarItemDropPosition::Bottom
    } else {
        SidebarItemDropPosition::Center
    }
}

// Components of a row's natural width (web nav row: 12px horizontal padding,
// 12px gap between the icon and its label).
const ROW_LEFT_PAD: f32 = 8.0;
/// Icon box of a nav row.
pub const ROW_ICON_SIZE: f32 = 20.0;
/// Gap between icon and label (`space::MD`).
pub const ROW_ICON_TEXT_GAP: f32 = crate::themes::shape::space::MD;
/// Right padding of the label (`space::MD`).
pub const ROW_TEXT_RIGHT_MARGIN: f32 = crate::themes::shape::space::MD;

/// A row's natural width (icon + label, no truncation), to decide whether
/// the horizontal bar should appear. PURE: `text_width` is measured by the
/// caller (`approx_text_width` stays in drive-app).
pub fn sidebar_row_width(indent: f32, text_width: f32) -> f32 {
    ROW_LEFT_PAD + indent + ROW_ICON_SIZE + ROW_ICON_TEXT_GAP + text_width + ROW_TEXT_RIGHT_MARGIN
}

/// A row's icon, ALREADY resolved by the application (`shell_icon` contract).
pub enum RowIcon<'a> {
    /// Shell bitmap / imageres / embedded PNG, already resolved.
    Bitmap(&'a ID2D1Bitmap1),
    /// Tinted monochrome ThemedIcon geometry (a tag's `FilledTag`).
    Vector { name: &'static str, color: D2D1_COLOR_F },
    /// Segoe glyph fallback (when no bitmap could be resolved).
    Glyph { text: &'static str, color: D2D1_COLOR_F },
}

/// A sidebar row, in primitives.
pub struct SidebarRowView<'a> {
    pub rect: Rect,
    pub indent: f32,
    pub label: String,
    pub is_section: bool,
    pub selected: bool,
    pub hot: bool,
    /// `Some(expanded)` if the row carries a collapse chevron; `None` otherwise.
    pub chevron: Option<bool>,
    pub icon: RowIcon<'a>,
}
