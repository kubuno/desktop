//! Port of `Files.App/Views/Settings/` (the Settings sub-pages).

pub mod about_page;
pub mod actions_page;
pub mod advanced_page;
pub mod appearance_page;
pub mod controls;
pub mod dev_tools_page;
pub mod folders_page;
pub mod general_page;
pub mod layout_page;
pub mod tags_page;

/// Settings nav indices (SETTINGS_SECTIONS order).
pub const SECTION_GENERAL: usize = 0;
pub const SECTION_LAYOUT: usize = 2;
pub const SECTION_FOLDERS: usize = 3;
pub const SECTION_ACTIONS: usize = 4;
pub const SECTION_TAGS: usize = 5;
pub const SECTION_DEV_TOOLS: usize = 6;
pub const SECTION_ADVANCED: usize = 7;
pub const SECTION_ABOUT: usize = 8;

/// Rows of a generic settings section (every page except Appearance).
pub fn page_rows(
    section: usize,
    settings: &crate::services::settings::AppSettings,
    expanded: &[bool; 4],
) -> Vec<controls::SettingsRow> {
    match section {
        SECTION_GENERAL => general_page::rows(settings, expanded),
        SECTION_LAYOUT => layout_page::rows(settings, expanded),
        SECTION_FOLDERS => folders_page::rows(settings, expanded),
        SECTION_ACTIONS => actions_page::rows(),
        SECTION_TAGS => tags_page::rows(settings, expanded),
        SECTION_DEV_TOOLS => dev_tools_page::rows(settings, expanded),
        SECTION_ADVANCED => advanced_page::rows(settings, expanded),
        SECTION_ABOUT => about_page::rows(expanded),
        _ => Vec::new(),
    }
}

/// Default expander states per section (IsExpanded in the XAML).
pub fn default_expanded() -> [[bool; 4]; 9] {
    let mut expanded = [[false; 4]; 9];
    // FoldersPage: CalculateFolderSizes IsExpanded="True".
    expanded[SECTION_FOLDERS][folders_page::EXP_FOLDER_SIZES] = true;
    // TagsPage: FileTags IsExpanded="True".
    expanded[SECTION_TAGS][tags_page::EXP_TAGS] = true;
    expanded
}
