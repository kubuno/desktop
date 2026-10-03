//! Port of `Files.App/Services/Settings/`: all settings persisted in a
//! single JSON store (`UserSettingsService` / `BaseObservableJsonSettings`),
//! one Rust field per C# property.

use std::path::PathBuf;
use std::sync::{OnceLock, RwLock};

use serde::{Deserialize, Serialize};

// Settings sub-services (mirror of `Services/Settings/*`). The state is a
// SINGLE JSON store (`AppSettings`/`UserSettingsService`); these modules only
// carry the helpers specific to each C# service, with value enums
// living on the `Data/Enums` side (see `deferred`).
pub mod appearance_settings_service;
pub mod dev_tools_settings_service;
pub mod general_settings_service;

pub use appearance_settings_service::{app_theme_font_override, parse_color};
pub use dev_tools_settings_service::ide_display;
pub use general_settings_service::user_id;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThemeSetting {
    System,
    Light,
    Dark,
}

impl ThemeSetting {
    pub fn label(self) -> &'static str {
        match self {
            ThemeSetting::System => "Utiliser le thème du système",
            ThemeSetting::Light => "Clair",
            ThemeSetting::Dark => "Sombre",
        }
    }
}

/// Port of `AppearanceSettingsService.AppThemeBackdropMaterial`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BackdropSetting {
    Mica,
    MicaAlt,
    Acrylic,
}

impl BackdropSetting {
    pub fn label(self) -> &'static str {
        match self {
            BackdropSetting::Mica => "Mica",
            BackdropSetting::MicaAlt => "Mica Alt",
            BackdropSetting::Acrylic => "Acrylique",
        }
    }

    /// DWM_SYSTEMBACKDROP_TYPE value.
    pub fn dwm_value(self) -> i32 {
        match self {
            BackdropSetting::Mica => 2,        // DWMSBT_MAINWINDOW
            BackdropSetting::MicaAlt => 4,     // DWMSBT_TABBEDWINDOW
            BackdropSetting::Acrylic => 3,     // DWMSBT_TRANSIENTWINDOW
        }
    }
}

/// Port of `Stretch` for `AppThemeBackgroundImageFit`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ImageFit {
    None,
    Fill,
    Uniform,
    #[default]
    UniformToFill,
}

impl ImageFit {
    pub const ALL: [ImageFit; 4] = [ImageFit::None, ImageFit::Fill, ImageFit::Uniform, ImageFit::UniformToFill];

    /// Localization key (AppearanceViewModel.ImageStretchTypes).
    pub fn tr_key(self) -> &'static str {
        match self {
            ImageFit::None => "None",
            ImageFit::Fill => "Fill",
            ImageFit::Uniform => "Uniform",
            ImageFit::UniformToFill => "UniformToFill",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ImageVerticalAlignment {
    Top,
    #[default]
    Center,
    Bottom,
}

impl ImageVerticalAlignment {
    pub const ALL: [Self; 3] = [Self::Top, Self::Center, Self::Bottom];

    pub fn tr_key(self) -> &'static str {
        match self {
            Self::Top => "Top",
            Self::Center => "Center",
            Self::Bottom => "Bottom",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ImageHorizontalAlignment {
    Left,
    #[default]
    Center,
    Right,
}

impl ImageHorizontalAlignment {
    pub const ALL: [Self; 3] = [Self::Left, Self::Center, Self::Right];

    pub fn tr_key(self) -> &'static str {
        match self {
            Self::Left => "Left",
            Self::Center => "Center",
            Self::Right => "Right",
        }
    }
}

/// Port of `StatusCenterVisibility`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum StatusCenterVisibility {
    #[default]
    Always,
    DuringOngoingFileOperations,
}

impl StatusCenterVisibility {
    pub const ALL: [Self; 2] = [Self::Always, Self::DuringOngoingFileOperations];

    pub fn tr_key(self) -> &'static str {
        match self {
            Self::Always => "Always",
            Self::DuringOngoingFileOperations => "DuringOngoingFileOperations",
        }
    }
}

/// Port of `DateTimeFormats` (GeneralSettingsService.DateTimeFormat).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum DateTimeFormat {
    #[default]
    Application,
    System,
    Universal,
}

