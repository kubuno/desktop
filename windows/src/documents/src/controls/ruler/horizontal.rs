//! `HorizontalRuler` — the ruler above the page (the web's `HorizontalRuler`): a 20-DIP band with
//! the page's paper laid on it at `Origin` (the page's left edge, so it follows the page as it is
//! zoomed or centred), its margins grey and its text column white, centimetre graduations counted
//! from the left margin, the current paragraph's indent markers and tab stops.
//!
//! It is laid over the top of the page area, 25 DIP high: the band is the top 20 DIP and the
//! markers hang 5 DIP below it, over the page. Its background (`on_paint_background`) only paints
//! the band, so that overhang stays **transparent** and the page shows through around the markers.
//!
//! Interaction, as on the web: drag a margin edge from its grey side (a dashed guide crosses the
//! page and a tooltip gives the margin in centimetres), drag an indent marker (the hanging marker
//! keeps the first line in place, the bar under it moves both), click the text column to add a
//! tab stop of the corner's type, click a stop to remove it, double-click a marker for the
//! paragraph's indents or the band for the page setup. Every change is raised as an event; the
//! form applies it to the document and binds the result back.

use kubuno_desktop::prelude::*;
use kubuno_desktop::ui::graphics::{Brush, Graphics, StringAlignment, StringFormat, StringFormatFlags};
use kubuno_desktop::ui::{Canvas, Rect, Size};
use kubuno_desktop::views::component::{Component, Control, ControlCore, EventCx, PaintEventCx};
use kubuno_desktop::views::events::{EmptyEventArgs, Event};

use super::{
    cm_text, format_tab_stops, number_font, paint_indent_markers, parse_tab_stops, rule, ticks, HorizontalGeometry, HorizontalPart, Indents, RulerGuideEventArgs,
    RulerIndentsEventArgs, RulerMarginsEventArgs, RulerTabStopsEventArgs, RulerColors, TabKind, RULER_OVERHANG, RULER_SZ,
};

/// The ruler above the page (see the module doc).
#[derive(kubuno_desktop::views::component::Component)]
#[kubuno(extends = Control, overrides(Control))]
#[category("Documents")]
#[toolbox(icon = "ruler")]
#[default_event("IndentsChanged")]
pub struct HorizontalRuler {
    base: ControlCore,
    /// The page's width, in document pixels (A4: 794).
    #[property(bindable)]
    #[category("Page")]
    #[default_value(794.0)]
    pub page_width: f32,
    /// The left margin, in document pixels.
    #[property(bindable)]
    #[category("Page")]
    #[default_value(96.0)]
    pub margin_left: f32,
    /// The right margin, in document pixels.
    #[property(bindable)]
    #[category("Page")]
    #[default_value(96.0)]
    pub margin_right: f32,
    /// The page's zoom (1 = 100 %).
    #[property(bindable)]
    #[category("Page")]
    #[default_value(1.0)]
    pub zoom: f32,
    /// Where the page's paper starts, in DIP from the ruler's left edge (the page's left edge in
    /// the page area under it). `NaN`: centred in the ruler.
    #[property(bindable)]
    #[category("Page")]
    pub origin: f32,
    /// The current paragraph's left indent, in document pixels.
    #[property(bindable)]
    #[category("Paragraph")]
    pub indent_left: f32,
    /// The current paragraph's first-line indent, relative to its left indent (negative: hanging).
    #[property(bindable)]
    #[category("Paragraph")]
    pub indent_first_line: f32,
    /// The current paragraph's right indent, in document pixels.
    #[property(bindable)]
    #[category("Paragraph")]
    pub indent_right: f32,
    /// The current paragraph's tab stops: `pos:kind` pairs, comma-separated (`48:left,120:right`).
    #[property(bindable)]
    #[category("Paragraph")]
    pub tab_stops: String,
    /// The kind of tab stop a click adds (`left`, `center`, `right`, `decimal`, `bar`): the
    /// corner's selection.
    #[property(bindable)]
    #[category("Paragraph")]
    pub tab_type: String,
    /// Occurs while a margin edge is dragged, and once more when it is released (`commit`).
    #[event]
    #[category("Action")]
    pub margins_changed: Event<RulerMarginsEventArgs>,
    /// Occurs while an indent marker is dragged, and once more when it is released (`commit`).
    #[event]
    #[category("Action")]
    pub indents_changed: Event<RulerIndentsEventArgs>,
    /// Occurs when a click adds or removes a tab stop.
    #[event]
    #[category("Action")]
    pub tab_stops_changed: Event<RulerTabStopsEventArgs>,
    /// Occurs when the dashed guide over the page appears, moves or goes (a margin drag).
    #[event]
    #[category("Action")]
    pub drag_guide_changed: Event<RulerGuideEventArgs>,
    /// Occurs when an indent marker is double-clicked (the paragraph's indents dialog).
    #[event]
    #[category("Action")]
    pub paragraph_dialog_requested: Event<EmptyEventArgs>,
    /// Occurs when the band is double-clicked away from the markers (the page setup dialog).
    #[event]
    #[category("Action")]
    pub page_setup_requested: Event<EmptyEventArgs>,
    /// The drag in progress: what was grabbed and the geometry when it was.
    drag: Option<(HorizontalPart, HorizontalGeometry)>,
    /// The geometry while dragging (shown instead of the properties until the drag ends).
    live: Option<HorizontalGeometry>,
    /// A press on the band that may become a click (a tab stop).
    pressed: bool,
    /// The width of the last paint (a centred page needs it).
    width: f32,
}

