//! Port of `Files.App/UserControls/Pane/ShelfPane.xaml`: the Shelf pane —
//! a fixed-width card (240) with three rows:
//!   1. Header: the "Étagère" title (tertiary, centered) + a separator.
//!   2. Content: either the empty state (`EmptyShelf` illustration + `EmptyShelfText`),
//!      or the item list (`ShelfItemsList`: 16px icon + truncated name).
//!   3. Footer (only if populated): separator + "Effacer les éléments" link.
//!
//! The `EmptyShelf.48` illustration (a dedicated SVG) is rendered here via the enlarged
//! "Shelf" ThemedIcon geometry — the port doesn't embed the SVG yet.

use drive_app_controls::themes::shape;
use windows::Win32::Graphics::DirectWrite::DWRITE_TEXT_ALIGNMENT_CENTER;

use crate::ui::{Hot, Layout, Painter, Rect, UiState};
use crate::services::storage::IconCache;

impl Painter<'_> {
    pub(crate) fn draw_shelf_pane(&self, layout: &Layout, state: &UiState, icons: &IconCache, scale: f32) {
        let Some(pane) = &layout.shelf_pane else { return };
        let t = self.theme;
        let f = &self.renderer.formats;

        // The panel — same treatment as the Details pane beside it: a FLAT
        // `surface-0` surface at `--radius-xl` (8), no drop shadow. The web
        // stacks its panels on the page background, it never floats them.
        self.fill_rounded(pane, shape::radius::XL, &t.layer_background);

        // Header: centered "Étagère" title (TextFillColorTertiary → secondary
        // in the port) then a 1 DIP separator (Padding 12,12,12,4 + 8).
        let title = Rect::new(pane.left + 12.0, pane.top + 12.0, pane.right - 12.0, pane.top + 32.0);
        self.text(drive_localization::tr("Shelf"), &title, &f.body, &t.text_secondary, true);
        let divider = Rect::new(pane.left + 12.0, pane.top + 43.0, pane.right - 12.0, pane.top + 44.0);
        self.fill_rounded(&divider, 0.0, &t.divider);

        // Empty state: illustration + `EmptyShelfText`, centered vertically
        // (StackPanel VerticalAlignment="Center", Spacing="16").
        if state.shelf.is_empty() {
            let cx = (pane.left + pane.right) / 2.0;
            let cy = (pane.top + 44.0 + pane.bottom) / 2.0;
            let icon = Rect::new(cx - 24.0, cy - 48.0, cx + 24.0, cy);
            self.vector_icon("Shelf", &icon, 48.0, &t.text_secondary);
            let text = Rect::new(pane.left + 16.0, cy + 16.0, pane.right - 16.0, cy + 92.0);
            self.text_aligned(
                drive_localization::tr("EmptyShelfText"),
                &text,
                &f.caption_wrap,
                &t.text_secondary,
                DWRITE_TEXT_ALIGNMENT_CENTER,
            );
            return;
        }

        // Item list (`ShelfItemsList`).
        let size_px = (16.0 * scale).round() as i32;
        for (i, rect) in layout.shelf_items.iter().enumerate() {
            let Some((name, path, is_dir)) = state.shelf.get(i) else { break };
            let hovered = matches!(
                state.hot,
                Some(Hot::ShelfItem(j)) | Some(Hot::ShelfItemRemove(j)) if j == i
            );
            if hovered {
                self.fill_rounded(rect, shape::radius::SM, &t.control_fill_hover);
            }

            // 16×16 icon (shell thumbnail, falls back to the generic folder).
            let icon = Rect::new(rect.left + 4.0, rect.top + 10.0, rect.left + 20.0, rect.bottom - 10.0);
            match icons.get_thumbnail(path, size_px) {
                Some(bmp) => self.image(bmp, &icon, 16.0),
                None => {
                    let _ = is_dir;
                    self.image(&self.renderer.images.folder, &icon, 16.0)
                }
            }

            // Truncated name (`TextTrimming=CharacterEllipsis`), space reserved for the ×.
            let name_right = if hovered { rect.right - 30.0 } else { rect.right - 8.0 };
            let name_rect = Rect::new(rect.left + 28.0, rect.top, name_right, rect.bottom);
            self.text_ellipsis(name, &name_rect, &f.body, &t.text_primary);

            // × button on hover (`ShelfItem.Remove`).
            if hovered {
                let x = Rect::new(rect.right - 28.0, rect.top, rect.right, rect.bottom);
                if state.hot == Some(Hot::ShelfItemRemove(i)) {
                    self.fill_rounded(&x, shape::radius::SM, &t.control_fill_hover);
                }
                self.text("\u{E711}", &x, &f.icon_small, &t.text_secondary, true);
            }
        }

        // Footer: separator + "Effacer les éléments" — `@ui/Button.tsx` variant
        // "text" (Button.tsx:31): the accent without a fill, and
        // `hover:bg-primary-light` rather than a grey surface, at
        // `rounded-md` = 4.
        let clear = &layout.shelf_clear;
        let div_y = clear.top - 5.0;
        let fdiv = Rect::new(pane.left + 12.0, div_y, pane.right - 12.0, div_y + 1.0);
        self.fill_rounded(&fdiv, 0.0, &t.divider);
        if state.hot == Some(Hot::ShelfClear) {
            self.fill_rounded(clear, shape::radius::SM, &t.accent_light);
        }
        self.text(drive_localization::tr("ClearItems"), clear, &f.body, &t.accent, true);
    }
}