impl DateTimeFormat {
    pub const ALL: [Self; 3] = [Self::Application, Self::System, Self::Universal];

    /// Formatter display names (Application/System/UniversalDateTimeFormatter.Name).
    pub fn tr_key(self) -> &'static str {
        match self {
            Self::Application => "Application",
            Self::System => "SystemTimeStyle",
            Self::Universal => "Universal",
        }
    }
}

/// Port of `ShellPaneArrangement` (GeneralSettingsService).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ShellPaneArrangement {
    #[default]
    Vertical,
    Horizontal,
}

impl ShellPaneArrangement {
    pub const ALL: [Self; 2] = [Self::Vertical, Self::Horizontal];

    pub fn tr_key(self) -> &'static str {
        match self {
            Self::Vertical => "Vertical",
            Self::Horizontal => "Horizontal",
        }
    }
}

/// Port of `FolderLayoutModes` (LayoutSettingsService.DefaultLayoutMode).
/// XAML combo order: Details, List, Cards, Columns, Grid, Adaptive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum FolderLayoutMode {
    Details,
    List,
    Cards,
    Columns,
    Grid,
    #[default]
    Adaptive,
}

impl FolderLayoutMode {
    pub const ALL: [Self; 6] = [
        Self::Details,
        Self::List,
        Self::Cards,
        Self::Columns,
        Self::Grid,
        Self::Adaptive,
    ];

    pub fn tr_key(self) -> &'static str {
        match self {
            Self::Details => "Details",
            Self::List => "List",
            Self::Cards => "Cards",
            Self::Columns => "Columns",
            Self::Grid => "Grid",
            Self::Adaptive => "Adaptive",
        }
    }
}

/// Port of `SortOption` (LayoutSettingsService.DefaultSortOption).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum DefaultSortOption {
    #[default]
    Name,
    DateModified,
    DateCreated,
    Size,
    Type,
    FileTag,
}

impl DefaultSortOption {
    pub const ALL: [Self; 6] = [
        Self::Name,
        Self::DateModified,
        Self::DateCreated,
        Self::Size,
        Self::Type,
        Self::FileTag,
    ];

    /// LayoutPage.xaml SortBy combo item keys, same order.
    pub fn tr_key(self) -> &'static str {
        match self {
            Self::Name => "Name",
            Self::DateModified => "DateModifiedLowerCase",
            Self::DateCreated => "DateCreated",
            Self::Size => "Size",
            Self::Type => "Type",
            Self::FileTag => "Tag",
        }
    }
}

/// Port of `GroupOption` (LayoutSettingsService.DefaultGroupOption).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum DefaultGroupOption {
    #[default]
    None,
    Name,
    DateModified,
    DateCreated,
    Size,
    Type,
    FileTag,
}

impl DefaultGroupOption {
    pub const ALL: [Self; 7] = [
        Self::None,
        Self::Name,
        Self::DateModified,
        Self::DateCreated,
        Self::Size,
        Self::Type,
        Self::FileTag,
    ];

    pub fn tr_key(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Name => "Name",
            Self::DateModified => "DateModifiedLowerCase",
            Self::DateCreated => "DateCreated",
            Self::Size => "Size",
            Self::Type => "Type",
            Self::FileTag => "Tag",
        }
    }

    pub fn is_group_by_date(self) -> bool {
        matches!(self, Self::DateModified | Self::DateCreated)
    }
}

/// Port of `GroupByDateUnit` (LayoutSettingsService.DefaultGroupByDateUnit).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum GroupByDateUnit {
    #[default]
    Year,
    Month,
    Day,
}

impl GroupByDateUnit {
    pub const ALL: [Self; 3] = [Self::Year, Self::Month, Self::Day];

    pub fn tr_key(self) -> &'static str {
        match self {
            Self::Year => "Year",
            Self::Month => "Month",
            Self::Day => "Day",
        }
    }
}

