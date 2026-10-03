//! The shell's door to the sync engine (`kubuno_desktop_sync`), with an offline sample behind it.
//!
//! Every call the views' data comes from goes through here, under the engine's own names. Normally
//! each one simply forwards to `kubuno_desktop_sync`. Under the offline sample (`--sample`,
//! [`set_sample`]) they serve a fixed account instead — a launcher, labels, an activity log and an
//! administration console — so that every page can be shown with deterministic data
//! (screenshots, demos, tests) without a server, without reading or writing the user's
//! configuration, and without registering anything with the system. The sample's writes (a label
//! created, the offline switch…) change the sample in memory only.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use anyhow::Result;
use serde_json::json;

static SAMPLE: AtomicBool = AtomicBool::new(false);

/// Turns the offline sample on (`--sample`), before anything reads the data.
pub fn set_sample(on: bool) {
    SAMPLE.store(on, Ordering::Relaxed);
}

/// Whether this run shows the offline sample.
pub fn is_sample() -> bool {
    SAMPLE.load(Ordering::Relaxed)
}

/// The sample's mutable state: what its writes change.
struct SampleState {
    offline: bool,
    proxy: Option<String>,
    favorites: Option<Vec<String>>,
    labels: Option<Vec<kubuno_desktop_sync::Label>>,
    removed: Vec<String>,
    /// The main account's three unsent changes were « sent » (`sign_out` with « Envoyer d'abord »).
    sent_unsent: bool,
    disabled_modules: Vec<String>,
    enabled_modules: Vec<String>,
}

static STATE: Mutex<SampleState> = Mutex::new(SampleState {
    offline: false,
    proxy: None,
    favorites: None,
    labels: None,
    removed: Vec::new(),
    sent_unsent: false,
    disabled_modules: Vec::new(),
    enabled_modules: Vec::new(),
});

fn with_state<R>(f: impl FnOnce(&mut SampleState) -> R) -> R {
    let mut guard = STATE.lock().unwrap_or_else(|p| p.into_inner());
    f(&mut guard)
}

const MAIN: &str = "sample-cloud";
const SECOND: &str = "sample-asso";

fn config(id: &str, server: &str, root: &str, label: Option<&str>) -> Result<kubuno_desktop_sync::Config> {
    // Built through serde: the engine's type has no public constructor.
    Ok(serde_json::from_value(json!({ "id": id, "server_url": server, "sync_root": root, "label": label }))?)
}

// ── Accounts ─────────────────────────────────────────────────────────────────────────────────

pub fn list_instances() -> Vec<kubuno_desktop_sync::Config> {
    if !is_sample() {
        return kubuno_desktop_sync::list_instances();
    }
    let removed = with_state(|s| s.removed.clone());
    [
        config(MAIN, "https://cloud.exemple.fr", r"C:\Users\Camille\Kubuno", None),
        config(SECOND, "https://kubuno.asso-exemple.org", r"D:\Association\Kubuno", Some("Association")),
    ]
    .into_iter()
    .flatten()
    .filter(|c| !removed.contains(&c.id))
    .collect()
}

pub fn is_offline() -> bool {
    if is_sample() {
        with_state(|s| s.offline)
    } else {
        kubuno_desktop_sync::is_offline()
    }
}

pub fn set_offline(offline: bool) -> Result<()> {
    if is_sample() {
        with_state(|s| s.offline = offline);
        return Ok(());
    }
    kubuno_desktop_sync::set_offline(offline)
}

pub fn get_proxy() -> Option<String> {
    if is_sample() {
        with_state(|s| s.proxy.clone())
    } else {
        kubuno_desktop_sync::get_proxy()
    }
}

pub fn set_proxy(url: Option<String>) -> Result<()> {
    if is_sample() {
        with_state(|s| s.proxy = url);
        return Ok(());
    }
    kubuno_desktop_sync::set_proxy(url)
}

