//! Numeric parity probe — the PORT side.
//!
//! Reads the very same `tools/winforms-ref/parity/panels-cases.txt` the WinForms
//! probe reads, runs each case through `kubuno_controls::layout_panels`, and
//! writes `panels-port.json` in the identical shape. `compare-panels.ps1` then
//! diffs the two files.
//!
//! ```powershell
//! Set-Location Z:\projects\kubuno\desktop
//! $env:CARGO_TARGET_DIR = 'C:\kubuno-build\desktop-target'
//! cargo run -p kubuno-controls --example parity_panels -j 1
//! ```
//!
//! ## Why this probe is canvas-free
//!
//! `Canvas` is the paint surface: its `TextFormats` are DirectWrite objects a
//! `Renderer` owns, so there is no way to build one without a device. Everything
//! measured here is therefore taken from the family's **pure** surface —
//! `flow_layout`, `resolve_tracks` (through `column_widths`/`row_heights`),
//! `place_cells` (through `placements`), `cell_rect_in`, `clamp_distance`,
//! `split_layout`, `adjusted_distance`, `tab_strip`, `tab_rows`,
//! `tab_strip_thickness`, `tab_display_rect` — which is exactly the arithmetic
//! the parity run is about.
//!
//! Two places need a canvas in the library and are therefore **transcribed**
//! here, marked `TRANSCRIBED` at the call site, from the rule the library
//! documents: the auto-track measurement (`TableLayoutPanel::measure_auto_tracks`)
//! and the tab-strip thickness (`TabControl::strip_thickness`). The cases feed
//! them fixed-size children and, where the metric would come from the system
//! font, the difference is a reported finding rather than something to hide.
//!
//! ## The one rule of this file
//!
//! It measures. It never adjusts a number so the diff comes out clean — a
//! mismatch is the result, not a bug in the harness.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use kubuno_controls::containers::Panel;
use kubuno_controls::control::{Control, ControlBase};
use kubuno_controls::enums::{AnchorStyles, BorderStyle, DockStyle, Padding, Size};
use kubuno_controls::layout_panels::{
    adjusted_distance, cell_rect_in, clamp_distance, place_in_cell, tab_display_rect, tab_rects,
    tab_rows, tab_strip, tab_strip_thickness, CellSpec, FixedPanel, FlowChild, FlowDirection, FlowLayoutPanel,
    Orientation, SplitContainer, TabAlignment, TabControl, TabPage, TabSizeMode,
    TableLayoutPanel, TableLayoutPanelCellBorderStyle, TableLayoutPanelGrowStyle, TrackStyle,
};
use kubuno_controls::Rect;

// ═════════════════════════════════════════════════════════════════════════════
// The shared case model — one field per token of panels-cases.txt
// ═════════════════════════════════════════════════════════════════════════════

#[derive(Clone, Copy)]
struct ChildSpec {
    w: f32,
    h: f32,
    margin: Padding,
    flow_break: bool,
    col: i32,
    row: i32,
    col_span: u32,
    row_span: u32,
    /// The case file's last `cell` token (`fill` / `topleft`), as the child's
    /// own `Dock`/`Anchor`. A table cell is a miniature container: `fill`
    /// stretches the child to the cell, `topleft` leaves it at its natural size
    /// pinned to the cell's corner. The placement itself is not reproduced here
    /// — [`place_in_cell`] is called, so the harness cannot drift from the rule
    /// the library actually applies.
    dock: DockStyle,
    anchor: AnchorStyles,
}

impl Default for ChildSpec {
    fn default() -> Self {
        Self {
            w: 0.0,
            h: 0.0,
            margin: Padding::ZERO,
            flow_break: false,
            col: -1,
            row: -1,
            col_span: 1,
            row_span: 1,
            dock: DockStyle::None,
            anchor: AnchorStyles::default(),
        }
    }
}

#[derive(Default)]
struct Case {
    id: String,
    kind: String,
    box_w: f32,
    box_h: f32,
    padding: Padding,
    border: BorderStyle,

