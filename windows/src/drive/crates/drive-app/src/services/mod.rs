//! Port of `Files.App/Services/` — the application services.

pub mod app;
pub mod date_time_formatter;
pub mod folder_search;
pub mod preview_popup_providers;
pub mod settings;
pub mod size_provider;
pub mod storage;
pub mod windows;
pub mod windows_quick_access_service;
pub mod windows_wallpaper_service;

// Compatibility aliases: preserve the historical import paths
// (`crate::services::preview_popup`, `::jump_list`, `::app_update`) after
// moving these modules to the mirror subfolders of `Services/`.
// `app_update` has no caller yet (update channel not wired up).
#[allow(unused_imports)]
pub use app::app_update_sideload_service as app_update;
pub use preview_popup_providers as preview_popup;
pub use windows::windows_jump_list_service as jump_list;
