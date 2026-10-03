//! What the header shows at one moment: a [`HeaderSnapshot`], built off the UI thread from the broker's
//! accounts and the server's `/api/v1/modules` and `/api/v1/me` answers (or their copies on disk), and handed to
//! the UI thread whole.

use kubuno_account::{AccountSummary, SessionStatus};
use kubuno_shell_controls::{AccountEntry, AccountUser};

use crate::modules::{self, AppEntry};

/// Where a snapshot's data come from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Status {
    /// Nothing known yet (the first answer is on its way).
    #[default]
    Loading,
    /// The offline sample (`--sample`, a Debug run under a debugger): the controls' design data, no broker,
    /// no network, nothing written.
    Sample,
    /// The copy kept on disk by the last run, shown while the broker and the server answer.
    Cached,
    /// The server just answered.
    Online,
    /// The server could not be reached: the copy kept on disk (offline-first).
    Offline,
    /// The shell says the account's session ended (sign in again in the shell): the copy kept on disk.
    Expired,
    /// The shell has no account signed in.
    SignedOut,
    /// No shell answers the broker (not installed next to this program, or it could not start).
    NoShell,
}

/// The header's data (see the module doc). Plain data: it crosses threads.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct HeaderSnapshot {
    pub status: Status,
    /// The current account's key (`kubuno_account::AccountKey`), `None` without one.
    pub account: Option<String>,
    /// The current account's server (`https://cloud.exemple.fr`): the apps' web routes open under it.
    pub server: String,
    /// The instance's apps, in the server's order.
    pub apps: Vec<AppEntry>,
    /// `preferences.waffle_favorites`, ids this build does not know included.
    pub favorites: Vec<String>,
    /// The current account.
    pub user: AccountUser,
    /// The account may enter the administration console (the server enforces it regardless).
    pub is_admin: bool,
    /// The shell's other accounts (the current one excluded).
    pub others: Vec<AccountEntry>,
}

impl HeaderSnapshot {
    /// A snapshot that only says where things stand (no account, no data).
    pub fn empty(status: Status) -> Self {
        Self { status, ..Self::default() }
    }

    /// The offline sample: the shell controls' design data (`controls/design/apps.json`, twelve apps;
    /// `controls/design/accounts.json`, Camille Martin and two other accounts) — what the Visual Studio
    /// designer shows too. Nothing is read or written.
    pub fn sample() -> Self {
        use kubuno_shell_controls::controls::{account_menu, app_tile_grid};
        let apps = app_tile_grid::design_tiles()
            .into_iter()
            .map(|t| AppEntry {
                path: format!("/{}", t.id.replace('-', "/")),
                id: t.id,
                label: t.label,
                icon: t.icon,
                logo_url: None,
                logo_path: t.logo.map(Into::into),
                module: t.module.clone().unwrap_or_default(),
                module_label: t.module_label.unwrap_or_default(),
            })
            .collect();
        let (user, others, is_admin) = account_menu::design_data();
        Self {
            status: Status::Sample,
            account: None,
            server: "https://kubuno.exemple.fr".into(),
            apps,
            favorites: app_tile_grid::design_favorites(),
            user,
            is_admin,
            others,
        }
    }

    /// Whether a launch / a save can reach the server.
    pub fn is_live(&self) -> bool {
        matches!(self.status, Status::Online | Status::Cached | Status::Offline | Status::Expired)
    }
}

/// The user object of a `/api/v1/me` answer (`{ "user": {…}, "privileges": {…} }`, or a bare user).
pub fn me_user(me: &serde_json::Value) -> &serde_json::Value {
    me.get("user").filter(|u| u.is_object()).unwrap_or(me)
}

/// Whether a `/api/v1/me` answer lets the account enter the console (`privileges.is_admin` or
/// `is_superuser`).
pub fn me_is_admin(me: &serde_json::Value) -> bool {
    let flag = |k: &str| me.pointer(&format!("/privileges/{k}")).and_then(|v| v.as_bool()).unwrap_or(false);
    flag("is_admin") || flag("is_superuser")
}

/// The display name of a `/api/v1/me` user: `display_name`, else `username`, else `email` — the shell's rule.
pub fn me_name(user: &serde_json::Value) -> String {
    let field = |k: &str| user.get(k).and_then(|v| v.as_str()).map(str::trim).filter(|v| !v.is_empty()).map(str::to_string);
    field("display_name").or_else(|| field("username")).or_else(|| field("email")).unwrap_or_default()
}

/// The profile photo's server path of a `/api/v1/me` user (`avatar_url`), when it has one.
pub fn me_avatar_url(user: &serde_json::Value) -> Option<String> {
    user.get("avatar_url").and_then(|v| v.as_str()).map(str::trim).filter(|v| !v.is_empty()).map(str::to_string)
}

