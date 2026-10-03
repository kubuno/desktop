#![allow(unused_imports)]
//! Toolbar button clicks (mirror of `UserControls/NavigationToolbar.xaml.cs`) — `impl MainWindow` block, see `main_window/mod.rs`.
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
    /// Flyout for the toolbar's "New" button (the `CmdNew` branch of `on_click`).
    pub(crate) fn on_click_cmd_new(&mut self, layout: &Layout) {
                let anchor = layout
                    .cmd_buttons
                    .iter()
                    .find(|(_, h)| *h == Hot::CmdNew)
                    .map(|(r, _)| *r)
                    .unwrap_or_default();
                self.state.flyout = Some(crate::ui::Flyout {
                    kind: crate::ui::FlyoutKind::NewMenu,
                    width: crate::ui::FLYOUT_WIDTH,
                    x: anchor.left,
                    y: anchor.bottom + 2.0,
                    items: vec![
                        crate::ui::FlyoutItem {
                            glyph: "",
                            label: kubuno_drive_desktop_localization::tr("Folder").to_string(),
                            accel: Some("Ctrl+Maj+N".to_string()),
                            enabled: true,
                            has_submenu: false,
                            icon: Some("Folder"),
                            bitmap: None,
                            separator: false,
                            is_toggle: false,
                            checked: false,
                            pill: false,
                            command: crate::ui::MenuCommand::None,
                            children: Vec::new(),
                            children_width: crate::ui::SUBMENU_WIDTH,
                        },
                        crate::ui::FlyoutItem {
                            glyph: "",
                            label: "Document texte".to_string(),
                            accel: None,
                            enabled: true,
                            has_submenu: false,
                            icon: Some("File"),
                            bitmap: None,
                            separator: false,
                            is_toggle: false,
                            checked: false,
                            pill: false,
                            command: crate::ui::MenuCommand::None,
                            children: Vec::new(),
                            children_width: crate::ui::SUBMENU_WIDTH,
                        },
                    ],
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
                self.invalidate();
    }

    /// Flyout for the "Selection options" button (the `CmdSelOptions` branch).
    pub(crate) fn on_click_cmd_sel_options(&mut self, layout: &Layout) {
                let anchor = layout
                    .cmd_buttons
                    .iter()
                    .find(|(_, h)| *h == Hot::CmdSelOptions)
                    .map(|(r, _)| *r)
                    .unwrap_or_default();
                let make = |key: &'static str, accel: Option<&'static str>, enabled: bool| crate::ui::FlyoutItem {
                    glyph: "\u{E8B3}",
                    label: kubuno_drive_desktop_localization::tr(key).to_string(),
                    accel: accel.map(str::to_string),
                    enabled,
                    has_submenu: false,
                    icon: None,
                    bitmap: None,
                    separator: false,
                    is_toggle: false,
                    checked: false,
                    pill: false,
                    command: crate::ui::MenuCommand::None,
                    children: Vec::new(),
                    children_width: crate::ui::SUBMENU_WIDTH,
                };
                self.state.flyout = Some(crate::ui::Flyout {
                    kind: crate::ui::FlyoutKind::SelectionMenu,
                    width: crate::ui::FLYOUT_WIDTH,
                    x: (anchor.right - crate::ui::FLYOUT_WIDTH).max(0.0),
                    y: anchor.bottom + 2.0,
                    items: vec![
                        make("SelectAll", Some("Ctrl+A"), true),
                        make("InvertSelection", None, true),
                        make("ClearSelection", None, true),
                    ],
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
                self.invalidate();
    }

    /// Flyout for the "Sort" button (the `CmdSort` branch).
    pub(crate) fn on_click_cmd_sort(&mut self, layout: &Layout) {
                // The flyout of Toolbar.xaml's "Sort" button: a single
                // MenuFlyout carrying the "Sort by" and "Group
                // by" submenus, a separator, then the folders/files radio trio.
                // (There is NO separate "Group" button.)
                use crate::actions::display::sort_files_first_action as sf;
                use crate::actions::{Action, ToggleAction};
                use crate::services::settings::GroupByDateUnit as U;
                use crate::ui::{FlyoutItem, MenuCommand};
                use crate::view_models::shell_view_model::{GroupOption as G, SortColumn as S};
                let tr = kubuno_drive_desktop_localization::tr;
                let anchor = layout
                    .cmd_buttons
                    .iter()
                    .find(|(_, h)| *h == Hot::CmdSort)
                    .map(|(r, _)| *r)
                    .unwrap_or_default();

                let (sort_col, sort_asc, group_opt, group_asc, date_unit) = {
                    let t = self.state.active();
                    (t.sort_column, t.sort_ascending, t.group_option, t.group_ascending, t.group_by_date_unit)
                };
                let grouped = group_opt != G::None;

                // Level 2 — "Sort by": columns then separator, direction.
                let sort_by = vec![
                    FlyoutItem::new(tr("Name").to_string(), true)
                        .checked(sort_col == S::Name)
                        .with_command(MenuCommand::SortByName),
                    FlyoutItem::new(tr("DateModifiedLowerCase").to_string(), true)
                        .checked(sort_col == S::DateModified)
                        .with_command(MenuCommand::SortByDateModified),
                    FlyoutItem::new(tr("Type").to_string(), true)
                        .checked(sort_col == S::Type)
                        .with_command(MenuCommand::SortByType),
                    FlyoutItem::new(tr("Size").to_string(), true)
                        .checked(sort_col == S::Size)
                        .with_command(MenuCommand::SortBySize),
                    FlyoutItem::separator(),
                    FlyoutItem::new(tr("Ascending").to_string(), true)
                        .checked(sort_asc)
                        .with_command(MenuCommand::SortAscending),
                    FlyoutItem::new(tr("Descending").to_string(), true)
                        .checked(!sort_asc)
                        .with_command(MenuCommand::SortDescending),
                ];

                // Level 3 — "Date modified" › Year / Month / Day.
                let date_children = vec![
                    FlyoutItem::new(tr("Year").to_string(), true)
                        .checked(group_opt == G::DateModified && date_unit == U::Year),
                    FlyoutItem::new(tr("Month").to_string(), true)
                        .checked(group_opt == G::DateModified && date_unit == U::Month),
                    FlyoutItem::new(tr("Day").to_string(), true)
                        .checked(group_opt == G::DateModified && date_unit == U::Day),
                ];

                // Level 2 — "Group by": radio options (Date
                // modified expanding into Year/Month/Day), separator, direction.
                let mut group_by = vec![
                    FlyoutItem::new(tr("None").to_string(), true).checked(group_opt == G::None),
                    FlyoutItem::new(tr("Name").to_string(), true).checked(group_opt == G::Name),
                    FlyoutItem::new(tr("DateModifiedLowerCase").to_string(), true)
                        .checked(group_opt == G::DateModified)
                        .with_children(date_children),
                    FlyoutItem::new(tr("Type").to_string(), true).checked(group_opt == G::Type),
                    FlyoutItem::new(tr("Size").to_string(), true).checked(group_opt == G::Size),
                    FlyoutItem::separator(),
                    FlyoutItem::new(tr("Ascending").to_string(), grouped).checked(group_asc && grouped),
                    FlyoutItem::new(tr("Descending").to_string(), grouped).checked(!group_asc && grouped),
                ];

                // Panel widths: measured against their widest label.
                let sort_w = self.flyout_width(&sort_by);
                let group_w = self.flyout_width(&group_by);
                let date_w = self.flyout_width(&group_by[2].children);
                group_by[2].children_width = date_w;

                let mut items = vec![
                    FlyoutItem::new(tr("SortBy").to_string(), true).with_children(sort_by),
                    FlyoutItem::new(tr("GroupBy").to_string(), true).with_children(group_by),
                    FlyoutItem::separator(),
                ];
                items[0].children_width = sort_w;
                items[1].children_width = group_w;
                // The folders/files radio trio (`SortFoldersFirst`…).
                let trio: [(&dyn Action, bool); 3] = [
                    (&sf::SortFoldersFirst, sf::SortFoldersFirst.is_on(self)),
                    (&sf::SortFilesFirst, sf::SortFilesFirst.is_on(self)),
                    (&sf::SortFilesAndFoldersTogether, sf::SortFilesAndFoldersTogether.is_on(self)),
                ];
                for (action, on) in trio {
                    items.push(FlyoutItem::new(tr(action.label()).to_string(), true).checked(on));
                }
                self.state.flyout = Some(crate::ui::Flyout {
                    kind: crate::ui::FlyoutKind::SortMenu,
                    width: crate::ui::FLYOUT_WIDTH,
                    x: (anchor.right - crate::ui::FLYOUT_WIDTH).max(0.0),
                    y: anchor.bottom + 2.0,
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
                self.invalidate();
    }

    /// The "Layout" panel for the `CmdLayout` button (the `CmdLayout` branch).
    pub(crate) fn on_click_cmd_layout(&mut self, layout: &Layout) {
                // LayoutOptionsButton: the "Layout" panel — a real
                // Flyout (cards, size slider, toggles), not a menu.
                use crate::user_controls::layout_flyout::{LayoutPanel, LAYOUT_PANEL_WIDTH};
                let anchor = layout
                    .cmd_buttons
                    .iter()
                    .find(|(_, h)| *h == Hot::CmdLayout)
                    .map(|(r, _)| *r)
                    .unwrap_or_default();
                let settings = crate::services::settings::get();
                let mode = self.state.active().view_mode;
                let panel = LayoutPanel {
                    mode,
                    size: self.layout_size(mode),
                    show_hidden: settings.show_hidden_items,
                    show_extensions: settings.show_file_extensions,
                    hot: None,
                    dragging: false,
                };
                self.state.flyout = Some(crate::ui::Flyout {
                    kind: crate::ui::FlyoutKind::ViewMode,
                    width: LAYOUT_PANEL_WIDTH,
                    x: (anchor.right - LAYOUT_PANEL_WIDTH).max(0.0),
                    y: anchor.bottom + 2.0,
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
                    layout: Some(panel),
                    picker: None,
                    hot: None,
                    path: None,
                });
                self.invalidate();
    }
}
