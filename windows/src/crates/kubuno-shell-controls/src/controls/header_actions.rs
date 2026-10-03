//! Code-behind of the user control `HeaderActions` (`header_actions.kbcontrol`, see its comment): the
//! standard right-hand cluster of a Kubuno header, as the web's `HeaderActions` lays it out.
//!
//! The waffle and the avatar are [`crate::WaffleButton`] and [`crate::AccountButton`]: they open their
//! menus in popups of their own and take their data from the thread's services
//! ([`crate::set_default_launcher`], [`crate::set_default_accounts`]). The other buttons raise events
//! the host acts on (`NotificationsClicked`, `SettingsClicked`, `HelpClicked`). `Minimal` keeps only the
//! waffle and the avatar (the web's search mode).

use kubuno::views::prelude::*;

/// What the counter on the bell says for `unread` notifications: nothing for none, `9+` above nine.
pub fn unread_text(unread: u32) -> String {
    match unread {
        0 => String::new(),
        1..=9 => unread.to_string(),
        _ => "9+".into(),
    }
}

/// The trailing gap between the avatar and the caption buttons (Office's).
pub const TRAILING_GAP: f32 = 8.0;

/// The width the cluster needs for the buttons it shows: 36 each (30 `compact`), 2 before the avatar, and
/// the trailing gap.
pub fn width_for(buttons: usize, account: bool, compact: bool) -> f32 {
    buttons as f32 * if compact { 30.0 } else { 36.0 } + if account { 2.0 } else { 0.0 } + TRAILING_GAP
}

/// The header's right-hand cluster (see the module doc).
#[derive(UserControl)]
#[user_control(view = "header_actions.kbcontrol")]
#[category("Kubuno")]
#[toolbox(icon = "panel-top")]
#[default_event("NotificationsClicked")]
pub struct HeaderActions {
    base: UserControlCore,
    /// Shows the notifications bell.
    #[property(bindable, on_change = "shown_changed")]
    #[default_value(true)]
    #[category("Appearance")]
    pub show_notifications: bool,
    /// Shows the settings button.
    #[property(bindable, on_change = "shown_changed")]
    #[default_value(true)]
    #[category("Appearance")]
    pub show_settings: bool,
    /// Shows the help button.
    #[property(bindable, on_change = "shown_changed")]
    #[default_value(true)]
    #[category("Appearance")]
    pub show_help: bool,
    /// Shows the apps waffle.
    #[property(bindable, on_change = "shown_changed")]
    #[default_value(true)]
    #[category("Appearance")]
    pub show_waffle: bool,
    /// Shows the account avatar.
    #[property(bindable, on_change = "shown_changed")]
    #[default_value(true)]
    #[category("Appearance")]
    pub show_account: bool,
    /// Keeps only the waffle and the avatar (the web's search mode).
    #[property(bindable, on_change = "shown_changed")]
    #[category("Appearance")]
    pub minimal: bool,
    /// Sizes the buttons to a normal title bar's caption buttons (30) instead of the 64-DIP header's 36.
    #[property(bindable, on_change = "shown_changed")]
    #[category("Appearance")]
    pub compact: bool,
    /// A right-to-left window: the cluster runs from the left (its caption buttons are on the left), the
    /// avatar nearest to them.
    #[property(bindable, on_change = "shown_changed")]
    #[category("Appearance")]
    pub right_to_left: bool,
    /// The avatar's tint (see `AccountButton.AvatarTint`): `Accent` on an accent-coloured title band.
    #[property(bindable)]
    #[default_value("Primary")]
    #[category("Appearance")]
    pub avatar_tint: String,
    #[property(bindable)]
    #[browsable(false)]
    pub flow_direction: String,
    /// The unread notifications the bell's counter shows (none: no counter).
    #[property(bindable, on_change = "unread_changed")]
    #[category("Data")]
    pub unread_count: f32,
    /// Occurs when the bell is clicked.
    #[event]
    #[category("Action")]
    pub notifications_clicked: Event<EmptyEventArgs>,
    /// Occurs when the settings button is clicked.
    #[event]
    #[category("Action")]
    pub settings_clicked: Event<EmptyEventArgs>,
    /// Occurs when the help button is clicked.
    #[event]
    #[category("Action")]
    pub help_clicked: Event<EmptyEventArgs>,
    #[property(bindable)]
    #[browsable(false)]
    pub unread_text: String,
    #[property(bindable)]
    #[browsable(false)]
    pub has_unread: bool,
    /// What the view binds: each button's visibility after `Minimal`.
    #[property(bindable)]
    #[browsable(false)]
    pub bell_shown: bool,
    #[property(bindable)]
    #[browsable(false)]
    pub large: bool,
    #[property(bindable)]
    #[browsable(false)]
    pub settings_shown: bool,
    #[property(bindable)]
    #[browsable(false)]
    pub help_shown: bool,
}

