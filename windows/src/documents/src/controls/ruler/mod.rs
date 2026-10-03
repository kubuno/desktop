//! The rulers of the page — a port of the web editor's (`office/frontend/src/DocumentEditorPage.tsx`,
//! `HorizontalRuler`, `VerticalRuler`, `CornerCell`): the same 20-DIP bands, centimetre graduations,
//! grey margins either side of the white text column, the four Word indent markers (first line,
//! hanging, left, right) hanging below the horizontal band, the tab stops and the tab-type selector
//! in the corner. Each is a custom control that draws itself with the Graphics API (`on_paint` and
//! `on_paint_background`, EVT-8); this module holds what they share: the web's metrics and colours,
//! the pure geometry (graduations, hit-testing, dragging — unit-tested below) and the marker shapes.
//!
//! Units: positions on the page are **document pixels** (CSS px at 96 dpi, the stored format's unit);
//! a ruler multiplies them by `Zoom` to get DIP, exactly as the web multiplies them to get CSS px.

mod corner;
mod horizontal;
mod vertical;

pub use corner::RulerCorner;
pub use horizontal::HorizontalRuler;
pub use vertical::VerticalRuler;

use kubuno::ui::graphics::{Brush, Color, Graphics, GraphicsPath, Pen, PointF};
use kubuno::ui::Rect;

// ── The web's metrics (DocumentEditorPage.tsx) ──────────────────────────────────────────────

/// `RULER_SZ`: the visible height of the horizontal ruler, the width of the vertical one.
pub const RULER_SZ: f32 = 20.0;
/// `RULER_OVERHANG`: how far the indent markers hang below the horizontal band (over the page).
pub const RULER_OVERHANG: f32 = 5.0;
/// `RULER_SNAP`: the grab radius of a marker or a margin edge.
pub const RULER_SNAP: f32 = 8.0;
/// Document pixels per centimetre (96 dpi).
pub const PX_PER_CM: f32 = 96.0 / 2.54;
/// The narrowest text column a margin drag leaves (`MIN_CONTENT`).
pub const MIN_CONTENT: f32 = 96.0;
/// The smallest gap between the left and right indents (`MINGAP`).
pub const MIN_GAP: f32 = 16.0;
/// Half the width of an indent marker (`IH`).
const MARKER_HALF: f32 = 4.5;

// ── Colours: theme tokens (the web's values in the light theme, « Kubuno Dark » in the dark one) ──

/// A marker's soft shadow (`rgba(32,33,36,0.22)`).
const MARKER_SHADOW: Color = Color::rgba_f(32.0 / 255.0, 33.0 / 255.0, 36.0 / 255.0, 0.22);

/// The rulers' colours, from the theme's tokens. In the light theme they are the web's rulers'
/// (`#f1f3f4` band and margins = `Surface2`, white column = `Surface`, `#bdc1c6` margin edges =
/// `BorderStrong`, `#5f6368` graduations = `TextSecondary`, `#1a73e8` markers = `Primary`); in the
/// dark theme the band is the dark chrome (`Surface1`, as the status bar) and the text column one
/// step lighter (`Surface2`), so the column still stands out of the margins.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RulerColors {
    /// The band and the margins.
    pub band: Color,
    /// The text column.
    pub column: Color,
    /// The margin edges.
    pub edge: Color,
    /// The band's own border, towards the page.
    pub border: Color,
    /// Graduations and numbers.
    pub tick: Color,
    /// An indent marker's outline and its fill.
    pub marker: Color,
    pub marker_fill: Color,
    /// The tab stops and the corner's symbol.
    pub tab_ink: Color,
    /// The corner under the pointer.
    pub hover: Color,
}

impl RulerColors {
    pub fn of(t: &kubuno::ui::Theme) -> Self {
        let dark = t.mode == kubuno::ui::ThemeMode::Dark;
        let c = |v: windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F| Color::from(v);
        let (band, column) = if dark { (c(t.card_background), c(t.surface_2)) } else { (c(t.surface_2), c(t.layer_background)) };
        Self {
            band,
            column,
            edge: c(t.border_strong),
            border: c(t.card_stroke),
            tick: c(t.text_secondary),
            marker: c(t.accent),
            marker_fill: if dark { band } else { column },
            tab_ink: c(t.text_primary),
            hover: c(t.surface_3),
        }
    }
}

