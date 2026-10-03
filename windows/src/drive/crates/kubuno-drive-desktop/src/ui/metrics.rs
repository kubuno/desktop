#![allow(unused_imports)]
use windows::core::Result;
use windows::Win32::Graphics::Direct2D::Common::{D2D1_COLOR_F, D2D_RECT_F};
use windows::Win32::Graphics::Direct2D::{
    ID2D1Bitmap1, ID2D1DeviceContext, ID2D1SolidColorBrush, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
    D2D1_INTERPOLATION_MODE_LINEAR, D2D1_ROUNDED_RECT,
};
use windows::Win32::Graphics::DirectWrite::{
    IDWriteTextFormat, DWRITE_MEASURING_MODE_NATURAL, DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
    DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_LEADING,
};

use crate::graphics::Renderer;
use crate::services::storage::IconCache;
use crate::data::items::{HomeModel, QuickAccessKind};
use crate::view_models::shell_view_model::{Location, Tab, TabGroup};
use crate::styles::theme::Theme;
use super::*;

pub const ICON_ROW_DIP: f32 = 18.0;
pub const ICON_CARD_DIP: f32 = 40.0;

/// TabView Margin="0,10,0,0" + TabViewItemMinHeight (32) = 42.
pub const TAB_BAR_HEIGHT: f32 = 42.0;
pub const TOOLBAR_HEIGHT: f32 = 48.0;
/// Command bar (Nouveau, Couper, Copier…) shown on folder views.
/// `Toolbar.xaml`: the bar is ~48 DIP tall (AppBarButton height), with a
/// 4 DIP bottom margin (`Margin="0,0,0,4"`) before the file area.
pub const CMDBAR_HEIGHT: f32 = 52.0;
/// Height of an `AppBarButton` (the clickable track in the command bar).
pub const CMDBAR_BUTTON_HEIGHT: f32 = 36.0;
pub const STATUSBAR_HEIGHT: f32 = 32.0;
/// `InfoPane.xaml`: `MinWidth="90"`, and `InfoPaneSettingsService.VerticalSizePx`
/// won't accept less than 100.
pub const INFOPANE_MIN_WIDTH: f32 = 100.0;
/// `MainPage.xaml`: the content column has `MinWidth="208"` — it's what
/// bounds the pane's growth.
pub const CONTENT_MIN_WIDTH: f32 = 208.0;
/// `ShelfPane.xaml`: the Shelf pane has a FIXED width (`Grid Width="240"`).
pub const SHELF_PANE_WIDTH: f32 = 240.0;
/// `ShelfItemsList`: `ListViewItem MinHeight="36"`.
pub const SHELF_ROW_HEIGHT: f32 = 36.0;
// Sidebar thresholds, dimensions, and mode: ported to
// `kubuno-drive-desktop-app-controls::sidebar` (mirrors `SidebarView`). Re-exported here so
// that `crate::ui::SIDEBAR_*` / `crate::ui::SidebarMode` /
/// Width of a pane's resize rail (`w-3` in the web shell). Wider than the
/// 4 DIP blade resizer: it has to hold the grip pill that appears on hover.
pub const RESIZE_RAIL: f32 = 12.0;

// `crate::ui::RESIZER_WIDTH` paths stay unchanged for callers.
pub use kubuno_drive_desktop_app_controls::sidebar::{
    SidebarMode, RESIZER_WIDTH, SIDEBAR_ANIM_MS, SIDEBAR_COMPACT_MAX_WIDTH,
    SIDEBAR_COMPACT_WIDTH, SIDEBAR_MAX_WIDTH, SIDEBAR_MIN_WIDTH,
    SIDEBAR_MINIMAL_MAX_WINDOW, SIDEBAR_OPEN_PANE_LENGTH,
};

/// Current width of the info pane (`VerticalSizePx`).
pub fn infopane_width() -> f32 {
    crate::services::settings::get().info_pane_width.max(INFOPANE_MIN_WIDTH)
}

/// The current mode: injects the `sidebar_compact` setting preference into
/// the pure calculation of `kubuno-drive-desktop-app-controls::sidebar`.
pub fn sidebar_mode(window_width: f32) -> SidebarMode {
    kubuno_drive_desktop_app_controls::sidebar::sidebar_mode(
        window_width,
        crate::services::settings::get().sidebar_compact,
    )
}

