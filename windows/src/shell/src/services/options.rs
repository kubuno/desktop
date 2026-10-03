//! How the shell starts: the command line, read once by `main`.
//!
//! `kubuno-desktop.exe [--background] [--sample|--live] [--light|--dark] [--culture fr|en] [--no-splash]
//! [--page <page>|admin:<section>]`
//!
//! A Debug build started under a debugger (F5 in Visual Studio) runs the offline sample by default, so a
//! debugging session never reads or writes the real profile, nor registers the tray icon, the `Run` key or
//! the Explorer integration; `--live` opts out of it (a debugging session against the real account).

/// The command-line flag of a start at logon (the `Run` key's command, `settings::set_autostart`):
/// the shell starts hidden in the notification area, with no splash screen.
pub const BACKGROUND_FLAG: &str = "--background";

/// A page the command line asks for (`--page`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartPage {
    /// One of the shell's own pages.
    Page(crate::views::shell_window::Page),
    /// A section of the administration console, held until the identity says the account may
    /// enter it (`ShellWindow::open_pending_admin`).
    Admin(String),
}

/// What the command line asked for.
#[derive(Debug, Clone, Default)]
pub struct Options {
    /// Started at logon: hidden in the notification area until the user opens it, no splash.
    pub background: bool,
    /// The offline sample (`--sample`): fixed data, no server, no configuration read or written,
    /// nothing registered with the system (no tray icon, Explorer entry, Run key or sync).
    pub sample: bool,
    /// `--light` / `--dark`: the theme, whatever the settings say (the sample's default: the system).
    pub theme: Option<crate::services::settings::ThemeSetting>,
    /// `--culture fr|en`: the language of the views.
    pub culture: Option<String>,
    /// `--page launcher|settings|accounts|activity|labels|login|admin[:<section>]`.
    pub page: Option<StartPage>,
}

impl Options {
    /// Reads the process's command line (the sample by default in a Debug build under a debugger).
    pub fn from_args() -> Self {
        let debugging = cfg!(debug_assertions) && kubuno::controls::host::diagnostics::is_debugger_attached();
        Self::parse_for(std::env::args().skip(1), debugging)
    }

    /// Reads `args` as a start without a debugger would.
    pub fn parse(args: impl IntoIterator<Item = String>) -> Self {
        Self::parse_for(args, false)
    }

    /// Reads `args` (without the program name); `debugging`: a Debug build under a debugger, which runs the
    /// sample unless `--live` says otherwise. An unknown page is reported and ignored: this is a
    /// convenience, and it must never be the reason the shell fails to start.
    pub fn parse_for(args: impl IntoIterator<Item = String>, debugging: bool) -> Self {
        let args: Vec<String> = args.into_iter().collect();
        let value = |flag: &str| args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).cloned();
        let has = |flag: &str| args.iter().any(|a| a == flag);
        let theme = if has("--dark") {
            Some(crate::services::settings::ThemeSetting::Dark)
        } else if has("--light") {
            Some(crate::services::settings::ThemeSetting::Light)
        } else {
            None
        };
        Self {
            background: has(BACKGROUND_FLAG),
            sample: has("--sample") || (debugging && !has("--live")),
            theme,
            culture: value("--culture"),
            page: value("--page").and_then(|p| parse_page(&p)),
        }
    }
}

/// `--page`'s value.
pub fn parse_page(name: &str) -> Option<StartPage> {
    use crate::views::shell_window::Page;
    if let Some(section) = name.strip_prefix("admin:") {
        return Some(StartPage::Admin(section.to_string()));
    }
    Some(StartPage::Page(match name {
        "launcher" => Page::Launcher,
        "settings" => Page::Settings,
        "accounts" => Page::Accounts,
        "activity" => Page::Activity,
        "labels" => Page::Labels,
        "login" => Page::Login,
        "admin" => return Some(StartPage::Admin("dashboard".to_string())),
        other => {
            kubuno::tracing::warn!("[shell] unknown --page « {other} », staying on the launcher");
            return None;
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::views::shell_window::Page;

    fn parse(args: &[&str]) -> Options {
        Options::parse(args.iter().map(|s| s.to_string()))
    }

    #[test]
    fn the_command_line_is_read() {
        let o = parse(&["--background", "--sample", "--dark", "--culture", "en", "--page", "labels"]);
        assert!(o.background && o.sample);
        assert_eq!(o.theme, Some(crate::services::settings::ThemeSetting::Dark));
        assert_eq!(o.culture.as_deref(), Some("en"));
        assert_eq!(o.page, Some(StartPage::Page(Page::Labels)));
        let o = parse(&[]);
        assert!(!o.background && !o.sample && o.theme.is_none() && o.page.is_none());
    }

    #[test]
    fn a_debugging_session_runs_the_sample_unless_live() {
        let args = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(Options::parse_for(args(&[]), true).sample);
        assert!(!Options::parse_for(args(&["--live"]), true).sample);
        assert!(Options::parse_for(args(&["--sample", "--live"]), true).sample, "an explicit --sample wins");
        assert!(!Options::parse_for(args(&[]), false).sample);
    }

    #[test]
    fn admin_sections_wait_for_the_identity() {
        assert_eq!(parse_page("admin:storage"), Some(StartPage::Admin("storage".into())));
        assert_eq!(parse_page("admin"), Some(StartPage::Admin("dashboard".into())));
        assert_eq!(parse_page("login"), Some(StartPage::Page(Page::Login)));
        assert_eq!(parse_page("nowhere"), None);
    }
}
