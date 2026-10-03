//! Component family `layout` — `<TableLayoutPanel>`, WinForms' grid container, over the
//! arithmetic of `kubuno_desktop_controls::layout_panels` (`resolve_tracks`, `place_cells`: the toolkit's
//! own track sizing and cell placement, measured against WinForms). Compiled with the
//! `family-layout` feature (on by default through `all-families`).
//!
//! Its children are placed in cells: `TableLayoutPanel.Row` / `TableLayoutPanel.Column` (attached
//! to the child; unset for the next free cell, row by row) and `TableLayoutPanel.RowSpan` /
//! `ColumnSpan`. `ColumnStyles` / `RowStyles` size the tracks: `Absolute 120` (DIP), `Percent 50`
//! (a share of what is left), `AutoSize` (the content), separated by `;`. A child fills its cell
//! (less its `Margin`). The designer treats it as a flow container: a dropped control goes to the
//! next free cell.

#[allow(unused_imports)] // Used by the `component!` invocation below.
use crate::registry::macros::component;
use crate::registry::ComponentMeta;

use crate::binding::{PropSource, ViewModel};
use crate::node::{PaintCx, ViewNode};
use crate::props::{BuildError, Props};

use kubuno_desktop_controls::layout_panels::{place_cells, resolve_tracks, CellSpec, SizeType, TableLayoutPanelGrowStyle, TrackStyle};
use kubuno_desktop_ui::{Canvas, Rect, Size};

component! {
    mod_name: table_layout_panel,
    name: "TableLayoutPanel",
    // Note: WinForms `TableLayoutPanel` (see this family's doc).
    doc: "A grid: its children are placed in rows and columns sized in pixels, in shares of the room left, or to their content.",
    ctor: kubuno_desktop_controls::layout_panels::TableLayoutPanel::new(),
    children: ChildrenModel::List(&[]),
    layout: LayoutKind::Flow,
    props: [
        PropertyMeta::new("ColumnCount", PropKind::F32, "2", "The number of columns.").category("Layout"),
        PropertyMeta::new("RowCount", PropKind::F32, "2", "The number of rows (more are added when the children need them, with GrowStyle AddRows).").category("Layout"),
        PropertyMeta::new("ColumnStyles", PropKind::String, "", "How each column is sized, separated by semicolons: Absolute 120, Percent 50 or AutoSize. A missing one is Percent 1 (an equal share).").category("Layout"),
        PropertyMeta::new("RowStyles", PropKind::String, "", "How each row is sized, separated by semicolons: Absolute 40, Percent 50 or AutoSize. A missing one is AutoSize.").category("Layout"),
        PropertyMeta::new("GrowStyle", PropKind::Enum(&["AddRows", "AddColumns", "FixedSize"]), "AddRows", "What happens when the children do not fit the declared grid.").category("Layout"),
        PropertyMeta::new("CellBorderStyle", PropKind::Enum(&["None", "Single"]), "None", "Lines drawn around and between the cells.").category("Appearance"),
        PropertyMeta::new("CellSpacing", PropKind::F32, "0", "Space between two cells, in DIP.").category("Layout"),
    ],
    events: [],
    smoke: |mut t| {
        t.column_count = 2;
        t.column_styles = vec![kubuno_desktop_controls::layout_panels::TrackStyle::absolute(120.0), kubuno_desktop_controls::layout_panels::TrackStyle::percent(100.0)];
        t
    },
    build: |props, cx| {
        crate::registry::families::layout::build_table(props, cx)
    },
}

/// Every component this family declares.
pub const ALL: &[ComponentMeta] = &[table_layout_panel::META];

