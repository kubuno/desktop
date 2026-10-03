//! Code-behind of Kubuno Chat's window (`chat_window.kbview`) — Windows Forms' `Form1.cs`.
//!
//! The window owns the data ([`ChatState`]) and fills its two user controls through typed access
//! (`self.list_pane.with(…)`, `self.conversation.with(…)`) from what `view_model` computes. The
//! background API threads (`api`) post their results to the UI thread with the view's
//! `UiDispatcher` (Windows Forms' `BeginInvoke`); a `kubuno://` link forwarded by a second launch
//! arrives as `WM_COPYDATA` through the form's message hook (`Form::on_message`).

use std::sync::Arc;
use std::time::Instant;

use kubuno_desktop::prelude::*;

use crate::services::api::{self, ChatEvent, Sink};
use crate::pages::conversation_list_pane::ConversationListPane;
use crate::pages::conversation_pane::{CommandEventArgs, ConversationPane, SendEventArgs};
use crate::controls::message_thread::MessageActivatedEventArgs;
use crate::model::{ChatState, Section};
use crate::platform::protocol::{self, DeepLink};
use crate::model::view_model;
use crate::Resources;

/// How the window starts (the command line, read by `main`).
#[derive(Debug, Clone, Default)]
pub struct Options {
    /// Shows the offline sample only, never the server's data (`--sample`): deterministic
    /// screenshots and a demo without an account.
    pub sample: bool,
    /// A `kubuno://` link this launch carried.
    pub link: Option<DeepLink>,
}

impl Options {
    /// Reads `--sample` and a `kubuno://` argument. A Debug build under a debugger runs the sample unless
    /// `--live` says otherwise (the shell's rule: a debugging session never reaches the real profile).
    pub fn from_args() -> Self {
        let args: Vec<String> = std::env::args().collect();
        Self { sample: kubuno_desktop_header_data::sample_requested(&args), link: protocol::from_args() }
    }
}

/// Kubuno Chat's main window.
#[kubuno_desktop::view("chat_window.kbview")]
pub struct ChatWindow {
    /// The conversation list, typed (`Custom<T>`: its methods through `with`).
    #[control]
    list_pane: Custom<ConversationListPane>,
    /// The open conversation, typed.
    #[control]
    conversation: Custom<ConversationPane>,
    /// The rail's section (`chats`, `meetings`), two-way with the Sidebar.
    #[bind]
    section: String,
    #[bind]
    light_theme: bool,
    #[bind]
    dark_theme: bool,
    #[bind]
    french: bool,
    #[bind]
    english: bool,
    /// The profile menu's heading: the account, or the offline note.
    #[bind]
    account_text: String,
    state: ChatState,
    options: Options,
    /// Where the API threads post (built once the window is open).
    sink: Option<Sink>,
    /// A hand-off waiting for the conversation list to arrive.
    pending_link: Option<DeepLink>,
    /// The live WebSocket is opened once, after the first identity/list arrive.
    realtime_started: bool,
}

impl ChatWindow {
    pub fn new(options: Options) -> Self {
        let mut window = Self { state: ChatState::new(), pending_link: options.link.clone(), options, section: Section::Chats.key().to_string(), ..Self::default() };
        window.initialize_component();
        window
    }

    /// The data shown (tests, diagnostics).
    pub fn state(&self) -> &ChatState {
        &self.state
    }

    fn chat_window_load(&mut self, _sender: &Form, _e: &EventArgs) {
        self.sync_menus();
        self.refresh();
        // Results of the API threads come back on the UI thread, at the next frame.
        let Some(dispatcher) = self.dispatcher() else {
            tracing_warn("the window has no dispatcher: the chat stays offline");
            return;
        };
        // The title bar's waffle and avatar: the account's apps, favourites and other accounts, through the
        // shell's broker (the sample: the controls' design data).
        let mut header = kubuno_desktop_header_data::FeedConfig::new("kubuno-chat");
        if !self.options.sample {
            header.proxy = kubuno_desktop_sync::get_proxy();
        }
        kubuno_desktop_header_data::start(kubuno_desktop_header_data::HeaderOptions::for_app(&["chat"]), header, self.options.sample, Some(dispatcher.clone()));
        let post = dispatcher.clone();
        let sink: Sink = Arc::new(move |event: ChatEvent| {
            drop(post.begin_invoke(move |w: &mut ChatWindow| w.on_chat_event(event)));
        });
        self.sink = Some(sink.clone());
        // A `kubuno://` link forwarded by a second launch (`protocol::forward_to_running`).
        self.on_message(windows::Win32::UI::WindowsAndMessaging::WM_COPYDATA, move |m| {
            let link = protocol::link_from_copydata_lparam(m.lparam.0)?;
            drop(dispatcher.begin_invoke(move |w: &mut ChatWindow| w.open_deep_link(link)));
            Some(1)
        });
        if !self.options.sample {
            api::start_initial_load(sink);
        }
    }

