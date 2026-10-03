//! Port of `ViewModels/UserControls/ShelfViewModel.cs`: the view model for the
//! Shelf pane.
//!
//! The original holds an `ObservableCollection<ShelfItem> Items`, a
//! `ClearItems` command, and a set of `IFolderWatcher` per parent folder that
//! automatically removes an item when the underlying file disappears
//! (`Watcher_CollectionChanged`). The port replaces the watchers with a
//! [`prune_missing`] sweep called on render: same effect (the item disappears
//! once its path no longer exists) without an event stream. Persistence
//! remains a TODO in the original (`InitAsync`: "Load persisted shelf items").

use crate::data::items::ShelfItem;

#[derive(Default)]
pub struct ShelfViewModel {
    /// `ObservableCollection<ShelfItem> Items`.
    pub items: Vec<ShelfItem>,
}

impl ShelfViewModel {
    /// Adds a path to the Shelf if not already present (the original's
    /// drag-drop doesn't explicitly deduplicate, but dropping the same path
    /// twice makes no sense).
    pub fn add_path(&mut self, path: &std::path::Path) {
        let id = path.to_string_lossy();
        if self.items.iter().any(|it| it.path == id) {
            return;
        }
        self.items.push(ShelfItem::from_path(path));
    }

    /// `ShelfItem.Remove()`: removes the item at the given index.
    pub fn remove(&mut self, index: usize) {
        if index < self.items.len() {
            self.items.remove(index);
        }
    }

    /// `ClearItemsCommand`: clears the Shelf.
    pub fn clear(&mut self) {
        self.items.clear();
    }

    /// The counterpart to `Watcher_CollectionChanged`: removes items whose path
    /// no longer exists on disk. Returns `true` if the list changed.
    pub fn prune_missing(&mut self) -> bool {
        let before = self.items.len();
        self.items.retain(|it| std::path::Path::new(&it.path).exists());
        self.items.len() != before
    }

}
