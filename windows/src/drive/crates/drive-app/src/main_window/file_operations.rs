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
    pub(crate) fn drop_target(
        &self,
        hot: Option<Hot>,
        layout: &crate::ui::Layout,
    ) -> Option<DropTarget> {
        let folder = |rect: crate::ui::Rect, dest: std::path::PathBuf, name: String| {
            DropTarget::Folder { rect, dest, name }
        };
        match hot? {
            Hot::FileRow(i) => {
                let e = self.state.active().entries.get(i)?;
                if !e.is_dir {
                    return None;
                }
                Some(folder(*layout.file_rows.get(i)?, e.path.clone().into(), e.name.clone()))
            }
            Hot::SidebarItem(i) => {
                let (rect, entry) = layout.sidebar_items.get(i)?;
                match entry {
                    // Dropping on the "Pinned" header = pin the dragged
                    // folders (`HandleLocationItemDroppedAsync`, Pinned branch).
                    SidebarEntry::SectionPinned => Some(DropTarget::Pin { rect: *rect }),
                    // Dropping on a tag = assign it to the dragged files
                    // (`HandleTagItemDroppedAsync`).
                    SidebarEntry::Tag(idx) => {
                        let tag = crate::services::settings::get().file_tags.get(*idx)?.clone();
                        Some(DropTarget::Tag { rect: *rect, uid: tag.uid, name: tag.name })
                    }
                    _ => {
                        let (path, name) = self.sidebar_entry_dest(entry)?;
                        Some(folder(*rect, path, name))
                    }
                }
            }
            Hot::Breadcrumb(k) => {
                let segs = self.state.active().breadcrumbs();
                let (name, path) = segs.get(layout.breadcrumb_start + k)?.clone();
                Some(folder(*layout.breadcrumbs.get(k)?, path, name))
            }
            Hot::InactivePane => {
                let other = self.state.group().other()?;
                match &other.location {
                    Location::Dir(p) => Some(folder(layout.other_pane_rect?, p.clone(), folder_display_name(p.as_path()))),
                    _ => None,
                }
            }
            Hot::Tab(i) => {
                let g = self.state.tabs.get(i)?;
                match &g.active().location {
                    Location::Dir(p) => Some(folder(*layout.tabs.get(i)?, p.clone(), folder_display_name(p.as_path()))),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    /// The destination folder for a sidebar entry (pinned folder,
    /// drive, expanded folder). `None` for Home / settings / tags /
    /// headers / Recycle Bin (non-folders).
    pub(crate) fn sidebar_entry_dest(
        &self,
        entry: &SidebarEntry,
    ) -> Option<(std::path::PathBuf, String)> {
        let path: std::path::PathBuf = match entry {
            SidebarEntry::Pinned(idx) => self.model.quick_access.get(*idx)?.path.clone().into(),
            SidebarEntry::Drive(idx) | SidebarEntry::NetworkDrive(idx) => {
                format!("{}:\\", self.model.drives.get(*idx)?.letter).into()
            }
            SidebarEntry::Folder(path, _) => path.clone().into(),
            _ => return None,
        };
        // Only a real folder accepts a drop (excludes the Recycle Bin, etc.).
        if !path.is_dir() {
            return None;
        }
        let name = folder_display_name(&path);
        Some((path, name))
    }

    /// Executes the drop: filters out invalid targets (dropping onto itself or
    /// into its own subtree), picks move/copy, then launches
    /// the shell operation in the background (`paste_into_async`, like `Item_Drop`).
    pub(crate) fn perform_drop(&mut self, sources: &[std::path::PathBuf], dest: &std::path::Path) {
        // Never drop an item onto itself or into one of its descendants.
        let valid: Vec<String> = sources
            .iter()
            .filter(|s| {
                s.as_path() != dest
                    && !dest.starts_with(s.as_path())
                    && s.parent() != Some(dest)
            })
            .map(|s| s.to_string_lossy().into_owned())
            .collect();
        if valid.is_empty() {
            return;
        }
        let is_move = !self.drag_is_copy(dest, sources);
        // Goes through conflict detection (dialog if a name already exists).
        self.begin_paste(valid, is_move, dest.to_string_lossy().into_owned());
    }

    /// The dragged folders that can be pinned (directories not already
    /// pinned) — `haveFoldersToPin` from `HandleLocationItemDragOverAsync`.
    pub(crate) fn folders_to_pin<'a>(&self, sources: &'a [std::path::PathBuf]) -> Vec<&'a std::path::PathBuf> {
        let pinned: Vec<String> = self
            .model
            .quick_access
            .iter()
            .map(|q| q.path.to_lowercase())
            .collect();
        sources
            .iter()
            .filter(|s| s.is_dir() && !pinned.contains(&s.to_string_lossy().to_lowercase()))
            .collect()
    }

    /// Pins the dragged folders onto the "Pinned" header
    /// (`QuickAccessService.PinToSidebarAsync` → shell verb `pintohome`).
    pub(crate) fn perform_pin_drop(&mut self, sources: &[std::path::PathBuf]) {
        let to_pin = self.folders_to_pin(sources);
        if to_pin.is_empty() {
            return;
        }
        for path in to_pin {
            crate::view_models::shell_view_model::shell_verb(&path.to_string_lossy(), "pintohome");
        }
        self.model = crate::data::items::HomeModel::load();
    }

    /// Assigns the `uid` tag to the dragged files (`HandleTagItemDroppedAsync`:
    /// reads the tags, adds the uid if missing, rewrites).
    pub(crate) fn perform_tag_drop(&mut self, sources: &[std::path::PathBuf], uid: &str) {
        for path in sources {
            let p = path.to_string_lossy();
            let mut tags = crate::utils::file_tags::read_file_tags(&p);
            if !tags.iter().any(|t| t == uid) {
                tags.push(uid.to_string());
                crate::utils::file_tags::write_file_tags(&p, &tags);
            }
        }
    }

    /// The operation chosen for a drop (counterpart of the `Item_DragOver` cascade):
    /// Ctrl forces copy, Shift forces move; otherwise the same drive
    /// moves, a different drive copies.
    pub(crate) fn drag_is_copy(&self, dest: &std::path::Path, sources: &[std::path::PathBuf]) -> bool {
        use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VK_CONTROL, VK_SHIFT};
        let ctrl = unsafe { GetKeyState(VK_CONTROL.0 as i32) } < 0;
        let shift = unsafe { GetKeyState(VK_SHIFT.0 as i32) } < 0;
        if ctrl {
            return true;
        }
        if shift {
            return false;
        }
        match sources.first() {
            Some(src) => !same_drive(dest, src),
            None => false,
        }
    }

    pub(crate) fn paste_clipboard(&mut self) {
        let Location::Dir(dir) = &self.state.active().location else {
            return;
        };
        let dir = dir.to_string_lossy().into_owned();
        self.paste_clipboard_into(dir);
    }

    /// `PasteItemToSelectionAction`: paste into an arbitrary folder.
    pub(crate) fn paste_clipboard_into(&mut self, dir: String) {
        if let Some((paths, is_move)) = crate::utils::storage::clipboard_get_files(self.hwnd) {
            if !paths.is_empty() {
                self.begin_paste(paths, is_move, dir);
            }
        }
    }

    /// Detects name conflicts BEFORE launching copy/move
    /// (`FilesystemHelpers.GetCollisions`). If there are none, paste
    /// directly; otherwise open the `FilesystemOperationDialog` (modal), and
    /// `conflict_continue` relaunches the resolved operation.
    pub(crate) fn begin_paste(&mut self, sources: Vec<String>, is_move: bool, dest_dir: String) {
        use crate::dialogs::{ConflictDialog, ConflictItem, ConflictResolve};
        let dest_path = std::path::Path::new(&dest_dir);
        let mut items = Vec::new();
        let mut conflicts = 0usize;
        for src in &sources {
            let name = std::path::Path::new(src)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let target = dest_path.join(&name);
            let is_conflict = target.exists() && std::path::Path::new(src) != target.as_path();
            if is_conflict {
                conflicts += 1;
            }
            items.push(ConflictItem {
                source: src.clone(),
                source_name: name.clone(),
                dest_name: name,
                is_conflict,
                // Default "Generate a name" like the original.
                resolve: ConflictResolve::GenerateNewName,
            });
        }
        if conflicts == 0 {
            self.ops_seq += 1;
            crate::utils::storage::paste_into_async(
                sources,
                is_move,
                dest_dir,
                self.hwnd.0 as isize,
                std::sync::Arc::clone(&self.ops_monitor),
                self.ops_seq,
            );
            return;
        }
        let non_conflict = items.len() - conflicts;
        let title = if conflicts == 1 {
            "1 fichier en conflit".to_string()
        } else {
            format!("{conflicts} fichiers en conflit")
        };
        let mut description = if conflicts == 1 {
            "Il y a un nom de fichier en conflit.".to_string()
        } else {
            format!("Il y a {conflicts} noms de fichiers en conflit.")
        };
        if non_conflict > 0 {
            description.push_str(&format!(" {non_conflict} autre(s) élément(s) sans conflit."));
        }
        let mut dialog = ConflictDialog { title, description, items, is_move, dest_dir, aggregated: None };
        dialog.sync_aggregated();
        self.state.conflict = Some(dialog);
        self.state.conflict_scroll = 0.0;
        self.invalidate();
    }

    /// "Continue" from the conflict dialog: applies each item's resolution
    /// (Skip = removed, Replace = same name + overwrite, Generate =
    /// pre-computed unique name) then relaunches the operation.
    pub(crate) fn conflict_continue(&mut self) {
        use crate::dialogs::ConflictResolve;
        let Some(dialog) = self.state.conflict.take() else {
            return;
        };
        let dest_dir = dialog.dest_dir.clone();
        let dest_path = std::path::Path::new(&dest_dir);
        let mut resolved: Vec<(String, Option<String>)> = Vec::new();
        for item in &dialog.items {
            if !item.is_conflict {
                resolved.push((item.source.clone(), None));
                continue;
            }
            match item.resolve {
                ConflictResolve::Skip => {}
                ConflictResolve::ReplaceExisting => resolved.push((item.source.clone(), None)),
                ConflictResolve::GenerateNewName => {
                    let unique = crate::utils::storage::unique_path(&dest_path.join(&item.source_name));
                    let new_name = unique.file_name().map(|n| n.to_string_lossy().into_owned());
                    resolved.push((item.source.clone(), new_name));
                }
            }
        }
        self.invalidate();
        if resolved.is_empty() {
            return;
        }
        self.ops_seq += 1;
        crate::utils::storage::paste_resolved_async(
            resolved,
            dialog.is_move,
            dest_dir,
            self.hwnd.0 as isize,
            std::sync::Arc::clone(&self.ops_monitor),
            self.ops_seq,
        );
    }

}