/// Port of `SingleClickOpenMode` (FoldersSettingsService).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum SingleClickOpenMode {
    Never,
    #[default]
    OnlyForTouch,
    OnlyForMouse,
    Always,
}

impl SingleClickOpenMode {
    pub const ALL: [Self; 4] = [Self::Never, Self::OnlyForTouch, Self::OnlyForMouse, Self::Always];

    pub fn tr_key(self) -> &'static str {
        match self {
            Self::Never => "Never",
            Self::OnlyForTouch => "OnlyForTouch",
            Self::OnlyForMouse => "OnlyForMouse",
            Self::Always => "Always",
        }
    }
}

/// Port of `DeleteConfirmationPolicies` (FoldersSettingsService).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum DeleteConfirmationPolicy {
    #[default]
    Always,
    PermanentDeletionOnly,
    Never,
}

impl DeleteConfirmationPolicy {
    pub const ALL: [Self; 3] = [Self::Always, Self::PermanentDeletionOnly, Self::Never];

    pub fn tr_key(self) -> &'static str {
        match self {
            Self::Always => "Always",
            Self::PermanentDeletionOnly => "PermanentDeletionOnly",
            Self::Never => "Never",
        }
    }
}

/// Port of `SizeUnitTypes` (FoldersSettingsService.SizeUnitFormat).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum SizeUnitFormat {
    #[default]
    BinaryUnits,
    DecimalUnits,
}

impl SizeUnitFormat {
    pub const ALL: [Self; 2] = [Self::BinaryUnits, Self::DecimalUnits];

    /// FoldersViewModel.SizeUnitsOptions labels.
    pub fn tr_key(self) -> &'static str {
        match self {
            Self::BinaryUnits => "Binary",
            Self::DecimalUnits => "Decimal",
        }
    }
}

/// Port of `OpenInIDEOption` (DevToolsSettingsService).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum OpenInIDEOption {
    #[default]
    GitRepos,
    AllLocations,
}

impl OpenInIDEOption {
    pub const ALL: [Self; 2] = [Self::GitRepos, Self::AllLocations];

    /// DevToolsViewModel.OpenInIDEOptions labels.
    pub fn tr_key(self) -> &'static str {
        match self {
            Self::GitRepos => "GitRepos",
            Self::AllLocations => "AllLocations",
        }
    }
}

/// Port of `TagViewModel` (name, #RRGGBB color, uid) persisted like
/// FileTagsSettingsService.FileTagList.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileTag {
    pub name: String,
    pub color: String,
    pub uid: String,
}

/// `FileTagsSettingsService.DefaultFileTags`, verbatim.
pub fn default_file_tags() -> Vec<FileTag> {
    [
        ("Home", "#0072BD", "f7e0e137-2eb5-4fa4-a50d-ddd65df17c34"),
        ("Work", "#D95319", "c84a8131-c4de-47d9-9440-26e859d14b3d"),
        ("Photos", "#EDB120", "d4b8d4bd-ceaf-4e58-ac61-a185fcf96c5d"),
        ("Important", "#77AC30", "79376daf-c44a-4fe4-aa3b-8b30baea453e"),
    ]
    .into_iter()
    .map(|(name, color, uid)| FileTag {
        name: name.into(),
        color: color.into(),
        uid: uid.into(),
    })
    .collect()
}