pub fn current_user(id: &str) -> Result<(kubuno_desktop_sync::api::User, kubuno_desktop_sync::Privileges)> {
    if !is_sample() {
        return kubuno_desktop_sync::current_user(id);
    }
    let favorites = with_state(|s| s.favorites.clone())
        .unwrap_or_else(|| ["drive", "mail", "calendar", "chat", "office-documents", "notes"].iter().map(|s| s.to_string()).collect());
    let user = if id == SECOND {
        json!({ "id": "u-2", "display_name": "Camille M.", "email": "camille@asso-exemple.org", "used_bytes": 0, "quota_bytes": 0 })
    } else {
        json!({
            "id": "u-1",
            "display_name": "Camille Martin",
            "email": "camille.martin@exemple.fr",
            "used_bytes": 3_500_000_000u64,
            "quota_bytes": 16_000_000_000u64,
            "preferences": { "waffle_favorites": favorites }
        })
    };
    let privileges = json!({ "is_admin": id != SECOND, "is_superuser": false });
    Ok((serde_json::from_value(user)?, serde_json::from_value(privileges)?))
}

pub fn set_waffle_favorites(id: &str, favorites: &[String]) -> Result<()> {
    if is_sample() {
        with_state(|s| s.favorites = Some(favorites.to_vec()));
        return Ok(());
    }
    kubuno_desktop_sync::set_waffle_favorites(id, favorites)
}

/// How many changes of account `id` are not sent yet (asked before a sign-out). The sample's main account has
/// three, so that the sign-out dialog can be shown without a server.
pub fn unsent_count(id: &str) -> u32 {
    if is_sample() {
        let sent = with_state(|s| s.sent_unsent);
        return if id == MAIN && !sent { 3 } else { 0 };
    }
    crate::services::session::unsent_count(id)
}

/// Signs account `id` out (its sync folder's state goes, the downloaded files stay; the account's secrets go
/// with its last folder), after `choice` when changes were not sent.
pub fn sign_out(id: &str, choice: crate::services::session::SignOutChoice) -> Result<crate::services::session::SignOutOutcome> {
    use crate::services::session::{SignOutChoice, SignOutOutcome};
    if is_sample() {
        // The sample « sends » at once and exports nothing: there is no server and nothing on disk.
        if choice == SignOutChoice::SendFirst {
            with_state(|s| s.sent_unsent = true);
        }
        with_state(|s| s.removed.push(id.to_string()));
        return Ok(SignOutOutcome::SignedOut { exported: None });
    }
    crate::services::session::sign_out_instance(id, choice)
}

pub fn move_instance_folder(id: &str, new_path: &str) -> Result<()> {
    if is_sample() {
        anyhow::bail!("the sample's folders cannot be moved");
    }
    kubuno_desktop_sync::move_instance_folder(id, new_path)
}

/// Signs in to `server` (the sample refuses: there is no server behind it). The answer may ask for the
/// two-factor code ([`login_code`]).
pub fn login(server: &str, login: &str, password: &str, folder: &str) -> Result<crate::services::session::SignIn> {
    if is_sample() {
        anyhow::bail!("l'exemple hors ligne ne se connecte à aucun serveur");
    }
    crate::services::session::sign_in(server, login, password, folder)
}

/// The second step of a sign-in with two-factor authentication.
pub fn login_code(server: &str, totp_session: &str, code: &str, folder: &str) -> Result<crate::services::session::SignIn> {
    if is_sample() {
        anyhow::bail!("l'exemple hors ligne ne se connecte à aucun serveur");
    }
    crate::services::session::sign_in_code(server, totp_session, code, folder)
}

/// One push+pull cycle (the sample has nothing to synchronise).
pub fn sync_once(id: &str) -> Result<kubuno_desktop_sync::Summary> {
    if is_sample() {
        return Ok(kubuno_desktop_sync::Summary::default());
    }
    kubuno_desktop_sync::sync_once(id)
}

