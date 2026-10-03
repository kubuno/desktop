//! Code-behind of the user control `AccountsPage` (`accounts_page.kbcontrol`): the configured
//! accounts. The window gives it the rows (`view_model::account_rows`); the page raises
//! `AccountCommand` with the account's id.

use kubuno::prelude::Rows;
use kubuno::views::prelude::*;

use crate::model::events::ItemCommandEventArgs;

/// The accounts page (see the module doc).
#[derive(UserControl, Default)]
#[user_control(view = "accounts_page.kbcontrol", default_event = "AccountCommand")]
#[category("Kubuno")]
pub struct AccountsPage {
    base: UserControlCore,
    /// One row per configured instance (`Id`, `ActiveId`, `Title`, `Folder`, `Active`).
    #[property(bindable)]
    #[category("Data")]
    pub rows: Rows,
    /// The position of the current account among the rows.
    #[property(bindable)]
    #[category("Data")]
    pub active_index: f32,
    /// Occurs when the user acts on an account: `select`, `move`, `disconnect` (with its id), or
    /// `add`.
    #[event]
    #[category("Action")]
    pub account_command: Event<ItemCommandEventArgs>,
}

impl AccountsPage {
    /// Shows `rows`.
    pub fn show(&mut self, rows: Rows) {
        if !self.rows.same(&rows) {
            self.active_index = rows.iter().position(|r| r.get("Active") == Some(&Value::Bool(true))).map_or(-1.0, |i| i as f32);
            self.rows = rows;
        }
    }

    /// The id of the row that raised the current event.
    fn current_id() -> Option<String> {
        kubuno::views::binding::current_item().map(|item| item.row.text("Id"))
    }

    fn command(&mut self, command: &str, id: &str) {
        self.raise_account_command(ItemCommandEventArgs::new(command, id, ""));
    }
}

#[kubuno::views::event_handlers]
impl AccountsPage {
    fn list_item_click(&mut self, e: &ItemEventArgs) {
        if let Some(id) = self.rows.get(e.index).map(|r| r.text("Id")) {
            self.command("select", &id);
        }
    }

    fn row_move_requested(&mut self) {
        if let Some(id) = Self::current_id() {
            self.command("move", &id);
        }
    }

    fn row_disconnect_requested(&mut self) {
        if let Some(id) = Self::current_id() {
            self.command("disconnect", &id);
        }
    }

    fn add_click(&mut self) {
        self.command("add", "");
    }
}
