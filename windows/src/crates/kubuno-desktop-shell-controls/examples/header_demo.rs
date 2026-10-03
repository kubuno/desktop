//! A small window whose title bar holds a Kubuno header cluster (`HeaderActions` left of the caption buttons:
//! bell, settings, help, waffle, avatar), smaller than
//! the menus they open: their popups extend beyond it, and stay on the screen when it sits at an
//! edge. The data are the designer's samples; nothing is launched or saved (the picks are logged).
//!
//! `cargo run -p kubuno-desktop-shell-controls --example header_demo [-- --dark]`

use std::rc::Rc;

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
        kubuno_desktop::tracing::info!("[header_demo] launch {id}");
    }
    fn save_favorites(&self, favorites: &[String]) {
        kubuno_desktop::tracing::info!("[header_demo] favourites {favorites:?}");
    }
}

/// Camille Martin and two other accounts (a live one, a dead session, another instance's); or, with
/// `--web-data`, the account the web's reference captures show (for side-by-side comparisons). The picks
/// are logged.
struct SampleAccounts {
    web: bool,
}

impl AccountService for SampleAccounts {
    fn user(&self) -> AccountUser {
        if self.web {
            return AccountUser { name: "MySQL Test".into(), email: "zzmysql-admin@kubuno.local".into(), initials: "MT".into(), avatar: None };
        }
        account_menu::design_data().0
    }
    fn accounts(&self) -> Vec<AccountEntry> {
        if self.web {
            return vec![AccountEntry { id: "bob".into(), name: "Bob O'Brien".into(), email: "zzmysql-bob@kubuno.local".into(), initials: Some("BO".into()), connected: true, ..AccountEntry::default() }];
        }
        let mut accounts = account_menu::design_data().1;
        if let Some(a) = accounts.get_mut(1) {
            a.connected = false;
        }
        accounts.push(AccountEntry { id: "remote".into(), name: "Camille (Mairie)".into(), email: "camille@mairie.fr".into(), server: "kubuno.mairie.fr".into(), connected: true, remote: true, ..AccountEntry::default() });
        accounts
    }
    fn can_administer(&self) -> bool {
        true
    }
    fn act(&self, action: AccountAction) {
        kubuno_desktop::tracing::info!("[header_demo] {action:?}");
    }
}

#[kubuno_desktop::view(xml = r#"<Panel Title="Header demo" DesignWidth="320" DesignHeight="200" BackColor="Background" TitleBarBackground="Background" TitleBarForeground="TextPrimary">
  <Label x:Name="hint" Text="The menus open outside this window, from its title bar." ForeColor="TextSecondary" X="16" Y="80" Width="288" Height="40"/>
  <HeaderActions x:Name="header" TitleBar.Region="Right" Compact="true" Width="160" Height="50"/>
</Panel>"#)]
struct HeaderDemo;

fn main() -> kubuno_desktop::Result {
    let dark = std::env::args().any(|a| a == "--dark");
    kubuno_desktop::Application::set_theme(if dark { kubuno_desktop::ui::Theme::dark() } else { kubuno_desktop::ui::Theme::light() });
    set_default_launcher(Rc::new(SampleApps));
    set_default_accounts(Rc::new(SampleAccounts { web: std::env::args().any(|a| a == "--web-data") }));
    let mut demo = HeaderDemo::default();
    demo.initialize_component();
    kubuno_desktop::Application::run(demo)
}
