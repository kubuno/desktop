//! Kubuno Documents' platform-neutral core (vskubuno `docs/DOCUMENTS-EDITING.md` §1).
//!
//! * [`model`] — the stored document, held losslessly (bare ProseMirror document or multi-page
//!   envelope; every key we do not model kept as its original bytes).
//! * [`pm`] — ProseMirror positions over it.
//! * [`marks`] — character formatting read from and written to `marks` arrays.
//! * [`measure`] — the `Measure` trait, the only door to the platform's fonts.
//! * [`layout`] — the port of the web editor's `canvas-engine.ts`: parse, line breaking, tables,
//!   the continuous layout, pagination, caret and selection geometry.
//!
//! No UI and no OS API: the crate builds for `wasm32-unknown-unknown`, Linux, macOS and Windows.

pub mod base64;
pub mod edit;
pub mod editor;
pub mod layout;
pub mod marks;
pub mod measure;
pub mod model;
pub mod pm;
