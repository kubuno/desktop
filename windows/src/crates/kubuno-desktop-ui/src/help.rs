//! Family — **help**: the accent [`HelpBubble`] and the « ? » [`HelpButton`]
//! that opens it.
//!
//! A port of `@ui/HelpBubble` (`core/frontend/src/ui/HelpBubble.tsx`) and of the
//! trigger the admin console wraps it in (`core/admin/AdminHelp.tsx`). Where the
//! web reaches for a portal and `getBoundingClientRect`, the desktop gets a pure
//! [`place`] and a painter that takes its result — the split
//! [`crate::display::Tooltip`] makes too, so every placement rule is a unit test
//! rather than a screenshot.
//!
//! ## Why a filled bubble and not a popover (from the web source)
//!
//! A white card on a white panel is another panel: it reads as more of the form
//! rather than as an answer to the question just asked. A bubble in the accent
//! reads as a remark someone made — plainly not part of the page — and its arrow
//! says which control it is about, which matters where several « ? » sit close.
//!
//! ## Placement
//!
//! `prefer` first, then bottom → top → right → left: the first side that fits,
//! failing that the one that overflows least. The bubble is pushed back into the
//! viewport, [`EDGE`] off its sides; the arrow stays on the edge facing the
//! anchor, at the anchor's centre, never nearer a corner than [`ARROW_MIN`] so
//! it cannot straddle the rounding.
//!
//! ## Hosting
//!
//! The bubble is interactive (its buttons take clicks) and may overflow the
//! window that opened it, so a caller hosts it in an interactive popup window
//! (`kubuno_desktop_controls::host::popup`) covering [`HelpPlacement::paint_bounds`],
//! placed against the whole screen rather than the window. The pointer comes
//! back in the caller's own coordinates, and the caller routes a click through
//! [`HelpBubble::part_at`]: [`HelpPart::Ok`] and
//! [`HelpPart::Outside`] close it (the web's backdrop), [`HelpPart::Action`]
//! runs the optional second action, [`HelpPart::Bubble`] is swallowed.
//!
//! ## Keyboard (from the web source)
//!
//! The web bubble is a `role="dialog"` whose dismiss button is `autoFocus`ed,
//! and a document-level `keydown` closes it on Escape. The desktop keeps that
//! contract through the pure [`HelpBubble::key`]: Escape closes, Enter or Space
//! press the focused button, Tab / Shift+Tab cycle between the buttons (the
//! bubble keeps the focus while it is open, like a dialog). A caller opening
//! the bubble focuses [`HelpBubble::initial_focus`] — « OK », the web's
//! `autoFocus` — and paints the focused button's ring with
//! [`HelpBubble::paint_placed_focused`]. The « ? » itself opens on Enter or
//! Space, as any `<button>` does, and takes the focus back when the bubble
//! closes.

use std::ops::{Deref, DerefMut};

use kubuno_drive_desktop_app_controls::{Canvas, Rect};
use kubuno_desktop_controls::buttons as replica;
use kubuno_desktop_controls::enums::Size;
use kubuno_desktop_controls::host::{vk, Modifiers};
use kubuno_desktop_controls::labels as kc;
use kubuno_desktop_controls::Control;
use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;
use windows::Win32::Graphics::DirectWrite::IDWriteTextFormat;

use crate::buttons::circular_hit;
use crate::display::Side;
use crate::feedback::wrap_lines;
use crate::metrics::{pill, radius, space, ShadowLayer};
use crate::{Widget, WidgetState};

// ─────────────────────────────────────────────────────────────────────────────
// Metrics — each one named after the web constant or class it comes from
// ─────────────────────────────────────────────────────────────────────────────

