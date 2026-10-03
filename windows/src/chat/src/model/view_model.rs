//! What the views show, computed from [`ChatState`]: the list's rows (filtered by the
//! search and the rail's section), the open conversation's header and thread. Pure
//! functions of the state (and the current culture's strings), unit-tested — the views
//! only bind to their results.

use std::time::Instant;

use kubuno::prelude::{Row, Rows, Value};

use crate::controls::message_thread::{matches, ThreadData};
use crate::model::{ChatState, Conversation, Section};
use crate::Resources;

/// The indices (into `state.conversations`) the list shows, in order: the section's
/// conversations whose name or snippet contains the search text.
pub fn visible_indices(state: &ChatState) -> Vec<usize> {
    state
        .conversations
        .iter()
        .enumerate()
        .filter(|(_, c)| state.section == Section::Chats || c.meeting)
        .filter(|(_, c)| matches(&c.name, &state.filter) || matches(&c.snippet, &state.filter))
        .map(|(i, _)| i)
        .collect()
}

/// The list's row of conversation `index` (the fields `ConversationRow`'s view binds).
pub fn conversation_row(state: &ChatState, index: usize, c: &Conversation) -> Row {
    let unread = c.unread > 0;
    let badge = if c.unread > 99 { "99+".to_string() } else { c.unread.to_string() };
    let key = if c.id.is_empty() { format!("sample-{index}") } else { c.id.clone() };
    Row::new()
        .with("Key", Value::Str(key))
        .with("Index", Value::F32(index as f32))
        .with("Name", Value::Str(c.name.clone()))
        .with("Initial", Value::Str(c.initial()))
        .with("Snippet", Value::Str(c.snippet.clone()))
        .with("Time", Value::Str(c.time.clone()))
        .with("Unread", Value::Str(badge))
        .with("HasUnread", Value::Bool(unread))
        .with("NameFont", Value::Str(if unread { "style=Bold".into() } else { String::new() }))
        .with("TimeColor", Value::Str(if unread { "Primary" } else { "TextTertiary" }.into()))
        .with("SnippetColor", Value::Str(if unread { "TextSecondary" } else { "TextTertiary" }.into()))
        .with("AvatarShape", Value::Str(if c.group { "Rounded" } else { "Circle" }.into()))
        .with("Presence", Value::Str(if state.conversation_online(c) { "Online" } else { "None" }.into()))
}

/// The list's rows, and the position of the open conversation among them (-1: not shown).
pub fn list_rows(state: &ChatState) -> (Rows, i32) {
    let visible = visible_indices(state);
    let rows: Rows = visible.iter().map(|&i| conversation_row(state, i, &state.conversations[i])).collect();
    let selected = state.selected.and_then(|s| visible.iter().position(|&i| i == s)).map_or(-1, |p| p as i32);
    (rows, selected)
}

/// The list's heading (the rail's section).
pub fn list_title(state: &ChatState) -> String {
    match state.section {
        Section::Chats => Resources::list_title().to_string(),
        Section::Meetings => Resources::meetings_title().to_string(),
    }
}

/// The text the list shows when it has no row.
pub fn list_empty_text(state: &ChatState) -> String {
    if state.filter.trim().is_empty() { Resources::list_empty().to_string() } else { Resources::list_no_match().to_string() }
}

/// The open conversation's header.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Header {
    pub title: String,
    pub initial: String,
    /// The avatar's `Shape`: `Circle` (a person) or `Rounded` (a group).
    pub shape: String,
    /// The avatar's `Presence`.
    pub presence: String,
    /// The line under the name: typing beats presence; a group shows nothing.
    pub status: String,
    /// Its colour, a theme token (bound to the label's `ForeColor`).
    pub status_color: String,
}

/// The header of the open conversation, `None` when none is open.
pub fn header(state: &ChatState, now: Instant) -> Option<Header> {
    let c = state.active()?;
    let online = state.conversation_online(c);
    let (status, color) = if state.typing_in(&c.id, now) {
        (Resources::status_typing(), "Primary")
    } else if c.meeting {
        (Resources::status_meeting(), "Primary")
    } else if c.group {
        ("", "TextTertiary")
    } else if online {
        (Resources::status_online(), "Success")
    } else {
        (Resources::status_offline(), "TextTertiary")
    };
    Some(Header {
        title: c.name.clone(),
        initial: c.initial(),
        shape: if c.group { "Rounded" } else { "Circle" }.into(),
        presence: if online { "Online" } else { "None" }.into(),
        status: status.to_string(),
        status_color: color.into(),
    })
}

