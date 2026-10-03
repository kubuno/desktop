//! The crash window of a Kubuno GUI application (see [`super::diagnostics`]): what the user sees
//! when the UI thread panics — the Kubuno counterpart of Windows Forms' `ThreadExceptionDialog`.
//!
//! A small host window with the Kubuno caption and the Kubuno theme: an error badge, "<App> ran
//! into an unexpected error and has to close.", the panic message and its location, a *Show
//! details* expander with the backtrace (scrollable), and *Open log*, *Copy* and *Close* buttons
//! (Enter/Esc close it, Ctrl+C copies). It runs its own host on its own thread: the panicking UI
//! thread only waits for it, so it never re-enters its window procedure while the window is up.

use std::path::PathBuf;

use drive_app_controls::{Rect, Theme};

use super::{vk, Chrome, HostOptions, Modifiers};

/// Everything the crash window shows.
pub(crate) struct CrashReport {
    /// The application's display name ("EvtApp").
    pub app: String,
    pub message: String,
    pub location: String,
    /// Thread and backtrace.
    pub details: String,
    /// The log file the report was written to (none while a debugger is attached).
    pub log: Option<PathBuf>,
}

impl CrashReport {
    /// The whole report as text, what *Copy* puts on the clipboard.
    fn text(&self) -> String {
        let mut s = format!("{}: {}\r\n{}\r\n\r\n{}", self.app, self.message, self.location, self.details);
        if let Some(log) = &self.log {
            s.push_str(&format!("\r\n\r\nLog: {}", log.display()));
        }
        s
    }
}

/// Shows the crash window and waits until it is closed. `false` when it could not be shown (the
/// caller then falls back to a system message box).
pub(crate) fn show(report: CrashReport) -> bool {
    let window = std::thread::Builder::new().name("kubuno-crash-window".into()).spawn(move || run(report));
    matches!(window.map(|w| w.join()), Ok(Ok(true)))
}

struct Texts {
    heading: String,
    details_show: &'static str,
    details_hide: &'static str,
    open_log: &'static str,
    copy: &'static str,
    copied: &'static str,
    close: &'static str,
    log: &'static str,
}

fn texts(app: &str, french: bool) -> Texts {
    if french {
        Texts {
            heading: format!("{app} a rencontré une erreur inattendue et doit se fermer."),
            details_show: "Afficher les détails",
            details_hide: "Masquer les détails",
            open_log: "Ouvrir le journal",
            copy: "Copier",
            copied: "Copié",
            close: "Fermer",
            log: "Journal :",
        }
    } else {
        Texts {
            heading: format!("{app} ran into an unexpected error and has to close."),
            details_show: "Show details",
            details_hide: "Hide details",
            open_log: "Open log",
            copy: "Copy",
            copied: "Copied",
            close: "Close",
            log: "Log:",
        }
    }
}

const PAD: f32 = 24.0;
const BADGE: f32 = 40.0;
const BUTTON_H: f32 = 32.0;
const BUTTON_GAP: f32 = 8.0;
const RADIUS: f32 = 6.0;
const LINE_H: f32 = 17.0;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Action {
    Details,
    OpenLog,
    Copy,
    Close,
}

