//! Port of `Files.Shared/Utils` — cross-cutting abstraction traits.

use std::future::Future;

/// An object that requires async initialization (port of `IAsyncInitialize`).
pub trait AsyncInitialize {
    fn init(&self) -> impl Future<Output = anyhow_result::Result<()>> + Send;
}

/// An object that can be loaded from and saved to a persistence store
/// (port of `IPersistable`).
pub trait Persistable {
    fn load(&self) -> impl Future<Output = anyhow_result::Result<()>> + Send;
    fn save(&self) -> impl Future<Output = anyhow_result::Result<()>> + Send;
}

/// Marker for an image that can be displayed in the UI (port of `IImage`).
pub trait Image {}

/// Exposes a wrapped inner member (port of `IWrapper<T>`).
pub trait Wrapper<T> {
    fn inner(&self) -> &T;
}

mod anyhow_result {
    /// Minimal error alias until a richer error model is needed.
    pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
}
