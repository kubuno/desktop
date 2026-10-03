//! The open document's view state ([`state`]).
//!
//! The stored document itself — held losslessly, edited and laid out — is the platform-neutral
//! core's (`kubuno_office_docs_core::model`, `kubuno_office_docs_core::editor`): it moved there with the layout
//! engine so the web and the other desktops can share it (vskubuno `docs/DOCUMENTS-EDITING.md`).

pub mod state;
