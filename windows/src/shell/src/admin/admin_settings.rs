//! Code-behind of the user control `InstanceSettingsSection` (`admin_settings.kbcontrol`, see its
//! comment): the instance's settings, read only. [`load`] reads them (`GET /api/v1/admin/settings`,
//! off the UI thread) and groups them by category, in a stable order (category, then key).

use kubuno_desktop::views::component::Shared;
use kubuno_desktop::views::prelude::*;
use kubuno_desktop::{Row, Rows, Value};

use crate::admin::SectionState;
use crate::admin::admin_dashboard::group_digits;
use crate::model::events::ItemCommandEventArgs;
use crate::controls::status_presenter::StatusMode;
use crate::Resources;

/// One setting, as the console shows it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SettingInfo {
    pub label: String,
    pub value: String,
    pub description: String,
}

/// One category and its settings.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SettingsGroup {
    pub title: String,
    pub settings: Vec<SettingInfo>,
}

/// The instance's settings, by category.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SettingsData {
    pub groups: Vec<SettingsGroup>,
}

/// A raw value as the console reads it: a switch as on/off, a whole number with grouped digits.
pub fn format_value(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::Bool(b) => if *b { Resources::isettings_on() } else { Resources::isettings_off() }.to_string(),
        serde_json::Value::Number(n) => n.as_i64().map(group_digits).unwrap_or_else(|| n.to_string()),
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Null => "—".to_string(),
        other => other.to_string(),
    }
}

/// A category's title: capitalised, « Divers » for the uncategorised.
fn category_title(cat: &str) -> String {
    let mut chars = cat.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => Resources::isettings_misc().to_string(),
    }
}

/// Groups `settings` by category (category, then key: an order that does not depend on the server's).
pub fn group(settings: &[kubuno_desktop_sync::AdminSetting]) -> Vec<SettingsGroup> {
    let mut order: Vec<&kubuno_desktop_sync::AdminSetting> = settings.iter().collect();
    order.sort_by(|a, b| a.category.as_deref().unwrap_or("").cmp(b.category.as_deref().unwrap_or("")).then_with(|| a.key.cmp(&b.key)));
    let mut groups: Vec<(String, SettingsGroup)> = Vec::new();
    for s in order {
        let cat = s.category.clone().unwrap_or_default();
        let info = SettingInfo {
            label: s.label.clone().filter(|l| !l.trim().is_empty()).unwrap_or_else(|| s.key.clone()),
            value: format_value(&s.value),
            description: s.description.clone().unwrap_or_default(),
        };
        match groups.last_mut() {
            Some((c, g)) if *c == cat => g.settings.push(info),
            _ => groups.push((cat.clone(), SettingsGroup { title: category_title(&cat), settings: vec![info] })),
        }
    }
    groups.into_iter().map(|(_, g)| g).collect()
}

/// Reads the settings of instance `id` (blocking: run it off the UI thread).
pub fn load(id: &str) -> anyhow::Result<SettingsData> {
    Ok(SettingsData { groups: group(&crate::services::backend::admin_settings(id)?) })
}

/// The categories as the Repeater's rows: each with its card's height and its settings.
pub fn rows(groups: &[SettingsGroup]) -> Rows {
    Rows::from(
        groups
            .iter()
            .map(|g| {
                let last = g.settings.len().saturating_sub(1);
                let settings = Rows::from(
                    g.settings
                        .iter()
                        .enumerate()
                        .map(|(i, s)| {
                            Row::new()
                                .with("Label", Value::Str(s.label.clone()))
                                .with("Value", Value::Str(s.value.clone()))
                                .with("Description", Value::Str(s.description.clone()))
                                .with("Ruled", Value::Bool(i < last))
                        })
                        .collect::<Vec<_>>(),
                );
                Row::new()
                    .with("Title", Value::Str(g.title.clone()))
                    .with("Height", Value::F32(crate::admin::settings_category::height(g.settings.len())))
                    .with("Settings", Value::List(settings))
            })
            .collect::<Vec<_>>(),
    )
}

/// The instance's settings (see the module doc).
#[derive(UserControl, Default)]
#[user_control(view = "admin_settings.kbcontrol")]
#[category("Kubuno")]
pub struct InstanceSettingsSection {
    base: UserControlCore,
    /// What to show (bound by the console page).
    #[property(bindable, on_change = "state_changed")]
    #[category("Data")]
    pub state: Shared<SectionState<SettingsData>>,
    /// Occurs when the section asks the window for something (it asks for nothing: read only).
    #[event]
    #[category("Action")]
    pub command: Event<ItemCommandEventArgs>,
    #[property(bindable)]
    #[browsable(false)]
    pub categories: Rows,
    #[property(bindable)]
    #[browsable(false)]
    pub status_mode: String,
    #[property(bindable)]
    #[browsable(false)]
    pub error_text: String,
}

impl InstanceSettingsSection {
    fn state_changed(&mut self) {
        let state = self.state.clone();
        if let Some(data) = state.data.as_ref() {
            self.categories = rows(&data.groups);
        }
        self.error_text = state.error.clone();
        self.status_mode = StatusMode::of(!self.categories.is_empty(), state.loading, &state.error).name().into();
    }
}

#[kubuno_desktop::views::event_handlers]
impl InstanceSettingsSection {
    fn instance_settings_section_load(&mut self) {
        if self.design_mode() && self.categories.is_empty() {
            let s = |label: &str, value: &str, description: &str| SettingInfo { label: label.into(), value: value.into(), description: description.into() };
            self.categories = rows(&[
                SettingsGroup { title: "Courriel".into(), settings: vec![s("Expéditeur des notifications", "no-reply@exemple.fr", "Adresse des courriels envoyés par l'instance")] },
                SettingsGroup { title: "Général".into(), settings: vec![s("Langue par défaut", "fr", "Pour les nouveaux comptes"), s("Nom de l'instance", "Kubuno Exemple", "Affiché dans l'en-tête et les courriels")] },
            ]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setting(key: &str, category: Option<&str>, value: serde_json::Value) -> kubuno_desktop_sync::AdminSetting {
        kubuno_desktop_sync::AdminSetting { key: key.into(), label: None, description: None, category: category.map(str::to_string), value, is_public: false }
    }

    #[test]
    fn settings_are_grouped_by_category_then_key() {
        let groups = group(&[
            setting("b", Some("general"), serde_json::json!(true)),
            setting("a", Some("general"), serde_json::json!(10_737_418_240_i64)),
            setting("z", None, serde_json::Value::Null),
            setting("c", Some("email"), serde_json::json!("x@y")),
        ]);
        let titles: Vec<&str> = groups.iter().map(|g| g.title.as_str()).collect();
        assert_eq!(titles, [Resources::isettings_misc(), "Email", "General"]);
        assert_eq!(groups[2].settings[0].value, "10\u{202F}737\u{202F}418\u{202F}240");
        assert_eq!(groups[2].settings[1].value, Resources::isettings_on());
        let r = rows(&groups);
        assert_eq!(r.get(2).map(|g| g.text("Height")), Some((crate::admin::settings_category::CARD_CHROME + 2.0 * crate::admin::settings_category::ROW_H).to_string()));
    }
}