/// `width = 320`, the web default. `AdminHelp` asks for 360.
pub const WIDTH: f32 = 320.0;
/// `AdminHelp`'s width.
pub const WIDTH_ADMIN: f32 = 360.0;
/// `p-4`.
pub const PAD: f32 = space::LG;
/// `rounded-lg`.
pub const RADIUS: f32 = radius::LG;
/// `TIP`: how far the arrow shows beyond the bubble's edge. The web turns a
/// 14 px square by 45° and lets only its corner out, so what shows is a
/// right-angled tip — as tall as it is half-wide.
pub const ARROW: f32 = 5.0;
/// `GAP = TIP + 3`: anchor ↔ bubble, tip included.
pub const GAP: f32 = ARROW + 3.0;
/// `MIN`: the nearest the arrow may sit to a corner of the bubble.
pub const ARROW_MIN: f32 = 16.0;
/// `EDGE`: kept off the edges of the viewport.
pub const EDGE: f32 = space::SM;
/// The title's line box (`font-semibold leading-snug`).
pub const TITLE_LINE: f32 = 18.0;
/// A body line (`text-sm`, the desktop body's 20 px line box).
pub const LINE: f32 = 20.0;
/// `mt-2` between the title and the body.
pub const TITLE_GAP: f32 = space::SM;
/// `mt-3` above the button row.
pub const ACTIONS_GAP: f32 = space::MD;
/// `px-2 py-1` on a body line.
pub const BUTTON_H: f32 = LINE + 2.0 * space::XS;
pub const BUTTON_PAD_X: f32 = space::SM;
/// `gap-1` between the action and the dismiss button.
pub const BUTTON_GAP: f32 = space::XS;
/// `hover:bg-white/15`: the button's hover wash, over the accent.
pub const BUTTON_HOVER_ALPHA: f32 = 0.15;
/// `text-white/90`: the body is a shade quieter than the title.
pub const BODY_ALPHA: f32 = 0.9;
/// The keyboard focus ring (`focus-visible` outline), 2 DIP like every
/// desktop ring.
pub const FOCUS_RING: f32 = 2.0;

/// Tailwind's `shadow-xl`, verbatim:
/// `0 20px 25px -5px rgb(0 0 0 / .1), 0 8px 10px -6px rgb(0 0 0 / .1)`.
/// Pure black, as Tailwind writes it — hence [`SHADOW_BLACK`].
pub const SHADOW_XL: [ShadowLayer; 2] = [
    ShadowLayer { dy: 20.0, blur: 25.0, spread: -5.0, opacity: 0.10 },
    ShadowLayer { dy: 8.0, blur: 10.0, spread: -6.0, opacity: 0.10 },
];
const SHADOW_BLACK: (f32, f32, f32) = (0.0, 0.0, 0.0);

// ─────────────────────────────────────────────────────────────────────────────
// Placement — pure
// ─────────────────────────────────────────────────────────────────────────────

/// Where a bubble ended up.
///
/// Not `Debug`/`PartialEq`: [`Rect`] is neither (see
/// [`crate::display::Placement`]).
#[derive(Clone, Copy)]
pub struct HelpPlacement {
    pub rect: Rect,
    /// Which side of the ANCHOR the bubble sits on — so the arrow is on the
    /// bubble's opposite edge (a bubble `Bottom` of its anchor points up).
    pub side: Side,
    /// The arrow's position along the anchor-facing edge, measured from that
    /// edge's start (its left end for top/bottom, its top end for left/right).
    pub at: f32,
}

impl HelpPlacement {
    /// This placement moved by `(dx, dy)` — to repaint it in a popup window
    /// whose origin is not the page's.
    pub fn offset(self, dx: f32, dy: f32) -> Self {
        let r = self.rect;
        Self { rect: Rect::new(r.left + dx, r.top + dy, r.right + dx, r.bottom + dy), ..self }
    }

    /// Everything the bubble paints — the bubble, its arrow and its
    /// [`SHADOW_XL`] — which is what a popup window hosting it must cover, or
    /// the shadow is cut at the window's edge. The shadow reaches 7.5 DIP to the
    /// sides and 27.5 below (it is offset down); the arrow at most [`ARROW`].
    pub fn paint_bounds(&self) -> Rect {
        let r = self.rect;
        Rect::new(r.left - 8.0, r.top - (ARROW + 1.0), r.right + 8.0, r.bottom + 28.0)
    }
}

/// The web's search order after `prefer`.
const ORDER: [Side; 4] = [Side::Bottom, Side::Top, Side::Right, Side::Left];

