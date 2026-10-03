//! Port of `Files.App/Services/Windows/`.
//!
//! (Architecture note: `windows_quick_access_service` and
//! `windows_wallpaper_service` currently remain at the `services/` root;
//! they should eventually join this subfolder — see `deferred`.)

pub mod windows_jump_list_service;
