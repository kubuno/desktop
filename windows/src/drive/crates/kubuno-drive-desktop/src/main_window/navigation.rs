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
    pub(crate) fn navigate_active(&mut self, location: Location) {
        // `WindowsJumpListService.AddFolderAsync` on folder change:
        // only real folders enter the "Recent" list.
        if let Location::Dir(p) = &location {
            crate::services::jump_list::add_recent_folder(&p.to_string_lossy());
        }
        self.state.active_mut().navigate(location);
        self.invalidate();
    }

    /// `WindowsJumpListService.RefreshPinnedFoldersAsync`: (re)sets the
    /// taskbar's "Pinned" category from Quick Access.
    pub(crate) fn refresh_jump_list_pinned(&self) {
        let pinned: Vec<String> = self
            .model
            .quick_access
            .iter()
            .filter(|q| q.is_pinned)
            .map(|q| q.path.clone())
            .collect();
        crate::services::jump_list::refresh_pinned(&pinned);
    }

    /// Resolves the target of an item drag under the hovered point: the
    /// rectangle to highlight, the destination folder, its display name.
    /// `None` if this isn't a valid target (plain file, empty area,
    /// section header, Home, settings…). Counterpart of `GetItemFromElement`
    /// + the sidebar's / breadcrumb's `DragOver`.
    pub(crate) fn begin_filter_edit(&mut self) {
        let current = self.state.active().filter.clone();
        self.state.edit = Some(crate::ui::EditState {
            entry: crate::ui::EDIT_SEARCH,
            caret: current.len(),
            anchor: 0,
            text: current,
        });
        self.invalidate();
    }

    /// A ComboBox's dropdown menu: OUR acrylic flyout (items styled like
    /// `ComboBoxItem`, accent pill on selection), presented MODALLY —
    /// a nested message loop returns control when the menu closes,
    /// exactly the contract of the old `TrackPopupMenuEx`. Returns the
    /// choice 1-based, or 0 if the menu is dismissed.
    /// Navigates in-app for real directories, otherwise defers to the shell
    /// (e.g. shell: locations like the Recycle Bin).
    pub(crate) fn open_location(&mut self, path: &str) {
        if path.eq_ignore_ascii_case("shell:RecycleBinFolder") {
            // The Recycle Bin is an app VIEW (`ContentPageTypes.RecycleBin`),
            // no longer a delegation to Explorer.
            self.navigate_active(Location::RecycleBin);
        } else if browsable(path) {
            self.navigate_active(Location::Dir(path.into()));
        } else {
            shell_open(path);
        }
    }

    /// Omnibar path-mode suggestions: subdirectories of the deepest existing
    /// directory in the typed path, labelled relative to their parent.
    pub(crate) fn update_path_suggestions(&mut self) {
        self.state.path_suggestions.clear();
        let Some(edit) = self.state.edit.as_ref().filter(|e| e.entry == crate::ui::EDIT_PATH) else {
            return;
        };
        let typed = edit.text.replace('/', "\\");
        let (dir, prefix) = match typed.rfind('\\') {
            Some(pos) if std::path::Path::new(&typed[..pos.max(3)]).is_dir() => {
                (typed[..pos.max(3)].to_string(), typed[pos + 1..].to_lowercase())
            }
            _ if std::path::Path::new(&typed).is_dir() => (typed.clone(), String::new()),
            _ => return,
        };
        let parent_label = std::path::Path::new(&dir)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| dir.trim_end_matches('\\').to_string());

        // First row: the directory itself.
        self.state.path_suggestions.push((parent_label.clone(), dir.clone()));
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                if self.state.path_suggestions.len() > 9 {
                    break;
                }
                let name = entry.file_name().to_string_lossy().into_owned();
                if !prefix.is_empty() && !name.to_lowercase().starts_with(&prefix) {
                    continue;
                }
                if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                    self.state.path_suggestions.push((
                        format!("{parent_label}\\{name}"),
                        entry.path().to_string_lossy().into_owned(),
                    ));
                }
            }
        }
    }

    /// Closes a tab, remembering its location for ReopenClosedTab; the last
    /// tab closes the window (same as the original TabView behavior).
    /// History flyout of the Back/Forward buttons (right-click / long press in
    /// the original): lists the tab history and jumps on selection.
    pub(crate) fn show_history_menu(&mut self, forward: bool, x_px: f32, y_px: f32) {
        let tab = self.state.active();
        let label = |loc: &Location| -> String {
            match loc {
                Location::Home => kubuno_drive_desktop_localization::tr("Home").to_string(),
                Location::Settings => kubuno_drive_desktop_localization::tr("Settings").to_string(),
                Location::RecycleBin => kubuno_drive_desktop_localization::tr("RecycleBin").to_string(),
                Location::SearchResults { root, .. } => std::path::Path::new(root)
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| root.clone()),
                Location::Dir(p) => p
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| p.to_string_lossy().into_owned()),
            }
        };
        // Back: previous entries, most recent first. Forward: next entries.
        let indices: Vec<usize> = if forward {
            (tab.history_index + 1..tab.history.len()).collect()
        } else {
            (0..tab.history_index).rev().collect()
        };
        if indices.is_empty() {
            return;
        }
        let labels: Vec<String> = indices.iter().map(|&i| label(&tab.history[i])).collect();
        let items: Vec<(&str, bool)> = labels.iter().map(|l| (l.as_str(), false)).collect();
        let choice = self.dropdown(&items, x_px, y_px);
        if let Some(&index) = choice.checked_sub(1).and_then(|c| indices.get(c)) {
            self.state.active_mut().go_to_history(index);
            self.invalidate();
        }
    }

    /// Switches the omnibar to path-edit mode (EditPath action).
    pub(crate) fn begin_path_mode(&mut self) {
        let current = match &self.state.active().location {
            Location::Dir(p) => p.to_string_lossy().into_owned(),
            _ => String::new(),
        };
        self.state.edit = Some(crate::ui::EditState {
            entry: crate::ui::EDIT_PATH,
            caret: current.len(),
            anchor: 0,
            text: current,
        });
        self.update_path_suggestions();
        self.invalidate();
    }

    /// Switches the omnibar to command-palette mode (OpenCommandPalette).
    pub(crate) fn begin_palette_mode(&mut self) {
        self.state.edit = Some(crate::ui::EditState {
            entry: crate::ui::EDIT_PALETTE,
            caret: 0,
            anchor: 0,
            text: String::new(),
        });
        self.update_palette_suggestions();
        self.invalidate();
    }

    /// The command-palette entries: (localized label, command id). The ids
    /// are dispatched by `run_palette_command`.
    pub(crate) fn palette_commands() -> Vec<(String, String)> {
        // The registry (`CommandManager`): the palette shows the DESCRIPTION
        // of each action, like the original.
        crate::actions::commands()
            .iter()
            .map(|(code, action)| {
                (
                    kubuno_drive_desktop_localization::tr(action.description()).to_string(),
                    format!("action:{code}"),
                )
            })
            .collect()
    }

    /// Palette suggestions: commands whose label contains the typed text.
    pub(crate) fn update_palette_suggestions(&mut self) {
        self.state.path_suggestions.clear();
        let Some(edit) = self.state.edit.as_ref().filter(|e| e.entry == crate::ui::EDIT_PALETTE) else {
            return;
        };
        // The original (`NavigationToolbarViewModel.cs:1052-1055`,
        // PopulateOmnibarSuggestionsForCommandPaletteMode) filters with
        // `Contains(..., StringComparison.OrdinalIgnoreCase)`: case-insensitive
        // but NOT accent-insensitive. Deliberate deviation (ergonomics): we also
        // fold French diacritics on BOTH sides, so that "defaut"
        // matches "Utiliser le thème par défaut".
        fn fold_diacritics(s: &str) -> String {
            let mut out = String::with_capacity(s.len());
            for c in s.chars() {
                match c {
                    'é' | 'è' | 'ê' | 'ë' => out.push('e'),
                    'à' | 'â' | 'ä' => out.push('a'),
                    'î' | 'ï' => out.push('i'),
                    'ô' | 'ö' => out.push('o'),
                    'ù' | 'û' | 'ü' => out.push('u'),
                    'ç' => out.push('c'),
                    'œ' => out.push_str("oe"),
                    'æ' => out.push_str("ae"),
                    _ => out.push(c),
                }
            }
            out
        }
        // `to_lowercase` first: "É" becomes "é", then "e".
        let needle = fold_diacritics(&edit.text.to_lowercase());
        for (label, id) in Self::palette_commands() {
            if self.state.path_suggestions.len() > 9 {
                break;
            }
            if needle.is_empty() || fold_diacritics(&label.to_lowercase()).contains(&needle) {
                self.state.path_suggestions.push((label, format!("cmd:{id}")));
            }
        }
    }

    /// Executes a palette command id (see `palette_commands`).
    pub(crate) fn run_palette_command(&mut self, id: &str) {
        // Everything goes through the action registry (`CommandManager[string]`).
        let Some(code) = id.strip_prefix("action:") else { return };
        if let Some((_, action)) = crate::actions::commands().iter().find(|(c, _)| *c == code) {
            if action.is_executable(self) {
                action.execute(self, None);
            }
        }
        self.invalidate();
    }

}