impl Default for HorizontalRuler {
    fn default() -> Self {
        Self {
            base: ControlCore::default(),
            page_width: 794.0,
            margin_left: 96.0,
            margin_right: 96.0,
            zoom: 1.0,
            origin: f32::NAN,
            indent_left: 0.0,
            indent_first_line: 0.0,
            indent_right: 0.0,
            tab_stops: String::new(),
            tab_type: "left".to_string(),
            margins_changed: Event::default(),
            indents_changed: Event::default(),
            tab_stops_changed: Event::default(),
            drag_guide_changed: Event::default(),
            paragraph_dialog_requested: Event::default(),
            page_setup_requested: Event::default(),
            drag: None,
            live: None,
            pressed: false,
            width: 0.0,
        }
    }
}

impl HorizontalRuler {
    /// The geometry shown: the drag's, else the properties' (with a sample paragraph on the design
    /// surface, so the markers and tab stops can be seen there).
    pub fn geometry(&self) -> HorizontalGeometry {
        if let Some(live) = self.live {
            return live;
        }
        let zoom = if self.zoom > 0.0 { self.zoom } else { 1.0 };
        let paper = (self.page_width * zoom).round();
        let origin = if self.origin.is_finite() { self.origin } else { ((self.width - paper) / 2.0).floor().max(0.0) };
        let mut indents = Indents { left: self.indent_left, first_line: self.indent_first_line, right: self.indent_right };
        if self.design_mode() && indents == Indents::default() {
            indents = Indents { left: 24.0, first_line: 24.0, right: 0.0 };
        }
        HorizontalGeometry { page_width: self.page_width, margin_left: self.margin_left, margin_right: self.margin_right, zoom, origin, indents }
    }

    /// The tab stops shown (a sample set on the design surface).
    fn stops(&self) -> Vec<super::TabStop> {
        if self.design_mode() && self.tab_stops.trim().is_empty() {
            return parse_tab_stops("120:left,300:center,500:right");
        }
        parse_tab_stops(&self.tab_stops)
    }

    fn raise_guide(&mut self, g: &HorizontalGeometry, part: HorizontalPart, pointer: f32) {
        use crate::Resources as R;
        let (position, label) = match part {
            HorizontalPart::MarginLeft => (g.origin + g.ml(), R::margin_left_cm().replace("{value}", &cm_text(g.margin_left))),
            HorizontalPart::MarginRight => (g.origin + g.width() - g.mr(), R::margin_right_cm().replace("{value}", &cm_text(g.margin_right))),
            _ => return,
        };
        self.raise_drag_guide_changed(RulerGuideEventArgs { visible: true, vertical: true, position, label, pointer });
    }