/// The header's data for `account` (on `server`) from the server's answers, without the pictures (the
/// caller resolves `AppEntry::logo_path` and `AccountUser::avatar`). `summary` fills the name and address
/// while `/api/v1/me` has never answered.
pub fn build(status: Status, current: &AccountSummary, others: &[AccountSummary], modules_json: Option<&serde_json::Value>, me: Option<&serde_json::Value>) -> HeaderSnapshot {
    let apps = modules_json.map(modules::parse_modules).unwrap_or_default();
    let user_json = me.map(me_user);
    let name = user_json
        .map(me_name)
        .filter(|n| !n.is_empty())
        .or_else(|| current.info.display_name.clone().filter(|n| !n.trim().is_empty()))
        .or_else(|| current.info.email.clone())
        .unwrap_or_else(|| current.info.user_id.clone());
    let email = user_json
        .and_then(|u| u.get("email").and_then(|v| v.as_str()).map(str::to_string))
        .or_else(|| current.info.email.clone())
        .unwrap_or_default();
    let favorites = user_json.map(|u| modules::migrate_favorites(&modules::waffle_favorites(u), &apps)).unwrap_or_default();
    HeaderSnapshot {
        status,
        account: Some(current.info.key.as_str().to_string()),
        server: current.info.server_url.clone(),
        favorites,
        user: AccountUser { initials: modules::initials_of(&name), name, email, avatar: None },
        is_admin: me.is_some_and(me_is_admin),
        others: others.iter().filter(|o| o.info.key != current.info.key).map(entry_of).collect(),
        apps,
    }
}

/// A row of the account panel for another account of the shell.
pub fn entry_of(summary: &AccountSummary) -> AccountEntry {
    let info = &summary.info;
    let name = info
        .display_name
        .clone()
        .filter(|n| !n.trim().is_empty())
        .or_else(|| info.email.clone())
        .unwrap_or_else(|| info.user_id.clone());
    AccountEntry {
        id: info.key.as_str().to_string(),
        initials: Some(modules::initials_of(&name)),
        name,
        email: info.email.clone().unwrap_or_default(),
        server: host_of(&info.server_url),
        avatar: None,
        connected: summary.status == SessionStatus::Active,
        // Every account of the shell is switched to in place (the broker's `switch_account`), whatever its
        // server: the « Ouvrir » of the web's other-instance rows has no desktop meaning.
        remote: false,
        unread: 0,
    }
}

/// `kubuno.asso-exemple.org` of `https://kubuno.asso-exemple.org/`.
pub fn host_of(url: &str) -> String {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    rest.split('/').next().unwrap_or(rest).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use kubuno_account::AccountInfo;
    use serde_json::json;

    fn summary(server: &str, user: &str, name: Option<&str>, status: SessionStatus) -> AccountSummary {
        let mut info = AccountInfo::new(server, user).expect("info");
        info.display_name = name.map(str::to_string);
        info.email = Some(format!("{user}@{}", host_of(server)));
        AccountSummary { info, status }
    }

    #[test]
    fn the_server_answers_become_the_header() {
        let current = summary("https://cloud.exemple.fr", "camille", Some("Camille M."), SessionStatus::Active);
        let other = summary("https://kubuno.asso.org/", "c2", None, SessionStatus::SessionExpired);
        let modules = json!({ "modules": [
            { "module_id": "drive", "sidebar_items": [{ "id": "drive", "label": "Drive", "path": "/drive", "icon": "HardDrive" }] },
            { "module_id": "mail", "sidebar_items": [{ "id": "mail-inbox", "label": "Mail", "path": "/mail", "icon": "Inbox" }] }
        ]});
        let me = json!({
            "user": { "display_name": "Camille Martin", "email": "camille@exemple.fr", "avatar_url": "/api/v1/users/1/avatar",
                      "preferences": { "waffle_favorites": ["mail-inbox", "office-whiteboard"] } },
            "privileges": { "is_admin": true }
        });
        let s = build(Status::Online, &current, &[current.clone(), other.clone()], Some(&modules), Some(&me));
        assert_eq!(s.server, "https://cloud.exemple.fr");
        assert_eq!(s.apps.iter().map(|a| a.id.as_str()).collect::<Vec<_>>(), ["drive", "mail"]);
        assert_eq!(s.favorites, ["mail", "office-whiteboard"], "old ids mapped, unknown ids kept");
        assert_eq!((s.user.name.as_str(), s.user.initials.as_str(), s.user.email.as_str()), ("Camille Martin", "CM", "camille@exemple.fr"));
        assert!(s.is_admin);
        assert_eq!(s.others.len(), 1, "the current account is not listed among the others");
        let o = &s.others[0];
        assert_eq!((o.id.as_str(), o.server.as_str(), o.connected, o.remote), (other.info.key.as_str(), "kubuno.asso.org", false, false));
        assert_eq!(me_avatar_url(me_user(&me)).as_deref(), Some("/api/v1/users/1/avatar"));
    }

    /// Before `/api/v1/me` ever answered, the broker's account says who it is.
    #[test]
    fn the_account_names_the_user_until_the_server_answers() {
        let current = summary("https://cloud.exemple.fr", "camille", Some("Camille Martin"), SessionStatus::Active);
        let s = build(Status::Offline, &current, &[], None, None);
        assert_eq!((s.user.name.as_str(), s.user.initials.as_str()), ("Camille Martin", "CM"));
        assert!(s.apps.is_empty() && s.favorites.is_empty() && !s.is_admin);
        assert_eq!(me_name(&json!({ "username": "cam", "email": "c@x" })), "cam");
        assert!(!me_is_admin(&json!({ "user": {} })));
        assert!(me_is_admin(&json!({ "privileges": { "is_superuser": true } })));
    }

    #[test]
    fn the_sample_is_the_design_data() {
        let s = HeaderSnapshot::sample();
        assert_eq!(s.status, Status::Sample);
        assert_eq!(s.apps.len(), 12);
        assert_eq!(s.user.name, "Camille Martin");
        assert_eq!(s.others.len(), 2);
        assert!(!s.favorites.is_empty() && s.is_admin && !s.is_live());
        assert_eq!(host_of("http://host:8080/kubuno"), "host:8080");
    }
}
