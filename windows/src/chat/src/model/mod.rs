//! The chat's data: conversations, messages, and the state the window shows them from.
//!
//! The shapes mirror the web chat's `ConversationSummary` / `DecodedMessage`
//! (see `chat/frontend/src/api.ts`): a conversation carries a name, the newest
//! snippet and an unread count; a message is a decoded text with a side and,
//! for our own, a delivery status. [`sample`] is the offline dataset shown until
//! the server answers (and with `--sample`, for deterministic screenshots).
//!
//! Everything here is plain data and pure logic (no UI, no thread), so the
//! behaviour of the window is unit-tested through [`ChatState`].
//!
//! The folder also holds `view_model`: what the views show, computed from this state (pure too).

pub mod view_model;

use std::collections::HashSet;
use std::time::{Duration, Instant};

/// Delivery state of one of our own messages — the tick marks the web draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Status {
    /// Someone else's message: no ticks.
    #[default]
    None,
    /// Sent, not confirmed delivered yet (one tick): the optimistic state of a send.
    Sent,
    Delivered,
    Read,
}

/// One message in a conversation.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Message {
    /// The server's message id — used to drop the WebSocket echo of a message we
    /// already appended (optimistic sends carry an empty id until confirmed).
    pub id: String,
    pub text: String,
    /// Wall-clock label, `HH:MM`, as the bubble's meta line shows it.
    pub time: String,
    /// The day the message belongs to, as its date separator shows it ("Aujourd'hui",
    /// "Hier", "lun. 28/09"…); empty when unknown.
    pub day: String,
    /// Ours (right, accent) vs the other side's (left, card).
    pub mine: bool,
    pub status: Status,
}

impl Message {
    fn other(text: &str, day: &str, time: &str) -> Self {
        Self { id: String::new(), text: text.into(), time: time.into(), day: day.into(), mine: false, status: Status::None }
    }
    fn own(text: &str, day: &str, time: &str, status: Status) -> Self {
        Self { id: String::new(), text: text.into(), time: time.into(), day: day.into(), mine: true, status }
    }
}

/// One conversation: a row in the list and, when selected, the thread.
#[derive(Debug, Clone, Default)]
pub struct Conversation {
    /// The server's conversation id — the key `/chat/conversations/:id/messages`
    /// uses. Empty for the offline sample.
    pub id: String,
    pub name: String,
    /// Last-message preview shown under the name in the list.
    pub snippet: String,
    /// The list row's right-aligned time.
    pub time: String,
    pub unread: u32,
    /// A group (squared avatar) rather than a direct contact (round avatar), the
    /// web's `conv_type` distinction.
    pub group: bool,
    /// The other participant's user id for a direct conversation — used to read
    /// their presence (online dot / header status). `None` for groups.
    pub other_id: Option<String>,
    /// A meeting conversation (`is_meeting`) — the kind a `kubuno://meet/` link
    /// hands off; shown with a "Réunion" marker.
    pub meeting: bool,
    /// Messages are fetched the first time the conversation is opened; this says
    /// whether that has happened.
    pub loaded: bool,
    pub messages: Vec<Message>,
}

impl Conversation {
    /// The single uppercase initial the avatar shows, as the web's `HomeList`
    /// does (`getConvName`'s first letter).
    pub fn initial(&self) -> String {
        self.name.chars().find(|c| c.is_alphanumeric()).map(|c| c.to_uppercase().to_string()).unwrap_or_else(|| "?".into())
    }
}

/// Which conversations the list shows (the navigation rail).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Section {
    #[default]
    Chats,
    Meetings,
}

impl Section {
    /// The rail's key of the section (`SidebarItem Key`).
    pub fn key(self) -> &'static str {
        match self {
            Section::Chats => "chats",
            Section::Meetings => "meetings",
        }
    }

    pub fn from_key(key: &str) -> Self {
        if key == "meetings" { Section::Meetings } else { Section::Chats }
    }
}

/// How long a "typing" note lasts without a `typing_stop` (a missed one still clears).
pub const TYPING_TTL: Duration = Duration::from_secs(6);