/// The launcher's modules, in the shape of `/api/v1/modules`.
pub fn modules_for(id: &str) -> Result<serde_json::Value> {
    if !is_sample() {
        return kubuno_desktop_sync::modules_for(id);
    }
    // The shape the core answers with, ids, glyphs and `logo_url`s included (`<id>-logo.png`, the
    // host's files): the sample has no network, so its logos are the web's files embedded at build
    // time (`services::logos`) — exactly what a real start shows before its first download.
    let item = |id: &str, label: &str, path: &str, icon: &str| {
        json!({ "id": id, "label": label, "path": path, "icon": icon, "logo_url": format!("/{id}-logo.png") })
    };
    let one = |module: &str, label: &str, icon: &str| {
        json!({ "module_id": module, "logo_url": format!("/{module}-logo.png"), "sidebar_items": [item(module, label, &format!("/{module}"), icon)] })
    };
    Ok(json!({ "modules": [
        one("drive", "Drive", "HardDrive"),
        one("mail", "Courrier", "Mail"),
        one("calendar", "Agenda", "Calendar"),
        one("contacts", "Contacts", "Contact"),
        one("chat", "Discussions", "MessagesSquare"),
        // Office's own logo is an SVG the core does not name yet (`logo_url: null`): the sample shows
        // what that gives.
        { "module_id": "office", "logo_url": null, "sidebar_items": [
            { "id": "office", "label": "Office", "path": "/office", "icon": "Briefcase", "logo_url": null },
            item("office-documents", "Documents", "/office/documents", "FileText"),
            item("office-spreadsheets", "Tableurs", "/office/spreadsheets", "TableProperties"),
            item("office-presentations", "Présentations", "/office/presentations", "LayoutTemplate")
        ]},
        one("notes", "Notes", "StickyNote"),
        one("maps", "Plans", "Map"),
        one("photos", "Photos", "Image"),
        one("tasks", "Tâches", "ListChecks"),
        one("wiki", "Wiki", "BookOpen"),
        one("forms", "Formulaires", "ClipboardList"),
    ]}))
}

pub fn fetch_bytes(id: &str, path: &str) -> Result<Vec<u8>> {
    if is_sample() {
        anyhow::bail!("no network in the sample");
    }
    kubuno_desktop_sync::fetch_bytes(id, path)
}

// ── Labels ───────────────────────────────────────────────────────────────────────────────────

fn sample_labels() -> Result<Vec<kubuno_desktop_sync::Label>> {
    let label = |id: &str, name: &str, color: &str, count: i64, owner: Option<&str>| {
        json!({ "id": id, "name": name, "color": color, "link_count": count, "is_owner": owner.is_none(), "can_manage": owner.is_none(), "owner_name": owner })
    };
    Ok(serde_json::from_value(json!([
        label("l1", "Projet Atlas", "#1a73e8", 24, None),
        label("l2", "Comptabilité 2026", "#1e8e3e", 9, None),
        label("l3", "Urgent", "#d93025", 3, None),
        label("l4", "Voyage à Lyon", "#f59e0b", 12, None),
        label("l5", "Équipe design", "#9334e6", 31, Some("Alex Morel")),
    ]))?)
}

pub fn labels(id: &str) -> Result<Vec<kubuno_desktop_sync::Label>> {
    if !is_sample() {
        return kubuno_desktop_sync::labels(id);
    }
    match with_state(|s| s.labels.clone()) {
        Some(list) => Ok(list),
        None => sample_labels(),
    }
}

/// Runs `f` on the sample's labels (seeded on first use).
fn edit_sample_labels(f: impl FnOnce(&mut Vec<kubuno_desktop_sync::Label>)) -> Result<()> {
    let mut list = labels(MAIN)?;
    f(&mut list);
    with_state(|s| s.labels = Some(list));
    Ok(())
}

pub fn create_label(id: &str, name: &str, color: &str) -> Result<()> {
    if !is_sample() {
        return kubuno_desktop_sync::create_label(id, name, color);
    }
    let label: kubuno_desktop_sync::Label = serde_json::from_value(json!({ "id": format!("new-{name}"), "name": name, "color": color, "is_owner": true, "can_manage": true }))?;
    edit_sample_labels(|list| list.push(label))
}