/// The mode transitions' easing: the Bézier cubic `KeySpline="0.1,0.9
/// 0.2,1.0"` (brisk start, gentle deceleration). `t` is the progress [0,1].
pub fn sidebar_ease(t: f32) -> f32 {
    cubic_bezier_ease(0.1, 0.9, 0.2, 1.0, t)
}

/// A WinUI animation Bézier cubic (`KeySpline`): P0=(0,0), P3=(1,1),
/// P1=(x1,y1), P2=(x2,y2). Solves `x(u)=t` via Newton, then returns `y(u)`.
pub fn cubic_bezier_ease(x1: f32, y1: f32, x2: f32, y2: f32, t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    let bez = |p1: f32, p2: f32, u: f32| {
        let m = 1.0 - u;
        3.0 * m * m * u * p1 + 3.0 * m * u * u * p2 + u * u * u
    };
    let dbez = |p1: f32, p2: f32, u: f32| {
        let m = 1.0 - u;
        3.0 * m * m * p1 + 6.0 * m * u * (p2 - p1) + 3.0 * u * u * (1.0 - p2)
    };
    let mut u = t;
    for _ in 0..8 {
        let dx = dbez(x1, x2, u);
        if dx.abs() < 1e-5 {
            break;
        }
        u = (u - (bez(x1, x2, u) - t) / dx).clamp(0.0, 1.0);
    }
    bez(y1, y2, u)
}

/// Current sidebar width (`SidebarWidth`) — the 56 DIP rail in Compact
/// mode, like the SidebarView's `PaneColumnDefinition`.
pub fn sidebar_width() -> f32 {
    let s = crate::services::settings::get();
    let mode = if s.sidebar_compact { SidebarMode::Compact } else { SidebarMode::Expanded };
    kubuno_drive_desktop_app_controls::sidebar::sidebar_pane_width(mode, s.sidebar_width)
}
pub const CAPTION_BUTTON_WIDTH: f32 = 46.0;
/// Height of the Windows 11 caption buttons (min/max/close): 32 DIP, like
/// the original's system buttons — NOT the full title bar height
/// (`TAB_BAR_HEIGHT` = 42), otherwise they overflow into the content and
/// look too tall.
pub const CAPTION_BUTTON_HEIGHT: f32 = 32.0;
pub const FILE_ROW_HEIGHT: f32 = 34.0;
/// A group's header (`GroupSummary` from the layout pages).
pub const GROUP_HEADER_HEIGHT: f32 = 36.0;

// Segoe Fluent Icons glyphs. The navigation buttons (back/forward/up/refresh)
// and the pane toggle no longer live here: they are Material Symbols vector
// icons, drawn through `vector_icon`.
pub(crate) const GLYPH_HOME: &str = "\u{E80F}";
pub(crate) const GLYPH_ADD: &str = "\u{E710}";
pub(crate) const GLYPH_FOLDER: &str = "\u{E8B7}";
pub(crate) const GLYPH_STAR: &str = "\u{E735}";
pub(crate) const GLYPH_DRIVE: &str = "\u{EDA2}";
/// `Constants.ImageRes`: the `imageres.dll` indices for section headers.
pub const IMAGERES_THIS_PC: i32 = 109;
pub const IMAGERES_NETWORK: i32 = 25;

pub(crate) const GLYPH_SETTINGS: &str = "\u{E713}";
pub(crate) const GLYPH_PIN: &str = "\u{E840}";
pub(crate) const GLYPH_CHEVRON_UP: &str = "\u{E70E}";
pub(crate) const GLYPH_CHEVRON_RIGHT: &str = "\u{E76C}";
pub(crate) const GLYPH_RECYCLE: &str = "\u{E74D}";
pub(crate) const GLYPH_DOCUMENT: &str = "\u{E8A5}";
pub(crate) const GLYPH_DOWNLOAD: &str = "\u{E896}";
pub(crate) const GLYPH_PICTURE: &str = "\u{E91B}";
pub(crate) const GLYPH_MUSIC: &str = "\u{E8D6}";
pub(crate) const GLYPH_VIDEO: &str = "\u{E714}";
pub(crate) const GLYPH_DESKTOP: &str = "\u{E7F4}";
