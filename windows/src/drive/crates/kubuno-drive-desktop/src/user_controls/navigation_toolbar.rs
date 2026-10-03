//! Port of `Files.App/UserControls/NavigationToolbar.xaml`: the sidebar
//! hamburger (only when the pane is closed), navigation buttons
//! (back/forward/up/refresh), the Omnibar with its THREE modes — path
//! (breadcrumb), command palette and search — whose mode icons sit at the
//! right edge INSIDE the bar, and the StatusCenter button on the far right.

use kubuno_desktop_ui::navigation::Breadcrumb;

use crate::user_controls::edit_box::{EDIT_PALETTE, EDIT_PATH, EDIT_SEARCH};
use crate::view_models::shell_view_model::Location;
use crate::ui::{
    Hot, Layout, Painter, Rect, UiState, GLYPH_CHEVRON_RIGHT, TAB_BAR_HEIGHT, TAB_DRAG_OPACITY,
    TOOLBAR_HEIGHT,
};

/// Navigation icons are drawn at the same 16 DIP as every other command icon,
/// so the whole bar reads as one Material Symbols family.
const NAV_ICON_SIZE: f32 = 16.0;

/// Where the path trail starts inside the omnibar — past the home icon and its
/// chevron — and how far it stops short of the mode buttons.
const TRAIL_START_INSET: f32 = 52.0;
const TRAIL_RIGHT_MARGIN: f32 = 40.0;
/// The trail's own rows are 2 DIP inside the pill, top and bottom.
const TRAIL_INSET_Y: f32 = 2.0;
/// A chevron glyph's box — `formats().icon_small`'s em, which is the size the
/// design system draws its own `ChevronRight` geometry at.
const CHEVRON_SIZE: f32 = 12.0;

