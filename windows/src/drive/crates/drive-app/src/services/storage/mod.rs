//! Port of `Files.App/Services/Storage/` — storage access services
//! (icon cache, Recycle Bin).

pub mod icon_cache_service;
pub mod storage_trash_bin_service;

pub use icon_cache_service::*;
