//! Port of `Files.App/Views/Settings/TagsPage.xaml` + `TagsViewModel`:
//! the file-tags list (FileTagsSettingsService.FileTagList) with the
//! original default tags. Creating/renaming/recoloring tags (ColorPicker
//! flyout) is not ported yet: the "Nouvelle étiquette", "Modifier" and
//! "Supprimer" buttons are shown but inert.

use crate::services::settings::AppSettings;
use crate::views::settings::controls::{Control, Icon, SettingId, SettingsRow};

/// Expander index in `UiState::settings_expanded[SECTION_TAGS]`
/// (IsExpanded="True" in the XAML).
pub const EXP_TAGS: usize = 0;

/// ListView rows are more compact than SettingsCards.
pub const TAG_ROW_H: f32 = 44.0;

pub fn rows(s: &AppSettings, expanded: &[bool; 4]) -> Vec<SettingsRow> {
    let tr = kubuno_drive_desktop_localization::tr;
    let mut rows = Vec::new();

    rows.push(SettingsRow::expander(
        SettingId::TagsHeader,
        Icon::Glyph("\u{E8EC}"),
        tr("FileTags"),
        Control::Button(tr("NewTag").into()),
        EXP_TAGS,
        expanded[EXP_TAGS],
    ));
    if expanded[EXP_TAGS] {
        for (i, tag) in s.file_tags.iter().enumerate() {
            let color = crate::services::settings::parse_color(&tag.color).unwrap_or(
                windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F {
                    r: 0.62,
                    g: 0.64,
                    b: 0.63,
                    a: 1.0,
                },
            );
            rows.push(
                SettingsRow::item(
                    SettingId::TagRow(i),
                    tag.name.clone(),
                    Control::Buttons(vec![tr("Edit").into(), tr("Delete").into()]),
                )
                .with_height(TAG_ROW_H),
            );
            // PathIcon App.Theme.PathIcon.FilledTag tinted with the tag color.
            if let Some(row) = rows.last_mut() {
                row.icon = Icon::VectorColored("FilledTag", color);
            }
        }
    }

    rows
}