pub fn update_label(id: &str, label: &str, name: Option<&str>, color: Option<&str>) -> Result<()> {
    if !is_sample() {
        return kubuno_desktop_sync::update_label(id, label, name, color);
    }
    edit_sample_labels(|list| {
        if let Some(l) = list.iter_mut().find(|l| l.id == label) {
            if let Some(n) = name {
                l.name = n.to_string();
            }
            if let Some(c) = color {
                l.color = Some(c.to_string());
            }
        }
    })
}

pub fn delete_label(id: &str, label: &str) -> Result<()> {
    if !is_sample() {
        return kubuno_desktop_sync::delete_label(id, label);
    }
    edit_sample_labels(|list| list.retain(|l| l.id != label))
}

/// The activity the sample shows, newest first: (kind, title, body).
pub fn sample_activity() -> &'static [(&'static str, &'static str, &'static str)] {
    &[
        ("synced", "Synchronisation terminée", "↑ 3 envoyé(s), 1 modifié(s) — ↓ 12 reçu(s), 0 supprimé(s)"),
        ("conflict", "Conflit résolu", "Rapport annuel.docx — les deux versions sont conservées"),
        ("error", "Échec de l'envoi", "Budget 2026.xlsx : accès refusé par le serveur"),
        ("synced", "Dossier déplacé", r"C:\Users\Camille\Documents\Kubuno → C:\Users\Camille\Kubuno"),
        ("synced", "Synchronisation manuelle", "↑ 0 envoyé(s), 0 modifié(s) — ↓ 4 reçu(s), 1 supprimé(s)"),
    ]
}

// ── Administration ──────────────────────────────────────────────────────────────────────────

pub fn admin_stats(id: &str) -> Result<serde_json::Value> {
    if !is_sample() {
        return kubuno_desktop_sync::admin_stats(id);
    }
    let daily = |values: &[i64]| values.iter().map(|c| json!({ "count": c })).collect::<Vec<_>>();
    Ok(json!({
        "users_total": 248, "users_active": 231, "users_online": 37, "modules_active": 14,
        "sessions_active": 52, "new_users_7d": 6,
        "modules_by_status": [{ "key": "healthy", "count": 13 }, { "key": "degraded", "count": 1 }],
        "logins_daily": daily(&[112, 98, 131, 140, 127, 36, 21, 118, 125, 137, 142, 133, 41, 25]),
        "signups_daily": daily(&[1, 0, 2, 0, 1, 0, 0, 3, 1, 0, 2, 1, 0, 0]),
    }))
}

const FIRST: [&str; 10] = ["Camille", "Alex", "Sacha", "Noa", "Lou", "Charlie", "Maxime", "Robin", "Dominique", "Claude"];
const LAST: [&str; 10] = ["Martin", "Morel", "Bernard", "Petit", "Durand", "Leroy", "Moreau", "Simon", "Laurent", "Michel"];
const UNITS: [&str; 5] = ["ou-direction", "ou-rh", "ou-it", "ou-support", "ou-sales"];

fn all_users() -> Vec<serde_json::Value> {
    (0..248)
        .map(|i| {
            let (first, last) = (FIRST[i % 10], LAST[(i / 10) % 10]);
            let role = match i {
                0 | 17 | 101 => "admin",
                _ if i % 23 == 5 => "guest",
                _ => "user",
            };
            let login = match i % 7 {
                6 => serde_json::Value::Null,
                n => json!(format!("2026-09-{:02}T08:30:00Z", 30 - n * 3)),
            };
            json!({
                "id": format!("user-{i:03}"),
                "email": format!("{}.{}{}@exemple.fr", first.to_lowercase(), last.to_lowercase(), if i >= 100 { i.to_string() } else { String::new() }),
                "display_name": format!("{first} {last}"),
                "role": role,
                "is_active": i % 19 != 7,
                "used_bytes": (i as u64 * 37 % 97) * 104_857_600,
                "quota_bytes": 10_737_418_240u64,
                "last_login_at": login,
                "org_unit_id": if i % 11 == 4 { serde_json::Value::Null } else { json!(UNITS[i % 5]) },
            })
        })
        .collect()
}

