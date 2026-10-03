//! Tab ghost during a detached drag: the counterpart to the OLE drag
//! visual the original TabView shows when the tab leaves the band
//! (before `TabDroppedOutside` / a drop onto another window).
//!
//! Same recipe as `flyout_window`: a no-activate `WS_POPUP` composed by
//! DirectComposition (per-pixel alpha), TOPMOST, following the cursor. No
//! acrylic here — the original visual is a translucent snapshot of
//! the tab, not a blurred surface.

use windows::core::{Result, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, LoadCursorW, RegisterClassExW, SetWindowPos,
    ShowWindow, HMENU, IDC_ARROW, SWP_NOACTIVATE, SWP_NOZORDER, SW_HIDE, SW_SHOWNOACTIVATE,
    WNDCLASSEXW, WS_EX_NOACTIVATE, WS_EX_NOREDIRECTIONBITMAP, WS_EX_TOOLWINDOW,
    WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};

use crate::graphics::Renderer;
use crate::styles::theme::Theme;
use crate::ui::Rect;

/// Ghost size in DIP — the silhouette of a tab from the band.
const GHOST_W: f32 = 208.0;
const GHOST_H: f32 = 34.0;
/// Banner for the "Move tab here" label (DragUIOverride.Caption),
/// shown below the silhouette when hovering over a target band.
const CAPTION_H: f32 = 26.0;
const CAPTION_GAP: f32 = 6.0;
/// Offset under the cursor, like the shell drag visual.
const CURSOR_OFFSET: (f32, f32) = (12.0, 16.0);
/// The original "TabDragVisual" is translucent.
const GHOST_OPACITY: f32 = 0.85;

fn class_name() -> PCWSTR {
    use std::sync::Once;
    static REGISTER: Once = Once::new();
    let name = windows::core::w!("DriveTabGhost");
    REGISTER.call_once(|| unsafe {
        let instance = GetModuleHandleW(None).unwrap_or_default();
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(ghost_wndproc),
            hInstance: instance.into(),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            lpszClassName: name,
            ..Default::default()
        };
        RegisterClassExW(&wc);
    });
    name
}

unsafe extern "system" fn ghost_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

/// Passive popup that follows the cursor during a detached drag.
pub struct TabGhostWindow {
    hwnd: HWND,
    renderer: Renderer,
    visible: bool,
    size: (u32, u32),
    /// Last painted title — avoids repainting on every WM_MOUSEMOVE.
    painted: String,
}

impl TabGhostWindow {
    pub fn new(owner: HWND, dark: bool) -> Result<Self> {
        unsafe {
            let instance = GetModuleHandleW(None)?;
            // WS_EX_TRANSPARENT: hit-tests pass through it — the ghost
            // follows the cursor, `WindowFromPoint` must see the window underneath
            // (it's the one that receives the drop).
            let hwnd = CreateWindowExW(
                WS_EX_NOREDIRECTIONBITMAP
                    | WS_EX_TOOLWINDOW
                    | WS_EX_TOPMOST
                    | WS_EX_NOACTIVATE
                    | WS_EX_TRANSPARENT,
                class_name(),
                windows::core::w!(""),
                WS_POPUP,
                0,
                0,
                16,
                16,
                Some(owner),
                None::<HMENU>,
                Some(instance.into()),
                None,
            )?;
            let dark_i: i32 = dark as i32;
            let _ = windows::Win32::Graphics::Dwm::DwmSetWindowAttribute(
                hwnd,
                windows::Win32::Graphics::Dwm::DWMWA_USE_IMMERSIVE_DARK_MODE,
                &dark_i as *const _ as *const _,
                std::mem::size_of::<i32>() as u32,
            );
            let round = windows::Win32::Graphics::Dwm::DWMWCP_ROUND;
            let _ = windows::Win32::Graphics::Dwm::DwmSetWindowAttribute(
                hwnd,
                windows::Win32::Graphics::Dwm::DWMWA_WINDOW_CORNER_PREFERENCE,
                &round as *const _ as *const _,
                std::mem::size_of::<i32>() as u32,
            );
            let renderer = Renderer::new(hwnd, 16, 16, 96.0, crate::services::settings::app_theme_font_override().as_deref())?;
            Ok(Self { hwnd, renderer, visible: false, size: (16, 16), painted: String::new() })
        }
    }

