//! `layoutTable` (`canvas-engine.ts:1363-1592`): a table's lines and cell geometry.
//!
//! The geometry — column widths (autofit and fixed), the occupancy map for spans, row heights,
//! alignment, cell spacing — is [`super::tables::layout_with`], the desktop's port of that function.
//! This module is its caller's half: it answers the two questions the geometry asks about each
//! cell's content (intrinsic widths, laid-out extent) with the **same** paragraph engine as the
//! body, then places the cells' lines (`:1560-1590`).

use std::collections::HashMap;

use super::flow::layout_paragraphs;
use super::tables::{self, CellContent, Grid};
use super::{DocPx, LayoutLine, LayoutParagraph, LayoutTable, RenderParagraph, RenderTable, SpanKind};
use crate::measure::Measure;
use crate::model::Node;

/// `contentWidths` (`:1389-1412`): the widest unbreakable unit and the unwrapped width of a cell's
/// paragraphs, padding excluded.
pub fn content_widths(paras: &[RenderParagraph], m: &dyn Measure) -> (DocPx, DocPx) {
    let (mut min, mut max) = (0.0f32, 0.0f32);
    for para in paras {
        let pad = para.indent + para.indent_right;
        let (mut sum, mut word) = (0.0f32, 0.0f32);
        for sp in &para.spans {
            match &sp.kind {
                SpanKind::Image(img) => {
                    sum += img.w;
                    min = min.max(img.w + pad);
                    word = 0.0;
                }
                SpanKind::Footnote { .. } | SpanKind::Endnote { .. } => {
                    let w = m.width(&sp.text, &sp.marks);
                    sum += w;
                    word += w;
                }
                SpanKind::Field { .. } => {
                    let w = m.width(&sp.text, &sp.marks);
                    sum += w;
                    min = min.max(w + pad);
                    word = 0.0;
                }
                _ => {
                    for part in super::paragraph::split_keeping_spaces(&sp.text) {
                        let w = m.width(part, &sp.marks);
                        sum += w;
                        if part.chars().all(super::paragraph::is_js_space) {
                            min = min.max(word + pad);
                            word = 0.0;
                        } else {
                            word += w;
                        }
                    }
                }
            }
        }
        min = min.max(word + pad);
        max = max.max(sum + pad);
    }
    (min, max)
}

/// The cells' content, laid out on demand and cached by width.
struct Content<'a> {
    table: &'a RenderTable,
    m: &'a dyn Measure,
    laid: HashMap<(usize, usize, u32), (Vec<LayoutParagraph>, DocPx)>,
}

impl Content<'_> {
    fn indices(&self, cell: &Node) -> Option<(usize, usize)> {
        for (r, row) in self.table.node.children().iter().enumerate() {
            for (c, n) in row.children().iter().enumerate() {
                if std::ptr::eq(n, cell) {
                    return Some((r, c));
                }
            }
        }
        None
    }

    fn paras(&self, r: usize, c: usize) -> &[RenderParagraph] {
        self.table.rows.get(r).and_then(|row| row.get(c)).map(|cell| cell.paras.as_slice()).unwrap_or(&[])
    }

    fn layout(&mut self, r: usize, c: usize, width: DocPx) -> &(Vec<LayoutParagraph>, DocPx) {
        let key = (r, c, width.to_bits());
        if !self.laid.contains_key(&key) {
            let paras = self.paras(r, c).to_vec();
            let (out, total) = layout_paragraphs(&paras, &|_| width, self.m);
            self.laid.insert(key, (out, total));
        }
        &self.laid[&key]
    }
}

impl CellContent for Content<'_> {
    fn intrinsic_widths(&mut self, cell: &Node) -> (DocPx, DocPx) {
        match self.indices(cell) {
            Some((r, c)) => content_widths(self.paras(r, c), self.m),
            None => (0.0, 0.0),
        }
    }

    fn extent(&mut self, cell: &Node, width: DocPx) -> DocPx {
        let Some((r, c)) = self.indices(cell) else { return 0.0 };
        let (paras, total) = self.layout(r, c, width);
        if width >= tables::VERTICAL_LAYOUT_WIDTH {
            // A vertical cell: its longest line becomes its extent (`maxLineW`).
            return paras
                .iter()
                .flat_map(|p| p.lines.iter())
                .filter_map(|l| l.spans.last().map(|s| s.x + s.width))
                .fold(0.0, f32::max);
        }
        *total
    }
}

/// Lays a table out at `content_w`. Lines and geometry are relative to the table's top (y = 0) and
/// the content column's left edge; returns the lines, the geometry and the height.
pub fn layout_table(rt: &RenderTable, content_w: DocPx, m: &dyn Measure) -> (Vec<LayoutLine>, LayoutTable, DocPx) {
    let mut content = Content { table: rt, m, laid: HashMap::new() };
    let (grid, cells) = tables::layout_with(&rt.node, content_w, &mut content);
    let mut lines = Vec::new();
    for cell in &cells {
        // QUIRK-free simplification: a vertical cell (`cellDir` 90/270) is laid out horizontally.
        let width = cell.inner_width(&grid);
        let (paras, total) = content.layout(cell.source_row, cell.source_cell, width).clone();
        let (ox, oy) = cell.content_origin(&grid, total);
        for para in paras {
            for mut ln in para.lines {
                for sp in &mut ln.spans {
                    sp.x += ox;
                }
                if let Some(cx) = ln.caret_x.as_mut() {
                    *cx += ox;
                }
                if let Some(img) = ln.image.as_mut() {
                    img.x += ox;
                }
                ln.y += oy;
                ln.baseline += oy;
                ln.cell_x = Some(cell.x - grid.spacing / 2.0);
                ln.cell_w = Some(cell.width + grid.spacing);
                lines.push(ln);
            }
        }
    }
    let height = grid.height;
    let table = LayoutTable {
        style: format!("{:?}", grid.style).to_lowercase(),
        accent: Some(grid.accent.clone()),
        grid,
        cells,
        dx: 0.0,
        dy: 0.0,
    };
    (lines, table, height)
}

/// The grid of a laid-out table (for painting borders).
pub fn grid_of(t: &LayoutTable) -> &Grid {
    &t.grid
}
