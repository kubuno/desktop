//! `<DataTable>` column formats, sort and in-place editing, through a real view runtime painting
//! on a `RecordingCanvas`, with the frame input set on the host's thread-local queue (what the
//! widgets and the focus ring read).

use super::*;
use crate::binding::Row;
use crate::events::{CellCancelEventArgs, CellEventArgs, CellValidatingEventArgs};
use crate::ast::AstNode;
use crate::runtime::Runtime;
use kubuno_desktop_controls::host::{self, vk, InputEvent, Modifiers};
use kubuno_desktop_ui::graphics::testing::RecordingCanvas;

#[derive(Default)]
struct GridVm {
    rows: Vec<Row>,
    log: Vec<String>,
    refuse: bool,
    cancel_begin: bool,
}

impl GridVm {
    fn new() -> Self {
        let row = |id: &str, item: &str, amount: &str, ordered: &str| {
            Row::new()
                .with("id", Value::Str(id.into()))
                .with("item", Value::Str(item.into()))
                .with("amount", Value::Str(amount.into()))
                .with("ordered", Value::Str(ordered.into()))
        };
        Self { rows: vec![row("1", "Tea", "3.5", "2026-09-29"), row("2", "Engine", "1234.5", "1843-09-01"), row("3", "CLU", "", "")], ..Default::default() }
    }
}

impl ViewModel for GridVm {
    fn get(&self, path: &str) -> Option<Value> {
        (path == "Orders").then(|| Value::from(self.rows.clone()))
    }

    /// `Orders[<row>].<field>`: the documented write path of a view-model list.
    fn set(&mut self, path: &str, value: Value) {
        self.log.push(format!("set {path}={value:?}"));
        let Some(rest) = path.strip_prefix("Orders[") else { return };
        let Some((index, field)) = rest.split_once("].") else { return };
        let Ok(i) = index.parse::<usize>() else { return };
        if let Some(row) = self.rows.get_mut(i) {
            let text = match value {
                Value::F32(f) => f.to_string(),
                Value::Str(s) => s,
                other => format!("{other:?}"),
            };
            let mut fields: Vec<(String, Value)> = Vec::new();
            for name in ["id", "item", "amount", "ordered"] {
                let v = if name == field { Value::Str(text.clone()) } else { row.get(name).cloned().unwrap_or(Value::Str(String::new())) };
                fields.push((name.to_string(), v));
            }
            *row = fields.into_iter().fold(Row::new(), |r, (n, v)| r.with(n, v));
        }
    }
}

#[crate::event_handlers]
impl GridVm {
    fn begin(&mut self, e: &mut CellCancelEventArgs) {
        self.log.push(format!("begin {} {}", e.row_index, e.column));
        e.cancel = self.cancel_begin;
    }
    fn validating(&mut self, e: &mut CellValidatingEventArgs) {
        self.log.push(format!("validating {} {} {:?}", e.row_index, e.column, e.formatted_value));
        e.cancel = self.refuse;
    }
    fn changed(&mut self, e: &CellEventArgs) {
        self.log.push(format!("changed {} {}", e.row_index, e.column));
    }
    fn ended(&mut self, e: &CellEventArgs) {
        self.log.push(format!("ended {} {}", e.row_index, e.column));
    }
}

const VIEW: &str = r#"<Panel DesignWidth="600" DesignHeight="400">
  <DataTable x:Name="grid" ItemsSource="{Binding Orders}" Culture="fr-FR" X="0" Y="0" Width="600" Height="400"
             OnCellBeginEdit="begin" OnCellValidating="validating" OnCellValueChanged="changed" OnCellEndEdit="ended">
    <Column Header="Item" Binding="{Binding item}" Width="200"/>
    <Column Header="Montant" Binding="{Binding amount}" FormatString="N2" NullValue="-" Alignment="Right" Width="150"/>
    <Column Header="Date" Binding="{Binding ordered, FormatString=d}" Width="150"/>
    <Column Header="Id" Binding="{Binding id}" ReadOnly="true" Width="60"/>
  </DataTable>
</Panel>"#;

