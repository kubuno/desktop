//! The chat's talk to the server, through the core gateway.
//!
//! Every call goes through `kubuno_sync` as the account the shell shows: the
//! access token is borrowed from the shell's token broker (the shell is the only
//! refresh-token owner; it is started in the background when it is not running,
//! and the broker's server is verified to be the installed shell). The chat
//! never sees a refresh token. The core proxies `/api/v1/chat/*` to the chat module and injects
//! the module's internal auth. Shapes are navigated defensively (like the
//! launcher does with `/api/v1/modules`) so a field the server adds later cannot
//! crash the client. The message envelope is the web's base64url JSON
//! `{text, media?, …}` (see `chat/frontend/src/chatStore.ts`).
//!
//! The background threads never touch the UI: they hand their results to a
//! [`Sink`], which the window turns into a `UiDispatcher::begin_invoke` (Windows
//! Forms' `BeginInvoke`) — the closure runs on the UI thread at the next frame.

use base64::Engine;
use kubuno::tracing;
use serde_json::Value;

use crate::model::{Conversation, Message, Status};
use crate::Resources;

/// Where a background API thread hands its results: the window posts them to its UI
/// thread, a test collects them.
pub type Sink = std::sync::Arc<dyn Fn(ChatEvent) + Send + Sync>;

/// What a background API thread hands back to the UI thread (through a [`Sink`]).
#[derive(Debug)]
pub enum ChatEvent {
    /// The instance resolved, our id, and the conversation list — the first
    /// load. `me` may be `None` if the identity call failed but the list came.
    Ready { instance: String, me: Option<String>, conversations: Vec<Conversation> },
    /// One conversation's messages, after it was opened.
    Messages { conv_id: String, messages: Vec<Message> },
    /// A single message that arrived live over the WebSocket.
    Incoming { conv_id: String, message: Message },
    /// A user's presence changed (`presence_update`).
    Presence { user_id: String, online: bool },
    /// Someone started or stopped typing in a conversation.
    Typing { conv_id: String, user_id: String, on: bool },
    /// A failure to surface, e.g. no account or the server unreachable.
    Error(String),
}

/// Resolves the account, our id and the conversation list off the UI thread,
/// then posts the result back. Called once at startup.
pub fn start_initial_load(sink: Sink) {
    std::thread::spawn(move || {
        let instance = match active_instance() {
            Ok(Some(key)) => key,
            Ok(None) => {
                sink(ChatEvent::Error("Aucun compte connecté : connectez-vous dans Kubuno Desktop.".into()));
                return;
            }
            Err(e) => {
                sink(ChatEvent::Error(format!("Compte : {e}")));
                return;
            }
        };
        let me = me(&instance);
        match fetch_conversations(&instance, me.as_deref()) {
            Ok(conversations) => sink(ChatEvent::Ready { instance, me, conversations }),
            Err(e) => sink(ChatEvent::Error(format!("Discussions : {e}"))),
        }
    });
}

/// Fetches one conversation's history off the UI thread, then posts it back.
pub fn start_messages_load(sink: Sink, instance: String, conv_id: String, me: Option<String>) {
    std::thread::spawn(move || match fetch_messages(&instance, &conv_id, me.as_deref()) {
        Ok(messages) => {
            // Opening a conversation reads it up to its newest message.
            if let Some(last) = messages.last().filter(|m| !m.id.is_empty()) {
                let _ = mark_read(&instance, &conv_id, &last.id);
            }
            sink(ChatEvent::Messages { conv_id, messages });
        }
        Err(e) => sink(ChatEvent::Error(format!("Messages : {e}"))),
    });
}

/// Marks a conversation read up to a message off the UI thread (best-effort).
pub fn start_mark_read(instance: String, conv_id: String, up_to_message_id: String) {
    std::thread::spawn(move || {
        if let Err(e) = mark_read(&instance, &conv_id, &up_to_message_id) {
            tracing::warn!("read receipt of {conv_id}: {e}");
        }
    });
}

