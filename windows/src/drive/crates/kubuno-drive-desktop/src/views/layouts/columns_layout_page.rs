//! Port of `Files.App/Views/Layouts/ColumnsLayoutPage.xaml`: the 200 DIP
//! blade `BladeView` (`BladeItem`, 1 px right border), each a
//! `ColumnLayoutPage` listing its folder. Selecting a folder opens the
//! next blade — see `Tab::select_column_row` (`DismissOtherBlades`).
//!
//! The rendering of ONE blade lives in [`super::column_layout_page`]
//! (`ColumnLayoutPage.xaml.cs`); here we only hold the container: the loop
//! over the blades and their dividers.

use crate::services::storage::IconCache;
use crate::ui::{Layout, Painter, UiState};

impl Painter<'_> {
    pub(super) fn draw_columns_layout(&self, layout: &Layout, state: &UiState, icons: &IconCache, scale: f32) {
        let t = self.theme;
        let tab = state.active();
        let mode = tab.view_mode;
        let size = crate::view_models::shell_view_model::layout_size(mode);
        let icon_dip = crate::view_models::shell_view_model::icon_size_for(mode, size);

        for (c, (pane, rows)) in layout.column_panes.iter().enumerate() {
            if kubuno_drive_desktop_app_controls::blade_view::is_culled(pane, layout.content.left, layout.content.right) {
                continue;
            }
            let Some(column) = tab.columns.get(c) else { continue };
            self.draw_column_page(
                column,
                rows,
                c,
                layout.content.top,
                layout.content.bottom,
                state,
                icons,
                icon_dip,
                scale,
            );
            // The blade's divider (`BorderThickness="0,0,1,0"`).
            let divider = kubuno_drive_desktop_app_controls::blade_view::blade_divider(pane);
            self.fill_rounded(&divider, 0.0, &t.divider);
        }
    }
}
