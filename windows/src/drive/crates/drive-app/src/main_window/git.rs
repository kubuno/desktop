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
    pub(crate) fn git_dir(&self) -> Option<String> {
        match &self.state.active().location {
            Location::Dir(p) => Some(p.to_string_lossy().into_owned()),
            _ => None,
        }
    }

    /// Executes a blocking git operation (pull/push/sync/checkout/…) on a
    /// worker thread — like the original's `DoGitOperationAsync` — then
    /// signals completion via `WM_APP_GIT_DONE` to refresh the tab.
    pub(crate) fn run_git_op<F>(&self, op: F)
    where
        F: FnOnce() -> std::result::Result<(), git2::Error> + Send + 'static,
    {
        let hwnd = self.hwnd.0 as isize;
        std::thread::spawn(move || {
            if let Err(e) = op() {
                tracing::warn!("git operation failed: {e}");
            }
            unsafe {
                let _ = PostMessageW(
                    Some(HWND(hwnd as *mut std::ffi::c_void)),
                    WM_APP_GIT_DONE,
                    WPARAM(0),
                    LPARAM(0),
                );
            }
        });
    }

    /// Opens the git network actions flyout (Pull / Push / Sync), anchored on the
    /// status bar's `GitNetworkActions` button.
    pub(crate) fn open_git_actions_flyout(&mut self, anchor: crate::ui::Rect) {
        use crate::ui::FlyoutItem;
        let tr = drive_localization::tr;
        let items = vec![
            FlyoutItem::new(tr("GitPull").to_string(), true).with_icon("Git.Pull"),
            FlyoutItem::new(tr("Push").to_string(), true).with_icon("Git.Push"),
            FlyoutItem::new(tr("GitSync").to_string(), true).with_icon("Git.Sync"),
        ];
        let width = self.flyout_width(&items);
        self.state.flyout = Some(crate::ui::Flyout {
            kind: crate::ui::FlyoutKind::GitActions,
            width,
            x: (anchor.right - width).max(0.0),
            y: anchor.top - 4.0, // opens UPWARD (status bar at the bottom)
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
        // Anchor above: shift up by the panel's height.
        if let Some(f) = self.state.flyout.as_mut() {
            let h = f.panel_rect().bottom - f.panel_rect().top;
            f.y = (anchor.top - h - 2.0).max(0.0);
        }
        self.invalidate();
    }

    /// Opens the branch picker flyout: the list of branches (checkmark
    /// on HEAD), a separator, then "Create branch".
    pub(crate) fn open_git_branches_flyout(&mut self, anchor: crate::ui::Rect) {
        use crate::ui::FlyoutItem;
        let tr = drive_localization::tr;
        let Some(dir) = self.git_dir() else { return };
        self.state.git_branches = crate::utils::git::branches(&dir);
        // Local and remote in a single list (1:1 index mapping with
        // `git_branches`). HEAD carries its ahead/behind vs the tracked
        // upstream (`↓ behind  ↑ ahead`); remotes carry their own icon.
        let mut items: Vec<FlyoutItem> = self
            .state
            .git_branches
            .iter()
            .map(|b| {
                let mut item = FlyoutItem::new(b.name.clone(), true).checked(b.is_head);
                if b.is_remote {
                    item = item.with_glyph("\u{E753}"); // Cloud: remote branch.
                }
                if b.is_head && (b.ahead_by > 0 || b.behind_by > 0) {
                    item = item.with_accel(format!("\u{2193}{}  \u{2191}{}", b.behind_by, b.ahead_by));
                }
                item
            })
            .collect();
        items.push(FlyoutItem::separator());
        items.push(FlyoutItem::new(tr("CreateBranch").to_string(), true).with_icon("Git.Branch"));
        let width = self.flyout_width(&items).max(200.0);
        self.state.flyout = Some(crate::ui::Flyout {
            kind: crate::ui::FlyoutKind::GitBranches,
            width,
            x: (anchor.right - width).max(0.0),
            y: anchor.top,
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
        if let Some(f) = self.state.flyout.as_mut() {
            let h = f.panel_rect().bottom - f.panel_rect().top;
            f.y = (anchor.top - h - 2.0).max(0.0);
        }
        self.invalidate();
    }

    /// `GitCreateBranchDialog`: the "Name" field for a new branch.
    pub(crate) fn open_create_branch_dialog(&mut self) {
        let tr = drive_localization::tr;
        let mut dialog = crate::dialogs::DialogState::simple(
            tr("CreateBranch").to_string(),
            tr("NewBranch").to_string(),
            tr("Create").to_string(),
            tr("Cancel").to_string(),
            crate::dialogs::DialogAction::GitCreateBranch,
        );
        dialog.field_label = Some(tr("Name").to_string());
        self.state.dialog = Some(dialog);
        self.state.edit = Some(crate::ui::EditState {
            entry: crate::ui::EDIT_DIALOG,
            caret: 0,
            anchor: 0,
            text: String::new(),
        });
        self.invalidate();
    }

}