/// Marks a conversation read up to a message (`POST …/read`). Best-effort — a
/// failed read receipt must never break opening the conversation.
pub fn mark_read(id: &str, conv_id: &str, up_to_message_id: &str) -> anyhow::Result<()> {
    let path = format!("/api/v1/chat/conversations/{conv_id}/read");
    kubuno_sync::account_post_json(&account(id)?, &path, serde_json::json!({ "up_to_message_id": up_to_message_id }))?;
    Ok(())
}

/// Sends a text message off the UI thread, then reloads the thread so the sent
/// bubble (and anything that arrived meanwhile) shows.
pub fn start_send(sink: Sink, instance: String, conv_id: String, text: String, me: Option<String>) {
    std::thread::spawn(move || {
        if let Err(e) = send_text(&instance, &conv_id, &text, me.as_deref()) {
            sink(ChatEvent::Error(format!("Envoi : {e}")));
            return;
        }
        if let Ok(messages) = fetch_messages(&instance, &conv_id, me.as_deref()) {
            sink(ChatEvent::Messages { conv_id, messages });
        }
    });
}

/// Opens the chat WebSocket and posts each incoming event to the UI thread.
/// Its own thread, reconnecting with back-off on any socket error — the same
/// shape as the sync engine's live channel (`kubuno-sync`'s `ws`).
pub fn start_realtime(sink: Sink, instance: String, me: Option<String>) {
    std::thread::spawn(move || {
        let mut backoff = std::time::Duration::from_secs(2);
        loop {
            match run_ws(&sink, &instance, me.as_deref()) {
                // A clean close still reconnects — the server may cycle.
                Ok(()) => backoff = std::time::Duration::from_secs(2),
                Err(e) => tracing::warn!("[chat/ws] {e}"),
            }
            std::thread::sleep(backoff);
            backoff = (backoff * 2).min(std::time::Duration::from_secs(30));
        }
    });
}

fn run_ws(sink: &Sink, instance: &str, me: Option<&str>) -> anyhow::Result<()> {
    let account = account(instance)?;
    // A current access token borrowed from the shell's broker: the WS URL carries it and the gateway converts
    // it to the module's internal auth.
    let token = kubuno_sync::account_access_token(&account)?;
    let (mut socket, _resp) = tungstenite::connect(ws_url(&account.server_url, &token))?;
    loop {
        match socket.read()? {
            tungstenite::Message::Text(t) => {
                if let Some(event) = parse_event(&t, me) {
                    sink(event);
                }
            }
            tungstenite::Message::Ping(p) => {
                let _ = socket.send(tungstenite::Message::Pong(p));
            }
            tungstenite::Message::Close(_) => break,
            _ => {}
        }
    }
    Ok(())
}

/// The UI event a WS envelope (`{event, payload}`) carries, if the desktop shows it:
/// new messages, presence and typing; other events (reactions, polls, calls) are
/// ignored for now.
pub fn parse_event(text: &str, me: Option<&str>) -> Option<ChatEvent> {
    let env = serde_json::from_str::<Value>(text).ok()?;
    let payload = env.get("payload");
    let s = |key: &str| payload.and_then(|p| p.get(key)).and_then(|x| x.as_str());
    match env.get("event").and_then(|x| x.as_str()) {
        Some("new_message") => {
            let msg = payload.and_then(|p| p.get("message"))?;
            let conv_id = msg.get("conversation_id").and_then(|x| x.as_str())?;
            Some(ChatEvent::Incoming { conv_id: conv_id.to_string(), message: message_from(msg, me)? })
        }
        Some("presence_update") => {
            let (user_id, status) = (s("user_id")?, s("status")?);
            Some(ChatEvent::Presence { user_id: user_id.to_string(), online: status != "offline" })
        }
        Some(ev @ ("typing_start" | "typing_stop")) => {
            let (conv_id, user_id) = (s("conversation_id")?, s("user_id")?);
            // Our own typing echo is not shown.
            (me != Some(user_id)).then(|| ChatEvent::Typing { conv_id: conv_id.to_string(), user_id: user_id.to_string(), on: ev == "typing_start" })
        }
        _ => None,
    }
}