/// Row `i`'s vertical centre (a 40 DIP header, 40 DIP rows) and the columns' centres.
fn row_y(i: usize) -> f32 {
    40.0 + 40.0 * i as f32 + 20.0
}
const COL_X: [f32; 4] = [100.0, 275.0, 425.0, 530.0];

struct Bench {
    rt: Runtime,
    vm: GridVm,
    now: u64,
}

impl Bench {
    fn new() -> Self {
        let mut rt = Runtime::new();
        assert!(rt.reload_from_text(VIEW), "{:?}", rt.diagnostics());
        let mut b = Self { rt, vm: GridVm::new(), now: 0 };
        b.frame(vec![], None, false, 0);
        b
    }

    /// One painted frame with `events` as its input and the pointer at `mouse` (away when `None`).
    fn frame(&mut self, events: Vec<InputEvent>, mouse: Option<(f32, f32)>, down: bool, clicks: u8) -> RecordingCanvas {
        self.now += 16;
        host::input::set_frame_events(events);
        let (x, y) = mouse.unwrap_or((host::POINTER_AWAY, host::POINTER_AWAY));
        let frame = Frame {
            size: (600.0, 400.0),
            mouse: (x, y),
            mouse_down: down,
            right_down: false,
            middle_down: false,
            dismiss: false,
            scale: 1.0,
            client_origin: (0.0, 0.0),
            work_area: (0.0, 0.0, 600.0, 400.0),
            chrome_top: 0.0,
            mods: Modifiers::NONE,
            wheel: (0.0, 0.0),
            click_count: clicks,
            window_focused: true,
        };
        let canvas = RecordingCanvas::new();
        self.rt.frame_typed(&canvas, &frame, &mut self.vm, Rect::new(0.0, 0.0, 600.0, 400.0));
        host::input::set_frame_events(Vec::new());
        canvas
    }

    /// A click (press, then release) at `(x, y)`.
    fn click(&mut self, x: f32, y: f32, clicks: u8) -> RecordingCanvas {
        self.frame(vec![], Some((x, y)), true, clicks);
        self.frame(vec![], Some((x, y)), false, 0)
    }

    fn key(&mut self, k: u16, mods: Modifiers) -> RecordingCanvas {
        self.frame(vec![InputEvent::Key { vk: k, down: true, repeat: false, mods }], None, false, 0)
    }

    fn type_text(&mut self, s: &str) -> RecordingCanvas {
        self.frame(vec![InputEvent::Text(s.into())], None, false, 0)
    }

    fn take_log(&mut self) -> Vec<String> {
        std::mem::take(&mut self.vm.log)
    }
}

fn texts(c: &RecordingCanvas) -> Vec<String> {
    c.calls().into_iter().filter(|c| c.starts_with("text(")).collect()
}

fn shows(c: &RecordingCanvas, s: &str) -> bool {
    texts(c).iter().any(|t| t.starts_with(&format!("text({s:?}")))
}

#[test]
fn columns_are_formatted_per_their_format_string_culture_and_null_value() {
    let mut b = Bench::new();
    let c = b.frame(vec![], None, false, 0);
    assert!(shows(&c, "3,50") && shows(&c, "1\u{202F}234,50"), "N2 in French: {:?}", texts(&c));
    assert!(shows(&c, "-"), "NullValue for an empty amount");
    assert!(shows(&c, "29/09/2026") && shows(&c, "01/09/1843"), "d from the Binding's own FormatString, the table's culture");
    assert!(shows(&c, "Tea") && shows(&c, "2"), "unformatted columns as held");
    assert!(!shows(&c, "1234.5"), "never the raw number");
}