fn run(report: CrashReport) -> bool {
    let t = texts(&report.app, super::diagnostics::is_french_ui());
    let mut opts = HostOptions::new(&report.app, 620, 420, Theme::light());
    opts.chrome = Chrome::Kubuno;
    opts.client_size = true;
    opts.fit_work_area = true;
    opts.diagnostics = false;

    let mut expanded = false;
    let mut scroll = 0.0f32;
    let mut was_down = false;
    let mut pressed: Option<Action> = None;
    let mut copied_until = 0u64;
    let detail_lines = report.details.lines().count() as f32;

    super::run_with_options(opts, move |c, f| {
        let theme = c.theme();
        let formats = c.formats();
        let body = Rect::new(0.0, f.chrome_top, f.size.0, f.size.1);
        c.fill_rounded(&body, 0.0, &theme.window_background);

        // Badge and texts.
        let left = PAD + BADGE + 16.0;
        let badge = Rect::new(PAD, body.top + PAD, PAD + BADGE, body.top + PAD + BADGE);
        c.fill_rounded(&badge, BADGE / 2.0, &theme.danger);
        c.text("!", &badge, &formats.heading_strong, &theme.accent_foreground, true);
        let heading = Rect::new(left, body.top + PAD - 2.0, body.right - PAD, body.top + PAD + 48.0);
        c.text(&t.heading, &heading, &formats.subtitle, &theme.text_primary, false);
        let message = Rect::new(left, heading.bottom + 4.0, body.right - PAD, heading.bottom + 48.0);
        c.push_clip(&message);
        c.text(&report.message, &message, &formats.body_wrap, &theme.text_primary, false);
        c.pop_clip();
        let location = Rect::new(left, message.bottom + 2.0, body.right - PAD, message.bottom + 20.0);
        c.text_ellipsis(&report.location, &location, &formats.caption, &theme.text_secondary);

        // Details expander.
        let toggle_label = if expanded { t.details_hide } else { t.details_show };
        let chevron = if expanded { "\u{25BE}" } else { "\u{25B8}" };
        let toggle = Rect::new(left, location.bottom + 12.0, left + c.measure(toggle_label, &formats.body) + 24.0, location.bottom + 36.0);
        let buttons_top = body.bottom - PAD - BUTTON_H;
        let area = Rect::new(PAD, toggle.bottom + 8.0, body.right - PAD, buttons_top - 16.0);

        // Buttons, right to left: Close (primary), Copy, Open log.
        let copied = super::now_ms() < copied_until;
        let copy_label = if copied { t.copied } else { t.copy };
        let mut specs: Vec<(Action, &str, bool)> = vec![(Action::Close, t.close, true), (Action::Copy, copy_label, false)];
        if report.log.as_ref().is_some_and(|p| p.exists()) {
            specs.push((Action::OpenLog, t.open_log, false));
        }
        let mut right = body.right - PAD;
        let mut buttons: Vec<(Action, Rect, &str, bool)> = Vec::new();
        for (action, label, primary) in specs {
            let w = (c.measure(label, &formats.body_strong) + 32.0).max(96.0);
            buttons.push((action, Rect::new(right - w, buttons_top, right, buttons_top + BUTTON_H), label, primary));
            right -= w + BUTTON_GAP;
        }

        // Input.
        let (mx, my) = f.mouse;
        let hovered = |r: &Rect| !f.pointer_outside() && r.contains(mx, my);
        let mut hit: Option<Action> = None;
        if hovered(&toggle) {
            hit = Some(Action::Details);
        }
        for (action, rect, _, _) in &buttons {
            if hovered(rect) {
                hit = Some(*action);
            }
        }
        if hit.is_some() {
            super::set_cursor(super::Cursor::Hand);
        }
        let mut fire: Option<Action> = None;
        if f.mouse_down && !was_down {
            pressed = hit;
        } else if !f.mouse_down && was_down {
            if pressed.is_some() && pressed == hit {
                fire = pressed;
            }
            pressed = None;
        }
        was_down = f.mouse_down;
        if super::take_key(vk::ESCAPE, Modifiers::NONE) > 0 || super::take_key(vk::ENTER, Modifiers::NONE) > 0 {
            fire = Some(Action::Close);
        }
        if super::take_key(vk::letter('C'), Modifiers::CTRL) > 0 {
            fire = Some(Action::Copy);
        }
        match fire {
            Some(Action::Details) => {
                expanded = !expanded;
                scroll = 0.0;
            }
            Some(Action::Copy) => {
                super::set_clipboard_text(&report.text());
                copied_until = super::now_ms() + 1500;
                super::request_repaint_after(1600);
            }
            Some(Action::OpenLog) => {
                if let Some(log) = &report.log {
                    open_file(log);
                }
            }
            Some(Action::Close) => super::close_window(),
            None => {}
        }

        // Paint the expander and the details (or the log path).
        let link = if hit == Some(Action::Details) { theme.accent_hover } else { theme.accent };
        let chevron_rect = Rect::new(toggle.left, toggle.top, toggle.left + 18.0, toggle.bottom);
        c.text(chevron, &chevron_rect, &formats.body, &link, false);
        let label_rect = Rect::new(toggle.left + 18.0, toggle.top, toggle.right, toggle.bottom);
        c.text(toggle_label, &label_rect, &formats.body, &link, false);
        if expanded {
            c.fill_rounded(&area, RADIUS, &theme.surface_2);
            c.stroke_rounded(&area, RADIUS, &theme.card_stroke);
            let inner = Rect::new(area.left + 12.0, area.top + 8.0, area.right - 12.0, area.bottom - 8.0);
            let max_scroll = (detail_lines * LINE_H - (inner.bottom - inner.top)).max(0.0);
            if f.wheel.1 != 0.0 && hovered(&area) {
                scroll = (scroll + f.wheel.1 * 3.0 * LINE_H).clamp(0.0, max_scroll);
            }
            c.push_clip(&inner);
            let text = Rect::new(inner.left, inner.top - scroll, inner.right, inner.top - scroll + detail_lines * LINE_H + 40.0);
            c.text(&report.details, &text, &formats.caption_wrap, &theme.text_secondary, false);
            c.pop_clip();
        } else if let Some(log) = &report.log {
            let line = Rect::new(PAD, area.top, area.right, area.top + 20.0);
            c.text_ellipsis(&format!("{} {}", t.log, log.display()), &line, &formats.caption, &theme.text_tertiary);
        }

        for (action, rect, label, primary) in &buttons {
            let hot = hit == Some(*action);
            if *primary {
                c.fill_rounded(rect, RADIUS, if hot { &theme.accent_hover } else { &theme.accent });
                c.text(label, rect, &formats.body_strong, &theme.accent_foreground, true);
            } else {
                c.fill_rounded(rect, RADIUS, if hot { &theme.control_fill_hover } else { &theme.card_background });
                c.stroke_rounded(rect, RADIUS, &theme.border_strong);
                c.text(label, rect, &formats.body_strong, &theme.text_primary, true);
            }
        }
    })
    .is_ok()
}

