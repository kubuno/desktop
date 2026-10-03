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
    pub(crate) fn screen_to_client_dip(&self, x_px: i32, y_px: i32) -> (f32, f32) {
        let mut pt = windows::Win32::Foundation::POINT { x: x_px, y: y_px };
        unsafe {
            let _ = windows::Win32::Graphics::Gdi::ScreenToClient(self.hwnd, &mut pt);
        }
        (self.to_dip(pt.x as f32), self.to_dip(pt.y as f32))
    }

    /// Client-space pixels → screen pixels.
    pub(crate) fn client_to_screen_px(&self, x: f32, y: f32) -> (i32, i32) {
        let mut pt = windows::Win32::Foundation::POINT { x: x.round() as i32, y: y.round() as i32 };
        unsafe {
            let _ = windows::Win32::Graphics::Gdi::ClientToScreen(self.hwnd, &mut pt);
        }
        (pt.x, pt.y)
    }

    /// The work area (screen px) of the monitor the point sits on.
    pub(crate) fn work_area_px(&self, screen_x: i32, screen_y: i32) -> RECT {
        use windows::Win32::Graphics::Gdi::{
            GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTONEAREST,
        };
        unsafe {
            let mon = MonitorFromPoint(
                windows::Win32::Foundation::POINT { x: screen_x, y: screen_y },
                MONITOR_DEFAULTTONEAREST,
            );
            let mut info = MONITORINFO {
                cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            if GetMonitorInfoW(mon, &mut info).as_bool() {
                info.rcWork
            } else {
                RECT { left: 0, top: 0, right: 1920, bottom: 1080 }
            }
        }
    }

    /// A popup's screen rect (px), clamped to the monitor work area, for a
    /// panel at client-DIP (`left`,`top`) of size (`w_dip`,`h_dip`).
    pub(crate) fn place_popup(&self, left: f32, top: f32, w_dip: f32, h_dip: f32, scale: f32) -> (i32, i32, i32, i32) {
        let (mut sx, mut sy) = self.client_to_screen_px(left * scale, top * scale);
        let w = (w_dip * scale).round() as i32;
        let h = (h_dip * scale).round() as i32;
        let wa = self.work_area_px(sx, sy);
        if sx + w > wa.right {
            sx = (wa.right - w).max(wa.left);
        }
        if sy + h > wa.bottom {
            sy = (wa.bottom - h).max(wa.top);
        }
        (sx, sy, w, h)
    }

    /// Reconciles the acrylic popup windows with `state.flyout`: creates and
    /// positions them (clamped to the monitor), repaints on hover/submenu
    /// changes, and holds the mouse capture that makes the menu modal. Called
    /// from `render`, so any state change that repaints keeps the popups in sync.
    pub(crate) fn sync_flyout(&mut self) {
        use windows::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture};

        let dark = self.theme.mode == ThemeMode::Dark;
        if self.state.flyout.is_none() {
            for p in &mut self.menu_popups {
                p.hide();
            }
            if self.menu_capture {
                unsafe {
                    let _ = ReleaseCapture();
                }
                self.menu_capture = false;
            }
            return;
        }

        // Three popup windows: [0] menu, [1] submenu, [2] 3rd level
        // (e.g. Group by › Date modified › Year/Month/Day).
        while self.menu_popups.len() < 3 {
            match crate::user_controls::flyout_window::FlyoutWindow::new(self.hwnd, dark) {
                Ok(p) => self.menu_popups.push(p),
                Err(e) => {
                    tracing::error!("flyout popup creation failed: {e}");
                    return;
                }
            }
        }

        let scale = self.dpi / 96.0;
        // Snapshot the geometry so no borrow of state.flyout outlives the
        // popup calls below (which re-borrow it for the item slices).
        #[allow(clippy::type_complexity)]
        let (
            panel,
            sub_panel,
            subsub_panel,
            width,
            sub_width,
            subsub_width,
            hot_main,
            hot_sub,
            hot_sub2,
            hot_primary,
            has_sub,
            has_subsub,
        ) = {
            let f = self.state.flyout.as_ref().unwrap();
            // Hovering the 3rd level keeps the submenu (level 2) active on its
            // parent item, and the root menu active on ITS parent.
            let hot_main = match f.hot {
                Some((false, i)) => Some(i),
                _ => f.submenu,
            };
            let hot_sub = match f.hot {
                Some((true, i)) => Some(i),
                _ => f.subsubmenu,
            };
            let sub_width = f.submenu.and_then(|p| f.items.get(p)).map(|it| it.children_width);
            let subsub_width = f
                .subsubmenu
                .and_then(|p| f.sub_items().and_then(|e| e.get(p)).map(|it| it.children_width));
            (
                f.panel_rect(),
                f.sub_panel_rect(),
                f.subsub_panel_rect(),
                f.width,
                sub_width,
                subsub_width,
                hot_main,
                hot_sub,
                f.hot_sub2,
                f.hot_primary,
                f.sub_items().is_some(),
                f.subsub_items().is_some(),
            )
        };

        // Compute all three placements up front (each borrows &self briefly), so
        // the present() calls below only need the disjoint field borrows.
        let main_place = self.place_popup(panel.left, panel.top, width, panel.bottom - panel.top, scale);
        let sub_place = match (has_sub, sub_panel, sub_width) {
            (true, Some(sub), Some(sw)) => {
                Some((self.place_popup(sub.left, sub.top, sw, sub.bottom - sub.top, scale), sw))
            }
            _ => None,
        };
        let subsub_place = match (has_subsub, subsub_panel, subsub_width) {
            (true, Some(ss), Some(sw)) => {
                Some((self.place_popup(ss.left, ss.top, sw, ss.bottom - ss.top, scale), sw))
            }
            _ => None,
        };

        // The unfold: the popup window itself grows, so DWM animates
        // the acrylic, shadow, and rounded corners along with it — exactly what
        // WinUI does, animating the popup rather than its content. The content
        // itself is painted at its final place: the rows are revealed.
        let (opened, sub_opened, subsub_opened) = {
            let f = self.state.flyout.as_ref().unwrap();
            (f.opened, f.submenu_opened, f.subsubmenu_opened)
        };
        let unfold = |h: i32, since: std::time::Instant| {
            let p = crate::ui::flyout_progress(since);
            // Start from a third of the height, never zero: a 1 px popup
            // has no corners or shadow, and the appearance would "flicker".
            (h as f32 * (0.34 + 0.66 * p)).round().max(1.0) as i32
        };

        // Clamping to the monitor may have moved the panels: we
        // reinject their ACTUAL position into the state, otherwise the hover
        // test would target the theoretical spot rather than the one shown.
        // Since `place_popup` is idempotent, the position stabilizes on this frame.
        {
            let (sx, sy, ..) = main_place;
            let (dx, dy) = self.screen_to_client_dip(sx, sy);
            let sub = sub_place.map(|((sx, sy, ..), _)| self.screen_to_client_dip(sx, sy));
            let subsub = subsub_place.map(|((sx, sy, ..), _)| self.screen_to_client_dip(sx, sy));
            if let Some(f) = self.state.flyout.as_mut() {
                f.x = dx;
                f.y = dy;
                if let Some(pos) = sub {
                    f.sub_pos = Some(pos);
                }
                if let Some(pos) = subsub {
                    f.subsub_pos = Some(pos);
                }
            }
        }

        let (sx, sy, w, h) = main_place;
        {
            let f = self.state.flyout.as_ref().unwrap();
            self.menu_popups[0].present(
                sx,
                sy,
                w,
                unfold(h, opened),
                &self.theme,
                &f.items,
                &f.primary,
                hot_main,
                hot_primary,
                width,
                f.layout.as_ref(),
                f.picker.as_ref(),
            );
        }

        match sub_place {
            Some(((sx, sy, w, h), sw)) => {
                let h = sub_opened.map_or(h, |t| unfold(h, t));
                if let Some(entries) = self.state.flyout.as_ref().and_then(|f| f.sub_items()) {
                    self.menu_popups[1].present(
                        sx, sy, w, h, &self.theme, entries, &[], hot_sub, None, sw, None, None,
                    );
                }
            }
            None => self.menu_popups[1].hide(),
        }

        match subsub_place {
            Some(((sx, sy, w, h), sw)) => {
                let h = subsub_opened.map_or(h, |t| unfold(h, t));
                if let Some(entries) = self.state.flyout.as_ref().and_then(|f| f.subsub_items()) {
                    self.menu_popups[2].present(
                        sx, sy, w, h, &self.theme, entries, &[], hot_sub2, None, sw, None, None,
                    );
                }
            }
            None => self.menu_popups[2].hide(),
        }

        // While it's unfolding, request another frame (16 ms).
        if self.state.flyout.as_ref().is_some_and(|f| f.animating()) {
            unsafe {
                SetTimer(Some(self.hwnd), MENU_ANIM_TIMER, 16, None);
            }
        }

        if !self.menu_capture {
            unsafe {
                SetCapture(self.hwnd);
            }
            self.menu_capture = true;
        }
    }

    /// Port of `AddShellMenuItemsAsync`: the `ItemOverflow` entry opens on a
    /// disabled "Loading…", and the shell's own `IContextMenu` is queried
    /// off the current message, so that row is really shown before the (slow,
    /// extension-heavy) query runs.
    pub(crate) fn dropdown_flyout(&mut self, entries: Vec<(String, bool, bool)>, x_px: f32, y_px: f32) -> usize {
        use windows::Win32::UI::WindowsAndMessaging::{
            DispatchMessageW, GetMessageW, TranslateMessage, MSG,
        };
        let items: Vec<crate::ui::FlyoutItem> = entries
            .into_iter()
            .map(|(label, checked, enabled)| {
                let mut item = crate::ui::FlyoutItem::new(label, enabled);
                item.pill = true;
                item.checked = checked;
                item
            })
            .collect();
        self.combo_pick = None;
        // A ComboBox drops down under its button, left edge aligned, at least
        // as wide as it (wider if an item requires it); `sync_flyout`
        // already clamps to the monitor if the menu would go off-screen. Without
        // an anchor (ad hoc context menus), we fall back to the click point.
        let widest = items
            .iter()
            .map(|i| crate::ui::approx_text_width(&i.label, 14.0))
            .fold(0.0f32, f32::max);
        let (x, y, width) = match self.combo_anchor.take() {
            Some(a) => (a.left, a.bottom + 4.0, (a.right - a.left).max(widest + 60.0)),
            None => (
                self.to_dip(x_px),
                self.to_dip(y_px) + 4.0,
                crate::ui::FLYOUT_WIDTH.max(widest + 60.0),
            ),
        };
        self.state.flyout = Some(crate::ui::Flyout {
            kind: crate::ui::FlyoutKind::Combo,
            width,
            x,
            y,
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
        // The modal loop: the wndproc (reentrant, as under
        // TrackPopupMenuEx) handles the flyout's clicks, hovers, and Escape.
        unsafe {
            let mut msg = MSG::default();
            while self
                .state
                .flyout
                .as_ref()
                .is_some_and(|f| f.kind == crate::ui::FlyoutKind::Combo)
            {
                if !GetMessageW(&mut msg, None, 0, 0).as_bool() {
                    break;
                }
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        self.combo_pick.take().map(|i| i + 1).unwrap_or(0)
    }

    /// Dropdown menu with a greyed-out state per item (disabled MenuFlyoutItem).
    /// Dropdown menu with selection AND greyed-out state per item (the
    /// "Layout type" combo: Adaptive greyed out when preferences are
    /// synced).
    pub(crate) fn dropdown_checked_ex(&mut self, items: &[(&str, bool, bool)], x_px: f32, y_px: f32) -> usize {
        let entries = items.iter().map(|(l, c, e)| (l.to_string(), *c, *e)).collect();
        self.dropdown_flyout(entries, x_px, y_px)
    }

    pub(crate) fn dropdown(&mut self, items: &[(&str, bool)], x_px: f32, y_px: f32) -> usize {
        let entries = items.iter().map(|(l, c)| (l.to_string(), *c, true)).collect();
        self.dropdown_flyout(entries, x_px, y_px)
    }

    /// LIVE application of the ColorPicker's color
    /// (`AppThemeBackgroundColor` TwoWay): the background follows every gesture,
    /// without rebuilding the popups (the picker lives inside).
    pub(crate) fn flyout_width(&self, items: &[crate::ui::FlyoutItem]) -> f32 {
        let Some(renderer) = self.renderer.as_ref() else {
            return crate::ui::FLYOUT_WIDTH;
        };
        let measure = |text: &str, format: &windows::Win32::Graphics::DirectWrite::IDWriteTextFormat| {
            let wide: Vec<u16> = text.encode_utf16().collect();
            unsafe {
                renderer
                    .dwrite
                    .CreateTextLayout(&wide, format, f32::MAX, f32::MAX)
                    .ok()
                    .and_then(|layout| {
                        let mut metrics = Default::default();
                        layout.GetMetrics(&mut metrics).ok()?;
                        Some(metrics.widthIncludingTrailingWhitespace)
                    })
                    .unwrap_or(0.0)
            }
        };
        let label = items
            .iter()
            .map(|it| measure(&it.label, &renderer.formats.body))
            .fold(0.0f32, f32::max);
        let accel = items
            .iter()
            .filter_map(|it| it.accel.as_deref())
            .map(|a| measure(a, &renderer.formats.caption))
            .fold(0.0f32, f32::max);
        // Measured off the menu's own template constants so the width can
        // never drift from the drawing code: label column, accelerator gap,
        // then the row's closing padding. Floor at the web's `minWidth: 200`.
        use crate::ui::{ACCEL_GAP, CONTENT_RIGHT, LABEL_LEFT};
        (LABEL_LEFT + label + ACCEL_GAP + accel + CONTENT_RIGHT).max(200.0)
    }

}
