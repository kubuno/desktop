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
    pub fn create() -> Result<Box<Self>> {
        unsafe {
            let instance = GetModuleHandleW(None)?;
            let class_name = w!("DriveMainWindow");

            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                style: CS_HREDRAW | CS_VREDRAW | CS_DBLCLKS,
                lpfnWndProc: Some(wndproc),
                hInstance: instance.into(),
                // Drive web logo, embedded by build.rs (taskbar + Alt-Tab).
                hIcon: LoadIconW(Some(instance.into()), w!("app_icon"))?,
                hCursor: LoadCursorW(None, IDC_ARROW)?,
                lpszClassName: class_name,
                ..Default::default()
            };
            if RegisterClassExW(&wc) == 0 {
                return Err(windows::core::Error::from_thread());
            }

            // Open the directory passed on the command line (the protocol
            // contract used by Files.App.Launcher), or Home. `--pos x y`:
            // window opening position — tab tear-out opens
            // the new window at the drop point, like
            // `OpenTabInNewWindowAsync(…, droppedPoint.X, droppedPoint.Y)`.
            let mut state = UiState::new();
            let mut window_pos: Option<(i32, i32)> = None;
            let mut args = std::env::args().skip(1);
            while let Some(arg) = args.next() {
                if arg == "--pos" {
                    let x = args.next().and_then(|s| s.parse().ok());
                    let y = args.next().and_then(|s| s.parse().ok());
                    if let (Some(x), Some(y)) = (x, y) {
                        window_pos = Some((x, y));
                    }
                    continue;
                }
                // Always use Windows separators: paths with "/" break
                // IShellItem (icons) and IPersistFile further down.
                let arg = arg.replace('/', "\\");
                if browsable(&arg) {
                    state.active_mut().navigate(Location::Dir(arg.into()));
                }
            }

            let mut window = Box::new(MainWindow {
                hwnd: HWND::default(),
                renderer: None,
                theme: crate::styles::theme::from_settings(),
                state,
                model: HomeModel::load(),
                dpi: 96.0,
                mouse_tracking: false,
                backdrop_available: false,
                icons: IconCache::new(),
                tab_drag: None,
                pane_drag: None,
                breadcrumb_overflow: Vec::new(),
                history: Default::default(),
                shelf: Default::default(),
                item_drag: None,
                mouse_dip: (0.0, 0.0),
                tooltip_shown: None,
                combo_pick: None,
                combo_anchor: None,
                compact_overlay: None,
                fullscreen: None,
                watchers: Vec::new(),
                watcher_seq: 0,
                pending_refresh: std::collections::HashSet::new(),
                ops_monitor: std::sync::Arc::new(crate::utils::storage::OpsMonitor::default()),
                ops_seq: 0,
                search_generation: 0,
                bg_image: None,
                closed_tabs: Vec::new(),
                menu_popups: Vec::new(),
                menu_capture: false,
                shell_worker: None,
                tab_ghost: None,
                tab_preview_refresh: None,
                size_provider: Default::default(),
            });

            let (win_x, win_y) = window_pos.unwrap_or((CW_USEDEFAULT, CW_USEDEFAULT));
            let hwnd = CreateWindowExW(
                WS_EX_NOREDIRECTIONBITMAP,
                class_name,
                w!("Drive"),
                WS_OVERLAPPEDWINDOW,
                win_x,
                win_y,
                1440,
                900,
                None,
                None,
                Some(instance.into()),
                Some(window.as_mut() as *mut MainWindow as *const _),
            )?;
            debug_assert!(hwnd == window.hwnd);

            let _ = ShowWindow(hwnd, SW_SHOW);
            // `WindowsJumpListService.InitializeAsync` → RefreshPinnedFoldersAsync:
            // sets the taskbar's "Pinned" category at startup.
            window.refresh_jump_list_pinned();
            Ok(window)
        }
    }

    pub(crate) fn init(&mut self, hwnd: HWND) {
        self.hwnd = hwnd;
        unsafe {
            // Mica backdrop + immersive dark mode.
            let dark: i32 = (self.theme.mode == ThemeMode::Dark) as i32;
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_USE_IMMERSIVE_DARK_MODE,
                &dark as *const _ as *const _,
                std::mem::size_of::<i32>() as u32,
            );
            let backdrop =
                windows::Win32::Graphics::Dwm::DWM_SYSTEMBACKDROP_TYPE(crate::services::settings::get().backdrop.dwm_value());
            self.backdrop_available = DwmSetWindowAttribute(
                hwnd,
                DWMWA_SYSTEMBACKDROP_TYPE,
                &backdrop as *const _ as *const _,
                std::mem::size_of::<i32>() as u32,
            )
            .is_ok();
            // Prevent DWM from tinting the caption band with the user's
            // accent color — the backdrop must show through instead.
            const DWMWA_COLOR_NONE: u32 = 0xFFFFFFFE;
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_CAPTION_COLOR,
                &DWMWA_COLOR_NONE as *const _ as *const _,
                std::mem::size_of::<u32>() as u32,
            );
            // GUARANTEED Windows 11 rounded corner (like the original's
            // WinUI window): on a custom-frame window, we impose it
            // explicitly rather than relying on the default.
            {
                use windows::Win32::Graphics::Dwm::{
                    DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND, DWM_WINDOW_CORNER_PREFERENCE,
                };
                let pref = DWMWCP_ROUND;
                let _ = DwmSetWindowAttribute(
                    hwnd,
                    DWMWA_WINDOW_CORNER_PREFERENCE,
                    &pref as *const _ as *const _,
                    std::mem::size_of::<DWM_WINDOW_CORNER_PREFERENCE>() as u32,
                );
            }
            // We NO LONGER extend the frame above the client area: otherwise DWM
            // composited the standard title bar (with ITS OWN min/max/close
            // buttons) behind the glass, at a larger size — we now draw them
            // ourselves (draw_caption_buttons) at WinUI's dimensions. The
            // Mica backdrop is still applied via `DWMWA_SYSTEMBACKDROP_TYPE`
            // (independent of frame extension on Windows 11).
            let margins = MARGINS {
                cxLeftWidth: 0,
                cxRightWidth: 0,
                cyTopHeight: 0,
                cyBottomHeight: 0,
            };
            let _ = DwmExtendFrameIntoClientArea(hwnd, &margins);

            self.dpi = GetDpiForWindow(hwnd) as f32;
            // Scale the default window size to the monitor DPI and force a
            // frame recalculation so WM_NCCALCSIZE kicks in.
            let scale = self.dpi / 96.0;
            let _ = SetWindowPos(
                hwnd,
                None,
                0,
                0,
                (1280.0 * scale) as i32,
                (800.0 * scale) as i32,
                SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
            );
        }
    }

    pub(crate) fn client_size_px(&self) -> (u32, u32) {
        let mut rc = RECT::default();
        unsafe {
            let _ = GetClientRect(self.hwnd, &mut rc);
        }
        ((rc.right - rc.left).max(1) as u32, (rc.bottom - rc.top).max(1) as u32)
    }

    pub(crate) fn to_dip(&self, px: f32) -> f32 {
        px * 96.0 / self.dpi
    }

    pub(crate) fn to_px(&self, dip: f32) -> f32 {
        dip * self.dpi / 96.0
    }

    pub(crate) fn invalidate(&self) {
        unsafe {
            let _ = InvalidateRect(Some(self.hwnd), None, false);
        }
    }

    /// Keeps one watcher per open Dir location (port of the C# per-view
    /// FileSystemWatcher lifecycle). Called after any navigation.
    pub(crate) fn toggle_compact_overlay(&mut self) {
        unsafe {
            match self.compact_overlay.take() {
                None => {
                    let mut placement = WINDOWPLACEMENT {
                        length: std::mem::size_of::<WINDOWPLACEMENT>() as u32,
                        ..Default::default()
                    };
                    let _ = GetWindowPlacement(self.hwnd, &mut placement);
                    self.compact_overlay = Some(placement);
                    let scale = self.dpi / 96.0;
                    let _ = SetWindowPos(
                        self.hwnd,
                        Some(HWND_TOPMOST),
                        0,
                        0,
                        (420.0 * scale) as i32,
                        (400.0 * scale) as i32,
                        SWP_NOMOVE,
                    );
                }
                Some(placement) => {
                    let _ = SetWindowPos(
                        self.hwnd,
                        Some(HWND_NOTOPMOST),
                        0,
                        0,
                        0,
                        0,
                        SWP_NOMOVE | SWP_NOSIZE,
                    );
                    let _ = SetWindowPlacement(self.hwnd, &placement);
                }
            }
        }
        self.invalidate();
    }

    /// `IWindowContext.IsCompactOverlay`.
    pub(crate) fn is_compact_overlay(&self) -> bool {
        self.compact_overlay.is_some()
    }

    /// Port of `ToggleFullScreenAction` (F11) — `AppWindow.SetPresenter
    /// (FullScreen)`: borderless fullscreen style on the current monitor,
    /// placement restored on exit.
    pub(crate) fn toggle_fullscreen(&mut self) {
        use windows::Win32::Graphics::Gdi::{
            GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
        };
        use windows::Win32::UI::WindowsAndMessaging::{
            GetWindowLongPtrW, SetWindowLongPtrW, GWL_STYLE, HWND_TOP, SWP_FRAMECHANGED,
            SWP_NOOWNERZORDER, SWP_NOZORDER, WS_OVERLAPPEDWINDOW,
        };
        unsafe {
            let style = GetWindowLongPtrW(self.hwnd, GWL_STYLE);
            match self.fullscreen.take() {
                None => {
                    let mut placement = WINDOWPLACEMENT {
                        length: std::mem::size_of::<WINDOWPLACEMENT>() as u32,
                        ..Default::default()
                    };
                    let _ = GetWindowPlacement(self.hwnd, &mut placement);
                    let mut mi = MONITORINFO {
                        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                        ..Default::default()
                    };
                    let monitor = MonitorFromWindow(self.hwnd, MONITOR_DEFAULTTONEAREST);
                    if GetMonitorInfoW(monitor, &mut mi).as_bool() {
                        self.fullscreen = Some(placement);
                        SetWindowLongPtrW(
                            self.hwnd,
                            GWL_STYLE,
                            style & !(WS_OVERLAPPEDWINDOW.0 as isize),
                        );
                        let _ = SetWindowPos(
                            self.hwnd,
                            Some(HWND_TOP),
                            mi.rcMonitor.left,
                            mi.rcMonitor.top,
                            mi.rcMonitor.right - mi.rcMonitor.left,
                            mi.rcMonitor.bottom - mi.rcMonitor.top,
                            SWP_FRAMECHANGED | SWP_NOOWNERZORDER,
                        );
                    }
                }
                Some(placement) => {
                    SetWindowLongPtrW(
                        self.hwnd,
                        GWL_STYLE,
                        style | WS_OVERLAPPEDWINDOW.0 as isize,
                    );
                    let _ = SetWindowPlacement(self.hwnd, &placement);
                    let _ = SetWindowPos(
                        self.hwnd,
                        None,
                        0,
                        0,
                        0,
                        0,
                        SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER
                            | SWP_NOOWNERZORDER,
                    );
                }
            }
        }
        self.invalidate();
    }

    pub(crate) fn is_fullscreen(&self) -> bool {
        self.fullscreen.is_some()
    }

    /// `ReopenClosedTabAction` (Ctrl+Shift+T): reopens the last closed tab.
    pub(crate) fn apply_appearance(&mut self) {
        self.theme = crate::styles::theme::from_settings();
        // Rebuild the flyout popups so their DWM dark-mode flag matches.
        self.menu_popups.clear();
        self.menu_capture = false;
        let s = crate::services::settings::get();
        let dark: i32 = (self.theme.mode == ThemeMode::Dark) as i32;
        let backdrop: i32 = s.backdrop.dwm_value();
        unsafe {
            let _ = DwmSetWindowAttribute(
                self.hwnd,
                DWMWA_USE_IMMERSIVE_DARK_MODE,
                &dark as *const _ as *const _,
                std::mem::size_of::<i32>() as u32,
            );
            let _ = DwmSetWindowAttribute(
                self.hwnd,
                DWMWA_SYSTEMBACKDROP_TYPE,
                &backdrop as *const _ as *const _,
                std::mem::size_of::<i32>() as u32,
            );
        }
        self.invalidate();
    }

}
