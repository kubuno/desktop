//! Code-behind of the user control `ConversationPane` (`conversation_pane.kbcontrol`): the open
//! conversation — header, `MessageThread`, composer — or the empty state. The window shows a
//! conversation with [`ConversationPane::show`] (typed access: `self.conversation.with(|p| p.show(…))`)
//! and handles what it raises: `SendRequested` (Enter or the Send button), `Command` (the details
//! and more menus), `MessageActivated`, `ReachedTop`.

use std::sync::Arc;

use kubuno_desktop::prelude::Shared;
use kubuno_desktop::views::prelude::*;

use crate::controls::message_thread::{MessageActivatedEventArgs, ThreadData};
use crate::model::view_model::Header;

/// `SendRequested`: the text to send (already trimmed, never empty).
#[derive(EventArgs, Debug, Clone, Default, PartialEq)]
pub struct SendEventArgs {
    pub text: String,
}

/// `Command`: a menu command of the pane the window carries out (`copy_link`, `copy_id`,
/// `mark_read`, `close`).
#[derive(EventArgs, Debug, Clone, Default, PartialEq)]
pub struct CommandEventArgs {
    pub command: String,
}

/// The open conversation (see the module doc).
#[derive(UserControl, Default)]
#[user_control(view = "conversation_pane.kbcontrol", default_event = "SendRequested")]
#[category("Chat")]
pub struct ConversationPane {
    base: UserControlCore,
    #[property(bindable)]
    #[category("Data")]
    pub title: String,
    #[property(bindable)]
    #[category("Data")]
    pub initial: String,
    /// `Circle` (a person) or `Rounded` (a group).
    #[property(bindable)]
    #[category("Appearance")]
    pub avatar_shape: String,
    #[property(bindable)]
    #[category("Appearance")]
    pub presence: String,
    /// The line under the name ("en ligne", "en train d'écrire…").
    #[property(bindable)]
    #[category("Data")]
    pub status: String,
    /// The status line's colour: a theme token (`Success`, `Primary`, `TextTertiary`).
    #[property(bindable)]
    #[category("Appearance")]
    pub status_color: String,
    /// The conversation's kind, the details menu's heading.
    #[property(bindable)]
    #[category("Data")]
    pub kind_text: String,
    /// The conversation exists on the server (its link and id can be copied).
    #[property(bindable)]
    #[category("Data")]
    pub has_server_id: bool,
    #[property(bindable)]
    #[category("Behavior")]
    pub has_conversation: bool,
    #[property(bindable)]
    #[category("Behavior")]
    pub no_conversation: bool,
    /// The header shows the in-conversation search field.
    #[property(bindable)]
    #[category("Behavior")]
    pub searching: bool,
    #[property(bindable)]
    #[category("Behavior")]
    pub not_searching: bool,
    #[property(bindable)]
    #[category("Data")]
    pub search_text: String,
    /// The composer's text.
    #[property(bindable)]
    #[category("Data")]
    pub draft: String,
    /// The thread shown by the MessageThread.
    #[property(bindable)]
    #[category("Data")]
    pub thread: Shared<ThreadData>,
    /// Occurs when the user sends the composer's text (Enter, or the Send button).
    #[event]
    #[category("Action")]
    pub send_requested: Event<SendEventArgs>,
    /// Occurs when a command of the details or more menu is chosen.
    #[event]
    #[category("Action")]
    pub command: Event<CommandEventArgs>,
    /// Occurs when a message is double-clicked.
    #[event]
    #[category("Action")]
    pub message_activated: Event<MessageActivatedEventArgs>,
    /// Occurs when the thread is scrolled to its top.
    #[event]
    #[category("Action")]
    pub reached_top: Event<EmptyEventArgs>,
}

impl ConversationPane {
    /// Shows a conversation (its header and thread), or the empty state for `None`. A change of
    /// conversation leaves the in-conversation search.
    pub fn show(&mut self, header: Option<&Header>, kind: &str, has_server_id: bool, thread: ThreadData) {
        let open = header.is_some();
        if self.thread.key != thread.key {
            self.searching = false;
            self.search_text.clear();
        }
        let h = header.cloned().unwrap_or_default();
        self.title = h.title;
        self.initial = h.initial;
        self.avatar_shape = h.shape;
        self.presence = h.presence;
        self.status = h.status;
        self.status_color = h.status_color;
        self.kind_text = kind.to_string();
        self.has_server_id = has_server_id;
        self.has_conversation = open;
        self.no_conversation = !open;
        self.not_searching = !self.searching;
        let thread = ThreadData { filter: if self.searching { self.search_text.clone() } else { String::new() }, ..thread };
        if *self.thread != thread {
            self.thread = Shared::from(Arc::new(thread));
        }
    }

    /// The composer's text.
    pub fn draft(&self) -> &str {
        &self.draft
    }

    /// Applies the search text to the thread shown.
    fn refilter(&mut self) {
        let filter = if self.searching { self.search_text.clone() } else { String::new() };
        if self.thread.filter != filter {
            let thread = ThreadData { filter, ..(*self.thread).clone() };
            self.thread = Shared::from(Arc::new(thread));
        }
    }
}

#[kubuno_desktop::views::event_handlers]
impl ConversationPane {
    fn send_click(&mut self) {
        let text = self.draft.trim().to_string();
        if text.is_empty() {
            return;
        }
        self.draft.clear();
        self.raise_send_requested(SendEventArgs { text });
    }

    fn search_button_click(&mut self) {
        self.searching = !self.searching;
        self.not_searching = !self.searching;
        if !self.searching {
            self.search_text.clear();
        }
        self.refilter();
    }

    fn thread_search_text_changed(&mut self, e: &TextChangedEventArgs) {
        self.search_text = e.new.clone();
        self.refilter();
    }

    fn copy_link_click(&mut self) {
        self.raise_command(CommandEventArgs { command: "copy_link".into() });
    }

    fn copy_id_click(&mut self) {
        self.raise_command(CommandEventArgs { command: "copy_id".into() });
    }

    fn mark_read_click(&mut self) {
        self.raise_command(CommandEventArgs { command: "mark_read".into() });
    }

    fn close_click(&mut self) {
        self.raise_command(CommandEventArgs { command: "close".into() });
    }

    fn thread_message_activated(&mut self, e: &MessageActivatedEventArgs) {
        self.raise_message_activated(e.clone());
    }

    fn thread_reached_top(&mut self) {
        self.raise_reached_top(EmptyEventArgs);
    }
}
