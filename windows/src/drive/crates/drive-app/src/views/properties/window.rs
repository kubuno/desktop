//! `FilesPropertiesWindow` : the 2nd top-level window of the properties
//! sheet — the counterpart of the non-modal `WindowEx` from
//! `FilePropertiesHelpers.OpenPropertiesWindow` (800x500-DPI, Mica, non-
//! maximizable, positioned at the cursor and clamped to the work area).
//!
//! Same recipe as the main window (`main_window.rs`): frame removed via
//! `WM_NCCALCSIZE`, `DwmExtendFrameIntoClientArea` glass sheet, backdrop
//! `DWMWA_SYSTEMBACKDROP_TYPE` (Mica), caption buttons drawn by DWM via
//! `DwmDefWindowProc`. It lives on the UI thread and is pumped by the SAME
//! `GetMessage` loop as the main window (like `flyout_window`), so no pump of
//! its own.
//!
//! Internal layout = mirror of `MainPropertiesPage.xaml`: 36 title bar,
//! 200 sidebar (tab list), content panel (the `Frame`) with the General tab,
//! and 52 bottom bar (accent Save + Cancel).

use windows::core::{Result, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;
use windows::Win32::Graphics::Dwm::{
    DwmDefWindowProc, DwmExtendFrameIntoClientArea, DwmSetWindowAttribute, DWMWA_CAPTION_COLOR,
    DWMWA_SYSTEMBACKDROP_TYPE, DWMWA_USE_IMMERSIVE_DARK_MODE, DWM_SYSTEMBACKDROP_TYPE,
};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MonitorFromPoint, ScreenToClient, ValidateRect, MONITORINFO,
    MONITOR_DEFAULTTONEAREST,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::{MARGINS, WM_MOUSELEAVE};
use windows::Win32::UI::HiDpi::{GetDpiForWindow, GetSystemMetricsForDpi};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetClientRect, GetCursorPos,
    GetWindowLongPtrW, KillTimer, LoadCursorW, RegisterClassExW, SetTimer, SetWindowLongPtrW,
    SetWindowPos, ShowWindow, CREATESTRUCTW, CW_USEDEFAULT, GWLP_USERDATA, HMENU, IDC_ARROW,
    SM_CXPADDEDBORDER, SM_CXSIZEFRAME, SM_CYSIZEFRAME, SWP_FRAMECHANGED, SWP_NOACTIVATE,
    SWP_NOZORDER, SW_SHOW, WM_DPICHANGED, WM_ERASEBKGND, WM_KEYDOWN, WM_LBUTTONDOWN, WM_MOUSEMOVE,
    WM_MOUSEWHEEL, WM_NCCALCSIZE, WM_NCCREATE, WM_NCDESTROY, WM_NCHITTEST, WM_PAINT, WM_SIZE, WM_TIMER,
    WNDCLASSEXW, WS_EX_NOREDIRECTIONBITMAP, WS_MAXIMIZEBOX, WS_MINIMIZEBOX, WS_OVERLAPPEDWINDOW,
};

use crate::graphics::Renderer;
use crate::services::size_provider::{SizeProvider, WM_APP_FOLDER_SIZE};
use crate::styles::theme::{Theme, ThemeMode};
use crate::ui::{Painter, Rect};
use crate::services::storage::IconCache;

use super::general::{self, GeneralHits, GeneralModel};
use super::hashes::{self, HashesState, WM_APP_HASH_READY};
use super::{
    compatibility, customization, details, navigation_items, security, shortcut, PropTab,
    PropertiesTarget,
};

/// Layout constants (`MainPropertiesPage.xaml`).
const TITLE_BAR: f32 = 36.0;
const SIDEBAR_W: f32 = 200.0;
const BOTTOM_BAR: f32 = 52.0;
const TAB_ROW_H: f32 = 40.0;
const BTN_W: f32 = 96.0;
const BTN_H: f32 = 32.0;

/// The animation timer for the indeterminate ProgressBar (folder size).
const ANIM_TIMER: usize = 1;

/// Clickable areas of the shell.
#[derive(Clone, Copy, PartialEq, Eq)]
enum PropHot {
    Tab(usize),
    Save,
    Cancel,
    DetailsHeader,
    AttributesHeader,
    ReadOnly,
    Hidden,
}

