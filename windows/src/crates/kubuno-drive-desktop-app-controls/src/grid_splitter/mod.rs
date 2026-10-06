//! `GridSplitter` — the PURE MATH of panel resizing.
//! Port of the core of `Files.App.Controls/GridSplitter/GridSplitter.Helper.cs`
//! (`SetColumnWidth`/`SetRowHeight`) + the Compact threshold from
//! `SidebarView.UpdateDisplayModeForPaneWidth`. No WinUI dependency: the
//! column/row routing, cursors, and visual states stay in the window layer
//! (`main_window`).
//!
//! This module is split up mirroring the (flat) C# folder:
//! - [`data`] — the four enums from `GridSplitter.Data.cs`.
//! - [`helper`] — the clamp core `resize` from `GridSplitter.Helper.cs`.
//! - [`events`] — the constants from `GridSplitter.Events.cs`.
//! - [`sidebar_resize`] — port-only helpers (MainPage/SidebarView).
//!
//! `mod.rs` is just glue: it re-exports the historical flat API
//! (`grid_splitter::resize`, `available_max`, `resolve_sidebar`, `PaneResize`)
//! so no caller breaks.

pub mod data;
pub mod events;
pub mod helper;
pub mod sidebar_resize;

pub use helper::resize;
pub use sidebar_resize::{available_max, resolve_sidebar, PaneResize};
