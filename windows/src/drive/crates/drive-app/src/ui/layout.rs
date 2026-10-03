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

/// All hit-testable rectangles, recomputed per frame (DIPs).
pub struct Layout {
    pub width: f32,
    pub height: f32,
    pub caption_min: Rect,
    pub caption_max: Rect,
    pub caption_close: Rect,
    pub tabs: Vec<Rect>,
    pub new_tab: Rect,
    pub nav_back: Rect,
    pub nav_forward: Rect,
    pub nav_up: Rect,
    pub nav_refresh: Rect,
    /// SidebarPaneToggleButton, only when the sidebar pane is closed.
    pub hamburger: Option<Rect>,
    /// ShowStatusCenterButton, per the StatusCenterVisibility setting.
    pub status_center_button: Option<Rect>,
    /// Status bar's git widget (repo only): the network actions button
    /// (`GitNetworkActions`) and branch selector (`GitBranch`).
    pub status_git_actions: Option<Rect>,
    pub status_git_branch: Option<Rect>,
    /// Omnibar mode buttons inside the bar: path, command palette, search.
    pub omnibar_modes: [Rect; 3],
    pub address_bar: Rect,
    pub breadcrumbs: Vec<Rect>,
    /// `BreadcrumbBar.EllipsisButton`: present when the path overflows; it
    /// replaces the leading segments `0..breadcrumb_start`.
    pub breadcrumb_ellipsis: Option<Rect>,
    /// Index of the first VISIBLE segment (earlier ones are under the ellipsis).
    pub breadcrumb_start: usize,
    pub sidebar_items: Vec<(Rect, SidebarEntry)>,
    /// The sidebar content's extent, to bound its scrolling.
    pub sidebar_extent: f32,
    /// The sidebar content's natural width (untruncated labels), to bound
    /// its HORIZONTAL scrolling.
    pub sidebar_hextent: f32,
    /// The sidebar's ScrollBar (absent when everything fits).
    pub sidebar_scrollbar: Option<crate::user_controls::scrollbar::Scrollbar>,
    /// The sidebar's HORIZONTAL ScrollBar (absent when everything fits, or in
    /// Compact mode where `HorizontalScrollMode` is `Disabled`).
    pub sidebar_hscrollbar: Option<crate::user_controls::scrollbar::Scrollbar>,
    /// The pane's current display mode (`SidebarDisplayMode`).
    pub sidebar_mode: SidebarMode,
    /// In Minimal mode: the FLOATING pane (acrylic + shadow), positioned at
    /// its animated `TranslateX`. `None` in docked modes.
    pub sidebar_overlay: Option<Rect>,
    /// The light-dismiss layer behind the expanded Minimal pane: a click
    /// outside the pane closes it (`PaneLightDismissLayer`).
    pub sidebar_light_dismiss: Option<Rect>,
    /// The open `ContentDialog`'s geometry (hit-testing becomes modal).
    pub dialog_rects: Option<crate::dialogs::DialogRects>,
    pub conflict_rects: Option<crate::dialogs::ConflictRects>,
    pub quick_cards: Vec<Rect>,
    pub drive_cards: Vec<Rect>,
    pub recent_rows: Vec<Rect>,
    pub file_header: Rect,
    /// The Recycle Bin columns (widened original path) are active —
    /// `file_columns_for` must receive the SAME flag for drawing and
    /// hit-testing.
    pub recycle_columns: bool,
    pub file_rows: Vec<Rect>,
    /// True in Grid/Cards: items carry a clickable `SelectionCheckbox` in
    /// their top-left corner.
    pub grid_checkboxes: bool,
    /// Group headers (active grouping, Details view).
    pub group_rows: Vec<(Rect, String, usize)>,
    pub setting_rows: Vec<Rect>,
    pub settings_nav: Vec<Rect>,
    /// Appearance settings page (section 1) detailed layout.
    pub appearance: Option<crate::views::settings::appearance_page::AppearanceLayout>,
    /// Generic settings page layout (every other section).
    pub settings_page: Option<crate::views::settings::controls::SettingsPageLayout>,
    /// Command-bar buttons (folder views only) + its card rect.
    pub cmd_buttons: Vec<(Rect, Hot)>,
    pub cmdbar: Option<Rect>,
    /// Details pane region + its Properties button (folder views).
    pub info_pane: Option<Rect>,
    pub info_properties: Rect,
    /// The two resize handles (`SidebarResizer`, `InfoPaneSizer`).
    pub sidebar_resizer: Option<Rect>,
    pub info_resizer: Option<Rect>,
    /// The Détails/Aperçu radio buttons (InfoPane.xaml selector).
    pub info_tabs: [Rect; 2],
    /// The open omnibar suggestion panel: its surface and its row count. Held
    /// here so the hit-test can treat it as the topmost surface.
    pub suggestions: Option<(Rect, usize)>,
    /// Shelf pane (`ShelfPane.xaml`): the card, its item rows
    /// (`ShelfItemsList`) and the « Effacer les éléments » footer link.
    pub shelf_pane: Option<Rect>,
    pub shelf_items: Vec<Rect>,
    pub shelf_clear: Rect,
    pub content: Rect,
    /// Columns layout: one entry per blade (`BladeItem`), with its rows.
    pub column_panes: Vec<(Rect, Vec<Rect>)>,
    /// True when `content_extent` is measured in width, not height.
    pub scroll_horizontal: bool,
    /// The scrolling area's `ScrollBar`, absent when everything fits on screen.
    pub scrollbar: Option<crate::user_controls::scrollbar::Scrollbar>,
    /// Dual pane: unfocused pane region, its header/rows, and the divider.
    pub other_pane_rect: Option<Rect>,
    pub other_header: Rect,
    pub other_rows: Vec<Rect>,
    pub pane_divider: Option<Rect>,
    /// Natural height of the scrolled content, used to clamp scrolling.
    pub content_extent: f32,
}

