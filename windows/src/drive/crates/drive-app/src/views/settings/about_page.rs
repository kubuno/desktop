//! Port of `Files.App/Views/Settings/AboutPage.xaml` + `AboutViewModel`:
//! app version card, sponsor / help / feedback / open-source links with the
//! original `Constants.ExternalUrl` targets and the third-party libraries
//! grid (OpenSourceLibraries, verbatim).

use crate::ui::{Painter, Rect};
use crate::views::settings::controls::{Control, Icon, SettingId, SettingsRow};

/// Expander indices in `UiState::settings_expanded[SECTION_ABOUT]`.
pub const EXP_FEEDBACK: usize = 0;
pub const EXP_LIBRARIES: usize = 1;

/// App display name; the version mirrors `Package.Current.Id.Version` of the
/// original Files app this port is based on.
pub const APP_NAME: &str = "Drive";
pub const APP_VERSION: &str = "4.2.0.0";

/// External links (layout of `Constants.ExternalUrl`, repointed to Kubuno).
pub const GITHUB_REPO_URL: &str = "https://github.com/kubuno/drive";
pub const DOCUMENTATION_URL: &str = "https://github.com/kubuno/drive#readme";
pub const DISCORD_URL: &str = "https://github.com/kubuno/drive/discussions";
pub const FEATURE_REQUEST_URL: &str = "https://github.com/kubuno/drive/issues/new";
pub const BUG_REPORT_URL: &str = "https://github.com/kubuno/drive/issues/new?labels=bug";
pub const PRIVACY_POLICY_URL: &str = "https://github.com/kubuno/drive";
pub const SUPPORT_US_URL: &str = "https://github.com/kubuno";
/// Upstream project this port is based on (credited in the About page).
pub const CROWDIN_URL: &str = "https://crowdin.com/project/files-app";

/// `AboutViewModel.OpenSourceLibraries`, verbatim (url, name).
pub const OPEN_SOURCE_LIBRARIES: [(&str, &str); 21] = [
    ("https://github.com/omar/ByteSize", "ByteSize"),
    ("https://github.com/CommunityToolkit/dotnet", "CommunityToolkit.Mvvm"),
    ("https://github.com/DiscUtils/DiscUtils", "DiscUtils.Udf"),
    ("https://github.com/robinrodricks/FluentFTP", "FluentFTP"),
    ("https://github.com/libgit2/libgit2sharp", "libgit2sharp"),
    ("https://github.com/jeffijoe/messageformat.net", "MessageFormat"),
    ("https://github.com/dotnet/efcore", "EF Core for SQLite"),
    ("https://github.com/dotnet/runtime", "Microsoft.Extensions"),
    ("https://github.com/files-community/SevenZipSharp", "SevenZipSharp"),
    ("https://sourceforge.net/projects/sevenzip", "7zip"),
    ("https://github.com/ericsink/SQLitePCL.raw", "PCL for SQLite"),
    ("https://github.com/microsoft/WindowsAppSDK", "WindowsAppSDK"),
    ("https://github.com/microsoft/microsoft-ui-xaml", "WinUI 3"),
    ("https://github.com/microsoft/Win2D", "Win2D"),
    ("https://github.com/CommunityToolkit/Windows", "Windows Community Toolkit"),
    ("https://github.com/mono/taglib-sharp", "TagLibSharp"),
    ("https://github.com/microsoft/CsWin32", "CsWin32"),
    ("https://github.com/microsoft/CsWinRT", "CsWinRT"),
    ("https://github.com/GihanSoft/NaturalStringComparer", "NaturalStringComparer"),
    ("https://github.com/dongle-the-gadget/GuidRVAGen", "Dongle.GuidRVAGen"),
    ("https://github.com/leeqwind/PESignAnalyzer", "PESignAnalyzer"),
];

/// Libraries grid: UniformGridLayout MaximumRowsOrColumns=4, MinItemWidth=200.
pub const LIBS_COLUMNS: usize = 4;
pub const LIBS_CELL_H: f32 = 24.0;

pub fn libraries_panel_height() -> f32 {
    let rows = OPEN_SOURCE_LIBRARIES.len().div_ceil(LIBS_COLUMNS);
    rows as f32 * LIBS_CELL_H + 32.0
}

