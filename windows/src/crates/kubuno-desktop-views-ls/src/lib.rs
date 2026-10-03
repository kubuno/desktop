//! `kubuno-desktop-views-ls` — the LSP language server for `.kbview` files
//! (`vskubuno/docs/ARCHITECTURE.md` phase 3, `vskubuno/docs/XML_VIEWS.md`
//! §8's phase 3 row: "Language server over the same `rowan` AST + registry
//! (diagnostics, hover, completion from `enum` metadata)").
//!
//! Built entirely on [`kubuno_desktop_views`]' public API — the lossless
//! [`kubuno_desktop_views::syntax`] tree, the typed [`kubuno_desktop_views::ast`] layer, the
//! [`kubuno_desktop_views::validate`] validator, the [`kubuno_desktop_views::registry`]
//! component metadata and, since work package DSG-2
//! (`vskubuno/docs/DESIGNER.md`), [`kubuno_desktop_views::edit`]'s surgical splice
//! API — never on the in-progress interpreter internals (`props`/`node`/
//! `compile`/`runtime`/`binding`), which another agent is actively building
//! at the time this crate was written. See each module's own doc comment for
//! the LSP feature it implements.
//!
//! | Module | Feature |
//! |---|---|
//! | [`position`] | Byte offset ⇄ LSP UTF-16 `Position` conversion |
//! | [`documents`] | Open-document store (full text sync) |
//! | [`tree`] | Shared cursor-offset → syntax-tree-context helpers |
//! | [`common_attrs`] | The `Dock`/`Anchor`/… attributes every element accepts (worked around — see its own doc) |
//! | [`diagnostics`] | Parse errors + validator findings (and contrast warnings) → LSP `Diagnostic` |
//! | [`bindings`] | `kubuno/bindingPaths` — the paths of a view's data context (kept for older clients) |
//! | [`binding_sources`] | `kubuno/bindingSources` — the source schema of a view's bindings (data context, item row, data components, resources, converters) |
//! | [`binding_lsp`] | `{Binding …}` diagnostics, completion, hover, definition and the « Mettre à jour les liaisons » quick fix |
//! | [`completion`] | Element name / attribute name / enum value / closing tag |
//! | [`hover`] | Component/property/event docs from the registry |
//! | [`symbols`] | `textDocument/documentSymbol` — the element tree, `x:Name` as the label |
//! | [`definition`] | `Click="handler"` → `fn handler` in a sibling `.rs` file (text search) |
//! | [`edit_bridge`] | `kubuno/applyEdit`, `kubuno/elementAtOffset`, `kubuno/rangeOfElement` — the designer's surgical edit/selection-sync methods (DSG-2) |
//! | [`handler_insert`] | `kubuno/createHandler` — double-click a control/event → create the Rust handler (DSG-10) |
//! | [`handlers`] | `kubuno/compatibleHandlers`, `kubuno/renameHandler`, `kubuno/removeHandler`, handler warnings and quick fixes, F2 rename (EVT-5) |
//! | [`sources`] | The client's open buffers, read instead of the files (`openFiles`) |
//! | [`project`] | The project's own controls, scanned from its sources (EVT-7b) |
//! | [`view_kind`] | `.kbview` (forms, windows, dialogs) vs `.kbcontrol` (user controls): the extension warning and its rename quick fix |
//! | [`server`] | The `lsp-server` `Connection` main loop tying all of the above together |
//!
//! ## Formatting — deliberately not implemented
//!
//! The brief allows skipping `textDocument/formatting` when it cannot be
//! done losslessly, and that is the case here: `kubuno-desktop-views`' own `edit`
//! module doc is explicit that "every byte" outside a precisely-touched
//! splice range is meaningful (`XML_VIEWS.md` §6 — comments, attribute
//! order, blank-line grouping the VS designer and Claude both edit
//! surgically around), and `syntax::lexer`'s `TEXT` token can be an
//! element's actual content (`<Label> foo </Label>`), so a generic
//! reformatter cannot tell "insignificant layout whitespace" from
//! "significant text content" without a policy this phase's design note
//! never states (no canonical indentation width/style is documented
//! anywhere in `XML_VIEWS.md`, unlike, say, `rustfmt`'s settings for Rust).
//! Guessing one blind risks exactly the corruption class `kubuno-desktop-views`'
//! whole surgical-edit design (splice one range, touch nothing else) exists
//! to prevent. `server::server_capabilities` therefore does not advertise
//! `document_formatting_provider`, and no `formatting` module exists.

// `{Binding …}`: the source schema (`binding_sources`) and its diagnostics, completion, hover, definition and quick fix (`binding_lsp`).
pub mod binding_lsp;
pub mod binding_sources;
pub mod bindings;
pub mod code_behind;
pub mod common_attrs;
pub mod convert_handlers;
pub mod completion;
pub mod definition;
pub mod diagnostics;
pub mod documents;
pub mod edit_bridge;
pub mod fs_uri;
pub mod handler_insert;
pub mod handlers;
pub mod hover;
// `xmlns` / `xmlns:x` / `xmlns:d`: the undeclared-prefix note, its quick fix and completion (VIEWS-SPEC.md §3).
pub mod namespaces;
pub mod position;
pub mod project;
// `{Res key}`: resource keys of the project (vskubuno docs/RESOURCES.md).
pub mod resources;
pub mod server;
pub mod sources;
// `<Settings>` members from `.kbsettings`, platform-only elements (vskubuno docs/STORAGE-COMPONENTS.md).
pub mod storage;
pub mod symbols;
pub mod tree;
pub mod view_kind;
