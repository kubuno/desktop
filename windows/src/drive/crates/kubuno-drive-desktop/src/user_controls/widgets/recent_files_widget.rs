//! RecentFilesWidget (mirrors RecentFilesWidget.xaml.cs) — the Home page
//! recent files rows.

use crate::services::storage::IconCache;
use crate::data::items::HomeModel;
use crate::ui::{
    Hot, Layout, Painter, Rect, UiState, GLYPH_CHEVRON_UP, GLYPH_DOCUMENT, ICON_ROW_DIP,
};

impl Painter<'_> {
    pub(crate) fn draw_recent_files(&self, layout: &Layout, state: &UiState, model: &HomeModel, icons: &IconCache, scale: f32) {
        let t = self.theme;
        let f = &self.renderer.formats;
        let pad = 24.0;

        if let Some(first) = layout.recent_rows.first() {
            let header = Rect::new(layout.content.left + pad, first.top - 44.0, layout.content.right - pad, first.top - 12.0);
            let chevron = Rect::new(header.left, header.top, header.left + 20.0, header.bottom);
            self.text(GLYPH_CHEVRON_UP, &chevron, &f.icon_small, &t.text_secondary, true);
            let title = Rect::new(header.left + 28.0, header.top, header.right, header.bottom);
            self.text(kubuno_drive_desktop_localization::tr("RecentFiles"), &title, &f.subtitle, &t.text_primary, false);
        }

        for (i, rect) in layout.recent_rows.iter().enumerate() {
            let item = &model.recent_files[i];
            // A recent row is a LIST row, not a card: `--radius-md` = 4
            // (`FilesRecentWidget.tsx:55` uses `hover:bg-surface-1`, but those
            // rows sit inside a white card, whereas these sit straight on the
            // page background where surface-1 would be invisible — the desktop
            // keeps its own surface-3 row tint here).
            if state.hot == Some(Hot::RecentRow(i)) {
                self.fill_rounded(
                    &rect.inflate(4.0, 0.0),
                    kubuno_drive_desktop_app_controls::themes::shape::radius::SM,
                    &t.control_fill_hover,
                );
            }
            let icon_rect = Rect::new(rect.left, rect.top, rect.left + 24.0, rect.bottom);
            match self.shell_icon(icons, &item.path, ICON_ROW_DIP, scale) {
                Some(bitmap) => self.image(bitmap, &icon_rect, 18.0),
                None => self.text(GLYPH_DOCUMENT, &icon_rect, &f.icon_small, &t.text_secondary, true),
            }

            let name_w = ((rect.right - rect.left) * 0.45).min(420.0);
            let name_rect = Rect::new(rect.left + 32.0, rect.top, rect.left + 32.0 + name_w, rect.bottom);
            self.text_ellipsis(&item.name, &name_rect, &f.body, &t.text_primary);

            let path_rect = Rect::new(name_rect.right + 16.0, rect.top, rect.right, rect.bottom);
            self.text_ellipsis(&item.path, &path_rect, &f.caption, &t.text_secondary);
        }
    }
}
