//! Code-behind of the user control `ConversationRow` (`conversation_row.kbcontrol`): one conversation
//! of the list, the item template of `ConversationListPane`'s Repeater. Everything it shows comes
//! from the fields of its item's row (`view_model::conversation_row`), so it has no state of its own.

use kubuno_desktop::views::prelude::*;

/// One conversation of the list: avatar, name, time, last message and unread badge.
#[derive(UserControl, Default)]
#[user_control(view = "conversation_row.kbcontrol")]
#[category("Chat")]
pub struct ConversationRow {
    base: UserControlCore,
}
