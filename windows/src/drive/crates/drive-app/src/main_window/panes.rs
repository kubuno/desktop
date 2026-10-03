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
    pub(crate) fn override_pane_hit(
        &self,
        hit: Option<Hot>,
        layout: &Layout,
        x_dip: f32,
        y_dip: f32,
    ) -> Option<Hot> {
        let in_pane = layout.info_pane.is_some_and(|p| p.contains(x_dip, y_dip));
        if in_pane {
            if let Some((l, t, r, b)) = self.state.info_properties_rect.get() {
                if x_dip >= l && x_dip <= r && y_dip >= t && y_dip <= b {
                    return Some(Hot::InfoProperties);
                }
            }
        }
        hit
    }

    /// The path the info pane displays: the selection, otherwise the
    /// current folder (same rule as the pane's drawing).
    pub(crate) fn info_pane_path(&self) -> Option<String> {
        let tab = self.state.active();
        tab.selected_entry().map(|e| e.path.clone()).or_else(|| match &tab.location {
            Location::Dir(p) => Some(p.to_string_lossy().into_owned()),
            _ => None,
        })
    }

    /// "Edit tags" (pane): the list of defined tags
    /// (settings), checked according to the item — the `EditTags` flyout
    /// of the original DetailsEditTags.
    pub(crate) fn start_sidebar_animation(&mut self) {
        unsafe {
            SetTimer(Some(self.hwnd), SIDEBAR_ANIM_TIMER, 16, None);
        }
    }

    /// `LayoutSettingsService`: each layout keeps ITS OWN size.
    /// Expands/collapses a sidebar tree node. On expand, the
    /// child folders (and only them) are loaded into the cache — via
    /// `load_directory`, which already honors "Hidden items".
    pub(crate) fn toggle_sidebar_folder(&mut self, path: &str) {
        let key = path.to_lowercase();
        if !self.state.sidebar_expanded.remove(&key) {
            let children: Vec<(String, String)> =
                crate::data::items::load_directory(path)
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|e| e.is_dir)
                    .map(|e| (e.name, e.path))
                    .collect();
            self.state.sidebar_children.insert(key.clone(), children);
            self.state.sidebar_expanded.insert(key);
        }
        self.invalidate();
    }

    /// `UpdateDisplayModeForPaneWidth`: below `COMPACT_MAX_WIDTH`, the original
    /// switches to Compact mode. Lacking a Compact mode here, we close the pane
    /// — which the hamburger button already does — and keep the last width.
    /// `UpdateDisplayModeForPaneWidth`: below `COMPACT_MAX_WIDTH` (200) the
    /// bar switches to COMPACT mode (56 rail) — not closed — and switches back
    /// to Expanded as soon as the drag goes back past the threshold; the drag
    /// stays live from one edge to the other, like the XAML's continuous manipulation.
    pub(crate) fn set_sidebar_width(&mut self, width: f32) {
        // The threshold decision + clamp lives in `grid_splitter` (mirroring
        // `UpdateDisplayModeForPaneWidth`); writing the settings stays here.
        use drive_app_controls::grid_splitter::{resolve_sidebar, PaneResize};
        match resolve_sidebar(width, crate::ui::SIDEBAR_COMPACT_MAX_WIDTH, crate::ui::SIDEBAR_MAX_WIDTH) {
            PaneResize::Compact => crate::services::settings::update(|s| s.sidebar_compact = true),
            PaneResize::Expanded(w) => crate::services::settings::update(|s| {
                s.sidebar_compact = false;
                s.sidebar_width = w;
            }),
        }
    }

    /// `InfoPaneSettingsService.VerticalSizePx`: never less than 100, and the
    /// content column keeps its 208 DIP (`ContentColumn.MinWidth`).
    pub(crate) fn set_info_pane_width(&mut self, width: f32) {
        use drive_app_controls::grid_splitter::{available_max, resize};
        let sidebar = if self.state.sidebar_visible { crate::ui::sidebar_width() } else { 0.0 };
        let max = available_max(self.layout().width - sidebar, crate::ui::CONTENT_MIN_WIDTH, crate::ui::INFOPANE_MIN_WIDTH);
        let width = resize(width, 0.0, crate::ui::INFOPANE_MIN_WIDTH, max, 0.0);
        crate::services::settings::update(|s| s.info_pane_width = width);
    }

}
