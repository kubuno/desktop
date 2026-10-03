//! # `kubuno-views` — declarative XML views for `kubuno_ui`
//!
//! Phases 2a-2c of the plan in `vskubuno/docs/XML_VIEWS.md` §8: the lossless
//! syntax tree and metadata registry (2a), scoped to the tokenizer subset §6
//! describes (2b — folded into [`syntax`]/[`ast`] rather than a separate
//! module), and now the interpreter (2c): parse → validate → build a live
//! [`node::ViewNode`] tree, and paint it every frame against a
//! [`binding::ViewModel`] and a [`binding::HandlerTable`], with hot reload
//! keeping the last good tree on a broken file ([`runtime::Runtime`]).
//! Nothing in `kubuno-ui` is imported by name here except through its public
//! builder API (§4: "never the reverse" — this crate depends on `kubuno-ui`,
//! not vice versa; `kubuno-ui` is untouched).
//!
//! | Module | What | Design note reference |
//! |---|---|---|
//! | [`syntax`] | Lossless lexer + error-tolerant parser + `rowan` green/red tree | §6 |
//! | [`ast`] | Typed `Document`/`Element`/`Attribute` layer over [`syntax`] | §6 |
//! | [`edit`] | Surgical set/insert/remove/move, one precise text splice each | §6 ("edited … by the VS designer and by Claude") |
//! | [`registry`] | The `component!` metadata + `build` table (Button, Switch, TextField, Card, Stack) | §4 |
//! | [`validate`] | Unknown elements/attributes, bad enum/type values, invalid children | §5 ("the same diagnostic … the language server", "hot reload") |
//! | [`binding`] | `{Binding …}` parsing, [`binding::ViewModel`], [`binding::PropSource`], [`binding::HandlerTable`] | §3, §2 |
//! | [`props`] | `Props`'s typed per-property accessors a `build` closure reads | §4 (the orchestrator's `build:` extension) |
//! | [`node`] | [`node::ViewNode`], the live tree, and its five concrete nodes | §2 ("a generic interpreter") |
//! | [`compile`] | Parse + validate + build, once per file change | §5 |
//! | [`runtime`] | [`runtime::Runtime`] (bind+paint every frame, last-good-plan on error) + [`runtime::FileWatcher`] | §5 |
//!
//! Since WV-1 (`vskubuno/docs/WEB-VIEWS.md`) [`syntax`], [`ast`], [`edit`] and the `{Binding}`/`{Res}` grammars live in the
//! platform-neutral `kubuno-views-syntax` crate, and the registry metadata types and JSON schema in
//! `kubuno-views-model` — both buildable for any OS and for WebAssembly, so the web tooling shares them.
//! This crate re-exports them under the paths above and keeps the runtime, the rendering, Windows and
//! the design mode.
//!
//! ## What is still deferred
//!
//! - **Dock/Anchor layout, and any component beyond the five registered
//!   ones** (`Panel`, `Label`, `RadioButtons`, `NumericField`, `ScrollArea`…):
//!   `<Card>`/`<Stack>` already exercise Flow layout and the `SingleWidget`/
//!   `List` children models end to end; Dock/Anchor and the rest of §7's
//!   worked example are mechanical repetition of the same pattern, left for
//!   whichever screen needs them first rather than speculative metadata.
//! - **Named spacing tokens** (`Padding="XL"`, `Gap="none"`): still plain DIP
//!   numbers (see [`registry::PropKind::F32`]'s doc) — no closed enum for
//!   them exists in `kubuno-ui`/`kubuno-controls` to validate against.
//! - **`build.rs` code generation** (§2's option B) and the language server
//!   (§8 phase 3): both read this crate's `ast`/`registry` unchanged: nothing
//!   here needs to change to support them later.
//! - **List-bound `<DataTable>`/`<Column>`** (§3's "lists follow the shape
//!   `admin_users.rs::table()` already uses"): not one of the five registered
//!   components.

// Visual Studio shows these types readably (Locals, Watch, DataTips): the natvis is embedded in the PDB
// of every application linking this crate (`vskubuno/docs/DEBUGGING.md`).
#![debugger_visualizer(natvis_file = "../natvis/kubuno_views.natvis")]

// `#[derive(EventArgs)]` expands to `::kubuno_views::events::…` paths; this
// alias makes them resolve inside this crate too (the standard args use it).
extern crate self as kubuno_views;