/// The whole window's data: the conversations, which one is open, the filters,
/// presence and typing.
#[derive(Debug)]
pub struct ChatState {
    pub conversations: Vec<Conversation>,
    /// The open conversation (an index into `conversations`), `None` when none is.
    pub selected: Option<usize>,
    /// The list's search text.
    pub filter: String,
    pub section: Section,
    /// The connected instance id the API calls go through; `None` before it is
    /// resolved (the offline sample is shown until then).
    pub instance: Option<String>,
    /// Our own user id, so a message's `sender_id` tells own from other.
    pub me: Option<String>,
    /// Whether the shown data is live (server) or the offline sample.
    pub live: bool,
    /// User ids the server has told us are online (from `presence_update`).
    pub online: HashSet<String>,
    /// The conversation someone is currently typing in, and when that note expires.
    pub typing: Option<(String, Instant)>,
}

impl Default for ChatState {
    fn default() -> Self {
        Self::new()
    }
}

impl ChatState {
    /// The offline sample, its first conversation open.
    pub fn new() -> Self {
        Self::with(sample())
    }

    pub fn with(conversations: Vec<Conversation>) -> Self {
        let selected = (!conversations.is_empty()).then_some(0);
        Self {
            conversations,
            selected,
            filter: String::new(),
            section: Section::Chats,
            instance: None,
            me: None,
            live: false,
            online: HashSet::new(),
            typing: None,
        }
    }

    pub fn active(&self) -> Option<&Conversation> {
        self.selected.and_then(|i| self.conversations.get(i))
    }

    /// Whether someone is typing in the given conversation right now (unexpired).
    pub fn typing_in(&self, conv_id: &str, now: Instant) -> bool {
        self.typing.as_ref().is_some_and(|(id, until)| id == conv_id && *until > now)
    }

    /// Whether a user id is currently online.
    pub fn is_online(&self, user_id: &str) -> bool {
        self.online.contains(user_id)
    }

    /// Whether the conversation's other side is online (a direct conversation only).
    pub fn conversation_online(&self, c: &Conversation) -> bool {
        !c.group && c.other_id.as_deref().is_some_and(|id| self.is_online(id))
    }

    /// Opens conversation `index`: it is read (its unread count clears), the typing
    /// note of the previous one goes. `false` when it was already open or out of range.
    pub fn select(&mut self, index: usize) -> bool {
        if index >= self.conversations.len() || self.selected == Some(index) {
            return false;
        }
        self.selected = Some(index);
        self.typing = None;
        if let Some(c) = self.conversations.get_mut(index) {
            c.unread = 0;
        }
        true
    }

    /// Selects the conversation of id `id`, if the list holds it.
    pub fn select_id(&mut self, id: &str) -> bool {
        if id.is_empty() {
            return false;
        }
        match self.conversations.iter().position(|c| c.id == id) {
            Some(i) => {
                self.select(i);
                true
            }
            None => false,
        }
    }

    /// Replaces the list with the server's conversations, keeping the selection
    /// on the same conversation id where possible.
    pub fn apply_conversations(&mut self, convs: Vec<Conversation>) {
        if convs.is_empty() {
            return;
        }
        let keep = self.active().map(|c| c.id.clone());
        self.conversations = convs;
        self.live = true;
        let at = keep.and_then(|id| self.conversations.iter().position(|c| c.id == id)).unwrap_or(0);
        self.selected = Some(at.min(self.conversations.len() - 1));
    }

    /// Fills one conversation's thread once its messages have been fetched.
    pub fn apply_messages(&mut self, conv_id: &str, messages: Vec<Message>) {
        if let Some(c) = self.conversations.iter_mut().find(|c| c.id == conv_id) {
            c.messages = messages;
            c.loaded = true;
        }
    }

