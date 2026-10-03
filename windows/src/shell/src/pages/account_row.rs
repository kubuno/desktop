//! Code-behind of the user control `AccountRow` (`account_row.kbcontrol`): one account of the
//! accounts page, the item template of its Repeater. The page sets its properties from the item's
//! row (`<AccountRow Title="{Binding Title}" …/>`); its two buttons raise its events, which the
//! page handles knowing the item (`current_item`).

use kubuno_desktop::views::prelude::*;

/// One configured account (see the module doc).
#[derive(UserControl, Default)]
#[user_control(view = "account_row.kbcontrol", default_event = "MoveRequested")]
#[category("Kubuno")]
pub struct AccountRow {
    base: UserControlCore,
    /// The instance's id.
    #[property(bindable)]
    #[category("Data")]
    pub account_id: String,
    /// The current instance's id: the radio is on when it is this one.
    #[property(bindable)]
    #[category("Data")]
    pub active_id: String,
    /// Who is signed in, and on which server.
    #[property(bindable)]
    #[category("Data")]
    pub title: String,
    /// The local sync folder.
    #[property(bindable)]
    #[category("Data")]
    pub folder: String,
    /// Occurs when « Déplacer… » is clicked.
    #[event]
    #[category("Action")]
    pub move_requested: Event<EmptyEventArgs>,
    /// Occurs when « Déconnecter » is clicked.
    #[event]
    #[category("Action")]
    pub disconnect_requested: Event<EmptyEventArgs>,
}

#[kubuno_desktop::views::event_handlers]
impl AccountRow {
    fn account_row_load(&mut self) {
        if self.design_mode() && self.title.is_empty() {
            self.account_id = "sample-cloud".into();
            self.active_id = "sample-cloud".into();
            self.title = "Camille Martin — cloud.exemple.fr".into();
            self.folder = r"C:\Users\Camille\Kubuno".into();
        }
    }

    fn move_click(&mut self) {
        self.raise_move_requested(EmptyEventArgs);
    }

    fn disconnect_click(&mut self) {
        self.raise_disconnect_requested(EmptyEventArgs);
    }
}