    // flow
    flow_dir: FlowDirection,
    wrap: bool,

    // table
    cols: u32,
    rows: u32,
    cell_border: TableLayoutPanelCellBorderStyle,
    grow: TableLayoutPanelGrowStyle,
    col_styles: Vec<TrackStyle>,
    row_styles: Vec<TrackStyle>,

    // split
    orientation: Orientation,
    fixed_panel: FixedPanel,
    distance: f32,
    splitter_width: f32,
    min1: f32,
    min2: f32,
    collapse1: bool,
    collapse2: bool,
    resize: Option<(f32, f32)>,

    // tab
    alignment: TabAlignment,
    multiline: bool,
    size_mode: TabSizeMode,
    item_size: Size,
    tab_padding: Size,
    pages: Vec<String>,

    children: Vec<ChildSpec>,
}

// ═════════════════════════════════════════════════════════════════════════════
// Parsing
// ═════════════════════════════════════════════════════════════════════════════

fn f(tok: &str) -> f32 {
    tok.parse::<f32>().unwrap_or_else(|_| panic!("not a number: {tok}"))
}

fn b(tok: &str) -> bool {
    tok.eq_ignore_ascii_case("true")
}

fn parse(text: &str) -> Vec<Case> {
    let mut cases: Vec<Case> = Vec::new();
    for raw in text.lines() {
        let line = raw.split('#').next().unwrap_or("");
        let t: Vec<&str> = line.split_whitespace().collect();
        if t.is_empty() {
            continue;
        }
        if t[0] == "case" {
            cases.push(Case {
                id: t[1].to_string(),
                kind: t[2].to_string(),
                box_w: 100.0,
                box_h: 100.0,
                wrap: true,
                distance: 50.0,
                splitter_width: 4.0,
                min1: 25.0,
                min2: 25.0,
                tab_padding: Size::new(6.0, 3.0),
                ..Case::default()
            });
            continue;
        }
        let c = cases.last_mut().expect("a record before any `case` line");
        match t[0] {
            "box" => {
                c.box_w = f(t[1]);
                c.box_h = f(t[2]);
                c.padding = Padding::new(f(t[3]), f(t[4]), f(t[5]), f(t[6]));
                c.border = match t[7] {
                    "single" => BorderStyle::FixedSingle,
                    "fixed3d" => BorderStyle::Fixed3D,
                    _ => BorderStyle::None,
                };
            }
            "flow" => {
                c.flow_dir = match t[1] {
                    "td" => FlowDirection::TopDown,
                    "rtl" => FlowDirection::RightToLeft,
                    "bu" => FlowDirection::BottomUp,
                    _ => FlowDirection::LeftToRight,
                };
                c.wrap = b(t[2]);
            }
            "child" => c.children.push(ChildSpec {
                w: f(t[1]),
                h: f(t[2]),
                margin: Padding::new(f(t[3]), f(t[4]), f(t[5]), f(t[6])),
                flow_break: b(t[7]),
                ..ChildSpec::default()
            }),
            "grid" => {
                c.cols = f(t[1]) as u32;
                c.rows = f(t[2]) as u32;
                c.cell_border = match t[3] {
                    "single" => TableLayoutPanelCellBorderStyle::Single,
                    "inset" => TableLayoutPanelCellBorderStyle::Inset,
                    "insetdouble" => TableLayoutPanelCellBorderStyle::InsetDouble,
                    "outset" => TableLayoutPanelCellBorderStyle::Outset,
                    "outsetdouble" => TableLayoutPanelCellBorderStyle::OutsetDouble,
                    "outsetpartial" => TableLayoutPanelCellBorderStyle::OutsetPartial,
                    _ => TableLayoutPanelCellBorderStyle::None,
                };
                c.grow = match t[4] {
                    "fixed" => TableLayoutPanelGrowStyle::FixedSize,
                    "addcolumns" => TableLayoutPanelGrowStyle::AddColumns,
                    _ => TableLayoutPanelGrowStyle::AddRows,
                };
            }
            "col" => c.col_styles.push(track(t[1], f(t[2]))),
            "row" => c.row_styles.push(track(t[1], f(t[2]))),
            "cell" => {
                // `fill` docks the child to the whole cell; `topleft` is the
                // WinForms default anchor, which keeps the child's own size.
                let (dock, anchor) = match t.get(11).copied().unwrap_or("fill") {
                    "fill" => (DockStyle::Fill, AnchorStyles::default()),
                    _ => (DockStyle::None, AnchorStyles::TOP.union(AnchorStyles::LEFT)),
                };
                c.children.push(ChildSpec {
                    w: f(t[1]),
                    h: f(t[2]),
                    margin: Padding::new(f(t[3]), f(t[4]), f(t[5]), f(t[6])),
                    col: f(t[7]) as i32,
                    row: f(t[8]) as i32,
                    col_span: f(t[9]) as u32,
                    row_span: f(t[10]) as u32,
                    flow_break: false,
                    dock,
                    anchor,
                })
            }
            "split" => {
                c.orientation = if t[1] == "horizontal" {
                    Orientation::Horizontal
                } else {
                    Orientation::Vertical
                };
                c.fixed_panel = match t[2] {
                    "panel1" => FixedPanel::Panel1,
                    "panel2" => FixedPanel::Panel2,
                    _ => FixedPanel::None,
                };
                c.distance = f(t[3]);
                c.splitter_width = f(t[4]);
                c.min1 = f(t[5]);
                c.min2 = f(t[6]);
                c.collapse1 = b(t[7]);
                c.collapse2 = b(t[8]);
            }
            "resize" => c.resize = Some((f(t[1]), f(t[2]))),
            "tabs" => {
                c.alignment = match t[1] {
                    "bottom" => TabAlignment::Bottom,
                    "left" => TabAlignment::Left,
                    "right" => TabAlignment::Right,
                    _ => TabAlignment::Top,
                };
                c.multiline = b(t[2]);
                c.size_mode = match t[3] {
                    "filltoright" => TabSizeMode::FillToRight,
                    "fixed" => TabSizeMode::Fixed,
                    _ => TabSizeMode::Normal,
                };
                c.item_size = Size::new(f(t[4]), f(t[5]));
                c.tab_padding = Size::new(f(t[6]), f(t[7]));
            }
            "page" => c.pages.push(t[1].to_string()),
            other => panic!("unknown record '{other}'"),
        }
    }
    cases
}

