//! The launcher's apps: what `/api/v1/modules` answers, read the way the web's launcher reads it.
//!
//! Moved here from the shell (`services/apps.rs`) so that every desktop app shows the same tiles as the
//! shell and the web: [`parse_modules`], [`module_label`], [`migrate_favorites`], [`initials_of`] and
//! [`tile`] are pure functions, with no network and no UI state.

use kubuno_shell_controls::Tile;

/// One launchable app of the connected instance.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AppEntry {
    /// The app id, as the server's `sidebar_items[].id` — the key the waffle's favourites are stored
    /// under, shared with the web.
    pub id: String,
    pub label: String,
    /// The SPA route (`/drive`, `/office/documents`): what a launch opens on the web.
    pub path: String,
    /// The app's glyph: the Lucide name the server gives (`sidebar_items[].icon`), `Cloud` when the
    /// embedded icon set does not know it. Shown only when there is no logo — the web's fallback too.
    pub icon: String,
    /// The brand logo the server serves for this app (`logo_url`): the very file the web launcher
    /// shows. `None` keeps `icon`.
    pub logo_url: Option<String>,
    /// The logo as a local file the waffle paints (the server's, cached, else the web's own file
    /// embedded at build time: [`crate::logos`]).
    pub logo_path: Option<std::path::PathBuf>,
    /// The module the app belongs to (`module_id`) and its label: a module with several apps has its
    /// other apps grouped under it in the launcher, as on the web.
    pub module: String,
    pub module_label: String,
}

/// A launcher tile for `app` (its cached logo wins over its icon, as on the web).
pub fn tile(app: &AppEntry) -> Tile {
    Tile {
        id: app.id.clone(),
        label: app.label.clone(),
        icon: app.icon.clone(),
        logo: app.logo_path.as_ref().map(|p| p.to_string_lossy().into_owned()),
        module: Some(app.module.clone()).filter(|m| !m.is_empty()),
        module_label: Some(app.module_label.clone()).filter(|m| !m.is_empty()),
    }
}

/// The URL app `route` opens at on the instance at `server`.
pub fn web_url(server: &str, route: &str) -> String {
    let route = if route.starts_with('/') { route.to_string() } else { format!("/{route}") };
    format!("{}{route}", server.trim_end_matches('/'))
}

/// The API hands back a Lucide icon NAME; the geometry is embedded under that same name, so it is
/// resolved straight from the icon set. An unknown name falls back to `Cloud`, exactly as `getIcon`
/// does on the web: the launcher must never come up empty-handed.
fn lucide(name: &str) -> String {
    kubuno::views::icon::glyph(name).unwrap_or("Cloud").to_string()
}

/// The launchable apps of a `/api/v1/modules` answer (the `{ "modules": [...] }` envelope or a bare
/// array).
pub fn parse_modules(value: &serde_json::Value) -> Vec<AppEntry> {
    let Some(list) = value.get("modules").and_then(|m| m.as_array()).or_else(|| value.as_array()).cloned() else {
        return Vec::new();
    };
    let mut apps = Vec::new();
    for m in list {
        let module = m.get("module_id").and_then(|x| x.as_str()).unwrap_or("");
        if module.is_empty() {
            continue;
        }
        // The core flags a module's internal views (shared, recent, trash…) as not launchable; only
        // real apps get a tile.
        let items: Vec<&serde_json::Value> = m
            .get("sidebar_items")
            .and_then(|x| x.as_array())
            .map(|items| {
                items
                    .iter()
                    .filter(|it| it.get("launchable").and_then(|x| x.as_bool()).unwrap_or(true))
                    .filter(|it| !it.get("path").and_then(|x| x.as_str()).unwrap_or("").is_empty())
                    .collect()
            })
            .unwrap_or_default();
        let module_logo = logo_url(&m);
        // The module's ROOT entry wears the module's brand logo; a sub-app (Office ▸ Documents) wears
        // its own logo or its glyph. The root is the entry named after the module, else its first one —
        // the web's `moduleGlyph.ts` rule (Mail's only entry is `mail-inbox`) — unless a sub-app already
        // carries that very logo as its own (Media's module logo is its Listen sub-app's).
        let item_id = |it: &serde_json::Value| it.get("id").and_then(|x| x.as_str()).unwrap_or(module).to_string();
        let root = items.iter().position(|it| item_id(it) == module).or_else(|| {
            let carried = module_logo.is_some() && items.iter().any(|it| logo_url(it) == module_logo);
            (!carried && !items.is_empty()).then_some(0)
        });
        // A module with ONE app is that app: its tile is keyed by the module's id, as the web's launcher
        // registers it (Mail's only entry, `mail-inbox` at `/mail`, is the tile and favourite `mail`).
        let single = items.len() == 1
            && items[0].get("path").and_then(|x| x.as_str()).is_some_and(|p| p.trim_end_matches('/') == format!("/{module}"));
        for (i, it) in items.iter().enumerate() {
            apps.push(AppEntry {
                id: if single { module.to_string() } else { item_id(it) },
                module: module.to_string(),
                module_label: module_label(module),
                label: it.get("label").and_then(|x| x.as_str()).unwrap_or(module).to_string(),
                path: it.get("path").and_then(|x| x.as_str()).unwrap_or("").to_string(),
                icon: lucide(it.get("icon").and_then(|x| x.as_str()).unwrap_or("")),
                logo_url: logo_url(it).or_else(|| (root == Some(i)).then(|| module_logo.clone()).flatten()),
                logo_path: None,
            });
        }
        // A module with no launchable entry still opens at its root route.
        if items.is_empty() {
            apps.push(AppEntry {
                id: module.to_string(),
                module: module.to_string(),
                module_label: module_label(module),
                label: capitalize(module),
                path: format!("/{module}"),
                icon: "Cloud".to_string(),
                logo_url: module_logo,
                logo_path: None,
            });
        }
    }
    apps
}

