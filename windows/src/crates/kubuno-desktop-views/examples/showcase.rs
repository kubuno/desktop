//! Visual check for the five component families integrated in this phase
//! (`display`/`choice`/`text`/`containers`/`data`) — one tab per family, all
//! wired against a single [`ShowcaseViewModel`]. Not a generic file viewer
//! like `view_preview` (that one stays scoped to `settings.kbview`'s own
//! bindings): this example's view model exists only to give
//! `examples/views/showcase.kbview` every path it binds to.
//!
//! ```text
//! cargo run -p kubuno-desktop-views --example showcase
//! ```
//!
//! Hot reload still works (the same [`Runtime`]/[`FileWatcher`] split
//! `view_preview` uses): editing `showcase.kbview` and saving picks the
//! change up within ~250 ms, without losing whatever the view model already
//! holds (the checkbox/radio/slider/tab selection… state).

use std::process::ExitCode;

use kubuno_desktop_controls::host::{self, Chrome, Frame};
use kubuno_desktop_controls::ControlCanvas;
use kubuno_desktop_ui::{Rect, Theme};
use kubuno_desktop_views::binding::{HandlerTable, Row, Value, ViewModel};
use kubuno_desktop_views::runtime::{FileWatcher, Runtime};
use kubuno_desktop_views::{handlers, node::ViewEventKind};

const SHOWCASE_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/examples/views/showcase.kbview");

/// One row `DataTable`'s `ItemsSource="{Binding Users}"` reads — proof the
/// `Value::List`/`Row` binding (not just static `<Column>` markup) is wired
/// end to end, alongside the static-children `ListBox`/`ListView`/`TreeView`
/// on the same tab.
#[derive(Clone)]
struct UserRow {
    name: &'static str,
    role: &'static str,
}

struct ShowcaseViewModel {
    active_tab: f32,
    progress: f32,
    agree: bool,
    theme: String,
    slider_value: f32,
    number_value: f32,
    notes: String,
    query: String,
    date_text: String,
    lang: String,
    lang2: String,
    picked_date: String,
    picked_color: String,
    picked_gradient: String,
    pane_width: f32,
    current_step: f32,
    list_sel_index: f32,
    selected_date: String,
    users: Vec<UserRow>,
    status: String,
}

impl Default for ShowcaseViewModel {
    fn default() -> Self {
        Self {
            active_tab: 0.0,
            progress: 42.0,
            agree: true,
            theme: "Light".to_string(),
            slider_value: 30.0,
            number_value: 3.0,
            notes: String::new(),
            query: String::new(),
            date_text: String::new(),
            lang: "fr".to_string(),
            lang2: "a".to_string(),
            picked_date: "2026-09-25".to_string(),
            picked_color: "#3B82F6".to_string(),
            picked_gradient: String::new(),
            pane_width: 220.0,
            current_step: 0.0,
            list_sel_index: 0.0,
            selected_date: "2026-09-25".to_string(),
            users: vec![UserRow { name: "Alice", role: "Admin" }, UserRow { name: "Bob", role: "Editor" }],
            status: "Ready.".to_string(),
        }
    }
}

impl ShowcaseViewModel {
    fn users_as_rows(&self) -> Vec<Row> {
        self.users.iter().map(|u| Row::new().with("Name", Value::Str(u.name.to_string())).with("Role", Value::Str(u.role.to_string()))).collect()
    }
}

impl ViewModel for ShowcaseViewModel {
    fn get(&self, path: &str) -> Option<Value> {
        Some(match path {
            "ActiveTab" => Value::F32(self.active_tab),
            "Progress" => Value::F32(self.progress),
            "Agree" => Value::Bool(self.agree),
            "Theme" => Value::Str(self.theme.clone()),
            "SliderValue" => Value::F32(self.slider_value),
            "NumberValue" => Value::F32(self.number_value),
            "Notes" => Value::Str(self.notes.clone()),
            "Query" => Value::Str(self.query.clone()),
            "DateText" => Value::Str(self.date_text.clone()),
            "Lang" => Value::Str(self.lang.clone()),
            "Lang2" => Value::Str(self.lang2.clone()),
            "PickedDate" => Value::Str(self.picked_date.clone()),
            "PickedColor" => Value::Str(self.picked_color.clone()),
            "PickedGradient" => Value::Str(self.picked_gradient.clone()),
            "PaneWidth" => Value::F32(self.pane_width),
            "CurrentStep" => Value::F32(self.current_step),
            "ListSelIndex" => Value::F32(self.list_sel_index),
            "SelectedDate" => Value::Str(self.selected_date.clone()),
            "Users" => Value::from(self.users_as_rows()),
            "Status" => Value::Str(self.status.clone()),
            _ => return None,
        })
    }

