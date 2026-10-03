//! Code-behind of the user control `SettingsCategory` (`settings_category.kbcontrol`, see its comment):
//! one category of the instance's settings, an item of the settings section's Repeater.

use kubuno_desktop::views::prelude::*;
use kubuno_desktop::{Row, Rows, Value};

/// A setting row's height.
pub const ROW_H: f32 = 56.0;
/// What a dense card takes around its body: its frame, its title band and its body's padding.
pub const CARD_CHROME: f32 = 63.0;

/// How tall the card of a category with `rows` settings is.
pub fn height(rows: usize) -> f32 {
    CARD_CHROME + rows as f32 * ROW_H
}

/// One category of settings (see the module doc).
#[derive(UserControl, Default)]
#[user_control(view = "settings_category.kbcontrol")]
#[category("Kubuno")]
pub struct SettingsCategory {
    base: UserControlCore,
    /// The category's name (the card's title).
    #[property(bindable)]
    #[category("Data")]
    pub category_title: String,
    /// Its settings (fields `Label`, `Value`, `Description`, `Ruled`).
    #[property(bindable)]
    #[category("Data")]
    pub settings: Rows,
}

#[kubuno_desktop::views::event_handlers]
impl SettingsCategory {
    fn settings_category_load(&mut self) {
        if self.design_mode() && self.category_title.is_empty() && self.settings.is_empty() {
            self.category_title = "Général".into();
            let row = |label: &str, value: &str, description: &str, ruled: bool| {
                Row::new()
                    .with("Label", Value::Str(label.into()))
                    .with("Value", Value::Str(value.into()))
                    .with("Description", Value::Str(description.into()))
                    .with("Ruled", Value::Bool(ruled))
            };
            self.settings = Rows::from(vec![row("Langue par défaut", "fr", "Pour les nouveaux comptes", true), row("Nom de l'instance", "Kubuno Exemple", "Affiché dans l'en-tête et les courriels", false)]);
        }
    }
}