pub struct FilesPropertiesWindow {
    hwnd: HWND,
    renderer: Option<Renderer>,
    theme: Theme,
    dpi: f32,
    backdrop_available: bool,
    target: PropertiesTarget,
    tabs: Vec<PropTab>,
    /// Selected tab (index into `tabs`). This slice: always General.
    selected: usize,
    model: GeneralModel,
    icons: IconCache,
    size_provider: SizeProvider,
    /// Has the folder already been submitted to the size worker.
    size_requested: bool,
    details_expanded: bool,
    attr_expanded: bool,
    /// Areas of the General tab collected on the last draw.
    gen_hits: GeneralHits,
    /// Tab models, gathered LAZILY on first display.
    details_model: Option<details::DetailsModel>,
    hashes_state: Option<HashesState>,
    security_model: Option<security::SecurityModel>,
    shortcut_model: Option<shortcut::ShortcutModel>,
    compatibility_model: Option<compatibility::CompatibilityModel>,
    customization_model: Option<customization::CustomizationModel>,
    /// Principal (ACE) selected in the Security tab.
    security_selected: usize,
    /// Vertical scroll of the current tab (reset on change).
    content_scroll: f32,
    /// Total content height of the tab, collected by the draw (bounds the
    /// scroll).
    content_extent: std::cell::Cell<f32>,
    /// "Copy" buttons of the Hashes tab, collected by the draw.
    hash_copy_hits: Vec<(Rect, usize)>,
    /// ACE rows of the Security tab, collected by the draw.
    ace_hits: Vec<Rect>,
    /// "Open location" button of the Shortcut tab, collected by the draw.
    shortcut_open_hit: Option<Rect>,
    /// Buttons of the Customization tab, collected by the draw.
    customization_hits: customization::CustomizationHits,
    hot: Option<PropHot>,
    /// Time origin for the indeterminate ProgressBar.
    anim_start: std::time::Instant,
    mouse_tracking: bool,
}

impl FilesPropertiesWindow {
    /// Creates and shows the window. The `Box` is leaked into `GWLP_USERDATA`
    /// and reclaimed/destroyed on `WM_NCDESTROY` (the pattern for a Win32
    /// window whose lifecycle follows its HWND).
    pub fn open(parent: HWND, target: PropertiesTarget) -> Result<()> {
        unsafe {
            let instance = GetModuleHandleW(None)?;
            let class_name = class_name();

            let tabs = navigation_items(&target);
            let model = GeneralModel::gather(&target);

            let boxed = Box::new(FilesPropertiesWindow {
                hwnd: HWND::default(),
                renderer: None,
                theme: crate::styles::theme::from_settings(),
                dpi: 96.0,
                backdrop_available: false,
                target,
                tabs,
                selected: 0,
                model,
                icons: IconCache::new(),
                size_provider: SizeProvider::default(),
                size_requested: false,
                details_expanded: true,
                attr_expanded: false,
                gen_hits: GeneralHits::default(),
                details_model: None,
                hashes_state: None,
                security_model: None,
                shortcut_model: None,
                compatibility_model: None,
                customization_model: None,
                security_selected: 0,
                content_scroll: 0.0,
                content_extent: std::cell::Cell::new(0.0),
                hash_copy_hits: Vec::new(),
                ace_hits: Vec::new(),
                shortcut_open_hit: None,
                customization_hits: customization::CustomizationHits::default(),
                hot: None,
                anim_start: std::time::Instant::now(),
                mouse_tracking: false,
            });
            let raw = Box::into_raw(boxed);

            // Not maximizable AND not minimizable (`IsMaximizable`/`IsMinimizable`
            // = false): remove both boxes from the overlapped style.
            let style = WS_OVERLAPPEDWINDOW & !WS_MAXIMIZEBOX & !WS_MINIMIZEBOX;
            let hwnd = CreateWindowExW(
                WS_EX_NOREDIRECTIONBITMAP,
                class_name,
                &windows::core::HSTRING::from(drive_localization::tr("Properties")),
                style,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                800,
                500,
                Some(parent),
                None::<HMENU>,
                Some(instance.into()),
                Some(raw as *const _),
            );
            match hwnd {
                Ok(hwnd) => {
                    let _ = ShowWindow(hwnd, SW_SHOW);
                    Ok(())
                }
                Err(e) => {
                    // Creation failed before WM_NCCREATE: reclaim the Box.
                    drop(Box::from_raw(raw));
                    Err(e)
                }
            }
        }
    }

    fn init(&mut self, hwnd: HWND) {
        self.hwnd = hwnd;
        unsafe {
            let dark: i32 = (self.theme.mode == ThemeMode::Dark) as i32;
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_USE_IMMERSIVE_DARK_MODE,
                &dark as *const _ as *const _,
                std::mem::size_of::<i32>() as u32,
            );
            let backdrop = DWM_SYSTEMBACKDROP_TYPE(crate::services::settings::get().backdrop.dwm_value());
            self.backdrop_available = DwmSetWindowAttribute(
                hwnd,
                DWMWA_SYSTEMBACKDROP_TYPE,
                &backdrop as *const _ as *const _,
                std::mem::size_of::<DWM_SYSTEMBACKDROP_TYPE>() as u32,
            )
            .is_ok();
            const DWMWA_COLOR_NONE: u32 = 0xFFFFFFFE;
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_CAPTION_COLOR,
                &DWMWA_COLOR_NONE as *const _ as *const _,
                std::mem::size_of::<u32>() as u32,
            );
            // Glass sheet: the backdrop covers the entire client area.
            let margins = MARGINS { cxLeftWidth: -1, cxRightWidth: -1, cyTopHeight: -1, cyBottomHeight: -1 };
            let _ = DwmExtendFrameIntoClientArea(hwnd, &margins);