/// Places a bubble of `size` against `anchor` inside `viewport`.
///
/// `prefer` first, then bottom → top → right → left: the first side with room
/// for the bubble, else the one it overflows least. Centred on the anchor along
/// the other axis, then pushed back inside the viewport ([`EDGE`] off it). The
/// arrow sits at the anchor's centre, held [`ARROW_MIN`] from either corner.
///
/// Pure: no canvas, no window — the same arithmetic as the web's `place()`,
/// with the viewport as a rectangle (the web's host offset folded in).
pub fn place(anchor: Rect, size: Size, prefer: Side, viewport: Rect) -> HelpPlacement {
    let (w, h) = (size.width, size.height);
    let acx = (anchor.left + anchor.right) / 2.0;
    let acy = (anchor.top + anchor.bottom) / 2.0;

    let room = |s: Side| match s {
        Side::Bottom => viewport.bottom - anchor.bottom - GAP - EDGE,
        Side::Top => anchor.top - viewport.top - GAP - EDGE,
        Side::Right => viewport.right - anchor.right - GAP - EDGE,
        Side::Left => anchor.left - viewport.left - GAP - EDGE,
    };
    let needs = |s: Side| if s.is_vertical() { h } else { w };

    // `prefer` leads; a repeat of it further down changes neither the first
    // fit nor the least overflow (ties keep the earlier side), so the web's
    // de-duplication is not needed here.
    let order = [prefer, ORDER[0], ORDER[1], ORDER[2], ORDER[3]];
    let side = order.iter().copied().find(|&s| room(s) >= needs(s)).unwrap_or_else(|| {
        order.iter().copied().fold(prefer, |best, s| {
            if room(s) - needs(s) > room(best) - needs(best) {
                s
            } else {
                best
            }
        })
    });

    let (left, top) = match side {
        Side::Bottom => (acx - w / 2.0, anchor.bottom + GAP),
        Side::Top => (acx - w / 2.0, anchor.top - GAP - h),
        Side::Right => (anchor.right + GAP, acy - h / 2.0),
        Side::Left => (anchor.left - GAP - w, acy - h / 2.0),
    };
    // `min(max(v, lo), max(lo, hi))`, as the web writes it: a viewport smaller
    // than the bubble pins it to the near edge rather than crossing the clamps.
    let lo_x = viewport.left + EDGE;
    let hi_x = (viewport.right - w - EDGE).max(lo_x);
    let lo_y = viewport.top + EDGE;
    let hi_y = (viewport.bottom - h - EDGE).max(lo_y);
    let left = left.max(lo_x).min(hi_x);
    let top = top.max(lo_y).min(hi_y);

    let (along, span) = if side.is_vertical() { (acx - left, w) } else { (acy - top, h) };
    let at = along.max(ARROW_MIN).min((span - ARROW_MIN).max(ARROW_MIN));

    HelpPlacement { rect: Rect::new(left, top, left + w, top + h), side, at }
}

// ─────────────────────────────────────────────────────────────────────────────
// HelpBubble
// ─────────────────────────────────────────────────────────────────────────────

/// What a point on (or off) an open bubble means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HelpPart {
    /// The dismiss button (« OK »).
    Ok,
    /// The optional second action, left of the dismiss button.
    Action,
    /// Anywhere else on the bubble or its arrow — swallowed.
    Bubble,
    /// Off the bubble: the web's backdrop, which closes it.
    Outside,
}

/// What a key does to an open bubble — see [`HelpBubble::key`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HelpKey {
    /// Close the bubble (Escape, or Enter/Space on the dismiss button).
    Close,
    /// Run the second action (Enter/Space on it). The bubble stays open, as on
    /// the web, where the action's own handler decides.
    Action,
    /// Move the focus to this button (Tab / Shift+Tab).
    Focus(HelpPart),
    /// Not a key the bubble acts on.
    Ignored,
}

/// The bubble — `@ui/HelpBubble`.
///
/// Its model is a [`kc::Label`] holding the body text (so `text` and `enabled`
/// are the replica's); the title, the button labels, the width and the side
/// preference are what the web component adds.
pub struct HelpBubble {
    inner: kc::Label,
    /// The bold opening line. Optional: a bubble may be one paragraph.
    pub title: Option<String>,
    /// The dismiss button's label — « OK » by default.
    pub ok_label: String,
    /// An optional second action's label, shown left of the dismiss button.
    pub action: Option<String>,
    pub width: f32,
    /// Where to try first.
    pub prefer: Side,
}

impl Deref for HelpBubble {
    type Target = kc::Label;
    fn deref(&self) -> &kc::Label {
        &self.inner
    }
}
impl DerefMut for HelpBubble {
    fn deref_mut(&mut self) -> &mut kc::Label {
        &mut self.inner
    }
}

impl HelpBubble {
    pub fn new(body: impl Into<String>) -> Self {
        let mut inner = kc::Label::new();
        inner.text = body.into();
        Self {
            inner,
            title: None,
            ok_label: "OK".into(),
            action: None,
            width: WIDTH,
            prefer: Side::Bottom,
        }
    }

    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn ok_label(mut self, label: impl Into<String>) -> Self {
        self.ok_label = label.into();
        self
    }