/// Maps the HTTP base to the chat WebSocket URL, carrying the auth token.
fn ws_url(server_url: &str, token: &str) -> String {
    let base = server_url.trim_end_matches('/');
    let ws = if let Some(rest) = base.strip_prefix("https://") {
        format!("wss://{rest}")
    } else if let Some(rest) = base.strip_prefix("http://") {
        format!("ws://{rest}")
    } else {
        format!("wss://{base}")
    };
    format!("{ws}/api/v1/chat/ws?token={token}")
}

/// The account the chat was last resolved for (its key, server and user id: no secret).
static ACCOUNT: std::sync::Mutex<Option<kubuno_sync::AccountRef>> = std::sync::Mutex::new(None);

/// The account the chat runs as: the one the shell shows (its current account, else
/// the first active one), asked of the shell's broker. Returns its key, which the
/// window keeps as its `instance`.
pub fn active_instance() -> anyhow::Result<Option<String>> {
    let Some(account) = kubuno_sync::current_account()? else { return Ok(None) };
    let key = account.key.clone();
    *ACCOUNT.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(account);
    Ok(Some(key))
}

/// The account of key `key` (cached, else asked of the broker again).
fn account(key: &str) -> anyhow::Result<kubuno_sync::AccountRef> {
    if let Some(a) = ACCOUNT.lock().unwrap_or_else(std::sync::PoisonError::into_inner).as_ref().filter(|a| a.key == key) {
        return Ok(a.clone());
    }
    kubuno_sync::tokens::accounts()?.into_iter().find(|a| a.key == key).ok_or_else(|| anyhow::anyhow!("ce compte n'est plus connecté"))
}

/// The server the account talks to, for the profile menu.
pub fn server_of(instance: &str) -> Option<String> {
    account(instance).ok().map(|a| a.server_url)
}

/// Our own user id, so a message's `sender_id` distinguishes own from other: the
/// account's user id, known without a request.
pub fn me(id: &str) -> Option<String> {
    account(id).ok().map(|a| a.user_id)
}

/// The conversation list: `GET /api/v1/chat/conversations`. `me` prefixes our
/// own last message with "Vous : " in the snippet, as the web does.
pub fn fetch_conversations(id: &str, me: Option<&str>) -> anyhow::Result<Vec<Conversation>> {
    let v = kubuno_sync::account_get_json(&account(id)?, "/api/v1/chat/conversations")?;
    let rows = v.get("conversations").and_then(|x| x.as_array()).or_else(|| v.as_array()).cloned().unwrap_or_default();
    let today = days_from_civil_now();
    Ok(rows.iter().filter_map(|r| summary_to_conv(r, me, today)).collect())
}

/// One conversation's history: `GET …/messages?limit=50`. The endpoint returns
/// newest-first; we reverse to chronological for top-to-bottom display.
pub fn fetch_messages(id: &str, conv_id: &str, me: Option<&str>) -> anyhow::Result<Vec<Message>> {
    let path = format!("/api/v1/chat/conversations/{conv_id}/messages?limit=50");
    let v = kubuno_sync::account_get_json(&account(id)?, &path)?;
    let mut rows = v.get("messages").and_then(|x| x.as_array()).or_else(|| v.as_array()).cloned().unwrap_or_default();
    rows.reverse();
    Ok(rows.iter().filter_map(|m| message_from(m, me)).collect())
}

