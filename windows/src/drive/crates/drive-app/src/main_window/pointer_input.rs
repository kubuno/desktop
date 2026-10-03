#![allow(unused_imports)]
//! Sous-module de `MainWindow` — voir `main_window/mod.rs`.
use windows::core::{w, Result};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{
    DwmExtendFrameIntoClientArea, DwmSetWindowAttribute,
    DWMWA_CAPTION_COLOR, DWMWA_SYSTEMBACKDROP_TYPE, DWMWA_USE_IMMERSIVE_DARK_MODE,
};
use windows::Win32::Graphics::Gdi::{InvalidateRect, ScreenToClient, ValidateRect};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::{MARGINS, WM_MOUSELEAVE};
use windows::Win32::UI::HiDpi::{GetDpiForWindow, GetSystemMetricsForDpi};
use windows::Win32::UI::Input::KeyboardAndMouse::ReleaseCapture;
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::graphics::Renderer;
use crate::services::storage::IconCache;
use crate::data::items::HomeModel;
use crate::view_models::shell_view_model::{browsable, shell_open, Location, TabGroup};
use crate::styles::theme::{Theme, ThemeMode};
use crate::ui::{Hot, Layout, SidebarEntry, UiState, TAB_BAR_HEIGHT};
use super::*;

impl MainWindow {
    pub(crate) fn on_right_click(&mut self, x_px: f32, y_px: f32) {
        let layout = self.layout();
        let target = layout.hit_test(self.to_dip(x_px), self.to_dip(y_px));
        // Back/Forward history flyouts (BackHistoryFlyout/ForwardHistoryFlyout).
        match target {
            Some(Hot::NavBack) | Some(Hot::NavForward) => {
                self.show_history_menu(target == Some(Hot::NavForward), x_px, y_px);
                return;
            }
            // TabFlyout: the per-tab context menu of TabBar.xaml.
            Some(Hot::Tab(i)) | Some(Hot::TabClose(i)) => {
                self.show_tab_context_menu(i, x_px, y_px);
                return;
            }
            _ => {}
        }
        // The sidebar has its own context flyout in the original (SidebarView),
        // the file area shares ContentPageContextFlyoutFactory: the SAME factory
        // builds the item menu and the empty-space menu, the latter being the
        // one where `itemsSelected` is false.
        match target {
            Some(Hot::SidebarItem(i)) => {
                let path = match layout.sidebar_items.get(i) {
                    Some((_, SidebarEntry::Pinned(idx))) => {
                        Some(self.model.quick_access[*idx].path.clone())
                    }
                    Some((_, SidebarEntry::Drive(idx))) => {
                        Some(format!("{}:\\", self.model.drives[*idx].letter))
                    }
                    _ => None,
                };
                if let Some(path) = path {
                    self.show_sidebar_context_menu(path, x_px, y_px);
                }
            }
            Some(Hot::FileRow(i)) | Some(Hot::FileCheckbox(i)) => {
                // Right-clicking an item OUTSIDE the selection reduces it to
                // that item, but preserves an existing multi-selection
                // (Explorer behavior).
                if !self.state.active().selected.contains(&i) {
                    self.state.active_mut().select_single(i);
                }
                let path = self.state.active().entries[i].path.clone();
                self.show_item_context_menu(path, x_px, y_px);
            }
            Some(Hot::QuickCard(i)) => {
                let path = self.model.quick_access[i].path.clone();
                self.show_item_context_menu(path, x_px, y_px);
            }
            Some(Hot::DriveCard(i)) => {
                let path = format!("{}:\\", self.model.drives[i].letter);
                self.show_item_context_menu(path, x_px, y_px);
            }
            // Empty space of a folder view: the no-selection menu.
            _ if matches!(
                self.state.active().location,
                Location::Dir(_) | Location::RecycleBin | Location::SearchResults { .. }
            ) && layout.content.contains(self.to_dip(x_px), self.to_dip(y_px)) =>
            {
                self.state.active_mut().clear_selection();
                self.show_empty_space_menu(x_px, y_px);
            }
            _ => {}
        }
    }