    pub fn action(mut self, label: impl Into<String>) -> Self {
        self.action = Some(label.into());
        self
    }

    pub fn width(mut self, width: f32) -> Self {
        self.width = width;
        self
    }

    pub fn prefer(mut self, side: Side) -> Self {
        self.prefer = side;
        self
    }

    /// The width text wraps at: the bubble minus its padding.
    fn text_width(&self) -> f32 {
        (self.width - 2.0 * PAD).max(1.0)
    }

    fn title_lines(&self, canvas: &dyn Canvas) -> Vec<String> {
        match &self.title {
            Some(t) => wrap_paragraphs(t, self.text_width(), canvas, &canvas.formats().body_strong),
            None => Vec::new(),
        }
    }

    fn body_lines(&self, canvas: &dyn Canvas) -> Vec<String> {
        let shown = self.inner.shown_text();
        wrap_paragraphs(&shown, self.text_width(), canvas, &canvas.formats().body)
    }

    /// Places this bubble against `anchor` in `viewport`, at its measured size.
    pub fn place(&self, canvas: &dyn Canvas, anchor: Rect, viewport: Rect) -> HelpPlacement {
        place(anchor, self.measure(canvas), self.prefer, viewport)
    }

    /// The dismiss button's rectangle and, when there is one, the action's —
    /// right-aligned on the bubble's last row. The paint pass and
    /// [`HelpBubble::part_at`] both read them from here, so a click can never
    /// land on a different button than the one drawn under the pointer.
    pub fn buttons(&self, canvas: &dyn Canvas, rect: Rect) -> (Rect, Option<Rect>) {
        let fmt = &canvas.formats().body;
        let top = rect.bottom - PAD - BUTTON_H;
        let ok_w = button_width(canvas, &self.ok_label, fmt);
        let ok = Rect::new(rect.right - PAD - ok_w, top, rect.right - PAD, top + BUTTON_H);
        let action = self.action.as_ref().map(|label| {
            let w = button_width(canvas, label, fmt);
            let right = ok.left - BUTTON_GAP;
            Rect::new(right - w, top, right, top + BUTTON_H)
        });
        (ok, action)
    }

    /// What a pointer at `(x, y)` is over, for an open bubble placed at `p`.
    pub fn part_at(&self, canvas: &dyn Canvas, p: &HelpPlacement, x: f32, y: f32) -> HelpPart {
        let (ok, action) = self.buttons(canvas, p.rect);
        if ok.contains(x, y) {
            HelpPart::Ok
        } else if action.is_some_and(|a| a.contains(x, y)) {
            HelpPart::Action
        } else if p.rect.contains(x, y) || arrow_box(p).contains(x, y) {
            HelpPart::Bubble
        } else {
            HelpPart::Outside
        }
    }

    /// The buttons in Tab order: the action (when there is one), then « OK ».
    pub fn tab_order(&self) -> Vec<HelpPart> {
        if self.action.is_some() {
            vec![HelpPart::Action, HelpPart::Ok]
        } else {
            vec![HelpPart::Ok]
        }
    }

    /// The button focused when the bubble opens: « OK » (`autoFocus`).
    pub fn initial_focus(&self) -> HelpPart {
        HelpPart::Ok
    }

    /// The button Tab (`forward`) or Shift+Tab reaches from `current`,
    /// wrapping inside the bubble. From nothing (or a non-button part), the
    /// first / last button.
    pub fn next_focus(&self, current: Option<HelpPart>, forward: bool) -> HelpPart {
        let order = self.tab_order();
        let n = order.len();
        match current.and_then(|c| order.iter().position(|&o| o == c)) {
            Some(i) if forward => order[(i + 1) % n],
            Some(i) => order[(i + n - 1) % n],
            None if forward => order[0],
            None => order[n - 1],
        }
    }

    /// What `key` (a key-down with `mods`) does to the open bubble whose
    /// focused button is `focus`. Pure — the caller takes the key from the
    /// host queue only when the answer is not [`HelpKey::Ignored`].
    pub fn key(&self, key: u16, mods: Modifiers, focus: Option<HelpPart>) -> HelpKey {
        match key {
            // Escape closes whatever the modifiers (the web's handler ignores
            // them).
            vk::ESCAPE => HelpKey::Close,
            vk::TAB if mods.matches(Modifiers::NONE) => HelpKey::Focus(self.next_focus(focus, true)),
            vk::TAB if mods.matches(Modifiers::SHIFT) => HelpKey::Focus(self.next_focus(focus, false)),
            vk::ENTER | vk::SPACE if mods.matches(Modifiers::NONE) => match focus {
                Some(HelpPart::Action) if self.action.is_some() => HelpKey::Action,
                Some(HelpPart::Ok) => HelpKey::Close,
                _ => HelpKey::Ignored,
            },
            _ => HelpKey::Ignored,
        }
    }