/// The server logo reference of a module or sidebar item, if any: `logo_url` (a path the desktop
/// fetches) or a bare `logo` string, so the core can pick the field name without a client change. A
/// blank value is `None`.
pub fn logo_url(v: &serde_json::Value) -> Option<String> {
    ["logo_url", "logo"]
        .iter()
        .find_map(|k| v.get(k).and_then(|x| x.as_str()))
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// A module's label, as the web's launcher registers it (`WaffleAppRegistry.register(id, label, …)`):
/// the id capitalised, with the brands that spell themselves otherwise.
pub fn module_label(module: &str) -> String {
    match module {
        "paintsharp" => "PaintSharp".into(),
        "keestore" => "KeeStore".into(),
        "p2pnas" => "P2P NAS".into(),
        other => capitalize(other),
    }
}

/// The account's saved favourites with the ids an older desktop wrote mapped to the web's: a module's
/// only entry was saved under its item id (`mail-inbox`), the web keys it by the module (`mail`). Ids of
/// no such entry are kept as they are (an edit carries them through).
pub fn migrate_favorites(saved: &[String], apps: &[AppEntry]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for id in saved {
        let known = apps.iter().any(|a| &a.id == id);
        let mapped = if known {
            id.clone()
        } else {
            id.split_once('-')
                .map(|(module, _)| module)
                .filter(|module| apps.iter().filter(|a| a.module == *module).count() == 1 && apps.iter().any(|a| a.id == *module))
                .map_or_else(|| id.clone(), str::to_string)
        };
        if !out.contains(&mapped) {
            out.push(mapped);
        }
    }
    out
}

/// The waffle's favourites as the web stores them: `preferences.waffle_favorites` of a user object, an
/// ordered list of app ids.
pub fn waffle_favorites(user: &serde_json::Value) -> Vec<String> {
    user.pointer("/preferences/waffle_favorites")
        .and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect())
        .unwrap_or_default()
}

/// The first letter of each word, two at most — the rule the web's `HeaderActions` uses.
pub fn initials_of(name: &str) -> String {
    let from_words: String = name.split_whitespace().filter_map(|w| w.chars().next()).take(2).collect();
    let s = if from_words.is_empty() { name.chars().take(2).collect::<String>() } else { from_words };
    if s.is_empty() {
        "?".into()
    } else {
        s.to_uppercase()
    }
}

fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn app(id: &str, module: &str) -> AppEntry {
        AppEntry { id: id.into(), label: id.into(), path: format!("/{id}"), icon: "Cloud".into(), module: module.into(), module_label: capitalize(module), ..AppEntry::default() }
    }

    #[test]
    fn reads_logo_url_then_logo_then_none() {
        assert_eq!(logo_url(&json!({ "logo_url": "/api/v1/modules/photos/logo" })).as_deref(), Some("/api/v1/modules/photos/logo"));
        assert_eq!(logo_url(&json!({ "logo": "/x.png" })).as_deref(), Some("/x.png"));
        assert_eq!(logo_url(&json!({ "logo_url": "/a", "logo": "/b" })).as_deref(), Some("/a"));
        assert_eq!(logo_url(&json!({ "logo_url": "   " })), None);
        assert_eq!(logo_url(&json!({ "icon": "Map" })), None);
        assert_eq!(logo_url(&json!({ "logo_url": 42 })), None);
    }

    /// Glyphs are the server's, an unknown glyph is a cloud, internal views are not launched, and a
    /// module without entries opens at its root.
    #[test]
    fn modules_become_tiles() {
        let apps = parse_modules(&json!({ "modules": [
            { "module_id": "drive", "logo_url": "/drive-logo.png", "sidebar_items": [
                { "id": "drive", "label": "Drive", "path": "/drive", "icon": "HardDrive", "logo_url": "/drive-logo.png" },
                { "id": "trash", "label": "Corbeille", "path": "/drive/trash", "launchable": false }
            ]},
            { "module_id": "office", "sidebar_items": [{ "id": "office-documents", "label": "Documents", "path": "/office/documents", "icon": "NoSuchGlyph" }] },
            { "module_id": "forum" }
        ]}));
        let ids: Vec<&str> = apps.iter().map(|a| a.id.as_str()).collect();
        assert_eq!(ids, ["drive", "office-documents", "forum"]);
        assert_eq!(apps[0].icon, "HardDrive");
        assert_eq!(apps[0].logo_url.as_deref(), Some("/drive-logo.png"));
        assert_eq!(apps[1].icon, "Cloud");
        assert_eq!((apps[2].label.as_str(), apps[2].path.as_str()), ("Forum", "/forum"));
        // A bare array is read too.
        assert_eq!(parse_modules(&json!([{ "module_id": "notes" }])).len(), 1);
        assert!(parse_modules(&json!({ "error": "x" })).is_empty());
    }

    /// The module logo goes to the module's root: the entry named after it, else its first entry —
    /// unless a sub-app already wears that logo as its own.
    #[test]
    fn the_module_logo_goes_to_its_root() {
        let apps = parse_modules(&json!({ "modules": [
            { "module_id": "mail", "logo_url": "/mail-logo.png", "sidebar_items": [
                { "id": "mail-inbox", "label": "Boîte de réception", "path": "/mail", "icon": "Inbox", "logo_url": null }
            ]},
            { "module_id": "media", "logo_url": "/media-listen-logo.png", "sidebar_items": [
                { "id": "media-watch", "label": "Watch", "path": "/media/watch", "icon": "Tv", "logo_url": null },
                { "id": "media-listen", "label": "Listen", "path": "/media/listen", "icon": "Music", "logo_url": "/media-listen-logo.png" }
            ]},
            { "module_id": "calendar", "logo_url": "/calendar-logo.png", "sidebar_items": [
                { "id": "calendar", "label": "Calendar", "path": "/calendar", "icon": "Calendar", "logo_url": null },
                { "id": "calendar-new-event", "label": "Planification", "path": "/calendar/new", "icon": "Users", "logo_url": null }
            ]}
        ]}));
        let logo = |id: &str| apps.iter().find(|a| a.id == id).and_then(|a| a.logo_url.clone());
        assert_eq!(logo("mail").as_deref(), Some("/mail-logo.png"), "the tile is keyed by the module, as on the web");
        assert_eq!(logo("media-watch"), None);
        assert_eq!(logo("media-listen").as_deref(), Some("/media-listen-logo.png"));
        assert_eq!(logo("calendar").as_deref(), Some("/calendar-logo.png"));
        assert_eq!(logo("calendar-new-event"), None);
    }

    /// An older desktop saved Mail as `mail-inbox`: it becomes `mail`, once, in its place; other unknown
    /// ids (the web's, a module not installed) are kept.
    #[test]
    fn old_desktop_ids_are_mapped_to_the_web_s() {
        let apps = [app("mail", "mail"), app("office-documents", "office"), app("office-sheets", "office")];
        let saved: Vec<String> = ["drive", "mail-inbox", "office-documents", "mail", "office-whiteboard"].iter().map(|s| s.to_string()).collect();
        assert_eq!(migrate_favorites(&saved, &apps), ["drive", "mail", "office-documents", "office-whiteboard"]);
        assert_eq!(module_label("paintsharp"), "PaintSharp");
        assert_eq!(module_label("office"), "Office");
    }

    #[test]
    fn favorites_initials_tiles_and_routes() {
        let user = json!({ "email": "a@b", "preferences": { "waffle_favorites": ["drive", 3, "mail"] } });
        assert_eq!(waffle_favorites(&user), ["drive", "mail"]);
        assert!(waffle_favorites(&json!({ "email": "a@b" })).is_empty());
        assert_eq!(initials_of("Camille Martin"), "CM");
        assert_eq!(initials_of("camille"), "C");
        assert_eq!(initials_of(""), "?");
        let mut a = app("drive", "drive");
        a.logo_path = Some("C:/cache/logo.png".into());
        let t = tile(&a);
        assert_eq!((t.id.as_str(), t.logo.as_deref(), t.module.as_deref()), ("drive", Some("C:/cache/logo.png"), Some("drive")));
        assert_eq!(web_url("https://cloud.exemple.fr/", "office/documents"), "https://cloud.exemple.fr/office/documents");
        assert_eq!(web_url("https://x", "/drive"), "https://x/drive");
    }
}