#[test]
fn a_header_click_sorts_by_the_underlying_values() {
    let mut b = Bench::new();
    // The Montant header: ascending by the numbers, the empty (NULL) amount first.
    let c = b.click(COL_X[1] - 50.0, 20.0, 1);
    let order: Vec<usize> = ["\"CLU\"", "\"Tea\"", "\"Engine\""].iter().map(|n| texts(&c).iter().position(|t| t.contains(n)).unwrap_or(usize::MAX)).collect();
    assert!(order[0] < order[1] && order[1] < order[2], "CLU (-), Tea (3,50), Engine (1 234,50): {:?}", texts(&c));
    // Descending: 1 234,50 before 3,50 (a text sort of the formatted cells would not do that).
    let c = b.click(COL_X[1] - 50.0, 20.0, 1);
    let e = texts(&c).iter().position(|t| t.contains("\"Engine\"")).unwrap_or(usize::MAX);
    let t = texts(&c).iter().position(|t| t.contains("\"Tea\"")).unwrap_or(0);
    assert!(e < t);
    // The write path still names the row of the bound list, whatever the grid's order.
    b.click(COL_X[1], row_y(0), 1); // Engine, first on screen now
    b.type_text("7");
    b.key(vk::ENTER, Modifiers::NONE);
    assert!(b.take_log().iter().any(|l| l == "set Orders[1].amount=F32(7.0)"));
}

#[test]
fn typing_begins_an_edit_and_enter_writes_the_parsed_value_back() {
    let mut b = Bench::new();
    b.click(COL_X[1], row_y(1), 1); // Engine / Montant: focus and current cell
    b.take_log();
    b.type_text("12,5");
    assert_eq!(b.take_log(), ["begin 1 amount"]);
    let c = b.frame(vec![], None, false, 0);
    assert!(shows(&c, "12,5"), "the editor shows the typed text: {:?}", texts(&c));
    b.key(vk::ENTER, Modifiers::NONE);
    assert_eq!(
        b.take_log(),
        ["validating 1 amount \"12,5\"", "set Orders[1].amount=F32(12.5)", "changed 1 amount", "ended 1 amount"],
        "WinForms order: CellValidating, the value, CellValueChanged, CellEndEdit"
    );
    let c = b.frame(vec![], None, false, 0);
    assert!(shows(&c, "12,50"), "the new value, formatted: {:?}", texts(&c));
    // Enter moved down: typing edits row 2.
    b.type_text("1");
    assert_eq!(b.take_log(), ["begin 2 amount"]);
}

#[test]
fn f2_edits_the_formatted_text_and_escape_cancels() {
    let mut b = Bench::new();
    b.click(COL_X[1], row_y(2), 1);
    b.take_log();
    b.key(vk::F2, Modifiers::NONE);
    assert_eq!(b.take_log(), ["begin 2 amount"]);
    b.type_text("9");
    let c = b.frame(vec![], None, false, 0);
    assert!(shows(&c, "-9"), "F2 keeps the cell's text, the caret at its end: {:?}", texts(&c));
    b.key(vk::ESCAPE, Modifiers::NONE);
    assert_eq!(b.take_log(), ["ended 2 amount"], "nothing written");
    let c = b.frame(vec![], None, false, 0);
    assert!(shows(&c, "-") && !shows(&c, "-9"));
}

#[test]
fn a_refused_value_keeps_the_editor_and_a_read_only_column_is_not_edited() {
    let mut b = Bench::new();
    b.click(COL_X[3], row_y(0), 1);
    b.take_log();
    b.key(vk::F2, Modifiers::NONE);
    b.type_text("x");
    assert!(b.take_log().is_empty(), "the Id column is read-only");
    b.key(vk::LEFT, Modifiers::NONE);
    b.type_text("abc");
    assert_eq!(b.take_log(), ["begin 0 ordered"]);
    b.vm.refuse = true;
    b.key(vk::ENTER, Modifiers::NONE);
    assert_eq!(b.take_log(), ["validating 0 ordered \"abc\""]);
    let c = b.frame(vec![], None, false, 0);
    assert!(shows(&c, "abc"), "still editing");
    b.vm.refuse = false;
    b.key(vk::TAB, Modifiers::NONE);
    let log = b.take_log();
    assert_eq!(log[0], "validating 0 ordered \"abc\"");
    assert_eq!(log[1], "set Orders[0].ordered=Str(\"abc\")", "a text that is not a date is written as typed");
    // Tab went to the next cell (the read-only Id): typing edits nothing there.
    b.type_text("5");
    assert!(b.take_log().is_empty());
    // A cancelled CellBeginEdit keeps the cell out of edit mode.
    b.vm.cancel_begin = true;
    b.click(COL_X[0], row_y(1), 1);
    b.take_log();
    b.type_text("Z");
    assert_eq!(b.take_log(), ["begin 1 item"]);
    b.key(vk::ENTER, Modifiers::NONE);
    assert!(b.take_log().is_empty(), "no editor to commit");
}