pub fn admin_users(id: &str, offset: u32, limit: u32, query: &str) -> Result<kubuno_desktop_sync::AdminUsers> {
    if !is_sample() {
        return kubuno_desktop_sync::admin_users(id, offset, limit, query);
    }
    let all = all_users();
    let page: Vec<_> = all.iter().skip(offset as usize).take(limit as usize).cloned().collect();
    Ok(serde_json::from_value(json!({ "users": page, "total": all.len(), "offset": offset, "limit": limit }))?)
}

fn units() -> serde_json::Value {
    json!([
        { "id": "ou-direction", "name": "Direction", "description": "Comité de direction" },
        { "id": "ou-rh", "name": "Ressources humaines", "parent_id": "ou-direction" },
        { "id": "ou-it", "name": "Informatique", "description": "Systèmes et réseaux" },
        { "id": "ou-support", "name": "Support", "parent_id": "ou-it" },
        { "id": "ou-sales", "name": "Commercial" },
    ])
}

pub fn admin_org_units(id: &str) -> Result<Vec<kubuno_desktop_sync::OrgUnit>> {
    if !is_sample() {
        return kubuno_desktop_sync::admin_org_units(id);
    }
    Ok(serde_json::from_value(units())?)
}

pub fn admin_org_units_with_counts(id: &str) -> Result<(Vec<kubuno_desktop_sync::OrgUnit>, HashMap<String, i64>)> {
    if !is_sample() {
        return kubuno_desktop_sync::admin_org_units_with_counts(id);
    }
    let counts = [("ou-direction", 8), ("ou-rh", 14), ("ou-it", 42), ("ou-support", 61), ("ou-sales", 101)];
    Ok((serde_json::from_value(units())?, counts.iter().map(|(k, v)| (k.to_string(), *v)).collect()))
}

pub fn admin_groups(id: &str) -> Result<Vec<kubuno_desktop_sync::AdminGroup>> {
    if !is_sample() {
        return kubuno_desktop_sync::admin_groups(id);
    }
    Ok(serde_json::from_value(json!([
        { "id": "g-admins", "name": "Administrateurs", "description": "Accès complet à la console", "permissions": ["admin.*"], "is_system": true, "member_count": 3, "created_at": "2025-11-02T09:00:00Z" },
        { "id": "g-users", "name": "Utilisateurs", "description": "Tous les comptes créés", "permissions": ["drive.use", "mail.use"], "is_default": true, "member_count": 240, "created_at": "2025-11-02T09:00:00Z" },
        { "id": "g-compta", "name": "Comptabilité", "description": "Accès aux classeurs financiers", "permissions": ["office.sheets"], "member_count": 12, "created_at": "2026-01-14T10:20:00Z" },
        { "id": "g-projet", "name": "Équipe projet Atlas", "description": null, "permissions": [], "member_count": 18, "created_at": "2026-03-03T14:05:00Z" },
        { "id": "g-guests", "name": "Invités", "description": "Comptes externes, accès limité", "permissions": ["drive.read"], "member_count": 5, "created_at": "2026-05-21T16:45:00Z" },
    ]))?)
}

pub fn admin_audiences(id: &str) -> Result<Vec<kubuno_desktop_sync::Audience>> {
    if !is_sample() {
        return kubuno_desktop_sync::admin_audiences(id);
    }
    Ok(serde_json::from_value(json!([
        { "id": "a-all", "name": "Tout le monde", "description": "Chaque compte actif de l'instance", "is_everyone": true, "reach": 231, "applied_to": 9 },
        { "id": "a-paris", "name": "Équipe Paris", "description": "Bureaux de Paris", "member_count": 3, "reach": 64, "applied_to": 4 },
        { "id": "a-direction", "name": "Direction", "description": null, "member_count": 1, "reach": 8, "applied_to": 2 },
        { "id": "a-pilot", "name": "Pilote Tableurs", "description": "Testeurs de la nouvelle version", "member_count": 6, "reach": 6, "applied_to": 0 },
    ]))?)
}

