//! One module per family, each owning its own page file — so families can be
//! built in parallel without ever editing the same file.

pub mod buttons;
pub mod containers;
pub mod display;
pub mod lists;
pub mod navigation;
pub mod range;
pub mod text;
pub mod views;

pub mod color;
pub mod datetime;
pub mod dialogs;
pub mod editors;
pub mod feedback;
pub mod fields;
pub mod tables;

// Third wave: composed screens and the rich text editor.
pub mod composition;
pub mod richtext;
pub mod ribbon;
pub mod docking;

pub mod interact;
pub mod sheet;
