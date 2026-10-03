//! The shared machinery an interactive column needs.
//!
//! The gallery's pages are otherwise stateless — each is a `fn(&Canvas, &Frame)`
//! that paints a fixed exposition. An interactive column adds two things: it
//! remembers what the user did between frames (a toggle stays on), and it reads
//! the live input to light hover, press and click and to take keys. This module
//! holds the small reusable parts so every page's column works the same way;
//! the *state* itself (which toggle, which value) is a `thread_local` the page
//! owns, because it differs per family.
//!
//! Keyboard: the host queues keys and typed text per frame
//! (`kubuno_desktop_controls::host::events`), and the gallery runs ONE
//! [`kubuno_desktop_ui::FocusRing`] for the current page ([`focus_begin`] /
//! [`focus_end`] in `main.rs`). A page registers its focusable controls with
//! [`Live::focus`] while painting, in visual order; Tab / Shift+Tab then walk
//! them, a click focuses one, and [`FocusState::visible`] says whether to draw
//! the ring (`:focus-visible`).

#![allow(dead_code)] // Each page uses a subset; the helper set is shared.

use std::cell::RefCell;

use kubuno_desktop_controls::host::{self, Frame, InputEvent, Modifiers};
use kubuno_desktop_ui::focus::{FocusId, FocusOpts, FocusRing, FocusState};
use kubuno_desktop_ui::{Canvas, Rect, WidgetState};


/// The width the interactive column takes on the right of every page.
///
/// This used to be a plain `const`, but the shell now hosts a splitter whose
/// grip lets the user tune the column's width. The pages still lay out against
/// « the right slab of the window », so they keep asking for that width — the
/// answer just varies per frame. The shell writes it in [`set_panel_w`] before
/// each frame, and reads it out of [`PANEL_W`] on the way back.
pub const DEFAULT_PANEL_W: f32 = 440.0;
pub const MIN_PANEL_W: f32 = 260.0;
pub const MAX_PANEL_W: f32 = 640.0;

thread_local! {
    static PANEL_W_CELL: std::cell::Cell<f32> = const { std::cell::Cell::new(DEFAULT_PANEL_W) };
}

/// The width the interactive column takes on the right of every page, right
/// now. Reading a stale value is impossible: the shell writes the frame's
/// width before it calls the page, and drops back to the default outside of a
/// paint. The old `const PANEL_W` name is kept as a `fn` at call sites so no
/// page has to be touched, using a `PANEL_W` fn on a module reads the same as
/// a `const` at the point of call. Yes it looks like a constant on read — that
/// is the point.
#[allow(non_snake_case)]
pub fn PANEL_W() -> f32 { PANEL_W_CELL.with(|c| c.get()) }

/// The shell calls this once per frame to publish the splitter's current
/// value; the clamp is authoritative because the splitter also does it, but a
/// second one here makes the module safe against a rogue caller.
pub fn set_panel_w(w: f32) {
    PANEL_W_CELL.with(|c| c.set(w.clamp(MIN_PANEL_W, MAX_PANEL_W)));
}

thread_local! {
    static PANEL_RECT_CELL: std::cell::Cell<Rect> = const { std::cell::Cell::new(Rect { left: 0.0, top: 0.0, right: 0.0, bottom: 0.0 }) };
}

/// Called from `main.rs` once per frame with the splitter's pane 2 — the real
/// interactive column, in the caller's coordinates. The pages keep asking for
/// [`panel_rect`], but the answer now IS the splitter's pane, not a synthetic
/// rectangle recomputed from `f.size` and `PANEL_W`.
pub fn set_panel_rect(r: Rect) {
    PANEL_RECT_CELL.with(|c| c.set(r));
}

// ── Focus: one ring for the current page ─────────────────────────────────

thread_local! {
    static FOCUS: RefCell<FocusRing> = RefCell::new(FocusRing::new());
}

/// `main.rs`, once per frame BEFORE the page paints: resolves clicks and
/// Tab / Shift+Tab against last frame's registrations.
pub fn focus_begin(f: &Frame) {
    FOCUS.with(|r| r.borrow_mut().begin_frame(f));
}

