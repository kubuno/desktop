//! What the shell's views show, computed from its state — pure functions, unit-tested: the window
//! hands their results to its user controls (typed access) and to its own bindings.
//!
//! The state itself ([`ShellState`]) is what the engine, the background fetches and the user's
//! actions feed; nothing here touches Windows, the network or the disk.

use std::time::Instant;

use kubuno_desktop::prelude::{Row, Rows, Value};

use crate::services::apps::{AppEntry, Conn, Identity};
use crate::views::shell_window::Page;
use crate::Resources;

/// Everything the window shows, gathered from the engine and the fetches.
#[derive(Clone, Default)]
pub struct ShellState {
    pub page: Page,
    pub apps: Vec<AppEntry>,
    pub conn: Conn,
    /// The last sync summary, shown on the launcher.
    pub status: String,
    pub server: String,
    pub folder: String,
    pub syncing: bool,
    /// The forced offline mode, as the engine last said.
    pub offline: bool,
    /// The signed-in user (initials `?` until the identity arrives).
    pub identity: Identity,
    /// The configured instances, the current one, and who is signed in on each (fetched).
    pub accounts: Vec<AccountInfo>,
    /// The labels page.
    pub labels: Vec<kubuno_desktop_sync::Label>,
    pub labels_loading: bool,
    pub labels_error: String,
    /// The sign-in page, while it is up.
    pub login: Option<LoginState>,
    /// The rail shows icons only (a narrow window).
    pub compact: bool,
    /// How many activity events the bell counts.
    pub unread: usize,
}

/// One configured instance, as the accounts page and the account panel list it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AccountInfo {
    pub id: String,
    pub server_url: String,
    pub folder: String,
    pub label: Option<String>,
    pub active: bool,
    /// Who is signed in there, once fetched (empty offline).
    pub user: String,
}

/// The sign-in page's own state (the fields live in the page).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LoginState {
    pub busy: bool,
    /// Why the last attempt failed.
    pub error: String,
    /// Opened to add an account to an existing set: « Annuler » goes back.
    pub cancellable: bool,
    /// The server asked for the two-factor code: the session of that second step.
    pub totp_session: Option<String>,
}

impl ShellState {
    /// The state the designer shows: the sample account, online.
    pub fn design() -> Self {
        Self {
            conn: Conn::Online,
            server: "https://cloud.exemple.fr".into(),
            folder: r"C:\Users\Camille\Kubuno".into(),
            identity: Identity {
                initials: "CM".into(),
                name: "Camille Martin".into(),
                email: "camille.martin@exemple.fr".into(),
                used: 3_500_000_000,
                quota: 16_000_000_000,
                is_admin: true,
                ..Identity::default()
            },
            ..Self::default()
        }
    }
}

/// The host part of a server URL (`https://cloud.exemple.fr/` → `cloud.exemple.fr`).
pub fn host_of(server_url: &str) -> String {
    server_url.split("://").last().unwrap_or(server_url).trim_end_matches('/').to_string()
}

/// Base 1024, one decimal for kB/MB and TWO for GB — `format.ts` verbatim, so the desktop reads
/// exactly like the web; the units are the culture's (« Ko », « Mo », « Go » in French).
pub fn format_size(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = 1_048_576.0;
    const GB: f64 = 1_073_741_824.0;
    let b = bytes as f64;
    if b < KB {
        format!("{bytes} {}", Resources::unit_bytes())
    } else if b < MB {
        format!("{:.1} {}", b / KB, Resources::unit_kb())
    } else if b < GB {
        format!("{:.1} {}", b / MB, Resources::unit_mb())
    } else {
        format!("{:.2} {}", b / GB, Resources::unit_gb())
    }
}

/// The bell's counter: the number, capped at « 9+ » like the web.
pub fn unread_text(unread: usize) -> String {
    if unread > 9 {
        "9+".to_string()
    } else {
        unread.to_string()
    }
}

// ── The rail ─────────────────────────────────────────────────────────────────────────────────
pub const RAIL_ROW_INDENT: f32 = 28.0;
/// The rail's widths, expanded and icons-only, and the window width under which it collapses
/// (chosen so the default window keeps its labels: at 175 % a 1100 px window is 629 DIP wide).
pub const RAIL_WIDE: f32 = 196.0;
pub const RAIL_NARROW: f32 = 64.0;
pub const RAIL_COLLAPSE_BELOW: f32 = 520.0;

