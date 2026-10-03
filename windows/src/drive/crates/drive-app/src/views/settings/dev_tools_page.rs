//! Port of `Files.App/Views/Settings/DevToolsPage.xaml` + `DevToolsViewModel`:
//! "Open in IDE" status-bar button configuration and GitHub connection.
//! Editing the IDE name/path and the GitHub OAuth flow are not ported yet:
//! the values are shown read-only and "Connexion" is inert.

use crate::services::settings::{AppSettings, OpenInIDEOption};
use crate::views::settings::controls::{Control, Icon, SettingId, SettingsRow};

/// Expander index in `UiState::settings_expanded[SECTION_DEV_TOOLS]`.
pub const EXP_OPEN_IDE: usize = 0;

pub fn rows(s: &AppSettings, expanded: &[bool; 4]) -> Vec<SettingsRow> {
    let tr = drive_localization::tr;
    let mut rows = Vec::new();

    // Display Open IDE status bar button.
    rows.push(SettingsRow::expander(
        SettingId::DevOpenIdeHeader,
        Icon::Glyph("\u{E7AC}"),
        tr("DisplayOpenIDE"),
        Control::Combo(tr(s.open_in_ide_option.tr_key()).into()),
        EXP_OPEN_IDE,
        expanded[EXP_OPEN_IDE],
    ));
    if expanded[EXP_OPEN_IDE] {
        let (ide_path, ide_name) = crate::services::settings::ide_display(s);
        rows.push(
            SettingsRow::item(SettingId::DevIdeName, tr("Name"), Control::Text(ide_name))
                .with_height(48.0),
        );
        rows.push(
            SettingsRow::item(SettingId::DevIdePath, tr("PathOrAlias"), Control::Text(ide_path))
                .with_height(48.0),
        );
    }

    // Connect to GitHub (logged-out variant: no saved credentials in the port).
    rows.push(SettingsRow::card(
        SettingId::DevGitHub,
        Icon::Glyph("\u{F0B9}"),
        tr("ConnectToGitHub"),
        Control::Button(tr("Login").into()),
    ));

    rows
}

pub fn open_in_ide_options(current: OpenInIDEOption) -> Vec<(&'static str, bool)> {
    OpenInIDEOption::ALL
        .iter()
        .map(|v| (drive_localization::tr(v.tr_key()), *v == current))
        .collect()
}
