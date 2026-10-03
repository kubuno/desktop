//! Port of `Files.App/Views/Layouts/GridLayoutPage.xaml`.
//!
//! Like the original, this page hosts all THREE layouts — its three
//! `DataTemplate`s: `ListViewBrowserTemplate` (List), `CardsBrowserTemplate`
//! (Cards) and `GridViewBrowserTemplate` (Grid). The sizes come from
//! `GridLayoutPage.xaml.cs` (`RowHeightListView`, `ItemWidthGridView`,
//! `CardsViewIconBox*`…), ported in `shell_view_model`.

use crate::services::storage::IconCache;
use crate::view_models::shell_view_model::ViewMode;
use crate::ui::{Hot, Layout, Painter, Rect, UiState};
use kubuno_drive_desktop_app_controls::themes::shape;

use super::display_name;

/// A list row's corner radius (`--radius-sm`).
const ROW_RADIUS: f32 = shape::radius::SM;
/// A tile/card's corner radius (`--radius-xl`).
const CARD_RADIUS: f32 = shape::radius::XL;
/// The accent border of a selected tile.
const SELECTION_BORDER: f32 = 2.0;
/// `@ui/checkboxCanvas` `CHECKBOX_GEOMETRY`: an 18 DIP box with a 2 DIP border
/// at `--radius-sm`, holding an 11 DIP tick. The margin off the preview corner
/// is the template's own (6).
const CHECKBOX_SIZE: f32 = 18.0;
const CHECKBOX_BORDER: f32 = 2.0;
const CHECKBOX_MARGIN: f32 = 6.0;