    /// Paints the bubble **and its arrow** at a computed placement. `hot` is
    /// the part under the pointer, which lights the button it names.
    pub fn paint_placed(&self, canvas: &dyn Canvas, p: &HelpPlacement, hot: Option<HelpPart>) {
        self.paint_placed_focused(canvas, p, hot, None);
    }

    /// [`HelpBubble::paint_placed`] with the keyboard focus ring on the button
    /// `focus` names — pass it only while the ring should show
    /// (`:focus-visible`).
    pub fn paint_placed_focused(&self, canvas: &dyn Canvas, p: &HelpPlacement, hot: Option<HelpPart>, focus: Option<HelpPart>) {
        let fill = canvas.theme().accent;
        // The shadow first, under both the bubble and its tip.
        canvas.draw_shadow(&p.rect, RADIUS, &SHADOW_XL, SHADOW_BLACK);
        canvas.fill_rounded(&p.rect, RADIUS, &fill);
        paint_arrow(canvas, p, &fill);
        self.paint_content(canvas, p.rect, hot, focus);
    }

    fn paint_content(&self, canvas: &dyn Canvas, rect: Rect, hot: Option<HelpPart>, focus: Option<HelpPart>) {
        let t = canvas.theme();
        let f = canvas.formats();
        // `text-white` on `--color-primary`: the accent's own foreground, which
        // is white in the light palette and the dark ink the dark palette pairs
        // with its lighter accent — never a literal white.
        let ink = t.accent_foreground;
        let quiet = fade(ink, BODY_ALPHA);

        let left = rect.left + PAD;
        let right = rect.right - PAD;
        let mut y = rect.top + PAD;

        let title = self.title_lines(canvas);
        for line in &title {
            canvas.text_ellipsis(line, &Rect::new(left, y, right, y + TITLE_LINE), &f.body_strong, &ink);
            y += TITLE_LINE;
        }
        if !title.is_empty() {
            y += TITLE_GAP;
        }
        for line in self.body_lines(canvas) {
            canvas.text_ellipsis(&line, &Rect::new(left, y, right, y + LINE), &f.body, &quiet);
            y += LINE;
        }

        let (ok, action) = self.buttons(canvas, rect);
        let button = |r: Rect, label: &str, part: HelpPart| {
            if hot == Some(part) {
                canvas.fill_rounded(&r, radius::SM, &fade(ink, BUTTON_HOVER_ALPHA));
            }
            // The label is ellipsised to its pill, so a long translation can
            // never spill over the bubble's padding.
            canvas.text_ellipsis_center(label, &r, &f.body, &ink);
            if focus == Some(part) {
                // The browser's focus-visible outline on a button over the
                // accent: the accent's own foreground, 2 DIP, following the
                // `rounded` pill.
                canvas.stroke_rounded_w(&r, radius::SM, &ink, FOCUS_RING);
            }
        };
        if let (Some(r), Some(label)) = (action, self.action.as_deref()) {
            button(r, label, HelpPart::Action);
        }
        button(ok, &self.ok_label, HelpPart::Ok);
    }
}

impl Widget for HelpBubble {
    fn model(&self) -> &dyn Control {
        &self.inner
    }

    fn type_name(&self) -> &'static str {
        "HelpBubble"
    }

    fn measure(&self, canvas: &dyn Canvas) -> Size {
        let title = self.title_lines(canvas).len() as f32;
        let body = self.body_lines(canvas).len() as f32;
        let title_h = if title > 0.0 { title * TITLE_LINE + TITLE_GAP } else { 0.0 };
        Size::new(self.width, PAD + title_h + body * LINE + ACTIONS_GAP + BUTTON_H + PAD)
    }

    /// The bubble alone, without its arrow — the `Widget` contract knows a
    /// rectangle and not which side of what it is on. A caller with a
    /// [`HelpPlacement`] uses [`HelpBubble::paint_placed`].
    fn paint(&self, canvas: &dyn Canvas, bounds: Rect, state: WidgetState) {
        let fill = canvas.theme().accent;
        canvas.draw_shadow(&bounds, RADIUS, &SHADOW_XL, SHADOW_BLACK);
        canvas.fill_rounded(&bounds, RADIUS, &fill);
        let hot = if state.hot { Some(HelpPart::Bubble) } else { None };
        let focus = if state.show_focus_ring() { Some(self.initial_focus()) } else { None };
        self.paint_content(canvas, bounds, hot, focus);
    }

    fn hit_test(&self, bounds: Rect, x: f32, y: f32) -> bool {
        bounds.contains(x, y)
    }
}

