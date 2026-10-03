//! # `kubuno-views-syntax` — the `.kbview` grammar
//!
//! The platform-neutral half of `kubuno-views` (`vskubuno/docs/WEB-VIEWS.md` WV-1): the lossless
//! parser, the typed tree, the surgical edits and the grammars of attribute values, with no renderer,
//! no Windows API and no `kubuno_ui`. It builds for every desktop OS and for
//! `wasm32-unknown-unknown`, so the web's `@kubuno/views-compiler` (WASM), its Vite plugin and the
//! language server's web profile parse views with the *same* grammar as the desktop
//! (`XML_VIEWS.md` §6: one grammar, never a second one drifting from it).
//!
//! | Module | What |
//! |---|---|
//! | [`syntax`] | Lossless lexer + error-tolerant parser + `rowan` green/red tree, [`syntax::Diagnostic`] |
//! | [`ast`] | Typed `Document`/`Element`/`Attribute` layer, element ids ([`ast::Element::stable_id`]) |
//! | [`edit`] | Surgical set/insert/remove/move, one precise text splice each; [`edit::match_line_endings`] |
//! | [`ids`] | Element id arithmetic (parent, ancestry, top-level selection) |
//! | [`binding`] | The `{Binding …}` grammar: parts, keys, issues, [`binding::BindingSyntax`] |
//! | [`res`] | The `{Res key}` grammar |
//! | [`shortcut`] | The keyboard shortcut grammar (`Ctrl+Shift+S`) and mnemonic letters |
//! | [`markup`] | `x:` directives and `d:` design-time attributes |
//! | [`view_kind`] | The `.kbview` / `.kbcontrol` file-kind rules |
//! | [`validate`] | The registry-independent core of the validator |
//!
//! `kubuno-views` re-exports every module here under its historical path
//! (`kubuno_views::syntax::parse`, `kubuno_views::ast::Element`, `kubuno_views::edit::set_attribute`…),
//! so its users never name this crate.

pub mod ast;
pub mod binding;
pub mod edit;
pub mod ids;
pub mod markup;
pub mod namespaces;
pub mod res;
pub mod shortcut;
pub mod syntax;
pub mod validate;
pub mod view_kind;

/// The registry model this grammar validates against (re-exported for convenience).
pub use kubuno_views_model as model;
