//! DrivesWidget (mirrors DrivesWidget.xaml.cs) — the Home page drive cards
//! with usage gauge.

use kubuno_drive_desktop_app_controls::themes::shape;

use super::gauge_color;
use crate::services::storage::IconCache;
use crate::data::items::HomeModel;
use crate::ui::{
    Hot, Layout, Painter, Rect, UiState, GLYPH_CHEVRON_UP, GLYPH_DRIVE, ICON_CARD_DIP,
};

impl Painter<'_> {
    pub(crate) fn draw_drives(&self, layout: &Layout, state: &UiState, model: &HomeModel, icons: &IconCache, scale: f32) {
        let t = self.theme;
        let f = &self.renderer.formats;
        let pad = 24.0;

        if let Some(first) = layout.drive_cards.first() {
            let header = Rect::new(layout.content.left + pad, first.top - 44.0, layout.content.right - pad, first.top - 12.0);
            let chevron = Rect::new(header.left, header.top, header.left + 20.0, header.bottom);
            self.text(GLYPH_CHEVRON_UP, &chevron, &f.icon_small, &t.text_secondary, true);
            let title = Rect::new(header.left + 28.0, header.top, header.right, header.bottom);
            self.text(kubuno_drive_desktop_localization::tr("Drives"), &title, &f.subtitle, &t.text_primary, false);
        }

        for (i, rect) in layout.drive_cards.iter().enumerate() {
            let drive = &model.drives[i];
            let hot = state.hot == Some(Hot::DriveCard(i));
            // `@ui/Card.tsx:55` — see the note in `quick_access_widget`:
            // `rounded-xl` (8) `border border-border` `bg-surface-0`.
            let fill = if hot { &t.card_preview_background } else { &t.layer_background };
            self.fill_rounded(rect, shape::radius::XL, fill);
            self.stroke_rounded(rect, shape::radius::XL, &t.card_stroke);

            let icon_rect = Rect::new(rect.left + 8.0, rect.top, rect.left + 48.0, rect.bottom);
            let root = format!("{}:\\", drive.letter);
            match self.shell_icon(icons, &root, ICON_CARD_DIP, scale) {
                Some(bitmap) => self.image(bitmap, &icon_rect, 40.0),
                None => self.text(GLYPH_DRIVE, &icon_rect, &f.icon_large, &t.text_secondary, true),
            }

            let name_rect = Rect::new(rect.left + 56.0, rect.top + 8.0, rect.right - 8.0, rect.top + 28.0);
            self.text_ellipsis(&drive.display_name(), &name_rect, &f.body, &t.text_primary);

            // Usage gauge — the drive web's storage bar
            // (`FilesStorageGaugeHeader.tsx:37-41`): `h-1.5` = 6 DIP, not 4,
            // `rounded-full` on BOTH track and fill, `bg-black/10` track, and a
            // fill colour-coded by the ratio instead of a permanent accent.
            const BAR_H: f32 = 6.0;
            let bar_track = Rect::new(rect.left + 56.0, rect.top + 32.0, rect.right - 16.0, rect.top + 32.0 + BAR_H);
            self.fill_rounded(&bar_track, shape::pill(BAR_H), &t.drive_bar_track);
            let frac = drive.used_fraction();
            let used_w = (bar_track.right - bar_track.left) * frac;
            let bar_fill = Rect::new(bar_track.left, bar_track.top, bar_track.left + used_w, bar_track.bottom);
            self.fill_rounded(&bar_fill, shape::pill(BAR_H), &gauge_color(t, frac));

            let usage_rect = Rect::new(rect.left + 56.0, rect.top + 40.0, rect.right - 8.0, rect.bottom - 4.0);
            self.text(&drive.usage_text(), &usage_rect, &f.caption, &t.text_secondary, false);
        }
    }
}