/// The system default font family (Constants.Appearance.StandardFont):
/// "Segoe UI Variable" on Windows 11.
pub const STANDARD_FONT: &str = "Segoe UI Variable";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppSettings {
    pub theme: ThemeSetting,
    pub backdrop: BackdropSetting,
    /// Port of `FoldersSettingsService.ShowHiddenItems`.
    pub show_hidden_items: bool,
    /// Port of `FoldersSettingsService.ShowFileExtensions`.
    pub show_file_extensions: bool,
    /// `LayoutSettingsService`: one size per layout, like the original
    /// (each has its own scale — see `ViewMode::size_max`).
    pub details_view_size: u8,
    pub list_view_size: u8,
    pub cards_view_size: u8,
    pub columns_view_size: u8,
    pub grid_view_size: u8,
    /// The per-folder preferences "database" (`LayoutPreferencesDatabase`):
    /// normalized path → preferences. Ignored when
    /// `sync_folder_preferences_across_directories` is true.
    #[serde(default)]
    pub folder_prefs: std::collections::HashMap<String, crate::helpers::layout_preferences::LayoutPreferences>,
    /// Port of `InfoPaneSettingsService.IsEnabled`.
    pub show_info_pane: bool,
    /// Port of `InfoPaneSettingsService.SelectedTab`.
    pub info_pane_tab: InfoPaneTab,
    /// `InfoPaneSettingsService.VerticalSizePx`: the pane's width when it's
    /// on the right (`Math.Max(100, …)`, default 250).
    pub info_pane_width: f32,
    /// `AppearanceSettingsService.SidebarWidth`: default 255, clamped between
    /// `Constants.UI.MinimumSidebarWidth` (180) and 500.
    pub sidebar_width: f32,
    /// `IsSidebarOpen` inverted: true when the sidebar is in COMPACT mode
    /// (the 56 DIP icon rail of `SidebarDisplayMode.Compact`).
    #[serde(default)]
    pub sidebar_compact: bool,
    /// `AppearanceSettingsService.AppThemeBackgroundColor` (#AARRGGBB).
    pub app_theme_background_color: String,
    /// `AppearanceSettingsService.AppThemeBackgroundImageSource`.
    pub app_theme_background_image_source: String,
    /// `AppearanceSettingsService.AppThemeBackgroundImageOpacity` (0.1–1.0).
    pub app_theme_background_image_opacity: f32,
    pub app_theme_background_image_fit: ImageFit,
    pub app_theme_background_image_vertical_alignment: ImageVerticalAlignment,
    pub app_theme_background_image_horizontal_alignment: ImageHorizontalAlignment,
    /// `AppearanceSettingsService.AppThemeFontFamily`.
    pub app_theme_font_family: String,
    /// `AppearanceSettingsService.ShowToolbar`.
    pub show_toolbar: bool,
    /// `AppearanceSettingsService.ShowStatusBar`.
    pub show_status_bar: bool,
    /// `AppearanceSettingsService.ShowTabActions`.
    pub show_tab_actions: bool,
    pub status_center_visibility: StatusCenterVisibility,
    /// `GeneralSettingsService.ShowShelfPane`: the Shelf pane is visible.
    /// (Reserved for the Dev build in the original; ported here as a
    /// full-fledged feature.)
    #[serde(default)]
    pub show_shelf_pane: bool,

    // === GeneralSettingsService ===
    /// `GeneralSettingsService.DateTimeFormat`.
    pub date_time_format: DateTimeFormat,
    /// Startup: `OpenSpecificPageOnStartup` / `ContinueLastSessionOnStartUp`
    /// / `OpenNewTabOnStartup` (mutually exclusive combo in GeneralPage).
    pub open_specific_page_on_startup: bool,
    pub continue_last_session_on_startup: bool,
    pub open_new_tab_on_startup: bool,
    /// `GeneralSettingsService.TabsOnStartupList`.
    pub tabs_on_startup_list: Vec<String>,
    /// `GeneralSettingsService.OpenTabInExistingInstance`.
    pub open_tab_in_existing_instance: bool,
    /// `GeneralSettingsService.AlwaysSwitchToNewlyOpenedTab`.
    pub always_switch_to_newly_opened_tab: bool,
    /// `GeneralSettingsService.ReverseTabScrollDirection`.
    pub reverse_tab_scroll_direction: bool,
    /// GeneralPage "Widgets" expander.
    pub show_quick_access_widget: bool,
    pub show_drives_widget: bool,
    pub show_network_locations_widget: bool,
    pub show_file_tags_widget: bool,
    pub show_recent_files_widget: bool,
    /// `GeneralSettingsService.AlwaysOpenDualPaneInNewTab`.
    pub always_open_dual_pane_in_new_tab: bool,
    /// `GeneralSettingsService.ShellPaneArrangementOption`.
    pub shell_pane_arrangement: ShellPaneArrangement,
    /// GeneralPage "Context menu options" expander.
    pub show_open_in_new_tab: bool,
    pub show_open_in_new_window: bool,
    pub show_open_in_new_pane: bool,
    pub show_copy_path: bool,
    pub show_create_folder_with_selection: bool,
    pub show_create_alternate_data_stream: bool,
    pub show_create_shortcut: bool,
    pub show_pin_to_sidebar: bool,
    pub show_compression_options: bool,
    pub show_send_to_menu: bool,
    pub show_open_terminal: bool,
    pub show_edit_tags_menu: bool,
    pub show_pin_to_start: bool,
    /// `GeneralSettingsService.MoveShellExtensionsToSubMenu`.
    pub move_shell_extensions_to_sub_menu: bool,
    /// `GeneralSettingsService.EnableSmoothScrolling`.
    pub enable_smooth_scrolling: bool,
    /// The sidebar sections (`ShowPinnedSection`…) and their expanded
    /// state (`IsPinnedSectionExpanded`…), persisted like the original.
    #[serde(default = "yes")]
    pub show_pinned_section: bool,
    #[serde(default = "yes")]
    pub show_drives_section: bool,
    /// `ShowLibrarySection` — default FALSE in the original.
    #[serde(default)]
    pub show_library_section: bool,
    #[serde(default = "yes")]
    pub is_library_section_expanded: bool,
    #[serde(default = "yes")]
    pub show_cloud_drives_section: bool,
    #[serde(default = "yes")]
    pub is_cloud_drive_section_expanded: bool,
    #[serde(default = "yes")]
    pub show_network_section: bool,
    #[serde(default = "yes")]
    pub show_file_tags_section: bool,
    #[serde(default = "yes")]
    pub is_pinned_section_expanded: bool,
    #[serde(default = "yes")]
    pub is_drive_section_expanded: bool,
    #[serde(default)]
    pub is_network_section_expanded: bool,
    #[serde(default)]
    pub is_file_tags_section_expanded: bool,

    // === LayoutSettingsService ===
    pub sync_folder_preferences_across_directories: bool,
    pub default_layout_mode: FolderLayoutMode,
    pub default_sort_option: DefaultSortOption,
    /// true = SortDirection.Descending.
    pub default_sort_descending: bool,
    /// `DefaultSortDirectoriesAlongsideFiles` / `DefaultSortFilesFirst`.
    pub default_sort_directories_alongside_files: bool,
    pub default_sort_files_first: bool,
    pub default_group_option: DefaultGroupOption,
    pub default_group_descending: bool,
    pub default_group_by_date_unit: GroupByDateUnit,
    pub auto_size_columns_in_details_layout: bool,
    pub show_file_tag_column: bool,
    pub show_size_column: bool,
    pub show_type_column: bool,
    pub show_date_column: bool,
    pub show_date_created_column: bool,

    // === FoldersSettingsService (rest) ===
    pub show_dot_files: bool,
    pub show_protected_system_files: bool,
    pub are_alternate_streams_visible: bool,
    pub show_thumbnails: bool,
    pub show_checkboxes_when_selecting_items: bool,
    pub open_files_with_single_click: SingleClickOpenMode,
    pub open_folders_with_single_click: SingleClickOpenMode,
    pub open_folders_in_columns_view_with_single_click: SingleClickOpenMode,
    pub open_folders_in_new_tab: bool,
    pub delete_confirmation_policy: DeleteConfirmationPolicy,
    pub show_file_extension_warning: bool,
    pub select_files_on_hover: bool,
    pub double_click_to_go_up: bool,
    pub scroll_to_previous_folder_when_navigating_up: bool,
    pub size_unit_format: SizeUnitFormat,
    pub calculate_folder_sizes: bool,

    // === FileTagsSettingsService ===
    #[serde(default = "default_file_tags")]
    pub file_tags: Vec<FileTag>,

    // === DevToolsSettingsService ===
    pub open_in_ide_option: OpenInIDEOption,
    pub ide_path: String,
    pub ide_name: String,

    // === GeneralSettingsService (Advanced page) ===
    /// AdvancedPage "Launch at Windows startup" (StartupTask).
    pub open_on_windows_startup: bool,
    /// `GeneralSettingsService.LeaveAppRunning` (RELEASE default: true).
    pub leave_app_running: bool,
    /// `GeneralSettingsService.ShowSystemTrayIcon`.
    pub show_system_tray_icon: bool,
    /// AdvancedPage "Replace File Explorer".
    pub is_set_as_default_file_manager: bool,
    /// `GeneralSettingsService.ShowFlattenOptions`.
    pub show_flatten_options: bool,
    /// `GeneralSettingsService.UserId` (default: a fresh GUID).
    pub user_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum InfoPaneTab {
    #[default]
    Details,
    Preview,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            theme: ThemeSetting::System,
            // Original default: BackdropMaterialType.MicaAlt.
            backdrop: BackdropSetting::MicaAlt,
            show_hidden_items: false,
            show_file_extensions: true,
            details_view_size: 2,
            list_view_size: 2,
            cards_view_size: 2,
            columns_view_size: 2,
            grid_view_size: 2,
            folder_prefs: std::collections::HashMap::new(),
            show_info_pane: true,
            info_pane_tab: InfoPaneTab::Details,
            info_pane_width: 250.0,
            sidebar_width: 255.0,
            sidebar_compact: false,
            app_theme_background_color: "#00000000".into(),
            app_theme_background_image_source: String::new(),
            app_theme_background_image_opacity: 1.0,
            app_theme_background_image_fit: ImageFit::UniformToFill,
            app_theme_background_image_vertical_alignment: ImageVerticalAlignment::Center,
            app_theme_background_image_horizontal_alignment: ImageHorizontalAlignment::Center,
            app_theme_font_family: STANDARD_FONT.into(),
            show_toolbar: true,
            show_status_bar: true,
            show_tab_actions: true,
            status_center_visibility: StatusCenterVisibility::Always,
            show_shelf_pane: false,

            // GeneralSettingsService defaults (Get(...) values, verbatim).
            date_time_format: DateTimeFormat::Application,
            open_specific_page_on_startup: false,
            continue_last_session_on_startup: true,
            open_new_tab_on_startup: false,
            tabs_on_startup_list: Vec::new(),
            open_tab_in_existing_instance: true,
            always_switch_to_newly_opened_tab: false,
            reverse_tab_scroll_direction: false,
            show_quick_access_widget: true,
            show_drives_widget: true,
            show_network_locations_widget: true,
            show_file_tags_widget: false,
            show_recent_files_widget: true,
            always_open_dual_pane_in_new_tab: false,
            shell_pane_arrangement: ShellPaneArrangement::Vertical,
            show_open_in_new_tab: true,
            show_open_in_new_window: true,
            show_open_in_new_pane: true,
            show_copy_path: true,
            show_create_folder_with_selection: true,
            show_create_alternate_data_stream: false,
            show_create_shortcut: true,
            show_pin_to_sidebar: true,
            show_compression_options: true,
            show_send_to_menu: true,
            show_open_terminal: true,
            show_edit_tags_menu: true,
            show_pin_to_start: true,
            move_shell_extensions_to_sub_menu: true,
            enable_smooth_scrolling: true,
            show_pinned_section: true,
            show_drives_section: true,
            show_library_section: false,
            is_library_section_expanded: true,
            show_cloud_drives_section: true,
            is_cloud_drive_section_expanded: true,
            show_network_section: true,
            show_file_tags_section: true,
            is_pinned_section_expanded: true,
            is_drive_section_expanded: true,
            is_network_section_expanded: false,
            is_file_tags_section_expanded: false,

            // LayoutSettingsService defaults.
            sync_folder_preferences_across_directories: false,
            default_layout_mode: FolderLayoutMode::Adaptive,
            default_sort_option: DefaultSortOption::Name,
            default_sort_descending: false,
            default_sort_directories_alongside_files: false,
            default_sort_files_first: false,
            default_group_option: DefaultGroupOption::None,
            default_group_descending: false,
            default_group_by_date_unit: GroupByDateUnit::Year,
            auto_size_columns_in_details_layout: false,
            show_file_tag_column: true,
            show_size_column: true,
            show_type_column: true,
            show_date_column: true,
            show_date_created_column: false,

            // FoldersSettingsService defaults.
            show_dot_files: true,
            show_protected_system_files: false,
            are_alternate_streams_visible: false,
            show_thumbnails: true,
            show_checkboxes_when_selecting_items: true,
            open_files_with_single_click: SingleClickOpenMode::OnlyForTouch,
            open_folders_with_single_click: SingleClickOpenMode::OnlyForTouch,
            open_folders_in_columns_view_with_single_click: SingleClickOpenMode::Always,
            open_folders_in_new_tab: false,
            delete_confirmation_policy: DeleteConfirmationPolicy::Always,
            show_file_extension_warning: true,
            select_files_on_hover: false,
            double_click_to_go_up: true,
            scroll_to_previous_folder_when_navigating_up: true,
            size_unit_format: SizeUnitFormat::BinaryUnits,
            calculate_folder_sizes: false,

            file_tags: default_file_tags(),

            // DevToolsSettingsService defaults (VS Code detection is done
            // lazily at first read, see `ide_display()`).
            open_in_ide_option: OpenInIDEOption::GitRepos,
            ide_path: String::new(),
            ide_name: String::new(),

            // GeneralSettingsService (Advanced) defaults.
            open_on_windows_startup: false,
            leave_app_running: true,
            show_system_tray_icon: true,
            is_set_as_default_file_manager: false,
            show_flatten_options: false,
            user_id: String::new(),
        }
    }
}