/// Parses `ColumnStyles`/`RowStyles`: `Absolute 120; Percent 50; AutoSize`.
pub fn parse_styles(text: &str) -> Result<Vec<TrackStyle>, String> {
    text.split(';')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| {
            let mut words = s.split_whitespace();
            let kind = words.next().unwrap_or_default();
            let value = words.next().map(|v| v.trim_end_matches('%').parse::<f32>().map_err(|_| format!("`{s}`: expected a number after `{kind}`"))).transpose()?;
            match (kind, value) {
                ("Absolute", Some(v)) => Ok(TrackStyle::absolute(v)),
                ("Percent", Some(v)) => Ok(TrackStyle::percent(v)),
                ("Percent", None) => Ok(TrackStyle::percent(1.0)),
                ("AutoSize", _) => Ok(TrackStyle::auto()),
                _ => Err(format!("`{s}`: expected Absolute <n>, Percent <n> or AutoSize")),
            }
        })
        .collect()
}

fn literal(props: &Props<'_>, name: &str, default: &str) -> Result<String, BuildError> {
    match props.str(name, default)? {
        PropSource::Literal(s) => Ok(s),
        PropSource::Bound { .. } => Err(BuildError::new(format!("attribute `{name}` must be a literal value, not a binding"), props.element().name_range())),
    }
}

fn number(props: &Props<'_>, name: &str, default: f32) -> Result<f32, BuildError> {
    match props.f32(name, default)? {
        PropSource::Literal(v) => Ok(v),
        PropSource::Bound { .. } => Err(BuildError::new(format!("attribute `{name}` must be a literal value, not a binding"), props.element().name_range())),
    }
}

pub(crate) fn build_table(props: &Props<'_>, cx: &mut crate::props::BuildCx) -> Result<Box<dyn ViewNode>, BuildError> {
    let columns = parse_styles(&literal(props, "ColumnStyles", "")?).map_err(|e| BuildError::new(format!("attribute `ColumnStyles`: {e}"), props.element().name_range()))?;
    let rows = parse_styles(&literal(props, "RowStyles", "")?).map_err(|e| BuildError::new(format!("attribute `RowStyles`: {e}"), props.element().name_range()))?;
    let cell = |el: &crate::ast::Element, name: &str| el.attribute(name).and_then(|a| a.value()).and_then(|v| v.trim().parse::<f32>().ok());
    let specs: Vec<CellSpec> = props
        .element()
        .children()
        .map(|c| CellSpec {
            col: cell(&c, "TableLayoutPanel.Column").map_or(-1, |v| v.max(0.0) as i32),
            row: cell(&c, "TableLayoutPanel.Row").map_or(-1, |v| v.max(0.0) as i32),
            col_span: cell(&c, "TableLayoutPanel.ColumnSpan").map_or(1, |v| v.max(1.0) as u32),
            row_span: cell(&c, "TableLayoutPanel.RowSpan").map_or(1, |v| v.max(1.0) as u32),
        })
        .collect();
    let grow = match literal(props, "GrowStyle", "AddRows")?.as_str() {
        "AddColumns" => TableLayoutPanelGrowStyle::AddColumns,
        "FixedSize" => TableLayoutPanelGrowStyle::FixedSize,
        _ => TableLayoutPanelGrowStyle::AddRows,
    };
    Ok(Box::new(TableNode {
        column_count: number(props, "ColumnCount", 2.0)?.max(1.0) as u32,
        row_count: number(props, "RowCount", 2.0)?.max(1.0) as u32,
        column_styles: columns,
        row_styles: rows,
        grow,
        border: literal(props, "CellBorderStyle", "None")? == "Single",
        spacing: props.f32("CellSpacing", 0.0)?,
        sizes: props
            .element()
            .children()
            .map(|c| (cell(&c, "Width").filter(|v| *v > 0.0), cell(&c, "Height").filter(|v| *v > 0.0)))
            .collect(),
        specs,
        children: props.build_children(cx)?,
    }))
}

