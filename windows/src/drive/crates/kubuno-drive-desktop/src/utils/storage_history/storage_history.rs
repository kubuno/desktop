//! StorageHistory (mirrors StorageHistory.cs)
//!
//! (Partial) port of `Files.App/Utils/Storage/History/StorageHistory.cs`
//! (`IStorageHistory`): the RECORDING of a reversible file operation.
//!
//! Each operation records what it takes to REVERT it; the stack (see
//! `StorageHistoryWrapper`) pops the undo, applies the inverse, and
//! pushes the operation onto the redo stack. This first increment covers
//! renaming and creation (folder/file), which are deterministic;
//! move/copy/delete will follow once the storage layer returns the
//! result of shell operations (created paths / recycle bin).

use std::path::PathBuf;

/// A reversible file operation (`IStorageHistory`).
#[derive(Clone, Debug)]
pub enum FileOp {
    /// Rename: `before` → `after`. Inverse: `after` → `before`'s name.
    Rename { before: PathBuf, after: PathBuf },
    /// Creation of an item. Inverse: deletion (recycle bin).
    Create { path: PathBuf, is_dir: bool },
}