            self.dpi = GetDpiForWindow(hwnd) as f32;
            let scale = self.dpi / 96.0;
            // Size 800x500-DPI (`OpenPropertiesWindow`), position at the
            // cursor clamped to the monitor's work area.
            let (w, h) = ((800.0 * scale) as i32, (500.0 * scale) as i32);
            let (x, y) = cursor_clamped_position(w, h);
            let _ = SetWindowPos(hwnd, None, x, y, w, h, SWP_FRAMECHANGED | SWP_NOZORDER | SWP_NOACTIVATE);
        }
    }

    fn client_size_px(&self) -> (u32, u32) {
        let mut rc = RECT::default();
        unsafe {
            let _ = GetClientRect(self.hwnd, &mut rc);
        }
        ((rc.right - rc.left).max(1) as u32, (rc.bottom - rc.top).max(1) as u32)
    }

    fn to_dip(&self, px: f32) -> f32 {
        px * 96.0 / self.dpi
    }

    fn invalidate(&self) {
        unsafe {
            let _ = windows::Win32::Graphics::Gdi::InvalidateRect(Some(self.hwnd), None, false);
        }
    }

    /// Shell geometry in DIP (mirror of the grid's columns/rows).
    fn layout(&self) -> PropLayout {
        let (wpx, hpx) = self.client_size_px();
        let width = self.to_dip(wpx as f32);
        let height = self.to_dip(hpx as f32);
        let title_band = Rect::new(0.0, 0.0, width, TITLE_BAR);
        let panel = Rect::new(SIDEBAR_W, TITLE_BAR, width, height);
        let bottom_bar = Rect::new(panel.left, height - BOTTOM_BAR, width, height);
        let content = Rect::new(panel.left, TITLE_BAR, width, height - BOTTOM_BAR);
        // Save / Cancel buttons (right, spacing 12, margin 12).
        let by = bottom_bar.top + (BOTTOM_BAR - BTN_H) / 2.0;
        let save = Rect::new(width - 12.0 - BTN_W, by, width - 12.0, by + BTN_H);
        let cancel = Rect::new(save.left - 12.0 - BTN_W, by, save.left - 12.0, by + BTN_H);
        // Sidebar tab rows.
        let mut tab_rows = Vec::new();
        let mut ty = TITLE_BAR + 4.0;
        for _ in 0..self.tabs.len() {
            tab_rows.push(Rect::new(8.0, ty, SIDEBAR_W - 8.0, ty + TAB_ROW_H));
            ty += TAB_ROW_H;
        }
        PropLayout { title_band, panel, content, bottom_bar, save, cancel, tab_rows }
    }

    fn render(&mut self) {
        let (w, h) = self.client_size_px();
        if self.renderer.is_none() {
            match Renderer::new(self.hwnd, w, h, self.dpi, crate::services::settings::app_theme_font_override().as_deref()) {
                Ok(r) => self.renderer = Some(r),
                Err(e) => {
                    tracing::error!("propriétés: création du renderer échouée: {e}");
                    return;
                }
            }
        }
        let scale = self.dpi / 96.0;

        // Folder size request to the worker (once), + anim timer.
        if self.model.size_computing && !self.size_requested {
            if let PropertiesTarget::Path(path) = &self.target {
                self.size_provider.request(path, self.hwnd.0 as isize);
                self.size_requested = true;
                unsafe {
                    SetTimer(Some(self.hwnd), ANIM_TIMER, 33, None);
                }
            }
        }

        // Shell icon 48 (`ExtraLarge`) of the item, on OUR D2D context.
        let renderer = self.renderer.as_ref().unwrap();
        if let Some(path) = self.model.icon_path.clone() {
            self.icons.ensure(
                &renderer.d2d_context,
                &path,
                (general::SHELL_ICON_EXTRA_LARGE * scale).round() as i32,
            );
        }

        let layout = self.layout();
        let renderer = self.renderer.as_ref().unwrap();
        let ctx = &renderer.d2d_context;
        let theme = &self.theme;
        unsafe {
            ctx.BeginDraw();
            let clear = if self.backdrop_available {
                D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 0.0 }
            } else {
                theme.window_background
            };
            ctx.Clear(Some(&clear));
        }
        if let Ok(p) = Painter::new(renderer, theme) {
            p.set_scale(scale);
            let hits = self.paint(&p, &layout);
            self.gen_hits = hits.gen;
            self.hash_copy_hits = hits.hash_copies;
            self.ace_hits = hits.ace_hits;
            self.shortcut_open_hit = hits.shortcut_open;
            self.customization_hits = hits.customization;
        }
        unsafe {
            let _ = ctx.EndDraw(None, None);
        }
        let _ = renderer.present();
    }

    /// Paints the shell + the current tab ; returns the collected areas.
    fn paint(&self, p: &Painter, layout: &PropLayout) -> PaintHits {
        let t = &self.theme;
        let f = &p.renderer.formats;

        // Content panel (`InnerContent`): `LayerFillColorDefault` background,
        // top+left border, rounded top-left corner (CornerRadius 8,0,0,0).
        p.fill_rounded(&layout.panel, 0.0, &t.layer_fill);

        // Title bar: tr("Properties") text on the left (the caption buttons
        // are drawn by DWM on top, on the right).
        let title = Rect::new(48.0, 0.0, layout.title_band.right - 48.0, TITLE_BAR);
        p.text_ellipsis(drive_localization::tr("Properties"), &title, &f.caption_strong, &t.text_primary);

        // Sidebar: tab list. Only General is clickable ; the others are
        // greyed-out (`IsEnabled=false`).
        for (i, row) in layout.tab_rows.iter().enumerate() {
            let tab = self.tabs[i];
            let enabled = tab.is_implemented();
            let selected = i == self.selected;
            if selected {
                let sel = row.inflate(-2.0, 0.0);
                p.fill_rounded(&sel, 4.0, &t.control_fill_hover);
                // Accent bar on the left (selected `NavigationViewItem`).
                let bar = Rect::new(row.left, row.top + 8.0, row.left + 3.0, row.bottom - 8.0);
                p.fill_rounded(&bar, 1.5, &t.accent);
            } else if self.hot == Some(PropHot::Tab(i)) && enabled {
                p.fill_rounded(&row.inflate(-2.0, 0.0), 4.0, &t.control_fill_hover);
            }
            let fg = if enabled { t.text_primary } else { t.text_secondary };
            let icon = Rect::new(row.left + 12.0, row.top, row.left + 12.0 + 20.0, row.bottom);
            // The real `App.ThemedIcons.Properties.*` ThemedIcon, monochrome.
            p.vector_icon(tab.vector_name(), &icon, 16.0, &fg);
            let label = Rect::new(icon.right + 8.0, row.top, row.right - 8.0, row.bottom);
            p.text_ellipsis(drive_localization::tr(tab.label_key()), &label, &f.body, &fg);
        }

        // Content of the current tab. General draws as-is (no scrolling of
        // its own) ; Details/Hashes/Security are clipped, scrollable stacks
        // (the `ScrollViewer` of each page).
        let mut result = PaintHits::default();
        let current = self.tabs[self.selected];
        match current {
            PropTab::General => {
                result.gen = general::draw_general(
                    p,
                    t,
                    &layout.content,
                    &self.model,
                    self.details_expanded,
                    self.attr_expanded,
                    &self.icons,
                    self.dpi / 96.0,
                    self.anim_start.elapsed(),
                );
            }
            PropTab::Details
            | PropTab::Hashes
            | PropTab::Security
            | PropTab::Shortcut
            | PropTab::Compatibility
            | PropTab::Customization => {
                // Scroll bounded by the extent collected on the previous frame.
                let viewport = layout.content.bottom - layout.content.top;
                let scroll = self
                    .content_scroll
                    .min((self.content_extent.get() - viewport).max(0.0))
                    .max(0.0);
                unsafe {
                    p.ctx.PushAxisAlignedClip(
                        &layout.content.d2d(),
                        windows::Win32::Graphics::Direct2D::D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
                    );
                }
                let extent = match current {
                    PropTab::Details => {
                        if let Some(m) = &self.details_model {
                            details::draw(p, t, &layout.content, m, scroll)
                        } else {
                            0.0
                        }
                    }
                    PropTab::Hashes => {
                        if let Some(s) = &self.hashes_state {
                            let (ext, copies) =
                                hashes::draw(p, t, &layout.content, s, scroll, self.anim_start.elapsed());
                            result.hash_copies = copies;
                            ext
                        } else {
                            0.0
                        }
                    }
                    PropTab::Security => {
                        if let Some(m) = &self.security_model {
                            let (ext, hits) =
                                security::draw(p, t, &layout.content, m, self.security_selected, scroll);
                            result.ace_hits = hits;
                            ext
                        } else {
                            0.0
                        }
                    }
                    PropTab::Shortcut => {
                        if let Some(m) = &self.shortcut_model {
                            let (ext, open) = shortcut::draw(p, t, &layout.content, m, scroll);
                            result.shortcut_open = open;
                            ext
                        } else {
                            0.0
                        }
                    }
                    PropTab::Compatibility => {
                        if let Some(m) = &self.compatibility_model {
                            compatibility::draw(p, t, &layout.content, m, scroll)
                        } else {
                            0.0
                        }
                    }
                    PropTab::Customization => {
                        if let Some(m) = &self.customization_model {
                            let (ext, hits) = customization::draw(p, t, &layout.content, m, scroll);
                            result.customization = hits;
                            ext
                        } else {
                            0.0
                        }
                    }
                    _ => 0.0,
                };
                unsafe {
                    p.ctx.PopAxisAlignedClip();
                }
                self.content_extent.set(extent);
            }
            _ => {}
        }

        // Bottom bar: `CardBackgroundFillColorSecondary` background, top border.
        p.fill_rounded(&layout.bottom_bar, 0.0, &t.toolbar_background);
        let sep = Rect::new(layout.bottom_bar.left, layout.bottom_bar.top, layout.bottom_bar.right, layout.bottom_bar.top + 1.0);
        p.fill_rounded(&sep, 0.0, &t.card_stroke);
        // Save (accent) + Cancel.
        self.draw_button(p, &layout.save, drive_localization::tr("Save"), true, self.hot == Some(PropHot::Save));
        self.draw_button(p, &layout.cancel, drive_localization::tr("Cancel"), false, self.hot == Some(PropHot::Cancel));

        result
    }

    fn draw_button(&self, p: &Painter, rect: &Rect, label: &str, accent: bool, hot: bool) {
        let t = &self.theme;
        let f = &p.renderer.formats;
        let (bg, fg) = if accent {
            (t.accent, t.accent_foreground)
        } else {
            (t.card_background, t.text_primary)
        };
        p.fill_rounded(rect, 4.0, &bg);
        if !accent {
            p.stroke_rounded(rect, 4.0, &t.card_stroke);
        }
        if hot {
            p.fill_rounded(rect, 4.0, &t.control_fill_hover);
        }
        p.text(label, rect, &f.body, &fg, true);
    }

    fn nc_hit_test(&self, x_screen: i32, y_screen: i32) -> u32 {
        use windows::Win32::UI::WindowsAndMessaging::{HTCAPTION, HTCLIENT, HTTOP};
        let mut pt = POINT { x: x_screen, y: y_screen };
        unsafe {
            let _ = ScreenToClient(self.hwnd, &mut pt);
        }
        let y = self.to_dip(pt.y as f32);
        // Top resize border.
        let frame_y = self.to_dip(unsafe { GetSystemMetricsForDpi(SM_CYSIZEFRAME, self.dpi as u32) as f32 });
        if y < frame_y {
            return HTTOP;
        }
        // Title band: no interactive element in this slice → drag.
        if y < TITLE_BAR {
            return HTCAPTION;
        }
        HTCLIENT
    }

    fn on_click(&mut self, x_px: f32, y_px: f32) {
        let (x, y) = (self.to_dip(x_px), self.to_dip(y_px));
        let layout = self.layout();
        // Bottom bar buttons.
        if layout.save.contains(x, y) {
            self.save();
            return;
        }
        if layout.cancel.contains(x, y) {
            self.close();
            return;
        }
        // IMPLEMENTED tabs: General, Details, Hashes, Security.
        for (i, row) in layout.tab_rows.iter().enumerate() {
            if row.contains(x, y) && self.tabs[i].is_implemented() {
                self.select_tab(i);
                return;
            }
        }
        // Areas specific to the current tab.
        match self.tabs[self.selected] {
            PropTab::General => {
                if self.gen_hits.details_header.is_some_and(|r| r.contains(x, y)) {
                    self.details_expanded = !self.details_expanded;
                    self.invalidate();
                } else if self.gen_hits.attributes_header.is_some_and(|r| r.contains(x, y)) {
                    self.attr_expanded = !self.attr_expanded;
                    self.invalidate();
                } else if self.gen_hits.read_only.is_some_and(|r| r.contains(x, y)) && self.model.read_only_enabled {
                    self.model.is_read_only = !self.model.is_read_only;
                    self.invalidate();
                } else if self.gen_hits.hidden.is_some_and(|r| r.contains(x, y)) {
                    self.model.is_hidden = !self.model.is_hidden;
                    self.invalidate();
                }
            }
            PropTab::Hashes => {
                // Copy button of a row (`CopyHashButton`).
                if let Some((_, idx)) = self.hash_copy_hits.iter().find(|(r, _)| r.contains(x, y)).copied() {
                    if let Some(state) = &self.hashes_state {
                        if let Some(value) = state.rows.get(idx).and_then(|r| r.value.clone()) {
                            crate::utils::storage::clipboard_set_text(self.hwnd, &value);
                        }
                    }
                }
            }
            PropTab::Security => {
                // Selecting a principal (ACE) → recompute permissions.
                if let Some(idx) = self.ace_hits.iter().position(|r| r.contains(x, y)) {
                    if self.security_selected != idx {
                        self.security_selected = idx;
                        self.invalidate();
                    }
                }
            }
            PropTab::Shortcut => {
                // `ShortcutItemOpenLinkCommand`: opens the target's folder.
                if self.shortcut_open_hit.is_some_and(|r| r.contains(x, y)) {
                    if let Some(model) = &self.shortcut_model {
                        // For a web link (`IsLinkItem`) the original launches
                        // the target ; otherwise it opens the parent folder.
                        // We stick to the common case: reveal the target in
                        // Explorer.
                        let target = model.target_path.clone();
                        if !target.trim().is_empty() {
                            open_file_location(&target);
                        }
                    }
                }
            }
            PropTab::Customization
                // "Browse" / "Restore default" buttons: UI only, applying the
                // choice (`UpdateIcon`) is a TODO.
                if (self
                    .customization_hits
                    .browse
                    .is_some_and(|r| r.contains(x, y))
                    || self
                        .customization_hits
                        .restore_default
                        .is_some_and(|r| r.contains(x, y)))
                => {
                    tracing::info!("personnalisation d'icône : action non encore portée (TODO)");
                }
            _ => {}
        }
    }

    /// Changes the current tab: resets the scroll and lazily gathers the
    /// tab's model (starts the hash worker).
    fn select_tab(&mut self, index: usize) {
        if self.selected != index {
            self.selected = index;
            self.content_scroll = 0.0;
            self.content_extent.set(0.0);
        }
        self.ensure_tab_model();
        self.invalidate();
    }

    /// Gathers the current tab's model if it isn't already.
    fn ensure_tab_model(&mut self) {
        match self.tabs[self.selected] {
            PropTab::Details => {
                if self.details_model.is_none() {
                    self.details_model = Some(details::DetailsModel::gather(&self.target));
                }
            }
            PropTab::Hashes => {
                if self.hashes_state.is_none() {
                    let mut state = HashesState::new();
                    if let PropertiesTarget::Path(path) = &self.target {
                        if !std::path::Path::new(path).is_dir() {
                            // Background computation ; animates the ProgressBar.
                            state.start(path, self.hwnd.0 as isize);
                            unsafe {
                                SetTimer(Some(self.hwnd), ANIM_TIMER, 33, None);
                            }
                        }
                    }
                    self.hashes_state = Some(state);
                }
            }
            PropTab::Security => {
                if self.security_model.is_none() {
                    self.security_selected = 0;
                    self.security_model = Some(security::SecurityModel::gather(&self.target));
                }
            }
            PropTab::Shortcut => {
                if self.shortcut_model.is_none() {
                    self.shortcut_model = Some(shortcut::ShortcutModel::gather(&self.target));
                }
            }
            PropTab::Compatibility => {
                if self.compatibility_model.is_none() {
                    self.compatibility_model =
                        Some(compatibility::CompatibilityModel::gather(&self.target));
                }
            }
            PropTab::Customization
                if self.customization_model.is_none() => {
                    self.customization_model =
                        Some(customization::CustomizationModel::gather(&self.target));
                }
            _ => {}
        }
    }

    /// Wheel scroll of the current tab (Details/Hashes/Security).
    fn on_wheel(&mut self, delta: i16) {
        if matches!(
            self.tabs[self.selected],
            PropTab::Details
                | PropTab::Hashes
                | PropTab::Security
                | PropTab::Shortcut
                | PropTab::Compatibility
                | PropTab::Customization
        ) {
            let viewport = {
                let l = self.layout();
                l.content.bottom - l.content.top
            };
            let max = (self.content_extent.get() - viewport).max(0.0);
            // 3 lines per notch (~48 DIP), like the standard WinUI scroll.
            let step = delta as f32 / 120.0 * 48.0;
            self.content_scroll = (self.content_scroll - step).clamp(0.0, max);
            self.invalidate();
        }
    }

    fn on_mouse_move(&mut self, x_px: f32, y_px: f32) {
        if !self.mouse_tracking {
            use windows::Win32::UI::Input::KeyboardAndMouse::{
                TrackMouseEvent, TRACKMOUSEEVENT, TME_LEAVE,
            };
            let mut tme = TRACKMOUSEEVENT {
                cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                dwFlags: TME_LEAVE,
                hwndTrack: self.hwnd,
                dwHoverTime: 0,
            };
            unsafe {
                let _ = TrackMouseEvent(&mut tme);
            }
            self.mouse_tracking = true;
        }
        let (x, y) = (self.to_dip(x_px), self.to_dip(y_px));
        let layout = self.layout();
        let mut hot = None;
        if layout.save.contains(x, y) {
            hot = Some(PropHot::Save);
        } else if layout.cancel.contains(x, y) {
            hot = Some(PropHot::Cancel);
        } else {
            for (i, row) in layout.tab_rows.iter().enumerate() {
                if row.contains(x, y) && self.tabs[i].is_implemented() {
                    hot = Some(PropHot::Tab(i));
                }
            }
        }
        if hot != self.hot {
            self.hot = hot;
            self.invalidate();
        }
    }

    /// `SaveChangedPropertiesCommand`: writes the modified attributes then
    /// closes (the port has no renaming/other properties to commit yet).
    fn save(&mut self) {
        if let PropertiesTarget::Path(path) = &self.target {
            general::write_attributes(path, self.model.is_read_only, self.model.is_hidden);
        }
        self.close();
    }

    fn close(&self) {
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
    }

    fn handle_message(&mut self, msg: u32, wparam: WPARAM, lparam: LPARAM) -> Option<LRESULT> {
        match msg {
            WM_NCCALCSIZE => {
                // Removes the frame: the client occupies the whole window
                // (glass sheet). Same computation as the main window —
                // whether wparam is TRUE (NCCALCSIZE_PARAMS) or FALSE (RECT),
                // the rectangle to adjust is at the head of lparam.
                let rc = unsafe { &mut *(lparam.0 as *mut RECT) };
                let dpi = unsafe { GetDpiForWindow(self.hwnd) };
                let fx = unsafe { GetSystemMetricsForDpi(SM_CXSIZEFRAME, dpi) }
                    + unsafe { GetSystemMetricsForDpi(SM_CXPADDEDBORDER, dpi) };
                let fy = unsafe { GetSystemMetricsForDpi(SM_CYSIZEFRAME, dpi) }
                    + unsafe { GetSystemMetricsForDpi(SM_CXPADDEDBORDER, dpi) };
                rc.left += fx;
                rc.right -= fx;
                rc.bottom -= fy;
                Some(LRESULT(0))
            }
            WM_NCHITTEST => {
                use windows::Win32::UI::WindowsAndMessaging::HTCLIENT;
                let x = (lparam.0 & 0xFFFF) as i16 as i32;
                let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as i32;
                let hit = self.nc_hit_test(x, y);
                if hit == HTCLIENT {
                    return None;
                }
                Some(LRESULT(hit as isize))
            }
            WM_ERASEBKGND => Some(LRESULT(1)),
            WM_PAINT => {
                self.render();
                unsafe {
                    let _ = ValidateRect(Some(self.hwnd), None);
                }
                Some(LRESULT(0))
            }
            WM_SIZE => {
                let (w, h) = self.client_size_px();
                if let Some(r) = self.renderer.as_mut() {
                    if let Err(e) = r.resize(w, h, self.dpi) {
                        tracing::error!("propriétés: resize échoué: {e}");
                        self.renderer = None;
                    }
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
                self.on_mouse_move(x, y);
                Some(LRESULT(0))
            }
            WM_MOUSELEAVE => {
                self.mouse_tracking = false;
                if self.hot.is_some() {
                    self.hot = None;
                    self.invalidate();
                }
                Some(LRESULT(0))
            }
            WM_LBUTTONDOWN => {
                let x = (lparam.0 & 0xFFFF) as i16 as f32;
                let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as f32;
                self.on_click(x, y);
                Some(LRESULT(0))
            }
            WM_MOUSEWHEEL => {
                // High wParam: signed delta (multiples of 120).
                let delta = ((wparam.0 >> 16) & 0xFFFF) as i16;
                self.on_wheel(delta);
                Some(LRESULT(0))
            }
            WM_KEYDOWN => {
                // Escape closes (behavior of a properties sheet).
                const VK_ESCAPE: usize = 0x1B;
                if wparam.0 == VK_ESCAPE {
                    self.close();
                }
                Some(LRESULT(0))
            }
            WM_TIMER if wparam.0 == ANIM_TIMER => {
                // Animates the indeterminate ProgressBars: folder size
                // (General tab) OR hash computation in progress.
                let hashing = self.hashes_state.as_ref().is_some_and(|s| s.calculating());
                if self.model.size_computing || hashing {
                    self.invalidate();
                } else {
                    unsafe {
                        let _ = KillTimer(Some(self.hwnd), ANIM_TIMER);
                    }
                }
                Some(LRESULT(0))
            }
            m if m == WM_APP_FOLDER_SIZE => {
                // A batch of sizes is ready: take the value of the leading
                // folder and update the Size row (intermediate/final).
                if let PropertiesTarget::Path(path) = &self.target {
                    let path = path.clone();
                    let results = self.size_provider.drain();
                    for (p, size, done) in results {
                        if p == path {
                            self.model.size_bytes = size;
                            if done {
                                self.model.size_computing = false;
                            }
                        }
                    }
                    self.invalidate();
                }
                Some(LRESULT(0))
            }
            m if m == WM_APP_HASH_READY => {
                // The hashes are ready: drain the worker's results.
                if let Some(state) = self.hashes_state.as_mut() {
                    state.drain();
                    self.invalidate();
                }
                Some(LRESULT(0))
            }
            _ => None,
        }
    }
}