/// The number font of the graduations (`9px Arial`).
pub fn number_font() -> kubuno::ui::graphics::Font {
    kubuno::ui::graphics::Font::with_dip("Arial", 9.0, kubuno::ui::graphics::FontStyle::REGULAR)
}

// ── Tab stops ───────────────────────────────────────────────────────────────────────────────

/// A tab stop's alignment (`TabType`), in the corner selector's cycle order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TabKind {
    #[default]
    Left,
    Center,
    Right,
    Decimal,
    Bar,
}

impl TabKind {
    /// Every kind, in the corner's cycle (`TAB_CYCLE`).
    pub const ALL: [TabKind; 5] = [TabKind::Left, TabKind::Center, TabKind::Right, TabKind::Decimal, TabKind::Bar];

    /// The stored name (`'left'`, `'center'`…), as the web writes it in `tabStops`.
    pub fn key(self) -> &'static str {
        match self {
            TabKind::Left => "left",
            TabKind::Center => "center",
            TabKind::Right => "right",
            TabKind::Decimal => "decimal",
            TabKind::Bar => "bar",
        }
    }

    /// The kind of a stored name; `Left` for anything else (the web's default).
    pub fn parse(key: &str) -> Self {
        Self::ALL.into_iter().find(|k| k.key() == key.trim()).unwrap_or_default()
    }

    /// The symbol drawn on the ruler and in the corner (`TAB_SYMBOL`).
    pub fn symbol(self) -> &'static str {
        match self {
            TabKind::Left => "⌞",
            TabKind::Center => "⊥",
            TabKind::Right => "⌟",
            TabKind::Decimal => "⊿",
            TabKind::Bar => "│",
        }
    }

    /// The next kind of the corner's cycle.
    pub fn next(self) -> Self {
        let i = Self::ALL.iter().position(|k| *k == self).unwrap_or(0);
        Self::ALL[(i + 1) % Self::ALL.len()]
    }

    /// Its name for the corner's tooltip.
    pub fn label(self) -> String {
        use crate::Resources as R;
        match self {
            TabKind::Left => R::tab_left(),
            TabKind::Center => R::tab_center(),
            TabKind::Right => R::tab_right(),
            TabKind::Decimal => R::tab_decimal(),
            TabKind::Bar => R::tab_bar(),
        }
        .to_string()
    }
}

/// One tab stop: where it is, in document pixels from the left margin, and its alignment.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TabStop {
    pub pos: f32,
    pub kind: TabKind,
}

/// Reads `TabStops` (`"48:left,120:right"`, the property's text form). Malformed entries are
/// skipped; the stops come back sorted by position.
pub fn parse_tab_stops(text: &str) -> Vec<TabStop> {
    let mut out: Vec<TabStop> = text
        .split(',')
        .filter_map(|part| {
            let mut it = part.split(':');
            let pos = it.next()?.trim().parse::<f32>().ok().filter(|p| p.is_finite())?;
            Some(TabStop { pos, kind: TabKind::parse(it.next().unwrap_or("left")) })
        })
        .collect();
    out.sort_by(|a, b| a.pos.total_cmp(&b.pos));
    out
}

/// Writes tab stops in `TabStops`' text form (`"48:left,120:right"`).
pub fn format_tab_stops(stops: &[TabStop]) -> String {
    stops.iter().map(|t| format!("{}:{}", crate::controls::ruler::number(t.pos), t.kind.key())).collect::<Vec<_>>().join(",")
}

/// A number as short text: no trailing `.0`.
pub fn number(v: f32) -> String {
    let r = (v * 100.0).round() / 100.0;
    if r.fract() == 0.0 {
        format!("{}", r as i64)
    } else {
        format!("{r}")
    }
}

/// A length in document pixels as the web's drag tooltip writes it: centimetres, two decimals.
pub fn cm_text(px: f32) -> String {
    format!("{:.2}", px / PX_PER_CM)
}

// ── Graduations ─────────────────────────────────────────────────────────────────────────────

/// One graduation: where (along the ruler, DIP), whether it is a whole centimetre (a long tick
/// and, but at the origin, its number) and the number it shows.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tick {
    pub at: f32,
    pub whole: bool,
    pub label: Option<u32>,
}

