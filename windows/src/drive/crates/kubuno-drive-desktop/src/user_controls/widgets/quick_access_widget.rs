//! QuickAccessWidget (mirrors QuickAccessWidget.xaml.cs) — the Home page
//! "Quick access" cards.

use kubuno_drive_desktop_app_controls::themes::shape;

use crate::services::storage::IconCache;
use crate::data::items::{HomeModel, QuickAccessKind};
use crate::ui::{
    quick_access_glyph, Hot, Layout, Painter, Rect, UiState, GLYPH_CHEVRON_UP, GLYPH_PIN,
    ICON_CARD_DIP,
};

impl Painter<'_> {
    pub(crate) fn draw_quick_access(&self, layout: &Layout, state: &UiState, model: &HomeModel, icons: &IconCache, scale: f32) {
        let t = self.theme;
        let f = &self.renderer.formats;
        let pad = 24.0;
        let scroll = state.active().scroll;

        // The first widget hugs the top of the content area, as in
        // the original: `ListView` with no margin, `ListViewItem Margin="0,4"` and an
        // `Expander` header of ~32 DIP (cf. MainPage.xaml — the command
        // `Toolbar` only has `Margin="0,0,0,4"` before `PageContent`).
        // The "Quick access" header is only drawn if the widget is visible
        // (the Drives / Recent headers are already conditioned on the presence
        // of their cards further down).
        if crate::services::settings::get().show_quick_access_widget {
            let header = Rect::new(
                layout.content.left + pad,
                layout.content.top + 4.0 - scroll,
                layout.content.right - pad,
                layout.content.top + 32.0 - scroll,
            );
            let chevron = Rect::new(header.left, header.top, header.left + 20.0, header.bottom);
            self.text(GLYPH_CHEVRON_UP, &chevron, &f.icon_small, &t.text_secondary, true);
            let title = Rect::new(header.left + 28.0, header.top, header.right, header.bottom);
            self.text(kubuno_drive_desktop_localization::tr("QuickAccess"), &title, &f.subtitle, &t.text_primary, false);
        }

        for (i, rect) in layout.quick_cards.iter().enumerate() {
            let item = &model.quick_access[i];
            let hot = state.hot == Some(Hot::QuickCard(i));
            // `@ui/Card.tsx:55`: `rounded-xl border border-border bg-surface-0`
            // — `--radius-xl` is 8, not the 6 this used, and the surface is
            // WHITE, not surface-1 (which is all but the page background, so
            // the cards read as outlines only). Confirmed by the drive home's
            // own cards: `MobileHome.tsx:89`,
            // `rounded-xl border border-border bg-white`. Card.tsx declares no
            // hover, so the hover borrows `hover:bg-surface-2` — the tint the
            // system uses for a hovered surface (Tabs.tsx:121, Button ghost).
            let fill = if hot { &t.card_preview_background } else { &t.layer_background };
            self.fill_rounded(rect, shape::radius::XL, fill);
            self.stroke_rounded(rect, shape::radius::XL, &t.card_stroke);

            let icon_rect = Rect::new(rect.left, rect.top + 10.0, rect.right, rect.top + 52.0);
            match self.shell_icon(icons, &item.path, ICON_CARD_DIP, scale) {
                Some(bitmap) => self.image(bitmap, &icon_rect, 40.0),
                None if item.kind == QuickAccessKind::Generic => {
                    self.image(&self.renderer.images.folder, &icon_rect, 40.0)
                }
                None => {
                    let (glyph, color) = quick_access_glyph(item.kind);
                    self.text(glyph, &icon_rect, &f.icon_large, &color, true);
                }
            }

            let name_rect = Rect::new(rect.left + 6.0, rect.bottom - 34.0, rect.right - 6.0, rect.bottom - 8.0);
            // Name centered, truncated with "…" rather than clipped at the card edge.
            self.text_ellipsis_center(&item.name, &name_rect, &f.body, &t.text_primary);

            if item.is_pinned {
                let pin = Rect::new(rect.right - 26.0, rect.top + 4.0, rect.right - 6.0, rect.top + 24.0);
                self.text(GLYPH_PIN, &pin, &f.icon_small, &t.text_secondary, true);
            }
        }
    }
}