thread_local! {
    /// The width each path segment asked for on the last frame, straight from
    /// [`Breadcrumb::segment_widths`].
    ///
    /// A segment's width is a DirectWrite extent and `Layout::compute` — which
    /// needs the folded trail's rectangles to hit-test them — is static and has
    /// no drawing surface, so the widths are taken once per frame in
    /// [`premeasure`] and the layout pass reuses them. The folding itself is
    /// computed in ONE place, [`Breadcrumb::layout_with`], which both the
    /// layout pass and the paint go through.
    static SEGMENT_WIDTHS: std::cell::RefCell<Vec<f32>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// Whether this location shows a path trail at all.
pub(crate) fn shows_breadcrumbs(location: &Location) -> bool {
    matches!(location, Location::Dir(_) | Location::RecycleBin | Location::SearchResults { .. })
}

/// The trail, as the design system's own primitive. `root_chevron` stays off:
/// the chevron after the home icon belongs to the omnibar, not to the trail —
/// its box is measured from the icon, and it shows for `Paramètres` too, where
/// there is no trail at all.
fn build_breadcrumb(segments: &[(String, std::path::PathBuf)]) -> Breadcrumb {
    let mut trail = Breadcrumb::new();
    for (name, _) in segments {
        trail = trail.with(name);
    }
    trail
}

/// The box the trail is laid out in, inside the omnibar pill.
pub(crate) fn breadcrumb_bounds(address_bar: Rect, first_mode: Rect) -> Rect {
    Rect::new(
        address_bar.left + TRAIL_START_INSET,
        address_bar.top + TRAIL_INSET_Y,
        first_mode.left - TRAIL_RIGHT_MARGIN,
        address_bar.bottom - TRAIL_INSET_Y,
    )
}

/// Measures the path segments while a drawing surface exists, for the layout
/// pass that follows. See [`SEGMENT_WIDTHS`].
pub(crate) fn premeasure(c: &dyn kubuno_desktop_ui::Canvas, state: &UiState) {
    let tab = state.active();
    if !shows_breadcrumbs(&tab.location) {
        return;
    }
    let widths = build_breadcrumb(&tab.breadcrumbs()).segment_widths(c);
    SEGMENT_WIDTHS.with(|w| *w.borrow_mut() = widths);
}

/// The trail's geometry — which head segments fold behind the `…`, and where
/// the visible ones sit — for the layout pass to hit-test.
pub(crate) fn breadcrumb_layout(
    segments: &[(String, std::path::PathBuf)],
    bounds: Rect,
) -> kubuno_drive_desktop_app_controls::BreadcrumbLayout {
    let trail = build_breadcrumb(segments);
    SEGMENT_WIDTHS.with(|w| {
        let cached = w.borrow();
        // `Layout::compute` also runs between two frames — on a mouse move,
        // say — and a navigation changes the path before the repaint that
        // measures it. A measurement for another path would fold the wrong
        // segments, so it is only used when it still describes this one;
        // otherwise the estimate stands in for the frame, as it did before
        // anything was measured at all. `BreadcrumbBarItemPadding = 8,0` is
        // the 16 the primitive adds to a segment's label.
        if cached.len() == segments.len() {
            trail.layout_with(&cached, bounds)
        } else {
            let widths: Vec<f32> = segments
                .iter()
                .map(|(n, _)| crate::ui::approx_text_width(n, 14.0) + 16.0)
                .collect();
            trail.layout_with(&widths, bounds)
        }
    })
}

/// Row rect of the i-th omnibar suggestion (relative to the address bar).
pub fn suggestion_rect(address_bar: &Rect, i: usize) -> Rect {
    Rect::new(
        address_bar.left + 4.0,
        address_bar.bottom + 8.0 + i as f32 * 44.0,
        address_bar.right - 4.0,
        address_bar.bottom + 8.0 + (i + 1) as f32 * 44.0,
    )
}

impl Painter<'_> {
    pub(crate) fn draw_toolbar(&self, layout: &Layout, state: &UiState) {
        let t = self.theme;
        let f = &self.renderer.formats;
        let tab = state.active();

        // The toolbar strip and the ACTIVE TAB above it are ONE translucent
        // surface in the original (both LayerOnMicaBaseAlt). Fill them as a
        // single WINDING-mode geometry: no internal edges, one smooth
        // antialiased boundary around the whole silhouette.
        {
            use windows::core::Interface;
            use windows::Win32::Graphics::Direct2D::Common::{
                D2D1_FIGURE_BEGIN_FILLED, D2D1_FIGURE_END_CLOSED, D2D1_FILL_MODE_WINDING,
            };
            // Accent-tinted title band ("accent color on title bars"),
            // light theme only — under the tab/strip union. When the window
            // loses focus, we switch to the INACTIVE tint (the accent
            // disappears from the title bar, as in the original).
            let titlebar = if state.window_active {
                &t.titlebar_background
            } else {
                &t.titlebar_background_inactive
            };
            if titlebar.a > 0.0 {
                let band = Rect::new(0.0, 0.0, layout.width, TAB_BAR_HEIGHT);
                self.fill_rounded(&band, 0.0, titlebar);
            }
            let strip = Rect::new(
                self.px(0.0),
                self.px(TAB_BAR_HEIGHT),
                self.px(layout.width),
                self.px(TAB_BAR_HEIGHT + TOOLBAR_HEIGHT),
            );
            // The active tab follows the reorder animation / the drag cursor.
            let active_tab = layout
                .tabs
                .get(state.active_tab)
                .map(|slot| slot.shift_x(state.tab_offset(state.active_tab)));
            let active_tab = active_tab.as_ref();

            // TabBarItemStyle paints, in this order (the selected item's
            // Canvas.ZIndex is raised by TabView, so its geometry covers the
            // neighbors' border line):
            //   1. BottomBorderLine  — 1px, TabViewBorderBrush, per item; the
            //      SELECTED item hides its own, so the line stops at its walls.
            //   2. Left/RightRadiusRenderArc — 4×4 crescents, same brush, that
            //      carry that 1px line around the concave flares.
            //   3. SelectedBackgroundPath — TabGeometry, translucent, on top.
            // While the active tab is being DRAGGED it is lifted out of the
            // strip (ListView detaches the item), so it no longer notches the
            // border line and no longer merges with the surface below.
            let dragging_active =
                state.tab_drag_offset.map(|(d, _)| d) == Some(state.active_tab);
            let seated = if dragging_active { None } else { active_tab };

            if let Some(tab) = seated {
                let y = TAB_BAR_HEIGHT - 1.0;
                let (left, right) = (self.px(tab.left), self.px(tab.right));
                if left > 0.0 {
                    self.fill_rounded(&Rect::new(0.0, y, left, y + 1.0), 0.0, &t.tab_border);
                }
                if right < layout.width {
                    self.fill_rounded(
                        &Rect::new(right, y, layout.width, y + 1.0),
                        0.0,
                        &t.tab_border,
                    );
                }
                let (foot_left, foot_right) = self.tab_foot_boxes(tab);
                self.vector_icon("TabBorderArcLeft", &foot_left, 4.0, &t.tab_border);
                self.vector_icon("TabBorderArcRight", &foot_right, 4.0, &t.tab_border);
            } else {
                let y = TAB_BAR_HEIGHT - 1.0;
                self.fill_rounded(&Rect::new(0.0, y, layout.width, y + 1.0), 0.0, &t.tab_border);
            }

            let filled = (|| -> windows::core::Result<()> {
                let factory = self
                    .renderer
                    .d2d_factory
                    .cast::<windows::Win32::Graphics::Direct2D::ID2D1Factory>()?;
                unsafe {
                    let geometry = factory.CreatePathGeometry()?;
                    let sink = geometry.Open()?;
                    sink.SetFillMode(D2D1_FILL_MODE_WINDING);
                    let p = |x: f32, y: f32| windows_numerics::Vector2 { X: x, Y: y };
                    sink.BeginFigure(p(strip.left, strip.top), D2D1_FIGURE_BEGIN_FILLED);
                    sink.AddLine(p(strip.right, strip.top));
                    sink.AddLine(p(strip.right, strip.bottom));
                    sink.AddLine(p(strip.left, strip.bottom));
                    sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                    if let Some(tab) = seated {
                        self.add_tab_figure(&sink, tab, 2.0);
                    }
                    sink.Close()?;
                    self.ctx
                        .FillGeometry(&geometry, self.set_color(&t.tab_active_background), None);

                    // Dragged: its own surface, faded, floating over the strip.
                    if let Some(tab) = active_tab.filter(|_| dragging_active) {
                        let lifted = factory.CreatePathGeometry()?;
                        let sink = lifted.Open()?;
                        sink.SetFillMode(D2D1_FILL_MODE_WINDING);
                        self.add_tab_figure(&sink, tab, 0.0);
                        sink.Close()?;
                        let color = crate::ui::fade(&t.tab_active_background, TAB_DRAG_OPACITY);
                        self.ctx.FillGeometry(&lifted, self.set_color(&color), None);
                    }
                }
                Ok(())
            })()
            .is_ok();
            if !filled {
                self.fill_rounded(&strip, 0.0, &t.tab_active_background);
            }
        }

        // SidebarPaneToggleButton (GlobalNavigationButton) when pane closed.
        if let Some(rect) = &layout.hamburger {
            if state.hot == Some(Hot::Hamburger) {
                self.fill_rounded(rect, kubuno_drive_desktop_app_controls::themes::shape::pill(rect.bottom - rect.top), &t.control_fill_hover);
            }
            self.vector_icon("Menu", rect, NAV_ICON_SIZE, &t.text_primary);
        }

        let nav = [
            (layout.nav_back, "NavBack", Hot::NavBack, tab.can_go_back()),
            (layout.nav_forward, "NavForward", Hot::NavForward, tab.can_go_forward()),
            (layout.nav_up, "NavUp", Hot::NavUp, tab.can_go_up()),
            (layout.nav_refresh, "Refresh", Hot::NavRefresh, true),
        ];
        for (rect, icon, target, enabled) in nav {
            if enabled && state.hot == Some(target) {
                self.fill_rounded(&rect, kubuno_drive_desktop_app_controls::themes::shape::pill(rect.bottom - rect.top), &t.control_fill_hover);
            }
            let color = if enabled { &t.text_primary } else { &t.text_secondary };
            self.vector_icon(icon, &rect, NAV_ICON_SIZE, color);
        }

        // The Omnibar's DRAWING now lives in the
        // `kubuno_drive_desktop_app_controls::omnibar` control: pill + border + focus ring +
        // mode icon + ✕ + mode buttons + separator. We build the
        // view (primitives) from `layout`/`state`, then delegate.
        let ov = self.build_omnibar_view(layout, state);
        kubuno_drive_desktop_app_controls::omnibar::draw(self, &ov);

        // An active text mode (path / palette / search) replaces the
        // breadcrumb: the EDITABLE text box (+ suggestions) is drawn AROUND
        // the control, app-side.
        let text_mode = state
            .edit
            .as_ref()
            .filter(|e| matches!(e.entry, EDIT_PATH | EDIT_PALETTE | EDIT_SEARCH));
        if let Some(edit) = text_mode {
            let box_rect = Rect::new(
                layout.address_bar.left + 34.0,
                layout.address_bar.top + 3.0,
                layout.address_bar.right - 44.0,
                layout.address_bar.bottom - 3.0,
            );
            // The pill bar itself is the text box — no inner chrome.
            kubuno_drive_desktop_app_controls::edit_box::draw_text(
                self,
                &box_rect,
                &kubuno_drive_desktop_app_controls::EditView { text: &edit.text, caret: edit.caret, anchor: edit.anchor },
            );
            // The suggestions panel is drawn LAST (over the content) by
            // `draw_omnibar_suggestions`.
            self.draw_status_center_button(layout, state);
            return;
        }

        let home_icon = Rect::new(
            layout.address_bar.left + 10.0,
            layout.address_bar.top,
            layout.address_bar.left + 30.0,
            layout.address_bar.bottom,
        );
        self.vector_icon("Home", &home_icon, 16.0, &t.text_secondary);

        match &tab.location {
            Location::Home | Location::Settings => {
                // Original BreadcrumbBar: the root chevron shows as soon as
                // there is a child item ("🏠 > Paramètres"); Home is the
                // root itself, so no chevron there.
                let mut crumb_left = home_icon.right + 8.0;
                if tab.location == Location::Settings {
                    let home_chevron = Rect::new(home_icon.right + 2.0, layout.address_bar.top, home_icon.right + 20.0, layout.address_bar.bottom);
                    self.text(GLYPH_CHEVRON_RIGHT, &home_chevron, &f.icon_small, &t.text_secondary, true);
                    crumb_left = home_icon.right + 26.0;
                }
                let crumb = Rect::new(crumb_left, layout.address_bar.top, layout.omnibar_modes[0].left - 8.0, layout.address_bar.bottom);
                let label = if tab.location == Location::Settings { kubuno_drive_desktop_localization::tr("Settings") } else { kubuno_drive_desktop_localization::tr("Home") };
                self.text(label, &crumb, &f.body, &t.text_primary, false);
            }
            Location::Dir(_) | Location::RecycleBin | Location::SearchResults { .. } => {
                // The trail is `kubuno_desktop_ui::navigation::Breadcrumb`, which wraps
                // the very `layout_breadcrumbs` the layout pass folded the path
                // with — one algorithm, reached from both sides.
                let segments = tab.breadcrumbs();
                let trail = build_breadcrumb(&segments);
                // The root chevron is the omnibar's own, measured off the home
                // icon rather than off the trail, so it stays hand-drawn — with
                // the design system's chevron geometry, like the trail's.
                let home_chevron = Rect::new(
                    home_icon.right + 2.0,
                    layout.address_bar.top,
                    home_icon.right + 20.0,
                    layout.address_bar.bottom,
                );
                self.vector_icon("ChevronRight", &home_chevron, CHEVRON_SIZE, &t.text_tertiary);
                // `Hot::Breadcrumb` counts VISIBLE segments; the primitive
                // indexes the whole path, folded head included.
                let hot = match state.hot {
                    Some(Hot::Breadcrumb(k)) => Some(layout.breadcrumb_start + k),
                    _ => None,
                };
                trail.paint_segments(
                    self,
                    breadcrumb_bounds(layout.address_bar, layout.omnibar_modes[0]),
                    hot,
                    state.hot == Some(Hot::BreadcrumbEllipsis),
                );
            }
        }

        self.draw_status_center_button(layout, state);
    }

    /// Omnibar suggestions (path children or palette commands), drawn last
    /// so the panel sits over the content like a real flyout.
    pub(crate) fn draw_omnibar_suggestions(&self, layout: &Layout, state: &UiState) {
        let t = self.theme;
        let f = &self.renderer.formats;
        let editing = state
            .edit
            .as_ref()
            .is_some_and(|e| matches!(e.entry, EDIT_PATH | EDIT_PALETTE | EDIT_SEARCH));
        if !editing || state.path_suggestions.is_empty() {
            return;
        }
        let panel = Rect::new(
            layout.address_bar.left,
            layout.address_bar.bottom + 4.0,
            layout.address_bar.right,
            layout.address_bar.bottom + 12.0 + state.path_suggestions.len() as f32 * 44.0,
        );
        self.draw_flyout_panel(&panel);
        for (i, (label, _)) in state.path_suggestions.iter().enumerate() {
            let row = suggestion_rect(&layout.address_bar, i);
            // A menu row lights up under the pointer (`@ui/MenuDropdown`): the
            // panel had no hover feedback at all, so nothing said the rows
            // were clickable.
            if state.hot == Some(Hot::Suggestion(i)) {
                self.fill_rounded(&row, kubuno_drive_desktop_app_controls::themes::shape::radius::MENU_ITEM, &t.accent);
            }
            let fg = if state.hot == Some(Hot::Suggestion(i)) {
                t.accent_foreground
            } else {
                t.text_primary
            };
            self.text_ellipsis(label, &Rect::new(row.left + 20.0, row.top, row.right - 8.0, row.bottom), &f.body, &fg);
        }
    }

    /// ShowStatusCenterButton (`NavigationToolbar.xaml`): the ThemedIcon icon at
    /// rest; during operations (`ShowProgressRing`), the icon gives way
    /// to the `InfoBadge` — the operation COUNT (`InfoBadgeValue`), centered.
    ///
    /// The determinate `MedianOperationProgressRing` (`AverageOperationProgressValue`)
    /// is not drawn: `OpsMonitor` only tracks (id, label), with no percentage
    /// per operation — showing a ring at a made-up value would betray
    /// fidelity. The ring will follow once the worker reports real progress.
    fn draw_status_center_button(&self, layout: &Layout, state: &UiState) {
        use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;
        let t = self.theme;
        let Some(rect) = &layout.status_center_button else { return };
        if state.hot == Some(Hot::StatusCenter) || state.status_center_open {
            self.fill_rounded(rect, kubuno_drive_desktop_app_controls::themes::shape::pill(rect.bottom - rect.top), &t.control_fill_hover);
        }
        if state.ops_active {
            let n = state.ops_count.to_string();
            let f = &self.renderer.formats;
            let tw = self.measure(&n, &f.caption);
            let d = 18.0_f32.max(tw + 10.0);
            let cx = (rect.left + rect.right) / 2.0;
            let cy = (rect.top + rect.bottom) / 2.0;
            let badge = Rect::new(cx - d / 2.0, cy - 9.0, cx - d / 2.0 + d, cy + 9.0);
            self.fill_rounded(&badge, 9.0, &t.accent);
            let white = D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 1.0 };
            let text_rect = Rect::new(badge.left, badge.top + 1.0, badge.right, badge.bottom);
            self.text(&n, &text_rect, &f.caption, &white, true);
        } else {
            self.vector_icon("StatusCenterIcon", rect, 16.0, &t.text_primary);
        }
    }

    /// Builds the Omnibar's drawing snapshot (primitives) from
    /// `layout`/`state`, to pass to `kubuno_drive_desktop_app_controls::omnibar::draw`.
    /// The app→control "binding": in text mode, the pill is focused with a
    /// mode icon + ✕; otherwise the row of inactive modes + separator.
    pub(crate) fn build_omnibar_view(
        &self,
        layout: &Layout,
        state: &UiState,
    ) -> kubuno_drive_desktop_app_controls::omnibar::OmnibarView {
        use kubuno_drive_desktop_app_controls::omnibar::OmnibarView;
        let text_mode = state
            .edit
            .as_ref()
            .filter(|e| matches!(e.entry, EDIT_PATH | EDIT_PALETTE | EDIT_SEARCH));
        if let Some(edit) = text_mode {
            let active_mode_icon = Some(match edit.entry {
                EDIT_PALETTE => "OmnibarCommands",
                EDIT_SEARCH => "OmnibarSearch",
                _ => "OmnibarPath",
            });
            let clear_button = Some(Rect::new(
                layout.address_bar.right - 40.0,
                layout.address_bar.top + 4.0,
                layout.address_bar.right - 8.0,
                layout.address_bar.bottom - 4.0,
            ));
            return OmnibarView {
                address_bar: layout.address_bar,
                focused: true,
                active_mode_icon,
                clear_button,
                mode_buttons: Vec::new(),
                separator: None,
            };
        }
        // Navigation mode: only the INACTIVE modes [1..3] are listed, with a
        // vertical separator between them (like the original).
        let mut mode_buttons = Vec::new();
        for (i, rect) in layout.omnibar_modes.iter().enumerate() {
            if i == 0 {
                continue;
            }
            let name = ["OmnibarPath", "OmnibarCommands", "OmnibarSearch"][i];
            let hot = state.hot == Some(Hot::OmnibarMode(i));
            mode_buttons.push((*rect, name, hot));
        }
        let separator = kubuno_drive_desktop_app_controls::omnibar::separator_rect(&layout.omnibar_modes, 1);
        OmnibarView {
            address_bar: layout.address_bar,
            focused: false,
            active_mode_icon: None,
            clear_button: None,
            mode_buttons,
            separator,
        }
    }
}
