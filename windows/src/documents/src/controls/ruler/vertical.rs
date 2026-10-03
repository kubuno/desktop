//! `VerticalRuler` — the ruler left of the page (the web's `VerticalRuler`): a 20-DIP grey column
//! that graduates **only the active page** (the one the current paragraph is on when it is in view,
//! else the most visible one), like Word. That page's text column is white between its top and
//! bottom margins; the graduations count centimetres from the top margin, in absolute value both
//! ways; above and below the page the column stays plain grey. It scrolls with the page:
//! `PageTop` is where the active page's paper starts, in DIP from the ruler's top.
//!
//! Drag the top or bottom margin edge (no handle is drawn, as in Word): a dashed guide crosses the
//! page and a tooltip gives the margin in centimetres; the change is raised as `MarginsChanged`.

use kubuno::prelude::*;
use kubuno::ui::graphics::{Brush, Graphics, StringAlignment, StringFormat, StringFormatFlags};
use kubuno::ui::{Canvas, Rect, Size};
use kubuno::views::component::{Component, Control, ControlCore, EventCx, PaintEventCx};
use kubuno::views::events::{EmptyEventArgs, Event};

use super::{cm_text, number_font, rule, ticks, RulerGuideEventArgs, RulerMarginsEventArgs, RulerColors, VerticalGeometry, VerticalPart, RULER_SZ};

/// The ruler left of the page (see the module doc).
#[derive(kubuno::views::component::Component)]
#[kubuno(extends = Control, overrides(Control))]
#[category("Documents")]
#[toolbox(icon = "ruler")]
#[default_event("MarginsChanged")]
pub struct VerticalRuler {
    base: ControlCore,
    /// The page's height, in document pixels (A4: 1123).
    #[property(bindable)]
    #[category("Page")]
    #[default_value(1123.0)]
    pub page_height: f32,
    /// The top margin, in document pixels.
    #[property(bindable)]
    #[category("Page")]
    #[default_value(96.0)]
    pub margin_top: f32,
    /// The bottom margin, in document pixels.
    #[property(bindable)]
    #[category("Page")]
    #[default_value(96.0)]
    pub margin_bottom: f32,
    /// The page's zoom (1 = 100 %).
    #[property(bindable)]
    #[category("Page")]
    #[default_value(1.0)]
    pub zoom: f32,
    /// Where the active page's paper starts, in DIP from the ruler's top (negative once it has
    /// scrolled above it).
    #[property(bindable)]
    #[category("Page")]
    #[default_value(37.0)]
    pub page_top: f32,
    /// Occurs while a margin edge is dragged (`start`: top, `end`: bottom), and once more when it
    /// is released (`commit`).
    #[event]
    #[category("Action")]
    pub margins_changed: Event<RulerMarginsEventArgs>,
    /// Occurs when the dashed guide over the page appears, moves or goes.
    #[event]
    #[category("Action")]
    pub drag_guide_changed: Event<RulerGuideEventArgs>,
    /// Occurs when the ruler is double-clicked (the page setup dialog).
    #[event]
    #[category("Action")]
    pub page_setup_requested: Event<EmptyEventArgs>,
    drag: Option<(VerticalPart, VerticalGeometry)>,
    live: Option<VerticalGeometry>,
    /// The height of the last paint.
    height: f32,
}

impl Default for VerticalRuler {
    fn default() -> Self {
        Self {
            base: ControlCore::default(),
            page_height: 1123.0,
            margin_top: 96.0,
            margin_bottom: 96.0,
            zoom: 1.0,
            page_top: 37.0,
            margins_changed: Event::default(),
            drag_guide_changed: Event::default(),
            page_setup_requested: Event::default(),
            drag: None,
            live: None,
            height: 0.0,
        }
    }
}

impl VerticalRuler {
    /// The geometry shown: the drag's, else the properties'.
    pub fn geometry(&self) -> VerticalGeometry {
        self.live.unwrap_or(VerticalGeometry {
            page_height: self.page_height,
            margin_top: self.margin_top,
            margin_bottom: self.margin_bottom,
            zoom: if self.zoom > 0.0 { self.zoom } else { 1.0 },
            page_top: self.page_top,
        })
    }

    fn raise_guide(&mut self, g: &VerticalGeometry, part: VerticalPart, pointer: f32) {
        use crate::Resources as R;
        let (position, label) = match part {
            VerticalPart::MarginTop => (g.content_top(), R::margin_top_cm().replace("{value}", &cm_text(g.margin_top))),
            VerticalPart::MarginBottom => (g.content_bottom(), R::margin_bottom_cm().replace("{value}", &cm_text(g.margin_bottom))),
        };
        self.raise_drag_guide_changed(RulerGuideEventArgs { visible: true, vertical: false, position, label, pointer });
    }

    fn end_drag(&mut self) {
        if self.drag.take().is_none() {
            return;
        }
        if let Some(g) = self.live.take() {
            self.margin_top = g.margin_top;
            self.margin_bottom = g.margin_bottom;
            self.raise_margins_changed(RulerMarginsEventArgs { start: g.margin_top, end: g.margin_bottom, commit: true });
        }
        self.raise_drag_guide_changed(RulerGuideEventArgs::default());
        self.invalidate();
    }

