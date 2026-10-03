#![allow(unused_imports)]
//! Submodule of `MainWindow` — see `main_window/mod.rs`.
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
    pub(crate) fn on_tab_drag(&mut self, x_dip: f32, y_dip: f32) {
        const TAB_DETACH_MARGIN: f32 = 24.0;
        let Some(mut drag) = self.tab_drag.take() else {
            return;
        };
        if !drag.moved && (x_dip - drag.press_x).abs() > 4.0 {
            drag.moved = true;
        }
        if !drag.moved {
            self.tab_drag = Some(drag);
            return;
        }

        let layout = self.layout();
        let inside_band = (-TAB_DETACH_MARGIN..crate::ui::TAB_BAR_HEIGHT + TAB_DETACH_MARGIN)
            .contains(&y_dip)
            && (-TAB_DETACH_MARGIN..layout.width + TAB_DETACH_MARGIN).contains(&x_dip);

        if let Some((group, origin)) = drag.detached.take() {
            if inside_band {
                // Re-anchoring: insertion where the cursor is, then
                // in-strip dragging resumes.
                let insert = layout
                    .tabs
                    .iter()
                    .filter(|rect| (rect.left + rect.right) / 2.0 < x_dip)
                    .count()
                    .min(self.state.tabs.len());
                self.state.tabs.insert(insert, group);
                self.state.active_tab = insert;
                self.sync_tab_slides();
                drag.index = insert;
                drag.detached = None;
                if let Some(old) = drag.preview_target.take() {
                    post_tab_preview(old, None);
                }
                self.hide_tab_ghost();
                self.start_tab_animation();
            } else {
                // Still detached: the ghost follows the cursor on screen; if
                // it hovers over another port window's strip, THAT window
                // opens the insertion preview and the ghost takes the
                // "Move tab here" caption (DragUIOverride.Caption).
                drag.detached = Some((group, origin));
                let mut pt = windows::Win32::Foundation::POINT::default();
                unsafe {
                    let _ = windows::Win32::UI::WindowsAndMessaging::GetCursorPos(&mut pt);
                }
                let target = port_tab_strip_under_point(pt, self.hwnd);
                if drag.preview_target != target {
                    if let Some(old) = drag.preview_target {
                        post_tab_preview(old, None);
                    }
                }
                if let Some(t) = target {
                    post_tab_preview(t, Some(pt.x));
                }
                drag.preview_target = target;
                self.present_tab_ghost(&drag, target.is_some());
            }
            self.tab_drag = Some(drag);
            self.invalidate();
            return;
        }

        if !inside_band && self.state.tabs.len() > 1 {
            // DETACHMENT (the sole remaining tab does not detach — like the
            // TabView, which ignores TabDroppedOutside for the last tab).
            let index = drag.index;
            let Some(slot) = layout.tabs.get(index).copied() else { return };
            let width = slot.right - slot.left;
            let group = self.state.tabs.remove(index);
            if self.state.active_tab > index
                || self.state.active_tab >= self.state.tabs.len()
            {
                self.state.active_tab = self.state.active_tab.saturating_sub(1);
            }
            self.state.tab_drag_offset = None;
            // Tabs to the right slide in to fill the gap.
            if index < self.state.tab_slides.len() {
                self.state.tab_slides.remove(index);
            }
            self.sync_tab_slides();
            for i in index..self.state.tabs.len() {
                let carried = self.state.tab_slides[i].offset();
                self.state.tab_slides[i] = crate::ui::TabSlide::new(carried + width);
            }
            drag.detached = Some((group, index));
            self.present_tab_ghost(&drag, false);
            self.start_tab_animation();
            self.tab_drag = Some(drag);
            self.invalidate();
            return;
        }

        let index = drag.index;
        let Some(slot) = layout.tabs.get(index).copied() else { return };
        // The dragged tab tracks the cursor, keeping the grab point under it,
        // and stays inside the strip.
        let width = slot.right - slot.left;
        let (first, last) = (
            layout.tabs[0].left,
            layout.tabs[layout.tabs.len() - 1].right - width,
        );
        let dragged_left = (x_dip - drag.grab).clamp(first, last);
        self.state.tab_drag_offset = Some((index, dragged_left - slot.left));

        // Insertion index = tabs (other than the dragged one) whose center
        // lies left of the cursor.
        let target = layout
            .tabs
            .iter()
            .enumerate()
            .filter(|(i, rect)| *i != index && (rect.left + rect.right) / 2.0 < x_dip)
            .count()
            .min(self.state.tabs.len() - 1);

        if target != index {
            let tab = self.state.tabs.remove(index);
            self.state.tabs.insert(target, tab);
            // The tabs the dragged one stepped over each move one slot the
            // other way: start them where they were and let them glide in
            // (the ListView reorder transition of the original).
            let slide = self.state.tab_slides.remove(index);
            self.state.tab_slides.insert(target, slide);
            let (range, from) = if target > index {
                (index..target, width)
            } else {
                (target + 1..index + 1, -width)
            };
            for i in range {
                let carried = self.state.tab_slides[i].offset();
                self.state.tab_slides[i] = crate::ui::TabSlide::new(carried + from);
            }
            self.state.tab_drag_offset = Some((target, dragged_left - slot.left));

            // Keep the same tab active through the move.
            if self.state.active_tab == index {
                self.state.active_tab = target;
            } else if index < self.state.active_tab && target >= self.state.active_tab {
                self.state.active_tab -= 1;
            } else if index > self.state.active_tab && target <= self.state.active_tab {
                self.state.active_tab += 1;
            }
            self.start_tab_animation();
        }
        drag.index = target;
        self.tab_drag = Some(drag);
        self.invalidate();
    }

    /// Applies (or moves) the insertion preview on OUR strip, with the
    /// same sliding transition as reordering.
    pub(crate) fn set_tab_preview(&mut self, insert: Option<usize>) {
        if self.state.tab_preview_insert == insert {
            return;
        }
        let old: Vec<f32> = self.layout().tabs.iter().map(|r| r.left).collect();
        self.state.tab_preview_insert = insert;
        let new_layout = self.layout();
        self.sync_tab_slides();
        for (i, slide) in self.state.tab_slides.iter_mut().enumerate() {
            if let (Some(o), Some(n)) = (old.get(i), new_layout.tabs.get(i)) {
                let carried = slide.offset();
                *slide = crate::ui::TabSlide::new(carried + (o - n.left));
            }
        }
        self.start_tab_animation();
        self.invalidate();
    }

    /// Re-inserts the detached tab (Escape, accidental-drag guard, failed drop).
    pub(crate) fn reinsert_detached_tab(&mut self, group: crate::view_models::shell_view_model::TabGroup, origin: usize) {
        let at = origin.min(self.state.tabs.len());
        self.state.tabs.insert(at, group);
        self.state.active_tab = at;
        self.sync_tab_slides();
        self.hide_tab_ghost();
        self.invalidate();
    }

    /// Shows the tab ghost (popup) under the cursor during
    /// detachment — the equivalent of the TabView's OLE drag visual. With
    /// `over_strip`, it carries the "Move tab here" caption.
    pub(crate) fn present_tab_ghost(&mut self, drag: &TabDrag, over_strip: bool) {
        let Some((group, _)) = &drag.detached else { return };
        let title = group.active().title();
        let mut pt = windows::Win32::Foundation::POINT::default();
        unsafe {
            let _ = windows::Win32::UI::WindowsAndMessaging::GetCursorPos(&mut pt);
        }
        if self.tab_ghost.is_none() {
            match crate::user_controls::tab_ghost::TabGhostWindow::new(
                self.hwnd,
                self.theme.mode == crate::styles::theme::ThemeMode::Dark,
            ) {
                Ok(g) => self.tab_ghost = Some(g),
                Err(e) => tracing::error!("tab ghost creation failed: {e}"),
            }
        }
        let scale = self.dpi / 96.0;
        let caption = over_strip
            .then(|| drive_localization::tr("TabStripDragAndDropUIOverrideCaption"));
        if let Some(ghost) = self.tab_ghost.as_mut() {
            ghost.present(pt.x, pt.y, scale, &self.theme, &title, caption);
        }
    }

    pub(crate) fn hide_tab_ghost(&mut self) {
        if let Some(ghost) = self.tab_ghost.as_mut() {
            ghost.hide();
        }
    }

    /// End of a detached drag: drop on ANOTHER port window's strip
    /// (WM_COPYDATA transfer, port of `TabView_TabStripDrop`), otherwise
    /// a new window at the drop point (`TabView_TabDroppedOutside`), with
    /// the original's accidental-drag guard.
    pub(crate) fn drop_detached_tab(&mut self, group: crate::view_models::shell_view_model::TabGroup, origin: usize, drag: &TabDrag) {
        use windows::Win32::Foundation::POINT;
        use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;
        let mut pt = POINT::default();
        unsafe {
            let _ = GetCursorPos(&mut pt);
        }
        // The target's insertion preview closes: either the actual
        // insertion replaces it (the WM_COPYDATA receiver clears it itself),
        // or the drop happens elsewhere.
        if let Some(old) = drag.preview_target {
            post_tab_preview(old, None);
        }

        // Another port window's tab strip under the cursor?
        if let Some(target) = port_tab_strip_under_point(pt, self.hwnd) {
            if send_tab_to_window(target, &group, pt.x) {
                self.hide_tab_ghost();
                self.invalidate();
                return;
            }
        }

        let dt = drag.start.elapsed();
        let dx = (pt.x - drag.origin_screen.0) as f64;
        let dy = (pt.y - drag.origin_screen.1) as f64;
        let accidental = dt.as_secs_f64() < 1.0 && (dx * dx + dy * dy).sqrt() < 100.0;
        if accidental {
            self.reinsert_detached_tab(group, origin);
            return;
        }

        // New window with the tab's location, at the drop point.
        if let Ok(exe) = std::env::current_exe() {
            let mut cmd = std::process::Command::new(exe);
            if let Location::Dir(p) = &group.active().location {
                cmd.arg(p.as_os_str());
            }
            cmd.arg("--pos").arg(pt.x.to_string()).arg(pt.y.to_string());
            let _ = cmd.spawn();
        }
        self.hide_tab_ghost();
        self.invalidate();
    }

    /// Keeps `tab_slides` aligned with the tabs (tabs open and close from many
    /// places; a stale animation vector would shift the wrong tab).
    pub(crate) fn sync_tab_slides(&mut self) {
        let n = self.state.tabs.len();
        self.state
            .tab_slides
            .resize_with(n, || crate::ui::TabSlide::new(0.0));
    }

    /// Repaints at ~60 Hz until every tab has settled into its slot.
    pub(crate) fn start_tab_animation(&mut self) {
        unsafe {
            SetTimer(Some(self.hwnd), TAB_ANIM_TIMER, 16, None);
        }
    }

    /// Arms the 16 ms timer that replays the Minimal pane's slide.
    pub(crate) fn reopen_closed_tab(&mut self) {
        if let Some(location) = self.closed_tabs.pop() {
            self.open_tab_at(location);
        }
    }

    pub(crate) fn has_closed_tabs(&self) -> bool {
        !self.closed_tabs.is_empty()
    }

    /// The Omnibar switches to filter/search mode (`SearchAction`,
    /// `ToggleFilterHeaderAction`), pre-filled with the current filter.
    pub(crate) fn close_tab(&mut self, i: usize) {
        let Some(group) = self.state.tabs.get(i) else { return };
        self.closed_tabs.push(group.active().location.clone());
        if self.state.tabs.len() > 1 {
            self.state.tabs.remove(i);
            if self.state.active_tab >= self.state.tabs.len() {
                self.state.active_tab = self.state.tabs.len() - 1;
            }
            self.invalidate();
        } else {
            unsafe {
                let _ = PostMessageW(Some(self.hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
            }
        }
    }

    /// Opens a new tab at the given location, selected.
    pub(crate) fn open_tab_at(&mut self, location: Location) {
        let mut group = TabGroup::new_home();
        if location != Location::Home {
            group.active_mut().navigate(location);
        }
        self.state.tabs.push(group);
        self.state.active_tab = self.state.tabs.len() - 1;
        self.invalidate();
    }

    /// MoveTabToNewWindowAsync: spawns a new instance on the tab's location
    /// then removes the tab from this window.
    pub(crate) fn move_tab_to_new_window(&mut self, i: usize) {
        let Some(group) = self.state.tabs.get(i) else { return };
        let arg = match &group.active().location {
            Location::Dir(p) => Some(p.to_string_lossy().into_owned()),
            _ => None,
        };
        if let Ok(exe) = std::env::current_exe() {
            let mut cmd = std::process::Command::new(exe);
            if let Some(a) = &arg {
                cmd.arg(a);
            }
            let _ = cmd.spawn();
        }
        if self.state.tabs.len() > 1 {
            self.state.tabs.remove(i);
            if self.state.active_tab >= self.state.tabs.len() {
                self.state.active_tab = self.state.tabs.len() - 1;
            }
            self.invalidate();
        } else {
            unsafe {
                let _ = PostMessageW(Some(self.hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
            }
        }
    }

    /// The original TabFlyout (TabBar.xaml), as a Fluent MenuFlyout: seven
    /// items in this order, each carrying its command's
    /// KeyboardAcceleratorTextOverride. MoveTabToNewWindow is the only one with
    /// an icon (FontIcon E8A7) and is enabled only for more than one tab
    /// (TabItemContextMenu_Opening).
    pub(crate) fn show_tab_context_menu(&mut self, i: usize, x_px: f32, y_px: f32) {
        use crate::ui::FlyoutItem;
        let tr = drive_localization::tr;
        let count = self.state.tabs.len();
        let items = vec![
            FlyoutItem::new(tr("NewTab").to_string(), true)
                .with_accel(hotkey_text(&["Control"], "T")),
            FlyoutItem::new(tr("DuplicateTab").to_string(), true)
                .with_accel(hotkey_text(&["Control", "Shift"], "K")),
            FlyoutItem::new(
                tr("HorizontalMultitaskingControlMoveTabToNewWindow.Text").to_string(),
                count > 1,
            )
            .with_glyph("\u{E8A7}"),
            FlyoutItem::new(tr("CloseTabsToTheLeft").to_string(), i > 0),
            FlyoutItem::new(tr("CloseTabsToTheRight").to_string(), i + 1 < count),
            FlyoutItem::new(tr("CloseOtherTabs").to_string(), count > 1),
            FlyoutItem::new(tr("ReopenClosedTab").to_string(), !self.closed_tabs.is_empty())
                .with_accel(hotkey_text(&["Control", "Shift"], "T")),
        ];
        let width = self.flyout_width(&items);
        self.state.flyout = Some(crate::ui::Flyout {
            kind: crate::ui::FlyoutKind::TabContext(i),
            x: self.to_dip(x_px),
            y: self.to_dip(y_px),
            width,
            items,
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
        self.invalidate();
    }

    /// A MenuFlyout sizes to its content: icon column, the widest label, then
    /// the accelerator column.
    /// Runs the TabFlyout command picked in the flyout (1-based, like the order
    /// in TabBar.xaml).
    pub(crate) fn run_tab_context_command(&mut self, i: usize, command: usize) {
        match command {
            1 => self.open_tab_at(Location::Home),
            2 => {
                // DuplicateSelectedTab: same location, inserted after.
                let location = self.state.tabs[i].active().location.clone();
                let mut group = TabGroup::new_home();
                if location != Location::Home {
                    group.active_mut().navigate(location);
                }
                self.state.tabs.insert(i + 1, group);
                self.state.active_tab = i + 1;
                self.invalidate();
            }
            3 => self.move_tab_to_new_window(i),
            4 => {
                for group in self.state.tabs.drain(0..i) {
                    self.closed_tabs.push(group.active().location.clone());
                }
                self.state.active_tab = 0;
                self.invalidate();
            }
            5 => {
                for group in self.state.tabs.drain(i + 1..) {
                    self.closed_tabs.push(group.active().location.clone());
                }
                self.state.active_tab = i.min(self.state.tabs.len() - 1);
                self.invalidate();
            }
            6 => {
                let keep = self.state.tabs.remove(i);
                for group in self.state.tabs.drain(..) {
                    self.closed_tabs.push(group.active().location.clone());
                }
                self.state.tabs.push(keep);
                self.state.active_tab = 0;
                self.invalidate();
            }
            7 => {
                if let Some(location) = self.closed_tabs.pop() {
                    self.open_tab_at(location);
                }
            }
            _ => {}
        }
    }

}