/// Opens `path` with its associated application (the log in the user's text editor).
fn open_file(path: &std::path::Path) {
    let file: Vec<u16> = path.as_os_str().encode_wide_nul();
    let verb: Vec<u16> = "open".encode_utf16().chain(std::iter::once(0)).collect();
    // SAFETY: NUL-terminated UTF-16 buffers alive for the call; no owner window, default show state.
    unsafe {
        ShellExecuteW(0, verb.as_ptr(), file.as_ptr(), std::ptr::null(), std::ptr::null(), SW_SHOWNORMAL);
    }
}

trait EncodeWideNul {
    fn encode_wide_nul(&self) -> Vec<u16>;
}

impl EncodeWideNul for std::ffi::OsStr {
    fn encode_wide_nul(&self) -> Vec<u16> {
        use std::os::windows::ffi::OsStrExt;
        self.encode_wide().chain(std::iter::once(0)).collect()
    }
}

const SW_SHOWNORMAL: i32 = 1;

#[link(name = "shell32", kind = "raw-dylib")]
extern "system" {
    fn ShellExecuteW(hwnd: isize, verb: *const u16, file: *const u16, params: *const u16, dir: *const u16, show: i32) -> isize;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_copied_text_has_every_part_of_the_report() {
        let r = CrashReport {
            app: "EvtApp".into(),
            message: "boom".into(),
            location: "src\\main.rs:1:1".into(),
            details: "thread 'main'\r\n0: frame".into(),
            log: Some(PathBuf::from("C:\\logs\\evtapp.log")),
        };
        let text = r.text();
        for part in ["EvtApp: boom", "src\\main.rs:1:1", "0: frame", "Log: C:\\logs\\evtapp.log"] {
            assert!(text.contains(part), "{part} in {text}");
        }
        assert!(texts("EvtApp", true).heading.starts_with("EvtApp a rencontré"));
        assert!(texts("EvtApp", false).heading.starts_with("EvtApp ran into"));
    }
}
