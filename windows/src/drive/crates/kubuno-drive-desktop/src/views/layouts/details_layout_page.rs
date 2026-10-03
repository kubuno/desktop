//! Port of `Files.App/Views/Layouts/DetailsLayoutPage.xaml`: rows with
//! sortable column headers. The header lives OUTSIDE the `ScrollViewer` —
//! rows scroll underneath it, hence the clipping.


use crate::services::storage::IconCache;
use crate::ui::{
    approx_text_width, file_columns_for, Hot, Layout, Painter, Rect, UiState, GLYPH_CHEVRON_UP,
    ICON_ROW_DIP,
};
use kubuno_drive_desktop_app_controls::themes::shape;

use super::file_glyph;

/// A row's corner radius (`--radius-sm`).
const RADIUS: f32 = shape::radius::SM;
/// The accent edge on a selected row (`border-l-[3px] border-primary`).
const SELECTION_BAR: f32 = 3.0;

impl Painter<'_> {
    pub(super) fn draw_details_layout(&self, layout: &Layout, state: &UiState, icons: &IconCache, scale: f32) {
        let t = self.theme;
        let f = &self.renderer.formats;
        let tab = state.active();
        let mode = tab.view_mode;
        let size = crate::view_models::shell_view_model::layout_size(mode);
        let icon_dip = crate::view_models::shell_view_model::icon_size_for(mode, size);
        // Shared by the header and every cell so a larger icon pushes the name
        // rather than being overlapped by it.
        let name_left = crate::view_models::shell_view_model::name_left_for(mode, size);

        // Column layout: name flexible | modified 140 | type 150 | size 100.
        // The Recycle Bin substitutes "Original path (widened) / Deletion
        // date".
        let recycle =
            tab.location == crate::view_models::shell_view_model::Location::RecycleBin;
        let header = &layout.file_header;
        let [modified_left, type_left, size_left] = file_columns_for(header, recycle);
        let defs = crate::view_models::shell_view_model::detail_columns(recycle);
        let lefts = [header.left + name_left, modified_left, type_left, size_left];
        let columns = [
            (kubuno_drive_desktop_localization::tr(defs[0].0), lefts[0], defs[0].1),
            (kubuno_drive_desktop_localization::tr(defs[1].0), lefts[1], defs[1].1),
            (kubuno_drive_desktop_localization::tr(defs[2].0), lefts[2], defs[2].1),
            (kubuno_drive_desktop_localization::tr(defs[3].0), lefts[3], defs[3].1),
        ];
        // `@ui/DataTable`: the header row is `bg-surface-1` with a
        // `border-b border-border` under it — the ONLY rule in the table.
        self.fill_rounded(header, 0.0, &t.card_background);
        for (i, (label, left, column)) in columns.iter().enumerate() {
            let zone_right = [modified_left, type_left, size_left, header.right][i];
            // `@ui/DataTable` sort button: hovering a header darkens the LABEL
            // (`hover:text-text-primary`) — it paints no background pill.
            let hot = state.hot == Some(Hot::FileHeaderCol(i));
            let label_color = if hot { t.text_primary } else { t.text_secondary };
            // `<th>`: `font-medium text-text-secondary` at `--kb-text-body`
            // (14), NOT the 12 px caption it used to be.
            self.text(label, &Rect::new(*left, header.top, zone_right, header.bottom), &f.body_strong, &label_color, false);
            if tab.sort_column == *column {
                // The active sort arrow is the only accented thing in the
                // header (`<ArrowUp size={12} className="text-primary" />`).
                let arrow = if tab.sort_ascending { GLYPH_CHEVRON_UP } else { "\u{E70D}" };
                let ax = left + approx_text_width(label, shape::text::BODY) + shape::space::XS;
                self.text(arrow, &Rect::new(ax, header.top, ax + 12.0, header.bottom), &f.icon_small, &t.accent, true);
            }
        }
        let divider = Rect::new(header.left, header.bottom, header.right, header.bottom + 1.0);
        self.fill_rounded(&divider, 0.0, &t.divider);

        // The `DetailsLayoutPage` header is OUTSIDE the `ScrollViewer`: rows
        // scroll underneath it, they never pass over it.
        let viewport = Rect::new(
            layout.content.left,
            divider.bottom,
            layout.content.right,
            layout.content.bottom,
        );
        unsafe {
            self.ctx.PushAxisAlignedClip(
                &viewport.d2d(),
                windows::Win32::Graphics::Direct2D::D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
            );
        }

        // Group headers: label in BodyStrong, count in secondary
        // (the `GroupSummary` of the layout pages).
        for (rect, label, count) in &layout.group_rows {
            if rect.bottom < viewport.top || rect.top > viewport.bottom {
                continue;
            }
            let text_rect = Rect::new(rect.left + 4.0, rect.top + 6.0, rect.right - 8.0, rect.bottom);
            self.text(label, &text_rect, &f.body_strong, &t.text_primary, false);
            let count_x = rect.left + 4.0 + approx_text_width(label, 14.0) + 12.0;
            let count_rect = Rect::new(count_x, rect.top + 6.0, rect.right - 8.0, rect.bottom);
            self.text(&format!("({count})"), &count_rect, &f.caption, &t.text_secondary, false);
        }

        for (i, rect) in layout.file_rows.iter().enumerate() {
            // Skip rows completely outside the viewport.
            if rect.bottom < viewport.top || rect.top > viewport.bottom {
                continue;
            }
            let entry = &tab.entries[i];
            let selected = tab.selected.contains(&i);
            if selected {
                // Web signature: `bg-[#e8f0fe]` + `border-l-[3px] border-primary`.
                self.fill_rounded(rect, RADIUS, &t.list_selected);
                let bar = Rect::new(rect.left, rect.top, rect.left + SELECTION_BAR, rect.bottom);
                self.fill_rounded(&bar, SELECTION_BAR / 2.0, &t.accent);
            } else if state.hot == Some(Hot::FileRow(i)) {
                self.fill_rounded(rect, RADIUS, &t.row_hover);
            }

            // The icon follows the layout's size (`GetIconSize`). It starts
            // past the 3 DIP selection edge, like the web's `px-4` gutter.
            let icon_rect =
                Rect::new(rect.left + 8.0, rect.top, rect.left + 8.0 + icon_dip, rect.bottom);
            // Folders get the flat monochrome glyph the web uses (`FolderGlyph`
            // at #5f6368) rather than the shell's yellow bitmap: in a Drive-like
            // listing every folder reads the same, and only FILES carry a
            // colourful type icon.
            if entry.is_dir {
                self.vector_icon("Folder", &icon_rect, icon_dip, &t.text_secondary);
            } else {
                match self.shell_icon(icons, &entry.path, icon_dip.max(ICON_ROW_DIP), scale) {
                    Some(bitmap) => self.image(bitmap, &icon_rect, icon_dip),
                    None => {
                        let (glyph, glyph_color) = file_glyph(&entry.name, entry.is_dir);
                        self.text(glyph, &icon_rect, &f.icon_small, &glyph_color.unwrap_or(t.text_secondary), true);
                    }
                }
            }

            let name_cell = Rect::new(rect.left + name_left, rect.top + 2.0, modified_left - 8.0, rect.bottom - 2.0);
            if let Some(edit) = state.edit.as_ref().filter(|e| e.entry == i) {
                kubuno_drive_desktop_app_controls::edit_box::draw_box(
                    self,
                    &name_cell,
                    &kubuno_drive_desktop_app_controls::EditView { text: &edit.text, caret: edit.caret, anchor: edit.anchor },
                );
            } else {
                // `FileRow`: `text-sm text-text-primary truncate`. The left
                // edge is the shared `name_left`, so a larger icon pushes the
                // name instead of being overlapped by it (a literal 36 broke
                // that as soon as the layout size changed).
                self.text_ellipsis(&super::display_name(entry), &Rect::new(rect.left + name_left, rect.top, modified_left - 8.0, rect.bottom), &f.body, &t.text_primary);
            }
            // Collapsed columns (narrow panes) draw nothing. In the Recycle
            // Bin, the cells follow the headers: original path, deletion
            // date (carried by `modified`).
            // Metadata cells: `FileRow` renders date and size as
            // `text-xs text-text-tertiary`, one step paler than the name.
            if modified_left < type_left - 8.0 {
                let cell = if recycle {
                    entry.original_path.clone().unwrap_or_default()
                } else {
                    entry.modified_text()
                };
                self.text_ellipsis(&cell, &Rect::new(modified_left, rect.top, type_left - 8.0, rect.bottom), &f.caption, &t.text_tertiary);
            }
            if type_left < size_left - 8.0 {
                let cell = if recycle { entry.modified_text() } else { entry.type_text() };
                self.text_ellipsis(&cell, &Rect::new(type_left, rect.top, size_left - 8.0, rect.bottom), &f.caption, &t.text_tertiary);
            }
            if size_left < rect.right - 4.0 {
                // The Size column is left-aligned like the others
                // (`ColumnContentTextBlock`, no TextAlignment), not right.
                self.text_ellipsis(&entry.size_text(), &Rect::new(size_left, rect.top, rect.right - 4.0, rect.bottom), &f.caption, &t.text_tertiary);
            }
        }
        unsafe {
            self.ctx.PopAxisAlignedClip();
        }
    }
}
