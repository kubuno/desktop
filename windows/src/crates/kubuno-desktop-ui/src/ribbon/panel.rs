//! The drop-down panel of the ribbon: what a gallery's « more » button, a drop-down gallery, a
//! colour picker's chevron and a menu holding an in-menu gallery open. Menu rows (like
//! `MenuDropdown`'s, same grid), section headers and grids of cells (gallery items or colour
//! swatches) stacked in one floating surface.

use kubuno_drive_desktop_app_controls::{Canvas, Rect};
use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;
use windows::Win32::Graphics::DirectWrite::IDWriteTextFormat;

use crate::lists::menu_metrics as mm;
use crate::metrics::{SHADOW_GREY, SHADOW_MENU};

use super::{icon_str, Icon};

/// One cell of a grid.
#[derive(Clone, Debug)]
pub struct Cell {
    pub value: String,
    pub label: String,
    pub icon: Option<Icon>,
    /// A colour swatch (or a gallery item previewing a colour).
    pub color: Option<D2D1_COLOR_F>,
    pub selected: bool,
}

/// One row of the panel.
#[derive(Clone, Debug)]
pub enum Row {
    Header(String),
    /// A menu row: `entry` is what it reports (an entry id, or `value:<v>` for a value).
    Entry { entry: String, label: String, icon: Option<Icon>, checked: bool, enabled: bool },
    Separator,
    Grid { cells: Vec<Cell>, columns: usize, cell_w: f32, cell_h: f32, swatch: bool },
}

/// What a row or cell hit reports.
#[derive(Clone, Debug, PartialEq)]
pub enum Pick {
    Entry(String),
    Value(String),
}

#[derive(Clone, Debug, Default)]
pub struct Panel {
    pub rows: Vec<Row>,
    /// `(row, cell)` under the pointer (`cell` is 0 for an entry).
    pub hot: Option<(usize, usize)>,
}

const HEADER_H: f32 = 26.0;
const ENTRY_H: f32 = 30.0;
const SEP_H: f32 = 11.0;
const GRID_PAD: f32 = 6.0;
const GRID_GAP: f32 = 2.0;
const SWATCH_GAP: f32 = 4.0;
const RADIUS: f32 = 8.0;

fn row_height(r: &Row) -> f32 {
    match r {
        Row::Header(_) => HEADER_H,
        Row::Entry { .. } => ENTRY_H,
        Row::Separator => SEP_H,
        Row::Grid { cells, columns, cell_h, swatch, .. } => {
            let n = cells.len().div_ceil((*columns).max(1)) as f32;
            let gap = if *swatch { SWATCH_GAP } else { GRID_GAP };
            GRID_PAD * 2.0 + n * cell_h + (n - 1.0).max(0.0) * gap
        }
    }
}

fn grid_width(columns: usize, cell_w: f32, swatch: bool) -> f32 {
    let gap = if swatch { SWATCH_GAP } else { GRID_GAP };
    GRID_PAD * 2.0 + columns as f32 * cell_w + (columns as f32 - 1.0).max(0.0) * gap
}

impl Panel {
    pub fn new(rows: Vec<Row>) -> Self {
        Self { rows, hot: None }
    }

    /// The panel's size.
    pub fn measure(&self, c: &dyn Canvas) -> (f32, f32) {
        let body = &c.formats().body;
        let mut w: f32 = mm::MIN_WIDTH;
        for r in &self.rows {
            match r {
                Row::Entry { label, .. } => w = w.max(mm::LABEL_LEFT + c.measure(label, body) + mm::CONTENT_RIGHT),
                Row::Header(t) => w = w.max(mm::ICON_LEFT + c.measure(t, body) + mm::CONTENT_RIGHT),
                Row::Grid { columns, cell_w, swatch, .. } => w = w.max(grid_width(*columns, *cell_w, *swatch) + mm::PANEL_PAD * 2.0),
                Row::Separator => {}
            }
        }
        let h = self.rows.iter().map(row_height).sum::<f32>() + mm::PANEL_PAD * 2.0;
        (w.ceil(), h.ceil())
    }

    fn row_rects(&self, bounds: Rect) -> Vec<Rect> {
        let mut y = bounds.top + mm::PANEL_PAD;
        self.rows
            .iter()
            .map(|r| {
                let h = row_height(r);
                let rect = Rect::new(bounds.left + mm::PANEL_PAD, y, bounds.right - mm::PANEL_PAD, y + h);
                y += h;
                rect
            })
            .collect()
    }

