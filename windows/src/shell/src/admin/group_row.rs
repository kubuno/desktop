//! Code-behind of the user control `GroupRow` (`group_row.kbcontrol`, see its comment): one item of the
//! console's groups, a group's header or its details. Its chevron and its pencil raise
//! `ToggleRequested` and `EditRequested`, which the section handles knowing the item.

use kubuno::views::prelude::*;
use kubuno::Rows;

/// One group's header, or its details (see the module doc).
#[derive(UserControl, Default)]
#[user_control(view = "group_row.kbcontrol", default_event = "ToggleRequested")]
#[category("Kubuno")]
pub struct GroupRow {
    base: UserControlCore,
    /// `header` or `details`.
    #[property(bindable, on_change = "parts_changed")]
    #[category("Appearance")]
    pub kind: String,
    #[property(bindable)]
    #[category("Data")]
    pub group_name: String,
    #[property(bindable)]
    #[category("Data")]
    pub description: String,
    #[property(bindable)]
    #[category("Data")]
    pub is_default: bool,
    #[property(bindable)]
    #[category("Data")]
    pub is_system: bool,
    /// « 12 membres ».
    #[property(bindable)]
    #[category("Data")]
    pub count_text: String,
    /// Whether its details show (the chevron points down).
    #[property(bindable, on_change = "parts_changed")]
    #[category("Data")]
    pub expanded: bool,
    /// Its permissions (field `Label`), for the details.
    #[property(bindable)]
    #[category("Data")]
    pub permissions: Rows,
    /// « Créé le 2026-02-12 », for the details.
    #[property(bindable)]
    #[category("Data")]
    pub created_text: String,
    #[property(bindable)]
    #[browsable(false)]
    pub is_header: bool,
    #[property(bindable)]
    #[browsable(false)]
    pub is_details: bool,
    #[property(bindable)]
    #[browsable(false)]
    pub chevron_icon: String,
    /// Occurs when the chevron is clicked.
    #[event]
    #[category("Action")]
    pub toggle_requested: Event<EmptyEventArgs>,
    /// Occurs when the pencil is clicked.
    #[event]
    #[category("Action")]
    pub edit_requested: Event<EmptyEventArgs>,
}

impl GroupRow {
    fn parts_changed(&mut self) {
        self.is_details = self.kind == "details";
        self.is_header = !self.is_details;
        self.chevron_icon = if self.expanded { "ChevronDown" } else { "ChevronRight" }.into();
    }
}

#[kubuno::views::event_handlers]
impl GroupRow {
    fn group_row_load(&mut self) {
        if self.design_mode() && self.group_name.is_empty() && self.kind.is_empty() {
            self.group_name = "Utilisateurs".into();
            self.description = "Tous les comptes créés".into();
            self.is_default = true;
            self.count_text = "240 membres".into();
        }
        self.parts_changed();
    }

    fn chevron_click(&mut self) {
        self.raise_toggle_requested(EmptyEventArgs);
    }

    fn edit_click(&mut self) {
        self.raise_edit_requested(EmptyEventArgs);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_row_is_a_header_or_its_details() {
        let mut r = GroupRow::default();
        r.parts_changed();
        assert!(r.is_header && !r.is_details && r.chevron_icon == "ChevronRight");
        r.expanded = true;
        r.kind = "details".into();
        r.parts_changed();
        assert!(r.is_details && !r.is_header && r.chevron_icon == "ChevronDown");
    }
}