    /// Raises what a drag step (or its end, `commit`) changed.
    fn raise_drag(&mut self, part: HorizontalPart, g: HorizontalGeometry, commit: bool) {
        if part.is_indent() {
            let i = g.indents;
            self.raise_indents_changed(RulerIndentsEventArgs { left: i.left, first_line: i.first_line, right: i.right, commit });
        } else {
            self.raise_margins_changed(RulerMarginsEventArgs { start: g.margin_left, end: g.margin_right, commit });
        }
    }

    /// Ends the drag in progress: the last values become the properties (until the form binds
    /// the document's back), the guide goes.
    fn end_drag(&mut self) {
        let Some((part, _)) = self.drag.take() else { return };
        if let Some(g) = self.live.take() {
            self.margin_left = g.margin_left;
            self.margin_right = g.margin_right;
            self.indent_left = g.indents.left;
            self.indent_first_line = g.indents.first_line;
            self.indent_right = g.indents.right;
            self.raise_drag(part, g, true);
            if !part.is_indent() {
                self.raise_drag_guide_changed(RulerGuideEventArgs::default());
            }
        }
        self.invalidate();
    }

    /// The band, the paper and its graduations, in the ruler's own coordinates.
    fn paint_ruler(&self, g: &Graphics<'_>) {
        let geo = self.geometry();
        let col = RulerColors::of(&g.theme_colors());
        let band = RULER_SZ - 1.0;
        let (w, ml, mr) = (geo.width(), geo.ml(), geo.mr());
        g.with_saved(|g| {
            g.translate_transform(geo.origin, 0.0);
            // The margins grey, the text column white, the margin edges.
            rule(g, col.band, Rect::new(0.0, 0.0, w, band));
            rule(g, col.column, Rect::new(ml, 0.0, w - mr, band));
            rule(g, col.edge, Rect::new(ml.round() - 0.5, 0.0, ml.round() + 0.5, band));
            rule(g, col.edge, Rect::new((w - mr).round() - 0.5, 0.0, (w - mr).round() + 0.5, band));
            // Graduations: half-centimetre ticks, numbered centimetres, from the left margin.
            let font = number_font();
            let centred = StringFormat::generic_typographic().with_alignment(StringAlignment::Center).with_flags(StringFormatFlags::NO_WRAP);
            for t in ticks(ml, geo.zoom, 0.0, w) {
                let len = if t.whole { 8.0 } else { 4.0 };
                let x = t.at.round();
                rule(g, col.tick, Rect::new(x - 0.5, band - len, x + 0.5, band));
                if let Some(n) = t.label {
                    g.draw_string(&n.to_string(), &font, Brush::solid(col.tick), Rect::new(t.at - 16.0, 0.0, t.at + 16.0, 12.0), &centred);
                }
            }
            // The current paragraph's indent markers, over the band and below it.
            paint_indent_markers(g, &col, &geo);
            // Its tab stops, at the bottom of the band.
            let tab_font = kubuno_desktop::ui::graphics::Font::with_dip("Arial", 11.0, kubuno_desktop::ui::graphics::FontStyle::REGULAR);
            let bottom = centred.with_line_alignment(StringAlignment::Far);
            for t in self.stops() {
                let x = ml + t.pos * geo.zoom;
                if x < ml - 2.0 || x > w - mr + 2.0 {
                    continue;
                }
                g.draw_string(t.kind.symbol(), &tab_font, Brush::solid(col.tab_ink), Rect::new(x - 8.0, 0.0, x + 8.0, RULER_SZ + 1.0), &bottom);
            }
        });
    }
}

impl Control for HorizontalRuler {
    fn get_preferred_size(&self, _canvas: &dyn Canvas, proposed: Size) -> Size {
        Size { width: proposed.width.max(200.0), height: RULER_SZ + RULER_OVERHANG }
    }

