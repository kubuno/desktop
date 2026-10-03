//! A real top-level popup window for context menus / flyouts.
//!
//! WinUI hosts each flyout in its own `Microsoft.UI.Content.PopupWindowSiteBridge`
//! popup carrying a DesktopAcrylic backdrop — which is exactly why the original's
//! menus can extend past the app window and are blurred. We reproduce that: a
//! no-activate `WS_POPUP` with a DirectComposition swap chain, real acrylic blur,
//! rounded corners and the drop shadow DWM gives every popup. The menu content is
//! drawn on a TRANSPARENT target; the compositor supplies the rest.
//!
//! Two dead ends, both worth remembering:
//!
//! * `DWMWA_SYSTEMBACKDROP_TYPE` (`DWMSBT_TRANSIENTWINDOW`) succeeds but only
//!   blurs while its window is ACTIVE — and a menu never takes focus, so it sat
//!   there showing the flat fallback colour forever.
//! * `DwmExtendFrameIntoClientArea(-1)` makes DWM paint the extended frame as an
//!   OPAQUE sheet *behind* our content, which hides any blur underneath it.

use windows::core::{Result, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;
use windows::Win32::Graphics::Dwm::{
    DwmSetWindowAttribute, DWMWA_USE_IMMERSIVE_DARK_MODE, DWMWA_WINDOW_CORNER_PREFERENCE,
    DWMWCP_ROUND,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, LoadCursorW, RegisterClassExW, SetWindowPos,
    ShowWindow, HMENU, IDC_ARROW, SWP_NOACTIVATE, SWP_NOZORDER, SW_HIDE, SW_SHOWNOACTIVATE,
    WNDCLASSEXW, WS_EX_NOACTIVATE, WS_EX_NOREDIRECTIONBITMAP, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
    WS_POPUP,
};

use crate::graphics::Renderer;
use crate::styles::theme::Theme;

use super::flyout::FlyoutItem;

fn class_name() -> PCWSTR {
    use std::sync::Once;
    static REGISTER: Once = Once::new();
    let name = windows::core::w!("DriveFlyoutPopup");
    REGISTER.call_once(|| unsafe {
        let instance = GetModuleHandleW(None).unwrap_or_default();
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(popup_wndproc),
            hInstance: instance.into(),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            lpszClassName: name,
            ..Default::default()
        };
        RegisterClassExW(&wc);
    });
    name
}

/// The popup is passive: the owning `MainWindow` holds mouse capture and does
/// all hit-testing, so this window never handles input.
unsafe extern "system" fn popup_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

#[repr(C)]
struct AccentPolicy {
    accent_state: u32,
    accent_flags: u32,
    /// ABGR. Its alpha is the acrylic's tint opacity.
    gradient_color: u32,
    animation_id: u32,
}

#[repr(C)]
struct WindowCompositionAttribData {
    attrib: u32,
    data: *mut std::ffi::c_void,
    size: usize,
}

/// Turns on the real acrylic blur behind `hwnd`. Returns false if the OS does
/// not expose the entry point, in which case the caller paints an opaque menu.
fn enable_acrylic(hwnd: HWND, dark: bool) -> bool {
    use windows::core::{s, PCSTR};
    use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryA};

    const WCA_ACCENT_POLICY: u32 = 19;
    const ACCENT_ENABLE_ACRYLICBLURBEHIND: u32 = 4;
    // The tint laid over the blur. `AccentPolicy::gradient_color` is packed
    // ABGR (0xAABBGGRR), NOT ARGB — the previous Fluent values (#2C2C2C /
    // #F9F9F9) were grey, so their byte order was never actually exercised.
    //
    // Kubuno's frosted menu surface (`--kb-float-surface` in `theme.css`):
    //   light  rgb(246 248 252 / 56%) → R F6, G F8, B FC, A round(.56*255)=8F
    //                                 → A8F · B FC · G F8 · R F6 = 0x8FFCF8F6
    //   dark   rgb( 40  40  44 / 58%) → R 28, G 28, B 2C, A round(.58*255)=94
    //                                 → A94 · B 2C · G 28 · R 28 = 0x942C2828
    // Both are noticeably more transparent than the Fluent B3 (70%) they
    // replace, which is what makes the web's menus read as glass.
    let tint: u32 = if dark { 0x942C_2828 } else { 0x8FFC_F8F6 };

    unsafe {
        let Ok(user32) = LoadLibraryA(s!("user32.dll")) else { return false };
        let Some(proc) = GetProcAddress(user32, PCSTR(c"SetWindowCompositionAttribute".as_ptr() as *const u8))
        else {
            return false;
        };
        let set_attr: extern "system" fn(HWND, *mut WindowCompositionAttribData) -> i32 =
            std::mem::transmute(proc);

        let mut policy = AccentPolicy {
            accent_state: ACCENT_ENABLE_ACRYLICBLURBEHIND,
            accent_flags: 0,
            gradient_color: tint,
            animation_id: 0,
        };
        let mut data = WindowCompositionAttribData {
            attrib: WCA_ACCENT_POLICY,
            data: &mut policy as *mut _ as *mut _,
            size: std::mem::size_of::<AccentPolicy>(),
        };
        set_attr(hwnd, &mut data) != 0
    }
}

