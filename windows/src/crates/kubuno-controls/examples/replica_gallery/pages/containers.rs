//! `04-containers` — Panel, GroupBox, FlowLayoutPanel, TableLayoutPanel,
//! TabControl / TabPage, SplitContainer.
//!
//! Four of these paint their frame but not their content: `TabControl` owns no
//! children, `SplitContainer` publishes its three rectangles, `TableLayoutPanel`
//! publishes its track sizes, and `FlowLayoutPanel` publishes its packing. The
//! page therefore decorates them — with library controls, placed against the
//! geometry those very controls resolved.

use kubuno_controls::buttons::{Button, RadioButton};
use kubuno_controls::containers::{border_thickness, content_origin, GroupBox, Panel, Point};
use kubuno_controls::labels::Label;
use kubuno_controls::layout_panels::{
    place_cells, CellSpec, FlowChild, FlowLayoutPanel, SplitContainer, TabControl, TabPage,
    TableLayoutPanel, TableLayoutPanelCellBorderStyle,
};
use kubuno_controls::{BorderStyle, Canvas, Control, ControlCanvas, Padding, Rect};

use crate::sheet::{group, kid, Group, Sheet};

pub fn build() -> Sheet {
    Sheet::new(vec![panel(), group_box(), flow(), table(), tabs(), split()])
}

// ── Small placement helpers ──────────────────────────────────────────────────

/// A label sized by its own `preferred_size` and parked at `(x, y)`.
fn label_at(c: &dyn Canvas, text: &str, x: f32, y: f32) -> Label {
    let mut l = Label::new();
    l.text = text.to_string();
    let s = l.preferred_size(c);
    l.set_bounds(Rect::new(x, y, x + s.width, y + s.height));
    l
}

fn button_at(c: &dyn Canvas, text: &str, x: f32, y: f32, w: f32) -> Button {
    let mut b = Button::new();
    b.text = text.to_string();
    let s = b.preferred_size(c);
    b.set_bounds(Rect::new(x, y, x + w.max(s.width), y + s.height));
    b
}

fn paint_at(c: &dyn ControlCanvas, ctrl: &dyn Control) {
    ctrl.paint(c, ctrl.control().bounds);
}

/// Moves a rectangle a container resolved in its **local** space into canvas
/// space.
///
/// The controls below publish their geometry the way the library lays it out —
/// parent-relative — so a demo that paints a stand-in control against that
/// geometry has to cross into canvas space itself. It asks the container where
/// its local origin landed (`content_origin`) instead of re-deriving padding,
/// border and caption band, which is exactly the duplication that let the page
/// and the controls disagree before.
fn at(r: Rect, o: Point) -> Rect {
    Rect::new(r.left + o.x, r.top + o.y, r.right + o.x, r.bottom + o.y)
}

// ── Panel ────────────────────────────────────────────────────────────────────

fn bordered_panel() -> Panel {
    let mut p = Panel::new();
    p.border_style = BorderStyle::FixedSingle;
    p.auto_scroll = true;
    p
}

/// The panel's own `paint` clips its children, which is what makes the
/// over-wide label read as scrollable content cut at the frame — the same thing
/// the reference sheet shows.
fn panel() -> Group {
    group(
        "Panel — BorderStyle / AutoScroll",
        300.0,
        vec![kid(bordered_panel()).size(260.0, 90.0).decor(|c, r| {
            let mut p = bordered_panel();
            p.set_bounds(r);
            // A child of a container is positioned in that container's LOCAL
            // space; `Panel::paint` translates it. Adding `r.left`/`r.top` here
            // would apply the panel's own origin a second time.
            p.children.push(Box::new(button_at(c, "inside panel", 8.0, 8.0, 120.0)));
            // 300 DIP inside a 260 DIP panel: the overflow is what `AutoScroll`
            // would scroll to, and what the panel's own clip cuts off.
            let mut wide = label_at(c, "AutoScroll content →", 8.0, 44.0);
            let h = wide.height();
            wide.set_size(kubuno_controls::Size::new(300.0, h));
            p.children.push(Box::new(wide));
            p.paint(c, r);
        })],
    )
}

// ── GroupBox ─────────────────────────────────────────────────────────────────

fn group_box() -> Group {
    group(
        "GroupBox",
        300.0,
        vec![kid(captioned()).size(260.0, 90.0).decor(|c, r| {
            let mut g = captioned();
            g.set_bounds(r);
            // Client coordinates: `y` is measured from the group box's own top,
            // so these clear the caption band themselves — the display rect's
            // origin is what the LAYOUT uses, not what a hand-placed child does.
            let radio = |text: &str, checked: bool, y: f32| {
                let mut b = RadioButton::new();
                b.text = text.to_string();
                b.checked = checked;
                let s = b.preferred_size(c);
                b.set_bounds(Rect::new(10.0, y, 10.0 + s.width, y + s.height));
                b
            };
            g.children.push(Box::new(radio("grouped A", true, 24.0)));
            g.children.push(Box::new(radio("grouped B", false, 50.0)));
            g.paint(c, r);
        })],
    )
}

fn captioned() -> GroupBox {
    let mut g = GroupBox::new();
    g.text = "GroupBox caption".to_string();
    g
}

// ── FlowLayoutPanel ──────────────────────────────────────────────────────────

fn flow_panel() -> FlowLayoutPanel {
    let mut f = FlowLayoutPanel::new();
    f.base.border_style = BorderStyle::FixedSingle;
    f
}