    /// Only the band: the 5 DIP below it, where the markers hang over the page, stay transparent.
    fn on_paint_background(&mut self, e: &mut PaintEventCx<'_>) {
        let b = e.clip_rectangle;
        let g = e.graphics;
        let col = RulerColors::of(&g.theme_colors());
        rule(g, col.band, Rect::new(b.left, b.top, b.right, b.top + RULER_SZ - 1.0));
        rule(g, col.border, Rect::new(b.left, b.top + RULER_SZ - 1.0, b.right, b.top + RULER_SZ));
    }

    fn on_paint(&mut self, e: &mut PaintEventCx<'_>) {
        let b = e.clip_rectangle;
        self.width = b.right - b.left;
        let g = e.graphics;
        g.with_saved(|g| {
            g.translate_transform(b.left, b.top);
            self.paint_ruler(g);
        });
        e.raise(self, "OnPaint");
    }

    fn cursor_at(&self, x: f32, y: f32) -> Option<kubuno_desktop::controls::host::Cursor> {
        if self.drag.is_some() || self.geometry().hit(x, y).is_some() {
            return Some(kubuno_desktop::controls::host::Cursor::ResizeEW);
        }
        None
    }

    fn tool_tip_at(&self, x: f32, y: f32) -> Option<String> {
        if self.drag.is_some() {
            return None;
        }
        match self.geometry().hit(x, y) {
            Some(part) if part.is_indent() => Some(part.label()),
            _ if y < RULER_SZ => Some(crate::Resources::ruler_page_setup_hint().to_string()),
            _ => None,
        }
    }

    fn on_mouse_down(&mut self, e: &mut EventCx<'_, MouseEventArgs>) {
        let (x, y, button, clicks) = (e.args().x, e.args().y, e.args().button, e.args().clicks);
        if button == MouseButton::Left {
            let hit = self.geometry().hit(x, y);
            if clicks >= 2 {
                // A double click: a marker opens the paragraph's indents, the band the page setup.
                match hit {
                    Some(part) if part.is_indent() => self.raise_paragraph_dialog_requested(EmptyEventArgs),
                    _ if y < RULER_SZ => self.raise_page_setup_requested(EmptyEventArgs),
                    _ => {}
                }
            } else if let Some(part) = hit {
                let start = self.geometry();
                self.drag = Some((part, start));
                self.live = Some(start);
                self.raise_guide(&start, part, x - start.origin);
                self.invalidate();
            } else {
                // A press in the overhang (below the band) is not a click on the ruler.
                self.pressed = y < RULER_SZ;
            }
        }
        e.raise(&*self, "OnMouseDown");
    }

    fn on_mouse_move(&mut self, e: &mut EventCx<'_, MouseEventArgs>) {
        if let Some((part, start)) = self.drag {
            if e.args().button != MouseButton::Left {
                // Released outside the window: the drag ends where it was.
                self.end_drag();
            } else {
                let g = start.dragged(part, e.args().x);
                if self.live != Some(g) {
                    self.live = Some(g);
                    self.raise_drag(part, g, false);
                    self.raise_guide(&g, part, e.args().x - g.origin);
                    self.invalidate();
                }
            }
        }
        e.raise(&*self, "OnMouseMove");
    }

    fn on_mouse_up(&mut self, e: &mut EventCx<'_, MouseEventArgs>) {
        if self.drag.is_some() {
            self.end_drag();
        } else if std::mem::take(&mut self.pressed) && e.args().y < RULER_SZ {
            let geo = self.geometry();
            let kind = TabKind::parse(&self.tab_type);
            if let Some(stops) = geo.clicked_tabs(&self.stops(), e.args().x, kind) {
                self.tab_stops = format_tab_stops(&stops);
                let tab_stops = self.tab_stops.clone();
                self.raise_tab_stops_changed(RulerTabStopsEventArgs { tab_stops });
                self.invalidate();
            }
        }
        e.raise(&*self, "OnMouseUp");
    }
}