    /// Positions a flyout so it stays inside the window, like a WinUI popup.
    pub(crate) fn on_mouse_move(&mut self, x_px: f32, y_px: f32) {
        if self.tab_drag.is_some() {
            self.on_tab_drag(self.to_dip(x_px), self.to_dip(y_px));
            return;
        }
        // Dragging the sidebar's ScrollBar thumb.
        if let Some(grab) = self.state.sidebar_scroll_drag {
            let layout = self.layout();
            if let Some(bar) = &layout.sidebar_scrollbar {
                let band = crate::ui::Rect::new(
                    0.0,
                    TAB_BAR_HEIGHT + crate::ui::TOOLBAR_HEIGHT + 8.0,
                    crate::ui::sidebar_width(),
                    layout.height - 56.0,
                );
                let scroll = bar
                    .scroll_at(&band, layout.sidebar_extent, self.to_dip(y_px), grab)
                    .clamp(0.0, (layout.sidebar_extent - (band.bottom - band.top)).max(0.0));
                self.state.sidebar_scroll = scroll;
                self.invalidate();
            }
            return;
        }
        // Dragging the sidebar's HORIZONTAL ScrollBar thumb.
        if let Some(grab) = self.state.sidebar_hscroll_drag {
            let layout = self.layout();
            if let Some(bar) = &layout.sidebar_hscrollbar {
                let band = crate::ui::Rect::new(
                    0.0,
                    TAB_BAR_HEIGHT + crate::ui::TOOLBAR_HEIGHT + 8.0,
                    crate::ui::sidebar_width(),
                    layout.height - 56.0,
                );
                let scroll = bar
                    .scroll_at(&band, layout.sidebar_hextent, self.to_dip(x_px), grab)
                    .clamp(0.0, (layout.sidebar_hextent - (band.right - band.left)).max(0.0));
                self.state.sidebar_hscroll = scroll;
                self.invalidate();
            }
            return;
        }
        // The pointer in the sidebar's gutter: its bar unfolds.
        {
            let (x, y) = (self.to_dip(x_px), self.to_dip(y_px));
            let top = TAB_BAR_HEIGHT + crate::ui::TOOLBAR_HEIGHT + 8.0;
            let w = crate::ui::sidebar_width();
            let bottom = self.layout().height - 56.0;
            let size = crate::user_controls::scrollbar::SCROLLBAR_SIZE;
            // The RIGHT gutter (vertical bar) OR the BOTTOM gutter
            // (horizontal bar) unfolds the indicator.
            let in_gutter = self.state.sidebar_visible
                && x < w
                && y > top
                && y < bottom
                && (x > w - size || y > bottom - size);
            if in_gutter != self.state.sidebar_scrollbar_expanded {
                self.state.sidebar_scrollbar_expanded = in_gutter;
                self.invalidate();
            }
        }

        // Dragging the `ScrollBar`'s thumb.
        if let Some(grab) = self.state.scroll_drag {
            let layout = self.layout();
            if let Some(bar) = &layout.scrollbar {
                let pos = if bar.horizontal { self.to_dip(x_px) } else { self.to_dip(y_px) };
                let scroll = bar
                    .scroll_at(&layout.content, layout.content_extent, pos, grab)
                    .clamp(0.0, layout.max_scroll());
                self.state.active_mut().scroll = scroll;
                self.invalidate();
            }
            return;
        }
        // The pointer enters (or leaves) the gutter: the bar unfolds or
        // folds back. It stays unfolded as long as we're in it.
        {
            let layout = self.layout();
            let (x, y) = (self.to_dip(x_px), self.to_dip(y_px));
            // The gutter is computed the same on the FOLDED bar as on the
            // unfolded one: it's the 12-DIP band at the scrollable area's edge.
            let gutter = layout.scrollbar.as_ref().map(|b| {
                let c = &layout.content;
                if b.horizontal {
                    crate::ui::Rect::new(
                        c.left,
                        c.bottom - crate::user_controls::scrollbar::SCROLLBAR_SIZE,
                        c.right,
                        c.bottom,
                    )
                } else {
                    crate::ui::Rect::new(
                        c.right - crate::user_controls::scrollbar::SCROLLBAR_SIZE,
                        c.top,
                        c.right,
                        c.bottom,
                    )
                }
            });
            let expanded = gutter.is_some_and(|g| g.contains(x, y));
            if expanded != self.state.scrollbar_expanded {
                self.state.scrollbar_expanded = expanded;
                self.invalidate();
            }
        }

        // `SidebarResizer_ManipulationDelta`: width at grab + translation.
        // The info pane, on the other hand, is on the right: it GROWS leftward.
        if let Some((hot, press_x, start_w)) = self.pane_drag {
            if hot == Hot::PaneDivider {
                // Ratio = cursor position within the two panes' region
                // (their union, independent of the current ratio).
                let layout = self.layout();
                if let (Some(other), content) = (layout.other_pane_rect, layout.content) {
                    let group = self.state.group();
                    let ratio = if group.split_vertical {
                        let lo = content.left.min(other.left);
                        let hi = content.right.max(other.right);
                        (self.to_dip(x_px) - lo) / (hi - lo).max(1.0)
                    } else {
                        let lo = content.top.min(other.top);
                        let hi = content.bottom.max(other.bottom);
                        (self.to_dip(y_px) - lo) / (hi - lo).max(1.0)
                    };
                    self.state.group_mut().split_ratio = ratio.clamp(0.05, 0.95);
                    self.invalidate();
                }
                return;
            }
            let delta = self.to_dip(x_px) - press_x;
            if hot == Hot::SidebarResizer {
                self.set_sidebar_width(start_w + delta);
            } else {
                self.set_info_pane_width(start_w - delta);
            }
            self.invalidate();
            return;
        }
        // ColorPicker: the drag follows the mouse over the armed zone
        // (square, sliders), the color applies continuously.
        if let Some(panel) = self.state.flyout.as_ref().and_then(|f| f.picker) {
            if let Some(zone) = panel.drag {
                let (x, y) = (self.to_dip(x_px), self.to_dip(y_px));
                let (px, py) = {
                    let f = self.state.flyout.as_ref().unwrap();
                    (f.x, f.y)
                };
                let mut p = panel;
                p.apply(zone, px, py, x, y);
                if p != panel {
                    if let Some(f) = self.state.flyout.as_mut() {
                        f.picker = Some(p);
                    }
                    self.apply_picker_color(&p);
                    self.invalidate();
                }
                return;
            }
        }
        // « Disposition » panel: hovering the cards / toggles, and dragging
        // the size slider while the button is held down.
        if let Some(panel) = self.state.flyout.as_ref().and_then(|f| f.layout) {
            let (x, y) = (self.to_dip(x_px), self.to_dip(y_px));
            let (px, py) = {
                let f = self.state.flyout.as_ref().unwrap();
                (f.x, f.y)
            };
            let hit = panel.hit(px, py, x, y);
            if panel.dragging {
                let size = panel.size_at(px, py, x);
                if size != panel.size {
                    self.set_layout_size(panel.mode, size);
                    if let Some(p) = self.state.flyout.as_mut().and_then(|f| f.layout.as_mut()) {
                        p.size = size;
                    }
                    self.invalidate();
                }
                return;
            }
            if panel.hot != Some(hit) {
                if let Some(p) = self.state.flyout.as_mut().and_then(|f| f.layout.as_mut()) {
                    p.hot = Some(hit);
                }
                self.invalidate();
            }
            return;
        }

        if let Some(flyout) = &mut self.state.flyout {
            use crate::ui::FlyoutHit;
            let (x, y) = (x_px * 96.0 / self.dpi, y_px * 96.0 / self.dpi);
            let hit = flyout.hit(x, y);
            let new_hot = match hit {
                FlyoutHit::Item(i) => Some((false, i)),
                FlyoutHit::SubItem(i) => Some((true, i)),
                // Staying on the 3rd level keeps its parent (level 2) highlighted.
                FlyoutHit::SubSubItem(_) => flyout.subsubmenu.map(|p| (true, p)),
                _ => None,
            };
            let new_hot_sub2 = match hit {
                FlyoutHit::SubSubItem(i) => Some(i),
                _ => None,
            };
            let new_hot_primary = match hit {
                FlyoutHit::Primary(i) => Some(i),
                _ => None,
            };
            let mut dirty = new_hot_primary != flyout.hot_primary;
            flyout.hot_primary = new_hot_primary;
            // Hovering a parent row opens its submenu; hovering any other row
            // closes the one that was open (WinUI's MenuFlyoutSubItem behaviour).
            if let Some((false, i)) = new_hot {
                if flyout.items[i].has_submenu {
                    flyout.set_submenu(Some(i));
                } else if flyout.submenu.is_some() {
                    flyout.set_submenu(None);
                }
            }
            // Same logic one level down: a submenu item that itself has
            // children (e.g. « Date de modification ») unfolds the 3rd level.
            if let FlyoutHit::SubItem(i) = hit {
                let has = flyout
                    .sub_items()
                    .and_then(|e| e.get(i))
                    .is_some_and(|it| it.has_submenu);
                if has {
                    flyout.set_subsubmenu(Some(i));
                } else if flyout.subsubmenu.is_some() {
                    flyout.set_subsubmenu(None);
                }
            }
            dirty |= new_hot != flyout.hot || new_hot_sub2 != flyout.hot_sub2;
            flyout.hot = new_hot;
            flyout.hot_sub2 = new_hot_sub2;
            if dirty {
                self.invalidate();
            }
            return;
        }
        if !self.mouse_tracking {
            let mut tme = windows::Win32::UI::Input::KeyboardAndMouse::TRACKMOUSEEVENT {
                cbSize: std::mem::size_of::<windows::Win32::UI::Input::KeyboardAndMouse::TRACKMOUSEEVENT>() as u32,
                dwFlags: windows::Win32::UI::Input::KeyboardAndMouse::TME_LEAVE,
                hwndTrack: self.hwnd,
                dwHoverTime: 0,
            };
            unsafe {
                let _ = windows::Win32::UI::Input::KeyboardAndMouse::TrackMouseEvent(&mut tme);
            }
            self.mouse_tracking = true;
        }

        self.mouse_dip = (self.to_dip(x_px), self.to_dip(y_px));
        let layout = self.layout();
        let hot = layout.hit_test(self.mouse_dip.0, self.mouse_dip.1);
        let hot = self.override_pane_hit(hot, &layout, self.mouse_dip.0, self.mouse_dip.1);
        if hot != self.state.hot {
            self.state.hot = hot;
            // Hover change: close the tooltip and restart the opening
            // delay, the way `ToolTipService` re-arms its timer when the
            // pointer moves from one element to another.
            self.tooltip_shown = None;
            unsafe {
                let _ = KillTimer(Some(self.hwnd), TOOLTIP_TIMER);
                if hot.is_some_and(|h| crate::ui::tooltip_for(h).is_some()) {
                    let _ = SetTimer(Some(self.hwnd), TOOLTIP_TIMER, TOOLTIP_DELAY_MS, None);
                }
            }
            self.invalidate();
        }
        // During an active drag, the ghost follows the pointer: we redraw
        // on every move, even without a hover change.
        if self.item_drag.as_ref().is_some_and(|(_, _, _, a)| *a) {
            self.invalidate();
        }
    }

