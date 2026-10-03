#![allow(unused_imports)]
//! Clicks for the Appearance settings page (mirror of `Views/Settings/AppearanceViewModel`) — `impl MainWindow` block, see `main_window/mod.rs`.
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
    /// Click handling for the Appearance page (AppearancePage.xaml actions).
    pub(crate) fn on_appearance_row(&mut self, row: usize, x_px: f32, y_px: f32) {
        use crate::services::settings::{
            BackdropSetting, ImageFit, ImageHorizontalAlignment, ImageVerticalAlignment,
            StatusCenterVisibility, ThemeSetting, STANDARD_FONT,
        };
        use crate::views::settings::appearance_page::{
            self as ap, ApRow, EXP_ADDRESS_BAR, EXP_BG_COLOR, EXP_BG_IMAGE, EXP_TOOLBAR,
        };
        let layout = self.layout();
        let Some(a) = &layout.appearance else { return };
        let Some(&(rect, kind)) = a.rows.get(row) else { return };
        let (x, y) = (self.to_dip(x_px), self.to_dip(y_px));
        let tr = kubuno_drive_desktop_localization::tr;
        // The dropdown menu's anchor: the EXACT rect of the Appearance
        // page's combo (`Painter::combo`: 188 wide, stuck to the right).
        if matches!(
            kind,
            ApRow::Theme
                | ApRow::Backdrop
                | ApRow::ImageFit
                | ApRow::ImageVAlign
                | ApRow::ImageHAlign
                | ApRow::Font
                | ApRow::StatusCenter
        ) {
            let cy = (rect.top + rect.bottom) / 2.0;
            self.combo_anchor =
                Some(crate::ui::Rect::new(rect.right - 200.0, cy - 16.0, rect.right - 12.0, cy + 16.0));
        }
        match kind {
            ApRow::Theme => {
                let current = crate::services::settings::get().theme;
                let items = [
                    (ThemeSetting::System.label(), current == ThemeSetting::System),
                    (ThemeSetting::Light.label(), current == ThemeSetting::Light),
                    (ThemeSetting::Dark.label(), current == ThemeSetting::Dark),
                ];
                match self.dropdown(&items, x_px, y_px) {
                    1 => crate::services::settings::update(|s| s.theme = ThemeSetting::System),
                    2 => crate::services::settings::update(|s| s.theme = ThemeSetting::Light),
                    3 => crate::services::settings::update(|s| s.theme = ThemeSetting::Dark),
                    _ => return,
                }
                self.apply_appearance();
            }
            ApRow::Backdrop => {
                let current = crate::services::settings::get().backdrop;
                let items = [
                    (BackdropSetting::Mica.label(), current == BackdropSetting::Mica),
                    (BackdropSetting::MicaAlt.label(), current == BackdropSetting::MicaAlt),
                    (BackdropSetting::Acrylic.label(), current == BackdropSetting::Acrylic),
                ];
                match self.dropdown(&items, x_px, y_px) {
                    1 => crate::services::settings::update(|s| s.backdrop = BackdropSetting::Mica),
                    2 => crate::services::settings::update(|s| s.backdrop = BackdropSetting::MicaAlt),
                    3 => crate::services::settings::update(|s| s.backdrop = BackdropSetting::Acrylic),
                    _ => return,
                }
                self.apply_appearance();
            }
            ApRow::BgColorHeader => {
                // The checkerboard button (current color) opens the ColorPicker of
                // AppearancePage.xaml's Flyout; the rest of the header expands.
                let cy = (rect.top + rect.bottom) / 2.0;
                let btn = crate::ui::Rect::new(rect.right - 108.0, cy - 16.0, rect.right - 44.0, cy + 16.0);
                if btn.contains(x, y) {
                    use crate::user_controls::color_picker::{ColorPickerPanel, PICKER_WIDTH};
                    let current = crate::services::settings::parse_color(
                        &crate::services::settings::get().app_theme_background_color,
                    )
                    .unwrap_or(windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F {
                        r: 0.0,
                        g: 0.0,
                        b: 0.0,
                        a: 0.0,
                    });
                    self.state.flyout = Some(crate::ui::Flyout {
                        kind: crate::ui::FlyoutKind::ColorPicker,
                        width: PICKER_WIDTH,
                        x: (btn.right - PICKER_WIDTH).max(0.0),
                        y: btn.bottom + 4.0,
                        items: Vec::new(),
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
                        picker: Some(ColorPickerPanel::from_color(current)),
                        hot: None,
                        path: None,
                    });
                } else {
                    self.state.appearance_expanded[EXP_BG_COLOR] ^= true;
                }
                self.invalidate();
            }
            ApRow::BgImageHeader => {
                let btn = ap::browse_button_rect(&rect);
                if btn.contains(x, y) {
                    if x > btn.right - 28.0 {
                        // SplitButton flyout: Remove.
                        if self.dropdown(&[(tr("Remove"), false)], x_px, y_px) == 1 {
                            crate::services::settings::update(|s| s.app_theme_background_image_source.clear());
                            self.invalidate();
                        }
                    } else if let Some(path) = pick_image_file(self.hwnd) {
                        crate::services::settings::update(|s| s.app_theme_background_image_source = path);
                        self.invalidate();
                    }
                } else {
                    self.state.appearance_expanded[EXP_BG_IMAGE] ^= true;
                    self.invalidate();
                }
            }
            ApRow::ImageOpacity => {
                let track = ap::opacity_slider_rect(&rect);
                if x >= track.left - 8.0 && x <= track.right + 8.0 {
                    // Slider: Minimum=.1, Maximum=1, StepFrequency=.1.
                    let v = ((x - track.left) / (track.right - track.left)).clamp(0.0, 1.0);
                    let value = (((0.1 + v * 0.9) * 10.0).round() / 10.0).clamp(0.1, 1.0);
                    crate::services::settings::update(|s| s.app_theme_background_image_opacity = value);
                    self.invalidate();
                }
            }
            ApRow::ImageFit => {
                let current = crate::services::settings::get().app_theme_background_image_fit;
                let items: Vec<(&str, bool)> =
                    ImageFit::ALL.iter().map(|f| (tr(f.tr_key()), *f == current)).collect();
                let choice = self.dropdown(&items, x_px, y_px);
                if let Some(f) = choice.checked_sub(1).and_then(|i| ImageFit::ALL.get(i)) {
                    crate::services::settings::update(|s| s.app_theme_background_image_fit = *f);
                    self.invalidate();
                }
            }
            ApRow::ImageVAlign => {
                let current = crate::services::settings::get().app_theme_background_image_vertical_alignment;
                let items: Vec<(&str, bool)> = ImageVerticalAlignment::ALL
                    .iter()
                    .map(|v| (tr(v.tr_key()), *v == current))
                    .collect();
                let choice = self.dropdown(&items, x_px, y_px);
                if let Some(v) = choice.checked_sub(1).and_then(|i| ImageVerticalAlignment::ALL.get(i)) {
                    crate::services::settings::update(|s| s.app_theme_background_image_vertical_alignment = *v);
                    self.invalidate();
                }
            }
            ApRow::ImageHAlign => {
                let current = crate::services::settings::get().app_theme_background_image_horizontal_alignment;
                let items: Vec<(&str, bool)> = ImageHorizontalAlignment::ALL
                    .iter()
                    .map(|h| (tr(h.tr_key()), *h == current))
                    .collect();
                let choice = self.dropdown(&items, x_px, y_px);
                if let Some(h) = choice.checked_sub(1).and_then(|i| ImageHorizontalAlignment::ALL.get(i)) {
                    crate::services::settings::update(|s| s.app_theme_background_image_horizontal_alignment = *h);
                    self.invalidate();
                }
            }
            ApRow::Font => {
                let families = self
                    .renderer
                    .as_ref()
                    .map(|r| r.system_font_families())
                    .unwrap_or_default();
                let current = crate::services::settings::get().app_theme_font_family;
                let mut items: Vec<(&str, bool)> = vec![(tr("Default"), current == STANDARD_FONT)];
                items.extend(families.iter().map(|f| (f.as_str(), *f == current)));
                let choice = self.dropdown(&items, x_px, y_px);
                let family = match choice {
                    0 => return,
                    1 => STANDARD_FONT.to_string(),
                    n => match families.get(n - 2) {
                        Some(f) => f.clone(),
                        None => return,
                    },
                };
                crate::services::settings::update(|s| s.app_theme_font_family = family);
                if let Some(r) = self.renderer.as_mut() {
                    if let Err(e) = r.rebuild_text_formats(crate::services::settings::app_theme_font_override().as_deref()) {
                        tracing::warn!("font change failed: {e}");
                    }
                }
                self.invalidate();
            }
            ApRow::ShowTabActions => {
                crate::services::settings::update(|s| s.show_tab_actions = !s.show_tab_actions);
                self.invalidate();
            }
            ApRow::AddressBarHeader => {
                self.state.appearance_expanded[EXP_ADDRESS_BAR] ^= true;
                self.invalidate();
            }
            ApRow::StatusCenter => {
                let current = crate::services::settings::get().status_center_visibility;
                let items: Vec<(&str, bool)> = StatusCenterVisibility::ALL
                    .iter()
                    .map(|v| (tr(v.tr_key()), *v == current))
                    .collect();
                let choice = self.dropdown(&items, x_px, y_px);
                if let Some(v) = choice.checked_sub(1).and_then(|i| StatusCenterVisibility::ALL.get(i)) {
                    crate::services::settings::update(|s| s.status_center_visibility = *v);
                    self.invalidate();
                }
            }
            ApRow::ToolbarHeader => {
                if ap::header_toggle_rect(&rect).contains(x, y) {
                    crate::services::settings::update(|s| s.show_toolbar = !s.show_toolbar);
                } else {
                    self.state.appearance_expanded[EXP_TOOLBAR] ^= true;
                }
                self.invalidate();
            }
            ApRow::CustomizeToolbar => {
                // ToolbarCustomizationDialog: not ported yet.
            }
            ApRow::ShowStatusBar => {
                crate::services::settings::update(|s| s.show_status_bar = !s.show_status_bar);
                self.invalidate();
            }
        }
    }
}
