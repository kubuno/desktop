//! Code-behind of the user control `ActivityPage` (`activity_page.kbcontrol`): the activity log. The
//! window gives it the rows (`view_model::activity_rows`) whenever the log or the clock moves.

use kubuno::prelude::Rows;
use kubuno::views::prelude::*;

/// The activity page (see the module doc).
#[derive(UserControl, Default)]
#[user_control(view = "activity_page.kbcontrol")]
#[category("Kubuno")]
pub struct ActivityPage {
    base: UserControlCore,
    /// The events, newest first (`Tone`, `Title`, `Body`, `Age`).
    #[property(bindable)]
    #[category("Data")]
    pub rows: Rows,
    #[property(bindable)]
    #[category("Behavior")]
    pub has_events: bool,
    #[property(bindable)]
    #[category("Behavior")]
    pub is_empty: bool,
}

impl ActivityPage {
    /// Shows `rows`.
    pub fn show(&mut self, rows: Rows) {
        self.has_events = !rows.is_empty();
        self.is_empty = rows.is_empty();
        self.rows = rows;
    }
}
