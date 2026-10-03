//! Code-behind of the user control `AccountMenu` (`account_menu.kbcontrol`, see its comment): the
//! account panel every Kubuno desktop app can show under its header's avatar.
//!
//! Its data come from the host app, through an [`AccountService`] ([`AccountMenu::set_service`]) or
//! its `User` / `Accounts` / `ShowAdmin` properties. Every click raises an event and does nothing
//! else; the host acts on it (navigates, switches, asks before signing out) and closes its flyout.
//! The names are those of the web's `AccountMenu` (vskubuno `docs/SHELL-CONTROLS.md` §2);
//! `RemoveAccount` and `ChangeAvatar` are declared for that parity, but this panel has no « Supprimer »
//! nor camera button yet, so nothing raises them.

use std::rc::Rc;

use kubuno::views::component::Shared;
use kubuno::views::prelude::*;

use crate::controls::panel_menu::{MenuItemEventArgs, MenuRow, PanelMenu};
use crate::model::account::{greeting, AccountEntry, AccountService, AccountUser};
use crate::ShellControlsResources;

/// `w-80`.
pub const WIDTH: f32 = 320.0;
/// The pinned header: the panel's border, `pt-4`, a 20 line, `pb-2`.
const HEAD: f32 = 44.6;
/// Where the cards start in the body: `pt-2`, a 96 avatar, `mb-3`, a 32 greeting, `mb-3`, the
/// 33.1 pill, `pb-4`.
const CARDS_TOP: f32 = 209.1;
/// Between the accounts card and the actions card (`mb-2`).
const CARDS_GAP: f32 = 8.0;
/// The room under the last card: `pb-2` and the border.
const FOOT: f32 = 8.6;

/// `OpenAccount`, `RemoveAccount`: the account.
#[derive(kubuno::views::events::EventArgs, Debug, Clone, Default, PartialEq)]
pub struct AccountEventArgs {
    /// The account's id (one of the `Accounts`).
    pub id: String,
}

/// The rows of the actions card: « Ajouter un compte », « Étiquettes », « Administration » when
/// `admin`, « Se déconnecter ».
pub fn action_rows(admin: bool) -> Vec<MenuRow> {
    let row = |id: &str, icon: &'static str, label: &str| MenuRow { id: id.into(), icon, label: label.to_string(), sub: String::new() };
    let mut rows = vec![row("add", "UserPlus", ShellControlsResources::account_add()), row("labels", "Tags", ShellControlsResources::account_labels())];
    if admin {
        rows.push(row("admin", "Shield", ShellControlsResources::account_admin()));
    }
    rows.push(row("logout", "LogOut", ShellControlsResources::account_logout()));
    rows
}

/// The rows of the accounts card: each account's initial, its name over its server.
pub fn account_rows(accounts: &[AccountEntry]) -> Vec<MenuRow> {
    accounts.iter().map(|a| MenuRow { id: a.id.clone(), icon: "", label: a.name.clone(), sub: a.server.clone() }).collect()
}

/// The panel's content height for the other `accounts` (their card `expanded` or folded) and an
/// actions card listing the console when `admin` (what the host sizes its panel to, at most the room
/// it has).
pub fn content_height(accounts: &[AccountEntry], expanded: bool, admin: bool) -> f32 {
    let accounts = if accounts.is_empty() { 0.0 } else { crate::controls::accounts_card::card_height(accounts, expanded) + CARDS_GAP };
    HEAD + CARDS_TOP + accounts + PanelMenu::height_for(action_rows(admin).len()) + FOOT
}