/// The pages the rail links to, in order: (page, key, icon).
pub const NAV: [(Page, &str, &str); 4] =
    [(Page::Launcher, "launcher", "Home"), (Page::Activity, "activity", "History"), (Page::Accounts, "accounts", "Users"), (Page::Settings, "settings", "Settings")];



/// Where every rail row's icon starts: past the console's chevron column, so the pages and the
/// console's groups line up.
pub const RAIL_INDENT: f32 = 28.0;

/// The rail's rows (`Sidebar.ItemsSource`): the pages, then — while the console is open, `admin`
/// holding its expanded groups — its tree under a header.
pub fn nav_rows(admin: Option<&[String]>) -> Rows {
    let text = |key: &str| match key {
        "launcher" => Resources::nav_home(),
        "activity" => Resources::nav_activity(),
        "accounts" => Resources::nav_accounts(),
        _ => Resources::nav_settings(),
    };
    let mut rows: Vec<Row> = NAV
        .iter()
        .map(|(_, key, icon)| {
            Row::new()
                .with("Key", Value::Str((*key).into()))
                .with("Text", Value::Str(text(key).into()))
                .with("Icon", Value::Str((*icon).into()))
                .with("Indent", Value::F32(RAIL_INDENT))
        })
        .collect();
    if let Some(expanded) = admin {
        rows.extend(crate::admin::rail_rows(expanded, RAIL_INDENT));
    }
    Rows::from(rows)
}

/// The rail's key for a page (`Sidebar.SelectedItem`); a page without a rail row selects none.
pub fn nav_key(page: Page) -> &'static str {
    NAV.iter().find(|(p, _, _)| *p == page).map(|(_, k, _)| *k).unwrap_or("")
}

/// The page a rail key names.
pub fn page_of_key(key: &str) -> Option<Page> {
    NAV.iter().find(|(_, k, _)| *k == key).map(|(p, _, _)| *p)
}

/// The rail collapses to its icons below [`RAIL_COLLAPSE_BELOW`] DIP.
pub fn rail_compact(window_width: f32) -> bool {
    window_width < RAIL_COLLAPSE_BELOW
}

/// The connection row at the foot of the rail: its dot's colour (a theme token) and its word.
pub fn connection(conn: Conn) -> (&'static str, &'static str) {
    match conn {
        Conn::Online => ("Success", Resources::conn_online()),
        Conn::Offline => ("TextTertiary", Resources::conn_offline()),
        Conn::Expired => ("Danger", Resources::conn_expired()),
    }
}

// ── The launcher ─────────────────────────────────────────────────────────────────────────────

/// What the launcher shows.
#[derive(Debug, Clone, PartialEq)]
pub struct Launcher {
    pub sync_tone: &'static str,
    pub sync_state: String,
    pub sync_detail: String,
    pub sync_button: String,
    pub syncing: bool,
    pub server_tone: &'static str,
    pub server_text: String,
    pub offline: bool,
    pub online_label: String,
    pub online_description: String,
}

pub fn launcher(s: &ShellState) -> Launcher {
    let (sync_tone, sync_state) = match (s.syncing, s.conn) {
        (true, _) => ("Primary", Resources::sync_busy()),
        (_, Conn::Online) => ("Success", Resources::sync_up_to_date()),
        (_, Conn::Expired) => ("Danger", Resources::session_expired()),
        (_, Conn::Offline) => ("Warning", Resources::offline()),
    };
    // The sub-line: the last summary, else where the files live.
    let sync_detail = if !s.status.is_empty() {
        s.status.clone()
    } else if !s.folder.is_empty() {
        s.folder.clone()
    } else {
        Resources::activity_placeholder().to_string()
    };
    let (server_tone, server_text) = match s.conn {
        Conn::Online => ("Success", s.server.clone()),
        Conn::Offline => ("TextTertiary", Resources::server_unreachable().to_string()),
        Conn::Expired => ("Danger", Resources::session_expired_reconnect().to_string()),
    };
    let (online_label, online_description) = if s.offline {
        (Resources::offline_label(), Resources::offline_description())
    } else {
        (Resources::online_label(), Resources::online_description())
    };
    Launcher {
        sync_tone,
        sync_state: sync_state.to_string(),
        sync_detail,
        sync_button: if s.syncing { Resources::sync_running_button() } else { Resources::sync_now_button() }.to_string(),
        syncing: s.syncing,
        server_tone,
        server_text,
        offline: s.offline,
        online_label: online_label.to_string(),
        online_description: online_description.to_string(),
    }
}