/// The graduations of a ruler whose origin (the 0, the text column's edge) is at `origin` DIP and
/// which spans `[from, to]` DIP: a short tick every half centimetre, a long one with its number
/// every centimetre, numbered in absolute value both ways from the origin (`« 2 1 · 1 2 3 »`, the
/// web's « Google Docs » style), no number at the origin.
pub fn ticks(origin: f32, zoom: f32, from: f32, to: f32) -> Vec<Tick> {
    let px_cm = PX_PER_CM * zoom.max(0.01);
    let start = ((from - origin) / px_cm * 10.0).floor() as i64 - 10;
    let end = ((to - origin) / px_cm * 10.0).ceil() as i64 + 10;
    (start..=end)
        .filter(|mm| mm % 5 == 0)
        .filter_map(|mm| {
            let at = origin + (mm as f32 / 10.0) * px_cm;
            if at < from - 1.0 || at > to + 1.0 {
                return None;
            }
            let whole = mm % 10 == 0;
            Some(Tick { at, whole, label: (whole && mm != 0).then(|| (mm / 10).unsigned_abs() as u32) })
        })
        .collect()
}

// ── Indents ─────────────────────────────────────────────────────────────────────────────────

/// A paragraph's indents, in document pixels (`indentLeft`, `indentFirstLine` — relative to the
/// left indent, negative for a hanging indent —, `indentRight`).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Indents {
    pub left: f32,
    pub first_line: f32,
    pub right: f32,
}

/// What the horizontal ruler's pointer can grab (`HRHit`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HorizontalPart {
    /// The left margin's edge (grabbed from the grey side).
    MarginLeft,
    /// The right margin's edge (grabbed from the grey side).
    MarginRight,
    /// The first-line indent (the « house » pointing down, at the top).
    FirstLine,
    /// The hanging indent (the « house » pointing up, under it).
    Hanging,
    /// The left indent (the small bar under the hanging marker): moves both.
    LeftIndent,
    /// The right indent.
    RightIndent,
}

impl HorizontalPart {
    /// Whether it is an indent marker (not a margin edge).
    pub fn is_indent(self) -> bool {
        !matches!(self, HorizontalPart::MarginLeft | HorizontalPart::MarginRight)
    }

    /// Its name, the tooltip over it (the web's `title`).
    pub fn label(self) -> String {
        use crate::Resources as R;
        match self {
            HorizontalPart::FirstLine => R::ruler_indent_first(),
            HorizontalPart::Hanging => R::ruler_indent_hanging(),
            HorizontalPart::LeftIndent => R::ruler_indent_left(),
            HorizontalPart::RightIndent => R::ruler_indent_right(),
            HorizontalPart::MarginLeft | HorizontalPart::MarginRight => R::ruler_page_setup_hint(),
        }
        .to_string()
    }
}

/// The horizontal ruler's geometry, in the ruler's own coordinates (DIP): the page's paper starts
/// at `origin`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HorizontalGeometry {
    pub page_width: f32,
    pub margin_left: f32,
    pub margin_right: f32,
    pub zoom: f32,
    pub origin: f32,
    pub indents: Indents,
}

impl HorizontalGeometry {
    /// The paper's width on screen (`w`).
    pub fn width(&self) -> f32 {
        (self.page_width * self.zoom).round()
    }

    /// The text column's left edge (`mlPx`), from the paper's left.
    pub fn ml(&self) -> f32 {
        self.margin_left * self.zoom
    }

    /// The right margin's width on screen (`mrPx`).
    pub fn mr(&self) -> f32 {
        self.margin_right * self.zoom
    }

    /// Where the indent markers stand, from the paper's left: `(first line, left/hanging, right)`.
    pub fn marker_xs(&self) -> (f32, f32, f32) {
        let (o, r) = (self.ml(), self.width() - self.mr());
        let i = self.indents;
        (o + (i.left + i.first_line) * self.zoom, o + i.left * self.zoom, r - i.right * self.zoom)
    }

