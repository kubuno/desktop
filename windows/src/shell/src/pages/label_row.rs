//! Code-behind of the user control `LabelRow` (`label_row.kbcontrol`): one label of the labels page,
//! the item template of its Repeater. The page sets its properties from the item's row
//! (`view_model::label_rows`); its button and swatches raise its events, which the page handles
//! knowing the item (`current_item`).

use kubuno::views::prelude::*;

use crate::model::view_model::PALETTE;

/// A swatch of the palette was clicked: its position in `view_model::PALETTE`.
#[derive(EventArgs, Debug, Clone, Default, PartialEq)]
pub struct ColourEventArgs {
    pub index: usize,
}

/// One label (see the module doc).
#[derive(UserControl, Default)]
#[user_control(view = "label_row.kbcontrol", default_event = "ColourRequested")]
#[category("Kubuno")]
pub struct LabelRow {
    base: UserControlCore,
    #[property(bindable)]
    #[category("Data")]
    pub label_name: String,
    /// The label's colour (`#rrggbb`); the palette rings it.
    #[property(bindable, on_change = "colour_changed")]
    #[category("Appearance")]
    pub colour: String,
    /// How many items carry it.
    #[property(bindable)]
    #[category("Data")]
    pub count: String,
    /// The user may rename, recolour and delete it.
    #[property(bindable, on_change = "can_manage_changed")]
    #[category("Behavior")]
    pub can_manage: bool,
    /// « Partagée par … » for a label someone else shared (empty for one's own).
    #[property(bindable, on_change = "shared_changed")]
    #[category("Data")]
    pub shared: String,
    /// The palette shows (the row is selected and the label manageable).
    #[property(bindable)]
    #[category("Behavior")]
    pub expanded: bool,
    #[property(bindable)]
    #[browsable(false)]
    pub is_shared: bool,
    /// The label is someone else's (no delete button: the count takes its place).
    #[property(bindable)]
    #[browsable(false)]
    pub is_read_only: bool,
    #[property(bindable)]
    #[browsable(false)]
    pub ring0: bool,
    #[property(bindable)]
    #[browsable(false)]
    pub ring1: bool,
    #[property(bindable)]
    #[browsable(false)]
    pub ring2: bool,
    #[property(bindable)]
    #[browsable(false)]
    pub ring3: bool,
    #[property(bindable)]
    #[browsable(false)]
    pub ring4: bool,
    #[property(bindable)]
    #[browsable(false)]
    pub ring5: bool,
    #[property(bindable)]
    #[browsable(false)]
    pub ring6: bool,
    #[property(bindable)]
    #[browsable(false)]
    pub ring7: bool,
    /// Occurs when the delete button is clicked.
    #[event]
    #[category("Action")]
    pub delete_requested: Event<EmptyEventArgs>,
    /// Occurs when a swatch of the palette is clicked.
    #[event]
    #[category("Action")]
    pub colour_requested: Event<ColourEventArgs>,
}

impl LabelRow {
    fn colour(&mut self, index: usize) {
        self.raise_colour_requested(ColourEventArgs { index });
    }

    /// Rings the palette's swatch of the label's colour.
    fn colour_changed(&mut self) {
        let rings: Vec<bool> = PALETTE.iter().map(|hex| self.colour.eq_ignore_ascii_case(hex)).collect();
        let fields = [&mut self.ring0, &mut self.ring1, &mut self.ring2, &mut self.ring3, &mut self.ring4, &mut self.ring5, &mut self.ring6, &mut self.ring7];
        for (field, on) in fields.into_iter().zip(rings) {
            *field = on;
        }
    }

    fn shared_changed(&mut self) {
        self.is_shared = !self.shared.is_empty();
    }

    fn can_manage_changed(&mut self) {
        self.is_read_only = !self.can_manage;
    }
}

#[kubuno::views::event_handlers]
impl LabelRow {
    fn label_row_load(&mut self) {
        if self.design_mode() && self.label_name.is_empty() {
            self.label_name = "Projet Atlas".into();
            self.colour = PALETTE[1].into();
            self.count = "24".into();
            self.can_manage = true;
            self.expanded = true;
            self.colour_changed();
        }
    }

    fn delete_click(&mut self) {
        self.raise_delete_requested(EmptyEventArgs);
    }

    fn swatch0_click(&mut self) {
        self.colour(0);
    }

    fn swatch1_click(&mut self) {
        self.colour(1);
    }

    fn swatch2_click(&mut self) {
        self.colour(2);
    }

    fn swatch3_click(&mut self) {
        self.colour(3);
    }

    fn swatch4_click(&mut self) {
        self.colour(4);
    }

    fn swatch5_click(&mut self) {
        self.colour(5);
    }

    fn swatch6_click(&mut self) {
        self.colour(6);
    }

    fn swatch7_click(&mut self) {
        self.colour(7);
    }
}
