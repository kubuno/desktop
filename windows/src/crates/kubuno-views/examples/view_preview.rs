//! A live preview for `.kbview` files — `vskubuno/docs/XML_VIEWS.md` §8's
//! phase 2c deliverable: open a file, render it through `kubuno_views`'
//! interpreter, and hot-reload on every save without restarting or losing
//! the view model's state.
//!
//! ```text
//! cargo run -p kubuno-views --example view_preview -- <file.kbview>
//! ```
//!
//! Polls the file's mtime (cheap: one `stat` per check) and asks the host to
//! wake the window up every 250 ms with [`host::request_repaint_after`], so a
//! save is picked up promptly even with no mouse/keyboard input — a hot
//! reload should not need a nudge from the user to show up. A parse or
//! validation failure never blanks the window: [`Runtime`] keeps painting the
//! last tree that DID compile and this example draws a small red banner with
//! the file, line and column on top of it (`XML_VIEWS.md` §5).

use std::env;
use std::process::ExitCode;

use kubuno_controls::host::{self, Chrome, Frame};
use kubuno_controls::ControlCanvas;
use kubuno_ui::{Rect, Theme};
use kubuno_views::binding::{HandlerTable, Value, ViewModel};
use kubuno_views::runtime::{FileWatcher, Runtime};
use kubuno_views::{handlers, node::ViewEventKind};

/// The demo code-behind's state — `XML_VIEWS.md` §7's `SettingsState`,
/// trimmed to what the five registered components can bind to. `status` is
/// not bound by any element; it is written by the handlers below through the
/// very same [`ViewModel::set`] path every other binding uses (`"Status"`),
/// which is the point: a handler only ever needs the trait, never the
/// concrete struct.
struct SettingsViewModel {
    notifications: bool,
    offline: bool,
    proxy: String,
    status: String,
}

impl Default for SettingsViewModel {
    fn default() -> Self {
        Self {
            notifications: true,
            offline: false,
            proxy: String::new(),
            status: "Ready.".to_string(),
        }
    }
}

impl ViewModel for SettingsViewModel {
    fn get(&self, path: &str) -> Option<Value> {
        match path {
            "Notifications" => Some(Value::Bool(self.notifications)),
            "Offline" => Some(Value::Bool(self.offline)),
            "Proxy" => Some(Value::Str(self.proxy.clone())),
            "Status" => Some(Value::Str(self.status.clone())),
            _ => None,
        }
    }

    fn set(&mut self, path: &str, value: Value) {
        match (path, value) {
            ("Notifications", Value::Bool(b)) => self.notifications = b,
            // `Offline` is one-way bound in the demo view (see
            // `examples/views/settings.kbview`'s comment): the interpreter's
            // own two-way write-back never touches it. But `offline_toggled`
            // below DOES write it, through this very `set` — which is the
            // point of `XML_VIEWS.md` §7's worked example ("binding cannot
            // guess [the side effect]; a handler can"): in a real app the
            // handler would ask the live sync engine to flip and that
            // engine's own state is what `Offline` would actually be bound
            // to; this demo has no such engine, so the handler is where that
            // state legitimately lives instead.
            ("Offline", Value::Bool(b)) => self.offline = b,
            ("Proxy", Value::Str(s)) => self.proxy = s,
            ("Status", Value::Str(s)) => self.status = s,
            _ => {}
        }
    }
}

fn handler_table() -> HandlerTable {
    handlers! {
        "offline_toggled" => |vm, v| {
            if let Value::Bool(on) = v {
                vm.set("Offline", Value::Bool(on));
                let msg = if on {
                    "Working offline (Offline set by offline_toggled, not by binding)".to_string()
                } else {
                    "Back online.".to_string()
                };
                vm.set("Status", Value::Str(msg));
            }
        },
        "save_clicked" => |vm, _v| {
            let proxy = match vm.get("Proxy") {
                Some(Value::Str(s)) if !s.is_empty() => s,
                _ => "(none)".to_string(),
            };
            vm.set("Status", Value::Str(format!("Saved. Proxy = {proxy}")));
        },
    }
}

fn main() -> ExitCode {
    let Some(path) = env::args().nth(1) else {
        eprintln!("usage: view_preview <file.kbview>");
        return ExitCode::FAILURE;
    };

    let mut watcher = FileWatcher::new(&path);
    let mut runtime = Runtime::new();
    watcher.poll(&mut runtime); // Load once before the window even opens.

    let mut vm = SettingsViewModel::default();
    let mut handlers = handler_table();

    let title = format!("Kubuno view preview — {path}");
    let result = host::run_with_chrome(&title, 900, 700, if env::var_os("KUBUNO_UI_DARK").is_some() { Theme::dark() } else { Theme::light() }, Chrome::Kubuno, move |c, f: &Frame| {
        host::request_repaint_after(250);
        watcher.poll(&mut runtime);

        let margin = 24.0_f32;
        let mut top = f.chrome_top + margin;
        let content = Rect::new(margin, top, (f.size.0 - margin).max(margin), (f.size.1 - margin).max(top));

        if !runtime.diagnostics().is_empty() {
            top = paint_diagnostics_banner(c, content, top, &runtime);
        }

        let body = Rect::new(content.left, top, content.right, (content.bottom - 28.0).max(top));
        if runtime.has_view() {
            let events = runtime.frame(c, f, &mut vm, &mut handlers, body);
            for e in &events {
                if matches!(e.kind, ViewEventKind::Clicked | ViewEventKind::Toggled(_) | ViewEventKind::Changed(_)) {
                    // Handlers already reacted through the view model; this
                    // is only a developer-visible trace of what fired.
                    eprintln!("[view_preview] event: {e:?}");
                }
            }
        } else {
            let t = c.theme();
            let fmt = &c.formats().body;
            c.text("Waiting for a view that compiles…", &body, fmt, &t.text_secondary, false);
        }

        // The status line — what the two handlers write to, and the
        // clearest signal a click/toggle actually reached the view model.
        let status_rect = Rect::new(content.left, content.bottom - 20.0, content.right, content.bottom);
        let status = vm.get("Status").and_then(|v| if let Value::Str(s) = v { Some(s) } else { None }).unwrap_or_default();
        c.text(&format!("Status: {status}"), &status_rect, &c.formats().caption, &c.theme().text_secondary, false);
    });

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("view_preview: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Draws the "last good tree, new diagnostics" banner (`XML_VIEWS.md` §5) and
/// returns the y where normal content should resume.
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