/// The account panel (see the module doc).
#[derive(UserControl)]
#[user_control(view = "account_menu.kbcontrol")]
#[category("Kubuno")]
#[toolbox(icon = "circle-user")]
#[default_event("OpenAccount")]
pub struct AccountMenu {
    base: UserControlCore,
    /// The active account (a binding to a `Shared<AccountUser>`; or [`AccountMenu::set_service`]).
    #[property(bindable, on_change = "user_changed")]
    #[category("Data")]
    pub user: Shared<AccountUser>,
    /// The OTHER accounts (a binding to a `Shared<Vec<AccountEntry>>`).
    #[property(bindable, on_change = "accounts_changed")]
    #[category("Data")]
    pub accounts: Shared<Vec<AccountEntry>>,
    /// Lists « Administration » (the account may enter the console).
    #[property(bindable, on_change = "show_admin_changed")]
    #[category("Behavior")]
    pub show_admin: bool,
    /// The other accounts' card is unfolded (the web's default).
    #[property(bindable, on_change = "report_height")]
    #[default_value(true)]
    #[category("Behavior")]
    pub expanded: bool,
    /// A switch is under way: the accounts' rows take no click.
    #[property(bindable)]
    #[category("Behavior")]
    pub busy: bool,
    /// An upload of the photo is under way: the camera takes no click.
    #[property(bindable)]
    #[category("Behavior")]
    pub avatar_busy: bool,
    /// Occurs when « Gérer votre compte » is clicked.
    #[event]
    #[category("Action")]
    pub manage_account: Event<EmptyEventArgs>,
    /// Occurs when another account's row is clicked: the account to open.
    #[event]
    #[category("Action")]
    pub open_account: Event<AccountEventArgs>,
    /// Occurs when an account is to be removed from the list (« Supprimer » on a dead session or another instance's account).
    #[event]
    #[category("Action")]
    pub remove_account: Event<AccountEventArgs>,
    /// Occurs when « Ajouter un compte » is clicked.
    #[event]
    #[category("Action")]
    pub add_account: Event<EmptyEventArgs>,
    /// Occurs when « Étiquettes » is clicked.
    #[event]
    #[category("Action")]
    pub open_labels: Event<EmptyEventArgs>,
    /// Occurs when « Administration » is clicked.
    #[event]
    #[category("Action")]
    pub open_admin: Event<EmptyEventArgs>,
    /// Occurs when « Se déconnecter » is clicked.
    #[event]
    #[category("Action")]
    pub sign_out: Event<EmptyEventArgs>,
    /// Occurs when the photo is to be changed (the camera button on the avatar).
    #[event]
    #[category("Action")]
    pub change_avatar: Event<EmptyEventArgs>,
    /// Occurs when the close button or Escape asks to close the panel.
    #[event]
    #[category("Action")]
    pub close_requested: Event<EmptyEventArgs>,
    /// Occurs when the content's height changes (the accounts' card folded or unfolded).
    #[event]
    #[category("Layout")]
    pub content_height_changed: Event<crate::controls::waffle_menu::ContentHeightEventArgs>,
    #[property(bindable)]
    #[browsable(false)]
    pub email: String,
    #[property(bindable)]
    #[browsable(false)]
    pub user_name: String,
    #[property(bindable)]
    #[browsable(false)]
    pub initials: String,
    #[property(bindable)]
    #[browsable(false)]
    pub avatar_path: String,
    #[property(bindable)]
    #[browsable(false)]
    pub greeting: String,
    #[property(bindable)]
    #[browsable(false)]
    pub has_others: bool,
    #[property(bindable)]
    #[browsable(false)]
    pub action_rows: Shared<Vec<MenuRow>>,
    /// The last height `ContentHeightChanged` reported.
    reported: f32,
}

impl Default for AccountMenu {
    fn default() -> Self {
        Self {
            base: UserControlCore::default(),
            user: Shared::default(),
            accounts: Shared::default(),
            show_admin: false,
            expanded: true,
            busy: false,
            avatar_busy: false,
            manage_account: Event::default(),
            open_account: Event::default(),
            remove_account: Event::default(),
            add_account: Event::default(),
            open_labels: Event::default(),
            open_admin: Event::default(),
            sign_out: Event::default(),
            change_avatar: Event::default(),
            close_requested: Event::default(),
            content_height_changed: Event::default(),
            email: String::new(),
            user_name: String::new(),
            initials: String::new(),
            avatar_path: String::new(),
            greeting: String::new(),
            has_others: false,
            action_rows: Shared::default(),
            reported: 0.0,
        }
    }
}

impl AccountMenu {
    /// Takes the active account, the other accounts and the console's visibility from `service`.
    pub fn set_service(&mut self, service: Rc<dyn AccountService>) {
        self.user = Shared::new(service.user());
        self.accounts = Shared::new(service.accounts());
        self.show_admin = service.can_administer();
        self.refresh();
    }

    /// The height the content needs now, in DIP.
    pub fn content_height(&self) -> f32 {
        content_height(&self.accounts, self.expanded, self.show_admin)
    }

    /// What Escape does: raises `CloseRequested`.
    pub fn escape(&mut self) {
        self.raise_close_requested(EmptyEventArgs);
    }

    fn refresh(&mut self) {
        self.user_changed();
        self.accounts_changed();
        self.show_admin_changed();
    }

    fn user_changed(&mut self) {
        let user = self.user.clone();
        self.email = user.email.clone();
        self.user_name = user.name.clone();
        self.initials = user.initials.clone();
        self.avatar_path = user.avatar.clone().unwrap_or_default();
        self.greeting = greeting(ShellControlsResources::account_greeting(), &user.name);
    }

    fn accounts_changed(&mut self) {
        self.has_others = !self.accounts.is_empty();
        self.report_height();
    }

    fn show_admin_changed(&mut self) {
        self.action_rows = Shared::new(action_rows(self.show_admin));
        self.report_height();
    }

    fn report_height(&mut self) {
        let height = self.content_height();
        if (height - self.reported).abs() > 0.01 {
            self.reported = height;
            self.raise_content_height_changed(crate::controls::waffle_menu::ContentHeightEventArgs { height });
        }
    }
}

/// One account of the designer's sample (`design/accounts.json`).
#[derive(serde::Deserialize, Default)]
struct DesignAccount {
    #[serde(default)]
    id: String,
    name: String,
    email: String,
    #[serde(default)]
    server: String,
    #[serde(default)]
    initials: String,
}