    /// A message that arrived live: it refreshes the list preview and, if the
    /// conversation is loaded, appends the bubble — dropping our own echo (the
    /// WebSocket also delivers what we just sent) by id or by optimistic match.
    /// A message for a conversation that is not open counts as unread.
    pub fn apply_incoming(&mut self, conv_id: &str, message: Message, you_prefix: &str) {
        let open = self.active().is_some_and(|c| c.id == conv_id);
        let Some(conv) = self.conversations.iter_mut().find(|c| c.id == conv_id) else { return };
        conv.snippet = if message.mine { format!("{you_prefix}{}", message.text) } else { message.text.clone() };
        if !message.time.is_empty() {
            conv.time = message.time.clone();
        }
        // Already held (its own send round-trip, or a re-delivery): nothing more.
        if !message.id.is_empty() && conv.messages.iter().any(|m| m.id == message.id) {
            return;
        }
        if !open && !message.mine {
            conv.unread += 1;
        }
        if !conv.loaded {
            return;
        }
        // Confirm a pending optimistic copy in place, else append.
        let pending = if message.mine { conv.messages.iter().position(|m| m.id.is_empty() && m.mine && m.text == message.text) } else { None };
        match pending {
            Some(p) => conv.messages[p] = message,
            None => conv.messages.push(message),
        }
    }

    /// A user's presence changed.
    pub fn apply_presence(&mut self, user_id: String, online: bool) {
        if online {
            self.online.insert(user_id);
        } else {
            self.online.remove(&user_id);
        }
    }

    /// Someone started (or stopped) typing in a conversation.
    pub fn apply_typing(&mut self, conv_id: String, on: bool, now: Instant) {
        self.typing = on.then(|| (conv_id, now + TYPING_TTL));
    }

    /// Clears an expired typing note; `true` when it did (the window repaints).
    pub fn expire_typing(&mut self, now: Instant) -> bool {
        if self.typing.as_ref().is_some_and(|(_, until)| *until <= now) {
            self.typing = None;
            return true;
        }
        false
    }

    /// Appends our own message to the open conversation (the optimistic bubble a send
    /// shows at once). Returns the conversation's id (empty for the sample) when one
    /// is open and `text` is not blank.
    pub fn push_own(&mut self, text: &str, day: &str, time: &str, you_prefix: &str) -> Option<String> {
        let text = text.trim();
        if text.is_empty() {
            return None;
        }
        let conv = self.selected.and_then(|i| self.conversations.get_mut(i))?;
        // Answering a conversation reads it.
        conv.unread = 0;
        conv.messages.push(Message { id: String::new(), text: text.to_string(), time: time.to_string(), day: day.to_string(), mine: true, status: Status::Sent });
        conv.snippet = format!("{you_prefix}{text}");
        if !time.is_empty() {
            conv.time = time.to_string();
        }
        Some(conv.id.clone())
    }

    /// Marks the open conversation read (the unread count), returning its id and newest message id.
    pub fn mark_active_read(&mut self) -> Option<(String, String)> {
        let conv = self.selected.and_then(|i| self.conversations.get_mut(i))?;
        conv.unread = 0;
        let last = conv.messages.iter().rev().find(|m| !m.id.is_empty()).map(|m| m.id.clone())?;
        Some((conv.id.clone(), last))
    }
}