    /// The active page's column and graduations, in the ruler's own coordinates.
    fn paint_ruler(&self, g: &Graphics<'_>) {
        let geo = self.geometry();
        let col = RulerColors::of(&g.theme_colors());
        let band = RULER_SZ - 1.0;
        let (top, bottom) = (geo.content_top(), geo.content_bottom());
        rule(g, col.column, Rect::new(0.0, top, band, bottom));
        rule(g, col.edge, Rect::new(0.0, top.round(), band, top.round() + 1.0));
        rule(g, col.edge, Rect::new(0.0, bottom.round(), band, bottom.round() + 1.0));
        // Graduations of the active page only, from its top margin.
        let from = geo.page_top.max(-20.0);
        let to = geo.paper_bottom().min(self.height + 20.0);
        let font = number_font();
        let right = StringFormat::generic_typographic()
            .with_alignment(StringAlignment::Far)
            .with_line_alignment(StringAlignment::Center)
            .with_flags(StringFormatFlags::NO_WRAP);
        for t in ticks(top, geo.zoom, from, to) {
            if t.at < geo.page_top - 1.0 || t.at > geo.paper_bottom() + 1.0 {
                continue;
            }
            let len = if t.whole { 8.0 } else { 4.0 };
            let y = t.at.round();
            rule(g, col.tick, Rect::new(band - len, y - 0.5, band, y + 0.5));
            if let Some(n) = t.label {
                // Right-aligned on x = 10 (`fillText(…, w - 10)` with `textAlign = right`): measured, so a
                // two-digit number starts at its true left edge instead of being cut by a layout box.
                let text = n.to_string();
                let size = g.measure_string(&text, &font, None, &right);
                let left = (RULER_SZ - 10.0 - size.width).max(0.0);
                g.draw_string(&text, &font, Brush::solid(col.tick), Rect::new(left, t.at - 7.0, left + size.width + 1.0, t.at + 7.0), &right.with_alignment(StringAlignment::Near));
            }
        }
    }
}

impl Control for VerticalRuler {
    fn get_preferred_size(&self, _canvas: &dyn Canvas, proposed: Size) -> Size {
        Size { width: RULER_SZ, height: proposed.height.max(200.0) }
    }

    /// The grey column and its right border.
    fn on_paint_background(&mut self, e: &mut PaintEventCx<'_>) {
        let b = e.clip_rectangle;
        let g = e.graphics;
        let col = RulerColors::of(&g.theme_colors());
        rule(g, col.band, Rect::new(b.left, b.top, b.left + RULER_SZ - 1.0, b.bottom));
        rule(g, col.border, Rect::new(b.left + RULER_SZ - 1.0, b.top, b.left + RULER_SZ, b.bottom));
    }

    fn on_paint(&mut self, e: &mut PaintEventCx<'_>) {
        let b = e.clip_rectangle;
        self.height = b.bottom - b.top;
        let g = e.graphics;
        g.with_saved(|g| {
            g.intersect_clip(b);
            g.translate_transform(b.left, b.top);
            self.paint_ruler(g);
        });
        e.raise(self, "OnPaint");
    }

    fn cursor_at(&self, _x: f32, y: f32) -> Option<kubuno::controls::host::Cursor> {
        (self.drag.is_some() || self.geometry().hit(y).is_some()).then_some(kubuno::controls::host::Cursor::ResizeNS)
    }

    fn tool_tip_at(&self, _x: f32, _y: f32) -> Option<String> {
        self.drag.is_none().then(|| crate::Resources::ruler_page_setup_hint().to_string())
    }

    fn on_mouse_down(&mut self, e: &mut EventCx<'_, MouseEventArgs>) {
        let (y, button, clicks) = (e.args().y, e.args().button, e.args().clicks);
        if button == MouseButton::Left && !self.design_mode() {
            if clicks >= 2 {
                self.raise_page_setup_requested(EmptyEventArgs);
            } else if let Some(part) = self.geometry().hit(y) {
                let start = self.geometry();
                self.drag = Some((part, start));
                self.live = Some(start);
                self.raise_guide(&start, part, y);
                self.invalidate();
            }
        }
        e.raise(&*self, "OnMouseDown");
    }

    fn on_mouse_move(&mut self, e: &mut EventCx<'_, MouseEventArgs>) {
        if let Some((part, start)) = self.drag {
            if e.args().button != MouseButton::Left {
                self.end_drag();
            } else {
                let g = start.dragged(part, e.args().y);
                if self.live != Some(g) {
                    self.live = Some(g);
                    self.raise_margins_changed(RulerMarginsEventArgs { start: g.margin_top, end: g.margin_bottom, commit: false });
                    self.raise_guide(&g, part, e.args().y);
                    self.invalidate();
                }
            }
        }
        e.raise(&*self, "OnMouseMove");
    }

    fn on_mouse_up(&mut self, e: &mut EventCx<'_, MouseEventArgs>) {
        self.end_drag();
        e.raise(&*self, "OnMouseUp");
    }
}
