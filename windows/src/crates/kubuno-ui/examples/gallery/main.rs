//! The Kubuno primitive gallery — a single navigable window over every family.
//!
//! One page per family, each showing every variant in every state — and, where
//! a primitive replaces a hand-written predecessor from `drive-app-controls`,
//! the two painted **side by side from the same inputs**. That pairing is the
//! point: the shipping shell calls the predecessor 149 times, so the rebuild is
//! only acceptable if the pair is indistinguishable.
//!
//! Launch it and browse: a strip of tabs across the top switches pages, so the
//! whole design system is explorable from one window. Ctrl+Tab /
//! Ctrl+Shift+Tab (or Ctrl+PageDown / Ctrl+PageUp) switch pages from the
//! keyboard; Tab / Shift+Tab move the focus within the page.
//!
//! ```text
//! cargo run -p kubuno-ui --example gallery -j 1
//! cargo run -p kubuno-ui --example gallery -j 1 -- --page fields   # open on a page
//! ```
//!
//! `KUBUNO_UI_DARK=1` opens in the dark palette — several tokens differ, and a
//! primitive that hard-codes a colour shows up immediately.

mod pages;

use std::cell::Cell;

use drive_app_controls::Theme;
use kubuno_controls::host::{self, vk, Cursor, Frame, Modifiers};
use kubuno_ui::navigation::{Tabs, TabsController};
use kubuno_ui::{containers::Splitter, Canvas, Rect, WidgetState};

use pages::interact::{
    focus_begin, focus_end, focus_reset, set_panel_rect, set_panel_w, with_focus, DEFAULT_PANEL_W, MAX_PANEL_W,
    MIN_PANEL_W,
};

/// The page `steps` tabs away from `from`, wrapping — Ctrl+Tab / Ctrl+PageDown
/// go forward, Ctrl+Shift+Tab / Ctrl+PageUp back, as in a browser.
fn page_step(from: usize, steps: isize) -> usize {
    let n = PAGES.len() as isize;
    (((from as isize + steps) % n + n) % n) as usize
}

