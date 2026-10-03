//! `Toolbar` item (mirrors `Files.App.Controls/Toolbar/ToolbarItem/`).
//!
//! Glue: the C# `ToolbarItem` control is WinUI machinery (empty); only the
//! type enum is portable.

pub mod toolbar_item_types;

pub use toolbar_item_types::ToolbarItemType;
