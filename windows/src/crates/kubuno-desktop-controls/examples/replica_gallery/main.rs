//! The control library's demo application — the « formulaires fictifs ».
//!
//! One page per control family, each mirroring the reference sheet the real
//! toolkit produced (`C:\kubuno-build\winforms-ref\shots\NN-family.png`), so the
//! two can be put side by side and read as the same document. Every pixel comes
//! from the library: the demo owns no painter, no font and no metric of its own,
//! and it hard-codes no DPI — sizes come from each control's `preferred_size`
//! and from `Canvas::scale()` inside the controls themselves.
//!
//! ```text
//! cargo run -p kubuno-desktop-controls --example replica_gallery
//! cargo run -p kubuno-desktop-controls --example replica_gallery -- --page 06-range --dark
//! cargo run -p kubuno-desktop-controls --example replica_gallery -- --list
//! ```
//!
//! There is deliberately no `--shot`: the host hands the paint closure a
//! `&dyn Canvas`, which has no read-back, and neither `Frame` nor `Canvas`
//! exposes the swap chain or the concrete `Painter`. Capturing a page to PNG
//! from inside this binary is therefore not possible with today's API — screen
//! captures are taken from outside instead.

mod pages;
mod sheet;

use kubuno_drive_desktop_app_controls::Theme;
use kubuno_desktop_controls::buttons::CheckBox;
use kubuno_desktop_controls::host::{self, Frame};
use kubuno_desktop_controls::layout_panels::{flow_layout, FlowChild, FlowDirection};
use kubuno_desktop_controls::{Appearance, Control, ControlCanvas, Padding, Rect};

/// The band the page switcher sits in, and the spacing between its tabs — the
/// same DIP vocabulary the sheet uses.
const SWITCHER_PADDING: Padding = Padding::all(8.0);
const TAB_MARGIN: Padding = Padding::all(4.0);

/// The measuring line a wrapping run is laid out against; the horizontal flow
/// only ever compares against the right edge, so the bottom just has to be out
/// of the way.
const UNBOUNDED: f32 = 100_000.0;

/// `COINIT_APARTMENTTHREADED`.
const COINIT_APARTMENTTHREADED: u32 = 0x2;

// The renderer behind `Canvas` builds its bitmaps through WIC, which is a COM
// class: on a thread with no apartment `CoCreateInstance` fails, `Renderer::new`
// returns an error, and `host::run` — which swallows it with `.ok()` — paints a
// blank window. The drive shell's own `main` initialises COM before it opens its
// window for exactly this reason; `host::run` does not, so the demo must.
//
// Declared by hand because this crate does not enable the `windows` crate's
// `Win32_System_Com` feature, and the demo may not change its manifest.
#[link(name = "ole32")]
extern "system" {
    fn CoInitializeEx(reserved: *mut core::ffi::c_void, co_init: u32) -> i32;
}

fn main() {
    let Some(opts) = parse_args() else { return };

    // Failure is not fatal: a thread already in an apartment answers
    // `RPC_E_CHANGED_MODE`, and the renderer works either way.
    unsafe {
        let _ = CoInitializeEx(std::ptr::null_mut(), COINIT_APARTMENTTHREADED);
    }

    let mut current = opts.page;
    let mut was_down = false;

    let result = host::run(
        "Kubuno — galerie de contrôles",
        1500,
        1000,
        opts.theme,
        move |c, f| {
            let area = Rect::new(0.0, 0.0, f.size.0, f.size.1);
            let band = switcher(c, f, area, &mut current, &mut was_down);
            let page = Rect::new(area.left, band, area.right, area.bottom);
            (pages::PAGES[current].build)().paint(c, page);
        },
    );
    if let Err(e) = result {
        eprintln!("[gallery] la fenêtre n'a pas pu s'ouvrir : {e}");
    }
}

/// Paints the page switcher and returns the bottom of its band.
///
/// The tabs are `CheckBox`es in `Appearance::Button` — the library's own toggle
/// face, so the selected page reads as pressed without the demo inventing a
/// look. Hit-testing goes through `Control::hit_test` against `Frame::mouse`,
/// and the click is edge-triggered so holding the button does not cycle pages.
fn switcher(c: &dyn ControlCanvas, f: &Frame, area: Rect, current: &mut usize, was_down: &mut bool) -> f32 {
    let mut tabs: Vec<CheckBox> = pages::PAGES
        .iter()
        .map(|p| {
            let mut t = CheckBox::new();
            t.appearance = Appearance::Button;
            t.text = p.label.to_string();
            t
        })
        .collect();

    let cells: Vec<FlowChild> = tabs
        .iter()
        .map(|t| FlowChild { size: t.preferred_size(c), margin: TAB_MARGIN, flow_break: false })
        .collect();
    let display = Rect::new(
        area.left + SWITCHER_PADDING.left,
        area.top + SWITCHER_PADDING.top,
        area.right - SWITCHER_PADDING.right,
        area.top + UNBOUNDED,
    );
    let rects = flow_layout(display, FlowDirection::LeftToRight, true, &cells);
    for (t, r) in tabs.iter_mut().zip(&rects) {
        t.control_mut().set_bounds(*r);
    }

    let pressed_now = f.mouse_down && !*was_down;
    *was_down = f.mouse_down;
    if pressed_now {
        if let Some(i) = tabs.iter().position(|t| t.hit_test(f.mouse.0, f.mouse.1)) {
            *current = i;
        }
    }

    for (i, t) in tabs.iter_mut().enumerate() {
        if i == *current {
            t.set_checked(true);
        } else if t.hit_test(f.mouse.0, f.mouse.1) {
            // `ButtonBase` paints its face from the theme alone, so hover can
            // only be expressed through the one colour it does resolve from the
            // control — the foreground.
            t.fore_color = Some(c.theme().accent);
        }
    }
    for t in &tabs {
        t.paint(c, t.bounds);
    }

    rects.iter().map(|r| r.bottom).fold(area.top, f32::max) + SWITCHER_PADDING.bottom
}

struct Options {
    page:  usize,
    theme: Theme,
}

/// Returns `None` when the binary should exit without opening a window (help,
/// a listing, or a bad argument).
fn parse_args() -> Option<Options> {
    let mut opts = Options { page: 0, theme: Theme::light() };
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--page" | "-p" => {
                let Some(name) = args.next() else {
                    eprintln!("[gallery] --page attend un nom de page");
                    return None;
                };
                match pages::find(&name) {
                    Some(i) => opts.page = i,
                    None => {
                        eprintln!("[gallery] page inconnue : {name}");
                        list();
                        return None;
                    }
                }
            }
            "--dark" => opts.theme = Theme::dark(),
            "--light" => opts.theme = Theme::light(),
            "--list" | "-l" => {
                list();
                return None;
            }
            "--help" | "-h" => {
                usage();
                return None;
            }
            other => {
                eprintln!("[gallery] option inconnue : {other}");
                usage();
                return None;
            }
        }
    }
    Some(opts)
}

fn usage() {
    println!(
        "galerie de contrôles Kubuno\n\
         \n\
         Options :\n  \
           --page, -p <nom>   ouvre directement une famille (nom, famille ou numéro)\n  \
           --list, -l         liste les pages disponibles\n  \
           --dark | --light   thème de la fenêtre (clair par défaut)\n  \
           --help, -h         cette aide"
    );
}

fn list() {
    println!("pages disponibles :");
    for p in pages::PAGES {
        println!("  {:<14} {}", p.id, p.label);
    }
}
