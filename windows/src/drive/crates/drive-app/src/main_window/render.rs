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
    pub(crate) fn layout(&self) -> Layout {
        let (w, h) = self.client_size_px();
        Layout::compute(
            self.to_dip(w as f32),
            self.to_dip(h as f32),
            &self.state,
            &self.model,
        )
    }

    pub(crate) fn render(&mut self) {
        // Keep the acrylic flyout popups in sync with state.flyout on every
        // repaint (open/close, hover, submenu).
        self.sync_flyout();
        let (w, h) = self.client_size_px();
        if self.renderer.is_none() {
            match Renderer::new(self.hwnd, w, h, self.dpi, crate::services::settings::app_theme_font_override().as_deref()) {
                Ok(r) => self.renderer = Some(r),
                Err(e) => {
                    tracing::error!("renderer creation failed: {e}");
                    return;
                }
            }
        }
        self.sync_watchers();
        // StatusCenter button/badge visibility follows in-flight operations.
        self.state.ops_count = self.ops_monitor.items.lock().unwrap().len();
        self.state.ops_active = self.state.ops_count > 0;
        // Shelf: remove items whose file has disappeared (the counterpart of
        // `ShelfViewModel`'s `IFolderWatcher`s), then reflect the list into the
        // render state (`ShelfViewModel.Items`).
        self.shelf.prune_missing();
        self.state.shelf = self
            .shelf
            .items
            .iter()
            .map(|it| (it.name.clone(), it.path.clone(), it.is_dir))
            .collect();
        // EXACT measurement, before layout, of every bar whose cells are sized
        // by their captions: the command bar, the path trail and the status
        // bar. `Layout::compute` is static and has no DirectWrite, so each of
        // them asks its own `kubuno_ui` primitive for the widths here, while a
        // `Canvas` exists, and the layout pass reuses them — one measurement,
        // one arrangement, shared by the hit test and the paint.
        if let Some(renderer) = self.renderer.as_ref() {
            if let Ok(canvas) = crate::ui::Painter::new(renderer, &self.theme) {
                crate::user_controls::toolbar::premeasure(&canvas, &self.state);
                crate::user_controls::navigation_toolbar::premeasure(&canvas, &self.state);
                crate::user_controls::status_bar::premeasure(&canvas, &self.state);
            }
        }
        let layout = self.layout();
        // Ghost + caption for an item drag in progress (`DragUIOverride`):
        // the target under the pointer yields "Move/Copy to X".
        self.state.drag = match self.item_drag.as_ref().filter(|(_, _, _, a)| *a).map(|(p, _, _, _)| p.clone()) {
            Some(paths) => {
                // The target is resolved from the LIVE pointer position (not
                // `state.hot`, which `WM_MOUSELEAVE` can clear), just like the
                // drop does on release.
                let hit = layout.hit_test(self.mouse_dip.0, self.mouse_dip.1);
                let (caption, target_rect) = match self.drop_target(hit, &layout) {
                    Some(DropTarget::Folder { rect, dest, name }) => {
                        let key = if self.drag_is_copy(&dest, &paths) {
                            "CopyToFolderCaptionText"
                        } else {
                            "MoveToFolderCaptionText"
                        };
                        (drive_localization::tr(key).replacen("{0}", &name, 1), Some(rect))
                    }
                    // "Pin to sidebar" — only if at least one dragged folder is not
                    // already pinned (`haveFoldersToPin`).
                    Some(DropTarget::Pin { rect }) if !self.folders_to_pin(&paths).is_empty() => {
                        (drive_localization::tr("PinFolderToSidebar").to_string(), Some(rect))
                    }
                    // Assign the tag (the original reuses "Create a link
                    // in {0}" with the tag name).
                    Some(DropTarget::Tag { rect, name, .. }) => {
                        (drive_localization::tr("LinkToFolderCaptionText").replacen("{0}", &name, 1), Some(rect))
                    }
                    _ => (
                        format!("{} {}", paths.len(), drive_localization::tr("ItemsLower")),
                        None,
                    ),
                };
                Some(crate::ui::DragVisual { cursor: self.mouse_dip, count: paths.len(), caption, target_rect })
            }
            None => None,
        };
        self.populate_icon_cache(&layout);
        let bg_image = self.ensure_bg_image();
        let Some(renderer) = self.renderer.as_ref() else {
            return;
        };

        unsafe {
            renderer.d2d_context.BeginDraw();
            // A transparent clear lets the Mica backdrop show through; when
            // no backdrop is available, fall back to an opaque background.
            let clear = if self.backdrop_available {
                windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 0.0 }
            } else {
                self.theme.window_background
            };
            renderer.d2d_context.Clear(Some(&clear));

            let ops: Vec<String> = self
                .ops_monitor
                .items
                .lock()
                .unwrap()
                .iter()
                .map(|(_, label)| label.clone())
                .collect();
            if let Ok(painter) = crate::ui::Painter::new(renderer, &self.theme) {
                painter.draw(&layout, &self.state, &self.model, &self.icons, self.dpi, &ops, bg_image.as_ref());
                // The drag ghost (highlighted target + pointer caption)
                // sits above the content, below the tooltip.
                if let Some(drag) = &self.state.drag {
                    painter.draw_item_drag(drag, (layout.width, layout.height));
                }
                // The tooltip sits ON TOP of everything else (the `ToolTip` is
                // a `Popup` at the top of the visual tree).
                if let Some(text) = self.tooltip_shown.as_deref() {
                    painter.draw_tooltip(text, self.mouse_dip, (layout.width, layout.height));
                }
            }

            if let Err(e) = renderer.d2d_context.EndDraw(None, None) {
                tracing::error!("EndDraw failed: {e}");
                // Device loss — rebuild everything next frame.
                self.renderer = None;
                self.icons.clear();
                self.bg_image = None;
                return;
            }
        }
        if let Err(e) = renderer.present() {
            tracing::error!("Present failed: {e}");
            self.renderer = None;
            self.icons.clear();
            self.bg_image = None;
        }
    }

    /// Loads/caches the Appearance background image for the current device.
    pub(crate) fn ensure_bg_image(&mut self) -> Option<windows::Win32::Graphics::Direct2D::ID2D1Bitmap1> {
        let src = crate::services::settings::get().app_theme_background_image_source;
        if src.is_empty() {
            self.bg_image = None;
            return None;
        }
        if let Some((key, bitmap)) = &self.bg_image {
            if *key == src {
                return bitmap.clone();
            }
        }
        let bitmap = self
            .renderer
            .as_ref()
            .and_then(|r| r.load_image_file(&src).map_err(|e| {
                tracing::warn!("background image load failed: {e}");
                e
            }).ok());
        self.bg_image = Some((src, bitmap.clone()));
        bitmap
    }

    pub(crate) fn sync_watchers(&mut self) {
        let mut open: Vec<String> = Vec::new();
        for group in &self.state.tabs {
            for pane in &group.panes {
                if let Location::Dir(path) = &pane.location {
                    let path = path.to_string_lossy().into_owned();
                    if !open.contains(&path) {
                        open.push(path);
                    }
                }
            }
        }
        self.watchers.retain(|w| open.contains(&w.path));
        for path in open {
            if !self.watchers.iter().any(|w| w.path == path) {
                self.watcher_seq += 1;
                if let Some(watcher) =
                    crate::utils::folder_watcher::DirWatcher::start(path, self.hwnd, self.watcher_seq)
                {
                    self.watchers.push(watcher);
                }
            }
        }
    }

    /// Debounced refresh of every pane showing a changed directory.
    pub(crate) fn flush_pending_refresh(&mut self) {
        let pending = std::mem::take(&mut self.pending_refresh);
        if pending.is_empty() {
            return;
        }
        for group in &mut self.state.tabs {
            for pane in &mut group.panes {
                if let Location::Dir(path) = &pane.location {
                    if pending.contains(&path.to_string_lossy().into_owned()) {
                        pane.refresh();
                    }
                }
            }
        }
        self.invalidate();
    }

    /// Prefetches shell icons for everything visible this frame, mirroring
    /// the lazy per-container icon loading of the C# layouts.
    pub(crate) fn populate_icon_cache(&mut self, layout: &crate::ui::Layout) {
        let Some(renderer) = self.renderer.as_ref() else {
            return;
        };
        let ctx = &renderer.d2d_context;
        let scale = self.dpi / 96.0;
        let row_px = (18.0 * scale).round() as i32;
        let card_px = (40.0 * scale).round() as i32;

        // Sidebar icons: shell for drives/folders/pinned items,
        // `imageres.dll` for the Drives/Network headers (like
        // `SidebarViewModel.CreateSection`).
        for (_, entry) in &layout.sidebar_items {
            use crate::ui::SidebarEntry as E;
            match entry {
                E::Pinned(i) => {
                    if let Some(item) = self.model.quick_access.get(*i) {
                        self.icons.ensure(ctx, &item.path.clone(), row_px);
                    }
                }
                E::Drive(i) | E::NetworkDrive(i) => {
                    if let Some(drive) = self.model.drives.get(*i) {
                        let root = format!("{}:\\", drive.letter);
                        self.icons.ensure(ctx, &root, row_px);
                    }
                }
                E::Folder(path, _) => {
                    let path = path.clone();
                    self.icons.ensure(ctx, &path, row_px);
                }
                E::CloudDrive(i) => {
                    if let Some(cloud) = self.model.cloud_drives.get(*i) {
                        self.icons.ensure(ctx, &cloud.sync_folder.clone(), row_px);
                    }
                }
                E::SectionDrives => {
                    self.icons.ensure_imageres(ctx, crate::ui::IMAGERES_THIS_PC, row_px);
                }
                E::SectionNetwork => {
                    self.icons.ensure_imageres(ctx, crate::ui::IMAGERES_NETWORK, row_px);
                }
                _ => {}
            }
        }

        match &self.state.active().location {
            Location::Home => {
                for item in &self.model.quick_access {
                    self.icons.ensure(ctx, &item.path.clone(), card_px);
                    self.icons.ensure(ctx, &item.path.clone(), row_px);
                }
                for drive in &self.model.drives {
                    let root = format!("{}:\\", drive.letter);
                    self.icons.ensure(ctx, &root, card_px);
                    self.icons.ensure(ctx, &root, row_px);
                }
                for item in &self.model.recent_files {
                    self.icons.ensure(ctx, &item.path.clone(), row_px);
                }
            }
            Location::Settings => {
                for item in &self.model.quick_access {
                    self.icons.ensure(ctx, &item.path.clone(), row_px);
                }
                for drive in &self.model.drives {
                    self.icons.ensure(ctx, &format!("{}:\\", drive.letter), row_px);
                }
            }
            Location::RecycleBin | Location::SearchResults { .. } => {
                // Shell icons for visible items (same sizes as Dir):
                // Recycle Bin and search results both list paths.
                let mode = self.state.active().view_mode;
                let size = crate::view_models::shell_view_model::layout_size(mode);
                let mut icon_dip = crate::view_models::shell_view_model::icon_size_for(mode, size);
                if mode == crate::view_models::shell_view_model::ViewMode::Details {
                    icon_dip = icon_dip.max(crate::ui::ICON_ROW_DIP);
                }
                let entry_px = (icon_dip * scale).round() as i32;
                let entries: Vec<String> = layout
                    .file_rows
                    .iter()
                    .zip(self.state.active().entries.iter())
                    .filter(|(rect, _)| {
                        rect.bottom >= layout.content.top && rect.top <= layout.content.bottom
                    })
                    .map(|(_, entry)| entry.path.clone())
                    .collect();
                for path in entries {
                    self.icons.ensure(ctx, &path, entry_px);
                }
            }
            Location::Dir(path) => {
                // The size ACTUALLY drawn by the active layout —
                // requesting another frame from the shell leaves the view on the
                // fallback glyph (the Card view drew at 48+ but we only
                // preloaded 18).
                let mode = self.state.active().view_mode;
                let size = crate::view_models::shell_view_model::layout_size(mode);
                let icon_dip = crate::view_models::shell_view_model::icon_size_for(mode, size);
                // The Details view draws at `icon_dip.max(ICON_ROW_DIP)` — the
                // cache key must be the SAME as the one used for drawing.
                let icon_dip = if mode == crate::view_models::shell_view_model::ViewMode::Details {
                    icon_dip.max(crate::ui::ICON_ROW_DIP)
                } else {
                    icon_dip
                };
                let entry_px = (icon_dip * scale).round() as i32;
                let dir_path = path.to_string_lossy().into_owned();
                self.icons.ensure(ctx, &dir_path, row_px);
                // Only visible rows.
                let entries: Vec<String> = layout
                    .file_rows
                    .iter()
                    .zip(self.state.active().entries.iter())
                    .filter(|(rect, _)| {
                        rect.bottom >= layout.content.top && rect.top <= layout.content.bottom
                    })
                    .map(|(_, entry)| entry.path.clone())
                    .collect();
                // FoldersPage "Show thumbnails": icons only when off.
                // Grid AND Cards views display the `FileImage` (the thumbnail).
                use crate::view_models::shell_view_model::ViewMode;
                let grid = matches!(mode, ViewMode::Grid | ViewMode::Cards)
                    && crate::services::settings::get().show_thumbnails;
                let hwnd_raw = self.hwnd.0 as isize;
                for path in entries {
                    if grid {
                        self.icons.ensure_thumbnail(ctx, &path, entry_px, hwnd_raw);
                    } else {
                        self.icons.ensure(ctx, &path, entry_px);
                    }
                }
                // Sidebar stays visible in folder views.
                for item in &self.model.quick_access {
                    self.icons.ensure(ctx, &item.path.clone(), row_px);
                }
                for drive in &self.model.drives {
                    self.icons.ensure(ctx, &format!("{}:\\", drive.letter), row_px);
                }
            }
        }

        // "Calculate folder sizes": fills in folders from the
        // SizeProvider cache and queues the ones still unknown (the
        // `TryGetSize` + `UpdateAsync` of the original Win32StorageEnumerator).
        if crate::services::settings::get().calculate_folder_sizes {
            let hwnd_raw = self.hwnd.0 as isize;
            let provider = &mut self.size_provider;
            for group in &mut self.state.tabs {
                let tab = group.active_mut();
                if !matches!(tab.location, Location::Dir(_)) {
                    continue;
                }
                for entry in tab.entries.iter_mut().filter(|e| e.is_dir && !e.size_known) {
                    if let Some(size) = provider.get(&entry.path) {
                        entry.size = size;
                        entry.size_known = true;
                    } else {
                        provider.request(&entry.path, hwnd_raw);
                    }
                }
            }
        }

        // Details pane thumbnail (selected item or current folder).
        if layout.info_pane.is_some() {
            let tab = self.state.active();
            let path = tab
                .selected_entry()
                .map(|e| e.path.clone())
                .or_else(|| match &tab.location {
                    Location::Dir(p) => Some(p.to_string_lossy().into_owned()),
                    _ => None,
                });
            if let Some(path) = path {
                let px = (140.0 * scale).round() as i32;
                self.icons.ensure_thumbnail(ctx, &path, px, self.hwnd.0 as isize);
            }
        }

        // Unfocused pane rows (dual-pane mode).
        if let (Some(rect), Some(other)) = (&layout.other_pane_rect, self.state.group().other()) {
            let paths: Vec<String> = layout
                .other_rows
                .iter()
                .zip(other.entries.iter())
                .filter(|(row, _)| row.bottom >= rect.top && row.top <= rect.bottom)
                .map(|(_, entry)| entry.path.clone())
                .collect();
            for path in paths {
                self.icons.ensure(ctx, &path, row_px);
            }
        }
    }

}
