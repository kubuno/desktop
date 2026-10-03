//! Code-behind of the user control `LauncherPage` (`launcher_page.kbcontrol`): the home page. It
//! keeps no logic of its own: the window computes what it shows (`view_model::launcher`) and
//! hands it over with [`LauncherPage::show`]; the page raises what the user asks for as `Command`.

use kubuno_desktop::views::prelude::*;

use crate::model::events::CommandEventArgs;
use crate::model::view_model::Launcher;

/// The home page (see the module doc).
#[derive(UserControl, Default)]
#[user_control(view = "launcher_page.kbcontrol", default_event = "Command")]
#[category("Kubuno")]
pub struct LauncherPage {
    base: UserControlCore,
    /// The sync dot's colour (a theme token).
    #[property(bindable)]
    #[category("Appearance")]
    pub sync_tone: String,
    /// « Vos fichiers sont à jour », « Synchronisation en cours… »…
    #[property(bindable)]
    #[category("Data")]
    pub sync_state: String,
    /// The last summary, else where the files live.
    #[property(bindable)]
    #[category("Data")]
    pub sync_detail: String,
    #[property(bindable)]
    #[category("Data")]
    pub sync_button_text: String,
    /// No sync is running.
    #[property(bindable)]
    #[category("Behavior")]
    pub can_sync: bool,
    #[property(bindable)]
    #[category("Appearance")]
    pub server_tone: String,
    #[property(bindable)]
    #[category("Data")]
    pub server_text: String,
    /// The switch says « En ligne » when on: off is the forced offline mode.
    #[property(bindable)]
    #[category("Data")]
    pub online: bool,
    #[property(bindable)]
    #[category("Data")]
    pub online_label: String,
    #[property(bindable)]
    #[category("Data")]
    pub online_description: String,
    /// Occurs when the user asks for something: `sync_now`, `open_folder`, `open_web`,
    /// `toggle_offline`.
    #[event]
    #[category("Action")]
    pub command: Event<CommandEventArgs>,
}

impl LauncherPage {
    /// Shows `l`.
    pub fn show(&mut self, l: &Launcher) {
        self.sync_tone = l.sync_tone.to_string();
        self.sync_state = l.sync_state.clone();
        self.sync_detail = l.sync_detail.clone();
        self.sync_button_text = l.sync_button.clone();
        self.can_sync = !l.syncing;
        self.server_tone = l.server_tone.to_string();
        self.server_text = l.server_text.clone();
        self.online = !l.offline;
        self.online_label = l.online_label.clone();
        self.online_description = l.online_description.clone();
    }
}

#[kubuno_desktop::views::event_handlers]
impl LauncherPage {
    fn launcher_page_load(&mut self) {
        if self.design_mode() {
            self.show(&crate::model::view_model::launcher(&crate::model::view_model::ShellState::design()));
        }
    }

    fn sync_button_click(&mut self) {
        self.raise_command(CommandEventArgs::new("sync_now"));
    }

    fn link_sync_click(&mut self) {
        self.raise_command(CommandEventArgs::new("sync_now"));
    }

    fn link_folder_click(&mut self) {
        self.raise_command(CommandEventArgs::new("open_folder"));
    }

    fn link_web_click(&mut self) {
        self.raise_command(CommandEventArgs::new("open_web"));
    }

    fn online_switch_checked_changed(&mut self, e: &CheckedChangedEventArgs) {
        // The window reads the engine back and shows the switch again; until then it shows what
        // the user asked for.
        self.online = e.new;
        self.raise_command(CommandEventArgs::new("toggle_offline"));
    }
}