/// Six equal buttons in a wrapping left-to-right run: the panel's own `arrange`
/// decides where they land, so the wrap is the library's, not the demo's.
fn flow() -> Group {
    group(
        "FlowLayoutPanel — wrap",
        300.0,
        vec![kid(flow_panel()).size(260.0, 90.0).decor(|c, r| {
            let mut f = flow_panel();
            f.control_mut().set_bounds(r);
            let buttons: Vec<Button> =
                (1..=6).map(|i| button_at(c, &format!("b{i}"), 0.0, 0.0, 70.0)).collect();
            let cells: Vec<FlowChild> =
                buttons.iter().map(|b| FlowChild::new(b.size())).collect();
            // `arrange` answers in the panel's local space, so the stand-in
            // buttons are translated once, here.
            let o = f.base.content_origin(r);
            for (mut b, cell) in buttons.into_iter().zip(f.arrange(&cells)) {
                b.set_bounds(at(cell, o));
                paint_at(c, &b);
            }
        })],
    )
}

// ── TableLayoutPanel ─────────────────────────────────────────────────────────

fn table_panel() -> TableLayoutPanel {
    let mut t = TableLayoutPanel::new();
    t.column_count = 3;
    t.row_count = 2;
    t.cell_border_style = TableLayoutPanelCellBorderStyle::Single;
    t
}

/// The panel resolves the tracks (`column_widths`/`row_heights`) and paints the
/// outer border; the per-cell lines it leaves to the host, so each cell's label
/// carries its own `FixedSingle` border — the closest the library offers.
fn table() -> Group {
    group(
        "TableLayoutPanel — cell borders",
        300.0,
        vec![kid(table_panel()).size(260.0, 90.0).decor(|c, r| {
            let mut t = table_panel();
            t.control_mut().set_bounds(r);
            let cells: Vec<CellSpec> = (0..6).map(|_| CellSpec::default()).collect();
            let placed = place_cells(t.column_count, t.row_count, t.grow_style, &cells);

            // Every track is `AutoSize` here, and an auto track is exactly as
            // big as the content it is told about — so the cells have to be
            // measured before the panel can resolve its columns and rows.
            let mut labels: Vec<Label> = (0..6)
                .map(|i| {
                    let mut l = label_at(c, &format!("cell {i}"), 0.0, 0.0);
                    l.border_style = BorderStyle::FixedSingle;
                    l.padding = Padding::new(4.0, 1.0, 4.0, 1.0);
                    l
                })
                .collect();
            let mut columns = vec![0.0f32; t.column_count as usize];
            let mut rows = vec![0.0f32; t.row_count as usize];
            for (l, (col, row)) in labels.iter().zip(&placed) {
                let s = l.preferred_size(c);
                if let Some(w) = columns.get_mut(*col as usize) {
                    *w = w.max(s.width);
                }
                if let Some(h) = rows.get_mut(*row as usize) {
                    *h = h.max(s.height);
                }
            }
            let widths = t.column_widths(&columns);
            let heights = t.row_heights(&rows);
            let border = t.cell_border_style.thickness();
            // The tracks are resolved in the panel's local space (`display`), and
            // crossed into canvas space once (`o`). `ControlBase::display_rect`
            // would be wrong here: it is the control's box in its PARENT's space.
            let display = t.base.local_display_rect();
            let o = t.base.content_origin(r);

            for (l, (col, row)) in labels.iter_mut().zip(&placed) {
                let (col, row) = (*col as usize, *row as usize);
                let x = display.left
                    + border
                    + widths.iter().take(col).map(|w| w + border).sum::<f32>();
                let y = display.top
                    + border
                    + heights.iter().take(row).map(|h| h + border).sum::<f32>();
                let w = widths.get(col).copied().unwrap_or(0.0);
                let h = heights.get(row).copied().unwrap_or(0.0);
                l.set_bounds(at(Rect::new(x, y, x + w, y + h), o));
                paint_at(c, l);
            }
        })],
    )
}

// ── TabControl / TabPage ─────────────────────────────────────────────────────

fn tab_control() -> TabControl {
    let mut t = TabControl::new();
    for name in ["First", "Second", "Third"] {
        t.add_page(TabPage::new(name));
    }
    t
}

/// `TabControl` paints the strip and the page frame but owns no children, so
/// the page's content is placed against the rectangle it publishes.
fn tabs() -> Group {
    group(
        "TabControl / TabPage",
        300.0,
        vec![kid(tab_control()).size(260.0, 100.0).decor(|c, r| {
            let mut t = tab_control();
            t.control_mut().set_bounds(r);
            // `display_rect` is in the tab control's own local space, whose
            // origin is the control's top-left — so the crossing is `r`'s origin.
            let page = at(t.display_rect(c), Point::new(r.left, r.top));
            paint_at(c, &label_at(c, "page content", page.left + 10.0, page.top + 10.0));
        })],
    )
}

// ── SplitContainer ───────────────────────────────────────────────────────────

fn split_container() -> SplitContainer {
    let mut s = SplitContainer::new();
    s.splitter_distance = 100.0;
    s.border_style = BorderStyle::FixedSingle;
    s
}

fn split() -> Group {
    group(
        "SplitContainer",
        300.0,
        vec![kid(split_container()).size(260.0, 100.0).decor(|c, r| {
            let mut s = split_container();
            s.control_mut().set_bounds(r);
            // `arrange` answers in client coordinates, which start just inside the
            // container's border — so crossing to canvas re-applies that border.
            let o = content_origin(r, border_thickness(s.border_style), s.auto_scroll_position);
            let rects = s.arrange();
            let p1 = at(rects.panel1, o);
            let p2 = at(rects.panel2, o);
            paint_at(c, &label_at(c, "Panel1", p1.left + 6.0, p1.top + 6.0));
            paint_at(c, &label_at(c, "Panel2", p2.left + 6.0, p2.top + 6.0));
        })],
    )
}
