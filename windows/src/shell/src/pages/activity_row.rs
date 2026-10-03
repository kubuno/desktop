//! Code-behind of the user control `ActivityRow` (`activity_row.kbcontrol`): one event of the activity
//! page, the item template of its Repeater. Everything it shows comes from its row
//! (`view_model::activity_rows`), so it has no state of its own.

use kubuno::views::prelude::*;

/// One event of the activity log: its kind's glyph, its title and details, its age.
#[derive(UserControl, Default)]
#[user_control(view = "activity_row.kbcontrol")]
#[category("Kubuno")]
pub struct ActivityRow {
    base: UserControlCore,
}
