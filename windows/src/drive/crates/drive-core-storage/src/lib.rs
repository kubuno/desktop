//! Core storage abstractions.
//! Rust port of `Files.Core.Storage` together with the underlying
//! `OwlCore.Storage` model it builds on (IStorable/IFile/IFolder hierarchy).

pub mod error;
pub mod service;
pub mod storables;
pub mod watchers;

pub use error::{StorageError, StorageResult};
pub use service::StorageService;
pub use storables::*;
