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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hot {
    CaptionMin,
    CaptionMax,
    CaptionClose,
    Tab(usize),
    TabClose(usize),
    NewTab,
    PaneToggle,
    /// SidebarPaneToggleButton: shown left of Back when the sidebar is closed.
    Hamburger,
    /// The Minimal pane's light-dismiss layer: a click closes it.
    SidebarLightDismiss,
    /// One of the Omnibar mode buttons: 0 = path, 1 = palette, 2 = search.
    OmnibarMode(usize),
    /// ShowStatusCenterButton (right of the omnibar).
    StatusCenter,
    CmdNew,
    /// `AppBarSeparator` after « Nouveau » (ToolbarSections: AlwaysVisible).
    CmdSeparator,
    CmdCut,
    CmdCopy,
    CmdPaste,
    CmdRename,
    /// `ShareItem` (AlwaysVisible context, icon only).
    CmdShare,
    CmdDelete,
    /// `OpenProperties` (AlwaysVisible context, icon only).
    CmdProperties,
    /// Recycle Bin — the « RecycleBin » context from `ToolbarSections.cs:51-56`:
    /// the three labeled AppBarButtons (`showLabel: true`) that REPLACE the
    /// « AlwaysVisible » block (mapping in `ToolbarItemDescriptor.cs:184`).
    CmdEmptyRecycleBin,
    CmdRestoreAllRecycleBin,
    CmdRestoreRecycleBin,
    CmdFilter,
    CmdSelOptions,
    CmdSort,
    /// LayoutOptionsButton (layout picker flyout).
    CmdLayout,
    CmdInfoPane,
    /// `ShelfPaneToggleButton`: the Shelf pane toggle (command bar).
    CmdShelf,
    /// A Shelf item row (`ShelfItemsList`), by index.
    ShelfItem(usize),
    /// The × button to remove a Shelf item (`ShelfItem.Remove`).
    ShelfItemRemove(usize),
    /// The « Effacer les éléments » link at the bottom of the Shelf (`ClearItemsCommand`).
    ShelfClear,
    /// A row of the open omnibar suggestion panel.
    Suggestion(usize),
    /// Anywhere else inside that panel. It exists so the panel SWALLOWS the
    /// pointer: without it, hovering the panel lit up whatever sits beneath
    /// (toolbar buttons, file rows) and clicking went straight through.
    SuggestionPanel,
    /// `SidebarResizer`: the 4 DIP handle on the sidebar's right edge.
    SidebarResizer,
    /// `InfoPaneSizer`: the 2 DIP `GridSplitter` to the left of the pane.
    InfoPaneResizer,
    InfoProperties,
    /// Status bar, git widget: the network actions button (ahead/behind
    /// counter + Pull/Push/Sync flyout) and the branch selector.
    StatusGitActions,
    StatusGitBranch,
    /// Détails (0) / Aperçu (1) selector in the info pane.
    InfoTab(usize),
    NavBack,
    NavForward,
    NavUp,
    NavRefresh,
    Breadcrumb(usize),
    /// `BreadcrumbBar`: the ellipsis (…) button that collapses the leading
    /// segments when the path overflows; a click opens their flyout.
    BreadcrumbEllipsis,
    AddressBar,
    SidebarItem(usize),
    QuickCard(usize),
    DriveCard(usize),
    RecentRow(usize),
    FileRow(usize),
    /// The `ScrollBar`'s thumb.
    ScrollThumb,
    /// The track: a click there pages (the `ScrollBar`'s `RepeatButton`).
    ScrollTrack,
    /// The SIDEBAR's ScrollBar (thumb, track).
    SidebarScrollThumb,
    SidebarScrollTrack,
    /// The sidebar's HORIZONTAL ScrollBar (`HorizontalScrollMode=Enabled`).
    SidebarHScrollThumb,
    SidebarHScrollTrack,
    /// The `ContentDialog`'s buttons.
    DialogPrimary,
    DialogClose,
    /// A dialog choice pill (Format zip/7z…).
    DialogChoice(usize),
    DialogCheckbox,
    DialogItem(usize),
    /// The conflict dialog: « apply to all » and an item's option.
    ConflictApplyAll,
    ConflictOption(usize),
    /// A Grid/Cards item's `SelectionCheckbox`.
    FileCheckbox(usize),
    /// Columns layout: (blade, row).
    ColumnRow(usize, usize),
    FileHeaderCol(usize),
    SettingRow(usize),
    SettingsNav(usize),
    /// One of the 20 background-color swatches (AppearancePage GridView).
    ThemeSwatch(usize),
    /// The unfocused pane in dual-pane mode; clicking focuses it.
    InactivePane,
    /// The `GridSplitter` between the two panes (ported from `ShellPanesPage`):
    /// drag it to resize, double-click to equalize.
    PaneDivider,
}