/// Sends a text message: `POST …/messages` with the base64url envelope and a
/// random idempotency `nonce`, as the web's `sendMessage` does. Returns the
/// stored message mapped for immediate display.
pub fn send_text(id: &str, conv_id: &str, text: &str, me: Option<&str>) -> anyhow::Result<Message> {
    let body = serde_json::json!({
        "encrypted_data": encode_envelope(text),
        "nonce": nonce(),
        "message_type": "text",
    });
    let path = format!("/api/v1/chat/conversations/{conv_id}/messages");
    let v = kubuno_sync::account_post_json(&account(id)?, &path, body)?;
    // The server echoes the stored message (possibly wrapped); fall back to a
    // local echo so the bubble appears even if the reply shape surprises us.
    let stored = v.get("message").unwrap_or(&v);
    Ok(message_from(stored, me).unwrap_or(Message { id: String::new(), text: text.to_string(), time: String::new(), day: String::new(), mine: true, status: Status::Sent }))
}

fn summary_to_conv(r: &Value, me: Option<&str>, today: i64) -> Option<Conversation> {
    let conv = r.get("conversation").unwrap_or(r);
    let id = conv.get("id").and_then(|x| x.as_str())?.to_string();
    let conv_type = conv.get("conv_type").and_then(|x| x.as_str()).unwrap_or("direct");
    let other = r.get("other_user");
    let name = conv
        .get("name")
        .and_then(|x| x.as_str())
        .filter(|s| !s.trim().is_empty())
        .map(str::to_string)
        .or_else(|| other.and_then(|o| o.get("display_name")).and_then(|x| x.as_str()).map(str::to_string))
        .or_else(|| other.and_then(|o| o.get("username")).and_then(|x| x.as_str()).map(str::to_string))
        .unwrap_or_else(|| "Sans nom".into());

    let last = r.get("last_message");
    let snippet = last
        .map(|m| {
            let mine = me.is_some() && m.get("sender_id").and_then(|x| x.as_str()) == me;
            let body = preview(m);
            if mine { format!("{}{body}", Resources::you_prefix()) } else { body }
        })
        .unwrap_or_default();
    let ts = last.and_then(|m| m.get("created_at")).or_else(|| conv.get("updated_at")).and_then(|x| x.as_str()).unwrap_or("");

    Some(Conversation {
        id,
        name,
        snippet,
        time: list_time(ts, today),
        unread: r.get("unread_count").and_then(|x| x.as_u64()).unwrap_or(0) as u32,
        group: conv_type != "direct",
        other_id: other.and_then(|o| o.get("id")).and_then(|x| x.as_str()).map(str::to_string),
        meeting: conv.get("is_meeting").and_then(|x| x.as_bool()).unwrap_or(false),
        loaded: false,
        messages: Vec::new(),
    })
}

fn message_from(m: &Value, me: Option<&str>) -> Option<Message> {
    let mine = me.is_some() && m.get("sender_id").and_then(|x| x.as_str()) == me;
    let status = if !mine {
        Status::None
    } else {
        match m.get("status").and_then(|x| x.as_str()) {
            Some("read") => Status::Read,
            Some("delivered") => Status::Delivered,
            _ => Status::Sent,
        }
    };
    let ts = m.get("created_at").and_then(|x| x.as_str()).unwrap_or("");
    Some(Message {
        id: m.get("id").and_then(|x| x.as_str()).unwrap_or("").to_string(),
        text: preview(m),
        time: hhmm(ts),
        day: day_label(ts, days_from_civil_now()),
        mine,
        status,
    })
}