/// A button's width: its label plus `px-2` either side.
fn button_width(canvas: &dyn Canvas, label: &str, fmt: &IDWriteTextFormat) -> f32 {
    canvas.measure(label, fmt).ceil() + 2.0 * BUTTON_PAD_X
}

/// Wraps `text` at `width`, keeping its explicit line breaks: each paragraph
/// wraps on its own and an empty one keeps its blank line.
fn wrap_paragraphs(text: &str, width: f32, canvas: &dyn Canvas, fmt: &IDWriteTextFormat) -> Vec<String> {
    let mut out = Vec::new();
    for paragraph in text.split('\n') {
        let lines = wrap_lines(paragraph, width, |s| canvas.measure(s, fmt));
        if lines.is_empty() {
            out.push(String::new());
        } else {
            out.extend(lines);
        }
    }
    // A trailing newline is not a request for an extra blank line.
    while out.last().is_some_and(|l| l.is_empty()) {
        out.pop();
    }
    out
}

/// The arrow's base centre on the anchor-facing edge, and its apex.
fn arrow_points(p: &HelpPlacement) -> ((f32, f32), (f32, f32)) {
    let r = p.rect;
    match p.side {
        // Bubble below its anchor → the arrow rises off the top edge.
        Side::Bottom => ((r.left + p.at, r.top), (r.left + p.at, r.top - ARROW)),
        // Bubble above → it hangs off the bottom edge.
        Side::Top => ((r.left + p.at, r.bottom), (r.left + p.at, r.bottom + ARROW)),
        // Bubble right of its anchor → it points left.
        Side::Right => ((r.left, r.top + p.at), (r.left - ARROW, r.top + p.at)),
        // Bubble left → it points right.
        Side::Left => ((r.right, r.top + p.at), (r.right + ARROW, r.top + p.at)),
    }
}

/// The square the arrow's tip occupies, for hit-testing.
fn arrow_box(p: &HelpPlacement) -> Rect {
    let ((bx, by), (ax, ay)) = arrow_points(p);
    let (l, r) = (bx.min(ax) - ARROW, bx.max(ax) + ARROW);
    let (t, b) = (by.min(ay) - ARROW, by.max(ay) + ARROW);
    if p.side.is_vertical() {
        Rect::new(l, by.min(ay), r, by.max(ay))
    } else {
        Rect::new(bx.min(ax), t, bx.max(ax), b)
    }
}

/// The tip: one anti-aliased triangle, right-angled like the corner of the web's
/// turned square. `LIP` pushes its base a hair into the bubble so the two
/// same-colour fills overlap and leave no seam.
fn paint_arrow(canvas: &dyn Canvas, p: &HelpPlacement, fill: &D2D1_COLOR_F) {
    const LIP: f32 = 0.75;
    let ((bx, by), apex) = arrow_points(p);
    let h = ARROW;
    let (a, b) = match p.side {
        Side::Bottom => ((bx - h, by + LIP), (bx + h, by + LIP)),
        Side::Top => ((bx - h, by - LIP), (bx + h, by - LIP)),
        Side::Right => ((bx + LIP, by - h), (bx + LIP, by + h)),
        Side::Left => ((bx - LIP, by - h), (bx - LIP, by + h)),
    };
    canvas.fill_triangle(a, b, apex, fill);
}

fn fade(c: D2D1_COLOR_F, alpha: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F { a: c.a * alpha, ..c }
}

// ─────────────────────────────────────────────────────────────────────────────
// HelpButton — the « ? »
// ─────────────────────────────────────────────────────────────────────────────

/// The « ? » that opens a [`HelpBubble`] — `AdminHelp`'s trigger: a
/// `HelpCircle` glyph in a round `p-1` hit area, tertiary at rest, primary on
/// hover (over surface-2) and in the accent while its bubble is open.
pub struct HelpButton {
    inner: replica::Button,
    /// The glyph: 16 beside a page title, 14 inline in a label.
    pub glyph: f32,
    /// Whether its bubble is open — the button then stays in the accent.
    pub open: bool,
}