/// Port of the chrome's `ToolTipService.ToolTip` (navigation bar, command
/// bar, status bar). Two forms in the original: `Commands.X.LabelWithHotKey`
/// — the label followed by « (shortcut) » — or a plain `{helpers:ResourceString}`.
///
/// Returns `None` for elements without a tooltip (text, file rows, etc.:
/// the original only puts one on icon-only buttons).
pub fn tooltip_for(hot: Hot) -> Option<String> {
    use crate::actions::by_name;
    use kubuno_drive_desktop_localization::tr;

    // `LabelWithHotKey`: « Libellé (Raccourci) » if the action has a HotKey,
    // otherwise the label alone — exactly the original's `Command.LabelWithHotKey`.
    fn cmd(name: &'static str) -> Option<String> {
        let a = by_name(name)?;
        let label = tr(a.label());
        Some(match a.hotkey() {
            Some(h) => format!("{label} ({})", h.text()),
            None => label.to_string(),
        })
    }

    match hot {
        // NavigationToolbar.xaml
        Hot::Hamburger => Some(tr("ToggleSidebar").to_string()),
        Hot::NavBack => cmd("NavigateBack"),
        Hot::NavForward => cmd("NavigateForward"),
        Hot::NavUp => cmd("NavigateUp"),
        Hot::NavRefresh => cmd("RefreshItems"),
        Hot::StatusCenter => Some(tr("StatusCenter").to_string()),
        // Toolbar.xaml (the command bar)
        Hot::CmdNew => cmd("AddItem"),
        Hot::CmdCut => cmd("CutItem"),
        Hot::CmdCopy => cmd("CopyItem"),
        Hot::CmdPaste => cmd("PasteItem"),
        Hot::CmdRename => cmd("Rename"),
        Hot::CmdShare => cmd("ShareItem"),
        Hot::CmdDelete => cmd("DeleteItem"),
        Hot::CmdProperties => cmd("OpenProperties"),
        Hot::CmdFilter => Some(tr("ToggleFilterHeader").to_string()),
        Hot::CmdSelOptions => Some(tr("SelectionOptions").to_string()),
        Hot::CmdSort => Some(tr("Sort").to_string()),
        Hot::CmdLayout => Some(tr("Layout").to_string()),
        Hot::CmdInfoPane => cmd("ToggleInfoPane"),
        Hot::CmdShelf => cmd("ToggleShelfPane"),
        // StatusBar.xaml
        Hot::StatusGitBranch => Some(tr("ManageBranches").to_string()),
        _ => None,
    }
}

/// Column boundaries of the details view: (name | modified | type | size).
/// Narrow panes (dual-pane mode) collapse the secondary columns so the name
/// keeps room, like the original responsive DetailsLayout.
pub fn file_columns(header: &Rect) -> [f32; 3] {
    file_columns_for(header, false)
}

/// Recycle-Bin-aware variant: the « Chemin d'origine » column (the
/// « modified » slot) widens for full paths.
pub fn file_columns_for(header: &Rect, recycle: bool) -> [f32; 3] {
    let width = header.right - header.left;
    if width < 420.0 {
        return [header.right; 3];
    }
    if width < 560.0 {
        let modified_left = header.right - 140.0;
        return [modified_left, header.right, header.right];
    }
    let size_left = header.right - 100.0;
    let type_left = size_left - 150.0;
    let modified_left = if recycle {
        // Original path: up to 320 DIP, keeping ≥ 220 for the name.
        (type_left - 320.0).max(header.left + 220.0)
    } else {
        type_left - 140.0
    };
    [modified_left, type_left, size_left]
}

/// Sections of the Settings page (mirrors `SettingsPageViewModel`):
/// (localization key, ThemedIcon name from `Icons.Settings.Sidebar.xaml`).
pub const SETTINGS_SECTIONS: [&str; 9] = [
    "General",
    "Appearance",
    "Layout",
    "FilesAndFolders",
    "Actions",
    "FileTags",
    "DevTools",
    "Advanced",
    "About",
];

/// `App.ThemedIcons.Settings.*` icon of each section, same order.
pub const SETTINGS_SECTION_ICONS: [&str; 9] = [
    "SettingsGeneral",
    "SettingsAppearance",
    "SettingsLayout",
    "SettingsFilesFolders",
    "SettingsKeyboardActions",
    "SettingsTags",
    "SettingsDevTools",
    "SettingsAdvanced",
    "SettingsAbout",
];