/// `<TableLayoutPanel>`'s live node.
pub struct TableNode {
    column_count: u32,
    row_count: u32,
    column_styles: Vec<TrackStyle>,
    row_styles: Vec<TrackStyle>,
    grow: TableLayoutPanelGrowStyle,
    border: bool,
    spacing: PropSource<f32>,
    specs: Vec<CellSpec>,
    /// Each child's own `Width` / `Height`, when it sets them: it keeps that size at the top left of
    /// its cell (WinForms' default anchor), else it fills the cell.
    sizes: Vec<(Option<f32>, Option<f32>)>,
    children: Vec<Box<dyn ViewNode>>,
}

impl TableNode {
    /// Every child's `(column, row)` and the grid's size.
    fn grid(&self) -> (Vec<(u32, u32)>, u32, u32) {
        let cells = place_cells(self.column_count, self.row_count, self.grow, &self.specs);
        let mut cols = self.column_count;
        let mut rows = self.row_count;
        if self.grow != TableLayoutPanelGrowStyle::FixedSize {
            for ((c, r), s) in cells.iter().zip(&self.specs) {
                cols = cols.max(c + s.col_span.max(1));
                rows = rows.max(r + s.row_span.max(1));
            }
        }
        (cells, cols.max(1), rows.max(1))
    }

    /// The styles of `n` tracks: the declared ones, then `fallback`.
    fn styles(declared: &[TrackStyle], n: u32, fallback: TrackStyle) -> Vec<TrackStyle> {
        (0..n as usize).map(|i| declared.get(i).copied().unwrap_or(fallback)).collect()
    }

    /// The column widths and row heights for a box of `width` × `height`.
    fn tracks(&self, c: &dyn Canvas, vm: &dyn ViewModel, width: f32, height: f32) -> (Vec<(u32, u32)>, Vec<f32>, Vec<f32>) {
        let (cells, cols, rows) = self.grid();
        let spacing = self.spacing.resolve(vm).max(0.0) + if self.border { 1.0 } else { 0.0 };
        let col_styles = Self::styles(&self.column_styles, cols, TrackStyle::percent(1.0));
        let row_styles = Self::styles(&self.row_styles, rows, TrackStyle::auto());
        // Auto tracks: the content of the single-cell children in them.
        let mut auto_cols = vec![0.0f32; cols as usize];
        let mut auto_rows = vec![0.0f32; rows as usize];
        for ((child, (col, row)), spec) in self.children.iter().zip(&cells).zip(&self.specs) {
            let s = child.measure(c, vm);
            if spec.col_span.max(1) == 1 {
                if let Some(a) = auto_cols.get_mut(*col as usize) {
                    *a = a.max(s.width);
                }
            }
            if spec.row_span.max(1) == 1 {
                if let Some(a) = auto_rows.get_mut(*row as usize) {
                    *a = a.max(s.height);
                }
            }
        }
        let avail_w = width - spacing * (cols.saturating_sub(1)) as f32 - if self.border { 2.0 } else { 0.0 };
        let avail_h = height - spacing * (rows.saturating_sub(1)) as f32 - if self.border { 2.0 } else { 0.0 };
        // An auto-sized height fits its content even when every row is auto (no slack to add).
        let widths = resolve_tracks(avail_w.max(0.0), &col_styles, &auto_cols);
        let heights = if row_styles.iter().all(|s| s.size_type == SizeType::AutoSize) { auto_rows } else { resolve_tracks(avail_h.max(0.0), &row_styles, &auto_rows) };
        (cells, widths, heights)
    }
}

impl ViewNode for TableNode {
    fn measure(&self, c: &dyn Canvas, vm: &dyn ViewModel) -> Size {
        let (_, widths, heights) = self.tracks(c, vm, 400.0, 0.0);
        let spacing = self.spacing.resolve(vm).max(0.0);
        let w: f32 = widths.iter().sum::<f32>() + spacing * widths.len().saturating_sub(1) as f32;
        let h: f32 = heights.iter().sum::<f32>() + spacing * heights.len().saturating_sub(1) as f32;
        Size::new(w.max(40.0), h.max(24.0))
    }

