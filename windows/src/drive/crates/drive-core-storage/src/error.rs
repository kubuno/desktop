//! Storage error model. The C# code signals failures via exceptions
//! (`FileNotFoundException`, `UnauthorizedAccessException`, …); this enum is
//! their Rust equivalent.

use thiserror::Error;

pub type StorageResult<T> = Result<T, StorageError>;

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("item not found: {0}")]
    NotFound(String),
    #[error("access denied: {0}")]
    AccessDenied(String),
    #[error("an item named '{0}' already exists")]
    AlreadyExists(String),
    #[error("operation is not supported: {0}")]
    NotSupported(String),
    #[error("operation was canceled")]
    Canceled,
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Other(String),
}
