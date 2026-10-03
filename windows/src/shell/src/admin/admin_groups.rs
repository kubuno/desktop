//! Code-behind of the user control `GroupsSection` (`admin_groups.kbcontrol`, see its comment): the
//! directory's groups. [`load`] reads them (`GET /api/v1/admin/groups`, off the UI thread);
//! [`GroupsSection::show`] lists them; the chevrons expand them here.

use kubuno::views::component::Shared;
use kubuno::views::prelude::*;
use kubuno::{Row, Rows, Value};

use crate::admin::SectionState;
use crate::model::events::ItemCommandEventArgs;
use crate::controls::status_presenter::StatusMode;
use crate::Resources;

/// One group, as the list shows it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GroupInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    pub permissions: Vec<String>,
    pub is_default: bool,
    pub is_system: bool,
    pub members: i64,
    /// The creation date (`2026-02-12`), empty when unknown.
    pub created: String,
}

/// The groups of the instance.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GroupsData {
    pub groups: Vec<GroupInfo>,
}

/// Reads the groups of instance `id` (blocking: run it off the UI thread).
pub fn load(id: &str) -> anyhow::Result<GroupsData> {
    let groups = crate::services::backend::admin_groups(id)?
        .into_iter()
        .map(|g| GroupInfo {
            id: g.id,
            name: g.name,
            description: g.description.unwrap_or_default(),
            permissions: g.permissions,
            is_default: g.is_default,
            is_system: g.is_system,
            members: g.member_count,
            created: g.created_at.map(|d| d.split('T').next().unwrap_or(&d).to_string()).unwrap_or_default(),
        })
        .collect();
    Ok(GroupsData { groups })
}

/// A permission as the console words it.
fn permission_label(p: &str) -> String {
    match p {
        "api_tokens.create" => Resources::groups_perm_api_tokens().to_string(),
        other => other.to_string(),
    }
}

/// « 1 membre », « 12 membres ».
pub fn members_text(n: i64) -> String {
    if n == 1 { Resources::groups_member() } else { Resources::groups_members() }.replace("{0}", &n.to_string())
}

/// The list's items: each group's header, followed by its details while it is `expanded`.
pub fn rows(groups: &[GroupInfo], expanded: &[String]) -> Rows {
    let mut out = Vec::new();
    for g in groups {
        let open = expanded.contains(&g.id);
        let base = |kind: &str| {
            Row::new()
                .with("Key", Value::Str(format!("{}/{kind}", g.id)))
                .with("Id", Value::Str(g.id.clone()))
                .with("Kind", Value::Str(kind.into()))
                .with("Expanded", Value::Bool(open))
        };
        out.push(
            base("header")
                .with("GroupName", Value::Str(g.name.clone()))
                .with("Description", Value::Str(g.description.clone()))
                .with("IsDefault", Value::Bool(g.is_default))
                .with("IsSystem", Value::Bool(g.is_system))
                .with("CountText", Value::Str(members_text(g.members))),
        );
        if open {
            let permissions = Rows::from(g.permissions.iter().map(|p| Row::new().with("Label", Value::Str(permission_label(p)))).collect::<Vec<_>>());
            let created = if g.created.is_empty() { String::new() } else { Resources::groups_created().replace("{0}", &g.created) };
            out.push(base("details").with("Permissions", Value::List(permissions)).with("CreatedText", Value::Str(created)));
        }
    }
    Rows::from(out)
}

/// The directory's groups (see the module doc).
#[derive(UserControl, Default)]
#[user_control(view = "admin_groups.kbcontrol")]
#[category("Kubuno")]
pub struct GroupsSection {
    base: UserControlCore,
    /// What to show (bound by the console page).
    #[property(bindable, on_change = "state_changed")]
    #[category("Data")]
    pub state: Shared<SectionState<GroupsData>>,
    /// Occurs when the section asks for the web console.
    #[event]
    #[category("Action")]
    pub command: Event<ItemCommandEventArgs>,
    #[property(bindable)]
    #[browsable(false)]
    pub rows: Rows,
    #[property(bindable)]
    #[browsable(false)]
    pub count_text: String,
    #[property(bindable)]
    #[browsable(false)]
    pub status_mode: String,
    #[property(bindable)]
    #[browsable(false)]
    pub error_text: String,
    groups: Vec<GroupInfo>,
    expanded: Vec<String>,
}