fn track(kind: &str, value: f32) -> TrackStyle {
    match kind {
        "abs" => TrackStyle::absolute(value),
        "pct" => TrackStyle::percent(value),
        _ => TrackStyle::auto(),
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// Measuring
// ═════════════════════════════════════════════════════════════════════════════

/// One emitted metric: a name and the numbers behind it. Rectangles are
/// `[x, y, width, height]` — WinForms' `Bounds`, so the two files line up
/// component by component.
type Metric = (String, Vec<f32>);

fn rect(r: Rect) -> Vec<f32> {
    vec![r.left, r.top, r.right - r.left, r.bottom - r.top]
}

fn leaf(spec: &ChildSpec) -> Panel {
    let mut p = Panel::new();
    p.control_mut().set_bounds(Rect::new(0.0, 0.0, spec.w, spec.h));
    p.control_mut().margin = spec.margin;
    p.control_mut().dock = spec.dock;
    p.control_mut().anchor = spec.anchor;
    p
}

fn measure(c: &Case) -> Vec<Metric> {
    match c.kind.as_str() {
        "flow" => measure_flow(c),
        "table" => measure_table(c),
        "split" => measure_split(c),
        "tab" => measure_tab(c),
        other => panic!("unknown kind '{other}'"),
    }
}

fn measure_flow(c: &Case) -> Vec<Metric> {
    let mut panel = FlowLayoutPanel::new();
    panel.control_mut().set_bounds(Rect::new(0.0, 0.0, c.box_w, c.box_h));
    panel.control_mut().padding = c.padding;
    panel.base.border_style = c.border;
    panel.flow_direction = c.flow_dir;
    panel.wrap_contents = c.wrap;

    let cells: Vec<FlowChild> = c
        .children
        .iter()
        .map(|s| FlowChild { size: Size::new(s.w, s.h), margin: s.margin, flow_break: s.flow_break })
        .collect();

    let display = panel.base.local_display_rect();
    let rects = panel.arrange(&cells);

    let mut out: Vec<Metric> = vec![("display".into(), rect(display))];
    // The port has no `ClientSize`; it is recovered from the client box the
    // panel actually lays out in, plus the padding it already took off.
    out.push((
        "clientSize".into(),
        vec![
            (display.right - display.left) + c.padding.horizontal(),
            (display.bottom - display.top) + c.padding.vertical(),
        ],
    ));
    for (i, r) in rects.iter().enumerate() {
        out.push((format!("child{i}"), rect(*r)));
    }
    out
}

fn measure_table(c: &Case) -> Vec<Metric> {
    let mut t = TableLayoutPanel::new();
    t.control_mut().set_bounds(Rect::new(0.0, 0.0, c.box_w, c.box_h));
    t.control_mut().padding = c.padding;
    t.base.border_style = c.border;
    t.column_count = c.cols;
    t.row_count = c.rows;
    t.column_styles = c.col_styles.clone();
    t.row_styles = c.row_styles.clone();
    t.cell_border_style = c.cell_border;
    t.grow_style = c.grow;
    t.cells = c
        .children
        .iter()
        .map(|s| CellSpec { col: s.col, row: s.row, col_span: s.col_span, row_span: s.row_span })
        .collect();
    for s in &c.children {
        t.base.children.push(Box::new(leaf(s)));
    }

    let placements = t.placements();

    // TRANSCRIBED from `TableLayoutPanel::measure_auto_tracks`, which needs a
    // canvas only to call `preferred_size` on auto-sizing children — and every
    // child in these cases carries an explicit size. The rule the library
    // documents: a child spanning several tracks contributes to none of them.
    let mut n_cols = c.cols as usize;
    let mut n_rows = c.rows as usize;
    for &(col, row) in &placements {
        n_cols = n_cols.max(col as usize + 1);
        n_rows = n_rows.max(row as usize + 1);
    }
    let mut auto_cols = vec![0.0f32; n_cols];
    let mut auto_rows = vec![0.0f32; n_rows];
    for (i, s) in c.children.iter().enumerate() {
        if s.col_span.max(1) != 1 || s.row_span.max(1) != 1 {
            continue;
        }
        let (col, row) = placements[i];
        if let Some(w) = auto_cols.get_mut(col as usize) {
            *w = w.max(s.w + s.margin.horizontal());
        }
        if let Some(h) = auto_rows.get_mut(row as usize) {
            *h = h.max(s.h + s.margin.vertical());
        }
    }

    let widths = t.column_widths(&auto_cols);
    let heights = t.row_heights(&auto_rows);
    let origin = t.base.local_display_rect();
    let border = t.cell_border_style.thickness();

    let mut out: Vec<Metric> = vec![("display".into(), rect(origin))];
    // The port never refuses a child, whatever the GrowStyle.
    out.push(("overflowThrew".into(), vec![0.0]));
    out.push(("columnWidths".into(), widths.clone()));
    out.push(("rowHeights".into(), heights.clone()));
    for (i, s) in c.children.iter().enumerate() {
        let (col, row) = placements[i];
        let placed =
            CellSpec { col: col as i32, row: row as i32, col_span: s.col_span, row_span: s.row_span };
        let cell = cell_rect_in(origin, &widths, &heights, border, placed);
        let m = s.margin;
        let inner = Rect::new(
            cell.left + m.left,
            cell.top + m.top,
            (cell.right - m.right).max(cell.left + m.left),
            (cell.bottom - m.bottom).max(cell.top + m.top),
        );
        // The per-cell placement comes from the library itself, so the anchoring
        // rule is exercised rather than assumed. Every child here carries an
        // explicit size, which is why `child_rects` (whose only use for a canvas
        // is measuring auto-sized children) can be reduced to this call.
        let mut cb = ControlBase::new();
        cb.dock = s.dock;
        cb.anchor = s.anchor;
        let r = place_in_cell(inner, &cb, Size::new(s.w, s.h));
        out.push((format!("child{i}"), rect(r)));
        out.push((format!("cell{i}"), vec![col as f32, row as f32]));
    }
    out
}

fn measure_split(c: &Case) -> Vec<Metric> {
    let mut s = SplitContainer::new();
    s.control_mut().set_bounds(Rect::new(0.0, 0.0, c.box_w, c.box_h));
    s.border_style = c.border;
    s.orientation = c.orientation;
    s.fixed_panel = c.fixed_panel;
    s.splitter_width = c.splitter_width;
    s.panel1_min_size = c.min1;
    s.panel2_min_size = c.min2;
    s.splitter_distance = c.distance;
    s.panel1_collapsed = c.collapse1;
    s.panel2_collapsed = c.collapse2;

    let along = |sc: &SplitContainer| {
        let d = sc.local_display_rect();
        if sc.orientation == Orientation::Vertical {
            d.right - d.left
        } else {
            d.bottom - d.top
        }
    };

    let total = along(&s);
    let effective = clamp_distance(total, c.distance, c.splitter_width, c.min1, c.min2);
    let r = s.arrange();

    let mut out: Vec<Metric> = vec![
        ("panel1".into(), rect(r.panel1)),
        ("splitter".into(), rect(r.splitter)),
        ("panel2".into(), rect(r.panel2)),
        ("distance".into(), vec![effective]),
        ("notHonoured".into(), vec![if effective != c.distance { 1.0 } else { 0.0 }]),
        // The port clamps silently; it has no way to refuse a distance.
        ("threw".into(), vec![0.0]),
    ];

    if let Some((w, h)) = c.resize {
        let old_total = total;
        s.control_mut().set_bounds(Rect::new(0.0, 0.0, w, h));
        let new_total = along(&s);
        let moved =
            adjusted_distance(old_total, new_total, effective, c.fixed_panel, c.splitter_width);
        s.splitter_distance = moved;
        let after = s.arrange();
        let settled = clamp_distance(new_total, moved, c.splitter_width, c.min1, c.min2);
        out.push(("panel1After".into(), rect(after.panel1)));
        out.push(("splitterAfter".into(), rect(after.splitter)));
        out.push(("panel2After".into(), rect(after.panel2)));
        out.push(("distanceAfter".into(), vec![settled]));
    }
    out
}

fn measure_tab(c: &Case) -> Vec<Metric> {
    let mut tc = TabControl::new();
    tc.control_mut().set_bounds(Rect::new(0.0, 0.0, c.box_w, c.box_h));
    tc.alignment = c.alignment;
    tc.multiline = c.multiline;
    tc.size_mode = c.size_mode;
    tc.item_size = c.item_size;
    tc.padding = c.tab_padding;
    for caption in &c.pages {
        tc.add_page(TabPage::new(caption.clone()));
    }

    let vertical = matches!(c.alignment, TabAlignment::Left | TabAlignment::Right);
    let along = if vertical { c.box_h } else { c.box_w };

    // The captions are measured as ZERO: there is no text metric without a
    // device. The `fixed` cases do not depend on them (every tab takes
    // `ItemSize`), and the `normal`/`filltoright` cases are kept precisely so
    // the font-derived gap is REPORTED rather than assumed.
    //
    // `ItemSize`'s axes are NOT swapped for a side strip: `Width` is the
    // along-strip extent and `Height` the row thickness under every alignment,
    // so an 80 × 20 item makes a left-aligned tab 20 wide and 80 tall.
    let labels = vec![0.0f32; c.pages.len()];
    let strips = tab_strip(&labels, c.tab_padding.width, c.item_size.width, c.size_mode, along);
    let extents: Vec<f32> = strips.iter().map(|t| t.1).collect();
    let row_map = if c.multiline { tab_rows(&extents, along) } else { vec![0; extents.len()] };
    let rows = row_map.iter().copied().max().map(|r| r + 1).unwrap_or(1);

    // TRANSCRIBED from `TabControl::row_thickness`: a caption line plus
    // `Padding.Y` above and below, when `ItemSize.Height` is left auto.
    let row_thickness =
        if c.item_size.height > 0.0 { c.item_size.height } else { 14.0 + 2.0 * c.tab_padding.height };
    let thickness = tab_strip_thickness(row_thickness, rows);
    let local = Rect::new(0.0, 0.0, c.box_w, c.box_h);
    let display = tab_display_rect(local, c.alignment, thickness);

    // `ItemSize` RESOLVES in `Normal` mode rather than staying at its unset
    // 0 × 0 — the width it reports is the first tab's.
    let item_size = if c.item_size.width > 0.0 && c.item_size.height > 0.0 {
        c.item_size
    } else {
        Size::new(extents.first().copied().unwrap_or(0.0), row_thickness)
    };

    let mut out: Vec<Metric> = vec![
        ("display".into(), rect(display)),
        ("itemSize".into(), vec![item_size.width, item_size.height]),
        ("rowCount".into(), vec![rows as f32]),
    ];
    if !c.pages.is_empty() {
        // `perform_layout` gives the selected page exactly the display rect.
        out.push(("page0".into(), rect(display)));
    }
    // All four alignments have tab geometry, from the library's own `tab_rects`:
    // it restarts the offset on each row and rotates the rows so the selected
    // tab's row sits against the page.
    let selected = if c.pages.is_empty() { -1 } else { 0 };
    for (i, r) in
        tab_rects(local, c.alignment, &extents, row_thickness, &row_map, c.size_mode, selected)
            .iter()
            .enumerate()
    {
        out.push((format!("tab{i}"), rect(*r)));
    }
    out
}

// ═════════════════════════════════════════════════════════════════════════════
// Entry point
// ═════════════════════════════════════════════════════════════════════════════

/// The parity directory, found from the crate rather than the working
/// directory, so `cargo run` works from anywhere in the workspace.
fn parity_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("tools")
        .join("winforms-ref")
        .join("parity")
}

fn main() {
    let dir = parity_dir();
    let cases_path = dir.join("panels-cases.txt");
    let text = std::fs::read_to_string(&cases_path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", cases_path.display()));
    let cases = parse(&text);

    let mut json = String::new();
    json.push_str("{\n  \"probe\": \"port\",\n  \"cases\": [\n");
    for (n, c) in cases.iter().enumerate() {
        let _ = write!(json, "    {{\"id\": \"{}\", \"kind\": \"{}\", \"values\": {{", c.id, c.kind);
        for (i, (key, values)) in measure(c).iter().enumerate() {
            if i > 0 {
                json.push_str(", ");
            }
            let _ = write!(json, "\"{key}\": [");
            for (k, v) in values.iter().enumerate() {
                if k > 0 {
                    json.push_str(", ");
                }
                // Four decimals, trailing zeros trimmed — the same shape the
                // WinForms probe writes, so the files diff cleanly by eye too.
                let s = format!("{v:.4}");
                let s = s.trim_end_matches('0').trim_end_matches('.');
                json.push_str(if s.is_empty() || s == "-" { "0" } else { s });
            }
            json.push(']');
        }
        json.push_str("}}");
        if n < cases.len() - 1 {
            json.push(',');
        }
        json.push('\n');
    }
    json.push_str("  ]\n}\n");

    let out = dir.join("panels-port.json");
    std::fs::write(&out, json).unwrap_or_else(|e| panic!("cannot write {}: {e}", out.display()));
    println!("{} cases -> {}", cases.len(), out.display());
}