    fn paint(&mut self, cx: &mut PaintCx<'_>, bounds: Rect) {
        let canvas: &dyn Canvas = cx.canvas;
        let (cells, widths, heights) = self.tracks(canvas, &*cx.vm, bounds.right - bounds.left, bounds.bottom - bounds.top);
        let spacing = self.spacing.resolve(&*cx.vm).max(0.0) + if self.border { 1.0 } else { 0.0 };
        let edge = if self.border { 1.0 } else { 0.0 };
        let starts = |sizes: &[f32], origin: f32| -> Vec<f32> {
            let mut out = Vec::with_capacity(sizes.len() + 1);
            let mut at = origin + edge;
            for s in sizes {
                out.push(at);
                at += s + spacing;
            }
            out.push(at - spacing);
            out
        };
        let xs = starts(&widths, bounds.left);
        let ys = starts(&heights, bounds.top);
        if self.border || cx.design.is_some() {
            // The grid lines (dashed in the designer, where an empty cell must still show).
            let theme = canvas.theme();
            let color = if self.border { theme.card_stroke } else { theme.divider };
            let right = xs.last().copied().unwrap_or(bounds.right);
            let bottom = ys.last().copied().unwrap_or(bounds.bottom);
            for x in &xs {
                canvas.fill_rounded(&Rect::new(x - edge.max(0.5), bounds.top, *x, bottom), 0.0, &color);
            }
            for y in &ys {
                canvas.fill_rounded(&Rect::new(bounds.left, y - edge.max(0.5), right, *y), 0.0, &color);
            }
        }
        for (((child, (col, row)), spec), (own_w, own_h)) in self.children.iter_mut().zip(&cells).zip(&self.specs).zip(&self.sizes) {
            let (c0, r0) = (*col as usize, *row as usize);
            let (c1, r1) = ((c0 + spec.col_span.max(1) as usize).min(widths.len()), (r0 + spec.row_span.max(1) as usize).min(heights.len()));
            if c0 >= widths.len() || r0 >= heights.len() {
                continue; // Past a fixed-size grid: not placed (WinForms throws).
            }
            let left = xs[c0];
            let top = ys[r0];
            let right = xs[c1.max(c0 + 1)] - if c1 < widths.len() { spacing } else { 0.0 };
            let bottom = ys[r1.max(r0 + 1)] - if r1 < heights.len() { spacing } else { 0.0 };
            let right = own_w.map_or(right, |w| (left + w).min(right));
            let bottom = own_h.map_or(bottom, |h| (top + h).min(bottom));
            let mut inner = cx.reborrow();
            child.paint(&mut inner, Rect::new(left, top, right.max(left), bottom.max(top)));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn styles_parse() {
        let s = parse_styles("Absolute 120; Percent 50%;AutoSize; Percent").expect("valid");
        assert_eq!(s.len(), 4);
        assert_eq!(s[0], TrackStyle::absolute(120.0));
        assert_eq!(s[1], TrackStyle::percent(50.0));
        assert_eq!(s[2], TrackStyle::auto());
        assert_eq!(s[3], TrackStyle::percent(1.0));
        assert!(parse_styles("Absolute").is_err());
        assert!(parse_styles("Wide 3").is_err());
    }

    #[test]
    fn a_table_compiles_and_places_its_children() {
        let src = r#"<TableLayoutPanel ColumnCount="2" ColumnStyles="Absolute 100; Percent 100" RowStyles="Absolute 30; Absolute 30"><Label Text="Nom"/><TextField/><Label Text="Rôle" TableLayoutPanel.Row="1" TableLayoutPanel.Column="0"/><TextField TableLayoutPanel.Row="1" TableLayoutPanel.Column="1"/></TableLayoutPanel>"#;
        let view = crate::compile::compile(src);
        assert!(view.is_ok(), "{:?}", view.err());
        let bad = r#"<TableLayoutPanel ColumnStyles="Huge 3"/>"#;
        assert!(crate::compile::compile(bad).is_err());
    }
}
