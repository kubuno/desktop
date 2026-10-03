//! The Windows integration the views do not touch: `painter` (the Direct2D drawing surface),
//! `clipboard` (the multi-format clipboard), `ime` (where the IME and the system caret think the
//! caret is) and `images` (WIC decoding and re-encoding).

pub mod clipboard;
pub mod file_dialog;
pub mod ime;
pub mod images;
pub mod painter;