impl Deref for HelpButton {
    type Target = replica::Button;
    fn deref(&self) -> &replica::Button {
        &self.inner
    }
}
impl DerefMut for HelpButton {
    fn deref_mut(&mut self) -> &mut replica::Button {
        &mut self.inner
    }
}

impl Default for HelpButton {
    fn default() -> Self {
        Self::new()
    }
}

impl HelpButton {
    /// The 16 px glyph `AdminHelp` defaults to.
    pub const GLYPH: f32 = 16.0;
    /// The inline size, to sit in a line of body text.
    pub const GLYPH_INLINE: f32 = 14.0;

    pub fn new() -> Self {
        Self { inner: replica::Button::new(), glyph: Self::GLYPH, open: false }
    }

    pub fn inline() -> Self {
        Self { glyph: Self::GLYPH_INLINE, ..Self::new() }
    }

    pub fn open(mut self, open: bool) -> Self {
        self.open = open;
        self
    }
}

impl Widget for HelpButton {
    fn model(&self) -> &dyn Control {
        &self.inner
    }

    fn type_name(&self) -> &'static str {
        "HelpButton"
    }

    /// The glyph plus `p-1` on every side.
    fn measure(&self, _canvas: &dyn Canvas) -> Size {
        let d = self.glyph + 2.0 * space::XS;
        Size::new(d, d)
    }

    fn paint(&self, canvas: &dyn Canvas, bounds: Rect, state: WidgetState) {
        let t = canvas.theme();
        let enabled = self.inner.enabled && !state.disabled;
        let hot = enabled && state.hot;
        if hot {
            canvas.fill_rounded(&bounds, pill(bounds.bottom - bounds.top), &t.surface_2);
        }
        let colour = if !enabled {
            fade(t.text_tertiary, 0.5)
        } else if self.open {
            t.accent
        } else if hot {
            t.text_primary
        } else {
            t.text_tertiary
        };
        canvas.vector_icon("HelpCircle", &bounds, self.glyph, &colour);
        if state.show_focus_ring() {
            // The keyboard focus ring every Kubuno control draws: 2 DIP in the
            // accent, following the round shape — on `:focus-visible` only, so
            // a click does not leave a ring behind.
            canvas.stroke_rounded_w(&bounds, pill(bounds.bottom - bounds.top), &t.accent, FOCUS_RING);
        }
    }

    /// Round: the corners of the square are not the button (`rounded-full`).
    fn hit_test(&self, bounds: Rect, x: f32, y: f32) -> bool {
        circular_hit(bounds, x, y)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests — the placement rules, one per clause of the web's `place()`
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const VIEW: Rect = Rect { left: 0.0, top: 0.0, right: 1000.0, bottom: 800.0 };
    const BUBBLE: Size = Size { width: 320.0, height: 120.0 };

    fn anchor_at(x: f32, y: f32) -> Rect {
        Rect::new(x - 12.0, y - 12.0, x + 12.0, y + 12.0)
    }

    #[test]
    fn below_the_anchor_by_default_centred_on_it() {
        let a = anchor_at(500.0, 200.0);
        let p = place(a, BUBBLE, Side::Bottom, VIEW);
        assert_eq!(p.side, Side::Bottom);
        assert_eq!(p.rect.top, a.bottom + GAP);
        assert_eq!(p.rect.left, 500.0 - 160.0);
        // The arrow sits at the anchor's centre along the top edge.
        assert_eq!(p.at, 160.0);
    }

    #[test]
    fn flips_above_when_the_bottom_has_no_room() {
        let a = anchor_at(500.0, 750.0);
        let p = place(a, BUBBLE, Side::Bottom, VIEW);
        assert_eq!(p.side, Side::Top);
        assert_eq!(p.rect.bottom, a.top - GAP);
    }

    #[test]
    fn prefer_is_tried_first() {
        let a = anchor_at(500.0, 400.0);
        let p = place(a, BUBBLE, Side::Right, VIEW);
        assert_eq!(p.side, Side::Right);
        assert_eq!(p.rect.left, a.right + GAP);
    }

    #[test]
    fn falls_back_to_right_then_left_when_neither_vertical_side_fits() {
        // A viewport too short for the bubble above or below the anchor.
        let short = Rect::new(0.0, 0.0, 1000.0, 150.0);
        let a = anchor_at(200.0, 75.0);
        let p = place(a, BUBBLE, Side::Bottom, short);
        assert_eq!(p.side, Side::Right);
        let a = anchor_at(900.0, 75.0);
        let p = place(a, BUBBLE, Side::Bottom, short);
        assert_eq!(p.side, Side::Left);
    }

    #[test]
    fn nothing_fits_takes_the_side_that_overflows_least() {
        let tiny = Rect::new(0.0, 0.0, 200.0, 100.0);
        let a = anchor_at(100.0, 30.0);
        let p = place(a, BUBBLE, Side::Bottom, tiny);
        // Below: 100-42-8-8 = 42 of room for 120 (−78); above: 18-16 = 2 (−118);
        // right/left: far worse against a 320 width. Bottom overflows least.
        assert_eq!(p.side, Side::Bottom);
    }

    #[test]
    fn pushed_back_inside_the_viewport_with_the_arrow_still_on_the_anchor() {
        let a = anchor_at(30.0, 200.0);
        let p = place(a, BUBBLE, Side::Bottom, VIEW);
        assert_eq!(p.rect.left, EDGE);
        // The anchor's centre is 30, the bubble starts at 8: the arrow is at 22.
        assert_eq!(p.at, 22.0);
    }

    #[test]
    fn the_arrow_never_straddles_a_corner() {
        let a = anchor_at(5.0, 200.0);
        let p = place(a, BUBBLE, Side::Bottom, VIEW);
        assert_eq!(p.at, ARROW_MIN);
        let a = anchor_at(998.0, 200.0);
        let p = place(a, BUBBLE, Side::Bottom, VIEW);
        assert_eq!(p.at, BUBBLE.width - ARROW_MIN);
    }

    #[test]
    fn a_viewport_offset_from_the_origin_is_honoured() {
        let view = Rect::new(100.0, 50.0, 700.0, 650.0);
        let a = anchor_at(110.0, 60.0);
        let p = place(a, BUBBLE, Side::Bottom, view);
        assert!(p.rect.left >= view.left + EDGE);
        assert!(p.rect.top >= view.top + EDGE);
        assert!(p.rect.right <= view.right - EDGE);
    }

    #[test]
    fn tab_cycles_between_the_buttons_inside_the_bubble() {
        let plain = HelpBubble::new("x");
        assert_eq!(plain.tab_order(), vec![HelpPart::Ok]);
        assert_eq!(plain.next_focus(Some(HelpPart::Ok), true), HelpPart::Ok);
        let two = HelpBubble::new("x").action("En savoir plus");
        assert_eq!(two.initial_focus(), HelpPart::Ok);
        assert_eq!(two.next_focus(Some(HelpPart::Ok), true), HelpPart::Action, "wraps");
        assert_eq!(two.next_focus(Some(HelpPart::Action), true), HelpPart::Ok);
        assert_eq!(two.next_focus(Some(HelpPart::Ok), false), HelpPart::Action);
        assert_eq!(two.next_focus(None, false), HelpPart::Ok);
    }

    #[test]
    fn keys_close_activate_and_move_the_focus() {
        let b = HelpBubble::new("x").action("Plus");
        assert_eq!(b.key(vk::ESCAPE, Modifiers::NONE, None), HelpKey::Close);
        assert_eq!(b.key(vk::ENTER, Modifiers::NONE, Some(HelpPart::Ok)), HelpKey::Close);
        assert_eq!(b.key(vk::SPACE, Modifiers::NONE, Some(HelpPart::Action)), HelpKey::Action);
        assert_eq!(b.key(vk::TAB, Modifiers::NONE, Some(HelpPart::Action)), HelpKey::Focus(HelpPart::Ok));
        assert_eq!(b.key(vk::TAB, Modifiers::SHIFT, Some(HelpPart::Action)), HelpKey::Focus(HelpPart::Ok));
        assert_eq!(b.key(vk::ENTER, Modifiers::CTRL, Some(HelpPart::Ok)), HelpKey::Ignored);
        assert_eq!(b.key(vk::letter('a'), Modifiers::NONE, Some(HelpPart::Ok)), HelpKey::Ignored);
        let plain = HelpBubble::new("x");
        assert_eq!(plain.key(vk::ENTER, Modifiers::NONE, Some(HelpPart::Action)), HelpKey::Ignored, "no action to run");
    }

    #[test]
    fn a_viewport_narrower_than_the_bubble_pins_it_to_the_near_edge() {
        let narrow = Rect::new(0.0, 0.0, 200.0, 800.0);
        let a = anchor_at(100.0, 200.0);
        let p = place(a, BUBBLE, Side::Bottom, narrow);
        assert_eq!(p.rect.left, EDGE);
    }
}
