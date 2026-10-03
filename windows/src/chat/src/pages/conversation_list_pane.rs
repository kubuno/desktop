//! Code-behind of the user control `ConversationListPane` (`conversation_list_pane.kbcontrol`): the
//! heading, the search field and the conversations. It keeps no chat logic: the window computes the
//! rows (`view_model::list_rows`) and gives them to it, and it reports what the user does.

use kubuno::prelude::Rows;
use kubuno::views::prelude::*;

/// The conversation list (see the module doc).
#[derive(UserControl, Default)]
#[user_control(view = "conversation_list_pane.kbcontrol", default_event = "ConversationSelected")]
#[category("Chat")]
pub struct ConversationListPane {
    base: UserControlCore,
    /// The heading ("Discussions", "Réunions").
    #[property(bindable)]
    #[category("Appearance")]
    pub title: String,
    /// The conversations shown, one `ConversationRow` each (see `view_model::conversation_row`).
    #[property(bindable)]
    #[category("Data")]
    pub rows: Rows,
    /// The position of the open conversation among the rows, -1 for none.
    #[property(bindable)]
    #[category("Data")]
    pub selected_index: f32,
    /// The search text.
    #[property(bindable)]
    #[category("Data")]
    pub filter: String,
    /// What the list says when it has no row.
    #[property(bindable)]
    #[category("Appearance")]
    pub empty_text: String,
    /// Occurs when a conversation is clicked: its position among the rows.
    #[event]
    #[category("Action")]
    pub conversation_selected: Event<ItemEventArgs>,
    /// Occurs when the search text changes.
    #[event]
    #[category("Action")]
    pub search_changed: Event<TextChangedEventArgs>,
}

impl ConversationListPane {
    /// Shows `rows` with the row `selected` (-1: none) highlighted.
    pub fn show_rows(&mut self, title: &str, rows: Rows, selected: i32, empty_text: &str) {
        self.title = title.to_string();
        if !self.rows.same(&rows) {
            self.rows = rows;
        }
        self.selected_index = selected as f32;
        self.empty_text = empty_text.to_string();
    }
}

#[kubuno::views::event_handlers]
impl ConversationListPane {
    fn list_item_click(&mut self, e: &ItemEventArgs) {
        self.raise_conversation_selected(ItemEventArgs { index: e.index });
    }

    fn search_text_changed(&mut self, e: &TextChangedEventArgs) {
        self.raise_search_changed(e.clone());
    }
}
