//! The Windows half of the document engine: DirectWrite measuring ([`fonts`], the core's
//! `Measure`) and Direct2D painting of the core's pages ([`paint`]).
//!
//! The engine itself — parse, line breaking, tables, pagination, caret geometry, editing — is the
//! platform-neutral `kubuno_docs_core`, a port of the web editor's `canvas-engine.ts`.

pub mod fonts;
pub mod paint;

/// Document pixels: CSS px at 96 dpi.
pub type DocPx = f32;