/// The open conversation's thread (its key changes with the conversation).
pub fn thread(state: &ChatState, filter: &str) -> ThreadData {
    match (state.selected, state.active()) {
        (Some(i), Some(c)) => ThreadData {
            key: if c.id.is_empty() { format!("sample-{i}") } else { c.id.clone() },
            messages: c.messages.clone(),
            filter: filter.to_string(),
        },
        _ => ThreadData::default(),
    }
}

/// The open conversation's kind, for its details menu.
pub fn kind_label(c: &Conversation) -> &'static str {
    if c.meeting {
        Resources::kind_meeting()
    } else if c.group {
        Resources::kind_group()
    } else {
        Resources::kind_direct()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Conversation;

    fn state() -> ChatState {
        kubuno::resources::set_culture("fr");
        ChatState::new()
    }

    #[test]
    fn the_list_follows_the_search_and_the_section() {
        let mut s = state();
        assert_eq!(visible_indices(&s), vec![0, 1, 2, 3]);
        s.filter = "equipe".into();
        assert_eq!(visible_indices(&s), vec![1], "accent- and case-insensitive, on the name");
        s.filter = "correctif".into();
        assert_eq!(visible_indices(&s), vec![1], "and on the snippet");
        s.filter = "zzz".into();
        let (rows, selected) = list_rows(&s);
        assert!(rows.is_empty());
        assert_eq!(selected, -1, "the open conversation is filtered out");
        assert_eq!(list_empty_text(&s), "Aucune discussion ne correspond");
        s.filter.clear();
        s.section = Section::Meetings;
        assert!(visible_indices(&s).is_empty());
        s.conversations[3].meeting = true;
        assert_eq!(visible_indices(&s), vec![3]);
        assert_eq!(list_title(&s), "Réunions");
    }

    #[test]
    fn rows_carry_what_the_row_view_binds() {
        let mut s = state();
        s.select(1);
        let (rows, selected) = list_rows(&s);
        assert_eq!(selected, 1);
        let unread = &rows[0];
        // Amélie's 2 unread were not cleared: she is not the open one any more.
        assert_eq!(unread.text("Unread"), "2");
        assert_eq!(unread.get("HasUnread"), Some(&Value::Bool(true)));
        assert_eq!(unread.text("TimeColor"), "Primary");
        assert_eq!(unread.text("NameFont"), "style=Bold");
        assert_eq!(unread.text("Initial"), "A");
        assert_eq!(rows[1].text("AvatarShape"), "Rounded", "a group");
        assert_eq!(rows[1].text("TimeColor"), "TextTertiary");
        assert_eq!(rows[1].text("Key"), "sample-1");
        let mut many = Conversation { name: "x".into(), unread: 150, ..Default::default() };
        assert_eq!(conversation_row(&s, 9, &many).text("Unread"), "99+");
        many.other_id = Some("u".into());
        s.apply_presence("u".into(), true);
        assert_eq!(conversation_row(&s, 9, &many).text("Presence"), "Online");
    }

    #[test]
    fn the_header_status_follows_typing_meeting_group_and_presence() {
        let mut s = state();
        let now = Instant::now();
        let h = header(&s, now).expect("open");
        assert_eq!((h.title.as_str(), h.status.as_str(), h.status_color.as_str()), ("Amélie Rousseau", "hors ligne", "TextTertiary"));
        s.conversations[0].other_id = Some("amelie".into());
        s.apply_presence("amelie".into(), true);
        let h = header(&s, now).expect("open");
        assert_eq!((h.status.as_str(), h.status_color.as_str(), h.presence.as_str()), ("en ligne", "Success", "Online"));
        s.apply_typing(String::new(), true, now);
        assert_eq!(header(&s, now).map(|h| h.status), Some("en train d'écrire…".into()));
        s.select(1);
        let h = header(&s, now).expect("open");
        assert_eq!((h.status.as_str(), h.shape.as_str()), ("", "Rounded"));
        s.conversations[1].meeting = true;
        assert_eq!(header(&s, now).map(|h| h.status_color), Some("Primary".into()));
        assert_eq!(kind_label(&s.conversations[1]), "Réunion");
        s.selected = None;
        assert!(header(&s, now).is_none());
        assert_eq!(thread(&s, ""), ThreadData::default());
    }

    #[test]
    fn the_thread_is_keyed_by_conversation() {
        let mut s = state();
        let a = thread(&s, "");
        assert_eq!(a.key, "sample-0");
        assert_eq!(a.messages.len(), 5);
        s.select(2);
        let b = thread(&s, "merci");
        assert_eq!(b.key, "sample-2");
        assert_eq!(b.filter, "merci");
    }
}