    /// Places the ghost at the screen point (physical px) and paints it if needed.
    /// `caption`: "Move tab here" label when hovering over a band.
    pub fn present(
        &mut self,
        x_px: i32,
        y_px: i32,
        scale: f32,
        theme: &Theme,
        title: &str,
        caption: Option<&str>,
    ) {
        let h_dip = GHOST_H + caption.map_or(0.0, |_| CAPTION_GAP + CAPTION_H);
        let w = (GHOST_W * scale).round().max(1.0) as u32;
        let h = (h_dip * scale).round().max(1.0) as u32;
        if self.size != (w, h) {
            if self.renderer.resize(w, h, scale * 96.0).is_err() {
                return;
            }
            self.size = (w, h);
            self.painted.clear();
        }
        unsafe {
            let _ = SetWindowPos(
                self.hwnd,
                None,
                x_px + (CURSOR_OFFSET.0 * scale) as i32,
                y_px + (CURSOR_OFFSET.1 * scale) as i32,
                w as i32,
                h as i32,
                SWP_NOACTIVATE | SWP_NOZORDER,
            );
        }
        // Paint key: title AND label (either one can change).
        let key = format!("{title}\u{1}{}", caption.unwrap_or(""));
        if self.painted != key {
            self.paint(theme, scale, title, caption);
            self.painted = key;
        }
        if !self.visible {
            unsafe {
                let _ = ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
            }
            self.visible = true;
        }
    }

    fn paint(&self, theme: &Theme, scale: f32, title: &str, caption: Option<&str>) {
        let ctx = &self.renderer.d2d_context;
        unsafe {
            ctx.BeginDraw();
            ctx.Clear(Some(&D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 0.0 }));
            if let Ok(painter) = crate::ui::Painter::new(&self.renderer, theme) {
                painter.set_scale(scale);
                let body = Rect::new(0.0, 0.0, GHOST_W, GHOST_H);
                // The tab's silhouette: dimmed opaque window background,
                // then the active tab surface on top, and the title.
                let base = crate::ui::fade(&theme.window_background, GHOST_OPACITY);
                painter.fill_rounded(&body, 8.0, &base);
                let sheet = crate::ui::fade(&theme.tab_active_background, GHOST_OPACITY);
                painter.fill_rounded(&body, 8.0, &sheet);
                painter.stroke_rounded(&body, 8.0, &theme.tab_border);
                let label = Rect::new(12.0, 0.0, GHOST_W - 12.0, GHOST_H);
                let fg = crate::ui::fade(&theme.text_primary, GHOST_OPACITY);
                // Title left-aligned, truncated with "…" at the silhouette's
                // edge — same Painter as the main window, the
                // ghost having its own Renderer of the same type.
                painter.text_ellipsis(title, &label, &painter.renderer.formats.body, &fg);
                // The label badge, accent + contrasting text, centered below
                // the silhouette (the original's DragUIOverride.Caption).
                if let Some(caption) = caption {
                    let tw = painter.measure(caption, &painter.renderer.formats.caption);
                    let bw = (tw + 24.0).min(GHOST_W);
                    let left = (GHOST_W - bw) / 2.0;
                    let badge = Rect::new(
                        left,
                        GHOST_H + CAPTION_GAP,
                        left + bw,
                        GHOST_H + CAPTION_GAP + CAPTION_H,
                    );
                    painter.fill_rounded(&badge, 4.0, &theme.accent);
                    painter.text(
                        caption,
                        &Rect::new(badge.left + 12.0, badge.top, badge.right, badge.bottom),
                        &painter.renderer.formats.caption,
                        &theme.accent_foreground,
                        false,
                    );
                }
            }
            let _ = ctx.EndDraw(None, None);
        }
        let _ = self.renderer.present();
    }

    pub fn hide(&mut self) {
        if self.visible {
            unsafe {
                let _ = ShowWindow(self.hwnd, SW_HIDE);
            }
            self.visible = false;
        }
    }
}

impl Drop for TabGhostWindow {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
    }
}