/// A one-line preview of a message: its text, or a placeholder for a deleted /
/// media message — the shape the web's `lastSnippet`/`MessageBubble` render.
fn preview(m: &Value) -> String {
    if m.get("deleted_at").map(|v| !v.is_null()).unwrap_or(false) {
        return "message supprimé".into();
    }
    let env = decode_envelope(m.get("encrypted_data").and_then(|x| x.as_str()).unwrap_or(""));
    if let Some(text) = env.get("text").and_then(|x| x.as_str()).filter(|s| !s.is_empty()) {
        return text.to_string();
    }
    if let Some(media) = env.get("media") {
        let kind = media.get("kind").and_then(|x| x.as_str()).unwrap_or("file");
        return match kind {
            "image" => "📷 Photo".into(),
            "video" => "🎬 Vidéo".into(),
            "audio" | "voice" => "🎤 Message vocal".into(),
            "gif" => "GIF".into(),
            "sticker" => "🏷️ Sticker".into(),
            _ => {
                let name = media.get("name").and_then(|x| x.as_str()).unwrap_or("fichier");
                format!("📎 {name}")
            }
        };
    }
    if env.get("poll").is_some() {
        return "📊 Sondage".into();
    }
    String::new()
}

/// Decodes the message envelope: base64url (padded or not) of a JSON object.
/// Returns `Null` if anything is off — a bad envelope must not crash a paint.
fn decode_envelope(data: &str) -> Value {
    if data.is_empty() {
        return Value::Null;
    }
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(data)
        .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(data))
        .or_else(|_| base64::engine::general_purpose::STANDARD.decode(data))
        .unwrap_or_default();
    serde_json::from_slice(&bytes).unwrap_or(Value::Null)
}

/// Encodes `{text}` as the base64url envelope the server stores.
fn encode_envelope(text: &str) -> String {
    let json = serde_json::json!({ "text": text }).to_string();
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(json.as_bytes())
}

/// A random idempotency key for a send. Time + a counter is enough here: the
/// server only needs it unique per client to dedupe a retried POST.
fn nonce() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    format!("kd-{t:x}-{n:x}")
}

/// The `HH:MM` a timestamp shows, pulled from an ISO-8601 string
/// (`2026-09-08T14:32:07Z`). Anything unparseable comes back blank rather than
/// wrong. Used inside a conversation, where date separators carry the day.
pub fn hhmm(ts: &str) -> String {
    if let Some((_, time)) = ts.split_once('T') {
        let hm: String = time.chars().take(5).collect();
        if hm.len() == 5 && hm.as_bytes()[2] == b':' {
            return hm;
        }
    }
    String::new()
}

/// The civil date of an ISO-8601 timestamp, in days since the epoch.
fn days_of(ts: &str) -> Option<(i64, i64, i64, i64)> {
    let (date, _) = ts.split_once('T')?;
    let mut it = date.split('-');
    let y = it.next()?.parse::<i64>().ok()?;
    let m = it.next()?.parse::<i64>().ok()?;
    let d = it.next()?.parse::<i64>().ok()?;
    Some((days_from_civil(y, m, d), y, m, d))
}

const WEEKDAYS: [&str; 7] = ["lun.", "mar.", "mer.", "jeu.", "ven.", "sam.", "dim."];

/// The weekday (0 = Monday) of a day count since the epoch (1970-01-01 was a Thursday).
fn weekday(days: i64) -> usize {
    (days.rem_euclid(7) as usize + 3) % 7
}

/// The relative label a LIST row shows for a timestamp: `HH:MM` today, "Hier"
/// yesterday, the weekday within the last week, else `DD/MM/YYYY` — the web's
/// `formatAge` shape. Computed from the civil date so it needs no date crate.
pub fn list_time(ts: &str, today: i64) -> String {
    let Some((msg_days, y, m, d)) = days_of(ts) else { return String::new() };
    match today - msg_days {
        0 => hhmm(ts),
        1 => Resources::yesterday().to_string(),
        2..=6 => WEEKDAYS[weekday(msg_days)].into(),
        _ => format!("{d:02}/{m:02}/{y}"),
    }
}

/// The date separator a message falls under: "Aujourd'hui", "Hier", the weekday
/// and date within the last week, else the full date.
pub fn day_label(ts: &str, today: i64) -> String {
    let Some((msg_days, y, m, d)) = days_of(ts) else { return String::new() };
    match today - msg_days {
        0 => Resources::today().to_string(),
        1 => Resources::yesterday().to_string(),
        2..=6 => format!("{} {d:02}/{m:02}", WEEKDAYS[weekday(msg_days)]),
        _ => format!("{d:02}/{m:02}/{y}"),
    }
}