pub fn admin_modules_and_default(id: &str) -> Result<(Vec<kubuno_desktop_sync::AdminModule>, Option<String>)> {
    if !is_sample() {
        return kubuno_desktop_sync::admin_modules_and_default(id);
    }
    let (disabled, enabled) = with_state(|s| (s.disabled_modules.clone(), s.enabled_modules.clone()));
    let m = |id: &str, name: &str, description: &str, icon: &str, on: bool, version: &str| {
        let on = (on || enabled.iter().any(|e| e == id)) && !disabled.iter().any(|d| d == id);
        json!({ "id": id, "display_name": name, "description": description, "icon": icon, "is_enabled": on, "version": version, "installed_at": "2026-06-15T10:00:00Z" })
    };
    let modules = json!([
        m("drive", "Drive", "Fichiers, partage et synchronisation", "HardDrive", true, "0.9.4"),
        m("mail", "Courrier", "Messagerie IMAP/SMTP", "Mail", true, "0.8.1"),
        m("calendar", "Agenda", "Agendas partagés et invitations", "Calendar", true, "0.7.0"),
        m("contacts", "Contacts", "Carnet d'adresses", "Contact", true, "0.6.2"),
        m("chat", "Discussions", "Messagerie instantanée et réunions", "MessagesSquare", true, "0.5.3"),
        m("office", "Office", "Documents, tableurs et présentations", "FileText", true, "0.9.0"),
        m("notes", "Notes", "Prise de notes", "StickyNote", true, "0.4.1"),
        m("maps", "Plans", "Cartes et itinéraires", "Map", true, "0.3.0"),
        m("photos", "Photos", "Albums et reconnaissance", "Image", true, "0.4.0"),
        m("tasks", "Tâches", "Listes et tableaux", "ListChecks", false, "0.2.5"),
        m("wiki", "Wiki", "Base de connaissances", "BookOpen", true, "0.3.2"),
        m("forms", "Formulaires", "Enquêtes et questionnaires", "ClipboardList", false, "0.1.9"),
    ]);
    Ok((serde_json::from_value(modules)?, Some("/drive".to_string())))
}

pub fn set_module_enabled(id: &str, module: &str, enabled: bool) -> Result<()> {
    if !is_sample() {
        return kubuno_desktop_sync::set_module_enabled(id, module, enabled);
    }
    with_state(|s| {
        s.disabled_modules.retain(|m| m != module);
        s.enabled_modules.retain(|m| m != module);
        if enabled {
            s.enabled_modules.push(module.to_string());
        } else {
            s.disabled_modules.push(module.to_string());
        }
    });
    Ok(())
}

pub fn admin_settings(id: &str) -> Result<Vec<kubuno_desktop_sync::AdminSetting>> {
    if !is_sample() {
        return kubuno_desktop_sync::admin_settings(id);
    }
    let s = |key: &str, label: &str, description: &str, category: &str, value: serde_json::Value| {
        json!({ "key": key, "label": label, "description": description, "category": category, "value": value })
    };
    Ok(serde_json::from_value(json!([
        s("instance.name", "Nom de l'instance", "Affiché dans l'en-tête et les courriels", "général", json!("Kubuno Exemple")),
        s("instance.url", "Adresse publique", "L'adresse à laquelle les utilisateurs se connectent", "général", json!("https://cloud.exemple.fr")),
        s("instance.locale", "Langue par défaut", "Pour les nouveaux comptes", "général", json!("fr")),
        s("auth.password_min_length", "Longueur minimale du mot de passe", "En caractères", "sécurité", json!(12)),
        s("auth.mfa_required", "Double authentification obligatoire", "Pour tous les comptes", "sécurité", json!(true)),
        s("auth.registration_open", "Inscriptions ouvertes", "Permet de créer un compte sans invitation", "sécurité", json!(false)),
        s("storage.default_quota", "Quota par défaut", "En octets, pour chaque nouveau compte", "stockage", json!(10_737_418_240u64)),
        s("storage.trash_days", "Conservation de la corbeille", "En jours", "stockage", json!(30)),
        s("mail.from", "Expéditeur des notifications", "Adresse des courriels envoyés par l'instance", "courriel", json!("no-reply@exemple.fr")),
    ]))?)
}

