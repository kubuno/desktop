//! A window whose title bar is a Kubuno header, made with the Form's own properties (vskubuno
//! `docs/SHELL-CONTROLS.md` §5): `ShowSearch`, `ShowNotifications`, `ShowSettings`, `ShowHelp`, `ShowWaffle`,
//! `ShowAccount` put the web header's standard items at the end of the title bar — the `HeaderActions` user control
//! this crate registers — and the free regions hold the app's own controls: a menu button on the left, a search
//! field in the centre. The data are the designer's samples; nothing is launched or saved (the picks are logged).
//!
//! `cargo run -p kubuno-desktop-shell-controls --example form_header [-- --dark] [--rtl] [--compact] [--minimal] [--accent]`
//! (`--compact`: the 50 DIP title bar instead of the 64 DIP header; `--minimal`: the waffle and the avatar only).

use std::rc::Rc;

use kubuno_desktop::prelude::*;
use kubuno_desktop_shell_controls::controls::{account_menu, app_tile_grid};
use kubuno_desktop_shell_controls::{set_default_accounts, set_default_launcher, AccountAction, AccountEntry, AccountService, AccountUser, LauncherService, Tile};

/// The sample instance's apps; a launch and a saved list are only logged.
struct SampleApps;

impl LauncherService for SampleApps {
    fn apps(&self) -> Vec<Tile> {
        app_tile_grid::design_tiles()
    }
    fn favorites(&self) -> Vec<String> {
        app_tile_grid::design_favorites()
    }
    fn launch(&self, id: &str) {
        kubuno_desktop::tracing::info!("[form_header] launch {id}");
    }
    fn save_favorites(&self, favorites: &[String]) {
        kubuno_desktop::tracing::info!("[form_header] favourites {favorites:?}");
    }
}

/// The designer's sample accounts; the picks are logged.
struct SampleAccounts;

impl AccountService for SampleAccounts {
    fn user(&self) -> AccountUser {
        account_menu::design_data().0
    }
    fn accounts(&self) -> Vec<AccountEntry> {
        account_menu::design_data().1
    }
    fn can_administer(&self) -> bool {
        true
    }
    fn act(&self, action: AccountAction) {
        kubuno_desktop::tracing::info!("[form_header] {action:?}");
    }
}

fn main() -> kubuno_desktop::Result {
    let flag = |name: &str| std::env::args().any(|a| a == name);
    kubuno_desktop::Application::set_theme(if flag("--dark") { kubuno_desktop::ui::Theme::dark() } else { kubuno_desktop::ui::Theme::light() });
    set_default_launcher(Rc::new(SampleApps));
    set_default_accounts(Rc::new(SampleAccounts));

    let form = Form::new().text("Kubuno Drive").client_size(1100.0, 440.0).start_position(StartPosition::CenterScreen);
    form.set_icon("HardDrive");
    if !flag("--compact") {
        // The web header's height (`h-16`).
        form.root().set_property("TitleBarHeight", 64.0);
    }
    if flag("--accent") {
        // A coloured band: the items take its ink, the avatar its pale accent tint.
        form.set_accent_color("Primary");
    }
    if flag("--rtl") {
        form.root().set_property("RightToLeftLayout", true);
    }
    let all = if flag("--minimal") { HeaderItems { waffle: true, account: true, ..HeaderItems::default() } } else { HeaderItems::ALL };
    form.set_header_items(all);
    form.set_unread_count(2);

    // The free regions: the app's own controls in the title bar.
    let menu = IconButton::new().name("menu").icon("Menu").size(36.0, 36.0).property("TitleBar.Region", "Left").property("ToolTip", "Menu");
    let search = TextField::new().name("search").placeholder("Rechercher dans Drive").size(480.0, 40.0).property("TitleBar.Region", "Center");
    // The page: what the items raised, and a switch that hides the waffle and the avatar live.
    let log = Label::new().name("log").text("Cliquez sur les boutons de l'en-tête.").bounds(24.0, 24.0, 600.0, 28.0);
    let toggle = Button::new().name("toggle").text("Masquer le gaufrier et l'avatar").variant("Secondary").bounds(24.0, 72.0, 280.0, 36.0);
    form.controls().add_range(&[&menu, &search, &log, &toggle]);

    let say = |log: &Label, text: &'static str| {
        let log = log.clone();
        move |_f: &Form, _e: &mut EventArgs| log.set_text(text)
    };
    form.search_clicked().subscribe(say(&log, "SearchClicked"));
    form.notifications_clicked().subscribe(say(&log, "NotificationsClicked"));
    form.settings_clicked().subscribe(say(&log, "SettingsClicked"));
    form.help_clicked().subscribe(say(&log, "HelpClicked"));
    let shown = std::cell::Cell::new(true);
    let me = form.clone();
    toggle.click().subscribe(move |button, _e| {
        let on = !shown.get();
        shown.set(on);
        me.set_header_items(HeaderItems { waffle: on, account: on, ..all });
        button.set_text(if on { "Masquer le gaufrier et l'avatar" } else { "Afficher le gaufrier et l'avatar" });
    });
    kubuno_desktop::Application::run(form)
}