    /// What is under `(mx, my)` (ruler coordinates): the indent markers first, by height (the
    /// first-line marker at the top, the left bar at the very bottom, the hanging one between),
    /// then the margin edges from their grey side — the web's `getHit`.
    pub fn hit(&self, mx: f32, my: f32) -> Option<HorizontalPart> {
        let mx = mx - self.origin;
        let (first_x, left_x, right_x) = self.marker_xs();
        let near = |a: f32, b: f32| (a - b).abs() <= RULER_SNAP;
        let (ml, w, mr) = (self.ml(), self.width(), self.mr());
        if my <= 12.0 && near(mx, first_x) {
            Some(HorizontalPart::FirstLine)
        } else if my >= 21.0 && near(mx, left_x) {
            Some(HorizontalPart::LeftIndent)
        } else if my > 12.0 && near(mx, left_x) {
            Some(HorizontalPart::Hanging)
        } else if my > 12.0 && near(mx, right_x) {
            Some(HorizontalPart::RightIndent)
        } else if mx < ml - 1.0 && near(mx, ml) {
            Some(HorizontalPart::MarginLeft)
        } else if mx > (w - mr) + 1.0 && near(mx, w - mr) {
            Some(HorizontalPart::MarginRight)
        } else {
            None
        }
    }

    /// The geometry after dragging `part` to `mx` (ruler coordinates), with the web's clamps: a
    /// margin keeps a 96-px text column; the first-line and hanging markers stay between the
    /// column's left edge and 16 px before the right indent; dragging the hanging marker keeps the
    /// first line where it is, dragging the left bar moves both; the right indent stays 16 px
    /// right of the rightmost of the other two.
    pub fn dragged(&self, part: HorizontalPart, mx: f32) -> HorizontalGeometry {
        let mut g = *self;
        let mx = mx - self.origin;
        let z = self.zoom.max(0.01);
        let w = self.width();
        match part {
            HorizontalPart::MarginLeft => {
                g.margin_left = (mx / z).min(self.page_width - MIN_CONTENT - self.margin_right).max(0.0);
            }
            HorizontalPart::MarginRight => {
                g.margin_right = ((w - mx) / z).min(self.page_width - MIN_CONTENT - self.margin_left).max(0.0);
            }
            _ => {
                let origin = self.ml();
                let column_right = w - self.mr();
                let right_x = column_right - self.indents.right * z;
                let clamp = |x: f32, lo: f32, hi: f32| x.min(hi).max(lo);
                let (old_left, old_first) = (self.indents.left, self.indents.first_line);
                match part {
                    HorizontalPart::FirstLine => {
                        let fx = clamp(mx, origin, right_x - MIN_GAP);
                        g.indents.first_line = (fx - origin) / z - old_left;
                    }
                    HorizontalPart::Hanging => {
                        let lx = clamp(mx, origin, right_x - MIN_GAP);
                        let left = (lx - origin) / z;
                        g.indents.left = left;
                        g.indents.first_line = (old_left + old_first) - left;
                    }
                    HorizontalPart::LeftIndent => {
                        let lx = clamp(mx, origin, right_x - MIN_GAP);
                        g.indents.left = (lx - origin) / z;
                    }
                    _ => {
                        let min_x = origin + old_left.max(old_left + old_first) * z + MIN_GAP;
                        let rx = clamp(mx, min_x, column_right);
                        g.indents.right = (column_right - rx) / z;
                    }
                }
            }
        }
        g
    }

    /// The tab stops after a click at `mx` (ruler coordinates): a click on an existing stop removes
    /// it, a click in the text column adds one of `kind` there (rounded to a whole pixel); `None`
    /// when the click changes nothing (`handleClick`).
    pub fn clicked_tabs(&self, stops: &[TabStop], mx: f32, kind: TabKind) -> Option<Vec<TabStop>> {
        let mx = mx - self.origin;
        let ml = self.ml();
        let near = |a: f32, b: f32| (a - b).abs() <= RULER_SNAP;
        if let Some(i) = stops.iter().position(|t| near(ml + t.pos * self.zoom, mx)) {
            let mut out = stops.to_vec();
            out.remove(i);
            return Some(out);
        }
        if mx < ml + 2.0 || mx > self.width() - self.mr() - 2.0 {
            return None;
        }
        let mut out = stops.to_vec();
        out.push(TabStop { pos: ((mx - ml) / self.zoom.max(0.01)).round(), kind });
        out.sort_by(|a, b| a.pos.total_cmp(&b.pos));
        Some(out)
    }
}

// ── The vertical ruler's geometry ───────────────────────────────────────────────────────────

/// What the vertical ruler's pointer can grab: the active page's top or bottom margin edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerticalPart {
    MarginTop,
    MarginBottom,
}

