//! Code-behind of the user control `AdminPage` (`admin_page.kbcontrol`, see its comment): the
//! administration console. The window opens a section ([`AdminPage::open`]) and hands it its state as it
//! loads ([`AdminPage::set_dashboard`]…); what a section asks for comes back as `Command`.

use kubuno::views::component::Shared;
use kubuno::views::prelude::*;

use crate::admin::{self, SectionState};
use crate::admin::admin_dashboard::DashboardData;
use crate::admin::admin_users::UsersData;
use crate::admin::admin_storage::StorageData;
use crate::admin::admin_settings::SettingsData;
use crate::admin::admin_modules::ModulesData;
use crate::admin::admin_org_units::OrgUnitsData;
use crate::admin::admin_audiences::AudiencesData;
use crate::admin::admin_groups::GroupsData;
use crate::model::events::{AdminCommandEventArgs, ItemCommandEventArgs};

/// The console (see the module doc).
#[derive(UserControl, Default)]
#[user_control(view = "admin_page.kbcontrol", default_event = "Command")]
#[category("Kubuno")]
pub struct AdminPage {
    base: UserControlCore,
    /// The section shown (a leaf id of [`admin::NAV`]).
    #[property(bindable, on_change = "section_changed")]
    #[category("Behavior")]
    pub section: String,
    /// Occurs when a section asks the window for something.
    #[event]
    #[category("Action")]
    pub command: Event<AdminCommandEventArgs>,
    #[property(bindable)]
    #[browsable(false)]
    pub section_title: String,
    #[property(bindable)]
    #[browsable(false)]
    pub not_ported: bool,
    #[property(bindable)]
    #[browsable(false)]
    pub on_dashboard: bool,
    #[property(bindable)]
    #[browsable(false)]
    pub dashboard: Shared<SectionState<DashboardData>>,
    #[property(bindable)]
    #[browsable(false)]
    pub on_users: bool,
    #[property(bindable)]
    #[browsable(false)]
    pub users: Shared<SectionState<UsersData>>,
    #[property(bindable)]
    #[browsable(false)]
    pub on_storage: bool,
    #[property(bindable)]
    #[browsable(false)]
    pub storage: Shared<SectionState<StorageData>>,
    #[property(bindable)]
    #[browsable(false)]
    pub on_settings: bool,
    #[property(bindable)]
    #[browsable(false)]
    pub settings: Shared<SectionState<SettingsData>>,
    #[property(bindable)]
    #[browsable(false)]
    pub on_modules: bool,
    #[property(bindable)]
    #[browsable(false)]
    pub modules: Shared<SectionState<ModulesData>>,
    #[property(bindable)]
    #[browsable(false)]
    pub on_org_units: bool,
    #[property(bindable)]
    #[browsable(false)]
    pub org_units: Shared<SectionState<OrgUnitsData>>,
    #[property(bindable)]
    #[browsable(false)]
    pub on_audiences: bool,
    #[property(bindable)]
    #[browsable(false)]
    pub audiences: Shared<SectionState<AudiencesData>>,
    #[property(bindable)]
    #[browsable(false)]
    pub on_groups: bool,
    #[property(bindable)]
    #[browsable(false)]
    pub groups: Shared<SectionState<GroupsData>>,
}

impl AdminPage {
    /// Shows section `id`.
    pub fn open(&mut self, id: &str) {
        self.section = id.to_string();
        self.section_changed();
    }

    fn section_changed(&mut self) {
        let id = self.section.as_str();
        self.section_title = admin::label(id);
        self.not_ported = !admin::is_native(id);
        self.on_dashboard = id == "dashboard";
        self.on_users = id == "users";
        self.on_storage = id == "storage";
        self.on_settings = id == "settings";
        self.on_modules = id == "modules";
        self.on_org_units = id == "org-units";
        self.on_audiences = id == "audiences";
        self.on_groups = id == "groups";
    }

    pub fn set_dashboard(&mut self, state: SectionState<DashboardData>) {
        self.dashboard = Shared::new(state);
    }

    pub fn set_users(&mut self, state: SectionState<UsersData>) {
        self.users = Shared::new(state);
    }

    pub fn set_groups(&mut self, state: SectionState<GroupsData>) {
        self.groups = Shared::new(state);
    }

    pub fn set_audiences(&mut self, state: SectionState<AudiencesData>) {
        self.audiences = Shared::new(state);
    }

    pub fn set_org_units(&mut self, state: SectionState<OrgUnitsData>) {
        self.org_units = Shared::new(state);
    }

    pub fn set_modules(&mut self, state: SectionState<ModulesData>) {
        self.modules = Shared::new(state);
    }

    pub fn set_settings(&mut self, state: SectionState<SettingsData>) {
        self.settings = Shared::new(state);
    }

    pub fn set_storage(&mut self, state: SectionState<StorageData>) {
        self.storage = Shared::new(state);
    }

    /// Section `id` starts loading: what it shows stays until the new data arrives.
    pub fn set_loading(&mut self, id: &str) {
        match id {
            "dashboard" => self.dashboard = Shared::new(SectionState::loading(self.dashboard.data.clone())),
            "users" => self.users = Shared::new(SectionState::loading(self.users.data.clone())),
            "storage" => self.storage = Shared::new(SectionState::loading(self.storage.data.clone())),
            "settings" => self.settings = Shared::new(SectionState::loading(self.settings.data.clone())),
            "modules" => self.modules = Shared::new(SectionState::loading(self.modules.data.clone())),
            "org-units" => self.org_units = Shared::new(SectionState::loading(self.org_units.data.clone())),
            "audiences" => self.audiences = Shared::new(SectionState::loading(self.audiences.data.clone())),
            "groups" => self.groups = Shared::new(SectionState::loading(self.groups.data.clone())),
            _ => {}
        }
    }

    fn raise(&mut self, command: &str, id: &str, value: &str) {
        let section = self.section.clone();
        self.raise_command(AdminCommandEventArgs { section, command: command.into(), id: id.into(), value: value.into() });
    }
}

#[kubuno::views::event_handlers]
impl AdminPage {
    fn admin_page_load(&mut self) {
        if self.section.is_empty() {
            self.section = admin::DEFAULT_SECTION.into();
        }
        self.section_changed();
    }

    fn section_command(&mut self, e: &ItemCommandEventArgs) {
        self.raise(&e.command, &e.id, &e.value);
    }

    fn not_ported_action(&mut self) {
        let path = admin::web_path(&self.section);
        self.raise("open-web", "", &path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_section_shows_natively_or_offers_the_browser() {
        let mut p = AdminPage::default();
        p.open("dashboard");
        assert!(p.on_dashboard && !p.not_ported);
        p.open("home");
        assert!(p.not_ported && !p.on_dashboard);
        assert_eq!(p.section_title, admin::label("home"));
    }
}
