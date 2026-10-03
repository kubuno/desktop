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
    /// Key handling while the rename editor is open. Returns Some when consumed.
    pub(crate) fn on_edit_key(&mut self, vk: u32) -> Option<LRESULT> {
        use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VK_SHIFT};
        let shift = unsafe { GetKeyState(VK_SHIFT.0 as i32) } < 0;
        match vk {
            0x0D => {
                self.end_rename(true);
                return Some(LRESULT(0));
            }
            0x1B => {
                self.end_rename(false);
                return Some(LRESULT(0));
            }
            _ => {}
        }
        let edit = self.state.edit.as_mut()?;
        match vk {
            0x25 => {
                edit.move_caret(false, shift);
                self.invalidate();
            }
            0x27 => {
                edit.move_caret(true, shift);
                self.invalidate();
            }
            0x24 => {
                edit.caret = 0;
                if !shift {
                    edit.anchor = 0;
                }
                self.invalidate();
            }
            0x23 => {
                edit.caret = edit.text.len();
                if !shift {
                    edit.anchor = edit.caret;
                }
                self.invalidate();
            }
            0x2E => {
                // Delete: clears the selection or the next character.
                if edit.selection().0 == edit.selection().1 {
                    edit.move_caret(true, true);
                }
                edit.replace_selection("");
                self.invalidate();
            }
            _ => return None,
        }
        Some(LRESULT(0))
    }

    /// Keyboard shortcuts, mirroring the C# `GeneralKeyboardAction` defaults.
    pub(crate) fn on_key_down(&mut self, vk: u32) -> Option<LRESULT> {
        use windows::Win32::UI::Input::KeyboardAndMouse::{
            GetKeyState, VK_CONTROL, VK_MENU,
        };
        let ctrl = unsafe { GetKeyState(VK_CONTROL.0 as i32) } < 0;
        let alt = unsafe { GetKeyState(VK_MENU.0 as i32) } < 0;

        // Escape during a tab drag: cancels it (the original's
        // `isCancelingDragOperation`) — a detached tab returns to its
        // original index.
        if vk == 0x1B && self.tab_drag.is_some() {
            if let Some(mut d) = self.tab_drag.take() {
                if let Some(old) = d.preview_target.take() {
                    post_tab_preview(old, None);
                }
                if let Some((group, origin)) = d.detached.take() {
                    self.reinsert_detached_tab(group, origin);
                }
            }
            if let Some((i, offset)) = self.state.tab_drag_offset.take() {
                self.sync_tab_slides();
                if let Some(slide) = self.state.tab_slides.get_mut(i) {
                    *slide = crate::ui::TabSlide::new(offset);
                }
                self.start_tab_animation();
            }
            unsafe {
                let _ = ReleaseCapture();
            }
            self.invalidate();
            return Some(LRESULT(0));
        }


        // The `ContentDialog` is modal: Enter = primary, Escape = close,
        // everything else is blocked (like the WinUI ContentDialog).
        if self.state.dialog.is_some() {
            match vk {
                0x0D => self.dialog_primary(),
                0x1B => {
                    self.state.dialog = None;
                    if self.state.edit.as_ref().is_some_and(|e| e.entry == crate::ui::EDIT_DIALOG) {
                        self.state.edit = None;
                    }
                    self.invalidate();
                }
                // The dialog's field receives the rest of the keyboard input.
                _ => {
                    if self.state.edit.as_ref().is_some_and(|e| e.entry == crate::ui::EDIT_DIALOG) {
                        if let Some(r) = self.on_edit_key(vk) {
                            return Some(r);
                        }
                    }
                }
            }
            return Some(LRESULT(0));
        }

        // Escape closes the flyout.
        if vk == 0x1B && self.state.flyout.is_some() {
            self.state.flyout = None;
            self.invalidate();
            return Some(LRESULT(0));
        }

        // Rename editor: it captures the keyboard.
        if self.state.edit.is_some() {
            if let Some(result) = self.on_edit_key(vk) {
                return Some(result);
            }
            // Lets WM_CHAR through for typing; blocks shortcuts.
            return Some(LRESULT(0));
        }

        // The action registry (`CommandManager[HotKey]`): shortcuts declared
        // by actions take precedence over the hard-coded shortcuts below,
        // which will migrate to `actions/` along with their commands.
        {
            use windows::Win32::UI::Input::KeyboardAndMouse::VK_SHIFT;
            let shift = unsafe { GetKeyState(VK_SHIFT.0 as i32) } < 0;
            if let Some(action) = crate::actions::by_hotkey(vk, ctrl, shift, alt) {
                if action.is_executable(self) {
                    action.execute(self, None);
                }
                return Some(LRESULT(0));
            }
        }

        // ALL the shortcuts (F2, Ctrl+C/X/V, Alt+arrows, Backspace,
        // Delete, F5…) live in `actions/`: the registry above has already
        // resolved them. Only the arrow keys remain — the ListView's own
        // keyboard handling (not a C# action): they move the FOCUS, plain =
        // simple selection, Shift = extends from the anchor, Ctrl = moves
        // the focus only.
        if !alt && (vk == 0x26 || vk == 0x28) {
            let tab = self.state.active();
            if matches!(tab.location, Location::Dir(_)) && !tab.entries.is_empty() {
                use windows::Win32::UI::Input::KeyboardAndMouse::VK_SHIFT;
                let shift = unsafe { GetKeyState(VK_SHIFT.0 as i32) } < 0;
                let len = self.state.active().entries.len();
                let current = self.state.active().focused;
                let next = match (current, vk) {
                    (None, _) => 0,
                    (Some(i), 0x26) => i.saturating_sub(1),
                    (Some(i), _) => (i + 1).min(len - 1),
                };
                let tab = self.state.active_mut();
                if ctrl {
                    tab.focused = Some(next);
                } else if shift {
                    tab.select_range(next);
                } else {
                    tab.select_single(next);
                }
                self.invalidate();
                return Some(LRESULT(0));
            }
        }
        None
    }

    pub(crate) fn nc_hit_test(&self, x_screen: i32, y_screen: i32) -> u32 {
        let mut pt = windows::Win32::Foundation::POINT { x: x_screen, y: y_screen };
        unsafe {
            let _ = ScreenToClient(self.hwnd, &mut pt);
        }
        let x = self.to_dip(pt.x as f32);
        let y = self.to_dip(pt.y as f32);

        // While the flyout is open the whole window is client area, so any
        // click can dismiss it (modal behavior).
        if self.state.flyout.is_some() {
            return HTCLIENT;
        }

        // Top resize border.
        let frame_y = self.to_dip(unsafe {
            GetSystemMetricsForDpi(SM_CYSIZEFRAME, self.dpi as u32) as f32
        });
        if !self.state.maximized && y < frame_y {
            return HTTOP;
        }

        let layout = self.layout();
        if y < TAB_BAR_HEIGHT {
            // Interactive title-bar elements stay client; the rest drags.
            // EXCEPTION: the Maximize button returns `HTMAXBUTTON` so
            // Windows 11 shows the snap-layouts flyout on hover (like the
            // original's system button); the click is then handled natively
            // by `DefWindowProc`. The visual hover is tracked via
            // `WM_NCMOUSEMOVE`.
            return match layout.hit_test(x, y) {
                Some(Hot::CaptionMax) => HTMAXBUTTON,
                Some(
                    Hot::CaptionClose
                    | Hot::CaptionMin
                    | Hot::Tab(_)
                    | Hot::TabClose(_)
                    | Hot::NewTab
                    | Hot::PaneToggle,
                ) => HTCLIENT,
                _ => HTCAPTION,
            };
        }
        HTCLIENT
    }

    pub(crate) fn handle_message(&mut self, msg: u32, wparam: WPARAM, lparam: LPARAM) -> Option<LRESULT> {
        match msg {
            WM_NCCALCSIZE => {
                // Whether wparam is TRUE (NCCALCSIZE_PARAMS) or FALSE (RECT),
                // the rectangle to adjust sits at the start of lparam.
                let rc = unsafe { &mut *(lparam.0 as *mut RECT) };
                let dpi = unsafe { GetDpiForWindow(self.hwnd) };
                let frame_x = unsafe { GetSystemMetricsForDpi(SM_CXSIZEFRAME, dpi) }
                    + unsafe { GetSystemMetricsForDpi(SM_CXPADDEDBORDER, dpi) };
                let frame_y = unsafe { GetSystemMetricsForDpi(SM_CYSIZEFRAME, dpi) }
                    + unsafe { GetSystemMetricsForDpi(SM_CXPADDEDBORDER, dpi) };
                rc.left += frame_x;
                rc.right -= frame_x;
                rc.bottom -= frame_y;
                if unsafe { IsZoomed(self.hwnd) }.as_bool() {
                    rc.top += frame_y;
                }
                Some(LRESULT(0))
            }
            WM_NCHITTEST => {
                let x = (lparam.0 & 0xFFFF) as i16 as i32;
                let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as i32;
                let hit = self.nc_hit_test(x, y);
                if hit == HTCLIENT {
                    return None; // DefWindowProc refines client hits.
                }
                Some(LRESULT(hit as isize))
            }
            // The Maximize button is non-client (HTMAXBUTTON, for the snap
            // flyout): we track its hover here to keep its hover background,
            // since `WM_MOUSEMOVE` (client) no longer fires on it. `wparam` =
            // hit-test code under the cursor.
            WM_NCMOUSEMOVE => {
                let over_max = wparam.0 as u32 == HTMAXBUTTON;
                let next = if over_max {
                    Some(Hot::CaptionMax)
                } else if self.state.hot == Some(Hot::CaptionMax) {
                    None
                } else {
                    self.state.hot
                };
                if self.state.hot != next {
                    self.state.hot = next;
                    self.invalidate();
                }
                None
            }
            // WM_ACTIVATE (0x0006): tracks the window's focus to switch the
            // title bar's active/inactive tint (`LOWORD` = WA_INACTIVE 0).
            0x0006 => {
                let active = (wparam.0 & 0xFFFF) != 0;
                if self.state.window_active != active {
                    self.state.window_active = active;
                    self.invalidate();
                }
                None
            }
            WM_PAINT => {
                self.render();
                unsafe {
                    let _ = ValidateRect(Some(self.hwnd), None);
                }
                Some(LRESULT(0))
            }
            WM_ERASEBKGND => Some(LRESULT(1)),
            WM_SIZE => {
                self.state.maximized = unsafe { IsZoomed(self.hwnd) }.as_bool();
                let (w, h) = self.client_size_px();
                if let Some(renderer) = self.renderer.as_mut() {
                    if let Err(e) = renderer.resize(w, h, self.dpi) {
                        tracing::error!("resize failed: {e}");
                        self.renderer = None;
                        self.bg_image = None;
                    }
                }
                // Reconciles the pane with the new width: crossing the
                // Minimal threshold (641) is DISCRETE — the pane snaps to
                // stowed (or docked), it doesn't slide.
                let width_dip = w as f32 / (self.dpi / 96.0);
                self.state.sidebar_pane_anim = None;
                if crate::ui::sidebar_mode(width_dip) == crate::ui::SidebarMode::Minimal {
                    if !self.state.sidebar_pane_open {
                        self.state.sidebar_pane_tx = -crate::ui::SIDEBAR_OPEN_PANE_LENGTH;
                    }
                } else {
                    self.state.sidebar_pane_open = false;
                    self.state.sidebar_pane_tx = 0.0;
                }
                self.render();
                Some(LRESULT(0))
            }
            WM_DPICHANGED => {
                self.dpi = (wparam.0 & 0xFFFF) as f32;
                let suggested = unsafe { &*(lparam.0 as *const RECT) };
                unsafe {
                    let _ = SetWindowPos(
                        self.hwnd,
                        None,
                        suggested.left,
                        suggested.top,
                        suggested.right - suggested.left,
                        suggested.bottom - suggested.top,
                        SWP_NOZORDER | SWP_NOACTIVATE,
                    );
                }
                Some(LRESULT(0))
            }
            WM_MOUSEMOVE => {
                let x = (lparam.0 & 0xFFFF) as i16 as f32;
                let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as f32;
                // Crossing ~4 DIP actually arms the item drag.
                let (dx, dy) = (self.to_dip(x), self.to_dip(y));
                if let Some((_, sx, sy, active)) = self.item_drag.as_mut() {
                    if !*active && ((dx - *sx).abs() > 4.0 || (dy - *sy).abs() > 4.0) {
                        *active = true;
                    }
                }
                self.on_mouse_move(x, y);
                Some(LRESULT(0))
            }
            WM_MOUSELEAVE => {
                self.mouse_tracking = false;
                // The pointer leaves the window: close the tooltip (`ToolTip`
                // closes on `PointerExited`) and cut off the pending delay.
                unsafe {
                    let _ = KillTimer(Some(self.hwnd), TOOLTIP_TIMER);
                }
                if self.state.hot.is_some() || self.tooltip_shown.is_some() {
                    self.state.hot = None;
                    self.tooltip_shown = None;
                    self.invalidate();
                }
                Some(LRESULT(0))
            }
            WM_LBUTTONDOWN => {
                let x = (lparam.0 & 0xFFFF) as i16 as f32;
                let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as f32;
                // A click closes the tooltip (`ToolTip` dismisses on
                // `PointerPressed`) and cancels a still-pending opening delay.
                unsafe {
                    let _ = KillTimer(Some(self.hwnd), TOOLTIP_TIMER);
                }
                if self.tooltip_shown.take().is_some() {
                    self.invalidate();
                }
                // The ColorPicker: the continuous zones (saturation/value area,
                // hue and alpha sliders)
                // ARM on button-down — the drag then follows the mouse and
                // releases on button-up, like a real Slider.
                if let Some(panel) = self.state.flyout.as_ref().and_then(|f| f.picker) {
                    use crate::user_controls::color_picker::PickerZone;
                    let (dx, dy) = (self.to_dip(x), self.to_dip(y));
                    let (px, py) = {
                        let f = self.state.flyout.as_ref().unwrap();
                        (f.x, f.y)
                    };
                    if let Some(zone @ PickerZone::Drag(_)) = panel.hit(px, py, dx, dy) {
                        let mut p = panel;
                        p.apply(zone, px, py, dx, dy);
                        p.drag = Some(zone);
                        if let Some(f) = self.state.flyout.as_mut() {
                            f.picker = Some(p);
                        }
                        self.apply_picker_color(&p);
                        self.invalidate();
                        return Some(LRESULT(0));
                    }
                }
                if self.state.flyout.is_some() {
                    // The flyout is modal: clicks resolve on button-up.
                    return Some(LRESULT(0));
                }
                let layout = self.layout();
                // The handles: we record the pane's width at grab time, like
                // `SidebarResizer_ManipulationStarted` (`preManipulationSidebarWidth`).
                // The sidebar's ScrollBar thumb is grabbed the same way.
                if let (Some(Hot::SidebarScrollThumb), Some(bar)) =
                    (layout.hit_test(self.to_dip(x), self.to_dip(y)), &layout.sidebar_scrollbar)
                {
                    self.state.sidebar_scroll_drag = Some(self.to_dip(y) - bar.thumb.top);
                    unsafe {
                        windows::Win32::UI::Input::KeyboardAndMouse::SetCapture(self.hwnd);
                    }
                    return Some(LRESULT(0));
                }
                // The sidebar's HORIZONTAL ScrollBar thumb.
                if let (Some(Hot::SidebarHScrollThumb), Some(bar)) =
                    (layout.hit_test(self.to_dip(x), self.to_dip(y)), &layout.sidebar_hscrollbar)
                {
                    self.state.sidebar_hscroll_drag = Some(self.to_dip(x) - bar.thumb.left);
                    unsafe {
                        windows::Win32::UI::Input::KeyboardAndMouse::SetCapture(self.hwnd);
                    }
                    return Some(LRESULT(0));
                }
                // The `ScrollBar`'s thumb is grabbed on button-down.
                if let (Some(Hot::ScrollThumb), Some(bar)) =
                    (layout.hit_test(self.to_dip(x), self.to_dip(y)), &layout.scrollbar)
                {
                    let (pos, start) = if bar.horizontal {
                        (self.to_dip(x), bar.thumb.left)
                    } else {
                        (self.to_dip(y), bar.thumb.top)
                    };
                    self.state.scroll_drag = Some(pos - start);
                    unsafe {
                        windows::Win32::UI::Input::KeyboardAndMouse::SetCapture(self.hwnd);
                    }
                    return Some(LRESULT(0));
                }
                match layout.hit_test(self.to_dip(x), self.to_dip(y)) {
                    Some(hot @ (Hot::SidebarResizer | Hot::InfoPaneResizer)) => {
                        let s = crate::services::settings::get();
                        let width = if hot == Hot::SidebarResizer {
                            crate::ui::sidebar_width()
                        } else {
                            s.info_pane_width
                        };
                        self.pane_drag = Some((hot, self.to_dip(x), width));
                        self.state.pane_dragging = Some(hot);
                        unsafe {
                            windows::Win32::UI::Input::KeyboardAndMouse::SetCapture(self.hwnd);
                        }
                        return Some(LRESULT(0));
                    }
                    // The `GridSplitter` between panes: the ratio is
                    // recomputed continuously from the absolute position (the
                    // two stored values aren't used here).
                    Some(Hot::PaneDivider) => {
                        self.pane_drag = Some((Hot::PaneDivider, 0.0, 0.0));
                        unsafe {
                            windows::Win32::UI::Input::KeyboardAndMouse::SetCapture(self.hwnd);
                        }
                        return Some(LRESULT(0));
                    }
                    _ => {}
                }
                if let Some(Hot::Tab(i)) = layout.hit_test(self.to_dip(x), self.to_dip(y)) {
                    // Select on press and arm the drag (browser behavior).
                    self.state.active_tab = i;
                    self.sync_tab_slides();
                    let press_x = self.to_dip(x);
                    let grab = press_x - layout.tabs[i].left;
                    let mut origin = windows::Win32::Foundation::POINT::default();
                    unsafe {
                        let _ = windows::Win32::UI::WindowsAndMessaging::GetCursorPos(&mut origin);
                    }
                    self.tab_drag = Some(TabDrag {
                        index: i,
                        press_x,
                        grab,
                        moved: false,
                        start: std::time::Instant::now(),
                        origin_screen: (origin.x, origin.y),
                        detached: None,
                        preview_target: None,
                    });
                    unsafe {
                        windows::Win32::UI::Input::KeyboardAndMouse::SetCapture(self.hwnd);
                    }
                    self.invalidate();
                }
                // Pressing an ALREADY-selected row arms an internal drag
                // (the counterpart of `Item_DragStarting`). The selection
                // stays unchanged; a plain click (without crossing the
                // threshold) resolves normally in `on_click` on release.
                if let Some(Hot::FileRow(i)) = layout.hit_test(self.to_dip(x), self.to_dip(y)) {
                    let tab = self.state.active();
                    // No dragging FROM the recycle bin (`InitializeDrag`
                    // excludes `IsUnderTrashBin` items).
                    let row_selected = tab.location != Location::RecycleBin
                        && tab.selected_paths().iter().any(|p| {
                            tab.entries.get(i).is_some_and(|e| e.path == *p)
                        });
                    if row_selected {
                        let paths: Vec<std::path::PathBuf> =
                            tab.selected_paths().into_iter().map(Into::into).collect();
                        if !paths.is_empty() {
                            self.item_drag = Some((paths, self.to_dip(x), self.to_dip(y), false));
                            unsafe {
                                windows::Win32::UI::Input::KeyboardAndMouse::SetCapture(self.hwnd);
                            }
                        }
                    }
                }
                Some(LRESULT(0))
            }
            // An open flyout holds the capture — that's what makes it modal.
            // Releasing it here would trigger WM_CAPTURECHANGED, and thus
            // close it: the « Disposition » panel would close on the first
            // click on a card. `sync_flyout` releases it when it closes.
            WM_LBUTTONUP if self.menu_capture => {
                let x = (lparam.0 & 0xFFFF) as i16 as f32;
                let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as f32;
                if let Some(panel) = self.state.flyout.as_mut().and_then(|f| f.layout.as_mut()) {
                    if panel.dragging {
                        panel.dragging = false;
                        self.invalidate();
                    }
                }
                // End of a ColorPicker drag: the release does NOT count as a
                // click (ending outside the panel must not close it).
                if let Some(p) = self.state.flyout.as_mut().and_then(|f| f.picker.as_mut()) {
                    if p.drag.take().is_some() {
                        self.invalidate();
                        return Some(LRESULT(0));
                    }
                }
                self.on_flyout_click(self.to_dip(x), self.to_dip(y));
                Some(LRESULT(0))
            }
            // `SidebarResizer_ManipulationCompleted`: the width is already
            // written to settings on every delta; only the capture remains
            // to be released.
            WM_LBUTTONUP if self.pane_drag.is_some() => {
                self.pane_drag = None;
                self.state.pane_dragging = None;
                unsafe {
                    let _ = ReleaseCapture();
                }
                Some(LRESULT(0))
            }
            WM_LBUTTONUP if self.state.scroll_drag.is_some() => {
                self.state.scroll_drag = None;
                unsafe {
                    let _ = ReleaseCapture();
                }
                self.mark_scrolled();
                self.invalidate();
                Some(LRESULT(0))
            }
            WM_LBUTTONUP if self.state.sidebar_scroll_drag.is_some() => {
                self.state.sidebar_scroll_drag = None;
                unsafe {
                    let _ = ReleaseCapture();
                }
                self.mark_sidebar_scrolled();
                self.invalidate();
                Some(LRESULT(0))
            }
            WM_LBUTTONUP if self.state.sidebar_hscroll_drag.is_some() => {
                self.state.sidebar_hscroll_drag = None;
                unsafe {
                    let _ = ReleaseCapture();
                }
                self.mark_sidebar_scrolled();
                self.invalidate();
                Some(LRESULT(0))
            }
            WM_LBUTTONUP => {
                // End of an internal drag (`Item_Drop`): if the threshold was
                // crossed, the drop point decides the target and the operation.
                if let Some((paths, _, _, active)) = self.item_drag.take() {
                    unsafe {
                        let _ = ReleaseCapture();
                    }
                    if active {
                        let x = (lparam.0 & 0xFFFF) as i16 as f32;
                        let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as f32;
                        let (dx, dy) = (self.to_dip(x), self.to_dip(y));
                        let layout = self.layout();
                        // The Shelf is a separate target (it doesn't open an op).
                        if layout.shelf_pane.is_some_and(|p| p.contains(dx, dy)) {
                            for path in &paths {
                                self.shelf.add_path(path);
                            }
                        } else {
                            match self.drop_target(layout.hit_test(dx, dy), &layout) {
                                Some(DropTarget::Folder { dest, .. }) => self.perform_drop(&paths, &dest),
                                Some(DropTarget::Pin { .. }) => self.perform_pin_drop(&paths),
                                Some(DropTarget::Tag { uid, .. }) => self.perform_tag_drop(&paths, &uid),
                                None => {}
                            }
                        }
                        self.invalidate();
                        return Some(LRESULT(0));
                    }
                    // No real dragging: let the click resolve.
                }
                let drag = self.tab_drag.take();
                let dragged = drag.as_ref().is_some_and(|d| d.moved);
                unsafe {
                    let _ = ReleaseCapture();
                }
                if dragged {
                    let mut drag = drag.unwrap();
                    if let Some((group, origin)) = drag.detached.take() {
                        // Detached drop: another window's strip, a new
                        // window at the drop point, or re-anchoring (accident).
                        self.drop_detached_tab(group, origin, &drag);
                    } else if let Some((i, offset)) = self.state.tab_drag_offset.take() {
                        // Drop: the tab glides from where the cursor left it
                        // back into its slot, instead of snapping.
                        self.sync_tab_slides();
                        if let Some(slide) = self.state.tab_slides.get_mut(i) {
                            *slide = crate::ui::TabSlide::new(offset);
                        }
                        self.start_tab_animation();
                    }
                }
                if !dragged {
                    let x = (lparam.0 & 0xFFFF) as i16 as f32;
                    let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as f32;
                    let omnibar_mode = self
                        .state
                        .edit
                        .as_ref()
                        .filter(|e| matches!(e.entry, crate::ui::EDIT_PATH | crate::ui::EDIT_PALETTE))
                        .map(|e| e.entry);
                    if let Some(mode) = omnibar_mode {
                        // Path / palette mode: suggestions and internal buttons.
                        let dip = (self.to_dip(x), self.to_dip(y));
                        let layout = self.layout();
                        let mut handled = false;
                        for (i, (_, target)) in self.state.path_suggestions.clone().iter().enumerate() {
                            if crate::ui::suggestion_rect(&layout.address_bar, i).contains(dip.0, dip.1) {
                                self.state.edit = None;
                                self.state.path_suggestions.clear();
                                match target.strip_prefix("cmd:") {
                                    Some(id) => {
                                        let id = id.to_string();
                                        self.run_palette_command(&id);
                                    }
                                    None => self.navigate_active(Location::Dir(target.clone().into())),
                                }
                                handled = true;
                                break;
                            }
                        }
                        if !handled {
                            let clear = crate::ui::Rect::new(layout.address_bar.right - 40.0, layout.address_bar.top, layout.address_bar.right - 8.0, layout.address_bar.bottom);
                            if clear.contains(dip.0, dip.1) {
                                if let Some(edit) = self.state.edit.as_mut() {
                                    edit.text.clear();
                                    edit.caret = 0;
                                    edit.anchor = 0;
                                }
                                if mode == crate::ui::EDIT_PATH {
                                    self.update_path_suggestions();
                                } else {
                                    self.update_palette_suggestions();
                                }
                                self.invalidate();
                                handled = true;
                            } else if layout.address_bar.contains(dip.0, dip.1) {
                                handled = true; // stay in edit mode
                            } else {
                                // Click outside the palette: cancel, do not run.
                                self.end_rename(mode == crate::ui::EDIT_PATH);
                            }
                        }
                        if handled {
                            return Some(LRESULT(0));
                        }
                    } else if self
                        .state
                        .edit
                        .as_ref()
                        .is_some_and(|e| e.entry != crate::ui::EDIT_DIALOG)
                    {
                        // Click outside the editor: commit the rename. A dialog's
                        // field lives as long as the dialog is open (the
                        // ContentDialog TextBox keeps its text on click).
                        self.end_rename(true);
                    }
                    if !self.on_flyout_click(self.to_dip(x), self.to_dip(y)) {
                        self.on_click(x, y);
                    }
                }
                Some(LRESULT(0))
            }
            WM_CAPTURECHANGED => {
                // Capture taken away while a flyout is open (user clicked another
                // window): dismiss it. Our own SetCapture in sync_flyout doesn't
                // reach here, and our ReleaseCapture runs only once flyout is
                // already None, so this can't loop.
                if self.menu_capture && self.state.flyout.is_some() {
                    self.menu_capture = false;
                    self.state.flyout = None;
                    self.invalidate();
                }
                // Capture lost mid-drag: a detached tab returns to its place
                // (like Escape), a tab still in the strip slides back to its slot.
                if let Some(mut d) = self.tab_drag.take() {
                    if let Some(old) = d.preview_target.take() {
                        post_tab_preview(old, None);
                    }
                    if let Some((group, origin)) = d.detached.take() {
                        self.reinsert_detached_tab(group, origin);
                    }
                }
                if let Some((i, offset)) = self.state.tab_drag_offset.take() {
                    self.sync_tab_slides();
                    if let Some(slide) = self.state.tab_slides.get_mut(i) {
                        *slide = crate::ui::TabSlide::new(offset);
                    }
                    self.start_tab_animation();
                    self.invalidate();
                }
                Some(LRESULT(0))
            }
            // The `ScrollBar` indicator fades out: we repaint until it has
            // disappeared, then stop the timer.
            WM_TIMER if wparam.0 == SCROLLBAR_TIMER => {
                if crate::ui::scrollbar_alpha(&self.state) <= 0.0
                    && crate::ui::sidebar_scrollbar_alpha(&self.state) <= 0.0
                {
                    unsafe {
                        let _ = KillTimer(Some(self.hwnd), SCROLLBAR_TIMER);
                    }
                    self.state.scrolled_at = None;
                    self.state.sidebar_scrolled_at = None;
                }
                self.invalidate();
                Some(LRESULT(0))
            }
            WM_TIMER if wparam.0 == MENU_ANIM_TIMER => {
                unsafe {
                    let _ = KillTimer(Some(self.hwnd), MENU_ANIM_TIMER);
                }
                // `sync_flyout` re-arms the timer as long as the unfold continues.
                self.invalidate();
                Some(LRESULT(0))
            }
            WM_TIMER if wparam.0 == TAB_PREVIEW_TIMER => {
                // The preview's source no longer refreshes it (window closed,
                // drop never received): the gap closes itself.
                let stale = self
                    .tab_preview_refresh
                    .is_none_or(|t| t.elapsed().as_millis() > 700);
                if stale {
                    self.set_tab_preview(None);
                    self.tab_preview_refresh = None;
                    unsafe {
                        let _ = KillTimer(Some(self.hwnd), TAB_PREVIEW_TIMER);
                    }
                }
                Some(LRESULT(0))
            }
            WM_TIMER if wparam.0 == TAB_ANIM_TIMER => {
                if self.state.tabs_sliding() {
                    self.invalidate();
                } else {
                    unsafe {
                        let _ = KillTimer(Some(self.hwnd), TAB_ANIM_TIMER);
                    }
                    self.invalidate();
                }
                Some(LRESULT(0))
            }
            WM_TIMER if wparam.0 == SIDEBAR_ANIM_TIMER => {
                if self.state.sidebar_pane_animating() {
                    self.invalidate();
                } else {
                    unsafe {
                        let _ = KillTimer(Some(self.hwnd), SIDEBAR_ANIM_TIMER);
                    }
                    // Freezes the rest position once the animation ends.
                    self.state.sidebar_pane_anim = None;
                    self.invalidate();
                }
                Some(LRESULT(0))
            }
            // The hover delay has elapsed: open the tooltip for the still
            // hovered element (`ToolTipService` after `InitialShowDelay`).
            WM_TIMER if wparam.0 == TOOLTIP_TIMER => {
                unsafe {
                    let _ = KillTimer(Some(self.hwnd), TOOLTIP_TIMER);
                }
                let text = self.state.hot.and_then(crate::ui::tooltip_for);
                if text.is_some() {
                    self.tooltip_shown = text;
                    self.invalidate();
                }
                Some(LRESULT(0))
            }
            // The handle takes the double-arrow cursor (`SidebarResizer_
            // PointerEntered`: `InputSystemCursorShape.SizeWestEast`).
            WM_SETCURSOR
                if (lparam.0 & 0xFFFF) as u32 == HTCLIENT
                    && (self.pane_drag.is_some()
                        || matches!(
                            self.state.hot,
                            Some(Hot::SidebarResizer | Hot::InfoPaneResizer | Hot::PaneDivider)
                        )) =>
            {
                // A HORIZONTAL split's divider takes the vertical double-arrow
                // (`SizeNorthSouth`); everything else, the horizontal one.
                let on_divider = self.pane_drag.map(|(h, _, _)| h) == Some(Hot::PaneDivider)
                    || self.state.hot == Some(Hot::PaneDivider);
                let shape = if on_divider && !self.state.group().split_vertical {
                    IDC_SIZENS
                } else {
                    IDC_SIZEWE
                };
                unsafe {
                    if let Ok(cursor) = LoadCursorW(None, shape) {
                        SetCursor(Some(cursor));
                    }
                }
                Some(LRESULT(1))
            }
            WM_LBUTTONDBLCLK => {
                let x = (lparam.0 & 0xFFFF) as i16 as f32;
                let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as f32;
                // `SidebarResizer_DoubleTapped`: toggles Expanded ⇄ Compact.
                if self.layout().hit_test(self.to_dip(x), self.to_dip(y))
                    == Some(Hot::SidebarResizer)
                {
                    crate::services::settings::update(|s| s.sidebar_compact = !s.sidebar_compact);
                    self.invalidate();
                    return Some(LRESULT(0));
                }
                // `Sizer_OnDoubleTapped`: double-clicking the divider = equal
                // split (all definitions revert to `1*`).
                if self.layout().hit_test(self.to_dip(x), self.to_dip(y)) == Some(Hot::PaneDivider) {
                    self.state.group_mut().split_ratio = 0.5;
                    self.invalidate();
                    return Some(LRESULT(0));
                }
                self.on_double_click(x, y);
                Some(LRESULT(0))
            }
            WM_RBUTTONUP => {
                let x = (lparam.0 & 0xFFFF) as i16 as f32;
                let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as f32;
                self.on_right_click(x, y);
                Some(LRESULT(0))
            }
            WM_MOUSEWHEEL => {
                let delta = ((wparam.0 >> 16) & 0xFFFF) as u16 as i16;
                // TabView_PointerWheelChanged: wheel over the tab strip
                // cycles through the tabs.
                let mut pt = windows::Win32::Foundation::POINT {
                    x: (lparam.0 & 0xFFFF) as i16 as i32,
                    y: ((lparam.0 >> 16) & 0xFFFF) as i16 as i32,
                };
                unsafe {
                    let _ = ScreenToClient(self.hwnd, &mut pt);
                }
                if self.to_dip(pt.y as f32) < TAB_BAR_HEIGHT {
                    let count = self.state.tabs.len();
                    if count > 1 {
                        let current = self.state.active_tab;
                        self.state.active_tab = if delta > 0 {
                            current.checked_sub(1).unwrap_or(count - 1)
                        } else {
                            (current + 1) % count
                        };
                        self.invalidate();
                    }
                } else if self.state.sidebar_visible
                    && self.to_dip(pt.x as f32) < crate::ui::sidebar_width()
                    && self.to_dip(pt.y as f32) > TAB_BAR_HEIGHT + crate::ui::TOOLBAR_HEIGHT
                {
                    // The wheel over the sidebar scrolls ITS content (its
                    // ScrollViewer), not the file list. With Shift held down
                    // (`MK_SHIFT`), it's HORIZONTAL scrolling.
                    let layout = self.layout();
                    let shift = (wparam.0 & 0x0004) != 0;
                    if shift {
                        let viewport = crate::ui::sidebar_width();
                        let max = (layout.sidebar_hextent - viewport).max(0.0);
                        let next = (self.state.sidebar_hscroll - delta as f32 / 120.0 * 96.0)
                            .clamp(0.0, max);
                        if (next - self.state.sidebar_hscroll).abs() > f32::EPSILON {
                            self.state.sidebar_hscroll = next;
                            self.mark_sidebar_scrolled();
                            self.invalidate();
                        }
                    } else {
                        let viewport =
                            layout.height - 56.0 - (TAB_BAR_HEIGHT + crate::ui::TOOLBAR_HEIGHT + 8.0);
                        let max = (layout.sidebar_extent - viewport).max(0.0);
                        let next =
                            (self.state.sidebar_scroll - delta as f32 / 120.0 * 96.0).clamp(0.0, max);
                        if (next - self.state.sidebar_scroll).abs() > f32::EPSILON {
                            self.state.sidebar_scroll = next;
                            self.mark_sidebar_scrolled();
                            self.invalidate();
                        }
                    }
                } else if self.layout().info_pane.is_some_and(|p| {
                    p.contains(self.to_dip(pt.x as f32), self.to_dip(pt.y as f32))
                }) {
                    // The wheel over the info pane scrolls ITS stack (the
                    // original's RootPropertiesScrollViewer). The visible
                    // area = below the thumbnail (tabs 12+32, thumbnail
                    // 12+156, margin 4 — the same layout as the drawing pass).
                    let layout = self.layout();
                    let pane = layout.info_pane.unwrap();
                    let viewport = pane.bottom - (pane.top + 216.0);
                    let max = (self.state.info_pane_extent.get() - viewport).max(0.0);
                    let next = (self.state.info_pane_scroll - delta as f32 / 120.0 * 96.0)
                        .clamp(0.0, max);
                    if (next - self.state.info_pane_scroll).abs() > f32::EPSILON {
                        self.state.info_pane_scroll = next;
                        self.invalidate();
                    }
                } else {
                    self.on_wheel(delta);
                }
                Some(LRESULT(0))
            }
            // Mouse 4/5: the `Mouse4`/`Mouse5` `HotKey`s for `NavigateBack` /
            // `NavigateForward` (the registry only knows the keyboard).
            WM_XBUTTONUP => {
                use crate::actions::Action;
                let button = ((wparam.0 >> 16) & 0xFFFF) as u16;
                let action: &dyn Action = if button == 1 {
                    &crate::actions::navigation::NavigateBack
                } else {
                    &crate::actions::navigation::NavigateForward
                };
                if action.is_executable(self) {
                    action.execute(self, None);
                }
                Some(LRESULT(1))
            }
            // Middle click on a tab closes it (TabView built-in behavior).
            WM_MBUTTONUP => {
                let x = (lparam.0 & 0xFFFF) as i16 as f32;
                let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as f32;
                let layout = self.layout();
                if let Some(Hot::Tab(i) | Hot::TabClose(i)) =
                    layout.hit_test(self.to_dip(x), self.to_dip(y))
                {
                    self.close_tab(i);
                }
                Some(LRESULT(0))
            }
            WM_KEYDOWN | WM_SYSKEYDOWN => self.on_key_down(wparam.0 as u32),
            WM_CHAR => {
                if let Some(edit) = self.state.edit.as_mut() {
                    let code = wparam.0 as u32;
                    match code {
                        0x08 => {
                            // Backspace: selection or previous character.
                            if edit.selection().0 == edit.selection().1 {
                                edit.move_caret(false, true);
                            }
                            edit.replace_selection("");
                        }
                        c if c >= 0x20 => {
                            if let Some(ch) = char::from_u32(c) {
                                // Forbidden characters are only filtered in
                                // NAME editors (inline rename, CreateArchive,
                                // CreateItem) — not in path fields nor the
                                // omnibar, like `IsValidForFilename` on the
                                // C# side.
                                use crate::dialogs::DialogAction as DA;
                                let name_editor = match edit.entry {
                                    crate::ui::EDIT_SEARCH
                                    | crate::ui::EDIT_PATH
                                    | crate::ui::EDIT_PALETTE => false,
                                    crate::ui::EDIT_DIALOG => matches!(
                                        self.state.dialog.as_ref().map(|d| &d.action),
                                        Some(DA::CompressInto(_) | DA::CreateItem(_))
                                    ),
                                    _ => true,
                                };
                                if !(name_editor && r#"\/:*?"<>|"#.contains(ch)) {
                                    edit.replace_selection(&ch.to_string());
                                }
                            }
                        }
                        _ => {}
                    }
                    // Live filtering for the search box.
                    if self.state.edit.as_ref().is_some_and(|e| e.entry == crate::ui::EDIT_SEARCH) {
                        let text = self.state.edit.as_ref().unwrap().text.clone();
                        self.state.active_mut().set_filter(&text);
                    }
                    // Live suggestions for path mode.
                    if self.state.edit.as_ref().is_some_and(|e| e.entry == crate::ui::EDIT_PATH) {
                        self.update_path_suggestions();
                    }
                    // Live suggestions for the command palette.
                    if self.state.edit.as_ref().is_some_and(|e| e.entry == crate::ui::EDIT_PALETTE) {
                        self.update_palette_suggestions();
                    }
                    self.invalidate();
                    return Some(LRESULT(0));
                }
                None
            }
            crate::utils::folder_watcher::WM_APP_DIR_CHANGED => {
                if let Some(watcher) = self.watchers.iter().find(|w| w.id == wparam.0) {
                    self.pending_refresh.insert(watcher.path.clone());
                    unsafe {
                        // 200ms debounce: bulk file operations arrive in bursts.
                        SetTimer(Some(self.hwnd), 42, 200, None);
                    }
                }
                Some(LRESULT(0))
            }
            crate::utils::storage::WM_APP_OPS_PROGRESS => {
                self.invalidate();
                Some(LRESULT(0))
            }
            WM_APP_SHELL_MENU => {
                self.load_shell_overflow();
                Some(LRESULT(0))
            }
            WM_APP_GIT_DONE => {
                // A git operation (pull/push/sync/checkout/…) has finished:
                // reload the listing and the tab's git status.
                self.state.active_mut().refresh();
                self.invalidate();
                Some(LRESULT(0))
            }
            crate::utils::thumbnails::WM_APP_ICON_READY => {
                if let Some(renderer) = self.renderer.as_ref() {
                    let ctx = renderer.d2d_context.clone();
                    self.icons.drain_results(&ctx);
                }
                self.invalidate();
                Some(LRESULT(0))
            }
            // Computed folder sizes: reported into the displayed entries
            // (the original's `SizeChanged`).
            crate::services::folder_search::WM_APP_SEARCH_RESULT => {
                // A batch of search results (`SearchTick`): only accept those
                // from the current generation (a cancelled search still
                // sends late messages).
                let gen = wparam.0 as u64;
                if gen == self.search_generation {
                    let batch = crate::services::folder_search::take_results(gen);
                    if !batch.is_empty()
                        && matches!(
                            self.state.active().location,
                            Location::SearchResults { .. }
                        )
                    {
                        self.state.active_mut().append_search_results(batch);
                        self.invalidate();
                    }
                }
                Some(LRESULT(0))
            }
            crate::services::size_provider::WM_APP_FOLDER_SIZE => {
                let results = self.size_provider.drain();
                if !results.is_empty() {
                    for group in &mut self.state.tabs {
                        let tab = group.active_mut();
                        for entry in tab.entries.iter_mut().filter(|e| e.is_dir) {
                            if let Some((_, size, _)) =
                                results.iter().find(|(p, _, _)| *p == entry.path)
                            {
                                entry.size = *size;
                                entry.size_known = true;
                            }
                        }
                    }
                    self.invalidate();
                }
                Some(LRESULT(0))
            }
            WM_TIMER if wparam.0 == 42 => {
                unsafe {
                    let _ = KillTimer(Some(self.hwnd), 42);
                }
                self.flush_pending_refresh();
                Some(LRESULT(0))
            }
            WM_GETMINMAXINFO => {
                let info = unsafe { &mut *(lparam.0 as *mut MINMAXINFO) };
                let scale = self.dpi / 96.0;
                info.ptMinTrackSize.x = (640.0 * scale) as i32;
                info.ptMinTrackSize.y = (400.0 * scale) as i32;
                Some(LRESULT(0))
            }
            WM_SETTINGCHANGE => {
                self.theme = crate::styles::theme::from_settings();
                let dark: i32 = (self.theme.mode == ThemeMode::Dark) as i32;
                unsafe {
                    let _ = DwmSetWindowAttribute(
                        self.hwnd,
                        DWMWA_USE_IMMERSIVE_DARK_MODE,
                        &dark as *const _ as *const _,
                        std::mem::size_of::<i32>() as u32,
                    );
                }
                self.invalidate();
                Some(LRESULT(0))
            }
            // A tab from another window hovers our strip: the insertion gap
            // opens under the cursor (port of TabStripDragOver).
            WM_APP_TAB_PREVIEW => {
                if wparam.0 == 1 {
                    let mut pt = windows::Win32::Foundation::POINT {
                        x: lparam.0 as i32,
                        y: 0,
                    };
                    unsafe {
                        let _ = windows::Win32::Graphics::Gdi::ScreenToClient(self.hwnd, &mut pt);
                    }
                    let x_dip = self.to_dip(pt.x as f32);
                    let layout = self.layout();
                    let insert = layout
                        .tabs
                        .iter()
                        .take_while(|r| x_dip >= r.right)
                        .count()
                        .min(self.state.tabs.len());
                    self.set_tab_preview(Some(insert));
                    self.tab_preview_refresh = Some(std::time::Instant::now());
                    unsafe {
                        SetTimer(Some(self.hwnd), TAB_PREVIEW_TIMER, 400, None);
                    }
                } else {
                    self.set_tab_preview(None);
                    self.tab_preview_refresh = None;
                    unsafe {
                        let _ = KillTimer(Some(self.hwnd), TAB_PREVIEW_TIMER);
                    }
                }
                Some(LRESULT(0))
            }
            // A tab dropped on our strip from another window of the port
            // (port of `TabView_TabStripDrop`: insertion at the drop point,
            // LRESULT(1) = taken — the sender then closes its tab).
            windows::Win32::UI::WindowsAndMessaging::WM_COPYDATA => {
                let cds = unsafe {
                    &*(lparam.0
                        as *const windows::Win32::System::DataExchange::COPYDATASTRUCT)
                };
                if cds.dwData != TAB_DROP_COPYDATA || cds.lpData.is_null() {
                    return None;
                }
                let words = unsafe {
                    std::slice::from_raw_parts(cds.lpData as *const u16, cds.cbData as usize / 2)
                };
                let payload = String::from_utf16_lossy(words);
                let (x_str, path) = payload.split_once('|')?;
                let x_screen: i32 = x_str.parse().ok()?;
                let mut pt = windows::Win32::Foundation::POINT { x: x_screen, y: 0 };
                unsafe {
                    let _ = windows::Win32::Graphics::Gdi::ScreenToClient(self.hwnd, &mut pt);
                }
                let x_dip = self.to_dip(pt.x as f32);
                let layout = self.layout();
                // Same rule as the original: insertion before the first tab
                // whose right edge is past the drop point.
                // (Computed BEFORE closing the preview: with the gap open,
                // the drop point falls on it and gives the same index.)
                let insert = layout
                    .tabs
                    .iter()
                    .take_while(|r| x_dip >= r.right)
                    .count()
                    .min(self.state.tabs.len());
                // The real tab takes the preview's slot: n+1 slots before as
                // after, with no visual jump.
                self.state.tab_preview_insert = None;
                self.tab_preview_refresh = None;
                unsafe {
                    let _ = KillTimer(Some(self.hwnd), TAB_PREVIEW_TIMER);
                }
                let mut group = TabGroup::new_home();
                if !path.is_empty() && browsable(path) {
                    group.active_mut().navigate(Location::Dir(path.into()));
                }
                self.state.tabs.insert(insert, group);
                self.state.active_tab = insert;
                self.sync_tab_slides();
                self.invalidate();
                Some(LRESULT(1))
            }
            WM_DESTROY => {
                unsafe { PostQuitMessage(0) };
                Some(LRESULT(0))
            }
            _ => None,
        }
    }
}
