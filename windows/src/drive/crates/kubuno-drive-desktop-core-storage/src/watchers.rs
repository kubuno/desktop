//! Port of `Files.Core.Storage.Contracts` watchers and
//! `Files.Core.Storage.EventArguments`.
//!
//! .NET `event EventHandler<T>` members become broadcast channels: callers
//! call `subscribe()` and receive [`TrashEvent`]/[`DeviceEvent`] values.

use tokio::sync::broadcast;

/// Port of `DeviceEventArgs`.
#[derive(Debug, Clone)]
pub struct DeviceEvent {
    pub kind: DeviceEventKind,
    pub device_name: String,
    pub device_id: String,
}

/// Discriminates the five C# device events (ItemAdded/Deleted/Changed/Inserted/Ejected).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceEventKind {
    Added,
    Deleted,
    Changed,
    Inserted,
    Ejected,
}

/// Port of the `System.IO.FileSystemEventArgs` payload used by `ITrashWatcher`.
#[derive(Debug, Clone)]
pub struct TrashEvent {
    pub kind: TrashEventKind,
    pub full_path: String,
    /// Previous path for renames.
    pub old_full_path: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrashEventKind {
    Added,
    Deleted,
    Changed,
    Renamed,
    RefreshRequested,
}

/// Base watcher contract (port of `IWatcher : IDisposable`).
/// Dropping a watcher must stop it (RAII replaces `Dispose`).
pub trait Watcher: Send {
    fn start(&mut self);
    fn stop(&mut self);
}

/// Recycle-bin watcher (port of `ITrashWatcher`).
pub trait TrashWatcher: Watcher {
    fn subscribe(&self) -> broadcast::Receiver<TrashEvent>;
}

/// Device watcher (port of the internal `IDeviceWatcher`).
pub trait DeviceWatcher: Send {
    fn can_be_started(&self) -> bool;
    fn start(&mut self);
    fn subscribe(&self) -> broadcast::Receiver<DeviceEvent>;
}
