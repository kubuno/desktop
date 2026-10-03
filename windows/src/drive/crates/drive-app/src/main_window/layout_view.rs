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
    pub(crate) fn layout_size(&self, mode: crate::view_models::shell_view_model::ViewMode) -> u8 {
        use crate::view_models::shell_view_model::ViewMode;
        let s = crate::services::settings::get();
        match mode {
            ViewMode::Details => s.details_view_size,
            ViewMode::List => s.list_view_size,
            ViewMode::Cards => s.cards_view_size,
            ViewMode::Columns => s.columns_view_size,
            ViewMode::Grid => s.grid_view_size,
        }
        .clamp(1, mode.size_max())
    }

    pub(crate) fn set_layout_size(&mut self, mode: crate::view_models::shell_view_model::ViewMode, size: u8) {
        use crate::view_models::shell_view_model::ViewMode;
        let size = size.clamp(1, mode.size_max());
        crate::services::settings::update(|s| match mode {
            ViewMode::Details => s.details_view_size = size,
            ViewMode::List => s.list_view_size = size,
            ViewMode::Cards => s.cards_view_size = size,
            ViewMode::Columns => s.columns_view_size = size,
            ViewMode::Grid => s.grid_view_size = size,
        });
    }

    /// `LayoutIncreaseSizeAction` / `LayoutDecreaseSizeAction`: grows or
    /// shrinks the current layout's icon size; at the limit, cycles
    /// to the next/previous layout (`LayoutCycler`) and resets
    /// its size so the next keystroke keeps growing/shrinking.
    pub(crate) fn adjust_layout_size(&mut self, forward: bool) {
        use crate::view_models::shell_view_model::{Location, ViewMode};
        let tab = self.state.active();
        // Home has no layout to adjust (`IsExecutable` excludes Home).
        if matches!(tab.location, Location::Home | Location::Settings) {
            return;
        }
        let mode = tab.view_mode;
        let size = self.layout_size(mode);
        let max = mode.size_max();
        if forward && size < max {
            self.set_layout_size(mode, size + 1);
            self.invalidate();
            return;
        }
        if !forward && size > 1 {
            self.set_layout_size(mode, size - 1);
            self.invalidate();
            return;
        }
        // Limit reached: cycle to the adjacent layout and reset its size.
        let recycle = matches!(tab.location, Location::RecycleBin);
        const ORDER: [ViewMode; 5] = [
            ViewMode::Details,
            ViewMode::List,
            ViewMode::Cards,
            ViewMode::Grid,
            ViewMode::Columns,
        ];
        let Some(cur) = ORDER.iter().position(|m| *m == mode) else { return };
        let count = ORDER.len() as i32;
        let step = if forward { 1 } else { -1 };
        for i in 1..=count {
            let next = ORDER[(((cur as i32 + step * i) % count + count) % count) as usize];
            // The Columns view is not supported in the Recycle Bin.
            if next == ViewMode::Columns && recycle {
                continue;
            }
            let reset = if forward { 1 } else { next.size_max() };
            self.set_layout_size(next, reset);
            self.set_view_mode(next);
            self.invalidate();
            return;
        }
    }

    /// Changes the current tab's layout and saves it for the folder
    /// (`LayoutPreferencesManager.SetLayoutPreferencesForPath`).
    pub(crate) fn set_view_mode(&mut self, mode: crate::view_models::shell_view_model::ViewMode) {
        let tab = self.state.active_mut();
        tab.view_mode = mode;
        tab.scroll = 0.0;
        // Entering (or leaving) the Columns layout creates (or empties) the
        // `BladeView`'s blade stack.
        tab.rebuild_columns();
        self.state.active().save_prefs();
    }

}