/// `main.rs`, once per frame AFTER the page painted.
pub fn focus_end() {
    FOCUS.with(|r| r.borrow_mut().end_frame());
}

/// `main.rs`, on a page switch: nothing focused, no stale registrations.
pub fn focus_reset() {
    FOCUS.with(|r| r.borrow_mut().reset());
}

/// Direct access to the page's ring — programmatic focus, `focus_visibly`,
/// `keep_focus_in` for an open popup, `set_blur_on_escape`…
pub fn with_focus<R>(f: impl FnOnce(&mut FocusRing) -> R) -> R {
    FOCUS.with(|r| f(&mut r.borrow_mut()))
}


/// One frame of input, with the click EDGE already resolved.
///
/// A click is the frame the left button goes from up to down. Resolving it here,
/// once, keeps every control's "was I clicked" a simple `hit(rect)` rather than
/// each re-deriving the edge (and racing on it).
///
/// The keyboard side is not copied in: [`Live::take_key`], [`Live::take_text`]
/// and [`Live::events`] read the host's per-frame queue, where an event taken
/// by one control is gone for the next.
#[derive(Clone, Copy)]
pub struct Live {
    pub mouse: (f32, f32),
    pub down:  bool,
    pub clicked: bool,
    /// Ctrl / Shift / Alt / Win held as the frame starts.
    pub mods: Modifiers,
    /// Wheel notches since last frame; `.1 > 0` scrolls down (web sign).
    pub wheel: (f32, f32),
    /// 1 / 2 / 3 for a single / double / triple click (read with `clicked`).
    pub click_count: u8,
    /// The window holds the keyboard focus (hide the caret when not).
    pub window_focused: bool,
}

impl Live {
    /// `prev_down` is the button state the page kept from last frame.
    pub fn new(f: &Frame, prev_down: bool) -> Self {
        Live {
            mouse: f.mouse,
            down: f.mouse_down,
            clicked: f.mouse_down && !prev_down,
            mods: f.mods,
            wheel: f.wheel,
            click_count: f.click_count,
            window_focused: f.window_focused,
        }
    }

    pub fn hover(&self, r: Rect) -> bool {
        r.contains(self.mouse.0, self.mouse.1)
    }

    /// A click landed inside `r` this frame — the edge a toggle or button acts on.
    pub fn hit(&self, r: Rect) -> bool {
        self.clicked && self.hover(r)
    }

    /// A double-click (the second press of one) landed inside `r` this frame.
    pub fn double_hit(&self, r: Rect) -> bool {
        self.hit(r) && self.click_count == 2
    }

    /// A triple-click landed inside `r` this frame.
    pub fn triple_hit(&self, r: Rect) -> bool {
        self.hit(r) && self.click_count >= 3
    }

    /// The transient state (hover, and press while held) for a control at `r`.
    pub fn state(&self, r: Rect) -> WidgetState {
        let h = self.hover(r);
        WidgetState::REST.hot(h).pressed(h && self.down)
    }

    /// Wheel travel in DIP (100 per notch), `.1 > 0` = scroll down — only
    /// when the pointer is over `r`, `(0, 0)` otherwise.
    ///
    /// A non-zero answer CLAIMS the wheel ([`host::claim_wheel`]): the control
    /// scrolling with it is under the pointer, so the tab panel around it does
    /// not scroll too.
    pub fn wheel_over(&self, r: Rect) -> (f32, f32) {
        if self.hover(r) {
            if self.wheel != (0.0, 0.0) {
                host::claim_wheel();
            }
            (self.wheel.0 * host::WHEEL_NOTCH_DIP, self.wheel.1 * host::WHEEL_NOTCH_DIP)
        } else {
            (0.0, 0.0)
        }
    }

    // ── Focus ───────────────────────────────────────────────────────────

    /// Registers a focusable control at `r` (call in paint order, only when
    /// enabled) and returns whether it is focused / shows its ring.
    pub fn focus(&self, id: impl Into<FocusId>, r: Rect) -> FocusState {
        with_focus(|ring| ring.register(id, r))
    }

