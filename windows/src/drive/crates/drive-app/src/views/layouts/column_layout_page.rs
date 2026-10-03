//! ColumnLayoutPage (mirrors ColumnLayoutPage.xaml.cs)
//!
//! The list of ONE blade of the Columns layout: the rows, the icon, the
//! chevron leading to the next blade. The `BladeView` container that stacks
//! these blades is [`super::columns_layout_page`] (`ColumnsLayoutPage.xaml.cs`).

use crate::services::storage::IconCache;
use crate::ui::{Hot, Painter, Rect, UiState};
use crate::view_models::shell_view_model::ColumnPane;

use super::display_name;

/// A row's corner radius (`--radius-sm`).
const RADIUS: f32 = drive_app_controls::themes::shape::radius::SM;
/// The accent edge on a selected row.
const SELECTION_BAR: f32 = 3.0;

impl Painter<'_> {
    #[allow(clippy::too_many_arguments)] // a paint entry point: the frame's inputs, passed flat
    pub(super) fn draw_column_page(
        &self,
        column: &ColumnPane,
        rows: &[Rect],
        c: usize,
        content_top: f32,
        content_bottom: f32,
        state: &UiState,
        icons: &IconCache,
        icon_dip: f32,
        scale: f32,
    ) {
        let t = self.theme;
        let f = &self.renderer.formats;
        for (i, rect) in rows.iter().enumerate() {
            if rect.bottom < content_top || rect.top > content_bottom {
                continue;
            }
            let entry = &column.entries[i];
            if column.selected == Some(i) {
                // Same signature as the Details rows: `bg-[#e8f0fe]` plus the
                // 3 DIP accent edge (`border-l-[3px] border-primary`).
                self.fill_rounded(rect, RADIUS, &t.list_selected);
                let bar = Rect::new(rect.left, rect.top, rect.left + SELECTION_BAR, rect.bottom);
                self.fill_rounded(&bar, SELECTION_BAR / 2.0, &t.accent);
            } else if state.hot == Some(Hot::ColumnRow(c, i)) {
                self.fill_rounded(rect, RADIUS, &t.row_hover);
            }
            let cy = (rect.top + rect.bottom) / 2.0;
            let icon_rect = Rect::new(
                rect.left + 8.0,
                cy - icon_dip / 2.0,
                rect.left + 8.0 + icon_dip,
                cy + icon_dip / 2.0,
            );
            let size_px = (icon_dip * scale).round() as i32;
            match icons.get(&entry.path, size_px) {
                Some(b) => self.image(b, &icon_rect, icon_dip),
                None if entry.is_dir => self.image(&self.renderer.images.folder, &icon_rect, icon_dip),
                None => {
                    let (glyph, color) = super::file_glyph(&entry.name, entry.is_dir);
                    self.text(glyph, &icon_rect, &f.icon_small, &color.unwrap_or(t.text_secondary), true);
                }
            }
            // A folder shows the chevron leading to the next blade.
            let name_right = if entry.is_dir { rect.right - 20.0 } else { rect.right - 4.0 };
            let name = Rect::new(icon_rect.right + 10.0, rect.top, name_right, rect.bottom);
            self.text_ellipsis(&display_name(entry), &name, &f.body, &t.text_primary);
            if entry.is_dir {
                let chevron =
                    Rect::new(rect.right - 20.0, rect.top, rect.right - 4.0, rect.bottom);
                self.text(
                    crate::ui::GLYPH_CHEVRON_RIGHT,
                    &chevron,
                    &f.icon_small,
                    &t.text_secondary,
                    true,
                );
            }
        }
    }
}
