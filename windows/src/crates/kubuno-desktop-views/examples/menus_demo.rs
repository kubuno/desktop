//! Every feature of the menu family at run time (`vskubuno/docs/MENUS.md`): `examples/views/menus.kbview`
//! hosted with a view model whose handlers write what ran to the status line.
//!
//! ```text
//! cargo run -p kubuno-desktop-views --example menus_demo
//! ```
//!
//! For captures: `KUBUNO_UI_DARK=1` selects the dark theme; `KUBUNO_MENUS_ZOOM=0.5714` renders at
//! another scale (`host::set_zoom`: 0.5714 on a 175 % monitor gives 100 %); `KUBUNO_MENUS_OPEN=bar:N`
//! opens the menu bar's N-th menu from the keyboard at start (`page` opens the page's context menu,
//! `split` the split button's), so a screenshot shows it without touching the mouse.

use std::env;
use std::process::ExitCode;

use kubuno_desktop_controls::host::{self, Chrome, Frame};
use kubuno_desktop_ui::{Rect, Theme};
use kubuno_desktop_views::binding::{HandlerTable, Row, Value, ViewModel};
use kubuno_desktop_views::runtime::Runtime;
use kubuno_desktop_views::window::{MenuAnchor, MenuRequest};

const VIEW: &str = include_str!("views/menus.kbview");

/// The demo's state: the status line, the bold command's state, the recent files.
struct DemoViewModel {
    status: String,
    bold: bool,
}

impl ViewModel for DemoViewModel {
    fn get(&self, path: &str) -> Option<Value> {
        match path {
            "Status" => Some(Value::Str(self.status.clone())),
            "Bold" => Some(Value::Bool(self.bold)),
            "CanPaste" => Some(Value::Bool(false)),
            "Recent" => Some(Value::from(
                [("rapport-annuel.kbdoc", "C:/Documents/rapport-annuel.kbdoc"), ("budget-2026.kbsheet", "C:/Documents/budget-2026.kbsheet"), ("notes.md", "C:/Documents/notes.md")]
                    .iter()
                    .map(|(name, path)| Row::new().with("Name", Value::Str((*name).into())).with("Path", Value::Str((*path).into())))
                    .collect::<Vec<_>>(),
            )),
            _ => None,
        }
    }

    fn set(&mut self, path: &str, value: Value) {
        match (path, value) {
            ("Status", Value::Str(s)) => self.status = s,
            ("Bold", Value::Bool(b)) => self.bold = b,
            _ => {}
        }
    }
}

/// One handler per command of the view, each writing its name to the status line.
fn handler_table() -> HandlerTable {
    let mut table = HandlerTable::new();
    for name in [
        "new_file", "open_file", "open_recent", "save", "save_as", "quit", "undo", "cut", "copy", "paste", "delete", "layout_changed", "bold", "zoom_in", "zoom_out", "zoom_reset", "help", "about",
        "new_document", "new_sheet", "new_deck", "export", "export_pdf", "export_docx", "print", "share_link", "share_mail", "page_opening", "menu_activate", "menu_deactivate",
    ] {
        table.insert(name, Box::new(move |vm: &mut dyn ViewModel, _value: Value| {
            let line = match name {
                "bold" => format!("{name} (Gras : {})", vm.get("Bold").is_some_and(|v| v == Value::Bool(true))),
                _ => name.to_string(),
            };
            eprintln!("[menus_demo] {line}");
            vm.set("Status", Value::Str(format!("Dernière commande : {line}")));
        }));
    }
    table
}

fn main() -> ExitCode {
    let mut runtime = Runtime::new();
    if !runtime.reload_from_text(VIEW) {
        eprintln!("menus_demo: the view does not compile: {:?}", runtime.diagnostics());
        return ExitCode::FAILURE;
    }
    let mut vm = DemoViewModel { status: "Clic droit sur la page, Alt ou F10 pour la barre de menus, Ctrl+S…".into(), bold: false };
    let mut handlers = handler_table();
    let zoom = env::var("KUBUNO_MENUS_ZOOM").ok().and_then(|z| z.parse::<f32>().ok());
    let mut open = env::var("KUBUNO_MENUS_OPEN").ok();
    let mut frames = 0u32;
    let theme = if env::var_os("KUBUNO_UI_DARK").is_some() { Theme::dark() } else { Theme::light() };
    let result = host::run_with_chrome("Menus — Kubuno", 760, 480, theme, Chrome::Kubuno, move |c, f: &Frame| {
        if let Some(z) = zoom {
            host::set_zoom(z);
        }
        let body = Rect::new(0.0, f.chrome_top, f.size.0, f.size.1);
        runtime.frame(c, f, &mut vm, &mut handlers, body);
        frames += 1;
        if frames < 4 {
            host::request_repaint_after(16);
        }
        // A menu opened at start for a capture, once the view has painted (its bar's labels are known).
        if frames == 3 {
            if let Some(what) = open.take() {
                let request = match what.as_str() {
                    "page" => Some(MenuRequest { keyboard: true, ..MenuRequest::new("page_menu", MenuAnchor::Point(260.0, f.chrome_top + 160.0)) }),
                    // The split button is the second child of the `<Stack>` (the root's fourth child).
                    "split" => Some(MenuRequest { keyboard: true, ..MenuRequest::new(kubuno_desktop_views::menus::drop_down_menu_name("3.1"), MenuAnchor::Point(160.0, f.chrome_top + 76.0)) }),
                    // The menu bar is the root's third child (after the two commands): its items are `2.N`; the
                    // runtime places the menu under the item's label.
                    w => w.strip_prefix("bar:").and_then(|n| n.parse::<usize>().ok()).map(|n| MenuRequest {
                        keyboard: true,
                        bar: Some(("2".into(), n)),
                        ..MenuRequest::new(kubuno_desktop_views::menus::bar_menu_name(&format!("2.{n}")), MenuAnchor::Point(0.0, 0.0))
                    }),
                };
                if let Some(r) = request {
                    kubuno_desktop_views::window::request_menu(r);
                }
            }
        }
    });
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("menus_demo: {e}");
            ExitCode::FAILURE
        }
    }
}