pub fn admin_storage(id: &str) -> Result<kubuno_desktop_sync::StorageOverview> {
    if !is_sample() {
        return kubuno_desktop_sync::admin_storage(id);
    }
    sample_storage()
}

/// The sample instance's storage overview (also the designer's data).
pub fn sample_storage() -> Result<kubuno_desktop_sync::StorageOverview> {
    const GB: u64 = 1_073_741_824;
    let cat = |id: &str, used: u64, objects: i64, billable: bool| {
        json!({ "category": id, "used_bytes": used, "object_count": objects, "accounts": 231, "billable": billable, "held": true })
    };
    Ok(serde_json::from_value(json!({
        "accounts": 248,
        "allocated_bytes": 2_480u64 * GB,
        "used_bytes": 812u64 * GB,
        "volume": { "path": "/var/lib/kubuno", "total_bytes": 4_000u64 * GB, "available_bytes": 2_930u64 * GB, "used_bytes": 1_070u64 * GB },
        "quota_states": { "ok": 239, "near": 7, "full": 2 },
        "warn_percent": 85,
        "by_unit": [
            { "unit_id": "ou-sales", "unit_name": "Commercial", "accounts": 101, "used_bytes": 341u64 * GB },
            { "unit_id": "ou-support", "unit_name": "Support", "accounts": 61, "used_bytes": 198u64 * GB },
            { "unit_id": "ou-it", "unit_name": "Informatique", "accounts": 42, "used_bytes": 176u64 * GB },
            { "unit_id": "ou-rh", "unit_name": "Ressources humaines", "accounts": 14, "used_bytes": 51u64 * GB },
            { "unit_id": null, "unit_name": null, "accounts": 22, "used_bytes": 46u64 * GB },
        ],
        "categories": [
            cat("content", 702u64 * GB, 1_284_331, true),
            cat("versions", 61u64 * GB, 92_410, true),
            cat("trash", 33u64 * GB, 18_207, true),
            cat("thumbnails", 9u64 * GB, 640_118, false),
            cat("index", 5u64 * GB, 1_301, false),
            cat("cache", 2u64 * GB, 4_022, false),
        ],
    }))?)
}

/// An instance's sync folder.
pub fn folder_of(id: &str) -> PathBuf {
    list_instances().into_iter().find(|c| c.id == id).map(|c| c.sync_root).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The sample serves every page from fixed data, and its writes stay in memory.
    #[test]
    fn the_sample_serves_every_page() {
        set_sample(true);
        let instances = list_instances();
        assert_eq!(instances.len(), 2);
        let (user, privileges) = current_user(&instances[0].id).expect("sample user");
        assert_eq!(user.email, "camille.martin@exemple.fr");
        assert!(privileges.is_admin);
        assert_eq!(labels(MAIN).expect("labels").len(), 5);
        assert_eq!(admin_users(MAIN, 0, 50, "").expect("users").users.len(), 50);
        assert_eq!(admin_users(MAIN, 200, 50, "").expect("users").users.len(), 48);
        assert_eq!(admin_groups(MAIN).expect("groups").len(), 5);
        assert_eq!(admin_audiences(MAIN).expect("audiences").len(), 4);
        assert_eq!(admin_org_units_with_counts(MAIN).expect("units").0.len(), 5);
        assert!(admin_storage(MAIN).expect("storage").volume.is_some());
        assert_eq!(admin_settings(MAIN).expect("settings").len(), 9);
        assert!(admin_stats(MAIN).expect("stats").get("users_total").is_some());
        assert!(modules_for(MAIN).expect("modules").get("modules").is_some());
        assert!(login("https://x", "a", "b", "c").is_err(), "the sample signs in nowhere");
        set_sample(false);
    }
}