#[test]
fn a_double_click_edits_and_a_click_elsewhere_commits() {
    let mut b = Bench::new();
    b.click(COL_X[0], row_y(0), 1);
    b.frame(vec![], Some((COL_X[0], row_y(0))), true, 2);
    b.frame(vec![], Some((COL_X[0], row_y(0))), false, 0);
    let log = b.take_log();
    assert!(log.contains(&"begin 0 item".to_string()), "{log:?}");
    // The whole text is selected: typing replaces it.
    b.type_text("Coffee");
    b.click(COL_X[0], row_y(2), 1);
    let log = b.take_log();
    assert!(log.contains(&"set Orders[0].item=Str(\"Coffee\")".to_string()), "{log:?}");
    let c = b.frame(vec![], None, false, 0);
    assert!(shows(&c, "Coffee"));
}

#[test]
fn an_editor_rect_is_painted_over_the_cell_being_edited() {
    let mut b = Bench::new();
    b.click(COL_X[0], row_y(0), 1);
    b.key(vk::F2, Modifiers::NONE);
    let c = b.frame(vec![], None, false, 0);
    // The text field's ground (radius SM) fills the cell inset by 2 DIP: x 2..198, row 0 (40..80).
    assert!(c.calls().iter().any(|l| l.starts_with("fill_rounded(2,42,198,78")), "{:?}", c.calls());
}

#[test]
fn a_read_only_or_unnamed_table_never_edits() {
    let view = VIEW.replace("x:Name=\"grid\" ", "x:Name=\"grid\" ReadOnly=\"true\" ");
    let mut b = Bench::new();
    assert!(b.rt.reload_from_text(&view));
    b.frame(vec![], None, false, 0);
    b.click(COL_X[0], row_y(0), 1);
    b.key(vk::F2, Modifiers::NONE);
    b.type_text("x");
    b.frame(vec![], Some((COL_X[0], row_y(0))), true, 2);
    b.frame(vec![], None, false, 0);
    assert!(!b.take_log().iter().any(|l| l.starts_with("begin")));
    let unnamed = VIEW.replace("x:Name=\"grid\" ", "");
    assert!(b.rt.reload_from_text(&unnamed));
    b.frame(vec![], None, false, 0);
    b.frame(vec![], Some((COL_X[0], row_y(0))), true, 2);
    b.frame(vec![], None, false, 0);
    assert!(!b.take_log().iter().any(|l| l.starts_with("begin")), "no focus, no editing");
}

#[test]
fn compare_cells_orders_numbers_numerically_and_blanks_first() {
    use std::cmp::Ordering;
    let s = |t: &str| Value::Str(t.into());
    assert_eq!(compare_cells(Some(&s("999")), Some(&s("1234.5"))), Ordering::Less);
    assert_eq!(compare_cells(Some(&Value::F32(2.0)), Some(&s("10"))), Ordering::Less);
    assert_eq!(compare_cells(Some(&s("")), Some(&s("0"))), Ordering::Less);
    assert_eq!(compare_cells(None, Some(&s("a"))), Ordering::Less);
    assert_eq!(compare_cells(Some(&s("b")), Some(&s("A"))), Ordering::Greater);
    assert_eq!(compare_cells(Some(&s("1843-09-01")), Some(&s("2026-09-29"))), Ordering::Less, "ISO dates sort as text");
    let rows = vec![Row::new().with("n", s("10")), Row::new().with("n", s("9")), Row::new().with("n", s(""))];
    assert_eq!(sort_order(&rows, Some("n"), false), [2, 1, 0]);
    assert_eq!(sort_order(&rows, Some("n"), true), [0, 1, 2]);
    assert_eq!(sort_order(&rows, None, false), [0, 1, 2]);
}