    pub(crate) fn on_click(&mut self, x_px: f32, y_px: f32) {
        let layout = self.layout();
        let target = layout.hit_test(self.to_dip(x_px), self.to_dip(y_px));
        let target =
            self.override_pane_hit(target, &layout, self.to_dip(x_px), self.to_dip(y_px));
        // The StatusCenter flyout is light-dismiss, like the original.
        if self.state.status_center_open && target != Some(Hot::StatusCenter) {
            self.state.status_center_open = false;
            self.invalidate();
        }
        match target {
            // A suggestion row is handled where the omnibar edit state lives
            // (`WM_LBUTTONDOWN`); the panel's own surface eats the click so it
            // cannot reach the toolbar or the listing underneath.
            Some(Hot::Suggestion(_) | Hot::SuggestionPanel) => {}
            // The handles are dragged, they don't activate on click.
            Some(Hot::SidebarResizer | Hot::InfoPaneResizer | Hot::PaneDivider) => {}
            Some(Hot::ScrollThumb) => {}
            Some(Hot::SidebarScrollThumb) => {}
            Some(Hot::SidebarHScrollThumb) => {}
            // The `ContentDialog`: primary executes, close dismisses. The
            // conflict dialog takes priority (it's modal above the others).
            Some(Hot::DialogPrimary) if self.state.conflict.is_some() => self.conflict_continue(),
            Some(Hot::DialogClose) if self.state.conflict.is_some() => {
                // Cancel = ignore everything (the operation doesn't happen).
                self.state.conflict = None;
                self.invalidate();
            }
            // Cycles the "apply to all" option and propagates it.
            Some(Hot::ConflictApplyAll) => {
                if let Some(c) = self.state.conflict.as_mut() {
                    let next = c.aggregated.unwrap_or(crate::dialogs::ConflictResolve::Skip).cycled();
                    c.apply_to_all(next);
                }
                self.invalidate();
            }
            // Cycles an item's option and recomputes "apply to all".
            Some(Hot::ConflictOption(i)) => {
                if let Some(c) = self.state.conflict.as_mut() {
                    if let Some(item) = c.items.get_mut(i) {
                        item.resolve = item.resolve.cycled();
                    }
                    c.sync_aggregated();
                }
                self.invalidate();
            }
            Some(Hot::DialogPrimary) => self.dialog_primary(),
            Some(Hot::DialogClose) => {
                self.state.dialog = None;
                if self.state.edit.as_ref().is_some_and(|e| e.entry == crate::ui::EDIT_DIALOG) {
                    self.state.edit = None;
                }
                self.invalidate();
            }
            Some(Hot::DialogChoice(i)) => {
                if let Some(d) = self.state.dialog.as_mut() {
                    d.choice = i;
                }
                self.invalidate();
            }
            Some(Hot::DialogCheckbox) => {
                if let Some(d) = self.state.dialog.as_mut() {
                    d.checkbox = !d.checkbox;
                }
                self.invalidate();
            }
            // `ListView_ItemClick` of the AddItemDialog: the click chains
            // into the naming dialog for the chosen item.
            Some(Hot::DialogItem(i)) => {
                if self
                    .state
                    .dialog
                    .as_ref()
                    .is_some_and(|d| d.action == crate::dialogs::DialogAction::AddItem)
                {
                    self.state.dialog = None;
                    match i {
                        0 => self.open_create_item_dialog(true),
                        1 => self.open_create_item_dialog(false),
                        _ => self.open_create_shortcut_dialog(),
                    }
                }
                self.invalidate();
            }
            // A click in the sidebar's track: one page of its viewport.
            Some(Hot::SidebarScrollTrack) => {
                let layout = self.layout();
                let Some(bar) = &layout.sidebar_scrollbar else { return };
                let top = TAB_BAR_HEIGHT + crate::ui::TOOLBAR_HEIGHT + 8.0;
                let viewport = layout.height - 56.0 - top;
                let max = (layout.sidebar_extent - viewport).max(0.0);
                let pos = self.to_dip(y_px);
                let next = if pos < bar.thumb.top {
                    (self.state.sidebar_scroll - viewport).clamp(0.0, max)
                } else {
                    (self.state.sidebar_scroll + viewport).clamp(0.0, max)
                };
                self.state.sidebar_scroll = next;
                self.mark_sidebar_scrolled();
                self.invalidate();
            }
            // A click in the HORIZONTAL track: one page wide.
            Some(Hot::SidebarHScrollTrack) => {
                let layout = self.layout();
                let Some(bar) = &layout.sidebar_hscrollbar else { return };
                let viewport = crate::ui::sidebar_width();
                let max = (layout.sidebar_hextent - viewport).max(0.0);
                let pos = self.to_dip(x_px);
                let next = if pos < bar.thumb.left {
                    (self.state.sidebar_hscroll - viewport).clamp(0.0, max)
                } else {
                    (self.state.sidebar_hscroll + viewport).clamp(0.0, max)
                };
                self.state.sidebar_hscroll = next;
                self.mark_sidebar_scrolled();
                self.invalidate();
            }
            // A click in the track scrolls one page, on the clicked side —
            // this is what a `ScrollBar`'s `RepeatButton`s do.
            Some(Hot::ScrollTrack) => {
                let layout = self.layout();
                let Some(bar) = &layout.scrollbar else { return };
                let (pos, thumb_start, page) = if bar.horizontal {
                    (
                        self.to_dip(x_px),
                        bar.thumb.left,
                        layout.content.right - layout.content.left,
                    )
                } else {
                    (
                        self.to_dip(y_px),
                        bar.thumb.top,
                        layout.content.bottom - layout.content.top,
                    )
                };
                let max = layout.max_scroll();
                let tab = self.state.active_mut();
                tab.scroll = if pos < thumb_start {
                    (tab.scroll - page).clamp(0.0, max)
                } else {
                    (tab.scroll + page).clamp(0.0, max)
                };
                self.mark_scrolled();
                self.invalidate();
            }
            Some(Hot::CaptionClose) => unsafe {
                let _ = PostMessageW(Some(self.hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
            },
            Some(Hot::CaptionMin) => unsafe {
                let _ = ShowWindow(self.hwnd, SW_MINIMIZE);
            },
            Some(Hot::CaptionMax) => unsafe {
                let _ = ShowWindow(self.hwnd, if self.state.maximized { SW_RESTORE } else { SW_MAXIMIZE });
            },
            Some(Hot::PaneToggle) => {
                // Port of the TabBar "TabActions" flyout (custom Fluent overlay).
                let compact_key = if self.compact_overlay.is_some() {
                    "ExitCompactOverlay"
                } else {
                    "EnterCompactOverlay"
                };
                self.state.flyout = Some(crate::ui::Flyout {
                    kind: crate::ui::FlyoutKind::TabActions,
                    width: crate::ui::FLYOUT_WIDTH,
                    x: 10.0,
                    y: TAB_BAR_HEIGHT + 2.0,
                    items: vec![
                        crate::ui::FlyoutItem {
                            glyph: "\u{E78B}",
                            label: drive_localization::tr("NewWindow").to_string(),
                            accel: Some("Ctrl+N".to_string()),
                            enabled: true,
                            has_submenu: false,
                            icon: None,
                            bitmap: None,
                            separator: false,
                            is_toggle: false,
                            checked: false,
                            pill: false,
                            command: crate::ui::MenuCommand::None,
                            children: Vec::new(),
                            children_width: crate::ui::SUBMENU_WIDTH,
                        },
                        crate::ui::FlyoutItem {
                            glyph: "\u{EE49}",
                            label: drive_localization::tr(compact_key).to_string(),
                            accel: None,
                            enabled: true,
                            has_submenu: false,
                            icon: None,
                            bitmap: None,
                            separator: false,
                            is_toggle: false,
                            checked: false,
                            pill: false,
                            command: crate::ui::MenuCommand::None,
                            children: Vec::new(),
                            children_width: crate::ui::SUBMENU_WIDTH,
                        },
                        crate::ui::FlyoutItem::new(
                            drive_localization::tr("SplitPane").to_string(),
                            true,
                        )
                        .with_glyph("\u{E784}")
                        .with_children(vec![
                            crate::ui::FlyoutItem::new(
                                drive_localization::tr("Vertical").to_string(),
                                true,
                            )
                            .with_glyph("\u{E784}"),
                            crate::ui::FlyoutItem::new(
                                drive_localization::tr("Horizontal").to_string(),
                                true,
                            )
                            .with_glyph("\u{E76F}"),
                        ]),
                    ],
                    primary: Vec::new(),
                    hot_primary: None,
                    submenu: None,
                    opened: std::time::Instant::now(),
                    submenu_opened: None,
                    sub_pos: None,
                    subsubmenu: None,
                    subsubmenu_opened: None,
                    subsub_pos: None,
                    hot_sub2: None,
                    layout: None,
                    picker: None,
                    hot: None,
                    path: None,
                });
                // `CloseActivePaneAction`: offered only in multi-pane
                // (`IsMultiPaneActive`), after the « Diviser le volet » submenu.
                if self.state.group().panes.len() == 2 {
                    if let Some(f) = self.state.flyout.as_mut() {
                        f.items.push(
                            crate::ui::FlyoutItem::new(
                                drive_localization::tr("CloseActivePane").to_string(),
                                true,
                            )
                            .with_glyph("\u{E89F}"),
                        );
                    }
                }
                self.invalidate();
            }
            Some(Hot::NewTab) => {
                self.state.tabs.push(TabGroup::new_home());
                self.state.active_tab = self.state.tabs.len() - 1;
                self.invalidate();
            }
            Some(Hot::Tab(i)) => {
                self.state.active_tab = i;
                self.invalidate();
            }
            Some(Hot::TabClose(i)) => {
                self.close_tab(i);
            }
            Some(Hot::NavBack) => {
                self.state.active_mut().go_back();
                self.invalidate();
            }
            Some(Hot::NavForward) => {
                self.state.active_mut().go_forward();
                self.invalidate();
            }
            Some(Hot::NavUp) => {
                self.state.active_mut().go_up();
                self.invalidate();
            }
            Some(Hot::CmdNew) => self.on_click_cmd_new(&layout),
            Some(Hot::CmdCut) | Some(Hot::CmdCopy) => {
                let cut = target == Some(Hot::CmdCut);
                let paths = self.state.active().selected_paths();
                if !paths.is_empty() {
                    crate::utils::storage::clipboard_set_files(self.hwnd, &paths, cut);
                }
            }
            Some(Hot::CmdPaste) => self.paste_clipboard(),
            Some(Hot::CmdRename) => self.begin_rename(),
            // The separator isn't clickable.
            Some(Hot::CmdSeparator) => {}
            Some(Hot::CmdShare) => {
                let path = self.state.active().selected_paths().into_iter().next();
                self.run_menu_command(crate::ui::MenuCommand::ShareItem, path);
            }
            Some(Hot::CmdProperties) => {
                // `OpenProperties`: on the selection, or the current folder.
                let path = self
                    .state
                    .active()
                    .selected_paths()
                    .into_iter()
                    .next()
                    .or_else(|| match &self.state.active().location {
                        crate::view_models::shell_view_model::Location::Dir(p) => {
                            Some(p.to_string_lossy().into_owned())
                        }
                        _ => None,
                    });
                self.run_menu_command(crate::ui::MenuCommand::OpenProperties, path);
            }
            Some(Hot::CmdInfoPane) => {
                crate::services::settings::update(|s| s.show_info_pane = !s.show_info_pane);
                self.invalidate();
            }
            Some(Hot::CmdShelf) => {
                // `ToggleShelfPaneAction`: toggles the Shelf's visibility.
                crate::services::settings::update(|s| s.show_shelf_pane = !s.show_shelf_pane);
                self.invalidate();
            }
            Some(Hot::ShelfClear) => {
                // `ClearItemsCommand`: empties the Shelf.
                self.shelf.clear();
                self.invalidate();
            }
            Some(Hot::ShelfItemRemove(i)) => {
                // `ShelfItem.Remove()`: removes the item from the list.
                self.shelf.remove(i);
                self.invalidate();
            }
            Some(Hot::ShelfItem(i)) => {
                // `ViewInFolderAsync`: navigates to the item's parent folder.
                if let Some(it) = self.shelf.items.get(i) {
                    if let Some(parent) = std::path::Path::new(&it.path).parent() {
                        self.navigate_active(Location::Dir(parent.to_path_buf()));
                    }
                }
                self.invalidate();
            }
            Some(Hot::CmdFilter) => {
                // ToggleFilterHeader: opens the omnibar filter box.
                self.begin_filter_edit();
            }
            Some(Hot::CmdSelOptions) => self.on_click_cmd_sel_options(&layout),
            Some(Hot::InfoTab(i)) => {
                // ToggleDetailsPane / TogglePreviewPane → SelectedTab.
                use crate::services::settings::InfoPaneTab;
                crate::services::settings::update(|s| {
                    s.info_pane_tab = if i == 1 { InfoPaneTab::Preview } else { InfoPaneTab::Details };
                });
                self.invalidate();
            }
            // « Modifier les étiquettes » (pane): flyout of the defined
            // tags, checked according to the item — the hit area is recorded
            // by the drawing pass (its position depends on measured text
            // heights and scrolling; the click must also land INSIDE the pane).
            _ if layout.info_pane.is_some_and(|p| {
                p.contains(self.to_dip(x_px), self.to_dip(y_px))
            }) && self.state.info_edit_tags_rect.get().is_some_and(|(l, t, r, b)| {
                let (x, y) = (self.to_dip(x_px), self.to_dip(y_px));
                x >= l && x <= r && y >= t && y <= b
            }) =>
            {
                self.open_edit_tags_flyout(x_px, y_px);
            }
            Some(Hot::InfoProperties) => {
                // Our ported properties window (like the Properties button
                // in the original's info pane).
                let tab = self.state.active();
                let path = tab
                    .selected_entry()
                    .map(|e| e.path.clone())
                    .or_else(|| match &tab.location {
                        Location::Dir(p) => Some(p.to_string_lossy().into_owned()),
                        _ => None,
                    });
                if let Some(path) = path {
                    use crate::views::properties::PropertiesTarget;
                    let target = if is_drive_root(&path) {
                        PropertiesTarget::Drive(path.chars().next().unwrap_or('C'))
                    } else {
                        PropertiesTarget::Path(path)
                    };
                    crate::views::properties::open_properties_window(self.hwnd, target);
                }
            }
            Some(Hot::CmdDelete) => {
                let paths = self.state.active().selected_paths();
                if !paths.is_empty() && crate::utils::storage::delete_items(&paths, false) {
                    self.state.active_mut().refresh();
                    self.invalidate();
                }
            }
            // Recycle bin — the three buttons of the command bar's
            // « RecycleBin » context (`ToolbarSections.cs:51-56`). Same
            // delegation as `run_menu_command`, but a greyed-out button
            // (`IsExecutable` false) stays inert, like the disabled AppBarButton.
            Some(Hot::CmdEmptyRecycleBin)
            | Some(Hot::CmdRestoreAllRecycleBin)
            | Some(Hot::CmdRestoreRecycleBin) => {
                use crate::actions::Action;
                let action: &dyn Action = match target {
                    Some(Hot::CmdEmptyRecycleBin) => &crate::actions::file_system::EmptyRecycleBin,
                    Some(Hot::CmdRestoreAllRecycleBin) => {
                        &crate::actions::file_system::RestoreAllRecycleBin
                    }
                    _ => &crate::actions::file_system::RestoreRecycleBin,
                };
                if action.is_executable(self) {
                    action.execute(self, None);
                }
            }
            Some(Hot::CmdSort) => self.on_click_cmd_sort(&layout),
            Some(Hot::Hamburger) => {
                // In Minimal mode, the hamburger toggles `IsPaneOpen`
                // (MinimalCollapsed ⇄ MinimalExpanded): the pane slides.
                if self.layout().sidebar_mode == crate::ui::SidebarMode::Minimal {
                    self.state.sidebar_pane_open = !self.state.sidebar_pane_open;
                    let target = if self.state.sidebar_pane_open {
                        0.0
                    } else {
                        -crate::ui::SIDEBAR_OPEN_PANE_LENGTH
                    };
                    self.state.sidebar_pane_animate_to(target);
                    self.start_sidebar_animation();
                } else if !self.state.sidebar_visible {
                    // `SidebarPaneToggleButton`: toggles Expanded ⇄ Compact
                    // (`IsSidebarOpen`), the pane doesn't hide.
                    self.state.sidebar_visible = true;
                } else {
                    crate::services::settings::update(|s| s.sidebar_compact = !s.sidebar_compact);
                }
                self.invalidate();
            }
            Some(Hot::SidebarLightDismiss) => {
                // `PaneLightDismissLayer`: closes the Minimal pane.
                self.state.sidebar_pane_open = false;
                self.state.sidebar_pane_animate_to(-crate::ui::SIDEBAR_OPEN_PANE_LENGTH);
                self.start_sidebar_animation();
                self.invalidate();
            }
            Some(Hot::StatusCenter) => {
                // ShowStatusCenterButton: the Status Center's flyout.
                self.state.status_center_open = !self.state.status_center_open;
                self.invalidate();
            }
            Some(Hot::StatusGitActions) => {
                if let Some(anchor) = self.layout().status_git_actions {
                    self.open_git_actions_flyout(anchor);
                }
            }
            Some(Hot::StatusGitBranch) => {
                if let Some(anchor) = self.layout().status_git_branch {
                    self.open_git_branches_flyout(anchor);
                }
            }
            // Omnibar modes: path / command palette / search.
            Some(Hot::OmnibarMode(0)) => {
                self.begin_path_mode();
            }
            Some(Hot::OmnibarMode(1)) => {
                self.begin_palette_mode();
            }
            Some(Hot::OmnibarMode(_)) => {
                // Search mode: live filter.
                let current = self.state.active().filter.clone();
                self.state.edit = Some(crate::ui::EditState {
                    entry: crate::ui::EDIT_SEARCH,
                    caret: current.len(),
                    anchor: 0,
                    text: current,
                });
                self.invalidate();
            }
            Some(Hot::CmdLayout) => self.on_click_cmd_layout(&layout),
            Some(Hot::NavRefresh) => {
                if self.state.active().location == Location::Home {
                    self.model = HomeModel::load();
                } else {
                    self.state.active_mut().refresh();
                }
                self.invalidate();
            }
            Some(Hot::AddressBar) => {
                // EditPath: switches the omnibar to path entry.
                self.begin_path_mode();
            }
            Some(Hot::Breadcrumb(k)) => {
                // Visible segments are offset by `breadcrumb_start`.
                let i = layout.breadcrumb_start + k;
                let segments = self.state.active().breadcrumbs();
                if let Some((_, path)) = segments.get(i) {
                    if !path.as_os_str().is_empty() {
                        self.navigate_active(Location::Dir(path.clone()));
                    }
                }
            }
            Some(Hot::BreadcrumbEllipsis) => {
                // The collapsed head segments: a flyout where each one navigates.
                use crate::ui::FlyoutItem;
                let segments = self.state.active().breadcrumbs();
                let hidden = &segments[..layout.breadcrumb_start.min(segments.len())];
                self.breadcrumb_overflow = hidden.iter().map(|(_, p)| p.clone()).collect();
                let items: Vec<FlyoutItem> = hidden
                    .iter()
                    .map(|(name, _)| FlyoutItem::new(name.clone(), true).with_icon("Folder"))
                    .collect();
                if let Some(ell) = &layout.breadcrumb_ellipsis {
                    let (ex, ey) = (self.to_px(ell.left), self.to_px(ell.bottom));
                    self.open_flyout(crate::ui::FlyoutKind::BreadcrumbOverflow, items, None, ex, ey);
                }
            }
            Some(Hot::SidebarItem(i)) => self.on_click_sidebar_item(i, &layout, x_px),
            Some(Hot::QuickCard(i)) => {
                let item = self.model.quick_access[i].clone();
                self.open_location(&item.path);
            }
            Some(Hot::DriveCard(i)) => {
                let root = format!("{}:\\", self.model.drives[i].letter);
                self.navigate_active(Location::Dir(root.into()));
            }
            Some(Hot::RecentRow(i)) => {
                shell_open(&self.model.recent_files[i].path.clone());
            }
            // `ItemSelected_Checked`/`Unchecked`: the checkbox toggles the
            // item's selection without touching the rest (like Ctrl+click).
            Some(Hot::FileCheckbox(i)) => {
                self.state.active_mut().toggle_select(i);
                self.invalidate();
            }
            Some(Hot::FileRow(i)) => {
                // Explorer's selection: Ctrl toggles, Shift extends from
                // the anchor, a plain click reduces to the item.
                use windows::Win32::UI::Input::KeyboardAndMouse::{
                    GetKeyState, VK_CONTROL, VK_SHIFT,
                };
                let ctrl = unsafe { GetKeyState(VK_CONTROL.0 as i32) } < 0;
                let shift = unsafe { GetKeyState(VK_SHIFT.0 as i32) } < 0;
                let tab = self.state.active_mut();
                if ctrl {
                    tab.toggle_select(i);
                } else if shift {
                    tab.select_range(i);
                } else {
                    tab.select_single(i);
                }
                self.invalidate();
            }
            Some(Hot::InactivePane) => {
                let group = self.state.group_mut();
                group.active_pane = 1 - group.active_pane;
                self.invalidate();
            }
            // `ColumnLayoutPage`: the selection opens the next blade and
            // closes the ones that followed it (`DismissOtherBlades`).
            Some(Hot::ColumnRow(c, i)) => {
                self.state.active_mut().select_column_row(c, i);
                self.invalidate();
            }
            Some(Hot::FileHeaderCol(i)) => {
                let recycle = self.state.active().location == Location::RecycleBin;
                let column = crate::view_models::shell_view_model::detail_columns(recycle)[i].1;
                self.state.active_mut().sort_by(column);
                self.state.active().save_prefs();
                self.invalidate();
            }
            Some(Hot::SettingsNav(i)) => {
                self.state.settings_section = i;
                self.state.active_mut().scroll = 0.0;
                self.invalidate();
            }
            Some(Hot::SettingRow(i)) => {
                self.on_setting_row(i, x_px, y_px);
            }
            Some(Hot::ThemeSwatch(i)) => {
                // AppearanceViewModel.SelectedAppThemeResources setter.
                use crate::views::settings::appearance_page::APP_THEME_RESOURCES;
                if let Some((_, color)) = APP_THEME_RESOURCES.get(i) {
                    crate::services::settings::update(|s| s.app_theme_background_color = (*color).into());
                    self.invalidate();
                }
            }
            None => {}
        }
    }

    /// Port of Enter/ExitCompactOverlay: small always-on-top window.
    pub(crate) fn on_double_click(&mut self, x_px: f32, y_px: f32) {
        let layout = self.layout();
        let target = layout.hit_test(self.to_dip(x_px), self.to_dip(y_px));
        match target {
            Some(Hot::FileRow(_))
                if self.state.active().location == Location::RecycleBin =>
            {
                // "Don't open files and folders inside recycle bin"
                // (`NavigationHelpers.OpenSelectedItemsAsync`).
            }
            Some(Hot::FileRow(i)) => {
                let entry = self.state.active().entries[i].clone();
                if entry.is_dir {
                    self.navigate_active(Location::Dir(entry.path.into()));
                } else {
                    shell_open(&entry.path);
                }
            }
            // In columns, a folder already opens on a single click (the next
            // blade); a double-click therefore only launches a file.
            Some(Hot::ColumnRow(c, i)) => {
                let entry = self
                    .state
                    .active()
                    .columns
                    .get(c)
                    .and_then(|p| p.entries.get(i))
                    .cloned();
                if let Some(entry) = entry {
                    if !entry.is_dir {
                        shell_open(&entry.path);
                    }
                }
            }
            _ => {}
        }
    }

    pub(crate) fn on_wheel(&mut self, delta: i16) {
        let layout = self.layout();
        let max_scroll = layout.max_scroll();
        let tab = self.state.active_mut();
        let new_scroll = (tab.scroll - delta as f32 / 120.0 * 96.0).clamp(0.0, max_scroll);
        if (new_scroll - tab.scroll).abs() > f32::EPSILON {
            tab.scroll = new_scroll;
            self.mark_scrolled();
            self.invalidate();
        }
    }

    /// Wakes up the `ScrollBar` indicator and arms its fade-out.
    pub(crate) fn mark_scrolled(&mut self) {
        self.state.scrolled_at = Some(std::time::Instant::now());
        unsafe {
            SetTimer(Some(self.hwnd), SCROLLBAR_TIMER, 16, None);
        }
    }

    /// Same for the SIDEBAR's ScrollBar.
    pub(crate) fn mark_sidebar_scrolled(&mut self) {
        self.state.sidebar_scrolled_at = Some(std::time::Instant::now());
        unsafe {
            SetTimer(Some(self.hwnd), SCROLLBAR_TIMER, 16, None);
        }
    }

    /// Background paste into the active directory (StatusCenter progress).
    /// Starts renaming the selected entry (F2), selecting the file stem like
    /// Explorer does.
    pub(crate) fn begin_rename(&mut self) {
        let tab = self.state.active();
        let Some(&i) = tab.selected.first() else { return };
        let entry = &tab.entries[i];
        let name = entry.name.clone();
        // Select up to (not including) the extension for files.
        let sel_end = if entry.is_dir {
            name.len()
        } else {
            name.rfind('.').unwrap_or(name.len())
        };
        self.state.edit = Some(crate::ui::EditState {
            entry: i,
            caret: sel_end,
            anchor: 0,
            text: name,
        });
        self.invalidate();
    }

    /// Commits (Enter / click-outside) or cancels (Esc) the rename editor.
    pub(crate) fn end_rename(&mut self, commit: bool) {
        let Some(edit) = self.state.edit.take() else { return };
        if edit.entry == crate::ui::EDIT_PATH {
            self.state.path_suggestions.clear();
            // Omnibar path mode: Enter navigates to the typed path.
            if commit {
                let typed = edit.text.trim().trim_end_matches(['\\', '/']).to_string();
                if browsable(&typed) {
                    self.navigate_active(Location::Dir(typed.into()));
                } else if !typed.is_empty() {
                    shell_open(&typed);
                }
            }
            self.invalidate();
            return;
        }
        if edit.entry == crate::ui::EDIT_PALETTE {
            // Command palette: Enter runs the first matching command.
            let first = self.state.path_suggestions.first().cloned();
            self.state.path_suggestions.clear();
            if commit {
                if let Some((_, target)) = first {
                    if let Some(id) = target.strip_prefix("cmd:") {
                        let id = id.to_string();
                        self.run_palette_command(&id);
                    }
                }
            }
            self.invalidate();
            return;
        }
        if edit.entry == crate::ui::EDIT_SEARCH {
            // Escape: abandons the live filter. Enter in a FOLDER: launches
            // a RECURSIVE search and switches the tab to results
            // (`SubmitSearch` → the `IsSearchResultPage` page).
            if !commit {
                self.state.active_mut().set_filter("");
                crate::services::folder_search::cancel();
            } else {
                let query = edit.text.trim().to_string();
                let root = match &self.state.active().location {
                    Location::Dir(p) => Some(p.to_string_lossy().into_owned()),
                    Location::SearchResults { root, .. } => Some(root.clone()),
                    _ => None,
                };
                if let (false, Some(root)) = (query.is_empty(), root) {
                    self.state.active_mut().set_filter("");
                    self.search_generation += 1;
                    let gen = self.search_generation;
                    self.state.active_mut().navigate(Location::SearchResults {
                        query: query.clone(),
                        root: root.clone(),
                    });
                    crate::services::folder_search::start_search(
                        crate::services::folder_search::SearchQuery::new(query, root),
                        self.hwnd.0 as isize,
                        gen,
                    );
                }
            }
            self.state.edit = None;
            self.invalidate();
            return;
        }
        if commit {
            let tab = self.state.active();
            if let Some(entry) = tab.entries.get(edit.entry) {
                let trimmed = edit.text.trim();
                if !trimmed.is_empty() && trimmed != entry.name {
                    let before = std::path::PathBuf::from(&entry.path);
                    if crate::utils::storage::rename_item(&entry.path.clone(), trimmed) {
                        // History: reversible rename (`StorageHistory`).
                        let after = before.parent().map(|p| p.join(trimmed)).unwrap_or_default();
                        self.history.record(
                            crate::utils::storage_history::FileOp::Rename { before, after },
                        );
                    } else {
                        tracing::warn!("rename failed");
                    }
                    self.state.active_mut().refresh();
                }
            }
        }
        self.invalidate();
    }

    /// Creates a folder/file in `dir` and records it in the history
    /// (`StorageHistory`), so "Undo" can delete it.
    pub(crate) fn create_item(&mut self, dir: &str, is_dir: bool) {
        use crate::utils::storage_history::FileOp;
        let created = if is_dir {
            crate::utils::storage::create_folder(dir)
        } else {
            crate::utils::storage::create_text_file(dir)
        };
        if let Some(path) = created {
            self.history.record(FileOp::Create { path, is_dir });
            self.state.active_mut().refresh();
        }
    }

    /// `CreateNewFileCommand` with a ShellNew `CommandParameter`: creates a
    /// file from template n° `index` of `list_shell_new_entries()`. The
    /// default name is `display_name + extension` made unique (like the C#
    /// which pre-fills the dialog with the template's `Name`).
    pub(crate) fn create_from_shell_new(&mut self, dir: &str, index: usize) {
        use crate::utils::storage_history::FileOp;
        if dir.is_empty() {
            return;
        }
        let Some(entry) = crate::utils::shell_new::list_shell_new_entries().into_iter().nth(index)
        else {
            return;
        };
        let base = format!("{}{}", entry.display_name, entry.extension);
        let dest = crate::utils::storage::unique_path(&std::path::Path::new(dir).join(base));
        let Some(file_name) = dest.file_name().map(|n| n.to_string_lossy().into_owned()) else {
            return;
        };
        if crate::data::items::create_from_template(&entry, dir, &file_name) {
            self.history.record(FileOp::Create { path: dest, is_dir: false });
            self.state.active_mut().refresh();
        }
    }

    pub(crate) fn can_undo(&self) -> bool {
        self.history.can_undo()
    }

    pub(crate) fn can_redo(&self) -> bool {
        self.history.can_redo()
    }

    /// `StorageHistoryHelpers.TryUndo`: pops the undo stack, applies the inverse.
    pub(crate) fn try_undo(&mut self) {
        use crate::utils::storage_history::FileOp;
        let Some(op) = self.history.pop_undo() else { return };
        let ok = match &op {
            FileOp::Rename { before, after } => {
                let name = before.file_name().map(|n| n.to_string_lossy().into_owned());
                name.is_some_and(|n| {
                    crate::utils::storage::rename_item(&after.to_string_lossy(), &n)
                })
            }
            FileOp::Create { path, .. } => {
                crate::utils::storage::delete_items(&[path.to_string_lossy().into_owned()], false)
            }
        };
        if ok {
            self.history.push_redo(op);
            self.state.active_mut().refresh();
            self.invalidate();
        } else {
            self.history.push_undo(op); // failure: put the operation back.
        }
    }

    /// `StorageHistoryHelpers.TryRedo`: pops the redo stack, re-applies it.
    pub(crate) fn try_redo(&mut self) {
        use crate::utils::storage_history::FileOp;
        let Some(op) = self.history.pop_redo() else { return };
        let ok = match &op {
            FileOp::Rename { before, after } => {
                let name = after.file_name().map(|n| n.to_string_lossy().into_owned());
                name.is_some_and(|n| {
                    crate::utils::storage::rename_item(&before.to_string_lossy(), &n)
                })
            }
            FileOp::Create { path, is_dir } => {
                crate::utils::storage::create_item_at(path, *is_dir)
            }
        };
        if ok {
            self.history.push_undo(op);
            self.state.active_mut().refresh();
            self.invalidate();
        } else {
            self.history.push_redo(op);
        }
    }

}