/// One acrylic popup surface hosting a single menu panel.
pub struct FlyoutWindow {
    hwnd: HWND,
    renderer: Renderer,
    dpi: f32,
    backdrop: bool,
    visible: bool,
    size: (u32, u32),
}

impl FlyoutWindow {
    /// Creates the popup (hidden), owned by `owner` so it floats above it and
    /// is destroyed with it.
    pub fn new(owner: HWND, dark: bool) -> Result<Self> {
        unsafe {
            let instance = GetModuleHandleW(None)?;
            let hwnd = CreateWindowExW(
                WS_EX_NOREDIRECTIONBITMAP | WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE,
                class_name(),
                windows::core::w!(""),
                // A bare popup: NO frame, and no DwmExtendFrameIntoClientArea.
                // Both were tried for the DWM system-backdrop route and are
                // actively harmful here — DWM paints the extended frame as an
                // OPAQUE sheet behind our content, which hid the blur entirely.
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
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_USE_IMMERSIVE_DARK_MODE,
                &dark_i as *const _ as *const _,
                std::mem::size_of::<i32>() as u32,
            );
            // The menu acrylic.
            //
            // NOT via DWMWA_SYSTEMBACKDROP_TYPE: a DWM system backdrop only
            // blurs while its window is ACTIVE, and a menu never takes focus —
            // it would forever show the flat fallback colour. (That is also why
            // Windows' own menus don't use it.) `SetWindowCompositionAttribute`
            // with ACCENT_ENABLE_ACRYLICBLURBEHIND blurs regardless of
            // activation, which is what a popup needs.
            let backdrop = enable_acrylic(hwnd, dark);
            let round = DWMWCP_ROUND;
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_WINDOW_CORNER_PREFERENCE,
                &round as *const _ as *const _,
                std::mem::size_of::<i32>() as u32,
            );
            let dpi = GetDpiForWindow(hwnd) as f32;
            let renderer = Renderer::new(hwnd, 16, 16, dpi, crate::services::settings::app_theme_font_override().as_deref())?;
            Ok(Self { hwnd, renderer, dpi, backdrop, visible: false, size: (16, 16) })
        }
    }

    /// Places, sizes and paints the popup, then shows it without stealing
    /// focus. `x_px`,`y_px`,`w_px`,`h_px` are physical **screen** pixels.
    #[allow(clippy::too_many_arguments)]
    pub fn present(
        &mut self,
        x_px: i32,
        y_px: i32,
        w_px: i32,
        h_px: i32,
        theme: &Theme,
        items: &[FlyoutItem],
        primary: &[FlyoutItem],
        hot: Option<usize>,
        hot_primary: Option<usize>,
        width_dip: f32,
        layout: Option<&super::layout_flyout::LayoutPanel>,
        picker: Option<&super::color_picker::ColorPickerPanel>,
    ) {
        let w = w_px.max(1) as u32;
        let h = h_px.max(1) as u32;
        // The content is painted at scale `self.dpi/96`; the window is
        // sized to `w_px` by the caller from `width_dip * (main_window_dpi
        // / 96)`. If the popup was born on a monitor of a different DPI
        // than the one it displays on (heterogeneous multi-monitor), the DPI cached
        // at creation would diverge and the text would be compressed or stretched. We
        // therefore derive the drawing DPI from the desired window/content ratio: the
        // content *always* fills exactly the window's width, whatever
        // the monitor. On a single screen, this gives back the current DPI.
        if width_dip > 0.0 {
            self.dpi = 96.0 * (w_px.max(1) as f32) / width_dip;
        }
        if self.size != (w, h) {
            if self.renderer.resize(w, h, self.dpi).is_err() {
                return;
            }
            self.size = (w, h);
        }
        unsafe {
            let _ = SetWindowPos(
                self.hwnd,
                None,
                x_px,
                y_px,
                w as i32,
                h as i32,
                SWP_NOACTIVATE | SWP_NOZORDER,
            );
        }
        self.paint(theme, items, primary, hot, hot_primary, width_dip, layout, picker);
        if !self.visible {
            unsafe {
                let _ = ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
            }
            self.visible = true;
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn paint(
        &self,
        theme: &Theme,
        items: &[FlyoutItem],
        primary: &[FlyoutItem],
        hot: Option<usize>,
        hot_primary: Option<usize>,
        width_dip: f32,
        layout: Option<&super::layout_flyout::LayoutPanel>,
        picker: Option<&super::color_picker::ColorPickerPanel>,
    ) {
        let ctx = &self.renderer.d2d_context;
        unsafe {
            ctx.BeginDraw();
            // Transparent when DWM supplies the acrylic; an opaque menu surface
            // otherwise (AlwaysUseFallback).
            let clear = if self.backdrop {
                D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 0.0 }
            } else {
                theme.flyout_background
            };
            ctx.Clear(Some(&clear));
            if let Ok(painter) = crate::ui::Painter::new(&self.renderer, theme) {
                match (layout, picker) {
                    (Some(panel), _) => painter.draw_layout_panel(panel, self.dpi),
                    (None, Some(p)) => {
                        painter.set_scale(self.dpi / 96.0);
                        painter.draw_color_picker(p, 0.0, 0.0);
                    }
                    (None, None) => {
                        painter.draw_menu(items, primary, hot, hot_primary, width_dip, self.dpi)
                    }
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

impl Drop for FlyoutWindow {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
    }
}
