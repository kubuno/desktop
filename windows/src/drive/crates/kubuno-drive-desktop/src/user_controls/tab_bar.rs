//! Port of `Files.App/UserControls/TabBar/TabBar.xaml`: browser-style tab
//! band with the panes-toggle button, per-tab icon/title/close, the new-tab
//! button and the caption buttons (min/max/close). Tab drag & drop reordering
//! is handled by the window message loop on the rects laid out here.

use windows::Win32::Graphics::Direct2D::D2D1_ANTIALIAS_MODE_PER_PRIMITIVE;

use crate::view_models::shell_view_model::Location;
use crate::ui::{
    fade, tab_close_rect, tab_container_rect, tab_content_rect, Hot, Layout, Painter, Rect, UiState,
    GLYPH_ADD, TAB_BAR_HEIGHT, TAB_DRAG_OPACITY,
};

impl Painter<'_> {
    /// Adds the WinUI `TabViewTemplateSettings.TabGeometry` figure to an
    /// open sink: rounded top corners (OverlayCornerRadius = 8), concave
    /// outward flares (r = 4) at the feet, plus a small skirt below the
    /// band so the figure overlaps the toolbar-strip figure — the WINDING
    /// union renders both as ONE surface with a single antialiased edge.
    /// The two 4×4 boxes hosting `Left/RightRadiusRenderArc` (Margin -4,0,0,0
    /// and 0,0,-4,0, VerticalAlignment=Bottom). Built from the SAME snapped
    /// bounds as `add_tab_figure` so the crescents land exactly on the flares.
    pub(crate) fn tab_foot_boxes(&self, tab: &Rect) -> (Rect, Rect) {
        let (left, right, bottom) = (self.px(tab.left), self.px(tab.right), self.px(tab.bottom));
        (
            Rect::new(left - 4.0, bottom - 4.0, left, bottom),
            Rect::new(right, bottom - 4.0, right + 4.0, bottom),
        )
    }

    pub(crate) fn add_tab_figure(
        &self,
        sink: &windows::Win32::Graphics::Direct2D::ID2D1GeometrySink,
        tab: &Rect,
        skirt: f32,
    ) {
        use windows::Win32::Graphics::Direct2D::Common::{
            D2D1_FIGURE_BEGIN_FILLED, D2D1_FIGURE_END_CLOSED, D2D_SIZE_F,
        };
        use windows::Win32::Graphics::Direct2D::{
            D2D1_ARC_SEGMENT, D2D1_ARC_SIZE_SMALL, D2D1_SWEEP_DIRECTION_CLOCKWISE,
            D2D1_SWEEP_DIRECTION_COUNTER_CLOCKWISE,
        };
        let (r_top, r_foot) = (8.0f32, 4.0f32);
        // Pixel-snapped bounds: half-pixel edges leave 50%-coverage rows
        // that read as a thin halo around the shape.
        let tab = Rect::new(
            self.px(tab.left),
            self.px(tab.top),
            self.px(tab.right),
            self.px(tab.bottom),
        );
        let p = |x: f32, y: f32| windows_numerics::Vector2 { X: x, Y: y };
        let arc = |x: f32, y: f32, r: f32, cw: bool| D2D1_ARC_SEGMENT {
            point: p(x, y),
            size: D2D_SIZE_F { width: r, height: r },
            rotationAngle: 0.0,
            sweepDirection: if cw {
                D2D1_SWEEP_DIRECTION_CLOCKWISE
            } else {
                D2D1_SWEEP_DIRECTION_COUNTER_CLOCKWISE
            },
            arcSize: D2D1_ARC_SIZE_SMALL,
        };
        unsafe {
            sink.BeginFigure(p(tab.left - r_foot, tab.bottom + skirt), D2D1_FIGURE_BEGIN_FILLED);
            sink.AddLine(p(tab.left - r_foot, tab.bottom));
            // Left foot: concave flare up to the wall.
            sink.AddArc(&arc(tab.left, tab.bottom - r_foot, r_foot, false));
            sink.AddLine(p(tab.left, tab.top + r_top));
            // Top-left rounded corner.
            sink.AddArc(&arc(tab.left + r_top, tab.top, r_top, true));
            sink.AddLine(p(tab.right - r_top, tab.top));
            // Top-right rounded corner.
            sink.AddArc(&arc(tab.right, tab.top + r_top, r_top, true));
            sink.AddLine(p(tab.right, tab.bottom - r_foot));
            // Right foot: concave flare out to the strip.
            sink.AddArc(&arc(tab.right + r_foot, tab.bottom, r_foot, false));
            sink.AddLine(p(tab.right + r_foot, tab.bottom + skirt));
            sink.EndFigure(D2D1_FIGURE_END_CLOSED);
        }
    }

    pub(crate) fn draw_tab_bar(&self, layout: &Layout, state: &UiState, scale: f32) {
        let t = self.theme;
        let f = &self.renderer.formats;

        // The band fill (accent tint, light theme only) is painted by
        // draw_toolbar BEFORE the tab/strip union surface.
        let band = Rect::new(0.0, 0.0, layout.width, TAB_BAR_HEIGHT);

        // Tab-actions button (TabStripHeader: 30×30, Margin 4,0,-2,0),
        // hidden by the ShowTabActions appearance toggle.
        if crate::services::settings::get().show_tab_actions {
            let toggle = Rect::new(4.0, 11.0, 34.0, 41.0);
            if state.hot == Some(Hot::PaneToggle) {
                self.fill_rounded(&toggle, 4.0, &t.control_fill_hover);
            }
            // The original `ThemedIcons.Panes.Single` geometry, verbatim.
            self.vector_icon("PanesSingle", &toggle, 16.0, &t.text_primary);
        }

        // BottomBorderLine, the foot crescents and the selected-tab surface are
        // all painted by draw_toolbar (which runs BEFORE this), in the
        // template's own z-order — see there.

        // Clip the rest of the tab band (hover fills, titles, separators).
        unsafe {
            self.ctx.PushAxisAlignedClip(&band.d2d(), D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
        }
        // TabView raises the dragged item's Canvas.ZIndex, so it floats above
        // the tabs it is passing over: paint it last.
        let dragged = state.tab_drag_offset.map(|(d, _)| d);
        let order = (0..layout.tabs.len())
            .filter(|i| Some(*i) != dragged)
            .chain(dragged.filter(|d| *d < layout.tabs.len()));
        for i in order {
            let slot = &layout.tabs[i];
            // Slots are fixed; the painted rect is offset by the reorder
            // animation (or by the cursor, for the tab being dragged).
            let tab = &slot.shift_x(state.tab_offset(i));
            let dragging = state.tab_drag_offset.map(|(d, _)| d) == Some(i);
            let op = if dragging { TAB_DRAG_OPACITY } else { 1.0 };
            let active = i == state.active_tab;
            let hovered = !dragging
                && (state.hot == Some(Hot::Tab(i)) || state.hot == Some(Hot::TabClose(i)));

            // The hover fill is TabContainer's Background — so it stops 4 DIP
            // short of the item's walls, which is exactly what leaves a visible
            // margin between the ACTIVE tab (whose TabGeometry does run to the
            // wall) and a hovered neighbor. Top corners only, radius 8
            // (TopCornerRadiusFilterConverter on OverlayCornerRadius).
            // At rest the background is SubtleFillColorTransparent: nothing.
            if !active && hovered {
                self.fill_top_rounded(&tab_container_rect(tab), 8.0, &t.tab_hover_background);
            }

            // TabViewItemHeaderPadding → content box; icon 16×16 with
            // IconMargin 0,0,10,0; title next to it; CloseButton 32×24 at the
            // right edge with CloseMargin 4,0,0,0 before it.
            let content = tab_content_rect(tab);
            let cy = (content.top + content.bottom) / 2.0;
            let icon_rect = Rect::new(content.left, cy - 8.0, content.left + 16.0, cy + 8.0);
            let close = tab_close_rect(tab);
            let title_rect = Rect::new(
                icon_rect.right + 10.0,
                content.top,
                close.left - 4.0,
                content.bottom,
            );

            // TabViewItemIconForeground / …Selected: secondary vs primary.
            // The dragged tab fades to ListViewItemDragThemeOpacity.
            let fg = fade(if active { &t.text_primary } else { &t.text_secondary }, op);
            // One outlined family across the whole window: a tab wears the same
            // Material mark as the nav row it mirrors, not the shell's bitmap.
            let tab_icon = match &state.tabs[i].active().location {
                Location::Home => "Home",
                Location::Settings => "Settings",
                Location::RecycleBin => "Delete",
                Location::SearchResults { .. } => "OmnibarSearch",
                Location::Dir(_) => "Folder",
            };
            self.vector_icon(tab_icon, &icon_rect, 16.0, &fg);

            // ContentPresenter: FontSize 12, SemiBold only when selected.
            let title_format = if active { &f.caption_strong } else { &f.caption };
            self.text_ellipsis(&state.tabs[i].title(), &title_rect, title_format, &fg);

            // CloseButton: glyph E711, FontSize 12, ControlCornerRadius 4.
            if state.hot == Some(Hot::TabClose(i)) && !dragging {
                self.fill_rounded(&close, 4.0, &t.control_fill_pressed);
            }
            self.text("\u{E711}", &close, &f.icon_small, &fg, true);

            // TabSeparator: 1px at the item's right edge, SeparatorMargin
            // 0,8,0,8. TabView hides it on the selected tab, on the one just
            // LEFT of it (HideLeftAdjacentTabSeparator), and around the hovered
            // tab — so no separator ever brushes against a highlighted tab.
            let next_active = i + 1 == state.active_tab;
            let next_hovered = state.hot == Some(Hot::Tab(i + 1))
                || state.hot == Some(Hot::TabClose(i + 1));
            let last = i + 1 == layout.tabs.len();
            if !active && !hovered && !next_active && !next_hovered && !last {
                let sep = Rect::new(tab.right - 1.0, tab.top + 8.0, tab.right, tab.bottom - 8.0);
                self.fill_rounded(&sep, 0.0, &t.tab_separator);
            }
        }
        unsafe {
            self.ctx.PopAxisAlignedClip();
        }

        // TabBarAddNewTabButton: 30×30, FontIcon 12, glyph E710.
        if state.hot == Some(Hot::NewTab) {
            self.fill_rounded(&layout.new_tab, 4.0, &t.control_fill_hover);
        }
        self.text(GLYPH_ADD, &layout.new_tab, &f.icon_small, &t.text_primary, true);

        // Min/max/close caption buttons, drawn by US at the same
        // dimensions as WinUI's `CaptionButton` (46 wide, 10px glyph),
        // no longer by DWM (whose version was bigger). The frame
        // is no longer extended above (init) so DWM doesn't compose them anymore.
        self.draw_caption_buttons(layout, state, scale);
    }

    /// The three window buttons (WinUI's `CaptionButton`): 46 wide,
    /// 10px Segoe Fluent Icons glyph, gray hover for min/max, RED for
    /// close (white glyph). Grayed out when the window has lost focus.
    fn draw_caption_buttons(&self, layout: &Layout, state: &UiState, scale: f32) {
        use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;
        let t = self.theme;
        let f = &self.renderer.formats;
        // Glyph dimmed when the window is inactive (like WinUI/DWM).
        let glyph_color = if state.window_active { t.text_primary } else { t.text_tertiary };
        // `ChromeMinimize`/`ChromeMaximize`/`ChromeRestore`/`ChromeClose`.
        let restore = state.maximized;
        let buttons = [
            (layout.caption_min, "\u{E921}", Hot::CaptionMin, false),
            (layout.caption_max, if restore { "\u{E923}" } else { "\u{E922}" }, Hot::CaptionMax, false),
            (layout.caption_close, "\u{E8BB}", Hot::CaptionClose, true),
        ];
        for (rect, glyph, hot, is_close) in buttons {
            let hovered = state.hot == Some(hot);
            let mut gc = glyph_color;
            if hovered {
                if is_close {
                    // `CloseButtonBackground` PointerOver = #C42B1C, white glyph.
                    // ONLY the top-right corner is rounded (radius = Windows 11 corner,
                    // ~8 physical px): the red hugs the window's corner on the
                    // right while staying flush on the left (next to Maximize).
                    let corner = 8.0 / scale.max(0.01);
                    self.fill_top_right_rounded(&rect, corner, &D2D1_COLOR_F { r: 0.769, g: 0.169, b: 0.110, a: 1.0 });
                    gc = D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 1.0 };
                } else {
                    // min/max: `SubtleFillColorSecondary`.
                    self.fill_rounded(&rect, 0.0, &t.control_fill_hover);
                }
            }
            self.text(glyph, &rect, &f.icon_caption, &gc, true);
        }
    }
}