impl GroupsSection {
    fn state_changed(&mut self) {
        let state = self.state.clone();
        if let Some(data) = state.data.as_ref() {
            self.show(data);
        }
        let has = !self.groups.is_empty();
        self.error_text = state.error.clone();
        self.status_mode = StatusMode::of(has, state.loading, &state.error).name().into();
    }

    /// Lists the groups.
    pub fn show(&mut self, data: &GroupsData) {
        self.groups = data.groups.clone();
        self.count_text = if self.groups.len() == 1 { Resources::groups_count_one() } else { Resources::groups_count() }.replace("{0}", &self.groups.len().to_string());
        self.refresh();
    }

    fn refresh(&mut self) {
        self.rows = rows(&self.groups, &self.expanded);
    }

    /// Expands or collapses group `id`.
    pub fn toggle(&mut self, id: &str) {
        match self.expanded.iter().position(|e| e == id) {
            Some(i) => {
                self.expanded.remove(i);
            }
            None => self.expanded.push(id.to_string()),
        }
        self.refresh();
    }

    fn current_id() -> Option<String> {
        kubuno::views::binding::current_item().map(|item| item.row.text("Id"))
    }
}

#[kubuno::views::event_handlers]
impl GroupsSection {
    fn groups_section_load(&mut self) {
        if self.design_mode() && self.groups.is_empty() {
            self.show(&design_data());
        }
    }

    fn new_group_click(&mut self) {
        self.raise_command(ItemCommandEventArgs::new("open-web", "", "/admin/groups?action=create"));
    }

    fn row_toggle_requested(&mut self) {
        if let Some(id) = Self::current_id() {
            self.toggle(&id);
        }
    }

    fn row_edit_requested(&mut self) {
        if let Some(id) = Self::current_id() {
            let path = format!("/admin/groups/{id}");
            self.raise_command(ItemCommandEventArgs::new("open-web", &id, &path));
        }
    }
}

/// What the designer shows: the sample instance's groups.
pub fn design_data() -> GroupsData {
    let g = |id: &str, name: &str, description: &str, members: i64, is_default: bool, is_system: bool| GroupInfo {
        id: id.into(),
        name: name.into(),
        description: description.into(),
        permissions: vec!["api_tokens.create".into()],
        is_default,
        is_system,
        members,
        created: "2026-02-12".into(),
    };
    GroupsData {
        groups: vec![
            g("admins", "Administrateurs", "Accès complet à la console", 3, false, true),
            g("users", "Utilisateurs", "Tous les comptes créés", 240, true, false),
            g("accounting", "Comptabilité", "Accès aux classeurs financiers", 12, false, false),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_expanded_group_is_followed_by_its_details() {
        let groups = design_data().groups;
        let r = rows(&groups, &[]);
        assert_eq!(r.len(), 3);
        let r = rows(&groups, &["users".to_string()]);
        assert_eq!(r.len(), 4);
        assert_eq!(r.get(2).map(|row| row.text("Kind")), Some("details".to_string()));
        assert_eq!(r.get(1).map(|row| row.text("Expanded")), Some("true".to_string()));
    }

    #[test]
    fn the_chevron_toggles_a_group() {
        let mut s = GroupsSection::default();
        s.show(&design_data());
        s.toggle("admins");
        assert_eq!(s.rows.len(), 4);
        s.toggle("admins");
        assert_eq!(s.rows.len(), 3);
        assert_eq!(members_text(1), Resources::groups_member().replace("{0}", "1"));
    }
}
