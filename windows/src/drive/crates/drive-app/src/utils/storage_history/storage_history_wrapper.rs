//! StorageHistoryWrapper (mirrors StorageHistoryWrapper.cs)
//!
//! (Partial) port of `Files.App/Utils/Storage/History/StorageHistoryWrapper.cs`
//! (`IStorageHistoryWrapper`): the undo/redo stack for file operations.

use super::storage_history::FileOp;

/// The history stack (`IStorageHistoryWrapper`).
#[derive(Default)]
pub struct StorageHistory {
    undo: Vec<FileOp>,
    redo: Vec<FileOp>,
}

impl StorageHistory {
    /// Records a completed operation: it becomes undoable, and the redo
    /// stack is cleared (a new action breaks "redo").
    pub fn record(&mut self, op: FileOp) {
        self.undo.push(op);
        self.redo.clear();
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn pop_undo(&mut self) -> Option<FileOp> {
        self.undo.pop()
    }

    pub fn pop_redo(&mut self) -> Option<FileOp> {
        self.redo.pop()
    }

    pub fn push_undo(&mut self, op: FileOp) {
        self.undo.push(op);
    }

    pub fn push_redo(&mut self, op: FileOp) {
        self.redo.push(op);
    }
}