/// The fixed offline sample: the list and the open thread before the server
/// answers, and the whole data with `--sample`.
pub fn sample() -> Vec<Conversation> {
    use Status::*;
    let today = "Aujourd'hui";
    vec![
        Conversation {
            name: "Amélie Rousseau".into(),
            snippet: "Parfait, on se voit demain alors 👍".into(),
            time: "14:32".into(),
            unread: 2,
            loaded: true,
            messages: vec![
                Message::other("Salut ! Tu es dispo demain pour la revue de design ?", today, "14:20"),
                Message::own("Oui, plutôt en début d'après-midi.", today, "14:22", Read),
                Message::other("14h ça te va ? Je réserve la petite salle.", today, "14:25"),
                Message::own("Nickel, 14h c'est parfait pour moi.", today, "14:28", Read),
                Message::other("Parfait, on se voit demain alors 👍", today, "14:32"),
            ],
            ..Default::default()
        },
        Conversation {
            name: "Équipe Produit".into(),
            snippet: "Martin : j'ai poussé le correctif".into(),
            time: "13:58".into(),
            group: true,
            loaded: true,
            messages: vec![
                Message::other("On garde le point de synchro à 10h ?", today, "13:40"),
                Message::own("Oui, je prépare l'ordre du jour.", today, "13:45", Delivered),
                Message::other("Martin : j'ai poussé le correctif", today, "13:58"),
            ],
            ..Default::default()
        },
        Conversation {
            name: "Karim Benali".into(),
            snippet: "Vous : merci beaucoup !".into(),
            time: "Hier".into(),
            loaded: true,
            messages: vec![
                Message::other("Voilà le document dont on parlait.", "Hier", "17:42"),
                Message::own("Reçu, merci beaucoup !", "Hier", "18:05", Read),
            ],
            ..Default::default()
        },
        Conversation {
            name: "Support Kubuno".into(),
            snippet: "Votre ticket a été résolu.".into(),
            time: "Lun".into(),
            loaded: true,
            messages: vec![Message::other("Votre ticket a été résolu.", "lun. 28/09", "09:15")],
            ..Default::default()
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn live(id: &str, name: &str) -> Conversation {
        Conversation { id: id.into(), name: name.into(), loaded: true, ..Default::default() }
    }

    #[test]
    fn selecting_reads_the_conversation_and_drops_the_typing_note() {
        let mut s = ChatState::new();
        s.typing = Some((String::new(), Instant::now() + TYPING_TTL));
        assert_eq!(s.conversations[0].unread, 2);
        assert!(!s.select(0), "already open");
        assert!(s.select(1));
        assert_eq!(s.selected, Some(1));
        assert!(s.typing.is_none());
        assert!(!s.select(99));
    }

    #[test]
    fn the_server_list_keeps_the_open_conversation() {
        let mut s = ChatState::with(vec![live("a", "A"), live("b", "B")]);
        s.select(1);
        s.apply_conversations(vec![live("c", "C"), live("b", "B"), live("a", "A")]);
        assert_eq!(s.active().map(|c| c.id.as_str()), Some("b"));
        assert!(s.live);
        s.apply_conversations(Vec::new());
        assert_eq!(s.conversations.len(), 3, "an empty answer keeps the list");
    }

    #[test]
    fn an_incoming_message_appends_or_confirms_and_counts_unread_elsewhere() {
        let mut s = ChatState::with(vec![live("a", "A"), live("b", "B")]);
        s.push_own("salut", "Aujourd'hui", "10:00", "Vous : ");
        assert_eq!(s.conversations[0].messages[0].status, Status::Sent);
        // The echo of our own send replaces the optimistic copy.
        let echo = Message { id: "m1".into(), text: "salut".into(), time: "10:00".into(), day: String::new(), mine: true, status: Status::Delivered };
        s.apply_incoming("a", echo.clone(), "Vous : ");
        assert_eq!(s.conversations[0].messages.len(), 1);
        assert_eq!(s.conversations[0].messages[0].id, "m1");
        // A re-delivery is dropped.
        s.apply_incoming("a", echo, "Vous : ");
        assert_eq!(s.conversations[0].messages.len(), 1);
        // Someone else's message in a closed conversation: unread, snippet updated.
        let other = Message { id: "m2".into(), text: "coucou".into(), time: "10:01".into(), day: String::new(), mine: false, status: Status::None };
        s.apply_incoming("b", other, "Vous : ");
        assert_eq!(s.conversations[1].unread, 1);
        assert_eq!(s.conversations[1].snippet, "coucou");
        assert_eq!(s.conversations[1].time, "10:01");
    }

    #[test]
    fn typing_expires() {
        let mut s = ChatState::with(vec![live("a", "A")]);
        let t0 = Instant::now();
        s.apply_typing("a".into(), true, t0);
        assert!(s.typing_in("a", t0));
        assert!(!s.expire_typing(t0));
        assert!(s.expire_typing(t0 + TYPING_TTL));
        assert!(!s.typing_in("a", t0 + TYPING_TTL));
    }

    #[test]
    fn a_blank_message_is_not_sent_and_presence_is_tracked() {
        let mut s = ChatState::with(vec![live("a", "A")]);
        assert_eq!(s.push_own("   ", "", "", ""), None);
        s.apply_presence("u1".into(), true);
        let mut direct = live("x", "X");
        direct.other_id = Some("u1".into());
        assert!(s.conversation_online(&direct));
        s.apply_presence("u1".into(), false);
        assert!(!s.conversation_online(&direct));
        assert_eq!(Section::from_key("meetings"), Section::Meetings);
        assert_eq!(Section::from_key(Section::Chats.key()), Section::Chats);
    }
}