    fn cell_rects(row: &Row, rect: Rect) -> Vec<Rect> {
        let Row::Grid { cells, columns, cell_w, cell_h, swatch } = row else { return Vec::new() };
        let gap = if *swatch { SWATCH_GAP } else { GRID_GAP };
        let cols = (*columns).max(1);
        let w = grid_width(cols, *cell_w, *swatch);
        let left = rect.left + ((rect.right - rect.left) - w).max(0.0) / 2.0 + GRID_PAD;
        (0..cells.len())
            .map(|i| {
                let (cx, cy) = ((i % cols) as f32, (i / cols) as f32);
                let x = left + cx * (cell_w + gap);
                let y = rect.top + GRID_PAD + cy * (cell_h + gap);
                Rect::new(x, y, x + cell_w, y + cell_h)
            })
            .collect()
    }

    /// The row / cell at `(x, y)`, for the panel drawn at `bounds`.
    pub fn hit(&self, bounds: Rect, x: f32, y: f32) -> Option<(usize, usize)> {
        if !bounds.contains(x, y) {
            return None;
        }
        for (i, (row, rect)) in self.rows.iter().zip(self.row_rects(bounds)).enumerate() {
            if !rect.contains(x, y) {
                continue;
            }
            return match row {
                Row::Entry { enabled: true, .. } => Some((i, 0)),
                Row::Grid { .. } => Self::cell_rects(row, rect).iter().position(|r| r.contains(x, y)).map(|c| (i, c)),
                _ => None,
            };
        }
        None
    }

    /// Every hot target with its rectangle (the designer's regions, the tooltip anchors).
    pub fn targets(&self, bounds: Rect) -> Vec<((usize, usize), Rect)> {
        let mut out = Vec::new();
        for (i, (row, rect)) in self.rows.iter().zip(self.row_rects(bounds)).enumerate() {
            match row {
                Row::Entry { .. } => out.push(((i, 0), rect)),
                Row::Grid { .. } => out.extend(Self::cell_rects(row, rect).into_iter().enumerate().map(|(c, r)| ((i, c), r))),
                _ => {}
            }
        }
        out
    }

    /// What `(row, cell)` reports.
    pub fn pick(&self, at: (usize, usize)) -> Option<Pick> {
        match self.rows.get(at.0)? {
            Row::Entry { entry, .. } => Some(match entry.strip_prefix("value:") {
                Some(v) => Pick::Value(v.to_string()),
                None => Pick::Entry(entry.clone()),
            }),
            Row::Grid { cells, .. } => cells.get(at.1).map(|c| Pick::Value(c.value.clone())),
            _ => None,
        }
    }

    /// Paints the panel (shadow, surface, rows) at `bounds`.
    pub fn paint(&self, c: &dyn Canvas, bounds: Rect, item_font: &IDWriteTextFormat, accent: D2D1_COLOR_F) {
        let t = c.theme();
        c.draw_shadow(&bounds, RADIUS, &SHADOW_MENU, SHADOW_GREY);
        c.fill_rounded(&bounds, RADIUS, &t.layer_background);
        c.stroke_rounded(&bounds, RADIUS, &t.card_stroke);
        let body = &c.formats().body;
        for (i, (row, rect)) in self.rows.iter().zip(self.row_rects(bounds)).enumerate() {
            match row {
                Row::Header(text) => {
                    let r = Rect::new(rect.left + mm::ROW_PAD_L, rect.top, rect.right - mm::ROW_PAD_R, rect.bottom);
                    c.text(text, &r, &c.formats().caption, &t.text_secondary, false);
                }
                Row::Separator => {
                    let y = rect.top + mm::SEPARATOR_MARGIN_V;
                    c.fill_rounded(&Rect::new(rect.left + mm::SEPARATOR_MARGIN_H, y, rect.right - mm::SEPARATOR_MARGIN_H, y + mm::SEPARATOR_LINE), 0.0, &t.card_stroke);
                }
                Row::Entry { label, icon, checked, enabled, .. } => {
                    let ink = if *enabled { t.text_primary } else { D2D1_COLOR_F { a: t.text_primary.a * mm::DISABLED_ALPHA, ..t.text_primary } };
                    if *enabled && self.hot == Some((i, 0)) {
                        c.fill_rounded(&rect, 4.0, &t.row_hover);
                    }
                    let icon_r = Rect::new(bounds.left + mm::ICON_LEFT, rect.top, bounds.left + mm::ICON_LEFT + mm::ICON_CELL, rect.bottom);
                    if *checked {
                        c.vector_icon("Check", &icon_r, mm::ICON_GLYPH, &accent);
                    } else if let Some(icon) = icon {
                        c.vector_icon(icon_str(icon), &icon_r, mm::ICON_GLYPH, &ink);
                    }
                    let text_r = Rect::new(bounds.left + mm::LABEL_LEFT, rect.top, rect.right - mm::ROW_PAD_R, rect.bottom);
                    c.text_ellipsis(label, &text_r, body, &ink);
                }
                Row::Grid { cells, swatch, .. } => {
                    for (j, (cell, r)) in cells.iter().zip(Self::cell_rects(row, rect)).enumerate() {
                        let hot = self.hot == Some((i, j));
                        if *swatch {
                            let colour = cell.color.unwrap_or(t.layer_background);
                            c.fill_rounded(&r, 2.0, &colour);
                            c.stroke_rounded(&r, 2.0, &t.card_stroke);
                            if cell.selected || hot {
                                let o = Rect::new(r.left - 2.0, r.top - 2.0, r.right + 2.0, r.bottom + 2.0);
                                c.stroke_rounded_w(&o, 3.0, if cell.selected { &accent } else { &t.text_secondary }, 1.5);
                            }
                        } else {
                            paint_cell(c, cell, r, hot, item_font, accent);
                        }
                    }
                }
            }
        }
    }
}

