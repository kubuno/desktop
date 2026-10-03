//! Kubuno Chat — the native desktop chat client, written like a Windows Forms application.
//!
//! The library holds everything the Visual Studio designer links to render the views (the
//! design build compiles this crate): the main form [`ChatWindow`] (`views/chat_window.kbview` +
//! `views/chat_window.rs`), its user controls ([`ConversationListPane`], [`ConversationRow`],
//! [`ConversationPane`]) and the [`MessageThread`] custom control. `main.rs` only starts it.
//!
//! The sources are grouped by role (vskubuno `docs/DESKTOP-MIGRATION.md`, "Source layout"), each
//! view next to its code-behind of the same name:
//!
//! - `views/` — the top-level view: the window;
//! - `pages/` — the panes the window shows and the item template of the conversation list;
//! - `controls/` — the custom-drawn controls (`MessageThread`);
//! - `model/` — the state (pure) and `view_model` (what the views show, pure);
//! - `services/` — `api`, the HTTP/WebSocket client, on background threads;
//! - `platform/` — `protocol`, the `kubuno://` hand-off and the single instance;
//! - `resources/` — the strings and icons (`resources.kbres`, `resources.fr.kbres`).

pub mod controls;
pub mod model;
pub mod pages;
pub mod platform;
pub mod services;
pub mod views;

pub use controls::message_thread::MessageThread;
pub use pages::conversation_list_pane::ConversationListPane;
pub use pages::conversation_pane::ConversationPane;
pub use pages::conversation_row::ConversationRow;
pub use views::chat_window::{ChatWindow, Options};

// `Resources::app_title()`, `Resources::status_online()`… — the strings of `resources/resources.kbres`
// (neutral English) and `resources/resources.fr.kbres`, in the current UI culture; `{Res key}` in the views.
kubuno::resources!(pub Resources, "resources/resources.kbres");