/// The vertical ruler's geometry, in its own coordinates (DIP): the active page's paper starts at
/// `page_top` (it scrolls with the page).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VerticalGeometry {
    pub page_height: f32,
    pub margin_top: f32,
    pub margin_bottom: f32,
    pub zoom: f32,
    pub page_top: f32,
}

impl VerticalGeometry {
    /// The text column's top edge (`contentTopY`).
    pub fn content_top(&self) -> f32 {
        self.page_top + self.margin_top * self.zoom
    }

    /// The text column's bottom edge (`contentBotY`).
    pub fn content_bottom(&self) -> f32 {
        self.content_top() + (self.page_height - self.margin_top - self.margin_bottom) * self.zoom
    }

    /// The paper's bottom edge.
    pub fn paper_bottom(&self) -> f32 {
        self.page_top + self.page_height * self.zoom
    }

    /// What is under `my`: a margin edge within the grab radius.
    pub fn hit(&self, my: f32) -> Option<VerticalPart> {
        if (my - self.content_top()).abs() <= RULER_SNAP {
            Some(VerticalPart::MarginTop)
        } else if (my - self.content_bottom()).abs() <= RULER_SNAP {
            Some(VerticalPart::MarginBottom)
        } else {
            None
        }
    }

    /// The geometry after dragging `part` to `my`, keeping a 96-px text column.
    pub fn dragged(&self, part: VerticalPart, my: f32) -> VerticalGeometry {
        let mut g = *self;
        let z = self.zoom.max(0.01);
        let doc = (my - self.page_top) / z;
        match part {
            VerticalPart::MarginTop => g.margin_top = doc.min(self.page_height - MIN_CONTENT - self.margin_bottom).max(0.0),
            VerticalPart::MarginBottom => g.margin_bottom = (self.page_height - doc).min(self.page_height - MIN_CONTENT - self.margin_top).max(0.0),
        }
        g
    }
}

// ── The markers ─────────────────────────────────────────────────────────────────────────────

/// A « house » pointing down: flat top, point at the bottom (the first-line indent).
fn house_down(cx: f32, top: f32, mid: f32, bottom: f32) -> GraphicsPath {
    let h = MARKER_HALF;
    let mut p = GraphicsPath::new();
    p.add_polygon(&[PointF::new(cx - h, top), PointF::new(cx + h, top), PointF::new(cx + h, mid), PointF::new(cx, bottom), PointF::new(cx - h, mid)]);
    p
}

/// A « house » pointing up: point at the top, flat base (the hanging and right indents).
fn house_up(cx: f32, top: f32, mid: f32, bottom: f32) -> GraphicsPath {
    let h = MARKER_HALF;
    let mut p = GraphicsPath::new();
    p.add_polygon(&[PointF::new(cx, top), PointF::new(cx + h, mid), PointF::new(cx + h, bottom), PointF::new(cx - h, bottom), PointF::new(cx - h, mid)]);
    p
}

/// Paints one marker: a soft shadow, a white fill, a thin blue outline (the web's `marker`).
fn paint_marker(g: &Graphics<'_>, col: &RulerColors, path: &GraphicsPath) {
    g.with_saved(|g| {
        g.translate_transform(0.0, 0.5);
        g.fill_path(Brush::solid(MARKER_SHADOW), path);
    });
    g.fill_path(Brush::solid(col.marker_fill), path);
    g.draw_path(&Pen::new(col.marker, 1.2), path);
}

/// Paints the four indent markers of a horizontal ruler whose paper starts at x = 0: the
/// first-line house (inside the band), the hanging house and the left bar (hanging below it) and
/// the right house.
pub fn paint_indent_markers(g: &Graphics<'_>, col: &RulerColors, geometry: &HorizontalGeometry) {
    let (first_x, left_x, right_x) = geometry.marker_xs();
    paint_marker(g, col, &house_down(first_x, 3.5, 8.0, 12.5));
    paint_marker(g, col, &house_up(left_x, 13.0, 17.0, 21.0));
    let mut bar = GraphicsPath::new();
    bar.add_rounded_rectangle(Rect::new(left_x - MARKER_HALF, 21.4, left_x + MARKER_HALF, 23.6), 1.0);
    paint_marker(g, col, &bar);
    paint_marker(g, col, &house_up(right_x, 13.0, 17.0, 21.0));
}

