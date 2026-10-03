//! The administration console's structure, without any UI: its navigation tree (the web's
//! `adminNav.ts`, verbatim in structure; the labels are resources `adm_<id>`), which sections this
//! build shows natively, the rail rows it adds under « ADMINISTRATION », and what a section is asked
//! to load.
//!
//! The console is 49 sections on the web; the desktop shows eight natively (the dashboard, the users,
//! groups, audiences and organisational units of the directory, the installed modules, the
//! instance's identity and the storage) and offers every other one in the browser. Nothing is
//! computed locally: the server owns every figure and enforces every privilege regardless of what
//! the console shows.
//!
//! The folder holds the console's views: the page itself (`admin_page`), the header every section
//! shares (`admin_section_header`), one user control per native section (`admin_<section>`), the item
//! templates those sections repeat (`group_row`, `settings_category`, `storage_block`), and the
//! designer's sample rows (`design/*.json`, the views' `d:ItemsSource`).

pub mod admin_audiences;
pub mod admin_dashboard;
pub mod admin_groups;
pub mod admin_modules;
pub mod admin_org_units;
pub mod admin_page;
pub mod admin_section_header;
pub mod admin_settings;
pub mod admin_storage;
pub mod admin_users;
pub mod group_row;
pub mod settings_category;
pub mod storage_block;

use kubuno::{Row, Rows, Value};

/// One node of the navigation tree: a group (with children, expandable) or a leaf (a section).
/// Top-level nodes carry an icon; the leaves of a group do not, as the web draws them.
pub struct NavNode {
    pub id: &'static str,
    pub icon: Option<&'static str>,
    pub children: &'static [NavNode],
}

macro_rules! leaf {
    ($id:literal) => {
        NavNode { id: $id, icon: None, children: &[] }
    };
}

/// The web's tree (`adminNav.ts`).
pub const NAV: &[NavNode] = &[
    NavNode { id: "home", icon: Some("Home"), children: &[] },
    NavNode { id: "dashboard", icon: Some("BarChart3"), children: &[] },
    NavNode {
        id: "directory",
        icon: Some("Contact"),
        children: &[leaf!("users"), leaf!("groups"), leaf!("audiences"), leaf!("org-units"), leaf!("resources"), leaf!("directory-settings")],
    },
    NavNode { id: "devices", icon: Some("HardDrive"), children: &[leaf!("device-sessions"), leaf!("networks")] },
    NavNode { id: "apps", icon: Some("LayoutGrid"), children: &[leaf!("modules"), leaf!("marketplace")] },
    NavNode { id: "core-features", icon: Some("Sparkles"), children: &[leaf!("voice-search")] },
    NavNode {
        id: "security",
        icon: Some("Shield"),
        children: &[
            leaf!("security-health"),
            leaf!("alerts"),
            leaf!("sso"),
            leaf!("ldap"),
            leaf!("session-policy"),
            leaf!("access-data"),
            leaf!("service-protection"),
            NavNode { id: "security-center", icon: None, children: &[leaf!("security-dashboard")] },
        ],
    },
    NavNode { id: "reporting", icon: Some("Activity"), children: &[leaf!("reports"), leaf!("event-log"), leaf!("audit")] },
    NavNode {
        id: "account",
        icon: Some("Building2"),
        children: &[
            leaf!("settings"),
            leaf!("holidays"),
            leaf!("apparence"),
            leaf!("domains"),
            leaf!("admin-roles"),
            leaf!("data-migration"),
            leaf!("data-export"),
        ],
    },
    NavNode { id: "automation", icon: Some("Workflow"), children: &[leaf!("rules"), leaf!("rules-log"), leaf!("detectors")] },
    NavNode { id: "billing", icon: Some("Award"), children: &[leaf!("subscription")] },
    NavNode { id: "storage", icon: Some("Database"), children: &[] },
    NavNode { id: "system", icon: Some("Server"), children: &[leaf!("background-jobs"), leaf!("email")] },
];

