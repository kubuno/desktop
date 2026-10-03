//! # `kubuno-views-model` — the element registry model of `.kbview` views
//!
//! The platform-neutral half of the element registry (`vskubuno/docs/WEB-VIEWS.md` WV-1,
//! `docs/VIEWS-SPEC.md` §10): no renderer, no Windows API, no `kubuno_ui` — it builds for every
//! desktop OS and for `wasm32-unknown-unknown`, so the web compiler, the Vite plugin and the
//! language server's web profile share it with the desktop.
//!
//! | Module | What |
//! |---|---|
//! | [`meta`] | The compiled-in metadata types: [`PropKind`], [`PropertyMeta`], [`EventMeta`], [`ArgsChain`], [`LevelMeta`], [`ChildrenModel`], [`LayoutKind`], the `d:` design-time attributes |
//! | [`editor`] | [`EditorKind`]: the Properties window editors (`icon`, `color`, `reference:<Class>`…) |
//! | [`schema`] | The wire shape the registry export writes (`kbview-registry.json`, version 1) |
//! | [`json`] | Reading that document back (desktop export or web registry), into owned types |
//! | [`binding_sources`] | The binding source schema: what a `{Binding …}` can name, and path resolution |
//!
//! `kubuno-views` re-exports every item under its historical path (`kubuno_views::registry::PropKind`,
//! `kubuno_views::events::ArgsChain`, `kubuno_views::registry::export::ComponentJson`…). The registry
//! *tables* (the built-in elements, the common events, the hierarchy levels) and `ComponentMeta`
//! (whose `build` function and class chain belong to the desktop runtime) stay in `kubuno-views`.

pub mod binding_sources;
pub mod editor;
pub mod json;
pub mod meta;
pub mod schema;

pub use editor::EditorKind;
pub use json::{load_registry, load_registry_slice, ComponentEntry, EventEntry, LoadError, PropKindEntry, PropertyEntry, RegistryDocument};
pub use meta::{
    design_time_attribute, ArgsChain, ChildrenModel, DesignTimeAttribute, EventCategory, EventMeta, LayoutKind, LevelMeta, PropKind, PropertyMeta, Routing,
    DESIGN_TIME_ATTRIBUTES,
};