impl Default for HeaderActions {
    fn default() -> Self {
        Self {
            base: UserControlCore::default(),
            show_notifications: true,
            show_settings: true,
            show_help: true,
            show_waffle: true,
            show_account: true,
            minimal: false,
            compact: false,
            right_to_left: false,
            avatar_tint: "Primary".into(),
            flow_direction: "RightToLeft".into(),
            large: true,
            unread_count: 0.0,
            notifications_clicked: Event::default(),
            settings_clicked: Event::default(),
            help_clicked: Event::default(),
            unread_text: String::new(),
            has_unread: false,
            bell_shown: true,
            settings_shown: true,
            help_shown: true,
        }
    }
}

impl HeaderActions {
    /// The width the cluster needs now (what a host gives it).
    pub fn content_width(&self) -> f32 {
        let shown = [self.shows(self.show_notifications), self.shows(self.show_settings), self.shows(self.show_help), self.show_waffle, self.show_account];
        width_for(shown.iter().filter(|s| **s).count(), self.show_account, self.compact)
    }

    fn shows(&self, button: bool) -> bool {
        button && !self.minimal
    }

    fn shown_changed(&mut self) {
        self.large = !self.compact;
        self.flow_direction = if self.right_to_left { "LeftToRight" } else { "RightToLeft" }.into();
        self.bell_shown = self.shows(self.show_notifications);
        self.settings_shown = self.shows(self.show_settings);
        self.help_shown = self.shows(self.show_help);
        self.unread_changed();
    }

    fn unread_changed(&mut self) {
        let unread = self.unread_count.max(0.0) as u32;
        self.unread_text = unread_text(unread);
        self.has_unread = unread > 0 && self.bell_shown;
    }
}

#[kubuno::views::event_handlers]
impl HeaderActions {
    fn header_actions_load(&mut self) {
        if self.design_mode() && self.unread_count == 0.0 {
            self.unread_count = 3.0;
        }
        self.shown_changed();
    }

    fn bell_button_click(&mut self) {
        self.raise_notifications_clicked(EmptyEventArgs);
    }

    fn settings_button_click(&mut self) {
        self.raise_settings_clicked(EmptyEventArgs);
    }

    fn help_button_click(&mut self) {
        self.raise_help_clicked(EmptyEventArgs);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_counter_and_the_width_follow_what_shows() {
        assert_eq!(unread_text(0), "");
        assert_eq!(unread_text(5), "5");
        assert_eq!(unread_text(12), "9+");
        let mut h = HeaderActions::default();
        assert_eq!(h.content_width(), 190.0, "five buttons, the avatar's 2 and the trailing gap");
        h.compact = true;
        assert_eq!(h.content_width(), 160.0);
        h.compact = false;
        h.minimal = true;
        h.unread_count = 4.0;
        h.shown_changed();
        assert_eq!(h.content_width(), 82.0, "the waffle and the avatar");
        assert!(!h.has_unread, "no counter without its bell");
    }
}
