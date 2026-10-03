//! Shared attributes, extensions, and common code.
//! Rust port of `Files.Shared`.

pub mod extensions;
pub mod helpers;
pub mod logger;
pub mod utils;

pub use logger::{FileLogger, LogLevel};