/// The summary of a manual sync cycle.
pub fn sync_summary(s: &kubuno_desktop_sync::Summary) -> String {
    Resources::sync_summary()
        .replace("{0}", &s.uploaded.to_string())
        .replace("{1}", &s.modified.to_string())
        .replace("{2}", &s.downloaded.to_string())
        .replace("{3}", &s.deleted_down.to_string())
}

// ── Accounts ─────────────────────────────────────────────────────────────────────────────────

/// The accounts page's rows: the radio, who and where, the folder.
pub fn account_rows(accounts: &[AccountInfo]) -> Rows {
    let active = accounts.iter().find(|a| a.active).map(|a| a.id.clone()).unwrap_or_default();
    accounts
        .iter()
        .map(|a| {
            let host = host_of(&a.server_url);
            let name = match &a.label {
                Some(l) if !l.trim().is_empty() => format!("{l} — {host}"),
                _ => host,
            };
            // The person first, the server after — the identity is what tells two accounts on the
            // same server apart.
            let title = if a.user.is_empty() { name } else { format!("{} — {name}", a.user) };
            Row::new()
                .with("Id", Value::Str(a.id.clone()))
                .with("ActiveId", Value::Str(active.clone()))
                .with("Title", Value::Str(title))
                .with("Folder", Value::Str(a.folder.clone()))
                .with("Active", Value::Bool(a.active))
        })
        .collect()
}

/// What a confirmation names an account by: its label, else its server's host.
pub fn account_name(a: &AccountInfo) -> String {
    a.label.clone().filter(|l| !l.trim().is_empty()).unwrap_or_else(|| host_of(&a.server_url))
}

/// The account panel's other accounts: every instance but the current one, by its name, over its
/// server (where the web shows the address).
pub fn other_accounts(accounts: &[AccountInfo]) -> Vec<kubuno_desktop_shell_controls::AccountEntry> {
    accounts
        .iter()
        .filter(|a| !a.active)
        .map(|a| kubuno_desktop_shell_controls::AccountEntry {
            id: a.id.clone(),
            name: account_name(a),
            email: String::new(),
            server: host_of(&a.server_url),
            initials: None,
            avatar: None,
            connected: true,
            remote: false,
            unread: 0,
        })
        .collect()
}

/// The account panel's data (`kubuno_desktop_shell_controls::AccountMenu`, in the header's avatar flyout): the
/// signed-in user, the other accounts and whether the console is listed, as the window holds them
/// when the panel opens.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AccountPanel {
    pub user: kubuno_desktop_shell_controls::AccountUser,
    pub others: Vec<kubuno_desktop_shell_controls::AccountEntry>,
    pub admin: bool,
}

impl kubuno_desktop_shell_controls::AccountService for AccountPanel {
    fn user(&self) -> kubuno_desktop_shell_controls::AccountUser {
        self.user.clone()
    }

    fn accounts(&self) -> Vec<kubuno_desktop_shell_controls::AccountEntry> {
        self.others.clone()
    }

    fn can_administer(&self) -> bool {
        self.admin
    }
}

// ── Activity ─────────────────────────────────────────────────────────────────────────────────

/// The activity page's rows, newest first: the glyph's colour, the title, the body, the age.
pub fn activity_rows(events: &[crate::services::activity::Event], now: Instant) -> Rows {
    events
        .iter()
        .map(|e| {
            // The kind carries the meaning, so it carries the colour.
            let tone = match e.kind.as_str() {
                "error" => "Danger",
                "conflict" => "Warning",
                _ => "Success",
            };
            Row::new()
                .with("Tone", Value::Str(tone.to_string()))
                .with("Title", Value::Str(e.title.clone()))
                .with("Body", Value::Str(e.body.clone()))
                .with("Age", Value::Str(crate::services::activity::age(now.saturating_duration_since(e.at))))
        })
        .collect()
}

// ── Labels ───────────────────────────────────────────────────────────────────────────────────

/// The palette the web offers, in its order — a new label takes the next one.
pub const PALETTE: [&str; 8] = ["#1e8e3e", "#1a73e8", "#9334e6", "#ec4899", "#d93025", "#f59e0b", "#14b8a6", "#5f6368"];

/// The labels page's rows.
pub fn label_rows(labels: &[kubuno_desktop_sync::Label]) -> Rows {
    labels
        .iter()
        .map(|l| {
            // Shared BY someone else: name them, so a homonym stays legible.
            let shared = match (l.is_owner, l.owner_name.as_deref()) {
                (false, Some(owner)) => Resources::shared_by().replace("{0}", owner),
                _ => String::new(),
            };
            Row::new()
                .with("Id", Value::Str(l.id.clone()))
                .with("Name", Value::Str(l.name.clone()))
                .with("Color", Value::Str(l.color.clone().unwrap_or_else(|| "TextSecondary".to_string())))
                .with("Count", Value::Str(l.link_count.to_string()))
                .with("CanManage", Value::Bool(l.can_manage))
                .with("Shared", Value::Str(shared.clone()))
                .with("IsShared", Value::Bool(!shared.is_empty()))
        })
        .collect()
}