/// Reclaims the `Box` and destroys the object when the HWND disappears.
impl FilesPropertiesWindow {
    unsafe fn drop_boxed(hwnd: HWND) {
        let ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut FilesPropertiesWindow;
        if !ptr.is_null() {
            unsafe {
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                drop(Box::from_raw(ptr));
            }
        }
    }
}

fn class_name() -> PCWSTR {
    use std::sync::Once;
    static REGISTER: Once = Once::new();
    let name = windows::core::w!("DrivePropertiesWindow");
    REGISTER.call_once(|| unsafe {
        let instance = GetModuleHandleW(None).unwrap_or_default();
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(properties_wndproc),
            hInstance: instance.into(),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            lpszClassName: name,
            ..Default::default()
        };
        RegisterClassExW(&wc);
    });
    name
}

extern "system" fn properties_wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        if msg == WM_NCCREATE {
            let create = &*(lparam.0 as *const CREATESTRUCTW);
            let window = create.lpCreateParams as *mut FilesPropertiesWindow;
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, window as isize);
            (*window).init(hwnd);
            return DefWindowProcW(hwnd, msg, wparam, lparam);
        }
        if msg == WM_NCDESTROY {
            FilesPropertiesWindow::drop_boxed(hwnd);
            return DefWindowProcW(hwnd, msg, wparam, lparam);
        }

        let window = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut FilesPropertiesWindow;
        if window.is_null() {
            return DefWindowProcW(hwnd, msg, wparam, lparam);
        }

        // DWM first: it gives the caption buttons (close) their hit-test,
        // hover and click (like the main window).
        let mut result = LRESULT(0);
        if DwmDefWindowProc(hwnd, msg, wparam, lparam, &mut result).as_bool() {
            return result;
        }

        match (*window).handle_message(msg, wparam, lparam) {
            Some(result) => result,
            None => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

/// "Open file location" (`ShortcutItemOpenLinkCommand`, non-web-link case):
/// reveals the target in Explorer (`explorer /select,<target>`).
/// The port doesn't reopen an internal tab like `OpenPathInNewTab` ; it
/// delegates to Explorer to stay simple and faithful to the intent.
fn open_file_location(target: &str) {
    use windows::core::HSTRING;
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    let op = HSTRING::from("open");
    let exe = HSTRING::from("explorer.exe");
    let args = HSTRING::from(format!("/select,\"{target}\""));
    unsafe {
        ShellExecuteW(None, &op, &exe, &args, None, SW_SHOWNORMAL);
    }
}

/// Opening position: at the cursor, clamped to the work area of the nearest
/// monitor (`OpenPropertiesWindow`: `DisplayArea.GetFromPoint` + clamp).
fn cursor_clamped_position(w: i32, h: i32) -> (i32, i32) {
    unsafe {
        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        let monitor = MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST);
        let mut mi = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if GetMonitorInfoW(monitor, &mut mi).as_bool() {
            let wa = mi.rcWork;
            let x = wa.left + (pt.x - wa.left).clamp(0, (wa.right - wa.left - w).max(0));
            let y = wa.top + (pt.y - wa.top).clamp(0, (wa.bottom - wa.top - h).max(0));
            (x, y)
        } else {
            (pt.x, pt.y)
        }
    }
}

/// The clickable areas collected by a tab draw.
#[derive(Default)]
struct PaintHits {
    gen: GeneralHits,
    /// Copy buttons of the Hashes tab: (rect, row index).
    hash_copies: Vec<(Rect, usize)>,
    /// ACE rows of the Security tab.
    ace_hits: Vec<Rect>,
    /// "Open location" button of the Shortcut tab.
    shortcut_open: Option<Rect>,
    /// Buttons of the Customization tab.
    customization: customization::CustomizationHits,
}

/// Shell geometry (DIP).
struct PropLayout {
    title_band: Rect,
    panel: Rect,
    content: Rect,
    bottom_bar: Rect,
    save: Rect,
    cancel: Rect,
    tab_rows: Vec<Rect>,
}
