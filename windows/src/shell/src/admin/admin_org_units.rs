//! Code-behind of the user control `OrgUnitsSection` (`admin_org_units.kbcontrol`, see its comment): the
//! directory's organisational units. [`load`] reads them with their accounts (off the UI thread);
//! the `OrgUnitTree` shows them.

use kubuno_desktop::views::component::Shared;
use kubuno_desktop::views::prelude::*;

use crate::admin::SectionState;
use crate::model::events::ItemCommandEventArgs;
use crate::controls::org_unit_tree::{Unit, UnitEventArgs};
use crate::controls::status_presenter::StatusMode;
use crate::Resources;

/// The units of the instance.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct OrgUnitsData {
    pub units: Vec<Unit>,
}

/// Reads the units of instance `id` and their accounts (blocking: run it off the UI thread).
pub fn load(id: &str) -> anyhow::Result<OrgUnitsData> {
    let (units, counts) = crate::services::backend::admin_org_units_with_counts(id)?;
    let units = units
        .into_iter()
        .map(|u| Unit { accounts: counts.get(&u.id).copied().unwrap_or(0), id: u.id, name: u.name, description: u.description.unwrap_or_default(), parent: u.parent_id })
        .collect();
    Ok(OrgUnitsData { units })
}

/// The organisational units (see the module doc).
#[derive(UserControl, Default)]
#[user_control(view = "admin_org_units.kbcontrol")]
#[category("Kubuno")]
pub struct OrgUnitsSection {
    base: UserControlCore,
    /// What to show (bound by the console page).
    #[property(bindable, on_change = "state_changed")]
    #[category("Data")]
    pub state: Shared<SectionState<OrgUnitsData>>,
    /// Occurs when the section asks for the web console.
    #[event]
    #[category("Action")]
    pub command: Event<ItemCommandEventArgs>,
    #[property(bindable)]
    #[browsable(false)]
    pub units: Shared<Vec<Unit>>,
    #[property(bindable)]
    #[browsable(false)]
    pub count_text: String,
    #[property(bindable)]
    #[browsable(false)]
    pub status_mode: String,
    #[property(bindable)]
    #[browsable(false)]
    pub error_text: String,
}

impl OrgUnitsSection {
    fn state_changed(&mut self) {
        let state = self.state.clone();
        if let Some(data) = state.data.as_ref() {
            self.show(data);
        }
        self.error_text = state.error.clone();
        self.status_mode = StatusMode::of(!self.units.is_empty(), state.loading, &state.error).name().into();
    }

    /// Shows the units.
    pub fn show(&mut self, data: &OrgUnitsData) {
        let n = data.units.len();
        self.count_text = if n == 1 { Resources::units_count_one() } else { Resources::units_count() }.replace("{0}", &n.to_string());
        self.units = Shared::new(data.units.clone());
    }
}

#[kubuno_desktop::views::event_handlers]
impl OrgUnitsSection {
    fn org_units_section_load(&mut self) {
        if self.design_mode() && self.units.is_empty() {
            self.show(&OrgUnitsData { units: crate::controls::org_unit_tree::design_units() });
        }
    }

    fn new_unit_click(&mut self) {
        self.raise_command(ItemCommandEventArgs::new("open-web", "", "/admin/org-units?action=create"));
    }

    /// The pencil and « + » both land on the unit's page of the web console.
    fn tree_edit_requested(&mut self, e: &UnitEventArgs) {
        let path = format!("/admin/org-units/{}", e.id);
        self.raise_command(ItemCommandEventArgs::new("open-web", &e.id, &path));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_count_follows_the_units() {
        let mut s = OrgUnitsSection::default();
        s.show(&OrgUnitsData { units: crate::controls::org_unit_tree::design_units() });
        assert_eq!(s.count_text, Resources::units_count().replace("{0}", "5"));
        assert_eq!(s.units.len(), 5);
    }
}