    fn chat_window_shown(&mut self, _sender: &Form, _e: &EventArgs) {
        // A link this launch carried: opened now if the list holds it, else once it arrives.
        if let Some(link) = self.pending_link.take() {
            self.open_deep_link(link);
        }
    }

    /// Pushes the state to the views: the list's rows, the open conversation.
    fn refresh(&mut self) {
        let (rows, selected) = view_model::list_rows(&self.state);
        let (title, empty) = (view_model::list_title(&self.state), view_model::list_empty_text(&self.state));
        self.list_pane.with(|p| p.show_rows(&title, rows, selected, &empty));
        let header = view_model::header(&self.state, Instant::now());
        let active = self.state.active();
        let kind = active.map(view_model::kind_label).unwrap_or("");
        let has_id = active.is_some_and(|c| !c.id.is_empty());
        let thread = view_model::thread(&self.state, "");
        self.conversation.with(|p| p.show(header.as_ref(), kind, has_id, thread));
    }

    /// The settings and profile menus' checks and texts.
    fn sync_menus(&mut self) {
        let dark = kubuno_desktop::Application::theme().mode == kubuno_desktop::ui::ThemeMode::Dark;
        self.light_theme = !dark;
        self.dark_theme = dark;
        let french = kubuno_desktop::resources::culture().starts_with("fr");
        self.french = french;
        self.english = !french;
        self.account_text = match (&self.state.instance, self.state.live) {
            (Some(instance), true) => format!("{} — {}", Resources::account_connected(), api::server_of(instance).unwrap_or_else(|| instance.clone())),
            _ => Resources::account_offline().to_string(),
        };
    }

    /// Applies what an API thread posted.
    fn on_chat_event(&mut self, event: ChatEvent) {
        match event {
            ChatEvent::Ready { instance, me, conversations } => {
                self.state.instance = Some(instance);
                if me.is_some() {
                    self.state.me = me;
                }
                self.state.apply_conversations(conversations);
                self.sync_menus();
            }
            ChatEvent::Messages { conv_id, messages } => self.state.apply_messages(&conv_id, messages),
            ChatEvent::Incoming { conv_id, message } => self.state.apply_incoming(&conv_id, message, Resources::you_prefix()),
            ChatEvent::Presence { user_id, online } => self.state.apply_presence(user_id, online),
            ChatEvent::Typing { conv_id, on, .. } => self.state.apply_typing(conv_id, on, Instant::now()),
            ChatEvent::Error(message) => tracing_warn(&message),
        }
        // A hand-off that arrived before the list can now be opened.
        if let Some(link) = self.pending_link.take() {
            if !self.state.select_id(link.conversation_id()) {
                self.pending_link = Some(link);
            }
        }
        // The live channel opens once we know the account — one socket for the run.
        if !self.realtime_started {
            if let (Some(instance), Some(sink)) = (self.state.instance.clone(), self.sink.clone()) {
                api::start_realtime(sink, instance, self.state.me.clone());
                self.realtime_started = true;
            }
        }
        self.ensure_thread_loaded();
        self.refresh();
    }

    /// Requests the open conversation's messages the first time it is shown.
    fn ensure_thread_loaded(&mut self) {
        if !self.state.live {
            return;
        }
        let (Some(instance), Some(sink)) = (self.state.instance.clone(), self.sink.clone()) else { return };
        if let Some(c) = self.state.active().filter(|c| !c.loaded && !c.id.is_empty()) {
            api::start_messages_load(sink, instance, c.id.clone(), self.state.me.clone());
        }
    }

