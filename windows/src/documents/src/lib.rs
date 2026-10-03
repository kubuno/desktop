//! Kubuno Documents — the native desktop word processor of the Office module, written like a
//! Windows Forms application.
//!
//! The library holds everything the Visual Studio designer links to render the views (the design
//! build compiles this crate): the main form [`DocumentWindow`] (`views/document_window.kbview` +
//! `views/document_window.rs`) with its declarative ribbon, the Backstage's user control
//! [`BackstageInfo`], and the custom controls [`PageCanvas`] (the paginated document),
//! [`HorizontalRuler`], [`VerticalRuler`], [`RulerCorner`] and [`ZoomSlider`]. `main.rs` only
//! starts it.
//!
//! The sources are grouped by role (vskubuno `docs/DESKTOP-MIGRATION.md`, "Source layout"), each
//! view next to its code-behind of the same name:
//!
//! - `views/` — the top-level view: the document window (and the tests over the views);
//! - `pages/` — the Backstage's user control (`backstage_info`);
//! - `controls/` — the custom-drawn controls: `page_canvas`, `zoom_slider`, `ruler/`;
//! - `model/` — the stored ProseMirror JSON, held losslessly, and `state`, the view state of an
//!   open document;
//! - `doc/` — the document engine: lays out and paints the pages (a port of the web editor's
//!   `canvas-engine.ts`, so the same file breaks lines and paginates identically in both);
//! - `edit/`, `api/` — the editing and server paths, not wired yet;
//! - `platform/` — `painter`, the Direct2D drawing surface;
//! - `resources/` — the strings and icons (`resources.kbres`, `resources.fr.kbres`).

pub mod api;
pub mod controls;
pub mod doc;
pub mod model;
pub mod pages;
pub mod platform;
pub mod views;

pub use controls::page_canvas::PageCanvas;
pub use controls::ruler::{HorizontalRuler, RulerCorner, VerticalRuler};
pub use controls::zoom_slider::ZoomSlider;
pub use pages::backstage_info::BackstageInfo;
pub use views::document_window::{DocumentWindow, Options};

// `Resources::app_title()`, `Resources::status_page()`… — the strings of `resources/resources.kbres`
// (neutral English) and `resources/resources.fr.kbres`, in the current UI culture; `{Res key}` in the views.
kubuno_desktop::resources!(pub Resources, "resources/resources.kbres");
