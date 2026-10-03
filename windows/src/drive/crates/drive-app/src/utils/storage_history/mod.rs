//! Port of `Files.App/Utils/Storage/History/` — the undo/redo stack for
//! file operations.

pub mod storage_history;
pub mod storage_history_wrapper;

pub use storage_history::*;
pub use storage_history_wrapper::*;