/// The section the console opens on (the web's landing section).
pub const DEFAULT_SECTION: &str = "dashboard";

/// The sections this build shows natively; every other leaf offers the browser.
pub const NATIVE: &[&str] = &["dashboard", "users", "groups", "audiences", "org-units", "modules", "settings", "storage"];

/// The prefix of the console's rail keys (the main rail already has a `settings` key).
pub const KEY_PREFIX: &str = "admin:";

fn find(nodes: &'static [NavNode], id: &str) -> Option<&'static NavNode> {
    for n in nodes {
        if n.id == id {
            return Some(n);
        }
        if let Some(found) = find(n.children, id) {
            return Some(found);
        }
    }
    None
}

/// The group holding `id`, if it is a leaf of one.
fn parent_of(nodes: &'static [NavNode], id: &str) -> Option<&'static NavNode> {
    for n in nodes {
        if n.children.iter().any(|c| c.id == id) {
            return Some(n);
        }
        if let Some(found) = parent_of(n.children, id) {
            return Some(found);
        }
    }
    None
}

/// Whether `id` is a section of the console (a leaf of the tree).
pub fn is_section(id: &str) -> bool {
    find(NAV, id).is_some_and(|n| n.children.is_empty())
}

/// Whether the desktop shows section `id` natively.
pub fn is_native(id: &str) -> bool {
    NATIVE.contains(&id)
}

/// A resource of the main set, by key (the labels of the tree are looked up by their id).
fn text(key: &str) -> String {
    match kubuno::views::resources::value_of(Some("resources"), key) {
        Some(Value::Str(s)) => s,
        _ => key.to_string(),
    }
}

/// The label of node `id` (« Utilisateurs »).
pub fn label(id: &str) -> String {
    text(&format!("adm_{}", id.replace('-', "_")))
}

/// The breadcrumb's first segment for section `id`: its group (« Annuaire »), or « Administration »
/// for a top-level section.
pub fn parent_label(id: &str) -> String {
    match parent_of(NAV, id) {
        Some(group) => label(group.id),
        None => text("adm_console"),
    }
}

/// Where the web console shows section `id` (`/admin/<id>`).
pub fn web_path(id: &str) -> String {
    format!("/admin/{id}")
}

/// The rail's rows while the console is open: a section header, then the tree, `expanded` groups
/// showing their children, `active` the section shown. `indent` lines the top-level rows' icons up
/// with the rest of the rail.
pub fn rail_rows(expanded: &[String], indent: f32) -> Vec<Row> {
    fn walk(nodes: &'static [NavNode], level: u32, expanded: &[String], indent: f32, out: &mut Vec<Row>) {
        for n in nodes {
            let group = !n.children.is_empty();
            let open = group && expanded.iter().any(|e| e == n.id);
            let mut row = Row::new()
                .with("Key", Value::Str(format!("{KEY_PREFIX}{}", n.id)))
                .with("Text", Value::Str(label(n.id)))
                .with("Icon", Value::Str(n.icon.unwrap_or("").to_string()))
                .with("Level", Value::F32(level as f32))
                .with("Expanded", Value::Bool(open));
            // A top-level row without children lines its icon up with the groups' (past their chevron).
            if level == 0 && !group {
                row = row.with("Indent", Value::F32(indent));
            }
            out.push(row);
            // The children follow their group; the rail shows them while it is expanded.
            walk(n.children, level + 1, expanded, indent, out);
        }
    }
    let mut out = vec![Row::new().with("Key", Value::Str(format!("{KEY_PREFIX}header"))).with("Text", Value::Str(text("adm_section_header"))).with("Kind", Value::Str("Section".into()))];
    walk(NAV, 0, expanded, indent, &mut out);
    out
}

/// The rail's rows as a list for `Sidebar.ItemsSource`.
pub fn rail(expanded: &[String], indent: f32) -> Rows {
    Rows::from(rail_rows(expanded, indent))
}

/// The groups to expand so section `id` shows in the rail.
pub fn groups_of(id: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = id;
    while let Some(group) = parent_of(NAV, current) {
        out.push(group.id.to_string());
        current = group.id;
    }
    out
}

/// What a section shows: its data once loaded, whether a load is running, and why the last one failed.
#[derive(Debug, Clone, PartialEq)]
pub struct SectionState<T> {
    pub loading: bool,
    pub error: String,
    pub data: Option<T>,
}

impl<T> Default for SectionState<T> {
    fn default() -> Self {
        Self { loading: false, error: String::new(), data: None }
    }
}

impl<T> SectionState<T> {
    /// A load is running (what was shown stays).
    pub fn loading(previous: Option<T>) -> Self {
        Self { loading: true, error: String::new(), data: previous }
    }

    /// The outcome of a load.
    pub fn done(result: Result<T, String>) -> Self {
        match result {
            Ok(data) => Self { loading: false, error: String::new(), data: Some(data) },
            Err(error) => Self { loading: false, error, data: None },
        }
    }
}

/// What a section is asked to load: a page of a list and a search, where it has them.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SectionRequest {
    pub page: u32,
    pub query: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_native_section_is_a_leaf_of_the_tree() {
        for id in NATIVE {
            assert!(is_section(id), "{id}");
        }
        assert!(!is_section("directory"), "a group is no section");
        assert!(is_section("home") && !is_native("home"));
    }

    #[test]
    fn a_leaf_names_its_group_in_the_breadcrumb() {
        assert_eq!(parent_of(NAV, "users").map(|n| n.id), Some("directory"));
        assert_eq!(parent_of(NAV, "security-dashboard").map(|n| n.id), Some("security-center"));
        assert_eq!(groups_of("security-dashboard"), ["security-center", "security"]);
        assert!(parent_of(NAV, "dashboard").is_none());
        assert_eq!(web_path("org-units"), "/admin/org-units");
    }

    #[test]
    fn the_rail_lists_the_tree_under_its_header() {
        let rows = rail_rows(&["directory".to_string()], 28.0);
        assert_eq!(rows[0].text("Kind"), "Section");
        let keys: Vec<String> = rows.iter().map(|r| r.text("Key")).collect();
        assert_eq!(keys[1..4], ["admin:home", "admin:dashboard", "admin:directory"]);
        // The expanded group's children follow it, one level down.
        assert_eq!(rows[4].text("Key"), "admin:users");
        assert_eq!(rows[4].text("Level"), "1");
        assert_eq!(rows[3].text("Expanded"), "true");
        // A top-level leaf lines up with the groups.
        assert_eq!(rows[1].text("Indent"), "28");
        assert!(keys.contains(&"admin:email".to_string()));
    }
}

#[cfg(test)]
mod view_tests {
    /// The console's views compile at run time (the macro checks their bindings, not every attribute
    /// of a custom control).
    #[test]
    fn the_admin_views_compile() {
        for text in [
            include_str!("admin_section_header.kbcontrol"),
            include_str!("../controls/stat_card.kbcontrol"),
            include_str!("admin_dashboard.kbcontrol"),
            include_str!("admin_page.kbcontrol"),
            include_str!("admin_users.kbcontrol"),
            include_str!("group_row.kbcontrol"),
            include_str!("admin_groups.kbcontrol"),
            include_str!("admin_audiences.kbcontrol"),
            include_str!("admin_org_units.kbcontrol"),
            include_str!("admin_modules.kbcontrol"),
            include_str!("settings_category.kbcontrol"),
            include_str!("admin_settings.kbcontrol"),
            include_str!("storage_block.kbcontrol"),
            include_str!("admin_storage.kbcontrol"),
        ] {
            if let Err(d) = kubuno::views::compile::compile(text) {
                panic!("{d:?}");
            }
        }
    }
}
