#![allow(unused_imports)]
//! Sidebar clicks (mirror of `ViewModels/UserControls/SidebarViewModel.cs`) — `impl MainWindow` block, see `main_window/mod.rs`.
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
    /// Click on a sidebar item (the `SidebarItem` branch of `on_click`).
    pub(crate) fn on_click_sidebar_item(&mut self, i: usize, layout: &Layout, x_px: f32) {
                if let Some((rect, entry)) = layout.sidebar_items.get(i) {
                    // In COMPACT mode, clicking a section header re-expands the
                    // sidebar (the original opens the children in a flyout — re-expanding
                    // stands in for that for now).
                    if crate::services::settings::get().sidebar_compact
                        && matches!(
                            entry,
                            SidebarEntry::SectionPinned
                                | SidebarEntry::SectionDrives
                                | SidebarEntry::SectionCloudDrives
                                | SidebarEntry::SectionNetwork
                                | SidebarEntry::SectionTags
                        )
                    {
                        crate::services::settings::update(|s| s.sidebar_compact = false);
                        self.invalidate();
                        return;
                    }
                    // The chevron zone, BEFORE the row (to the left of
                    // the icon): it expands instead of navigating.
                    let x_dip = self.to_dip(x_px);
                    let chevron_at = |indent: f32| x_dip < rect.left + indent;
                    match entry {
                        SidebarEntry::Home => self.navigate_active(Location::Home),
                        SidebarEntry::Pinned(idx) => {
                            let item = self.model.quick_access[*idx].clone();
                            self.open_location(&item.path);
                        }
                        SidebarEntry::Drive(idx) => {
                            let root = format!("{}:\\", self.model.drives[*idx].letter);
                            if chevron_at(48.0) {
                                self.toggle_sidebar_folder(&root);
                            } else {
                                self.navigate_active(Location::Dir(root.into()));
                            }
                        }
                        SidebarEntry::Folder(path, depth) => {
                            let path = path.clone();
                            if chevron_at(48.0 + 16.0 * *depth as f32) {
                                self.toggle_sidebar_folder(&path);
                            } else {
                                self.navigate_active(Location::Dir(path.into()));
                            }
                        }
                        SidebarEntry::Settings => {
                            self.navigate_active(Location::Settings);
                        }
                        // Opening a library: v1 = its default save
                        // folder (aggregation coming later).
                        SidebarEntry::Library(idx) => {
                            if let Some(lib) = self.model.libraries.get(*idx) {
                                if let Some(dir) = lib
                                    .default_save_folder
                                    .clone()
                                    .or_else(|| lib.folders.first().cloned())
                                {
                                    self.navigate_active(Location::Dir(dir.into()));
                                }
                            }
                        }
                        SidebarEntry::SectionLibraries => {
                            crate::services::settings::update(|s| {
                                s.is_library_section_expanded = !s.is_library_section_expanded
                            });
                            self.invalidate();
                        }
                        SidebarEntry::NetworkDrive(idx) => {
                            let root = format!("{}:\\", self.model.drives[*idx].letter);
                            self.navigate_active(Location::Dir(root.into()));
                        }
                        // Tag-based navigation (`tag:`) will come with
                        // search; the entry is visible but inert.
                        SidebarEntry::Tag(_) => {}
                        // The section header toggles its expanded state
                        // (`IsXxxSectionExpanded`), persisted.
                        SidebarEntry::SectionPinned => {
                            crate::services::settings::update(|s| {
                                s.is_pinned_section_expanded = !s.is_pinned_section_expanded
                            });
                            self.invalidate();
                        }
                        SidebarEntry::SectionDrives => {
                            crate::services::settings::update(|s| {
                                s.is_drive_section_expanded = !s.is_drive_section_expanded
                            });
                            self.invalidate();
                        }
                        SidebarEntry::SectionCloudDrives => {
                            crate::services::settings::update(|s| {
                                s.is_cloud_drive_section_expanded = !s.is_cloud_drive_section_expanded
                            });
                            self.invalidate();
                        }
                        SidebarEntry::CloudDrive(idx) => {
                            let folder = self.model.cloud_drives[*idx].sync_folder.clone();
                            if chevron_at(48.0) {
                                self.toggle_sidebar_folder(&folder);
                            } else {
                                self.navigate_active(Location::Dir(folder.into()));
                            }
                        }
                        SidebarEntry::SectionNetwork => {
                            crate::services::settings::update(|s| {
                                s.is_network_section_expanded = !s.is_network_section_expanded
                            });
                            self.invalidate();
                        }
                        SidebarEntry::SectionTags => {
                            crate::services::settings::update(|s| {
                                s.is_file_tags_section_expanded = !s.is_file_tags_section_expanded
                            });
                            self.invalidate();
                        }
                    }
                }
    }
}