/// Cell rect of library `i` inside the panel (ItemsRepeater Margin=58,16,16,16).
pub fn library_cell_rect(panel: &Rect, i: usize) -> Rect {
    let cols = LIBS_COLUMNS;
    let width = (panel.right - 16.0 - (panel.left + 58.0)) / cols as f32;
    let col = i % cols;
    let row = i / cols;
    let left = panel.left + 58.0 + col as f32 * width;
    let top = panel.top + 16.0 + row as f32 * LIBS_CELL_H;
    Rect::new(left, top, left + width - 8.0, top + LIBS_CELL_H)
}

pub fn version_text() -> String {
    format!("{} {}", drive_localization::tr("SettingsAboutVersionTitle"), APP_VERSION)
}

pub fn rows(expanded: &[bool; 4]) -> Vec<SettingsRow> {
    let tr = drive_localization::tr;
    // App Info.
    let mut rows = vec![
        SettingsRow::card(
            SettingId::AbtAppInfo,
            Icon::Glyph("\u{E946}"),
            APP_NAME,
            Control::Button(tr("Copy").into()),
        )
        .with_description(version_text()),
    ];

    // GitHub Sponsor.
    rows.push(SettingsRow::card(
        SettingId::AbtSponsor,
        Icon::Glyph("\u{EB51}"),
        tr("SponsorUsOnGitHub"),
        Control::Action,
    ));

    // Help and support.
    rows.push(SettingsRow::group(tr("HelpAndSupport")));
    rows.push(SettingsRow::card(
        SettingId::AbtDocumentation,
        Icon::Glyph("\u{E736}"),
        tr("Documentation"),
        Control::Action,
    ));
    rows.push(SettingsRow::card(
        SettingId::AbtDiscussions,
        Icon::Glyph("\u{E8F2}"),
        tr("QuestionsAndDiscussions"),
        Control::Action,
    ));

    // Feedback.
    rows.push(SettingsRow::expander(
        SettingId::AbtFeedbackHeader,
        Icon::Glyph("\u{ED15}"),
        tr("Feedback"),
        Control::None,
        EXP_FEEDBACK,
        expanded[EXP_FEEDBACK],
    ));
    if expanded[EXP_FEEDBACK] {
        rows.push(SettingsRow::item(
            SettingId::AbtFeatureRequest,
            tr("SubmitFeatureRequest"),
            Control::Action,
        ));
        rows.push(SettingsRow::item(
            SettingId::AbtBugReport,
            tr("SubmitBugReport"),
            Control::Action,
        ));
    }

    // Open Log File Location.
    rows.push(SettingsRow::card(
        SettingId::AbtLogLocation,
        Icon::Glyph("\u{E838}"),
        tr("OpenLogLocation"),
        Control::Action,
    ));

    // Open Source.
    rows.push(SettingsRow::group(tr("OpenSource")));
    rows.push(SettingsRow::card(
        SettingId::AbtTranslate,
        Icon::Glyph("\u{F2B7}"),
        tr("ImproveTranslation"),
        Control::Action,
    ));

    // Third Party Licenses.
    rows.push(SettingsRow::expander(
        SettingId::AbtLibrariesHeader,
        Icon::Glyph("\u{E90F}"),
        tr("ThirdPartyLibraries"),
        Control::None,
        EXP_LIBRARIES,
        expanded[EXP_LIBRARIES],
    ));
    if expanded[EXP_LIBRARIES] {
        rows.push(SettingsRow::custom(SettingId::AbtLibrariesPanel, libraries_panel_height()));
    }

    // Open GitHub repo.
    rows.push(SettingsRow::card(
        SettingId::AbtGitHubRepo,
        Icon::Glyph("\u{E774}"),
        tr("OpenGitHubRepo"),
        Control::Action,
    ));

    // Privacy.
    rows.push(SettingsRow::card(
        SettingId::AbtPrivacy,
        Icon::Glyph("\u{E72E}"),
        tr("Privacy"),
        Control::Action,
    ));

    rows
}

impl Painter<'_> {
    /// Third-party libraries grid (ItemsRepeater of hyperlinks).
    pub(crate) fn draw_libraries_panel(&self, panel: &Rect) {
        let f = &self.renderer.formats;
        for (i, (_, name)) in OPEN_SOURCE_LIBRARIES.iter().enumerate() {
            let cell = library_cell_rect(panel, i);
            self.text(name, &cell, &f.body, &self.theme.accent, false);
        }
    }
}