/// One page: its name on the tab, what it paints, and an optional overlay pass
/// painted **after** the nav strip. Floating surfaces (a tooltip, an open menu)
/// must be composited over the whole window, the tab strip included, so a page
/// that has one registers it here rather than drawing it in `draw` — where the
/// nav, painted last, would cover it.
type PageFn = fn(&dyn Canvas, &Frame);
type Page = (&'static str, PageFn, Option<PageFn>, Option<PageFn>);

/// The pages, in the order the families were built.
const PAGES: &[Page] = &[
    ("buttons", pages::buttons::draw, None, Some(pages::buttons::interactive_column)),
    ("text", pages::text::draw, None, Some(pages::text::interactive_column)),
    ("fields", pages::fields::draw, None, Some(pages::fields::interactive_column)),
    ("lists", pages::lists::draw, None, Some(pages::lists::interactive_column)),
    ("range", pages::range::draw, None, Some(pages::range::interactive_column)),
    ("containers", pages::containers::draw, None, Some(pages::containers::interactive_column)),
    ("navigation", pages::navigation::draw, None, Some(pages::navigation::interactive_column)),
    ("display", pages::display::draw, Some(pages::display::overlay), Some(pages::display::interactive_column)),
    ("views", pages::views::draw, None, Some(pages::views::interactive_column)),
    ("color", pages::color::draw, None, Some(pages::color::interactive_column)),
    ("datetime", pages::datetime::draw, None, Some(pages::datetime::interactive_column)),
    ("dialogs", pages::dialogs::draw, None, Some(pages::dialogs::interactive_column)),
    ("editors", pages::editors::draw, None, Some(pages::editors::interactive_column)),
    ("feedback", pages::feedback::draw, None, Some(pages::feedback::interactive_column)),
    ("tables", pages::tables::draw, None, Some(pages::tables::interactive_column)),
    ("composition", pages::composition::draw, None, Some(pages::composition::interactive_column)),
    ("richtext", pages::richtext::draw, None, Some(pages::richtext::interactive_column)),
    ("ribbon", pages::ribbon::draw, None, Some(pages::ribbon::interactive_column)),
    ("docking", pages::docking::draw, None, Some(pages::docking::interactive_column)),
];

/// Builds the `Tabs` control the gallery uses as its nav strip, populated with
/// every page. The selected index is set from the caller so the model stays a
/// simple `Cell<usize>` in `main`.
fn build_tabs(selected: usize) -> Tabs {
    let mut tabs = Tabs::new().small();
    for (name, _, _, _) in PAGES {
        tabs = tabs.with(name);
    }
    tabs.selected_index = selected as i32;
    tabs
}

fn main() -> windows::core::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let start = args
        .iter()
        .position(|a| a == "--page")
        .and_then(|i| args.get(i + 1))
        .and_then(|name| PAGES.iter().position(|(n, _, _, _)| n == name))
        .unwrap_or(0);

    // Same theme rule the dialogs use: follow the OS. `Theme::detect()` reads
    // `AppsUseLightTheme` from the registry, so the gallery matches whatever the
    // Windows session runs under. `KUBUNO_UI_DARK` / `KUBUNO_UI_LIGHT` force a
    // side, when needed to compare the two palettes.
    let theme = if std::env::var_os("KUBUNO_UI_DARK").is_some() {
        Theme::dark()
    } else if std::env::var_os("KUBUNO_UI_LIGHT").is_some() {
        Theme::light()
    } else {
        Theme::detect()
    };

    // The selected page and the previous button state live across paints — the
    // host re-invokes the closure on every move and click, so a click is the
    // edge where the button goes down.
    let selected = Cell::new(start);
    // The navigation strip's live state (indicator slide, scroll), kept by the
    // component's own controller across frames.
    let tabs_ctl = std::cell::RefCell::new(TabsController::new());
    let prev_down = Cell::new(false);
    // Splitter state: pane 1 is the exposition, pane 2 is the interactive
    // column. Its distance is pane 1's width; the panel's width is derived,
    // published to `interact::PANEL_W` before the page draws, and re-clamped
    // every frame so a window resize cannot leave the panel out of range.
    let dist = Cell::new(0.0_f32);            // 0 = "seed on first frame"
    let dragging = Cell::new(false);
    let drag_off = Cell::new(0.0_f32);

    // For captures at another scale: `KUBUNO_UI_ZOOM=0.5714` renders at 100 % on a 175 % monitor
    // (`host::set_zoom`, the window's frame and corners included).
    let zoom = std::env::var("KUBUNO_UI_ZOOM").ok().and_then(|z| z.parse::<f32>().ok()).filter(|z| *z > 0.0);

    host::run_with_chrome(
        "Kubuno UI — galerie",
        1280,
        960,
        theme,
        host::Chrome::Kubuno,
        move |c, f| {
        if let Some(z) = zoom {
            host::set_zoom(z);
        }
        let (w, _h) = f.size;
        let (mx, my) = f.mouse;

        // A click is the rising edge of the left button; a click on a tab is
        // handled below, once the strip is known.
        let clicked = f.mouse_down && !prev_down.get();
        prev_down.set(f.mouse_down);

        // ── Keyboard: gallery shortcuts first, then the page's focus ring ──
        // Taken from the host queue before the page runs, so a page never
        // sees them. Plain Tab / Shift+Tab are left for the focus ring.
        // The docking page keeps Ctrl+Tab for its dock (cycling the panels,
        // as in Visual Studio): there, Ctrl+PageDown / Ctrl+PageUp switch pages.
        let dock_page = PAGES[selected.get()].0 == "docking";
        let ctrl_tab = |mods| if dock_page { 0 } else { host::take_key(vk::TAB, mods) as isize };
        let steps = ctrl_tab(Modifiers::CTRL)
            + host::take_key(vk::PAGE_DOWN, Modifiers::CTRL) as isize
            - ctrl_tab(Modifiers::CTRL_SHIFT)
            - host::take_key(vk::PAGE_UP, Modifiers::CTRL) as isize;
        if steps != 0 {
            selected.set(page_step(selected.get(), steps));
            focus_reset();
        }
        focus_begin(f);
        let mut tabs = build_tabs(selected.get());

        // ── The splitter between the tab control and the interactive column ──
        // The body sits below the Kubuno caption band (the host reserved
        // `chrome_top` for it) and takes the rest of the window.
        let body = Rect::new(0.0, f.chrome_top, w, f.size.1);
        // Seed on the first frame the closure sees a real width.
        if dist.get() <= 0.0 && w > MIN_PANEL_W {
            dist.set((w - DEFAULT_PANEL_W).max(MIN_PANEL_W));
        }
        let mut splitter = Splitter::vertical()
            .with_minimums(MIN_PANEL_W, MIN_PANEL_W)
            .with_distance(dist.get().min(w - MIN_PANEL_W).max(MIN_PANEL_W));
        // Drag: the grip owns the pointer while the button is held. Recorded on
        // the rising edge so a click on the grip does not jump the split to the
        // pointer's exact x — the offset preserves where the pointer met it.
        if clicked && splitter.hit_test_grip(body, mx, my) {
            dragging.set(true);
            drag_off.set(mx - splitter.splitter_rect(body).left);
        }
        if !f.mouse_down {
            dragging.set(false);
        }
        if dragging.get() {
            let wanted = mx - drag_off.get();
            splitter.drag_to(body, wanted, my);
            dist.set(splitter.panel1_rect(body).right - body.left);
        }
        // Publish the panel's width so every page's `interact::PANEL_W()` reads
        // the current split — the same call that resizes on drag, kept live.
        let panel_w = (body.right - splitter.panel1_rect(body).right).clamp(MIN_PANEL_W, MAX_PANEL_W);
        set_panel_w(panel_w);

        // ── Pane 1 = the Tabs control (strip + page's exposition) ─────────
        // ── Pane 2 = the interactive column ───────────────────────────────
        // Under a clip each: a child that measured itself too wide is cut at
        // the pane's edge, never bleeds into the neighbour.
        let pane1 = splitter.panel1_rect(body);
        let pane2 = splitter.panel2_rect(body);
        let tab_page = tabs.page_rect(c, pane1);
        // The strip is pane 1 above the page area: the tab control's own
        // geometry, handed to the component as its bounds so the wheel and the
        // focus only answer over the strip, never over the page below it.
        let strip = Rect::new(pane1.left, pane1.top, pane1.right, tab_page.top);
        c.push_clip(&pane1);
        // The tab strip is transparent, as on the web: it shows the surface of
        // its parent. Here the parent is this pane, so the pane paints the page
        // ground itself — in the theme's colour, dark included.
        c.fill_rounded(&pane1, 0.0, &c.theme().window_background);
        // The gallery's navigation IS the Tabs component, driven by its own
        // controller — the sliding indicator, scroll arrows, wheel and arrow
        // keys are the component's, not a copy of them kept here, so every
        // change made to the component shows up in the gallery's own chrome.
        let tab_focus = with_focus(|ring| ring.register("gallery-tabs", strip));
        let run = tabs_ctl.borrow_mut().frame(c, &mut tabs, strip, f, Some(tab_focus));
        if run.changed {
            // A page switch clears the old page's focus — unless the switch
            // came from the arrow keys on the strip, which keeps the focus.
            if !tab_focus.focused {
                focus_reset();
            }
            selected.set(usize::try_from(tabs.selected_index).unwrap_or(0));
        }
        if run.hot.is_some() {
            host::set_cursor(Cursor::Hand);
        }
        let (_, draw, overlay, interactive) = PAGES[selected.get()];
        // The page paints into the tab control's page area, through the
        // component's own scrolled panel: anything the page lays out past the
        // area's edges brings up a vertical or horizontal bar. Its layout
        // still speaks in `Frame::size`; the frame it receives is translated.
        // The pages size themselves from `size.0 - PANEL_W` and `size.1`; the
        // size is trimmed so those land on the panel's own (inset) edges, and a
        // page that fits shows no bar.
        let trim = (pane1.right - tab_page.right, f.size.1 - tab_page.bottom);
        tabs_ctl.borrow_mut().page(c, &tabs, tab_page, f, |c, pf| {
            let pf = host::Frame { size: (pf.size.0 - trim.0, pf.size.1 - trim.1), ..*pf };
            draw(c, &pf)
        });
        c.pop_clip();
        if let Some(interactive) = interactive {
            // Publish the pane rectangle to `interact::panel_rect`: the pages
            // still call it, and it now returns the splitter's pane 2 rather
            // than a rectangle recomputed from `f.size` and `PANEL_W`.
            set_panel_rect(pane2);
            c.push_clip(&pane2);
            interactive(c, f);
            c.pop_clip();
        }

        // Paint the splitter itself. Kubuno's Splitter widget owns the
        // divider's look — a hairline at rest, growing to the bar's full width
        // in `border_strong` on hover, and turning to the accent while
        // dragged. Painted after the page so the line lands over any card
        // edge that runs up to it.
        {
            let over = splitter.hit_test_grip(body, mx, my) || dragging.get();
            if over {
                host::set_cursor(Cursor::ResizeEW);
            }
            let state = WidgetState::REST.hot(over).pressed(dragging.get());
            <Splitter as kubuno_ui::Widget>::paint(&splitter, c, body, state);
        }
        if let Some(overlay) = overlay {
            overlay(c, f);
        }
        focus_end();
    })
}
