//! BaseLayoutPage (mirrors BaseLayoutPage.cs)
//!
//! The role of `BaseLayoutPage`: the dispatch (`draw_files`: each
//! `ViewMode` to its page) and what all pages share (display name,
//! fallback glyph, unfocused pane).
//!
//! `BaseGroupableLayoutPage.cs` and `IBaseLayoutPage.cs` have no dedicated
//! Rust counterpart; `draw_other_pane` (dual-pane pane) is shell infra with
//! no 1:1 `.cs` file (port-only).

use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;
use windows::Win32::Graphics::Direct2D::D2D1_ANTIALIAS_MODE_PER_PRIMITIVE;

use crate::services::storage::IconCache;
use crate::view_models::shell_view_model::{Location, ViewMode};
use crate::ui::{
    file_columns, Layout, Painter, Rect, UiState, GLYPH_DOCUMENT, GLYPH_FOLDER, GLYPH_MUSIC,
    GLYPH_PICTURE, GLYPH_VIDEO, ICON_ROW_DIP,
};

/// Fallback glyph when neither a thumbnail nor a shell icon is available.
pub(in crate::views) fn file_glyph(name: &str, is_dir: bool) -> (&'static str, Option<D2D1_COLOR_F>) {
    use kubuno_drive_desktop_shared::helpers::file_extensions as fe;
    if is_dir {
        // The web's `FolderGlyph` paints folders #5f6368 (no colour of their
        // own) — that is exactly `text_secondary`, the callers' fallback.
        return (GLYPH_FOLDER, None);
    }
    let n = Some(name);
    if fe::is_image_file(n) {
        (GLYPH_PICTURE, None)
    } else if fe::is_video_file(n) {
        (GLYPH_VIDEO, None)
    } else if fe::is_audio_file(n) {
        (GLYPH_MUSIC, None)
    } else {
        (GLYPH_DOCUMENT, None)
    }
}

/// The name as displayed: without extension if `ShowFileExtensions` is false
/// (and never truncated for a folder).
pub(in crate::views) fn display_name(entry: &crate::data::items::DirEntryItem) -> String {
    let show_ext = crate::services::settings::get().show_file_extensions;
    if !show_ext && !entry.is_dir {
        std::path::Path::new(&entry.name)
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| entry.name.clone())
    } else {
        entry.name.clone()
    }
}

impl Painter<'_> {
    /// The `BaseLayoutPage` dispatch: each `ViewMode` to its page.
    pub(crate) fn draw_files(&self, layout: &Layout, state: &UiState, icons: &IconCache, scale: f32) {
        let t = self.theme;
        let f = &self.renderer.formats;
        let tab = state.active();

        if let Some(error) = &tab.load_error {
            let rect = Rect::new(layout.content.left, layout.content.top + 60.0, layout.content.right, layout.content.top + 100.0);
            self.text(error, &rect, &f.body, &t.text_secondary, true);
            return;
        }
        if tab.entries.is_empty() && tab.view_mode != ViewMode::Columns {
            let rect = Rect::new(layout.content.left, layout.content.top + 60.0, layout.content.right, layout.content.top + 100.0);
            self.text("Ce dossier est vide.", &rect, &f.body, &t.text_secondary, true);
            return;
        }

        match tab.view_mode {
            ViewMode::Grid | ViewMode::Cards | ViewMode::List => {
                self.draw_grid_layout(layout, state, icons, scale)
            }
            ViewMode::Columns => self.draw_columns_layout(layout, state, icons, scale),
            ViewMode::Details => self.draw_details_layout(layout, state, icons, scale),
        }
    }

    /// Read-only rendering of the unfocused pane (dual-pane mode).
    pub(crate) fn draw_other_pane(&self, layout: &Layout, state: &UiState, icons: &IconCache, scale: f32) {
        let t = self.theme;
        let f = &self.renderer.formats;
        let Some(rect) = &layout.other_pane_rect else {
            return;
        };
        let Some(other) = state.group().other() else {
            return;
        };

        if let Some(divider) = &layout.pane_divider {
            self.fill_rounded(divider, 0.0, &t.divider);
        }
        // Subtle backing + accent edge on the FOCUSED side of the divider.
        self.fill_rounded(&rect.inflate(-2.0, -2.0), 8.0, &t.control_fill_pressed);

        unsafe {
            self.ctx.PushAxisAlignedClip(&rect.d2d(), D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
        }
        if matches!(other.location, Location::Dir(_)) {
            let header = &layout.other_header;
            let [modified_left, _, _] = file_columns(header);
            self.text(kubuno_drive_desktop_localization::tr("Name"), &Rect::new(header.left + 36.0, header.top, modified_left, header.bottom), &f.caption, &t.text_secondary, false);
            let divider = Rect::new(header.left, header.bottom, header.right, header.bottom + 1.0);
            self.fill_rounded(&divider, 0.0, &t.divider);

            for (i, row) in layout.other_rows.iter().enumerate() {
                if row.bottom < rect.top || row.top > rect.bottom {
                    continue;
                }
                let entry = &other.entries[i];
                if other.selected.contains(&i) {
                    // Same selected-row fill as the focused pane, minus the
                    // accent edge (this pane has no focus).
                    self.fill_rounded(row, kubuno_drive_desktop_app_controls::themes::shape::radius::SM, &t.list_selected);
                }
                let icon_rect = Rect::new(row.left + 4.0, row.top, row.left + 28.0, row.bottom);
                match self.shell_icon(icons, &entry.path, ICON_ROW_DIP, scale) {
                    Some(bitmap) => self.image(bitmap, &icon_rect, 18.0),
                    None => self.text(GLYPH_FOLDER, &icon_rect, &f.icon_small, &t.text_secondary, true),
                }
                self.text_ellipsis(&entry.name, &Rect::new(row.left + 36.0, row.top, row.right - 8.0, row.bottom), &f.body, &t.text_primary);
            }
        } else {
            let center = Rect::new(rect.left, rect.top + 40.0, rect.right, rect.top + 80.0);
            // Centered title, truncated with "…" when the pane is narrow.
            self.text_ellipsis_center(&other.title(), &center, &f.body, &t.text_secondary);
        }
        unsafe {
            self.ctx.PopAxisAlignedClip();
        }
    }
}