    /// Acts on a `kubuno://` hand-off: raises the window and opens the target conversation, or
    /// holds the link until the list arrives.
    pub fn open_deep_link(&mut self, link: DeepLink) {
        kubuno_desktop::tracing::info!("[protocol] hand-off {} -> {}", if link.is_meeting() { "meeting" } else { "conversation" }, link.conversation_id());
        kubuno_desktop::controls::host::restore_and_focus();
        if link.is_meeting() {
            self.state.section = Section::Meetings;
            self.section = Section::Meetings.key().to_string();
        }
        if self.state.select_id(link.conversation_id()) {
            self.ensure_thread_loaded();
        } else {
            self.pending_link = Some(link);
        }
        self.refresh();
    }

    // ── Handlers ───────────────────────────────────────────────────────────────────────────

    fn nav_selection_changed(&mut self, e: &TextChangedEventArgs) {
        self.state.section = Section::from_key(&e.new);
        self.refresh();
    }

    fn list_pane_conversation_selected(&mut self, e: &ItemEventArgs) {
        let visible = view_model::visible_indices(&self.state);
        if let Some(&index) = visible.get(e.index) {
            if self.state.select(index) {
                self.ensure_thread_loaded();
            }
        }
        self.refresh();
    }

    fn list_pane_search_changed(&mut self, e: &TextChangedEventArgs) {
        self.state.filter = e.new.clone();
        self.refresh();
    }

    fn conversation_send_requested(&mut self, e: &SendEventArgs) {
        let Some(conv_id) = self.state.push_own(&e.text, Resources::today(), &api::now_hhmm(), Resources::you_prefix()) else { return };
        if let (true, false, Some(instance), Some(sink)) = (self.state.live, conv_id.is_empty(), self.state.instance.clone(), self.sink.clone()) {
            api::start_send(sink, instance, conv_id, e.text.clone(), self.state.me.clone());
        }
        self.refresh();
    }

    fn conversation_command(&mut self, e: &CommandEventArgs) {
        let active = self.state.active().cloned();
        match (e.command.as_str(), active) {
            ("copy_link", Some(c)) if !c.id.is_empty() => {
                let link = if c.meeting { DeepLink::Meet(c.id) } else { DeepLink::Chat(c.id) };
                kubuno_desktop::controls::host::set_clipboard_text(&link.to_url());
            }
            ("copy_id", Some(c)) if !c.id.is_empty() => {
                kubuno_desktop::controls::host::set_clipboard_text(&c.id);
            }
            ("mark_read", Some(_)) => {
                if let (Some((conv_id, last)), Some(instance)) = (self.state.mark_active_read(), self.state.instance.clone()) {
                    api::start_mark_read(instance, conv_id, last);
                }
            }
            ("close", _) => self.state.selected = None,
            _ => {}
        }
        self.refresh();
    }

    fn conversation_message_activated(&mut self, e: &MessageActivatedEventArgs) {
        // A double-click copies the message, like the web's "Copy" action.
        kubuno_desktop::controls::host::set_clipboard_text(&e.text);
    }

    fn conversation_reached_top(&mut self) {
        // The API serves the 50 newest messages only (`fetch_messages`): nothing older to load yet.
        kubuno_desktop::tracing::debug!("thread scrolled to its top");
    }

    fn typing_timer_tick(&mut self) {
        if self.state.expire_typing(Instant::now()) {
            self.refresh();
        }
    }

    fn theme_light_click(&mut self) {
        kubuno_desktop::Application::set_theme(kubuno_desktop::ui::Theme::light());
        self.sync_menus();
    }

    fn theme_dark_click(&mut self) {
        kubuno_desktop::Application::set_theme(kubuno_desktop::ui::Theme::dark());
        self.sync_menus();
    }

    fn language_fr_click(&mut self) {
        kubuno_desktop::resources::set_culture("fr");
        self.sync_menus();
        self.refresh();
    }

    fn language_en_click(&mut self) {
        kubuno_desktop::resources::set_culture("en");
        self.sync_menus();
        self.refresh();
    }

    fn quit_click(&mut self) {
        kubuno_desktop::Application::exit();
    }
}

fn tracing_warn(message: &str) {
    kubuno_desktop::tracing::warn!("[chat] {message}");
}