// The settings are stored through `kubuno_desktop::storage`'s engine (vskubuno docs/STORAGE-COMPONENTS.md, lot ST-2): one
// setting per field of `AppSettings` (`settings::serde_bridge`), in `%LOCALAPPDATA%\Kubuno\kubuno-drive\
// settings.local.settings.json` (sandbox-aware). The older `%LOCALAPPDATA%\KubunoDrive\settings.json` is imported
// once and kept as `settings.json.migrated`.

/// The id the settings are stored under.
const APP_ID: &str = "kubuno-drive";

static STORE: OnceLock<RwLock<AppSettings>> = OnceLock::new();
static ENGINE: OnceLock<kubuno_desktop_app_storage::Settings> = OnceLock::new();

/// Full path of the persisted settings file (Advanced page: edit/open).
pub fn settings_file_path() -> PathBuf {
    PathBuf::from(engine().location(kubuno_desktop_app_storage::Layer::UserLocal))
}

/// The older settings file (`%LOCALAPPDATA%\KubunoDrive\settings.json`; `<sandbox>\legacy\KubunoDrive` in a
/// sandboxed profile).
fn legacy_settings_path() -> PathBuf {
    if let Some(sandbox) = kubuno_desktop_app_storage::paths::sandbox_dir() {
        return sandbox.join("legacy").join("KubunoDrive").join("settings.json");
    }
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    base.join("KubunoDrive").join("settings.json")
}

