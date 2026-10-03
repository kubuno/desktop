//! Code-behind of the user control `LabelsPage` (`labels_page.kbcontrol`): the cross-module labels.
//! The window gives it the labels (`view_model::label_rows`) and what the fetch says
//! ([`LabelsPage::show`]); the page raises `LabelCommand` (`create` with the name, `delete` and
//! `colour` with the label's id and the palette colour).

use kubuno_desktop::prelude::Rows;
use kubuno_desktop::views::prelude::*;

use crate::model::events::ItemCommandEventArgs;
use crate::pages::label_row::ColourEventArgs;
use crate::controls::status_presenter::StatusMode;
use crate::model::view_model::PALETTE;

/// The labels page (see the module doc).
#[derive(UserControl, Default)]
#[user_control(view = "labels_page.kbcontrol", default_event = "LabelCommand")]
#[category("Kubuno")]
pub struct LabelsPage {
    base: UserControlCore,
    /// The labels shown, with the selected one's palette open.
    #[property(bindable)]
    #[category("Data")]
    pub rows: Rows,
    /// The selected label (its palette shows), -1 for none.
    #[property(bindable)]
    #[category("Data")]
    pub selected_index: f32,
    #[property(bindable)]
    #[category("Behavior")]
    pub has_labels: bool,
    /// What the status presenter says (`None`, `Loading`, `Empty`, `Error`).
    #[property(bindable)]
    #[category("Behavior")]
    pub status_mode: String,
    #[property(bindable)]
    #[category("Data")]
    pub error_text: String,
    /// The « new label » field.
    #[property(bindable)]
    #[category("Data")]
    pub new_name: String,
    #[property(bindable)]
    #[category("Behavior")]
    pub can_create: bool,
    /// The labels as the window gave them (without the selection's decorations).
    base_rows: Rows,
    /// Occurs when the user creates, deletes or recolours a label.
    #[event]
    #[category("Action")]
    pub label_command: Event<ItemCommandEventArgs>,
}

/// `rows` with the palette open on `selected`, when the label may be managed (the row rings the
/// label's current colour itself).
pub fn decorate(rows: &Rows, selected: Option<usize>) -> Rows {
    rows.iter()
        .enumerate()
        .map(|(i, r)| {
            let expanded = selected == Some(i) && r.get("CanManage") == Some(&Value::Bool(true));
            r.clone().with("Expanded", Value::Bool(expanded))
        })
        .collect()
}

impl LabelsPage {
    /// Shows the labels, and whether they are loading or failed.
    pub fn show(&mut self, rows: Rows, loading: bool, error: &str) {
        let mode = StatusMode::of(!rows.is_empty(), loading, error);
        self.status_mode = mode.name().to_string();
        self.error_text = error.to_string();
        self.has_labels = !rows.is_empty() && error.is_empty();
        if !self.base_rows.same(&rows) {
            self.base_rows = rows;
            if self.selected_index as usize >= self.base_rows.len() {
                self.selected_index = -1.0;
            }
            self.redecorate();
        }
    }

    fn selected(&self) -> Option<usize> {
        (self.selected_index >= 0.0).then_some(self.selected_index as usize)
    }

    fn redecorate(&mut self) {
        self.rows = decorate(&self.base_rows, self.selected());
    }

    fn current_id() -> Option<String> {
        kubuno_desktop::views::binding::current_item().map(|item| item.row.text("Id"))
    }

    fn command(&mut self, command: &str, id: &str, value: &str) {
        self.raise_label_command(ItemCommandEventArgs::new(command, id, value));
    }
}

#[kubuno_desktop::views::event_handlers]
impl LabelsPage {
    fn new_name_text_changed(&mut self) {
        self.can_create = !self.new_name.trim().is_empty();
    }

    fn create_click(&mut self) {
        let name = self.new_name.trim().to_string();
        if name.is_empty() {
            return;
        }
        self.new_name.clear();
        self.can_create = false;
        self.command("create", "", &name);
    }

    fn list_selection_changed(&mut self) {
        self.redecorate();
    }

    fn row_delete_requested(&mut self) {
        if let Some(id) = Self::current_id() {
            self.command("delete", &id, "");
        }
    }

    fn row_colour_requested(&mut self, e: &ColourEventArgs) {
        if let (Some(id), Some(colour)) = (Self::current_id(), PALETTE.get(e.index)) {
            self.command("colour", &id, colour);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kubuno_desktop::prelude::Row;

    #[test]
    fn the_selected_label_opens_its_palette() {
        let rows: Rows = vec![
            Row::new().with("Id", Value::Str("a".into())).with("Color", Value::Str("#1a73e8".into())).with("CanManage", Value::Bool(true)),
            Row::new().with("Id", Value::Str("b".into())).with("Color", Value::Str("#d93025".into())).with("CanManage", Value::Bool(false)),
        ]
        .into();
        let d = decorate(&rows, Some(0));
        assert_eq!(d[0].get("Expanded"), Some(&Value::Bool(true)));
        assert_eq!(d[1].get("Expanded"), Some(&Value::Bool(false)));
        let d = decorate(&rows, Some(1));
        assert_eq!(d[1].get("Expanded"), Some(&Value::Bool(false)), "a label shared by someone else has no palette");
    }
}