/// A filled 1-DIP rule.
pub fn rule(g: &Graphics<'_>, color: Color, rect: Rect) {
    g.fill_rectangle(Brush::solid(color), rect);
}

// ── Event args ──────────────────────────────────────────────────────────────────────────────

/// `MarginsChanged`: a margin edge was dragged. `start`/`end` are the left/right margins (the
/// horizontal ruler) or the top/bottom ones (the vertical ruler), in document pixels.
#[derive(kubuno::views::events::EventArgs, Debug, Clone, Default, PartialEq)]
pub struct RulerMarginsEventArgs {
    pub start: f32,
    pub end: f32,
    /// The drag ended (the button was released); `false` while it goes on.
    pub commit: bool,
}

/// `IndentsChanged`: an indent marker was dragged (document pixels).
#[derive(kubuno::views::events::EventArgs, Debug, Clone, Default, PartialEq)]
pub struct RulerIndentsEventArgs {
    pub left: f32,
    pub first_line: f32,
    pub right: f32,
    /// The drag ended; `false` while it goes on.
    pub commit: bool,
}

/// `TabStopsChanged`: a click added or removed a tab stop (`TabStops`' text form).
#[derive(kubuno::views::events::EventArgs, Debug, Clone, Default, PartialEq)]
pub struct RulerTabStopsEventArgs {
    pub tab_stops: String,
}

/// `DragGuideChanged`: where the dashed guide line goes over the page while a margin or an indent
/// is dragged, and the value to show beside it.
#[derive(kubuno::views::events::EventArgs, Debug, Clone, Default, PartialEq)]
pub struct RulerGuideEventArgs {
    /// A guide is shown (`false`: the drag ended, remove it).
    pub visible: bool,
    /// A vertical line at `position` DIP from the ruler's left (the horizontal ruler), else a
    /// horizontal line at `position` DIP from the ruler's top (the vertical ruler).
    pub vertical: bool,
    pub position: f32,
    /// The value tooltip (`« Gauche : 2.54 cm »`), empty for none (an indent).
    pub label: String,
    /// Where the pointer is along the ruler (DIP), to place the tooltip.
    pub pointer: f32,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a4(zoom: f32) -> HorizontalGeometry {
        HorizontalGeometry { page_width: 794.0, margin_left: 96.0, margin_right: 96.0, zoom, origin: 100.0, indents: Indents::default() }
    }

    #[test]
    fn graduations_number_whole_centimetres_both_ways_but_not_the_origin() {
        let t = ticks(100.0, 1.0, 0.0, 400.0);
        let px_cm = PX_PER_CM;
        let at = |cm: f32| 100.0 + cm * px_cm;
        let one = t.iter().find(|k| (k.at - at(1.0)).abs() < 0.01).expect("1 cm");
        assert!(one.whole && one.label == Some(1));
        let minus = t.iter().find(|k| (k.at - at(-2.0)).abs() < 0.01).expect("-2 cm");
        assert_eq!(minus.label, Some(2), "numbered in absolute value");
        let zero = t.iter().find(|k| (k.at - 100.0).abs() < 0.01).expect("origin");
        assert!(zero.whole && zero.label.is_none(), "no number at the origin");
        let half = t.iter().find(|k| (k.at - at(0.5)).abs() < 0.01).expect("half");
        assert!(!half.whole && half.label.is_none());
        assert!(t.iter().all(|k| k.at >= -1.0 && k.at <= 401.0));
    }

    #[test]
    fn graduations_follow_the_zoom() {
        let t = ticks(0.0, 2.0, 0.0, 200.0);
        let one = t.iter().find(|k| k.label == Some(1)).expect("1 cm");
        assert!((one.at - 2.0 * PX_PER_CM).abs() < 0.01);
    }