/// Opens the settings on `roots` (the platform's, or a test folder's), importing `legacy` once.
fn open_engine(roots: kubuno_desktop_app_storage::backend::FileRoots, legacy: &std::path::Path) -> kubuno_desktop_app_storage::Settings {
    use kubuno_desktop_app_storage::backend::FileBackend;
    use kubuno_desktop_app_storage::{AppId, Settings, SettingsOptions, SettingsSchema};
    let app = AppId::new(APP_ID).unwrap_or_else(|_| kubuno_desktop_app_storage::default_app_id());
    let schema = SettingsSchema::open_local("settings");
    let engine = Settings::with_backend(&app, schema.clone(), Box::new(FileBackend::new(&app, "settings", roots)), SettingsOptions::default()).unwrap_or_else(|e| {
        tracing::warn!("the settings cannot be opened, they are kept in memory: {e}");
        Settings::shared_or_memory(&app, &schema, kubuno_desktop_app_storage::backend::BackendKind::Memory)
    });
    let imported = kubuno_desktop_app_storage::settings::migrate::import_legacy_json(&engine, legacy, |object| {
        match serde_json::from_value::<AppSettings>(serde_json::Value::Object(object.clone())) {
            Ok(old) => kubuno_desktop_app_storage::settings::serde_bridge::to_values(&old).unwrap_or_default(),
            Err(e) => {
                tracing::warn!("the older settings.json does not fit the settings type: {e}");
                Vec::new()
            }
        }
    });
    if let Err(e) = imported {
        tracing::warn!("the older settings.json was not imported: {e}");
    }
    engine
}