impl Painter<'_> {
    /// The `SelectionCheckbox` of the Grid/Cards templates: top-left of the
    /// preview box, visible when selected or on hover
    /// (`UpdateCheckboxVisibility`), drawn as the web `@ui/Checkbox`.
    fn draw_selection_checkbox(&self, icon_box: &Rect, selected: bool, hot: bool) {
        if !selected && !hot {
            return;
        }
        let t = self.theme;
        let left = icon_box.left + CHECKBOX_MARGIN;
        let top = icon_box.top + CHECKBOX_MARGIN;
        let bx = Rect::new(left, top, left + CHECKBOX_SIZE, top + CHECKBOX_SIZE);
        if selected {
            // Checked: the box is FILLED and stroked with the accent, and the
            // tick is `mark: '#ffffff'` — 11 DIP inside an 18 DIP box, which
            // the 12 px glyph format matches.
            self.fill_rounded(&bx, ROW_RADIUS, &t.accent);
            self.stroke_rounded_w(&bx, ROW_RADIUS, &t.accent, CHECKBOX_BORDER);
            self.text("\u{E73E}", &bx, &self.renderer.formats.icon_small, &t.accent_foreground, true);
        } else {
            // Unchecked: `fill: null` in the web — but here the box sits over a
            // thumbnail, so it keeps the card surface to stay legible. Border =
            // `--color-border`, 2 DIP.
            self.fill_rounded(&bx, ROW_RADIUS, &t.card_background);
            self.stroke_rounded_w(&bx, ROW_RADIUS, &t.card_stroke, CHECKBOX_BORDER);
        }
    }

    pub(super) fn draw_grid_layout(&self, layout: &Layout, state: &UiState, icons: &IconCache, scale: f32) {
        let t = self.theme;
        let f = &self.renderer.formats;
        let tab = state.active();
        let mode = tab.view_mode;
        let size = crate::view_models::shell_view_model::layout_size(mode);
        let icon_dip = crate::view_models::shell_view_model::icon_size_for(mode, size);

        // Group headers (GroupStyle): label + count — for all THREE layouts
        // of the page, List returning early further below.
        for (rect, label, count) in &layout.group_rows {
            if rect.right < layout.content.left || rect.left > layout.content.right {
                continue;
            }
            let text_rect = Rect::new(rect.left + 4.0, rect.top + 6.0, rect.right - 8.0, rect.bottom);
            self.text(label, &text_rect, &f.body_strong, &t.text_primary, false);
            let count_x = rect.left + 4.0 + crate::ui::approx_text_width(label, 14.0) + 12.0;
            let count_rect = Rect::new(count_x, rect.top + 6.0, rect.right - 8.0, rect.bottom);
            self.text(&format!("({count})"), &count_rect, &f.caption, &t.text_secondary, false);
        }

        // List: `ItemsWrapGrid Orientation="Vertical"` — icon and name on one
        // row, items go down then wrap into a new column.
        if mode == ViewMode::List {
            for (i, rect) in layout.file_rows.iter().enumerate() {
                if rect.right < layout.content.left || rect.left > layout.content.right {
                    continue;
                }
                let entry = &tab.entries[i];
                let selected = tab.selected.contains(&i);
                if selected {
                    self.fill_rounded(rect, ROW_RADIUS, &t.list_selected);
                } else if state.hot == Some(Hot::FileRow(i)) {
                    self.fill_rounded(rect, ROW_RADIUS, &t.row_hover);
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
                let name = Rect::new(icon_rect.right + 10.0, rect.top, rect.right - 4.0, rect.bottom);
                self.text_ellipsis(&display_name(entry), &name, &f.body, &t.text_primary);
            }
            return;
        }


        // Grid and Cards: `ItemsWrapGrid Orientation="Horizontal"`.
        let cards = mode == ViewMode::Cards;
        for (i, rect) in layout.file_rows.iter().enumerate() {
            if rect.bottom < layout.content.top || rect.top > layout.content.bottom {
                continue;
            }
            let entry = &tab.entries[i];
            let selected = tab.selected.contains(&i);
            // Hovering the checkbox counts as hovering the item (it's part of it).
            let hot = matches!(state.hot, Some(Hot::FileRow(j) | Hot::FileCheckbox(j)) if j == i);
            // Kubuno tile, identical for Grid and Cards: a folder takes the
            // flat preview tint (#f1f3f4), a file the card surface (#f8f9fa),
            // both framed by `card_stroke` at `--radius-xl`. Selection tints
            // the whole tile and adds a 2 DIP accent border.
            let fill = if selected {
                if entry.is_dir { t.selected_folder } else { t.selected_card }
            } else if hot {
                t.row_hover
            } else if entry.is_dir {
                t.card_preview_background
            } else {
                t.card_background
            };
            self.fill_rounded(rect, CARD_RADIUS, &fill);
            self.stroke_rounded(rect, CARD_RADIUS, &t.card_stroke);
            if selected {
                self.stroke_rounded_w(rect, CARD_RADIUS, &t.accent, SELECTION_BORDER);
            }

            let size_px = (icon_dip * scale).round() as i32;
            let bitmap = icons.get_thumbnail(&entry.path, size_px);
            let name = display_name(entry);
            if cards {
                // Preview box (neutral fill) then details box, to the
                // right for the small card and below otherwise.
                let ((ibw, ibh), _) = crate::view_models::shell_view_model::card_boxes(size);
                let horizontal = crate::view_models::shell_view_model::cards_horizontal(size);
                let (icon_box, details) = if horizontal {
                    (
                        Rect::new(rect.left, rect.top, rect.left + ibw, rect.bottom),
                        Rect::new(rect.left + ibw, rect.top, rect.right, rect.bottom),
                    )
                } else {
                    (
                        Rect::new(rect.left, rect.top, rect.right, rect.top + ibh),
                        Rect::new(rect.left, rect.top + ibh, rect.right, rect.bottom),
                    )
                };
                // Neutral preview box, skipped when the card is tinted so the
                // selection/hover colour reads across the whole card.
                if !selected && !hot {
                    self.fill_rounded(&icon_box, 0.0, &t.card_preview_background);
                }
                match bitmap {
                    Some(b) => self.image(b, &icon_box, icon_dip),
                    None if entry.is_dir => self.image(&self.renderer.images.folder, &icon_box, icon_dip),
                    None => {
                        let (glyph, color) = super::file_glyph(&entry.name, entry.is_dir);
                        self.text(glyph, &icon_box, &f.icon_large, &color.unwrap_or(t.text_secondary), true);
                    }
                }
                // The template's `SelectionCheckbox`: top-left of the preview
                // box, margin 6 — visible when selected or on hover.
                self.draw_selection_checkbox(&icon_box, selected, hot);
                // `Padding="12,4,12,4"`: the name (BodyStrong) then the type.
                let name_rect =
                    Rect::new(details.left + 12.0, details.top + 4.0, details.right - 12.0, details.top + 30.0);
                self.text_ellipsis(&name, &name_rect, &f.body_strong, &t.text_primary);
                let type_rect =
                    Rect::new(details.left + 12.0, name_rect.bottom + 2.0, details.right - 12.0, name_rect.bottom + 24.0);
                self.text_ellipsis(&entry.type_text(), &type_rect, &f.caption, &t.text_secondary);
                // "Item Size": at the bottom of the details box (secondary
                // caption), like the template's bottom StackPanel.
                let size_text = entry.size_text();
                if !size_text.is_empty() {
                    let size_rect = Rect::new(
                        details.left + 12.0,
                        details.bottom - 24.0,
                        details.right - 12.0,
                        details.bottom - 4.0,
                    );
                    self.text(&size_text, &size_rect, &f.caption, &t.text_secondary, false);
                }
            } else {
                let tile = rect.right - rect.left;
                let icon_box = Rect::new(rect.left, rect.top, rect.right, rect.top + tile);
                match bitmap {
                    Some(b) => self.image(b, &icon_box, icon_dip),
                    None if entry.is_dir => self.image(&self.renderer.images.folder, &icon_box, icon_dip),
                    None => {
                        let (glyph, color) = super::file_glyph(&entry.name, entry.is_dir);
                        self.text(glyph, &icon_box, &f.icon_large, &color.unwrap_or(t.text_secondary), true);
                    }
                }
                self.draw_selection_checkbox(&icon_box, selected, hot);
                let name_rect =
                    Rect::new(rect.left + 4.0, rect.top + tile, rect.right - 4.0, rect.bottom - 8.0);
                // `TextWrapping="Wrap"` + `TextTrimming="CharacterEllipsis"`:
                // the name wraps onto at most two lines, the last one ending
                // with "…" if the name continues (the `ItemName` TextBlock
                // of `GridViewBrowserTemplate`).
                self.text_wrap_ellipsis(&name, &name_rect, &f.caption_wrap, &t.text_primary, 2, true);
            }
        }
    }
}