/// A gallery cell: a colour, an icon over its label, or its label alone; hot and selected states.
pub fn paint_cell(c: &dyn Canvas, cell: &Cell, r: Rect, hot: bool, font: &IDWriteTextFormat, accent: D2D1_COLOR_F) {
    let t = c.theme();
    if hot {
        c.fill_rounded(&r, 2.0, &t.row_hover);
    }
    c.stroke_rounded(&r, 2.0, &t.card_stroke);
    if cell.selected {
        c.stroke_rounded_w(&r, 2.0, &accent, 1.5);
    }
    let inner = Rect::new(r.left + 4.0, r.top + 4.0, r.right - 4.0, r.bottom - 4.0);
    let tall = r.bottom - r.top >= 40.0;
    if let Some(colour) = cell.color {
        let sw = if tall { Rect::new(inner.left, inner.top, inner.right, inner.bottom - 14.0) } else { inner };
        c.fill_rounded(&sw, 2.0, &colour);
        if tall {
            c.text_ellipsis_center(&cell.label, &Rect::new(inner.left, inner.bottom - 14.0, inner.right, inner.bottom), font, &t.text_primary);
        }
    } else if let Some(icon) = &cell.icon {
        if tall {
            let ir = Rect::new(inner.left, inner.top, inner.right, inner.bottom - 14.0);
            c.vector_icon(icon_str(icon), &ir, 20.0, &t.text_primary);
            c.text_ellipsis_center(&cell.label, &Rect::new(inner.left, inner.bottom - 14.0, inner.right, inner.bottom), font, &t.text_primary);
        } else {
            c.vector_icon(icon_str(icon), &inner, 16.0, &t.text_primary);
        }
    } else {
        c.text_ellipsis_center(&cell.label, &inner, font, &t.text_primary);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid(n: usize, columns: usize) -> Row {
        let cells = (0..n).map(|i| Cell { value: format!("v{i}"), label: format!("c{i}"), icon: None, color: None, selected: false }).collect();
        Row::Grid { cells, columns, cell_w: 20.0, cell_h: 20.0, swatch: true }
    }

    #[test]
    fn cells_and_entries_are_hit_and_picked() {
        let p = Panel::new(vec![
            Row::Entry { entry: "value:".into(), label: "Automatique".into(), icon: None, checked: false, enabled: true },
            Row::Header("Couleurs".into()),
            grid(20, 10),
            Row::Separator,
            Row::Entry { entry: "more".into(), label: "Autres…".into(), icon: None, checked: false, enabled: true },
        ]);
        let bounds = Rect::new(0.0, 0.0, 300.0, 400.0);
        let targets = p.targets(bounds);
        assert_eq!(targets.len(), 22);
        let (at, r) = targets[1];
        assert_eq!(at, (2, 0));
        assert_eq!(p.hit(bounds, (r.left + r.right) / 2.0, (r.top + r.bottom) / 2.0), Some((2, 0)));
        assert_eq!(p.pick((2, 3)), Some(Pick::Value("v3".into())));
        assert_eq!(p.pick((0, 0)), Some(Pick::Value(String::new())));
        assert_eq!(p.pick((4, 0)), Some(Pick::Entry("more".into())));
        assert_eq!(p.pick((1, 0)), None, "a header is not a target");
    }
}
