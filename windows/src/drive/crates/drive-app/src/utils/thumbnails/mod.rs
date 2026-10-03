//! Cache of real shell icons as Direct2D bitmaps — pair of the C#'s
//! `IconCacheService` (Services/Storage) + `FileThumbnailHelper`
//! (Utils/Storage/Helpers).

pub mod file_thumbnail_helper;

pub use file_thumbnail_helper::*;