#[test]
fn a_column_reads_its_format_from_its_attributes_its_binding_and_the_table() {
    let parsed = crate::syntax::parse(
        r#"<DataTable Culture="de-DE"><Column Binding="{Binding a, FormatString=N0, NullValue='?'}"/><Column Header="B" FormatString="C" Culture="en-US" ReadOnly="true" Alignment="Center"/></DataTable>"#,
    );
    let root = crate::ast::Document::cast(parsed.syntax()).and_then(|d| d.root_element()).expect("a root element");
    let (headers, cols) = data_table_columns(&root, Some("de-DE"));
    assert_eq!(cols[0].field, "a");
    assert!(cols[0].bound);
    assert_eq!(cols[0].format.format_string.as_deref(), Some("N0"));
    assert_eq!(cols[0].format.null_value.as_deref(), Some("?"));
    assert_eq!(cols[0].format.culture.as_deref(), Some("de-DE"), "the table's culture by default");
    assert_eq!((cols[1].field.as_str(), cols[1].bound, cols[1].read_only), ("B", false, true));
    assert_eq!(cols[1].format.culture.as_deref(), Some("en-US"));
    assert!(kubuno_desktop_ui::tables::has_flag(&headers[1], kubuno_desktop_ui::tables::flags::READ_ONLY));
    assert_eq!(headers[1].text_align, kubuno_desktop_controls::enums::HorizontalAlignment::Center);
    assert_eq!(headers[0].text_align, kubuno_desktop_controls::enums::HorizontalAlignment::Left, "WinForms' default: left");
    let row = Row::new().with("a", Value::Str("1234.4".into()));
    assert_eq!(cell_display(&row, &cols[0]), "1.234");
    assert_eq!(cell_display(&Row::new(), &cols[0]), "?");
}

/// In the designer a bound grid has no rows (no data at design time): like the Windows Forms
/// designer's DataGridView it shows its column headers over blank rows, never the empty state.
#[test]
fn the_designer_shows_the_headers_of_a_bound_grid_without_rows() {
    let mut rt = Runtime::new();
    assert!(rt.reload_from_text(VIEW), "{:?}", rt.diagnostics());
    let frame = Frame {
        size: (600.0, 400.0),
        mouse: (host::POINTER_AWAY, host::POINTER_AWAY),
        mouse_down: false,
        right_down: false,
        middle_down: false,
        dismiss: false,
        scale: 1.0,
        client_origin: (0.0, 0.0),
        work_area: (0.0, 0.0, 600.0, 400.0),
        chrome_top: 0.0,
        mods: Modifiers::NONE,
        wheel: (0.0, 0.0),
        click_count: 0,
        window_focused: true,
    };
    let texts = |c: &RecordingCanvas| c.calls().iter().filter(|l| l.starts_with("text(")).cloned().collect::<Vec<_>>();
    let mut empty = crate::binding::MapViewModel::new();
    let mut handlers = crate::binding::HandlerTable::new();
    let mut map = crate::design::LayoutMap::new();
    let design = RecordingCanvas::new();
    rt.frame_with_design(&design, &frame, &mut empty, &mut handlers, Rect::new(0.0, 0.0, 600.0, 400.0), Some(&mut map));
    let shown = texts(&design);
    for header in ["Item", "Montant", "Date", "Id"] {
        assert!(shown.iter().any(|t| t.starts_with(&format!("text({header:?}"))), "header {header} in the designer: {shown:?}");
    }
    // At run time the same unbound grid shows its empty state and no header.
    let mut rt = Runtime::new();
    assert!(rt.reload_from_text(VIEW), "{:?}", rt.diagnostics());
    let live = RecordingCanvas::new();
    rt.frame_model(&live, &frame, &mut empty, Rect::new(0.0, 0.0, 600.0, 400.0));
    assert!(!texts(&live).iter().any(|t| t.starts_with("text(\"Montant\"")), "no header over an empty body at run time");
}