/// The colour a new label takes: the next one of the palette.
pub fn next_colour(count: usize) -> &'static str {
    PALETTE[count % PALETTE.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fr() {
        kubuno_desktop::resources::set_culture("fr");
    }

    /// Sizes read exactly as on the web: base 1024, two decimals for Go.
    #[test]
    fn sizes_read_like_the_web() {
        fr();
        assert_eq!(format_size(512), "512 o");
        assert_eq!(format_size(1536), "1.5 Ko");
        assert_eq!(format_size(3 * 1_048_576), "3.0 Mo");
        assert_eq!(format_size(3_500_000_000), "3.26 Go");
    }

    /// The bell's counter caps at « 9+ », like the web.
    #[test]
    fn the_counter_caps_at_nine_plus() {
        assert_eq!(unread_text(3), "3");
        assert_eq!(unread_text(12), "9+");
    }

    #[test]
    fn the_rail_keys_name_the_pages_and_it_collapses_when_narrow() {
        fr();
        for (page, key, _) in NAV {
            assert_eq!(nav_key(page), key);
            assert_eq!(page_of_key(key), Some(page));
        }
        assert_eq!(nav_key(Page::Labels), "", "a page without a rail row selects none");
        assert!(rail_compact(460.0) && !rail_compact(629.0));
    }

    #[test]
    fn the_launcher_says_where_the_sync_stands() {
        fr();
        let mut s = ShellState::design();
        let l = launcher(&s);
        assert_eq!((l.sync_tone, l.sync_state.as_str()), ("Success", "Vos fichiers sont à jour"));
        assert_eq!(l.sync_detail, r"C:\Users\Camille\Kubuno", "no summary yet: where the files live");
        assert_eq!(l.server_text, "https://cloud.exemple.fr");
        assert_eq!(l.online_label, "En ligne");
        s.syncing = true;
        s.status = "↑ 1".into();
        let l = launcher(&s);
        assert_eq!((l.sync_tone, l.sync_button.as_str(), l.sync_detail.as_str()), ("Primary", "Synchronisation…", "↑ 1"));
        s = ShellState { conn: Conn::Expired, offline: true, ..ShellState::default() };
        let l = launcher(&s);
        assert_eq!(l.sync_tone, "Danger");
        assert_eq!(l.server_text, "Session expirée — reconnectez-vous");
        assert_eq!(l.online_label, "Hors ligne");
        assert_eq!(l.sync_detail, "Les activités s'afficheront ici");
    }

    #[test]
    fn accounts_name_the_person_then_the_server() {
        let accounts = [
            AccountInfo { id: "a".into(), server_url: "https://cloud.exemple.fr/".into(), folder: "C:\\K".into(), active: true, user: "Camille Martin".into(), ..AccountInfo::default() },
            AccountInfo { id: "b".into(), server_url: "https://x.org".into(), label: Some("Asso".into()), ..AccountInfo::default() },
        ];
        let rows = account_rows(&accounts);
        let rows: Vec<&Row> = rows.iter().collect();
        assert_eq!(rows[0].text("Title"), "Camille Martin — cloud.exemple.fr");
        assert_eq!(rows[1].text("Title"), "Asso — x.org");
        assert_eq!(account_name(&accounts[1]), "Asso");
        assert_eq!(account_name(&accounts[0]), "cloud.exemple.fr");
    }

    #[test]
    fn labels_say_who_shared_them_and_take_the_next_colour() {
        fr();
        let labels: Vec<kubuno_desktop_sync::Label> = serde_json::from_value(serde_json::json!([
            { "id": "1", "name": "Mine", "color": "#1a73e8", "link_count": 3, "is_owner": true, "can_manage": true },
            { "id": "2", "name": "Theirs", "is_owner": false, "owner_name": "Alex" }
        ]))
        .expect("labels");
        let rows = label_rows(&labels);
        let rows: Vec<&Row> = rows.iter().collect();
        assert_eq!(rows[0].text("Count"), "3");
        assert_eq!(rows[1].text("Shared"), "Partagée par Alex");
        assert_eq!(rows[1].text("Color"), "TextSecondary");
        assert_eq!(next_colour(9), PALETTE[1]);
    }
}