impl Layout {
    pub fn compute(width: f32, height: f32, state: &UiState, model: &HomeModel) -> Self {
        let tab = state.active();
        let scroll = tab.scroll;

        // Caption buttons: 32 DIP tall (Windows 11 metric), flush with the
        // top edge — not the whole title bar, otherwise they overflow into
        // the content and look too big.
        let caption_close = Rect::new(width - CAPTION_BUTTON_WIDTH, 0.0, width, CAPTION_BUTTON_HEIGHT);
        let caption_max = Rect::new(width - CAPTION_BUTTON_WIDTH * 2.0, 0.0, caption_close.left, CAPTION_BUTTON_HEIGHT);
        let caption_min = Rect::new(width - CAPTION_BUTTON_WIDTH * 3.0, 0.0, caption_max.left, CAPTION_BUTTON_HEIGHT);

        // Tabs: TabStripHeader button (30 wide, Margin 4,0,-2,0 → column ends
        // at 32) then the items (WinUI TabViewItem min 100 / max 240 wide,
        // 32 high, below the TabView's 10 DIP top margin), shrinking to spare
        // the caption buttons. The first item starts at 39, not 32: the
        // TabView's scroll host adds a 7 DIP inset that TabBarStyles.xaml does
        // not express — measured on the compiled original (its selected tab's
        // wall and its first tab's icon both land on 39).
        let tabs_left = 39.0;
        let available = caption_min.left - tabs_left - 44.0;
        // Insertion preview: a tab arrives from another window — the strip
        // sizes n+1 slots and leaves the one at `tab_preview_insert` EMPTY
        // (the gap the TabView opens during a TabStripDragOver).
        let preview = state.tab_preview_insert;
        let slots = state.tabs.len() + preview.is_some() as usize;
        let tab_width = (available / slots.max(1) as f32).clamp(100.0, 240.0);
        let mut x = tabs_left;
        let mut tabs = Vec::new();
        for i in 0..state.tabs.len() {
            if preview == Some(i) {
                x += tab_width;
            }
            // Tabs touch the bottom of the band so the active one merges
            // with the surface below (browser-style).
            tabs.push(Rect::new(x, 10.0, x + tab_width, TAB_BAR_HEIGHT));
            x += tab_width;
        }
        if preview == Some(state.tabs.len()) {
            x += tab_width;
        }
        // TabBarAddNewTabButton: 30×30, centered in the 32-high strip.
        let new_tab = Rect::new(x + 4.0, 11.0, x + 34.0, 41.0);

        let ty = TAB_BAR_HEIGHT;
        // NavigationToolbar.xaml: Grid Height=48 Padding="4,0,4,0",
        // StackPanel Spacing="4", buttons 36×32 vertically centered.
        // SidebarPaneToggleButton: when the pane is closed, and ALWAYS in
        // Minimal mode (`IsSidebarPaneOpenToggleButtonVisible=True`).
        let show_hamburger =
            !state.sidebar_visible || sidebar_mode(width) == SidebarMode::Minimal;
        let hamburger = show_hamburger.then(|| Rect::new(4.0, ty + 8.0, 40.0, ty + 40.0));
        let nav_left = if hamburger.is_some() { 44.0 } else { 4.0 };
        let nav = |i: f32| {
            Rect::new(nav_left + i * 40.0, ty + 8.0, nav_left + i * 40.0 + 36.0, ty + 40.0)
        };
        let nav_back = nav(0.0);
        let nav_forward = nav(1.0);
        let nav_up = nav(2.0);
        let nav_refresh = nav(3.0);

        // ShowStatusCenterButton: Always, or only during file operations.
        let settings_now = crate::services::settings::get();
        let sc_visible = match settings_now.status_center_visibility {
            crate::services::settings::StatusCenterVisibility::Always => true,
            crate::services::settings::StatusCenterVisibility::DuringOngoingFileOperations => state.ops_active,
        };
        // Right column: Grid Padding right = 4, ColumnSpacing = 4.
        let status_center_button =
            sc_visible.then(|| Rect::new(width - 40.0, ty + 8.0, width - 4.0, ty + 40.0));
        let address_right = if sc_visible { width - 44.0 } else { width - 4.0 };
        // OmnibarDefaultHeight = 38, centered in the 48 DIP row.
        let address_bar = Rect::new(nav_refresh.right + 4.0, ty + 5.0, address_right, ty + 43.0);

        // Omnibar mode buttons at the right edge INSIDE the bar
        // (path / command palette / search). Geometry ported from the
        // control: `OmnibarModeDefaultClickAreaWidth = 46`,
        // `OmnibarModeDefaultHeight = 34`, packed to the right with
        // separators (mirrors `Omnibar.cs`).
        let modes = drive_app_controls::omnibar::mode_button_rects(&address_bar, 3);
        let omnibar_modes: [Rect; 3] = [modes[0], modes[1], modes[2]];

        // Breadcrumb segment rects inside the address bar. Like the
        // `BreadcrumbBar`, when the path overflows we collapse the LEADING
        // segments behind an ellipsis (…) and keep the TRAILING segments (the
        // current folder stays visible), instead of truncating the end.
        // The folding, the segment padding and the chevron block all belong to
        // `kubuno_ui::navigation::Breadcrumb`, which the omnibar paints with;
        // `navigation_toolbar` hands us its geometry so the boxes the pointer
        // hits are the boxes that get drawn.
        let bc = crate::user_controls::navigation_toolbar::breadcrumb_layout(
            &tab.breadcrumbs(),
            crate::user_controls::navigation_toolbar::breadcrumb_bounds(
                address_bar,
                omnibar_modes[0],
            ),
        );
        let breadcrumbs = bc.items;
        let breadcrumb_ellipsis = bc.ellipsis;
        let breadcrumb_start = bc.start_index;

        // Sidebar (items overflowing into the settings row are dropped until
        // the sidebar gets scrolling).
        // The display mode (`SidebarDisplayMode`): Minimal if the window is
        // narrow (< 641), otherwise the Compact/Expanded preference.
        let sidebar_disp_mode = sidebar_mode(width);
        let sidebar_full = match sidebar_disp_mode {
            SidebarMode::Minimal => SIDEBAR_OPEN_PANE_LENGTH,
            SidebarMode::Compact => SIDEBAR_COMPACT_WIDTH,
            SidebarMode::Expanded => crate::services::settings::get()
                .sidebar_width
                .clamp(SIDEBAR_MIN_WIDTH, SIDEBAR_MAX_WIDTH),
        };
        // Pane's X origin: in Minimal it FLOATS at its `TranslateX` (animated,
        // -300 stowed → 0 expanded); anchored at 0 in the other modes.
        let sx0 = if sidebar_disp_mode == SidebarMode::Minimal {
            state.sidebar_pane_tx_value()
        } else {
            0.0
        };
        // Width the CONTENT has to yield: the Minimal pane is an overlay,
        // it doesn't take a column → 0.
        let sidebar_w = match sidebar_disp_mode {
            SidebarMode::Minimal => 0.0,
            _ => if state.sidebar_visible { sidebar_full } else { 0.0 },
        };
        // In Minimal we always build the rows (they slide, stowed off-
        // screen when `tx = -300`).
        let sidebar_build = state.sidebar_visible || sidebar_disp_mode == SidebarMode::Minimal;
        let mut sidebar_items = Vec::new();
        // The widest natural width among all rows (untruncated labels):
        // decides whether the horizontal bar appears, independent of
        // vertical scrolling (measured over ALL rows, not just the visible
        // ones, so the bar doesn't flicker while scrolling vertically).
        let mut sidebar_max_w: f32 = 0.0;
        // The sidebar's ScrollViewer: `sy` runs over ALL the content
        // (offset by scrolling); only fully visible rows are pushed, but the
        // extent is measured at the end of the run.
        let sidebar_top = TAB_BAR_HEIGHT + TOOLBAR_HEIGHT + 8.0;
        let mut sy = sidebar_top - state.sidebar_scroll;
        if sidebar_build {
        // The `SidebarViewModel`'s sections: visibility (`ShowXxxSection`)
        // and expanded state (`IsXxxSectionExpanded`) come from settings,
        // like the original — the header's chevron toggles the state.
        let s = crate::services::settings::get();
        // COMPACT mode: the rail only lists top-level entries (icons
        // only) — no children or trees, and no horizontal scrolling
        // (`HorizontalScrollMode=Disabled`). The Minimal pane, on the other
        // hand, shows the labels (300 DIP).
        let compact = sidebar_disp_mode == SidebarMode::Compact;
        let active_location = &state.active().location;
        // Kubuno nav row height (36) — `shape::height::SIDEBAR_ROW`.
        let item_h = drive_app_controls::sidebar::SIDEBAR_ROW_HEIGHT;
        // Scrollable rows stop above the pinned Settings row (+ 8 of breathing room).
        let sidebar_limit = height - 12.0 - item_h - 8.0;
        let mut push = |entry: SidebarEntry, sy: &mut f32| {
            // The row's natural width for horizontal scrolling (the
            // Compact rail only shows icons: no measurement).
            if !compact {
                // Only the WIDTH matters here — selection doesn't change it.
                let v = sidebar_visual(&entry, model, active_location, false);
                sidebar_max_w = sidebar_max_w.max(sidebar_row_width(&v));
            }
            if *sy >= sidebar_top - 1.0 && *sy + item_h <= sidebar_limit {
                sidebar_items
                    .push((Rect::new(sx0 + 8.0, *sy, sx0 + sidebar_full - 8.0, *sy + item_h), entry));
            }
            *sy += item_h + 2.0;
        };
        push(SidebarEntry::Home, &mut sy);
        if s.show_pinned_section {
            sy += 12.0; // FlatSidebarItem.SectionGapMargin (section gap)
            push(SidebarEntry::SectionPinned, &mut sy);
            if s.is_pinned_section_expanded && !compact {
                for i in 0..model.quick_access.len() {
                    push(SidebarEntry::Pinned(i), &mut sy);
                }
            }
        }
        // Libraries: 3rd position (after Pinned), setting-gated.
        if s.show_library_section && !model.libraries.is_empty() {
            sy += 12.0; // FlatSidebarItem.SectionGapMargin (section gap)
            push(SidebarEntry::SectionLibraries, &mut sy);
            if s.is_library_section_expanded && !compact {
                for i in 0..model.libraries.len() {
                    push(SidebarEntry::Library(i), &mut sy);
                }
            }
        }
        if s.show_drives_section {
            sy += 12.0; // FlatSidebarItem.SectionGapMargin (section gap)
            push(SidebarEntry::SectionDrives, &mut sy);
            if s.is_drive_section_expanded && !compact {
                for i in 0..model.drives.len() {
                    // MAPPED network drives stay in Drives, like the
                    // original (the Network section lists locations).
                    {
                        push(SidebarEntry::Drive(i), &mut sy);
                        // The expanded drive's tree (hierarchical
                        // `SidebarItem`): depth-first walk of the cache.
                        let root = format!("{}:\\", model.drives[i].letter).to_lowercase();
                        let mut stack: Vec<(String, u8)> = Vec::new();
                        if state.sidebar_expanded.contains(&root) {
                            if let Some(children) = state.sidebar_children.get(&root) {
                                for (_, p) in children.iter().rev() {
                                    stack.push((p.clone(), 1));
                                }
                            }
                        }
                        while let Some((path, depth)) = stack.pop() {
                            let key = path.to_lowercase();
                            push(SidebarEntry::Folder(path, depth), &mut sy);
                            if state.sidebar_expanded.contains(&key) {
                                if let Some(children) = state.sidebar_children.get(&key) {
                                    for (_, p) in children.iter().rev() {
                                        stack.push((p.clone(), depth + 1));
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        if s.show_cloud_drives_section && !model.cloud_drives.is_empty() {
            sy += 12.0; // FlatSidebarItem.SectionGapMargin (section gap)
            push(SidebarEntry::SectionCloudDrives, &mut sy);
            if s.is_cloud_drive_section_expanded && !compact {
                for i in 0..model.cloud_drives.len() {
                    push(SidebarEntry::CloudDrive(i), &mut sy);
                    let root = model.cloud_drives[i].sync_folder.to_lowercase();
                    let mut stack: Vec<(String, u8)> = Vec::new();
                    if state.sidebar_expanded.contains(&root) {
                        if let Some(children) = state.sidebar_children.get(&root) {
                            for (_, p) in children.iter().rev() {
                                stack.push((p.clone(), 1));
                            }
                        }
                    }
                    while let Some((path, depth)) = stack.pop() {
                        let key = path.to_lowercase();
                        push(SidebarEntry::Folder(path, depth), &mut sy);
                        if state.sidebar_expanded.contains(&key) {
                            if let Some(children) = state.sidebar_children.get(&key) {
                                for (_, p) in children.iter().rev() {
                                    stack.push((p.clone(), depth + 1));
                                }
                            }
                        }
                    }
                }
            }
        }
        if s.show_network_section {
            sy += 12.0; // FlatSidebarItem.SectionGapMargin (section gap)
            push(SidebarEntry::SectionNetwork, &mut sy);
            if s.is_network_section_expanded && !compact {
                for i in 0..model.drives.len() {
                    if model.drives[i].is_network {
                        push(SidebarEntry::NetworkDrive(i), &mut sy);
                    }
                }
            }
        }
        if s.show_file_tags_section {
            sy += 12.0; // FlatSidebarItem.SectionGapMargin (section gap)
            push(SidebarEntry::SectionTags, &mut sy);
            if s.is_file_tags_section_expanded && !compact {
                for i in 0..s.file_tags.len() {
                    push(SidebarEntry::Tag(i), &mut sy);
                }
            }
        }
        // Settings pins to the bottom, same nav-row height as the rest.
        sidebar_items.push((
            Rect::new(sx0 + 8.0, height - 12.0 - item_h, sx0 + sidebar_full - 8.0, height - 12.0),
            SidebarEntry::Settings,
        ));
        if !compact {
            let v = sidebar_visual(&SidebarEntry::Settings, model, active_location, false);
            sidebar_max_w = sidebar_max_w.max(sidebar_row_width(&v));
        }
        }
        // The sidebar's total scrollable content extent.
        let sidebar_extent = (sy + state.sidebar_scroll) - sidebar_top + 8.0;
        // The HORIZONTAL extent: the widest row, never below the pane's
        // visible width (otherwise there's nothing to scroll).
        let sidebar_hextent = sidebar_max_w.max(sidebar_full);

        // Command bar + status bar only wrap folder views, and both honor
        // their Appearance toggles (ShowToolbar / ShowStatusBar).
        let appearance_settings = crate::services::settings::get();
        let is_dir = matches!(tab.location, Location::Dir(_));
        // Recycle Bin: the command bar ALSO shows — `ToolbarSections.cs:51-56`
        // defines a « RecycleBin » context with its own buttons.
        let is_recycle_bin = tab.location == Location::RecycleBin;
        let show_cmdbar = (is_dir || is_recycle_bin) && appearance_settings.show_toolbar;
        let show_statusbar = is_dir && appearance_settings.show_status_bar;
        let content_top = TAB_BAR_HEIGHT
            + TOOLBAR_HEIGHT
            + if show_cmdbar { CMDBAR_HEIGHT } else { 0.0 };
        let content_bottom = height - if show_statusbar { STATUSBAR_HEIGHT } else { 0.0 };

        // The SidebarView's content region: `ContentPresenter`
        // Margin="2,0,8,0" — the command card, the file card and the
        // status bar share this same left edge (sidebar + 2).
        let content_left = sidebar_w + 2.0;

        // Status bar's git widget (StatusBar.xaml's Grid columns 1 & 2),
        // shown only INSIDE a repo: branch selector on the right, network
        // actions button (ahead/behind counter) to its left. Both cells are
        // placed by `kubuno_ui::navigation::StatusBar`'s own `Spring`
        // arrangement, which `status_bar` drives. They are right-aligned, so
        // only the strip's RIGHT edge decides where they land: the painter
        // starts the strip at the focused pane's edge (`content.left`, which
        // is not yet known here) and gets the same two boxes.
        let (status_git_actions, status_git_branch) = if show_statusbar {
            crate::user_controls::status_bar::git_rects(
                tab,
                crate::user_controls::status_bar::bounds(content_left, width, height),
            )
        } else {
            (None, None)
        };

        // The command card (Toolbar.xaml: CornerRadius=8, CardStroke,
        // ~48 DIP tall) encloses centered 36 DIP `AppBarButton`s, which
        // `kubuno_ui::navigation::Toolbar` arranges; `user_controls::toolbar`
        // owns the item model, so the boxes the pointer hits are the boxes
        // that get drawn.
        let bar_top = TAB_BAR_HEIGHT + TOOLBAR_HEIGHT;
        let cmdbar = show_cmdbar.then(|| Rect::new(content_left, bar_top, width - 8.0, bar_top + 48.0));
        let cmd_buttons: Vec<(Rect, Hot)> = match cmdbar {
            Some(card) => crate::user_controls::toolbar::cmdbar_arrangement(state, card),
            None => Vec::new(),
        };

        // Shelf pane at the far right (`MainPage.xaml` ShelfPaneColumn,
        // after InfoPaneColumn): fixed-width 240 column (Margin="4,0,0,8").
        // The content AND the preview pane shift to the left.
        let show_shelf = crate::services::settings::get().show_shelf_pane;
        let shelf_pane = show_shelf.then(|| {
            Rect::new(width - SHELF_PANE_WIDTH - 8.0, content_top + 4.0, width - 8.0, content_bottom - 8.0)
        });
        // The right edge available to the content and preview pane.
        let right_bound = match shelf_pane {
            Some(s) => s.left - 4.0,
            None => width,
        };

        // Details pane on the right (port of InfoPane, CornerRadius=8):
        // a separate CARD with margins, like the original block layout.
        let show_info = is_dir && crate::services::settings::get().show_info_pane;
        // The pane can't eat into the content column (`MinWidth="208"`).
        let info_w = infopane_width()
            .min((right_bound - sidebar_w - CONTENT_MIN_WIDTH).max(INFOPANE_MIN_WIDTH));
        let info_pane = show_info.then(|| {
            Rect::new(right_bound - info_w, content_top + 4.0, right_bound - 8.0, content_bottom - 8.0)
        });
        // « Propriétés »: button LEFT-aligned, sized to its content
        // (InfoPane.xaml: HorizontalAlignment="Left", Margin="12,0,8,8",
        // icon + label), not a full-width bar.
        let info_properties = info_pane
            .map(|p| {
                let w = (approx_text_width(drive_localization::tr("Properties"), 14.0) + 60.0)
                    .min(p.right - p.left - 24.0);
                Rect::new(p.left + 12.0, p.bottom - 44.0, p.left + 12.0 + w, p.bottom - 12.0)
            })
            .unwrap_or_default();
        // Détails/Aperçu selector: centered StackPanel (Margin=12), two
        // 32px radio buttons (InfoPane.xaml Local.RadioButtonStyle).
        let info_tabs = info_pane
            .map(|p| {
                let cx = (p.left + p.right) / 2.0;
                let y = p.top + 12.0;
                [
                    Rect::new(cx - 96.0, y, cx, y + 32.0),
                    Rect::new(cx, y, cx + 96.0, y + 32.0),
                ]
            })
            .unwrap_or_default();
        // Shelf: the item rows (`ShelfItemsList`, height 36) and the
        // « Effacer les éléments » footer link. The footer only exists if
        // the list is populated (`PopulatedListToVisibilityConverter`).
        let (shelf_items, shelf_clear) = match shelf_pane {
            Some(p) => {
                // Header: Padding 12,12,12,4 + title (~20) + 8 + divider (1).
                let header_bottom = p.top + 45.0;
                let n = state.shelf.len();
                let footer_h = if n > 0 { 44.0 } else { 0.0 };
                let list_top = header_bottom + 4.0; // `ListView` Padding 8,4.
                let list_bottom = p.bottom - footer_h;
                let mut items = Vec::with_capacity(n);
                let mut y = list_top;
                for _ in 0..n {
                    if y + SHELF_ROW_HEIGHT > list_bottom {
                        break; // beyond this, the list scrolls (out of click reach).
                    }
                    items.push(Rect::new(p.left + 8.0, y, p.right - 8.0, y + SHELF_ROW_HEIGHT));
                    y += SHELF_ROW_HEIGHT;
                }
                let clear = if n > 0 {
                    let cy = p.bottom - footer_h + 9.0; // divider (1) + Padding 12,4.
                    Rect::new(p.left + 12.0, cy, p.right - 12.0, cy + 28.0)
                } else {
                    Rect::default()
                };
                (items, clear)
            }
            None => (Vec::new(), Rect::default()),
        };

        // Gaps pixel-matched to the original app: ~8 DIP between the
        // command bar and the card. `content_top` ALREADY includes +4 DIP
        // (CMDBAR_HEIGHT = 48 card + 4 margin, cf. `Toolbar Margin="0,0,0,4"`),
        // so the card only adds +4 for a visible gap of 8. Sides/bottom keep 8.
        let card_right = if show_info { right_bound - info_w - 8.0 } else { right_bound - 8.0 };

        // Dual pane: the content region is halved and the focused pane gets
        // the regular layout; the other pane renders read-only rows.
        let group = state.group();
        // The file card: aligned on the same left edge as the command
        // card (sidebar + 2, the SidebarView's `ContentPresenter`).
        let full = Rect::new(sidebar_w + 2.0, content_top + 4.0, card_right, content_bottom - 8.0);
        let (content, other_pane_rect, pane_divider) = if group.panes.len() == 2 {
            // `GridSplitter`: the columns/rows are `Star`, the first pane
            // occupies `split_ratio` of the space. Each pane keeps ≥ 100
            // DIP (MinWidth/MinHeight="100" from ShellPanesPage's definitions).
            let (a, b, divider) = if group.split_vertical {
                let span = full.right - full.left;
                let r = group.split_ratio.clamp(100.0 / span, 1.0 - 100.0 / span);
                let mid = full.left + span * r;
                (
                    Rect::new(full.left, full.top, mid - 1.0, full.bottom),
                    Rect::new(mid + 1.0, full.top, full.right, full.bottom),
                    Rect::new(mid - 1.0, full.top, mid + 1.0, full.bottom),
                )
            } else {
                let span = full.bottom - full.top;
                let r = group.split_ratio.clamp(100.0 / span, 1.0 - 100.0 / span);
                let mid = full.top + span * r;
                (
                    Rect::new(full.left, full.top, full.right, mid - 1.0),
                    Rect::new(full.left, mid + 1.0, full.right, full.bottom),
                    Rect::new(full.left, mid - 1.0, full.right, mid + 1.0),
                )
            };
            if group.active_pane == 0 {
                (a, Some(b), Some(divider))
            } else {
                (b, Some(a), Some(divider))
            }
        } else {
            (full, None, None)
        };

        // Read-only layout of the unfocused pane (Dir locations only).
        let mut other_header = Rect::default();
        let mut other_rows = Vec::new();
        if let (Some(rect), Some(other)) = (&other_pane_rect, group.other()) {
            if matches!(other.location, Location::Dir(_)) {
                other_header = Rect::new(rect.left + 24.0, rect.top + 8.0, rect.right - 24.0, rect.top + 36.0);
                let mut fy = other_header.bottom + 4.0 - other.scroll;
                for _ in 0..other.entries.len() {
                    other_rows.push(Rect::new(rect.left + 24.0, fy, rect.right - 24.0, fy + FILE_ROW_HEIGHT));
                    fy += FILE_ROW_HEIGHT;
                }
            }
        }
        let pad = 24.0;
        let inner_w = (content.right - content.left) - pad * 2.0;

        let mut quick_cards = Vec::new();
        let mut drive_cards = Vec::new();
        let mut recent_rows = Vec::new();
        let mut file_rows = Vec::new();
        let mut setting_rows = Vec::new();
        let mut file_header = Rect::default();
        // The Details view's group headers: (rect, label, count).
        let mut group_rows: Vec<(Rect, String, usize)> = Vec::new();
        let content_extent;
        // The Columns layout's blades: (blade rect, its rows).
        let mut column_panes: Vec<(Rect, Vec<Rect>)> = Vec::new();
        // List and Columns scroll horizontally (vertical `ItemsWrapGrid`
        // and `BladeView`).
        let mut scroll_horizontal = false;

        let mut settings_nav = Vec::new();
        let mut appearance = None;
        let mut settings_page = None;
        match &tab.location {
            Location::Settings => {
                // Internal nav column (like the original SettingsPage): the
                // title carries `Margin="16,12,0,4"` (Subtitle line ~28), so
                // items start at 12 + 28 + 4 = 44.
                let mut ny = content.top + 44.0;
                for _ in SETTINGS_SECTIONS {
                    settings_nav.push(Rect::new(content.left + 16.0, ny, content.left + 276.0, ny + 36.0));
                    ny += 38.0;
                }
                if state.settings_section == 1 {
                    // Appearance: full AppearancePage layout.
                    let ap = crate::views::settings::appearance_page::compute(
                        &content,
                        scroll,
                        state.appearance_expanded,
                    );
                    for (r, _) in &ap.rows {
                        setting_rows.push(*r);
                    }
                    content_extent = ap.extent;
                    appearance = Some(ap);
                } else {
                    // Generic pages (General, Layout, Folders, Actions,
                    // Tags, DevTools, Advanced, About).
                    let settings = crate::services::settings::get();
                    let rows = crate::views::settings::page_rows(
                        state.settings_section,
                        &settings,
                        &state.settings_expanded[state.settings_section],
                    );
                    let page = crate::views::settings::controls::compute(&content, scroll, rows);
                    setting_rows.extend(page.rects.iter().copied());
                    content_extent = page.extent;
                    settings_page = Some(page);
                }
            }
            Location::Home => {
                // The Home widgets honor the visibility settings
                // (`ShowQuickAccessWidget` / `ShowDrivesWidget` /
                // `ShowRecentFilesWidget`): an unchecked section generates
                // no card and its header isn't drawn (see draw_home).
                let s = crate::services::settings::get();
                let card_gap = 12.0;
                // Vertical cursor that skips each hidden section.
                let mut y = content.top + 4.0 - scroll;

                // Quick access cards grid.
                if s.show_quick_access_widget {
                    let per_row = ((inner_w + card_gap) / (170.0 + card_gap)).floor().max(1.0) as usize;
                    let card_w = (inner_w - card_gap * (per_row as f32 - 1.0)) / per_row as f32;
                    let card_h = 92.0;
                    // 8 DIP below the « Accès rapide » header (4 margin + 28 header).
                    let mut cy = y + 36.0;
                    let mut cx = content.left + pad;
                    for i in 0..model.quick_access.len() {
                        if i > 0 && i % per_row == 0 {
                            cy += card_h + card_gap;
                            cx = content.left + pad;
                        }
                        quick_cards.push(Rect::new(cx, cy, cx + card_w, cy + card_h));
                        cx += card_w + card_gap;
                    }
                    y = quick_cards.last().map_or(cy, |r| r.bottom);
                }

                // Drive cards grid.
                if s.show_drives_widget {
                    let drive_per_row = ((inner_w + card_gap) / (290.0 + card_gap)).floor().max(1.0) as usize;
                    let drive_w = (inner_w - card_gap * (drive_per_row as f32 - 1.0)) / drive_per_row as f32;
                    let drive_h = 68.0;
                    let mut dy = y + 56.0;
                    let mut dx = content.left + pad;
                    for i in 0..model.drives.len() {
                        if i > 0 && i % drive_per_row == 0 {
                            dy += drive_h + card_gap;
                            dx = content.left + pad;
                        }
                        drive_cards.push(Rect::new(dx, dy, dx + drive_w, dy + drive_h));
                        dx += drive_w + card_gap;
                    }
                    y = drive_cards.last().map_or(dy, |r| r.bottom);
                }

                // Recent files rows.
                if s.show_recent_files_widget {
                    let mut ry = y + 56.0;
                    for _ in 0..model.recent_files.len() {
                        recent_rows.push(Rect::new(content.left + pad, ry, content.right - pad, ry + 30.0));
                        ry += 32.0;
                    }
                    y = ry;
                }
                content_extent = (y + scroll) - content.top + 16.0;
            }
            // The five `FolderLayoutModes` layouts, each with its own
            // size (`LayoutSizeKindHelper`).
            Location::Dir(_) | Location::RecycleBin | Location::SearchResults { .. } if tab.view_mode == crate::view_models::shell_view_model::ViewMode::Grid => {
                // `ItemsWrapGrid Orientation="Horizontal"`: tiles fill a
                // row then wrap to the next.
                let size = crate::view_models::shell_view_model::layout_size(tab.view_mode);
                let tile = crate::view_models::shell_view_model::grid_item_width_for(size);
                // `GridViewBrowserTemplate`: a square icon box of
                // `ItemWidthGridView`, then the name row (`Margin="4,0,4,8"`).
                let tile_h = tile + crate::view_models::shell_view_model::GRID_NAME_ROW;
                // The web grid breathes at `gap-3`; tiles now carry a real
                // frame (fill + 1px border), so 8 crowded them.
                let gap = drive_app_controls::themes::shape::space::MD;
                let inner = (content.right - content.left) - pad * 2.0;
                let per_row = ((inner + gap) / (tile + gap)).floor().max(1.0) as usize;
                let mut gx = content.left + pad;
                let mut gy = content.top + 12.0 - scroll;
                // Grouping: a full-width header before each group, and
                // wrapping restarts a new row (GroupStyle).
                let boundaries = tab.group_boundaries();
                let mut next_group = boundaries.iter().peekable();
                let mut col = 0usize;
                for i in 0..tab.entries.len() {
                    if let Some((start, label, count)) = next_group.peek() {
                        if *start == i {
                            if col > 0 {
                                gy += tile_h + gap;
                            }
                            group_rows.push((
                                Rect::new(content.left + pad, gy, content.right - pad, gy + GROUP_HEADER_HEIGHT),
                                label.clone(),
                                *count,
                            ));
                            gy += GROUP_HEADER_HEIGHT;
                            gx = content.left + pad;
                            col = 0;
                            next_group.next();
                        }
                    }
                    if col == per_row {
                        gy += tile_h + gap;
                        gx = content.left + pad;
                        col = 0;
                    }
                    file_rows.push(Rect::new(gx, gy, gx + tile, gy + tile_h));
                    gx += tile + gap;
                    col += 1;
                }

                let bottom = file_rows.last().map_or(gy, |r| r.bottom);
                content_extent = (bottom + scroll) - content.top + 16.0;
            }
            Location::Dir(_) | Location::RecycleBin | Location::SearchResults { .. } if tab.view_mode == crate::view_models::shell_view_model::ViewMode::Cards => {
                // Tiles: icon and name side by side (`CardsViewOrientation`).
                let size = crate::view_models::shell_view_model::layout_size(tab.view_mode);
                let (card_w, card_h) = crate::view_models::shell_view_model::card_size_for(size);
                // Same `gap-3` as the grid — cards are framed too.
                let gap = drive_app_controls::themes::shape::space::MD;
                let inner = (content.right - content.left) - pad * 2.0;
                let per_row = ((inner + gap) / (card_w + gap)).floor().max(1.0) as usize;
                let mut gx = content.left + pad;
                let mut gy = content.top + 12.0 - scroll;
                // Same GroupStyle as the grid.
                let boundaries = tab.group_boundaries();
                let mut next_group = boundaries.iter().peekable();
                let mut col = 0usize;
                for i in 0..tab.entries.len() {
                    if let Some((start, label, count)) = next_group.peek() {
                        if *start == i {
                            if col > 0 {
                                gy += card_h + gap;
                            }
                            group_rows.push((
                                Rect::new(content.left + pad, gy, content.right - pad, gy + GROUP_HEADER_HEIGHT),
                                label.clone(),
                                *count,
                            ));
                            gy += GROUP_HEADER_HEIGHT;
                            gx = content.left + pad;
                            col = 0;
                            next_group.next();
                        }
                    }
                    if col == per_row {
                        gy += card_h + gap;
                        gx = content.left + pad;
                        col = 0;
                    }
                    file_rows.push(Rect::new(gx, gy, gx + card_w, gy + card_h));
                    gx += card_w + gap;
                    col += 1;
                }

                let bottom = file_rows.last().map_or(gy, |r| r.bottom);
                content_extent = (bottom + scroll) - content.top + 16.0;
            }
            Location::Dir(_)
                if tab.view_mode == crate::view_models::shell_view_model::ViewMode::Columns =>
            {
                // `ColumnsLayoutPage`: a `BladeView` of 200 DIP blades, the
                // stack scrolls HORIZONTALLY and each blade lists its folder.
                use crate::view_models::shell_view_model::COLUMN_PANE_WIDTH as PANE_W;
                let size = crate::view_models::shell_view_model::layout_size(tab.view_mode);
                let row_h = crate::view_models::shell_view_model::list_row_height_for(size);
                let top = content.top + 8.0;
                for (c, pane) in tab.columns.iter().enumerate() {
                    // The blade's rect (horizontal stacking) comes from
                    // `blade_view` (mirrors `BladeView`); the internal rows
                    // remain a local vertical list (the blade's content).
                    let rect = drive_app_controls::blade_view::blade_rect(
                        c, PANE_W, content.left + pad, content.top, content.bottom, scroll,
                    );
                    let x0 = rect.left;
                    let mut rows = Vec::with_capacity(pane.entries.len());
                    for i in 0..pane.entries.len() {
                        let y0 = top + i as f32 * row_h - pane.scroll;
                        rows.push(Rect::new(x0 + 4.0, y0, x0 + PANE_W - 4.0, y0 + row_h));
                    }
                    column_panes.push((rect, rows));
                }
                content_extent =
                    drive_app_controls::blade_view::content_extent_uniform(tab.columns.len(), PANE_W, pad);
                scroll_horizontal = true;
            }
            Location::Dir(_) | Location::RecycleBin | Location::SearchResults { .. } if tab.view_mode == crate::view_models::shell_view_model::ViewMode::List => {
                // `ItemsWrapGrid Orientation="Vertical"`: items go down
                // then wrap into a new column. This is Explorer's list, and
                // scrolling is HORIZONTAL.
                let size = crate::view_models::shell_view_model::layout_size(tab.view_mode);
                let row_h = crate::view_models::shell_view_model::list_row_height_for(size);
                let col_w = 240.0;
                let top = content.top + 12.0;
                let _usable = (content.bottom - top - 12.0).max(row_h);
                // Grouped, the list becomes column BLOCKS: each group
                // carries its own header and columns, blocks follow each
                // other horizontally (GroupStyle + vertical wrap).
                let boundaries = tab.group_boundaries();
                let grouped = !boundaries.is_empty();
                let items_top = if grouped { top + GROUP_HEADER_HEIGHT } else { top };
                let usable = (content.bottom - items_top - 12.0).max(row_h);
                let per_col = (usable / row_h).floor().max(1.0) as usize;
                if grouped {
                    let mut gx = content.left + pad - scroll;
                    for (start, label, count) in &boundaries {
                        let cols = count.div_ceil(per_col.max(1)).max(1);
                        group_rows.push((
                            Rect::new(gx, top, gx + cols as f32 * col_w - 8.0, top + GROUP_HEADER_HEIGHT),
                            label.clone(),
                            *count,
                        ));
                        for k in 0..*count {
                            let col = k / per_col;
                            let row = k % per_col;
                            let x0 = gx + col as f32 * col_w;
                            let y0 = items_top + row as f32 * row_h;
                            let _ = start;
                            file_rows.push(Rect::new(x0, y0, x0 + col_w - 8.0, y0 + row_h));
                        }
                        gx += cols as f32 * col_w + 16.0;
                    }
                    content_extent = (gx + scroll) - content.left + pad;
                } else {
                    for i in 0..tab.entries.len() {
                        let col = i / per_col;
                        let row = i % per_col;
                        let x0 = content.left + pad + col as f32 * col_w - scroll;
                        let y0 = items_top + row as f32 * row_h;
                        file_rows.push(Rect::new(x0, y0, x0 + col_w - 8.0, y0 + row_h));
                    }
                    let cols = tab.entries.len().div_ceil(per_col.max(1)) as f32;
                    content_extent = cols * col_w + pad * 2.0;
                }
                scroll_horizontal = true;

            }
            Location::Dir(_) | Location::RecycleBin | Location::SearchResults { .. } => {
                let size = crate::view_models::shell_view_model::layout_size(tab.view_mode);
                let row_h = crate::view_models::shell_view_model::row_height_for(size);
                file_header = Rect::new(content.left + pad, content.top + 8.0, content.right - pad, content.top + 36.0);
                let mut fy = file_header.bottom + 4.0 - scroll;
                // Grouping: a header row before each group
                // (`GroupedCollection` + the XAML pages' `GroupSummary`).
                let boundaries = tab.group_boundaries();
                let mut next_group = boundaries.iter().peekable();
                for i in 0..tab.entries.len() {
                    if let Some((start, label, count)) = next_group.peek() {
                        if *start == i {
                            group_rows.push((
                                Rect::new(content.left + pad, fy, content.right - pad, fy + GROUP_HEADER_HEIGHT),
                                label.clone(),
                                *count,
                            ));
                            fy += GROUP_HEADER_HEIGHT;
                            next_group.next();
                        }
                    }
                    file_rows.push(Rect::new(content.left + pad, fy, content.right - pad, fy + row_h));
                    fy += row_h;
                }
                content_extent = (fy + scroll) - content.top + 16.0;
            }
        }

        Self {
            width,
            height,
            caption_min,
            caption_max,
            caption_close,
            tabs,
            new_tab,
            nav_back,
            nav_forward,
            nav_up,
            nav_refresh,
            hamburger,
            status_center_button,
            status_git_actions,
            status_git_branch,
            omnibar_modes,
            address_bar,
            breadcrumbs,
            breadcrumb_ellipsis,
            breadcrumb_start,
            sidebar_items,
            sidebar_extent,
            sidebar_hextent,
            // The sidebar's ScrollBar, in its scrolling band.
            dialog_rects: state
                .dialog
                .as_ref()
                .map(|d| crate::dialogs::dialog_rects(d, width, height)),
            conflict_rects: state
                .conflict
                .as_ref()
                .map(|c| crate::dialogs::conflict_rects(c, width, height, state.conflict_scroll)),
            sidebar_scrollbar: if sidebar_build {
                crate::user_controls::scrollbar::Scrollbar::new(
                    &Rect::new(sx0, TAB_BAR_HEIGHT + TOOLBAR_HEIGHT + 8.0, sx0 + sidebar_full, height - 56.0),
                    sidebar_extent,
                    state.sidebar_scroll,
                    false,
                    state.sidebar_scrollbar_expanded,
                )
            } else {
                None
            },
            // The HORIZONTAL ScrollBar (`HorizontalScrollMode=Enabled`): it
            // only appears if the widest row overflows the pane; in
            // Compact `sidebar_hextent == sidebar_full`, so `None`.
            sidebar_hscrollbar: if sidebar_build {
                crate::user_controls::scrollbar::Scrollbar::new(
                    &Rect::new(sx0, TAB_BAR_HEIGHT + TOOLBAR_HEIGHT + 8.0, sx0 + sidebar_full, height - 56.0),
                    sidebar_hextent,
                    state.sidebar_hscroll,
                    true,
                    state.sidebar_scrollbar_expanded,
                )
            } else {
                None
            },
            // Minimal mode's FLOATING pane (acrylic + shadow), its
            // light-dismiss band when expanded, and the current mode.
            sidebar_mode: sidebar_disp_mode,
            sidebar_overlay: (sidebar_disp_mode == SidebarMode::Minimal).then(|| {
                Rect::new(sx0, TAB_BAR_HEIGHT + TOOLBAR_HEIGHT, sx0 + sidebar_full, height)
            }),
            // The light-dismiss band: the region TO THE RIGHT of the
            // floating pane (clicking the pane itself doesn't close it).
            sidebar_light_dismiss: (sidebar_disp_mode == SidebarMode::Minimal
                && sx0 > -SIDEBAR_OPEN_PANE_LENGTH + 1.0)
            .then(|| Rect::new(sx0 + sidebar_full, TAB_BAR_HEIGHT + TOOLBAR_HEIGHT, width, height)),
            quick_cards,
            drive_cards,
            recent_rows,
            file_header,
            recycle_columns: tab.location == Location::RecycleBin,
            file_rows,
            grid_checkboxes: matches!(tab.location, Location::Dir(_) | Location::RecycleBin | Location::SearchResults { .. })
                && matches!(
                    tab.view_mode,
                    crate::view_models::shell_view_model::ViewMode::Grid
                        | crate::view_models::shell_view_model::ViewMode::Cards
                ),
            group_rows,
            setting_rows,
            settings_nav,
            appearance,
            settings_page,
            cmd_buttons,
            cmdbar,
            info_pane,
            info_properties,
            // The sidebar's resize rail. The web gives it `w-3` (12) — a
            // comfortable grab zone, and wide enough to contain the grip pill
            // it reveals on hover. Collapsed in Minimal (floating pane).
            sidebar_resizer: (state.sidebar_visible
                && sidebar_disp_mode != SidebarMode::Minimal)
            .then(|| {
                Rect::new(
                    sidebar_full - RESIZE_RAIL / 2.0,
                    TAB_BAR_HEIGHT + TOOLBAR_HEIGHT + 12.0,
                    sidebar_full + RESIZE_RAIL / 2.0,
                    height - 40.0,
                )
            }),
            // `InfoPaneSizer`: the GridSplitter between the content and the pane.
            info_resizer: info_pane.map(|p| {
                Rect::new(p.left - 12.0, p.top, p.left - 2.0, p.bottom)
            }),
            // Mirrors `draw_omnibar_suggestions`: only while the omnibar is in
            // edit mode AND has something to propose.
            suggestions: (state
                .edit
                .as_ref()
                .is_some_and(|e| {
                    matches!(e.entry, crate::ui::EDIT_PATH | crate::ui::EDIT_PALETTE | crate::ui::EDIT_SEARCH)
                })
                && !state.path_suggestions.is_empty())
            .then(|| {
                let rows = state.path_suggestions.len();
                (
                    Rect::new(
                        address_bar.left,
                        address_bar.bottom + 4.0,
                        address_bar.right,
                        address_bar.bottom + 12.0 + rows as f32 * 44.0,
                    ),
                    rows,
                )
            }),
            info_tabs,
            shelf_pane,
            shelf_items,
            shelf_clear,
            content,
            other_pane_rect,
            other_header,
            other_rows,
            pane_divider,
            content_extent,
            column_panes,
            scroll_horizontal,
            // The scrolling area's `ScrollBar`. The pointer in the gutter
            // expands it; otherwise it's just the 2 DIP indicator.
            scrollbar: crate::user_controls::scrollbar::Scrollbar::new(
                &content,
                content_extent,
                scroll,
                scroll_horizontal,
                state.scrollbar_expanded,
            ),
        }
    }

    pub fn max_scroll(&self) -> f32 {
        let visible = if self.scroll_horizontal {
            self.content.right - self.content.left
        } else {
            self.content.bottom - self.content.top
        };
        (self.content_extent - visible).max(0.0)
    }

    pub fn hit_test(&self, x: f32, y: f32) -> Option<Hot> {
        // The suggestion panel floats OVER the toolbar and the listing, so it
        // is tested before them — and it swallows whatever it covers, rows or
        // not (only a dialog, being modal, outranks it).
        if self.dialog_rects.is_none() && self.conflict_rects.is_none() {
            if let Some((panel, rows)) = &self.suggestions {
                if panel.contains(x, y) {
                    for i in 0..*rows {
                        if crate::ui::suggestion_rect(&self.address_bar, i).contains(x, y) {
                            return Some(Hot::Suggestion(i));
                        }
                    }
                    return Some(Hot::SuggestionPanel);
                }
            }
        }
        // The conflict dialog is MODAL, priority over everything.
        if let Some(c) = &self.conflict_rects {
            // The title bar's ✕ dismisses like Cancel does.
            if c.titlebar_close.contains(x, y) {
                return Some(Hot::DialogClose);
            }
            if c.primary.contains(x, y) {
                return Some(Hot::DialogPrimary);
            }
            if c.close.contains(x, y) {
                return Some(Hot::DialogClose);
            }
            if c.apply_all.contains(x, y) {
                return Some(Hot::ConflictApplyAll);
            }
            for (i, opt) in c.options.iter().enumerate() {
                if opt.contains(x, y) {
                    return Some(Hot::ConflictOption(i));
                }
            }
            return None;
        }
        // An open `ContentDialog` is MODAL: it intercepts everything.
        if let Some(d) = &self.dialog_rects {
            if d.titlebar_close.contains(x, y) {
                return Some(Hot::DialogClose);
            }
            if d.primary.contains(x, y) {
                return Some(Hot::DialogPrimary);
            }
            if d.close.contains(x, y) {
                return Some(Hot::DialogClose);
            }
            for (i, c) in d.choices.iter().enumerate() {
                if c.contains(x, y) {
                    return Some(Hot::DialogChoice(i));
                }
            }
            if d.checkbox.as_ref().is_some_and(|c| c.contains(x, y)) {
                return Some(Hot::DialogCheckbox);
            }
            for (i, row) in d.list.iter().enumerate() {
                if row.contains(x, y) {
                    return Some(Hot::DialogItem(i));
                }
            }
            return None;
        }
        // The handles have `Canvas.ZIndex="100"`: they take priority over
        // whatever they overlap — including the sidebar's `ScrollBar`, which
        // runs along the same edge (its thumb would otherwise cover the
        // handle's 4 DIP, and the Expanded ⇄ Compact double-click would
        // never fire).
        if self.sidebar_resizer.is_some_and(|r| r.contains(x, y)) {
            return Some(Hot::SidebarResizer);
        }
        if self.info_resizer.is_some_and(|r| r.contains(x, y)) {
            return Some(Hot::InfoPaneResizer);
        }
        // The `ScrollBar` sits ON the content: it takes priority over the rows.
        if let Some(bar) = &self.scrollbar {
            if bar.thumb.contains(x, y) {
                return Some(Hot::ScrollThumb);
            }
            if bar.expanded && bar.rail.contains(x, y) {
                return Some(Hot::ScrollTrack);
            }
        }
        if let Some(bar) = &self.sidebar_scrollbar {
            if bar.thumb.contains(x, y) {
                return Some(Hot::SidebarScrollThumb);
            }
            if bar.expanded && bar.rail.contains(x, y) {
                return Some(Hot::SidebarScrollTrack);
            }
        }
        if let Some(bar) = &self.sidebar_hscrollbar {
            if bar.thumb.contains(x, y) {
                return Some(Hot::SidebarHScrollThumb);
            }
            if bar.expanded && bar.rail.contains(x, y) {
                return Some(Hot::SidebarHScrollTrack);
            }
        }
        if self.caption_close.contains(x, y) {
            return Some(Hot::CaptionClose);
        }
        if self.caption_max.contains(x, y) {
            return Some(Hot::CaptionMax);
        }
        if self.caption_min.contains(x, y) {
            return Some(Hot::CaptionMin);
        }
        if self.new_tab.contains(x, y) {
            return Some(Hot::NewTab);
        }
        if crate::services::settings::get().show_tab_actions
            && Rect::new(4.0, 11.0, 34.0, 41.0).contains(x, y)
        {
            return Some(Hot::PaneToggle);
        }
        for (i, tab) in self.tabs.iter().enumerate() {
            if tab.contains(x, y) {
                if tab_close_rect(tab).contains(x, y) {
                    return Some(Hot::TabClose(i));
                }
                return Some(Hot::Tab(i));
            }
        }
        if self.nav_back.contains(x, y) {
            return Some(Hot::NavBack);
        }
        if self.nav_forward.contains(x, y) {
            return Some(Hot::NavForward);
        }
        if self.nav_up.contains(x, y) {
            return Some(Hot::NavUp);
        }
        if self.nav_refresh.contains(x, y) {
            return Some(Hot::NavRefresh);
        }
        if self.hamburger.is_some_and(|r| r.contains(x, y)) {
            return Some(Hot::Hamburger);
        }
        if self.status_center_button.is_some_and(|r| r.contains(x, y)) {
            return Some(Hot::StatusCenter);
        }
        if self.status_git_actions.is_some_and(|r| r.contains(x, y)) {
            return Some(Hot::StatusGitActions);
        }
        if self.status_git_branch.is_some_and(|r| r.contains(x, y)) {
            return Some(Hot::StatusGitBranch);
        }
        for (i, rect) in self.omnibar_modes.iter().enumerate() {
            if rect.contains(x, y) {
                return Some(Hot::OmnibarMode(i));
            }
        }
        if let Some(ell) = &self.breadcrumb_ellipsis {
            if ell.contains(x, y) {
                return Some(Hot::BreadcrumbEllipsis);
            }
        }
        for (i, rect) in self.breadcrumbs.iter().enumerate() {
            if rect.contains(x, y) {
                return Some(Hot::Breadcrumb(i));
            }
        }
        for (rect, hot) in &self.cmd_buttons {
            if rect.contains(x, y) {
                return Some(*hot);
            }
        }
        // Empty area of the omnibar: switches to path-edit mode.
        if self.address_bar.contains(x, y) {
            return Some(Hot::AddressBar);
        }
        for (i, (rect, _)) in self.sidebar_items.iter().enumerate() {
            if rect.contains(x, y) {
                return Some(Hot::SidebarItem(i));
            }
        }
        // The Minimal pane floats ON the content: a click in its band (but
        // off a row) is absorbed, a click in the light-dismiss layer (to
        // its right) closes it — before any content hit-testing.
        if let Some(overlay) = self.sidebar_overlay {
            if overlay.contains(x, y) {
                return None;
            }
        }
        if self.sidebar_light_dismiss.is_some_and(|r| r.contains(x, y)) {
            return Some(Hot::SidebarLightDismiss);
        }
        // The `GridSplitter` straddles the boundary between the two panes:
        // it must be tested BEFORE the block guarded by `content` (the
        // active pane), since its grab band often overflows `content`.
        if let Some(div) = self.pane_divider {
            let grab = Rect::new(div.left - 8.0, div.top, div.right + 8.0, div.bottom);
            if grab.contains(x, y) {
                return Some(Hot::PaneDivider);
            }
        }
        // Content items only hit inside the content viewport.
        if self.content.contains(x, y) {
            for (i, rect) in self.quick_cards.iter().enumerate() {
                if rect.contains(x, y) {
                    return Some(Hot::QuickCard(i));
                }
            }
            for (i, rect) in self.drive_cards.iter().enumerate() {
                if rect.contains(x, y) {
                    return Some(Hot::DriveCard(i));
                }
            }
            for (i, rect) in self.recent_rows.iter().enumerate() {
                if rect.contains(x, y) {
                    return Some(Hot::RecentRow(i));
                }
            }
            if !self.file_rows.is_empty() && self.file_header.contains(x, y) {
                let [modified_left, type_left, size_left] =
                    file_columns_for(&self.file_header, self.recycle_columns);
                let col = if x < modified_left {
                    0
                } else if x < type_left {
                    1
                } else if x < size_left {
                    2
                } else {
                    3
                };
                return Some(Hot::FileHeaderCol(col));
            }
            for (i, rect) in self.file_rows.iter().enumerate() {
                if rect.contains(x, y) {
                    // The Grid/Cards selection checkbox: its zone (top-left
                    // corner, 6 margin, 18 box — `@ui/Checkbox`'s size) is
                    // clicked separately and toggles the selection.
                    if self.grid_checkboxes {
                        let bx = Rect::new(rect.left + 6.0, rect.top + 6.0, rect.left + 24.0, rect.top + 24.0);
                        if bx.contains(x, y) {
                            return Some(Hot::FileCheckbox(i));
                        }
                    }
                    return Some(Hot::FileRow(i));
                }
            }
            for (c, (pane, rows)) in self.column_panes.iter().enumerate() {
                if !pane.contains(x, y) {
                    continue;
                }
                for (i, rect) in rows.iter().enumerate() {
                    if rect.contains(x, y) {
                        return Some(Hot::ColumnRow(c, i));
                    }
                }
            }
            if let Some(ap) = &self.appearance {
                for (i, rect) in ap.swatches.iter().enumerate() {
                    if rect.contains(x, y) {
                        return Some(Hot::ThemeSwatch(i));
                    }
                }
            }
            for (i, rect) in self.setting_rows.iter().enumerate() {
                if rect.contains(x, y) {
                    return Some(Hot::SettingRow(i));
                }
            }
            for (i, rect) in self.settings_nav.iter().enumerate() {
                if rect.contains(x, y) {
                    return Some(Hot::SettingsNav(i));
                }
            }
        }
        if self.other_pane_rect.is_some_and(|r| r.contains(x, y)) {
            return Some(Hot::InactivePane);
        }
        if self.info_pane.is_some() {
            // The Properties button is part of the pane's scrolling FLOW:
            // its zone is captured by the drawing (`info_properties_rect`)
            // and overridden in main_window, not here.
            for (i, rect) in self.info_tabs.iter().enumerate() {
                if rect.contains(x, y) {
                    return Some(Hot::InfoTab(i));
                }
            }
        }
        if self.shelf_pane.is_some() {
            if self.shelf_clear.contains(x, y) {
                return Some(Hot::ShelfClear);
            }
            for (i, rect) in self.shelf_items.iter().enumerate() {
                // The × button occupies the hovered row's right 28 DIP.
                let remove = Rect::new(rect.right - 28.0, rect.top, rect.right, rect.bottom);
                if remove.contains(x, y) {
                    return Some(Hot::ShelfItemRemove(i));
                }
                if rect.contains(x, y) {
                    return Some(Hot::ShelfItem(i));
                }
            }
        }
        None
    }
}

/// The TabContainer box: the tab item minus its two 4-DIP side columns
/// (LeftColumn/RightColumn of the template, which host the RadiusRenderArc
/// paths). Everything the tab paints and lays out lives in here — background,
/// icon, title, close button — so it is inset 4 DIP from the item's walls.
///
/// This inset is what makes the ACTIVE tab visibly detached from a hovered
/// neighbor: the active tab's TabGeometry runs to the item's wall, while the
/// neighbor's hover fill starts 4 DIP further in.
pub(crate) fn tab_container_rect(tab: &Rect) -> Rect {
    Rect::new(tab.left + 4.0, tab.top, tab.right - 4.0, tab.bottom)
}

/// Content box: TabContainer inset by `TabViewItemHeaderPadding` (8,3,4,3).
/// So the icon box lands on item.left + 12 and the title on + 38
/// (= 12 + 16 icon + 10 icon margin), both verified on the compiled original,
/// and the close button keeps its 4 DIP of air before the container's edge.
pub(crate) fn tab_content_rect(tab: &Rect) -> Rect {
    let c = tab_container_rect(tab);
    Rect::new(c.left + 8.0, c.top + 3.0, c.right - 4.0, c.bottom - 3.0)
}

/// CloseButton: 32×24 (TabViewItemHeaderCloseButtonWidth/Height), at the
/// right edge of the content box, vertically centered.
pub(crate) fn tab_close_rect(tab: &Rect) -> Rect {
    let c = tab_content_rect(tab);
    let cy = (c.top + c.bottom) / 2.0;
    Rect::new(c.right - 32.0, cy - 12.0, c.right, cy + 12.0)
}

pub(crate) fn quick_access_glyph(kind: QuickAccessKind) -> (&'static str, D2D1_COLOR_F) {
    let folder_yellow = D2D1_COLOR_F { r: 1.0, g: 0.80, b: 0.36, a: 1.0 };
    let blue = D2D1_COLOR_F { r: 0.30, g: 0.62, b: 0.94, a: 1.0 };
    match kind {
        QuickAccessKind::Downloads => (GLYPH_DOWNLOAD, blue),
        QuickAccessKind::Desktop => (GLYPH_DESKTOP, blue),
        QuickAccessKind::Documents => (GLYPH_DOCUMENT, folder_yellow),
        QuickAccessKind::Pictures => (GLYPH_PICTURE, blue),
        QuickAccessKind::Videos => (GLYPH_VIDEO, blue),
        QuickAccessKind::Music => (GLYPH_MUSIC, blue),
        QuickAccessKind::RecycleBin => (GLYPH_RECYCLE, blue),
        QuickAccessKind::Generic => (GLYPH_FOLDER, folder_yellow),
    }
}