/// Days since the Unix epoch for a civil date (Howard Hinnant's algorithm).
pub fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// Today, in days since the epoch (UTC — enough for a list label).
pub fn days_from_civil_now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| (d.as_secs() / 86400) as i64).unwrap_or(0)
}

/// The current wall-clock `HH:MM` (UTC, like the server's timestamps) — an optimistic
/// bubble's time until the server's copy replaces it.
pub fn now_hhmm() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    format!("{:02}:{:02}", (secs / 3600) % 24, (secs / 60) % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn envelope(text: &str) -> String {
        encode_envelope(text)
    }

    #[test]
    fn civil_dates_and_labels() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        let today = days_from_civil(2026, 10, 1);
        assert_eq!(weekday(today), 3, "2026-10-01 is a Thursday");
        kubuno::resources::set_culture("fr");
        assert_eq!(list_time("2026-10-01T14:32:07Z", today), "14:32");
        assert_eq!(list_time("2026-09-30T08:00:00Z", today), "Hier");
        assert_eq!(list_time("2026-09-28T08:00:00Z", today), "lun.");
        assert_eq!(list_time("2026-08-01T08:00:00Z", today), "01/08/2026");
        assert_eq!(list_time("garbage", today), "");
        assert_eq!(day_label("2026-10-01T14:32:07Z", today), "Aujourd'hui");
        assert_eq!(day_label("2026-09-28T08:00:00Z", today), "lun. 28/09");
        assert_eq!(hhmm("2026-10-01T09:05:00Z"), "09:05");
        assert_eq!(hhmm("2026-10-01"), "");
    }

    #[test]
    fn envelopes_round_trip_and_previews() {
        let m = serde_json::json!({ "id": "m1", "sender_id": "me", "status": "read", "created_at": "2026-10-01T10:00:00Z", "encrypted_data": envelope("bonjour") });
        let msg = message_from(&m, Some("me")).expect("message");
        assert_eq!(msg.text, "bonjour");
        assert!(msg.mine);
        assert_eq!(msg.status, Status::Read);
        assert_eq!(msg.time, "10:00");
        let deleted = serde_json::json!({ "deleted_at": "x", "encrypted_data": envelope("secret") });
        assert_eq!(preview(&deleted), "message supprimé");
        assert_eq!(decode_envelope("%%%"), Value::Null);
    }

    #[test]
    fn ws_events_are_parsed() {
        let msg = serde_json::json!({ "event": "new_message", "payload": { "message": { "id": "m", "conversation_id": "c", "sender_id": "u", "encrypted_data": envelope("hi") } } });
        match parse_event(&msg.to_string(), Some("me")) {
            Some(ChatEvent::Incoming { conv_id, message }) => {
                assert_eq!(conv_id, "c");
                assert_eq!(message.text, "hi");
                assert!(!message.mine);
            }
            other => panic!("unexpected {other:?}"),
        }
        let presence = r#"{"event":"presence_update","payload":{"user_id":"u","status":"offline"}}"#;
        assert!(matches!(parse_event(presence, None), Some(ChatEvent::Presence { online: false, .. })));
        let own_typing = r#"{"event":"typing_start","payload":{"conversation_id":"c","user_id":"me"}}"#;
        assert!(parse_event(own_typing, Some("me")).is_none(), "our own typing echo is dropped");
        assert!(parse_event("not json", None).is_none());
        assert_eq!(ws_url("https://k.example/", "t"), "wss://k.example/api/v1/chat/ws?token=t");
        assert_eq!(ws_url("http://localhost:8080", "t"), "ws://localhost:8080/api/v1/chat/ws?token=t");
    }
}
