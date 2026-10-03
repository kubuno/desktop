//! Code-behind of the user control `SettingsPage` (`settings_page.kbcontrol`): the shell's settings.
//! The window shows the stored values with [`SettingsPage::show`] and persists what the page
//! raises (`SettingChanged`).

use kubuno::views::prelude::*;

use crate::model::events::SettingEventArgs;

/// What the settings page shows.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SettingsValues {
    pub theme: crate::services::settings::ThemeSetting,
    pub interval: u32,
    pub notifications: bool,
    pub autostart: bool,
    pub offline: bool,
    pub proxy: String,
}

/// The settings page (see the module doc).
#[derive(UserControl, Default)]
#[user_control(view = "settings_page.kbcontrol", default_event = "SettingChanged")]
#[category("Kubuno")]
pub struct SettingsPage {
    base: UserControlCore,
    /// `System`, `Light` or `Dark`.
    #[property(bindable)]
    #[category("Data")]
    pub theme: String,
    /// Minutes between two automatic sync cycles.
    #[property(bindable)]
    #[category("Data")]
    pub interval: f32,
    #[property(bindable)]
    #[category("Data")]
    pub notifications: bool,
    #[property(bindable)]
    #[category("Data")]
    pub autostart: bool,
    #[property(bindable)]
    #[category("Data")]
    pub offline: bool,
    #[property(bindable)]
    #[category("Data")]
    pub proxy: String,
    /// The proxy as last saved (a focus loss with nothing typed saves nothing).
    saved_proxy: String,
    /// Occurs when the user changes a setting: `theme`, `interval`, `notifications`, `autostart`,
    /// `offline`, `proxy`, with its new value as text.
    #[event]
    #[category("Action")]
    pub setting_changed: Event<SettingEventArgs>,
}

impl SettingsPage {
    /// Shows `v` (the proxy field is left alone while it holds an edit not saved yet).
    pub fn show(&mut self, v: &SettingsValues) {
        self.theme = v.theme.key().to_string();
        self.interval = v.interval as f32;
        self.notifications = v.notifications;
        self.autostart = v.autostart;
        self.offline = v.offline;
        if self.proxy == self.saved_proxy {
            self.proxy = v.proxy.clone();
        }
        self.saved_proxy = v.proxy.clone();
    }

    /// Saves the proxy field when it changed — when it loses the focus, on Enter, or when the page
    /// is left: there is no Save button, so an unsaved edit would be silently lost.
    pub fn commit_proxy(&mut self) {
        let typed = self.proxy.trim().to_string();
        if typed != self.saved_proxy {
            self.saved_proxy = typed.clone();
            self.raise_setting_changed(SettingEventArgs { name: "proxy".into(), value: typed });
        }
    }

    fn changed(&mut self, name: &str, value: String) {
        self.raise_setting_changed(SettingEventArgs { name: name.to_string(), value });
    }
}

#[kubuno::views::event_handlers]
impl SettingsPage {
    fn theme_checked_changed(&mut self, e: &CheckedChangedEventArgs) {
        // Each radio of the group reports its own change; the one turned on carries the choice.
        if e.new {
            let theme = self.theme.clone();
            self.changed("theme", theme);
        }
    }

    fn interval_value_changed(&mut self) {
        let minutes = (self.interval.round() as u32).clamp(crate::services::settings::INTERVAL_MIN, crate::services::settings::INTERVAL_MAX);
        self.changed("interval", minutes.to_string());
    }

    fn notifications_checked_changed(&mut self, e: &CheckedChangedEventArgs) {
        self.changed("notifications", e.new.to_string());
    }

    fn autostart_checked_changed(&mut self, e: &CheckedChangedEventArgs) {
        self.changed("autostart", e.new.to_string());
    }

    fn offline_checked_changed(&mut self, e: &CheckedChangedEventArgs) {
        self.changed("offline", e.new.to_string());
    }

    fn proxy_lost_focus(&mut self) {
        self.commit_proxy();
    }

    fn proxy_key_down(&mut self, e: &mut KeyEventArgs) {
        if e.key == Key(kubuno::controls::host::vk::ENTER) {
            self.commit_proxy();
            e.handled = true;
        }
    }
}