#[derive(serde::Deserialize, Default)]
struct DesignData {
    user: DesignAccount,
    accounts: Vec<DesignAccount>,
    show_admin: bool,
}

/// What the designer shows: Camille Martin with two other accounts and the console
/// (`design/accounts.json`).
pub fn design_data() -> (AccountUser, Vec<AccountEntry>, bool) {
    let d: DesignData = serde_json::from_str(include_str!("design/accounts.json")).unwrap_or_default();
    let user = AccountUser { name: d.user.name, email: d.user.email, initials: d.user.initials, avatar: None };
    let accounts = d
        .accounts
        .into_iter()
        .map(|a| AccountEntry { id: a.id, name: a.name, email: a.email, server: a.server, initials: None, avatar: None, connected: true, remote: false, unread: 0 })
        .collect();
    (user, accounts, d.show_admin)
}

#[kubuno::views::event_handlers]
impl AccountMenu {
    fn account_menu_load(&mut self) {
        if self.design_mode() && self.user.name.is_empty() {
            let (user, accounts, admin) = design_data();
            self.user = Shared::new(user);
            self.accounts = Shared::new(accounts);
            self.show_admin = admin;
        }
        self.refresh();
    }

    fn close_button_click(&mut self) {
        self.raise_close_requested(EmptyEventArgs);
    }

    fn manage_click(&mut self) {
        self.raise_manage_account(EmptyEventArgs);
    }

    fn accounts_open_account(&mut self, e: &AccountEventArgs) {
        self.raise_open_account(e.clone());
    }

    fn accounts_remove_account(&mut self, e: &AccountEventArgs) {
        self.raise_remove_account(e.clone());
    }

    fn accounts_expanded_changed(&mut self, e: &crate::controls::accounts_card::ExpandedEventArgs) {
        self.expanded = e.expanded;
        self.report_height();
    }

    fn avatar_decor_camera_clicked(&mut self) {
        self.raise_change_avatar(EmptyEventArgs);
    }

    fn actions_item_clicked(&mut self, e: &MenuItemEventArgs) {
        match e.id.as_str() {
            "add" => self.raise_add_account(EmptyEventArgs),
            "labels" => self.raise_open_labels(EmptyEventArgs),
            "admin" => self.raise_open_admin(EmptyEventArgs),
            "logout" => self.raise_sign_out(EmptyEventArgs),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Sample {
        admin: bool,
        others: usize,
    }

    impl AccountService for Sample {
        fn user(&self) -> AccountUser {
            AccountUser { name: "Camille Martin".into(), email: "camille.martin@exemple.fr".into(), initials: "CM".into(), avatar: None }
        }
        fn accounts(&self) -> Vec<AccountEntry> {
            (0..self.others).map(|i| AccountEntry { id: format!("acc{i}"), name: format!("Compte {i}"), server: "kubuno.asso-exemple.org".into(), connected: true, ..AccountEntry::default() }).collect()
        }
        fn can_administer(&self) -> bool {
            self.admin
        }
    }

    fn menu(admin: bool, others: usize) -> AccountMenu {
        let mut m = AccountMenu::default();
        m.set_service(Rc::new(Sample { admin, others }));
        m
    }

    #[test]
    fn the_panel_lists_the_console_only_for_an_administrator() {
        let admin = menu(true, 1);
        let ids: Vec<&str> = admin.action_rows.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, ["add", "labels", "admin", "logout"]);
        // The web panel: the pinned head, the hero, the accounts' card (its toggle and one 64 row), 8, four
        // 56.6 actions and the foot.
        assert!((admin.content_height() - (44.6 + 209.1 + 44.0 + 64.0 + 8.0 + 4.0 * 56.6 + 8.6)).abs() < 0.2, "{}", admin.content_height());
        let plain = menu(false, 0);
        let ids: Vec<&str> = plain.action_rows.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, ["add", "labels", "logout"]);
        assert!(!plain.has_others);
        assert_eq!(plain.greeting, greeting(ShellControlsResources::account_greeting(), "Camille"));
        assert_eq!(plain.initials, "CM");
    }

    #[test]
    fn the_other_accounts_are_rows_of_their_name_over_their_server() {
        let m = menu(false, 2);
        assert!(m.has_others);
        assert!(m.content_height() > menu(false, 1).content_height());
        let mut m = m;
        let open = m.content_height();
        m.accounts_expanded_changed(&crate::controls::accounts_card::ExpandedEventArgs { expanded: false });
        assert!((open - m.content_height() - 2.0 * crate::controls::accounts_card::ROW_H).abs() < 0.01, "folded: the rows go");
    }

    #[test]
    fn the_designer_sample_has_two_other_accounts_and_the_console() {
        let (user, accounts, admin) = design_data();
        assert_eq!(user.name, "Camille Martin");
        assert_eq!(accounts.len(), 2);
        assert!(admin);
    }
}