// The grammar lives in `kubuno-views-syntax` (platform-neutral, WV-1): re-exported under its
// historical paths (`kubuno_views::ast`, `kubuno_views::edit`, `kubuno_views::syntax`).
pub use kubuno_views_syntax::ast;
pub mod binding;
// Every control clips its children to its own box (WinForms): the clip in force while a view paints.
mod clip;
mod clock;
pub mod compile;
// The control hierarchy (EVT-7a) and the built-in control classes: see their module docs.
pub mod component;
pub mod controls;
/// `Debugger.Break()`-like helpers: `debug_break`, `debug_break_on_error`, `.break_on_err()`.
pub mod debug;
pub use debug::{debug_break, debug_break_on_error};
/// The Rust half of the DSG-6 design mode (`vskubuno/docs/DESIGNER.md` §6):
/// the frame-local layout map + hit-testing, the [`node::ViewNode`] decorator
/// that records it, adorner geometry, and the pure selection/nudge/delete
/// request-generation logic `examples/view_embed.rs` drives.
pub mod design;
/// Drag and drop (EVT-8): `do_drag_drop` and the operation it returns.
pub mod dnd;
pub use kubuno_views_syntax::edit;
/// The typed, WinForms-like event system (`vskubuno/docs/EVENTS.md`, EVT-1):
/// [`events::EventArgs`], [`events::Event`] / [`events::Subscription`],
/// [`events::ElementRef`] / [`events::Sender`] and the standard args catalogue.
pub mod events;
/// Typed binding conversions (DATA-2): `FormatString`, `NullValue`, cultures.
pub mod format;
pub mod icon;
pub mod node;
/// Owner-draw of list-like elements (EVT-8): `DrawMode`, `OnDrawItem`, `OnMeasureItem`.
pub mod owner_draw;
/// `use kubuno_views::prelude::*;`: what a view's code-behind uses (EVT-4).
pub mod prelude;
/// The line-delimited JSON IPC protocol `examples/view_embed.rs` speaks with
/// `vskubuno`'s `RustDesignSurfaceHost` over its own stdin/stdout (DSG-6) —
/// see `vskubuno/docs/DESIGNER.md`'s "DSG-6 protocol" section.
pub mod protocol;
pub mod props;
pub mod registry;
// `{Res key}`: resource references (vskubuno docs/RESOURCES.md).
pub mod resources;
pub mod runtime;
/// The view's named components and data-binding providers (DATA-2).
pub mod scope;
/// The values of the WinForms-rich property set (`vskubuno/docs/EVENTS.md` §16): colours (theme
/// tokens and free colours), fonts, boxes, sizes, cursors and the WCAG contrast check.
pub mod style;
/// Honouring the property set every control inherits (`Enabled`, `Visible`, colours, font, size
/// limits, tooltip, cursor, accessibility…) around each element's own node.
pub mod common;
pub use common::FrameServices;
pub use kubuno_views_syntax::syntax;
/// The language of the messages shown to the user (`KUBUNO_UI_LANG`).
pub mod messages;
/// Tolerant compilation for the designer: placeholders, ignored attributes, the last good preview
/// (`vskubuno/docs/DESIGNER.md` §17).
pub mod tolerant;
pub mod validate;
/// Virtual regions (`vskubuno/docs/DESKTOP-MIGRATION.md` F3): the sub-elements a node lays out
/// itself (a ribbon's tabs, groups and buttons), selectable in the designer and routed like
/// elements.
pub mod virtual_regions;
/// The menu family (`vskubuno/docs/MENUS.md`): commands, accelerators, the menu bar, drop-down and
/// split buttons, and how a menu shows in the designer.
pub mod menus;
/// What a view asks of its window: the `Form` properties, tooltips, context menus.
pub mod window;

/// `#[kubuno_views::event_handlers] impl ViewModel { fn ok_click(&mut self, sender: &Sender<Button>, e: &MouseEventArgs) { … } }`:
/// typed handlers (`vskubuno/docs/EVENTS.md` §5.4, EVT-4) — see [`events::typed`] for the
/// accepted signatures and the macro's own documentation for its errors.
pub use kubuno_views_macros::event_handlers;
// `#[value_converter]` on an `impl binding::ValueConverter for T`: a `Converter=Name` of the bindings.
pub use kubuno_views_macros::value_converter;