#[test]
fn the_view_pages_sorts_and_states_the_table() {
    const PAGED: &str = r#"<Panel DesignWidth="600" DesignHeight="400">
  <DataTable ItemsSource="{Binding Orders}" X="0" Y="0" Width="600" Height="400" PageSize="2"
             PageIndex="{Binding Page, Mode=TwoWay}" SortColumn="item" SortOrder="Descending"
             ErrorText="{Binding Error}" EmptyText="Pas encore de commande" TotalRows="{Binding Total}">
    <Column Header="Item" Binding="{Binding item}" Width="200"/>
  </DataTable>
</Panel>"#;
    let rows = |names: &[&str]| Value::from(names.iter().map(|n| Row::new().with("item", Value::Str((*n).into()))).collect::<Vec<_>>());
    let frame = Frame {
        size: (600.0, 400.0),
        mouse: (host::POINTER_AWAY, host::POINTER_AWAY),
        mouse_down: false,
        right_down: false,
        middle_down: false,
        dismiss: false,
        scale: 1.0,
        client_origin: (0.0, 0.0),
        work_area: (0.0, 0.0, 600.0, 400.0),
        chrome_top: 0.0,
        mods: Modifiers::NONE,
        wheel: (0.0, 0.0),
        click_count: 0,
        window_focused: true,
    };
    let paint = |rt: &mut Runtime, vm: &mut crate::binding::MapViewModel| {
        let c = RecordingCanvas::new();
        rt.frame_model(&c, &frame, vm, Rect::new(0.0, 0.0, 600.0, 400.0));
        c
    };
    let mut rt = Runtime::new();
    assert!(rt.reload_from_text(PAGED), "{:?}", rt.diagnostics());
    let base = || {
        crate::binding::MapViewModel::new()
            .with("Orders", rows(&["a", "b", "c", "d", "e"]))
            .with("Error", Value::Str(String::new()))
            .with("Total", Value::F32(-1.0))
    };

    // Sorted descending by the declared column, two rows a page.
    let mut vm = base().with("Page", Value::F32(0.0));
    let c = paint(&mut rt, &mut vm);
    assert!(shows(&c, "e") && shows(&c, "d") && !shows(&c, "c"), "{:?}", texts(&c));
    // The view moves to page 1.
    let mut vm = base().with("Page", Value::F32(1.0));
    let c = paint(&mut rt, &mut vm);
    assert!(shows(&c, "c") && shows(&c, "b") && !shows(&c, "e"), "{:?}", texts(&c));

    // Manual paging: the rows handed ARE the page, whatever PageIndex says.
    let mut vm = base().with("Page", Value::F32(1.0)).with("Total", Value::F32(40.0)).with("Orders", rows(&["x", "y"]));
    let c = paint(&mut rt, &mut vm);
    assert!(shows(&c, "y") && shows(&c, "x"), "{:?}", texts(&c));

    // The error state replaces the rows; the empty state reads the view's text.
    let mut vm = base().with("Page", Value::F32(0.0)).with("Error", Value::Str("Serveur injoignable".into()));
    let c = paint(&mut rt, &mut vm);
    assert!(shows(&c, "Serveur injoignable") && !shows(&c, "e"), "{:?}", texts(&c));
    let mut vm = base().with("Page", Value::F32(0.0)).with("Orders", rows(&[]));
    let c = paint(&mut rt, &mut vm);
    assert!(shows(&c, "Pas encore de commande"), "{:?}", texts(&c));
}

#[test]
fn a_fill_column_keeps_its_width_as_a_minimum() {
    let parsed = crate::syntax::parse(r#"<DataTable><Column Header="User" Width="220" AutoSizeMode="Fill"/><Column Header="Role" Width="120"/></DataTable>"#);
    let root = crate::ast::Document::cast(parsed.syntax()).and_then(|d| d.root_element()).expect("a root element");
    let (_, cols) = data_table_columns(&root, None);
    assert!(cols[0].fill && !cols[1].fill);
    assert_eq!(cols[0].min_width, 220);
}
