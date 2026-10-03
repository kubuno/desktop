//! ShelfItem (mirrors ShelfItem.cs)
//!
//! The original wraps an `IStorableChild` (`Inner`) and exposes `Icon`/`Name`/
//! `Path`. Here the icon is loaded on the fly from the `IconCache` by path
//! (like the other items), so the model only keeps `name`/`path`/`is_dir`.

/// Port of `Data/Items/ShelfItem.cs`: an item dropped on the Shelf.
#[derive(Debug, Clone)]
pub struct ShelfItem {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
}

impl ShelfItem {
    /// Builds a `ShelfItem` from a path (C# `ctor`: `Name = storable.Name`,
    /// `Path = storable.Id`).
    pub fn from_path(path: &std::path::Path) -> Self {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.to_string_lossy().into_owned());
        Self {
            name,
            path: path.to_string_lossy().into_owned(),
            is_dir: path.is_dir(),
        }
    }
}
