//! Port of `Files.App/Views/Settings/AdvancedPage.xaml` + `AdvancedViewModel`:
//! export/import/edit settings, Windows startup, background behavior and the
//! experimental feature flags. The dev-environment-only rows (replace the
//! Open dialog, thumbnail cache) are hidden like in the release build
//! (`x:Load="{x:Bind ViewModel.IsAppEnvironmentDev}"`).

use crate::services::settings::AppSettings;
use crate::views::settings::controls::{Control, Icon, SettingId, SettingsRow};

pub fn rows(s: &AppSettings, _expanded: &[bool; 4]) -> Vec<SettingsRow> {
    let tr = kubuno_drive_desktop_localization::tr;
    // Export.
    let mut rows = vec![SettingsRow::card(
        SettingId::AdvExportSettings,
        Icon::Glyph("\u{EDE1}"),
        tr("ExportSettings"),
        Control::Action,
    )];

    // Import.
    rows.push(SettingsRow::card(
        SettingId::AdvImportSettings,
        Icon::Glyph("\u{EDE2}"),
        tr("ImportSettings"),
        Control::Action,
    ));

    // Edit Settings File.
    rows.push(SettingsRow::card(
        SettingId::AdvEditSettingsFile,
        Icon::Glyph("\u{E8DA}"),
        tr("EditSettingsFile"),
        Control::Action,
    ));

    // Open on Windows startup.
    rows.push(SettingsRow::card(
        SettingId::AdvOpenOnStartup,
        Icon::Glyph("\u{E7E8}"),
        tr("SettingsOpenInLogin"),
        Control::Toggle(s.open_on_windows_startup),
    ));

    // Leave App Running.
    rows.push(SettingsRow::card(
        SettingId::AdvLeaveAppRunning,
        Icon::Glyph("\u{E8E6}"),
        tr("SettingsLeaveAppRunning"),
        Control::Toggle(s.leave_app_running),
    ));

    // System Tray Icon.
    rows.push(SettingsRow::card(
        SettingId::AdvSystemTrayIcon,
        Icon::Glyph("\u{E75B}"),
        tr("ShowSystemTrayIcon"),
        Control::Toggle(s.show_system_tray_icon),
    ));

    // Experimental Settings.
    rows.push(SettingsRow::group(tr("ExperimentalFeatureFlags")));

    // Replace File Explorer.
    rows.push(
        SettingsRow::card(
            SettingId::AdvSetAsDefaultFileManager,
            Icon::Glyph("\u{EC50}"),
            tr("SettingsSetAsDefaultFileManager"),
            Control::Toggle(s.is_set_as_default_file_manager),
        )
        .with_description(tr("SettingsSetAsDefaultFileManagerDescription")),
    );

    // Flatten options.
    rows.push(
        SettingsRow::card(
            SettingId::AdvFlattenOptions,
            Icon::Glyph("\u{E8B7}"),
            tr("ShowFlattenOptions"),
            Control::Toggle(s.show_flatten_options),
        )
        .with_description(tr("ShowFlattenOptionsDescription")),
    );

    rows
}