    fn set(&mut self, path: &str, value: Value) {
        match (path, value) {
            ("ActiveTab", Value::F32(v)) => self.active_tab = v,
            ("Agree", Value::Bool(v)) => self.agree = v,
            ("Theme", Value::Str(v)) => self.theme = v,
            ("SliderValue", Value::F32(v)) => self.slider_value = v,
            ("NumberValue", Value::F32(v)) => self.number_value = v,
            ("Notes", Value::Str(v)) => self.notes = v,
            ("Query", Value::Str(v)) => self.query = v,
            ("DateText", Value::Str(v)) => self.date_text = v,
            ("Lang", Value::Str(v)) => self.lang = v,
            ("Lang2", Value::Str(v)) => self.lang2 = v,
            ("PickedDate", Value::Str(v)) => self.picked_date = v,
            ("PickedColor", Value::Str(v)) => self.picked_color = v,
            ("PickedGradient", Value::Str(v)) => self.picked_gradient = v,
            ("PaneWidth", Value::F32(v)) => self.pane_width = v,
            ("CurrentStep", Value::F32(v)) => self.current_step = v,
            ("ListSelIndex", Value::F32(v)) => self.list_sel_index = v,
            ("SelectedDate", Value::Str(v)) => self.selected_date = v,
            ("Status", Value::Str(v)) => self.status = v,
            _ => {}
        }
    }
}

fn handler_table() -> HandlerTable {
    handlers! {
        "link_clicked" => |vm, _v| { vm.set("Status", Value::Str("LinkLabel clicked.".to_string())); },
        "icon_button_clicked" => |vm, _v| { vm.set("Status", Value::Str("IconButton clicked.".to_string())); },
        "slider_changed" => |vm, v| {
            if let Value::F32(n) = v { vm.set("Status", Value::Str(format!("Slider = {n}"))); }
        },
        "number_changed" => |vm, v| {
            if let Value::F32(n) = v { vm.set("Status", Value::Str(format!("NumericField = {n}"))); }
        },
        "color_field_clicked" => |vm, _v| {
            vm.set("Status", Value::Str("ColorField clicked (its colour picker opens or closes).".to_string()));
        },
        "color_changed" => |vm, v| { vm.set("Status", Value::Str(format!("ColorField = {v:?}"))); },
        "gradient_changed" => |vm, v| { vm.set("Status", Value::Str(format!("GradientField = {v:?}"))); },
        "users_sorted" => |vm, v| { vm.set("Status", Value::Str(format!("DataTable sort: {v:?}"))); },
        "date_selected" => |vm, v| {
            if let Value::Str(d) = v { vm.set("Status", Value::Str(format!("MonthCalendar picked {d}"))); }
        },
    }
}

fn main() -> ExitCode {
    let mut watcher = FileWatcher::new(SHOWCASE_PATH);
    let mut runtime = Runtime::new();
    watcher.poll(&mut runtime);

    let mut vm = ShowcaseViewModel::default();
    let mut handlers = handler_table();

    let result = host::run_with_chrome("Kubuno XML views — family showcase", 1000, 760, Theme::light(), Chrome::Kubuno, move |c, f: &Frame| {
        host::request_repaint_after(250);
        watcher.poll(&mut runtime);

        let margin = 16.0_f32;
        let mut top = f.chrome_top + margin;
        let content = Rect::new(margin, top, (f.size.0 - margin).max(margin), (f.size.1 - margin).max(top));

        if !runtime.diagnostics().is_empty() {
            top = paint_diagnostics_banner(c, content, top, &runtime);
        }

        let body = Rect::new(content.left, top, content.right, (content.bottom - 24.0).max(top));
        if runtime.has_view() {
            let events = runtime.frame(c, f, &mut vm, &mut handlers, body);
            for e in &events {
                if matches!(e.kind, ViewEventKind::Clicked | ViewEventKind::Toggled(_) | ViewEventKind::Changed(_)) {
                    eprintln!("[showcase] event: {e:?}");
                }
            }
        } else {
            let t = c.theme();
            let fmt = &c.formats().body;
            c.text("Waiting for showcase.kbview to compile…", &body, fmt, &t.text_secondary, false);
        }

        let status_rect = Rect::new(content.left, content.bottom - 20.0, content.right, content.bottom);
        c.text(&format!("Status: {}", vm.status), &status_rect, &c.formats().caption, &c.theme().text_secondary, false);
    });

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("showcase: {e}");
            ExitCode::FAILURE
        }
    }
}

/// See `view_preview.rs`'s own copy — kept identical rather than shared,
/// since that example belongs to another agent's file right now.
fn paint_diagnostics_banner(c: &dyn ControlCanvas, content: Rect, top: f32, runtime: &Runtime) -> f32 {
    let diags = runtime.diagnostics();
    let lines = diags.len().min(3);
    let height = 20.0 + 16.0 * lines as f32;
    let banner = Rect::new(content.left, top, content.right, top + height);
    let theme = c.theme();
    c.fill_rounded(&banner, 4.0, &theme.danger_light);
    let fmt = &c.formats().body_strong;
    let head = Rect::new(banner.left + 8.0, banner.top + 4.0, banner.right - 8.0, banner.top + 20.0);
    c.text(&format!("{} diagnostic(s) — showing the last good view", diags.len()), &head, fmt, &theme.danger, false);
    let small = &c.formats().caption;
    for (i, d) in diags.iter().take(3).enumerate() {
        let y = banner.top + 22.0 + 16.0 * i as f32;
        let r = Rect::new(banner.left + 8.0, y, banner.right - 8.0, y + 16.0);
        c.text(&format!("{}:{}: {}", d.line, d.column, d.message), &r, small, &theme.danger, false);
    }
    banner.bottom + 8.0
}