fn engine() -> &'static kubuno_desktop_app_storage::Settings {
    ENGINE.get_or_init(|| match kubuno_desktop_app_storage::backend::FileRoots::platform() {
        Ok(roots) => open_engine(roots, &legacy_settings_path()),
        Err(e) => {
            tracing::warn!("the settings folder is unknown, settings are kept in memory: {e}");
            open_engine(kubuno_desktop_app_storage::backend::FileRoots::under(&std::env::temp_dir().join("kubuno-drive-unused")), std::path::Path::new(""))
        }
    })
}

fn store() -> &'static RwLock<AppSettings> {
    STORE.get_or_init(|| RwLock::new(kubuno_desktop_app_storage::settings::serde_bridge::load(engine())))
}

pub fn get() -> AppSettings {
    store().read().unwrap_or_else(std::sync::PoisonError::into_inner).clone()
}

/// Mutates the settings and persists them (the fields that changed).
pub fn update(mutate: impl FnOnce(&mut AppSettings)) {
    let snapshot = {
        let mut guard = store().write().unwrap_or_else(std::sync::PoisonError::into_inner);
        mutate(&mut guard);
        guard.clone()
    };
    if let Err(e) = kubuno_desktop_app_storage::settings::serde_bridge::save(engine(), &snapshot) {
        tracing::warn!("failed to save settings: {e}");
    }
}