    /// [`Live::focus`] with options (`FocusOpts::TEXT` for a text field).
    pub fn focus_with(&self, id: impl Into<FocusId>, r: Rect, opts: FocusOpts) -> FocusState {
        with_focus(|ring| ring.register_with(id, r, opts))
    }

    /// [`Live::state`] plus focus: registers the control and returns hover,
    /// press, `focused` and `focus_visible` in one [`WidgetState`].
    pub fn focus_state(&self, id: impl Into<FocusId>, r: Rect) -> WidgetState {
        self.focus(id, r).apply(self.state(r))
    }

    /// Consumes this frame's Escape, if any (first caller wins).
    pub fn take_escape(&self) -> bool {
        with_focus(|ring| ring.take_escape())
    }

    // ── Keyboard ────────────────────────────────────────────────────────

    /// This frame's key / text events nobody took yet.
    pub fn events(&self) -> Vec<InputEvent> {
        host::events()
    }

    /// Takes `vk` pressed with exactly `mods` (see `host::vk`); true if it was.
    pub fn take_key(&self, vk: u16, mods: Modifiers) -> bool {
        host::take_key(vk, mods) > 0
    }

    /// Whether `vk` was pressed with no modifier, without taking it.
    pub fn key(&self, vk: u16) -> bool {
        host::key_pressed(vk, Modifiers::NONE)
    }

    /// Takes the text typed this frame (empty if none).
    pub fn take_text(&self) -> String {
        host::take_text()
    }
}

/// The interactive column's rectangle — the splitter's pane 2, published by
/// `main.rs` every frame through [`set_panel_rect`]. The `size` argument is
/// ignored, but the signature is kept so the 15 pages that call this from
/// their `interactive_column` don't need to be touched. Before the first frame
/// wrote a real rect (early paint, headless test) the returned rectangle is
/// empty, which every downstream layout tolerates.
pub fn panel_rect(_size: (f32, f32)) -> Rect {
    PANEL_RECT_CELL.with(|c| c.get())
}

/// Paints the column as a `Panel` (`Surface::Layer`, LG padding), draws the
/// heading « Interactif — souris » and the hairline under it, and returns a
/// cursor at the first row. Uses the framework's container instead of hand
/// drawing a background — one theme, one set of tokens, no drift.
///
/// Returns `(content_left, first_y, content_right)`.
pub fn panel(c: &dyn Canvas, r: Rect) -> (f32, f32, f32) {
    use kubuno_desktop_ui::containers::{Panel, Surface};
    use kubuno_desktop_ui::WidgetState;
    use kubuno_desktop_controls::enums::Padding;
    use kubuno_desktop_ui::Widget;

    // The Panel's chrome: ground, border in the theme's role. `Padding::all`
    // sets the same 16 DIP on every side, the LG token both the exposition and
    // this column already speak in.
    let panel = Panel::new()
        .with_surface(Surface::Layer)
        .with_padding(Padding::all(kubuno_desktop_ui::metrics::space::LG));
    panel.paint(c, r, WidgetState::REST);

    // The inner rectangle the Panel leaves after its padding — the row every
    // caller then lays out inside.
    let inset = kubuno_desktop_ui::metrics::space::LG;
    let left = r.left + inset;
    let right = r.right - inset;
    let mut y = r.top + inset;

    let t = c.theme();
    let f = c.formats();
    c.text(
        "Interactif — souris",
        &Rect::new(left, y, right, y + 24.0),
        &f.heading,
        &t.text_primary,
        false,
    );
    y += 28.0;
    c.fill_rounded(&Rect::new(left, y, right, y + 1.0), 0.0, &t.card_stroke);
    (left, y + 12.0, right)
}

/// A caption above an interactive row, returning the row's top.
pub fn caption(c: &dyn Canvas, left: f32, right: f32, y: f32, label: &str) -> f32 {
    let t = c.theme();
    let f = c.formats();
    // Ellipsised at the column's edge: a long caption must never run past the
    // live column into the window's border.
    c.text_ellipsis(label, &Rect::new(left, y, right, y + 16.0), &f.caption, &t.text_secondary);
    y + 20.0
}