    #[test]
    fn the_markers_and_the_margin_edges_are_hit_where_the_web_hits_them() {
        let g = a4(1.0);
        let col = 100.0 + 96.0;
        assert_eq!(g.hit(col, 5.0), Some(HorizontalPart::FirstLine));
        assert_eq!(g.hit(col, 16.0), Some(HorizontalPart::Hanging));
        assert_eq!(g.hit(col, 22.0), Some(HorizontalPart::LeftIndent));
        assert_eq!(g.hit(100.0 + 794.0 - 96.0, 16.0), Some(HorizontalPart::RightIndent));
        // The left margin edge is grabbed from the grey side only (the markers own the white one).
        assert_eq!(g.hit(col - 4.0, 5.0), Some(HorizontalPart::FirstLine));
        assert_eq!(g.hit(col - 4.0, 16.0), Some(HorizontalPart::Hanging));
        let away = HorizontalGeometry { indents: Indents { left: 100.0, first_line: 0.0, right: 0.0 }, ..g };
        assert_eq!(away.hit(col - 4.0, 5.0), Some(HorizontalPart::MarginLeft));
        assert_eq!(away.hit(col + 4.0, 5.0), None);
        assert_eq!(g.hit(400.0, 5.0), None);
    }

    #[test]
    fn dragging_the_hanging_marker_keeps_the_first_line_in_place() {
        let g = HorizontalGeometry { indents: Indents { left: 40.0, first_line: 20.0, right: 0.0 }, ..a4(1.0) };
        let d = g.dragged(HorizontalPart::Hanging, 100.0 + 96.0 + 80.0);
        assert_eq!(d.indents.left, 80.0);
        assert_eq!(d.indents.left + d.indents.first_line, 60.0, "the first line did not move");
        let d = g.dragged(HorizontalPart::LeftIndent, 100.0 + 96.0 + 80.0);
        assert_eq!((d.indents.left, d.indents.first_line), (80.0, 20.0), "the left bar moves both");
        let d = g.dragged(HorizontalPart::FirstLine, 100.0 + 96.0 - 50.0);
        assert_eq!(d.indents.left + d.indents.first_line, 0.0, "clamped to the column's edge");
    }

    #[test]
    fn indents_keep_their_gap_and_margins_their_column() {
        let g = a4(2.0);
        let d = g.dragged(HorizontalPart::RightIndent, 100.0);
        assert!((d.indents.right * 2.0 - (g.width() - g.mr() - g.ml() - MIN_GAP)).abs() < 0.01);
        let d = g.dragged(HorizontalPart::MarginLeft, 100.0 + 2000.0);
        assert_eq!(d.margin_left, 794.0 - MIN_CONTENT - 96.0);
        let d = g.dragged(HorizontalPart::MarginRight, 100.0 + 2000.0);
        assert_eq!(d.margin_right, 0.0);
    }

    #[test]
    fn a_click_adds_a_tab_stop_in_the_column_and_a_second_removes_it() {
        let g = a4(1.0);
        let added = g.clicked_tabs(&[], 100.0 + 96.0 + 120.4, TabKind::Center).expect("added");
        assert_eq!(added, vec![TabStop { pos: 120.0, kind: TabKind::Center }]);
        assert_eq!(g.clicked_tabs(&added, 100.0 + 96.0 + 123.0, TabKind::Left), Some(vec![]));
        assert_eq!(g.clicked_tabs(&[], 120.0, TabKind::Left), None, "in the margin: nothing");
    }

    #[test]
    fn tab_stops_round_trip_through_their_text() {
        let stops = parse_tab_stops("120:right, 48:left,x:bar,  60.5:decimal");
        assert_eq!(stops.len(), 3);
        assert_eq!(stops[0], TabStop { pos: 48.0, kind: TabKind::Left });
        assert_eq!(format_tab_stops(&stops), "48:left,60.5:decimal,120:right");
        assert_eq!(TabKind::Bar.next(), TabKind::Left);
    }

    #[test]
    fn the_vertical_ruler_grabs_and_drags_the_margins_of_the_active_page() {
        let g = VerticalGeometry { page_height: 1123.0, margin_top: 96.0, margin_bottom: 96.0, zoom: 1.0, page_top: -200.0 };
        assert_eq!(g.hit(-200.0 + 96.0 + 3.0), Some(VerticalPart::MarginTop));
        assert_eq!(g.hit(-200.0 + 1123.0 - 96.0), Some(VerticalPart::MarginBottom));
        assert_eq!(g.hit(300.0), None);
        let d = g.dragged(VerticalPart::MarginTop, -200.0 + 150.0);
        assert_eq!(d.margin_top, 150.0);
        let d = g.dragged(VerticalPart::MarginBottom, 5000.0);
        assert_eq!(d.margin_bottom, 0.0);
    }

    #[test]
    fn a_length_reads_in_centimetres() {
        assert_eq!(cm_text(96.0), "2.54");
    }
}