fn yes() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_and_roundtrip() {
        let s = AppSettings::default();
        assert_eq!(s.theme, ThemeSetting::System);
        assert!(!s.show_hidden_items);
        assert!(s.show_file_extensions);

        let json = serde_json::to_string(&s).unwrap();
        let back: AppSettings = serde_json::from_str(&json).unwrap();
        assert_eq!(back.theme, s.theme);
    }
}

#[cfg(test)]
mod storage_tests {
    use super::*;

    /// The older `KubunoDrive\settings.json` is imported once into `kubuno_desktop::storage`, field by field, and kept as a
    /// backup; the settings then round-trip through the new store.
    #[test]
    fn the_older_settings_file_is_imported_once() {
        let dir = std::env::temp_dir().join(format!("kubuno-drive-settings-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("legacy")).unwrap();
        let legacy = dir.join("legacy").join("settings.json");
        let old = AppSettings { theme: ThemeSetting::Dark, show_hidden_items: true, show_file_extensions: false, ..Default::default() };
        std::fs::write(&legacy, serde_json::to_string_pretty(&old).unwrap()).unwrap();

        let roots = kubuno_desktop_app_storage::backend::FileRoots::under(&dir);
        let engine = open_engine(roots.clone(), &legacy);
        let read: AppSettings = kubuno_desktop_app_storage::settings::serde_bridge::load(&engine);
        assert_eq!(read.theme, ThemeSetting::Dark);
        assert!(read.show_hidden_items && !read.show_file_extensions);
        assert!(!legacy.exists() && dir.join("legacy").join("settings.json.migrated").is_file());
        assert!(dir.join("local").join(APP_ID).join("settings.local.settings.json").is_file());

        // A later start reads the new store (the backup is not imported again).
        let mut changed = read.clone();
        changed.theme = ThemeSetting::Light;
        kubuno_desktop_app_storage::settings::serde_bridge::save(&engine, &changed).unwrap();
        let again: AppSettings = kubuno_desktop_app_storage::settings::serde_bridge::load(&open_engine(roots, &legacy));
        assert_eq!(again.theme, ThemeSetting::Light);
        assert!(again.show_hidden_items);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
