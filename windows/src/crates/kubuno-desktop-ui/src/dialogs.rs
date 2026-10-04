//! Kubuno primitives — **dialogs**: the family the project's own rules make
//! mandatory.
//!
//! `CLAUDE.md` forbids the browser dialogs outright — « **Jamais** de dialogs
//! navigateur (`alert`/`confirm`/`prompt`) → `ConfirmDialog`/`useConfirm`,
//! `PromptDialog`/`prompt()` du core » — and until this file existed that rule
//! had no desktop counterpart at all: a shell that needed a confirmation had
//! nothing to call. Everything here is therefore a port of a **web component
//! that already ships**, not a design:
//!
//! | primitive | web source |
//! |---|---|
//! | [`FloatingWindow`] | `core/frontend/src/ui/FloatingWindow.tsx` |
//! | [`ConfirmDialog`] | `ui/ConfirmDialog.tsx` |
//! | [`PromptDialog`] | `ui/PromptDialog.tsx` |
//! | [`ConflictDialog`] | `ui/ConflictDialog.tsx` |
//! | [`Popover`] | `ui/AnchoredPopover.tsx` |
//! | [`Toast`] | `ui/Toast.tsx` |
//!
//! All six were **read**, not measured: the desktop has no browser to measure
//! in, which is what the brief asks a family to say out loud.
//!
//! ## What is reused rather than rebuilt
//!
//! A dialog is a floating surface with a header, a body and an action bar —
//! which is a [`crate::containers::Panel`] with three docked bands. So:
//!
//! * the **three bands** come from
//!   [`kubuno_desktop_controls::layout::layout`], through `Panel`'s `fill`/`top`/
//!   `bottom` builders: not one line of « the header is 44 tall so the body
//!   starts at `top + 44` » is written here;
//! * the **body's children** are a `Panel`'s children, placed and painted by
//!   that family;
//! * the **action bar's buttons** are [`crate::buttons::Button`] as they ship
//!   — variant, size, colours and hit-testing included;
//! * [`PromptDialog`]'s input is [`crate::text::TextField`], not a second
//!   field;
//! * [`Popover`]'s anchoring is [`crate::display::place`] — the tooltip's own
//!   flip-and-clamp, which already solved the four edges. See
//!   [`place_anchored`] for the two things a popover does differently and why
//!   they could not be expressed as arguments to it.
//!
//! ## Measure, then place — never inside the painter
//!
//! Every surface here answers two questions as pure(ish) functions:
//!
//! ```ignore
//! let size = dialog.measure(canvas, host_width);   // content decides the height
//! let rect = dialogs::place(host, size);           // the host decides the position
//! dialog.paint_modal(canvas, host, rect, state);
//! ```
//!
//! [`place`] and [`stack`] take no canvas at all, which is what makes « a
//! dialog in a host smaller than itself », « a popover against each of the four
//! edges » and « a stack of toasts overflowing » unit tests instead of
//! screenshots.

use std::ops::{Deref, DerefMut};

use kubuno_drive_desktop_app_controls::themes::shape::SHADOW_WINDOW;
use kubuno_drive_desktop_app_controls::{Canvas, Rect};
use kubuno_desktop_controls::enums::{Padding, Size};
use kubuno_desktop_controls::host::{vk, Modifiers};
use kubuno_desktop_controls::labels as kc;
use kubuno_desktop_controls::Control;
use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;
use windows::Win32::Graphics::DirectWrite::IDWriteTextFormat;

use crate::buttons::{Button, Size as ButtonSize, Variant};
use crate::containers::Panel;
use crate::display::{self, Placement, Role, Side};
use crate::focus::FocusId;
use crate::metrics::{height, radius, space, SHADOW_GREY, SHADOW_MENU};
use crate::text::{field_height, TextField};
use crate::widget::{Widget, WidgetState};

// ─────────────────────────────────────────────────────────────────────────────
// Metrics — every one of them citing the line it was read from.
//
// `crate::metrics` is the crate's one table and it is not this family's file,
// so what these six surfaces need and the design system never named as a token
// is stated here, once. Everything that IS a token (the 44 title bar, the 6
// window radius, the spacing scale, the button heights) is taken from there and
// never re-typed.
// ─────────────────────────────────────────────────────────────────────────────

/// The window's own radius by default: a Kubuno desktop window's
/// ([`kubuno_desktop_controls::host::form::DEFAULT_CORNER_RADIUS`], 8 DIP, Windows 11's). The web's is
/// square (`--kb-window-radius: 0px`, `theme.css`, a GPU workaround of 2026-08-30); desktop
/// windows, the in-window ones included, are rounded since 2026-10-02
/// ([`FloatingWindow::corner_radius`]).
const WINDOW_RADIUS: f32 = kubuno_desktop_controls::host::form::DEFAULT_CORNER_RADIUS;

/// `FloatingWindow`'s `defaultWidth = 560`, and the three widths its callers
/// override it with: `ConfirmDialog` 380, `PromptDialog` 400, `ConflictDialog`
/// 400.
pub const WIDTH_DEFAULT: f32 = 560.0;
pub const WIDTH_CONFIRM: f32 = 380.0;
pub const WIDTH_PROMPT: f32 = 400.0;
pub const WIDTH_CONFLICT: f32 = 400.0;

/// `minWidth = 280`, `minHeight = 120`.
pub const MIN_WIDTH: f32 = 280.0;
pub const MIN_HEIGHT: f32 = 120.0;

/// The keep-off from the host's edges: the window is clamped to
/// `calc(100vw - 16px)` / `calc(100vh - 16px)`, i.e. eight DIP on each side.
pub const HOST_MARGIN: f32 = space::SM;

/// Where the window sits vertically: `top: 33%` with `translateY(-33%)`, so a
/// third of the free space is above it. Horizontally it is `left: 50%` with
/// `translateX(-50%)` — plain centring, which needs no constant.
const TOP_FRACTION: f32 = 0.33;

// The title band is the standard Kubuno window band (32 DIP, Windows 11's caption height — decision
// of 2026-10-04, where the web's `.kb-window-titlebar` is 50): its height
// ([`kubuno_desktop_controls::window_chrome::TITLEBAR_HEIGHT`]), its insets and its close button
// are laid out and painted by [`kubuno_desktop_controls::window_chrome`], which holds the numbers.

/// The footer: `px-4 py-3`, `gap-2` between the buttons, each `min-w-[96px]`.
/// Its height is therefore two paddings around a `md` button.
const FOOTER_PAD_X: f32 = space::LG;
const FOOTER_PAD_Y: f32 = space::MD;
const FOOTER_GAP: f32 = space::SM;
const ACTION_MIN_WIDTH: f32 = 96.0;

/// The body inset every dialog in this family uses: `p-6` on the `<div>` each
/// of the three wraps its content in.
const BODY_PAD: f32 = space::XL;

/// A paragraph's line box. The three dialogs set their message `text-sm
/// leading-relaxed`, and Tailwind's `leading-relaxed` is `1.625`. The ratio
/// is kept over the body size (`text::BODY` = 13.5, the web's `text-sm`):
/// 13.5 × 1.625 = 21.94, ceiled the way `Font.Height` is — the same rounding
/// [`Role::line_height`] documents for `Title`.
pub const MESSAGE_LINE: f32 = 22.0;

/// The ring every focusable part of this family wears when it shows
/// (`:focus-visible`): `focus-visible:ring-2 ring-primary ring-offset-1` on
/// `Button.tsx`, on the toast's buttons and on the conflict rows. Two DIP of
/// ring one DIP off the part's edge.
const FOCUS_RING: f32 = 2.0;
const FOCUS_OFFSET: f32 = 1.0;


/// How far a floating surface's shadow reaches past its rectangle — what a
/// [`kubuno_desktop_controls::host::popup`] must include around the panel so the
/// shadow is not cut. [`SHADOW_MENU`]'s widest layer is `0 2px 6px 2px`:
/// 2 + 6 + 2 = 10 DIP below, 8 above; one number covers every side.
pub const SHADOW_PAD: f32 = 10.0;

/// `ConfirmDialog`'s glyph disc: `w-12 h-12 rounded-full` holding a `w-6 h-6`
/// icon, then `gap-4` before the message.
const CONFIRM_DISC: f32 = 48.0;
const CONFIRM_GLYPH: f32 = 24.0;
const CONFIRM_GAP: f32 = space::LG;

/// `ConflictDialog`: `gap-5` between the paragraph and the two option rows,
/// each `p-3 rounded-xl border`, whose icon box is `w-8 h-8 rounded-lg` around
/// a `size={15}` glyph, followed by `gap-3`. The `mt-0.5` under a row's title
/// is [`space::XXS`].
const CONFLICT_GAP: f32 = 20.0;
const OPTION_PAD: f32 = space::MD;
const OPTION_ICON_BOX: f32 = 32.0;
const OPTION_ICON_GLYPH: f32 = 15.0;
const OPTION_GAP: f32 = space::MD;

/// The toast card: `px-3 py-2.5`, `gap-2.5` between its three columns, a
/// `size={16}` variant glyph, a `size={14}` close cross in a `p-1` box, and
/// `gap-2` between two stacked cards. `bottom-4 right-4` is the anchor inset,
/// and `maxWidth: min(24rem, calc(100vw - 2rem))` the width — 24rem = 384.
const TOAST_PAD_X: f32 = space::MD;
const TOAST_PAD_Y: f32 = 10.0;
const TOAST_GAP: f32 = 10.0;
const TOAST_GLYPH: f32 = 16.0;
const TOAST_CLOSE_GLYPH: f32 = 14.0;
const TOAST_CLOSE_BOX: f32 = 22.0;
const TOAST_STACK_GAP: f32 = space::SM;
const TOAST_INSET: f32 = space::LG;
/// The inline action: `mt-1.5` above it, `px-1.5` inside it (and `-ml-1.5`,
/// the same six, pulling it back so its label lines up with the message).
const TOAST_ACTION_GAP: f32 = 6.0;
const TOAST_ACTION_PAD_X: f32 = 6.0;

/// `max = 4` on `<ToastProvider>` — past it the OLDEST is dropped, « the newest
/// message is the relevant one ».
pub const TOAST_MAX: usize = 4;

/// `duration ?? (variant === 'danger' ? 6000 : 4000)`, in milliseconds.
pub const TOAST_MS: u32 = 4000;
pub const TOAST_MS_DANGER: u32 = 6000;

/// The toast's own width, and the one a popover falls back to before it has
/// been measured (`p.offsetWidth || 232`, `p.offsetHeight || 300`).
pub const TOAST_WIDTH: f32 = 384.0;
pub const POPOVER_WIDTH: f32 = 232.0;
pub const POPOVER_HEIGHT: f32 = 300.0;

/// `AnchoredPopover`'s `gap = 4` — how far the panel sits off its anchor. Its
/// `M = 8` viewport keep-off is the SAME number as the tooltip's
/// [`crate::display::Tooltip::MARGIN`], and is taken from there rather than
/// restated.
pub const POPOVER_GAP: f32 = space::XS;

// ─────────────────────────────────────────────────────────────────────────────
// Text — the one thing the canvas cannot do for us.
// ─────────────────────────────────────────────────────────────────────────────

/// Breaks `text` into the lines that fit in `max_width`.
///
/// [`Canvas`] publishes fills, strokes, single-run text and named geometries —
/// **no wrapped layout**. A dialog's height is its message's height, so the
/// wrap has to happen somewhere; putting it here, as a pure function over a
/// measuring closure, is what lets « one line », « several lines » and « a very
/// long title » be unit tests rather than screenshots.
///
/// The rules are the browser's, restricted to what this family needs:
///
/// * an explicit `\n` always breaks — `ConfirmDialog`'s message is
///   `whitespace-pre-line`, so a caller's line breaks are content (a `\r`
///   before it, from a Windows clipboard, is dropped);
/// * words are greedy-fitted;
/// * a hyphen inside a word is a break opportunity, as in every browser
///   (UAX #14 classes `HY`/`BA`): « Compte-rendu » may end one line with
///   « Compte- » and start the next with « rendu »;
/// * a word wider than `max_width` on a line of its own is broken — at the
///   last separator a file name or a path is made of ([`SOFT_BREAKS`]) when
///   one keeps at least half the line, otherwise between two characters.
///   That is CSS `overflow-wrap: anywhere`, which is what a dialog quoting a
///   file name needs: the audit caught « Compte-rendu-comité-de-pilotage-…
///   -v4-final.docx » running past the dialog's right edge onto the veil,
///   because the previous rule (`overflow-wrap: normal`) let an over-long
///   word keep its line whatever its width.
///
/// Every returned line therefore fits in `max_width`, except a single
/// character wider than the box, which still takes a line so the wrap always
/// progresses.
pub fn wrap(text: &str, max_width: f32, width_of: &dyn Fn(&str) -> f32) -> Vec<String> {
    let mut out = Vec::new();
    if text.is_empty() {
        return out;
    }
    for paragraph in text.split('\n') {
        let paragraph = paragraph.strip_suffix('\r').unwrap_or(paragraph);
        let mut line = String::new();
        for word in paragraph.split_whitespace() {
            let candidate = if line.is_empty() { word.to_string() } else { format!("{line} {word}") };
            if width_of(&candidate) <= max_width {
                line = candidate;
                continue;
            }
            // The word does not fit after what the line already holds. Its
            // head up to a hyphen may still fit there; the rest (or the
            // whole word) starts a fresh line.
            let mut rest = word;
            if !line.is_empty() {
                if let Some((head, tail)) = hyphen_split(&line, word, max_width, width_of) {
                    line.push(' ');
                    line.push_str(head);
                    rest = tail;
                }
                out.push(std::mem::take(&mut line));
            }
            let mut pieces = break_word(rest, max_width, width_of);
            line = pieces.pop().unwrap_or_default();
            out.extend(pieces);
        }
        // An empty source line is a blank line the caller asked for, and it
        // still costs one line box — dropping it would close the gap a
        // `\n\n` message deliberately opens.
        out.push(line);
    }
    out
}

/// Where an over-long word prefers to break before falling back to a break
/// between any two characters: the separators file names, paths and
/// identifiers are made of. The character stays at the END of its line, as a
/// browser leaves a hyphen.
pub const SOFT_BREAKS: [char; 6] = ['-', '_', '.', '/', '\\', '–'];

/// The longest head of `word`, ending on a hyphen, that still fits after
/// `line` and a space — `None` when no hyphen does (or the hyphen is the
/// word's last character, which is no break).
fn hyphen_split<'a>(
    line: &str,
    word: &'a str,
    max_width: f32,
    width_of: &dyn Fn(&str) -> f32,
) -> Option<(&'a str, &'a str)> {
    word.char_indices()
        .filter(|&(_, ch)| ch == '-')
        .map(|(i, ch)| i + ch.len_utf8())
        .filter(|&end| end < word.len())
        .rev()
        .find(|&end| width_of(&format!("{line} {}", &word[..end])) <= max_width)
        .map(|end| (&word[..end], &word[end..]))
}

/// Cuts `word` into pieces that each fit in `max_width` — the last piece is
/// the remainder, which fits too and which the caller keeps open for the next
/// word. A word that already fits comes back whole.
///
/// Each cut is the longest fitting prefix, pulled back to the last
/// [`SOFT_BREAKS`] separator inside it when that separator keeps at least half
/// of the prefix (a break two characters in would leave a ragged stub of a
/// line). A prefix always holds at least one character, so the loop ends even
/// when `max_width` is narrower than a glyph.
fn break_word(word: &str, max_width: f32, width_of: &dyn Fn(&str) -> f32) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = word;
    // A last glyph wider than the box is cut « whole », which leaves nothing
    // behind: the loop then stops rather than pushing an empty remainder.
    while rest.chars().nth(1).is_some() && width_of(rest) > max_width {
        let mut fit = 0usize;
        let mut soft = None;
        for (i, ch) in rest.char_indices() {
            let end = i + ch.len_utf8();
            if width_of(&rest[..end]) > max_width {
                break;
            }
            fit = end;
            if SOFT_BREAKS.contains(&ch) && end < rest.len() {
                soft = Some(end);
            }
        }
        let cut = match soft {
            Some(s) if s * 2 >= fit => s,
            _ if fit > 0 => fit,
            // Not even one character fits: it takes a line of its own.
            _ => rest.chars().next().map_or(rest.len(), char::len_utf8),
        };
        out.push(rest[..cut].to_string());
        rest = &rest[cut..];
    }
    out.push(rest.to_string());
    out
}

/// [`wrap`] against the live font, in one text format.
fn wrap_on(canvas: &dyn Canvas, text: &str, max_width: f32, format: &IDWriteTextFormat) -> Vec<String> {
    wrap(text, max_width, &|s: &str| canvas.measure(s, format))
}

/// Paints already-wrapped lines inside `at` — whose `left`/`right` bound them
/// and whose `top` is the first line's — and answers how tall they were.
///
/// `at.bottom` is ignored: a paragraph is as tall as its lines, and passing a
/// rectangle rather than three floats is what keeps this under the argument
/// count the lint allows.
fn paint_lines(
    canvas: &dyn Canvas,
    lines: &[String],
    at: Rect,
    line_h: f32,
    format: &IDWriteTextFormat,
    colour: &D2D1_COLOR_F,
) -> f32 {
    for (i, line) in lines.iter().enumerate() {
        let y = at.top + i as f32 * line_h;
        canvas.text(line, &Rect::new(at.left, y, at.right, y + line_h), format, colour, false);
    }
    lines.len() as f32 * line_h
}

/// The `ring-2 ring-offset-1` ring around a focusable part, `corner` being the
/// part's own radius. `Canvas::stroke_rounded_w` strokes INSIDE its rectangle,
/// so the rectangle is grown by `offset + ring`: the ring covers 1 to 3 DIP
/// outside the part's edge and leaves the 1 DIP offset gap, as on the web.
fn paint_focus_ring(canvas: &dyn Canvas, rect: Rect, corner: f32, colour: &D2D1_COLOR_F) {
    let out = FOCUS_OFFSET + FOCUS_RING;
    canvas.stroke_rounded_w(&rect.inflate(out, out), corner + out, colour, FOCUS_RING);
}

// ─────────────────────────────────────────────────────────────────────────────
// Placement — pure, so a host smaller than the dialog is a test.
// ─────────────────────────────────────────────────────────────────────────────

/// Where a dialog of `size` goes inside `host`.
///
/// `FloatingWindow` positions itself with `left: 50%; top: 33%` and
/// `transform: translate(-50%, -33%)`, clamped to `calc(100vw - 16px)` /
/// `calc(100vh - 16px)`. That is: centred horizontally, a THIRD of the way
/// down (not half — a dialog sits above the optical centre), and never closer
/// than [`HOST_MARGIN`] to an edge.
///
/// A host smaller than the dialog is the interesting case, and it is why the
/// clamp shrinks the rectangle before it moves it: a window pinned to the top
/// margin but still 240 tall in a 200 tall host would hang out of the bottom,
/// which is exactly what the CSS `maxHeight` prevents.
pub fn place(host: Rect, size: Size) -> Rect {
    let avail_w = (host.right - host.left - 2.0 * HOST_MARGIN).max(0.0);
    let avail_h = (host.bottom - host.top - 2.0 * HOST_MARGIN).max(0.0);
    let w = size.width.min(avail_w);
    let h = size.height.min(avail_h);

    let centre_x = host.left + (host.right - host.left - w) / 2.0;
    let third_y = host.top + TOP_FRACTION * (host.bottom - host.top) - TOP_FRACTION * h;

    let left = clamp_into(centre_x, host.left, host.right, w);
    let top = clamp_into(third_y, host.top, host.bottom, h);
    Rect::new(left, top, left + w, top + h)
}

/// Keeps a `extent`-long box inside `[lo, hi]`, [`HOST_MARGIN`] off both ends.
/// The `max` guards a span too short to hold the margins, where the two clamps
/// would otherwise cross.
fn clamp_into(v: f32, lo: f32, hi: f32, extent: f32) -> f32 {
    let low = lo + HOST_MARGIN;
    let high = (hi - HOST_MARGIN - extent).max(low);
    v.clamp(low, high)
}

// ─────────────────────────────────────────────────────────────────────────────
// The action bar
// ─────────────────────────────────────────────────────────────────────────────

/// Which of the window's own buttons a point landed on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ActionId {
    /// The thing the window is for. Painted FIRST, i.e. to the left.
    Confirm,
    /// The way out. Always the rightmost button.
    Cancel,
}

/// One button of the window's footer — `WindowAction` in `FloatingWindow.tsx`.
///
/// `loading` is deliberately **not** ported: the web spins a spinner inside the
/// button, and neither [`crate::buttons::Button`] nor this family owns one (the
/// spinner belongs to `feedback`). A caller that needs it disables the action
/// and says so in the label, which is what the desktop shell does today.
#[derive(Debug, Clone)]
pub struct Action {
    pub label: String,
    /// Destructive: the label turns `danger`. Still a text button, never a
    /// filled one — the footer's own comment insists on it.
    pub danger: bool,
    pub enabled: bool,
}

impl Action {
    pub fn new(label: impl Into<String>) -> Self {
        Self { label: label.into(), danger: false, enabled: true }
    }

    pub fn danger(mut self, danger: bool) -> Self {
        self.danger = danger;
        self
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// The button that paints it: `variant={danger ? 'textDanger' : 'text'}`
    /// for the confirming action, `variant="ghost"` for the cancel — the two
    /// lines of `FloatingWindow.tsx`'s footer, at the `md` size every dialog
    /// button in the product is.
    fn button(&self, id: ActionId) -> Button {
        let variant = match (id, self.danger) {
            (ActionId::Confirm, true) => Variant::TextDanger,
            (ActionId::Confirm, false) => Variant::Text,
            (ActionId::Cancel, _) => Variant::Ghost,
        };
        let mut b = Button::new(&self.label).variant(variant).size(ButtonSize::Md);
        b.enabled = self.enabled;
        b
    }
}

/// What a window puts in its footer — `WindowActions`.
///
/// **The order lives here and nowhere else**: the action on the LEFT, the
/// cancel on the RIGHT, both at least [`ACTION_MIN_WIDTH`] wide so the pair
/// never jitters from one dialog to the next. That is the project's rule (the
/// one Word uses), and the whole point of stating it once is that no dialog
/// gets to have an opinion about it.
///
/// `None` for both means **no footer at all** — a tool panel confirms nothing
/// and must not grow a bar with a lonely « Fermer » in it.
#[derive(Debug, Clone, Default)]
pub struct Actions {
    pub confirm: Option<Action>,
    pub cancel: Option<Action>,
}

impl Actions {
    /// The pair every confirmation has.
    pub fn pair(confirm: impl Into<String>, cancel: impl Into<String>) -> Self {
        Self { confirm: Some(Action::new(confirm)), cancel: Some(Action::new(cancel)) }
    }

    /// A single way out — `actions={{ cancel: … }}`.
    pub fn only_cancel(cancel: impl Into<String>) -> Self {
        Self { confirm: None, cancel: Some(Action::new(cancel)) }
    }

    pub fn is_empty(&self) -> bool {
        self.confirm.is_none() && self.cancel.is_none()
    }

    /// The actions, in **paint order** — confirm first, then cancel.
    fn list(&self) -> Vec<(ActionId, &Action)> {
        let mut out = Vec::new();
        if let Some(a) = self.confirm.as_ref() {
            out.push((ActionId::Confirm, a));
        }
        if let Some(a) = self.cancel.as_ref() {
            out.push((ActionId::Cancel, a));
        }
        out
    }
}

/// Lays the action buttons out inside `footer`, right-aligned, given each
/// button's intrinsic width.
///
/// Pure — the widths come from [`crate::buttons::Button::width_of`], which a
/// test can feed without a live font. The group hangs off the RIGHT edge
/// (`ms-auto`), so it is built from the last button backwards; the returned
/// rectangles are in the input's order, which is the paint order.
pub fn action_rects(footer: Rect, widths: &[f32]) -> Vec<Rect> {
    let top = footer.top + FOOTER_PAD_Y;
    let bottom = top + height::BUTTON_MD;
    let mut right = footer.right - FOOTER_PAD_X;
    let mut out = vec![Rect::new(0.0, 0.0, 0.0, 0.0); widths.len()];
    for (i, w) in widths.iter().enumerate().rev() {
        let w = w.max(ACTION_MIN_WIDTH);
        out[i] = Rect::new(right - w, top, right, bottom);
        right -= w + FOOTER_GAP;
    }
    out
}

// ─────────────────────────────────────────────────────────────────────────────
// The keyboard — pure, so the focus trap and Entrée / Échap are unit tests.
// ─────────────────────────────────────────────────────────────────────────────

/// A focusable part of a dialog — what the keyboard can land on.
///
/// The web's dialogs are DOM trees whose tab order is document order: the ✕
/// in the title band, then the body's controls, then the footer's buttons.
/// [`FloatingWindow::focus_order`] (and each dialog's own) lists them in that
/// order, which is also the paint order a [`crate::focus::FocusRing`] walks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DialogPart {
    /// The ✕ at the end of the title band (`title="Fermer (Échap)"`).
    Close,
    /// A footer button.
    Action(ActionId),
    /// [`PromptDialog`]'s input.
    Field,
    /// One of [`ConflictDialog`]'s two option rows, by index.
    Option(usize),
}

impl DialogPart {
    /// A stable [`FocusId`] for the part, for a caller that drives the focus
    /// with a [`crate::focus::FocusRing`]. Unique across the parts of ONE
    /// dialog; a page showing two dialogs at once (it should not) would have
    /// to namespace them itself.
    pub fn focus_id(self) -> FocusId {
        match self {
            DialogPart::Close => FocusId::of("kb.dialog.close"),
            DialogPart::Action(ActionId::Confirm) => FocusId::of("kb.dialog.confirm"),
            DialogPart::Action(ActionId::Cancel) => FocusId::of("kb.dialog.cancel"),
            DialogPart::Field => FocusId::of("kb.dialog.field"),
            DialogPart::Option(i) => FocusId::indexed("kb.dialog.option", i),
        }
    }

    /// The part whose [`DialogPart::focus_id`] is `id`, among `order`.
    pub fn from_focus_id(id: FocusId, order: &[DialogPart]) -> Option<DialogPart> {
        order.iter().copied().find(|p| p.focus_id() == id)
    }

    /// Whether the part is a button — something Espace and Entrée press.
    pub fn is_button(self) -> bool {
        !matches!(self, DialogPart::Field)
    }
}

/// The keys a dialog answers, already decoded from the host's virtual keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogKey {
    Escape,
    Enter,
    /// Maj+Entrée — a new line in a multiline prompt, Entrée everywhere else.
    ShiftEnter,
    Space,
    Tab,
    ShiftTab,
}

impl DialogKey {
    /// Decodes a key-down. Chords the dialog does not own (Ctrl+Entrée,
    /// Alt+Tab…) are `None`, so they stay in the queue for someone else.
    pub fn from_vk(key: u16, mods: Modifiers) -> Option<Self> {
        match (key, mods.ctrl || mods.alt, mods.shift) {
            // Escape closes whatever the modifiers: the web's listener tests
            // `e.key` only.
            (vk::ESCAPE, _, _) => Some(DialogKey::Escape),
            (_, true, _) => None,
            (vk::ENTER, false, false) => Some(DialogKey::Enter),
            (vk::ENTER, false, true) => Some(DialogKey::ShiftEnter),
            (vk::SPACE, false, false) => Some(DialogKey::Space),
            (vk::TAB, false, false) => Some(DialogKey::Tab),
            (vk::TAB, false, true) => Some(DialogKey::ShiftTab),
            _ => None,
        }
    }
}

/// What a key asks the dialog's owner to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogCommand {
    /// Close without choosing — Échap, which `FloatingWindow` wires to
    /// `onClose` (the cancel).
    Dismiss,
    /// Press a part: a footer action, the ✕, a conflict row.
    Activate(DialogPart),
    /// Move the keyboard focus there (the trap's Tab / Maj+Tab).
    Focus(DialogPart),
}

/// The focus trap: the part after (or before) `current` in `order`, wrapping
/// at both ends so Tab never leaves the dialog — the WAI-ARIA dialog pattern
/// (« Tab: moves focus to the next tabbable element inside the dialog; if
/// focus is on the last one, moves focus to the first »). From no focus,
/// forward lands on the first part and backward on the last, as a browser's
/// Tab from the document does. `None` only for an empty order.
pub fn cycle_focus(order: &[DialogPart], current: Option<DialogPart>, forward: bool) -> Option<DialogPart> {
    let n = order.len();
    if n == 0 {
        return None;
    }
    let at = current.and_then(|c| order.iter().position(|p| *p == c));
    let next = match (at, forward) {
        (Some(i), true) => (i + 1) % n,
        (Some(i), false) => (i + n - 1) % n,
        (None, true) => 0,
        (None, false) => n - 1,
    };
    order.get(next).copied()
}

/// Resolves a key against a dialog's focus — shared by every dialog here.
///
/// * **Échap** dismisses (`FloatingWindow`'s capture-phase listener).
/// * **Tab / Maj+Tab** cycle inside `order` ([`cycle_focus`]).
/// * **Espace** presses the focused button (a native `<button>`).
/// * **Entrée** presses the focused button when that button is not the
///   confirming action — the ✕, « Annuler », a conflict row: a browser clicks
///   the focused button, and cancelling on Entrée after tabbing to
///   « Annuler » is what a keyboard user means. Anywhere else (the field, the
///   confirming action, nothing) it confirms: `ConfirmDialog`'s window-level
///   `keydown` → `onConfirm`, `PromptDialog`'s `onKeyDown` → `submit()`. A
///   disabled confirming action answers nothing — `submit` checks
///   `canConfirm` first.
///
/// `confirm_enabled` is `None` when the dialog has no confirming action.
pub fn resolve_key(
    key: DialogKey,
    focus: Option<DialogPart>,
    order: &[DialogPart],
    confirm_enabled: Option<bool>,
) -> Option<DialogCommand> {
    let confirm = || match confirm_enabled {
        Some(true) => Some(DialogCommand::Activate(DialogPart::Action(ActionId::Confirm))),
        _ => None,
    };
    match key {
        DialogKey::Escape => Some(DialogCommand::Dismiss),
        DialogKey::Tab => cycle_focus(order, focus, true).map(DialogCommand::Focus),
        DialogKey::ShiftTab => cycle_focus(order, focus, false).map(DialogCommand::Focus),
        DialogKey::Space => match focus {
            Some(DialogPart::Action(ActionId::Confirm)) => confirm(),
            Some(p) if p.is_button() => Some(DialogCommand::Activate(p)),
            _ => None,
        },
        DialogKey::Enter | DialogKey::ShiftEnter => match focus {
            Some(DialogPart::Action(ActionId::Confirm)) | Some(DialogPart::Field) | None => confirm(),
            Some(p) => Some(DialogCommand::Activate(p)),
        },
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// FloatingWindow — the surface all three dialogs are built on.
// ═════════════════════════════════════════════════════════════════════════════

/// A floating window: an accent title band, a body, and the footer the whole
/// product shares.
///
/// It owns a [`Panel`] — the body — so `padding`, `border_style`, `dock`,
/// `anchor`, `minimum_size` and the child list are that family's, reached
/// through [`Deref`]. The three bands are laid out by the dock engine
/// ([`Panel::layout_children`]), not by arithmetic written here.
///
/// ```ignore
/// let mut w = FloatingWindow::new("Renommer");
/// w.actions = Actions::pair("Renommer", "Annuler");
/// w.content_height = 96.0;                       // what the body needs
/// let rect = dialogs::place(host, w.measure(canvas, host_width));
/// w.paint_modal(canvas, host, rect, WidgetState::REST);
/// ```
///
/// ## What is not ported, and why
///
/// * **Dragging and resizing.** They are a *host* concern: the eight resize
///   strips and the pixel-mode switch in `FloatingWindow.tsx` move a DOM node,
///   and on the desktop the equivalent is the window manager moving an HWND.
///   The geometry this type publishes ([`FloatingWindow::titlebar_rect`]) is
///   what a host drags by.
/// * **Pop-out, portals, z-order, the tabbed-height lock.** All four are React
///   plumbing with no painted result.
/// * **`backdrop-blur-[1px]`.** [`Canvas`] has no blur; the scrim is the
///   `bg-black/30` half of it, which is the half that separates the dialog
///   from its host.
pub struct FloatingWindow {
    body: Panel,
    /// The band's caption.
    pub title: String,
    /// An optional glyph before it, by name from `assets/lucide-icons.txt`.
    pub icon: Option<&'static str>,
    /// The footer. Empty means no footer band at all.
    pub actions: Actions,
    /// `backdrop`: a modal window veils what it covers, and a click on the veil
    /// closes it (`onClick={onClose}` on the backdrop div).
    pub backdrop: bool,
    /// Whether the pointer is on the ✕.
    ///
    /// A [`WidgetState`] describes ONE widget and a window paints several
    /// targets, so the caller that tracks hover says which — the convention
    /// `Panel::paint_children` documents (« the pointer is over ONE child and
    /// the container does not know which »).
    pub close_hot: bool,
    /// `defaultWidth`.
    pub width: f32,
    /// What the body asks for, in DIP. A dialog built on this type sets it from
    /// its own measured content; a caller hosting free content sets it itself.
    pub content_height: f32,
    /// The footer button under the pointer, which paints its hover — the same
    /// « the caller says which » convention as [`FloatingWindow::close_hot`].
    pub hot_action: Option<ActionId>,
    /// The footer button the pointer is held down on (`:active`).
    pub pressed_action: Option<ActionId>,
    /// The part holding the keyboard focus. Set on open to
    /// [`FloatingWindow::initial_focus`] (the web's `autoFocus` / `el.focus()`)
    /// and moved by [`resolve_key`]'s Tab, or by a focus manager.
    pub focus: Option<DialogPart>,
    /// Whether the focus ring SHOWS (`:focus-visible`): true once the keyboard
    /// moved the focus, false for the programmatic `autoFocus` of a dialog
    /// opened by a click — a browser does not ring a button focused that way.
    /// A text field shows its focus whatever this says.
    pub focus_visible: bool,
    /// The radius of the window's corners, in DIP (8 by default, a desktop window's; 0 is square).
    pub corner_radius: f32,
}

impl FloatingWindow {
    /// Its corners' radius, in DIP (0: square).
    pub fn with_corner_radius(mut self, radius: f32) -> Self {
        self.corner_radius = if radius.is_finite() { radius.max(0.0) } else { 0.0 };
        self
    }

    pub fn new(title: impl Into<String>) -> Self {
        Self {
            body: Panel::new(),
            title: title.into(),
            icon: None,
            actions: Actions::default(),
            backdrop: false,
            close_hot: false,
            width: WIDTH_DEFAULT,
            content_height: 0.0,
            hot_action: None,
            pressed_action: None,
            focus: None,
            focus_visible: false,
            corner_radius: WINDOW_RADIUS,
        }
    }

    // ── Keyboard ─────────────────────────────────────────────────────────

    /// The tab order: the ✕, then the enabled footer actions (a disabled
    /// `<button>` is skipped by Tab). A window with free body content lists
    /// only its own chrome — the body's controls belong to its caller.
    pub fn focus_order(&self) -> Vec<DialogPart> {
        let mut out = vec![DialogPart::Close];
        out.extend(
            self.actions
                .list()
                .into_iter()
                .filter(|(_, a)| a.enabled)
                .map(|(id, _)| DialogPart::Action(id)),
        );
        out
    }

    /// Where the focus goes on open: the confirming action (`autoFocus: true`
    /// on `ConfirmDialog`'s confirm), else the first part after the ✕, else
    /// the ✕ — WAI-ARIA's « focus the first focusable element », with the ✕
    /// passed over because landing on « close » first is how a keyboard user
    /// loses a dialog by pressing Entrée.
    pub fn initial_focus(&self) -> Option<DialogPart> {
        let order = self.focus_order();
        order.iter().copied().find(|p| *p != DialogPart::Close).or(order.first().copied())
    }

    /// Resolves a key against this window's focus — see [`resolve_key`].
    pub fn key_command(&self, key: DialogKey) -> Option<DialogCommand> {
        let confirm = self.actions.confirm.as_ref().map(|a| a.enabled);
        resolve_key(key, self.focus, &self.focus_order(), confirm)
    }

    /// Moves the focus, and says whether it shows.
    pub fn set_focus(&mut self, part: Option<DialogPart>, visible: bool) {
        self.focus = part;
        self.focus_visible = visible;
    }

    /// The state the part paints in: focused / focus-visible, from
    /// [`FloatingWindow::focus`].
    fn focus_state(&self, part: DialogPart) -> WidgetState {
        let on = self.focus == Some(part);
        WidgetState::REST.focused(on).focus_visible(on && self.focus_visible)
    }

    /// **CANVAS space.** Where a part of the window's own chrome is — the ✕ or
    /// a footer action; `None` for a body part (a dialog built on this type
    /// answers those) or an action the window does not have.
    pub fn part_rect(&self, canvas: &dyn Canvas, bounds: Rect, part: DialogPart) -> Option<Rect> {
        match part {
            DialogPart::Close => Some(self.close_rect(bounds)),
            DialogPart::Action(id) => self
                .actions
                .list()
                .into_iter()
                .zip(self.action_rects(canvas, bounds))
                .find(|((a, _), _)| *a == id)
                .map(|(_, r)| r),
            DialogPart::Field | DialogPart::Option(_) => None,
        }
    }

    /// A modal window: it veils its host and dismisses on a click outside.
    pub fn modal(mut self) -> Self {
        self.backdrop = true;
        self
    }

    pub fn with_width(mut self, width: f32) -> Self {
        self.width = width;
        self
    }

    pub fn with_icon(mut self, icon: &'static str) -> Self {
        self.icon = Some(icon);
        self
    }

    pub fn with_actions(mut self, actions: Actions) -> Self {
        self.actions = actions;
        self
    }

    /// The body panel, for a caller that places its own children in it.
    pub fn body(&self) -> &Panel {
        &self.body
    }

    pub fn body_mut(&mut self) -> &mut Panel {
        &mut self.body
    }

    // ── Sizing ───────────────────────────────────────────────────────────

    /// The width this window takes when at most `max_width` is available:
    /// `defaultWidth`, floored by `minWidth` and clamped to the host.
    pub fn width_for(&self, max_width: f32) -> f32 {
        let room = (max_width - 2.0 * HOST_MARGIN).max(0.0);
        self.width.max(MIN_WIDTH).min(room)
    }

    /// The height of everything that is not the body: the title band, plus the
    /// footer when there is one.
    pub fn chrome_height(&self) -> f32 {
        kubuno_desktop_controls::window_chrome::TITLEBAR_HEIGHT + self.footer_height()
    }

    /// `py-3` around a `md` button — or nothing at all when the window has no
    /// actions.
    pub fn footer_height(&self) -> f32 {
        if self.actions.is_empty() {
            0.0
        } else {
            2.0 * FOOTER_PAD_Y + height::BUTTON_MD
        }
    }

    /// The window's size for a body `content` tall, honouring `minHeight`.
    pub fn size_for(&self, content: f32, max_width: f32) -> Size {
        Size::new(
            self.width_for(max_width),
            (self.chrome_height() + content).max(MIN_HEIGHT),
        )
    }

    /// See the module docs: content decides the height, the host decides the
    /// width.
    pub fn measure_at(&self, _canvas: &dyn Canvas, max_width: f32) -> Size {
        self.size_for(self.content_height, max_width)
    }

    // ── Geometry (all of it derived from the `bounds` ARGUMENT) ───────────

    /// The three bands — body, title, footer — from the dock engine.
    ///
    /// The `Fill` child is added FIRST on purpose: the engine walks the child
    /// list in reverse, so a `Fill` added last would be resolved first and take
    /// the whole rectangle, with the bands then overlapping it. That is the
    /// toolkit's rule, documented on [`Panel::fill`], not a quirk of this call.
    fn bands(&self, bounds: Rect) -> Vec<Rect> {
        let mut panel = Panel::new().fill().top(kubuno_desktop_controls::window_chrome::TITLEBAR_HEIGHT);
        let footer = self.footer_height();
        if footer > 0.0 {
            panel = panel.bottom(footer);
        }
        panel.layout_children(bounds)
    }

    /// **CANVAS space.** The accent band. A host drags the window by this.
    pub fn titlebar_rect(&self, bounds: Rect) -> Rect {
        self.bands(bounds).get(1).copied().unwrap_or(bounds)
    }

    /// **CANVAS space.** The content area between the band and the footer.
    pub fn body_rect(&self, bounds: Rect) -> Rect {
        self.bands(bounds).first().copied().unwrap_or(bounds)
    }

    /// **CANVAS space.** The action bar, or `None` when there is none.
    pub fn footer_rect(&self, bounds: Rect) -> Option<Rect> {
        if self.actions.is_empty() {
            return None;
        }
        self.bands(bounds).get(2).copied()
    }

    /// **CANVAS space.** The ✕ button, at the right end of the band.
    pub fn close_rect(&self, bounds: Rect) -> Rect {
        // Where the shared chrome lays it out (the standard 32 DIP band: a 24 DIP box, 8 from the edge).
        use kubuno_desktop_controls::window_chrome as wc;
        let band = self.titlebar_rect(bounds);
        let layout = wc::layout(&wc::ChromeStyle::default(), band, self.icon.is_some(), wc::SystemButtons::CLOSE_ONLY, wc::SlotWidths::default());
        // The band always has its close button (`CLOSE_ONLY`); an empty rectangle cannot be hit.
        layout.rect_of(wc::Part::Close).unwrap_or(Rect::new(band.right, band.top, band.right, band.top))
    }

    /// **CANVAS space.** One rectangle per action, in paint order (confirm
    /// then cancel), or empty when there is no footer.
    pub fn action_rects(&self, canvas: &dyn Canvas, bounds: Rect) -> Vec<Rect> {
        let Some(footer) = self.footer_rect(bounds) else {
            return Vec::new();
        };
        let widths: Vec<f32> =
            self.actions.list().iter().map(|(id, a)| a.button(*id).width(canvas)).collect();
        action_rects(footer, &widths)
    }

    /// Which action a canvas-space point landed on — `None` for a point on no
    /// button, or on a disabled one (a dead button does not answer).
    pub fn action_at(&self, canvas: &dyn Canvas, bounds: Rect, x: f32, y: f32) -> Option<ActionId> {
        let rects = self.action_rects(canvas, bounds);
        self.actions
            .list()
            .into_iter()
            .zip(rects)
            .find(|((_, a), r)| a.enabled && r.contains(x, y))
            .map(|((id, _), _)| id)
    }

    /// Whether the point is on the ✕.
    pub fn close_hit(&self, bounds: Rect, x: f32, y: f32) -> bool {
        self.close_rect(bounds).contains(x, y)
    }

    /// Whether a click at `(x, y)` on the **veil** dismisses this window.
    ///
    /// The web's backdrop is a full-host div with `onClick={onClose}` sitting
    /// one z-index under the window, so « outside the window, inside the host,
    /// and only when there IS a backdrop » is exactly its behaviour — a
    /// non-modal tool panel swallows nothing and closes on nothing.
    pub fn dismisses(&self, host: Rect, bounds: Rect, x: f32, y: f32) -> bool {
        self.backdrop && host.contains(x, y) && !bounds.contains(x, y)
    }

    // ── Painting ─────────────────────────────────────────────────────────

    /// The veil, then the window — what a modal caller draws.
    pub fn paint_modal(&self, canvas: &dyn Canvas, host: Rect, bounds: Rect, state: WidgetState) {
        if self.backdrop {
            canvas.fill_rounded(&host, 0.0, &canvas.theme().dialog_scrim);
        }
        self.paint(canvas, bounds, state);
    }

    /// The band: accent ground, optional glyph, title, ✕.
    fn paint_titlebar(&self, canvas: &dyn Canvas, bounds: Rect) {
        use kubuno_desktop_controls::window_chrome as wc;
        let t = canvas.theme();
        // The band every Kubuno window wears — top-level forms, dialogs, the designer's picture of a
        // form — painted by the one shared painter, so they cannot drift from this one.
        let style = wc::ChromeStyle::default();
        let window = Rect::new(bounds.left, bounds.top, bounds.right, bounds.top + kubuno_desktop_controls::window_chrome::TITLEBAR_HEIGHT);
        let layout = wc::layout(&style, window, self.icon.is_some(), wc::SystemButtons::CLOSE_ONLY, wc::SlotWidths::default());
        // Its top corners follow the window's.
        wc::paint_band_rounded(canvas, &style, &layout, self.corner_radius);
        let icon = self.icon.map_or(wc::ChromeIcon::None, wc::ChromeIcon::Glyph);
        let state = wc::ChromeState { hot: self.close_hot.then_some(wc::Part::Close), ..wc::ChromeState::default() };
        wc::paint_caption(canvas, &style, &layout, &self.title, icon, state);
        let ink = style.ink_color(t);
        let close = self.close_rect(bounds);
        if self.focus_state(DialogPart::Close).show_focus_ring() {
            // The ✕ has no ring class of its own on the web, so the browser's
            // `:focus-visible` outline shows; on the accent band it is drawn
            // in the band's ink, the only colour guaranteed to read there.
            // Inside the box, so it cannot touch the band's edge.
            let inset = FOCUS_RING / 2.0;
            canvas.stroke_rounded_w(&close.inflate(-inset, -inset), wc::TOOL_BUTTON_RADIUS, &ink, FOCUS_RING);
        }
    }

    /// The footer: the hairline that closes the body, then the buttons.
    fn paint_footer(&self, canvas: &dyn Canvas, bounds: Rect, state: WidgetState) {
        let Some(footer) = self.footer_rect(bounds) else {
            return;
        };
        let t = canvas.theme();
        // `border-top: 1px solid var(--color-border)` on `.kb-window-footer`.
        let rule = Rect::new(footer.left, footer.top, footer.right, footer.top + 1.0);
        canvas.fill_rounded(&rule, 0.0, &t.card_stroke);

        for ((id, action), rect) in
            self.actions.list().into_iter().zip(self.action_rects(canvas, bounds))
        {
            let live = action.enabled && !state.disabled;
            let st = self
                .focus_state(DialogPart::Action(id))
                .hot(live && self.hot_action == Some(id))
                .pressed(live && self.pressed_action == Some(id))
                .disabled(state.disabled);
            // `Button.tsx`'s `focus-visible:ring-2 ring-primary ring-offset-1`
            // is painted by the button itself from `st.show_focus_ring()`, so
            // the footer adds no ring of its own (it would double it).
            action.button(id).paint(canvas, rect, st);
        }
    }
}

impl Deref for FloatingWindow {
    type Target = Panel;
    fn deref(&self) -> &Panel {
        &self.body
    }
}

impl DerefMut for FloatingWindow {
    fn deref_mut(&mut self) -> &mut Panel {
        &mut self.body
    }
}

impl Widget for FloatingWindow {
    fn model(&self) -> &dyn Control {
        self.body.model()
    }

    fn measure(&self, canvas: &dyn Canvas) -> Size {
        self.measure_at(canvas, f32::INFINITY)
    }

    fn paint(&self, canvas: &dyn Canvas, bounds: Rect, state: WidgetState) {
        let t = canvas.theme();
        let radius = self.corner_radius;
        canvas.draw_shadow(&bounds, radius, &SHADOW_WINDOW, SHADOW_GREY);
        // `--kb-window-surface` / `--kb-window-content`, both `#ffffff`: one
        // opaque sheet, the frosted frame having been removed on 2026-08-09.
        canvas.fill_rounded(&bounds, radius, &t.layer_background);
        // Nothing the window holds paints past its rounded corners.
        canvas.push_clip_rounded(&bounds, radius);
        self.paint_titlebar(canvas, bounds);
        // The body is `overflow-auto` between the band and the footer: its
        // content never paints over either, even when `place` shrank the
        // window below what the content asked for.
        let body = self.body_rect(bounds);
        canvas.push_clip(&body);
        self.body.paint(canvas, body, state);
        canvas.pop_clip();
        self.paint_footer(canvas, bounds, state);
        canvas.pop_clip_rounded();
    }

    fn type_name(&self) -> &'static str {
        "FloatingWindow"
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// ConfirmDialog
// ═════════════════════════════════════════════════════════════════════════════

/// How loud a confirmation is — `ConfirmVariant` in `ConfirmDialog.tsx`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ConfirmVariant {
    #[default]
    Default,
    Warning,
    /// Destructive: a `Trash2` in a red disc, and a red confirming action.
    Danger,
}

/// Every variant, for the gallery and the tests.
pub const CONFIRM_VARIANTS: [ConfirmVariant; 3] =
    [ConfirmVariant::Default, ConfirmVariant::Warning, ConfirmVariant::Danger];

impl ConfirmVariant {
    /// `variant === 'danger' ? <Trash2/> : <AlertTriangle/>`.
    pub fn icon(self) -> &'static str {
        match self {
            ConfirmVariant::Danger => "Trash2",
            _ => "AlertTriangle",
        }
    }

    /// The disc and its glyph.
    ///
    /// The web writes these as raw Tailwind palette steps
    /// (`bg-red-100 text-red-600`, `bg-amber-100 text-amber-600`,
    /// `bg-gray-100 text-gray-600`) rather than as design tokens, which is a
    /// slip in the component and not a licence to hard-code a colour here: the
    /// three pairs are read as their token equivalents — `danger_light` /
    /// `danger`, `warning_light` / `caution` (the amber the palette already
    /// carries « for the conflict dialog's warning text »), and `surface_2` /
    /// `text_secondary`.
    fn ink(self, t: &kubuno_drive_desktop_app_controls::Theme) -> (D2D1_COLOR_F, D2D1_COLOR_F) {
        match self {
            ConfirmVariant::Danger => (t.danger_light, t.danger),
            ConfirmVariant::Warning => (t.warning_light, t.caution),
            ConfirmVariant::Default => (t.surface_2, t.text_secondary),
        }
    }
}

/// « Êtes-vous sûr ? » — the primitive the project's rules require instead of
/// `window.confirm`.
///
/// A [`FloatingWindow`] at `defaultWidth={380}` with a `backdrop`, whose body
/// is `p-6 flex flex-col gap-4`: a 48 DIP disc holding the variant's glyph,
/// then the message. The footer is the window's own — `ConfirmDialog.tsx`'s
/// own comment records the day it stopped drawing two filled buttons of its
/// own — so the confirming action is a TEXT button on the left and « Annuler »
/// a ghost on the right.
pub struct ConfirmDialog {
    window: FloatingWindow,
    /// The body text. `whitespace-pre-line`, so `\n` is honoured.
    pub message: String,
    pub variant: ConfirmVariant,
}

impl ConfirmDialog {
    /// The web's defaults: « Confirmer » / « Annuler », `variant='default'`.
    pub fn new(title: impl Into<String>, message: impl Into<String>) -> Self {
        let mut window = FloatingWindow::new(title)
            .modal()
            .with_width(WIDTH_CONFIRM)
            .with_actions(Actions::pair("Confirmer", "Annuler"));
        // `autoFocus: true` on the confirming action: Entrée confirms at once,
        // and no ring shows until the keyboard moves the focus.
        window.focus = window.initial_focus();
        Self { window, message: message.into(), variant: ConfirmVariant::default() }
    }

    /// The tab order — the window's own: the ✕, the confirm, the cancel. The
    /// disc and the message are not focusable.
    pub fn focus_order(&self) -> Vec<DialogPart> {
        self.window.focus_order()
    }

    /// Resolves a key: Échap cancels, Entrée confirms, Tab cycles inside —
    /// see [`resolve_key`].
    pub fn key_command(&self, key: DialogKey) -> Option<DialogCommand> {
        self.window.key_command(key)
    }

    /// The destructive form: `Trash2`, a red disc and a red confirming action.
    pub fn danger(title: impl Into<String>, message: impl Into<String>) -> Self {
        let mut d = Self::new(title, message);
        d.set_variant(ConfirmVariant::Danger);
        d
    }

    pub fn warning(title: impl Into<String>, message: impl Into<String>) -> Self {
        let mut d = Self::new(title, message);
        d.set_variant(ConfirmVariant::Warning);
        d
    }

    /// `variant === 'danger'` also reddens the confirming action
    /// (`danger: variant === 'danger'`), which is why this is a setter rather
    /// than a public field: the two must not disagree.
    pub fn set_variant(&mut self, variant: ConfirmVariant) {
        self.variant = variant;
        if let Some(confirm) = self.window.actions.confirm.as_mut() {
            confirm.danger = variant == ConfirmVariant::Danger;
        }
    }

    /// `hideCancel` — a one-button information dialog.
    pub fn hide_cancel(mut self) -> Self {
        self.window.actions.cancel = None;
        self
    }

    /// Overrides the two labels (`confirmLabel` / `cancelLabel`).
    pub fn labels(mut self, confirm: impl Into<String>, cancel: impl Into<String>) -> Self {
        let danger = self.variant == ConfirmVariant::Danger;
        self.window.actions.confirm = Some(Action::new(confirm).danger(danger));
        if self.window.actions.cancel.is_some() {
            self.window.actions.cancel = Some(Action::new(cancel));
        }
        self
    }

    /// **CANVAS space.** The disc the glyph sits in.
    pub fn disc_rect(&self, bounds: Rect) -> Rect {
        let body = self.window.body_rect(bounds);
        let left = body.left + BODY_PAD;
        let top = body.top + BODY_PAD;
        Rect::new(left, top, left + CONFIRM_DISC, top + CONFIRM_DISC)
    }

    /// The message, wrapped to the body's content width.
    pub fn lines(&self, canvas: &dyn Canvas, width: f32) -> Vec<String> {
        wrap_on(canvas, &self.message, width - 2.0 * BODY_PAD, &canvas.formats().body)
    }

    /// What the body needs at a given window width: `p-6`, the disc, `gap-4`,
    /// the message, `p-6`.
    fn content_height(&self, canvas: &dyn Canvas, width: f32) -> f32 {
        let lines = self.lines(canvas, width).len() as f32;
        2.0 * BODY_PAD + CONFIRM_DISC + CONFIRM_GAP + lines * MESSAGE_LINE
    }

    /// See the module docs.
    pub fn measure_at(&self, canvas: &dyn Canvas, max_width: f32) -> Size {
        let w = self.window.width_for(max_width);
        self.window.size_for(self.content_height(canvas, w), max_width)
    }

    /// The veil, then the dialog.
    pub fn paint_modal(&self, canvas: &dyn Canvas, host: Rect, bounds: Rect, state: WidgetState) {
        if self.window.backdrop {
            canvas.fill_rounded(&host, 0.0, &canvas.theme().dialog_scrim);
        }
        self.paint(canvas, bounds, state);
    }
}

impl Deref for ConfirmDialog {
    type Target = FloatingWindow;
    fn deref(&self) -> &FloatingWindow {
        &self.window
    }
}

impl DerefMut for ConfirmDialog {
    fn deref_mut(&mut self) -> &mut FloatingWindow {
        &mut self.window
    }
}

impl Widget for ConfirmDialog {
    fn model(&self) -> &dyn Control {
        self.window.model()
    }

    fn measure(&self, canvas: &dyn Canvas) -> Size {
        self.measure_at(canvas, f32::INFINITY)
    }

    fn paint(&self, canvas: &dyn Canvas, bounds: Rect, state: WidgetState) {
        self.window.paint(canvas, bounds, state);

        let t = canvas.theme();
        let (disc, glyph) = self.variant.ink(t);
        let d = self.disc_rect(bounds);
        let body = self.window.body_rect(bounds);
        canvas.push_clip(&body);
        // `rounded-full` on a 48 box.
        canvas.fill_rounded(&d, CONFIRM_DISC / 2.0, &disc);
        canvas.vector_icon(self.variant.icon(), &d, CONFIRM_GLYPH, &glyph);

        let lines = self.lines(canvas, bounds.right - bounds.left);
        paint_lines(
            canvas,
            &lines,
            Rect::new(body.left + BODY_PAD, d.bottom + CONFIRM_GAP, body.right - BODY_PAD, 0.0),
            MESSAGE_LINE,
            &canvas.formats().body,
            &t.text_secondary,
        );
        canvas.pop_clip();
    }

    fn type_name(&self) -> &'static str {
        "ConfirmDialog"
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// PromptDialog
// ═════════════════════════════════════════════════════════════════════════════

/// « Comment voulez-vous l'appeler ? » — the primitive that replaces
/// `window.prompt`.
///
/// A [`FloatingWindow`] at `defaultWidth={400}`, body `p-6 flex flex-col
/// gap-4`: an optional message, then the field.
///
/// ## The field is a [`TextField`], not a second one
///
/// `f.set_text(…)`, `f.placeholder_text`, `f.select(…)`, `f.max_length`,
/// `f.multiline` are the replica's, through that family — including the caret
/// and selection painting, which is the shipping `edit_box`.
///
/// ## One deliberate difference from the reference
///
/// `PromptDialog.tsx` still draws **its own** pair of half-width filled buttons
/// inside its body, where `ConfirmDialog.tsx` has moved to the window's footer
/// and records why in a comment (« a confirmation is a dialog like any other »).
/// This port follows the rule, not the straggler: the two buttons are the
/// window's, so every dialog in the product has one footer. The behaviour is
/// kept exactly — `canConfirm = allowEmpty || value.trim() !== ''` disables the
/// confirming action, it does not hide it.
pub struct PromptDialog {
    window: FloatingWindow,
    /// The optional line above the field.
    pub message: String,
    /// The input. Public: a caller drives its text, selection and placeholder
    /// through the replica.
    pub field: TextField,
    /// `allowEmpty` — whether an empty value may be confirmed.
    pub allow_empty: bool,
    /// The fixed end of the selection, in characters — where a Maj+arrow or a
    /// drag started. The replica stores a selection as `start + length` and
    /// has no direction, so the end that moves is kept here.
    anchor: i32,
    /// The moving end (the caret), in characters.
    caret: i32,
}

/// One editing operation on [`PromptDialog`]'s input — what the keys of a
/// native `<input>` / `<textarea>` do, decoded by [`FieldEdit::from_key`].
///
/// The field is the replica's `TextBox`; these operations go through its own
/// `select` / `set_selected_text`, so `MaxLength` and the selection clamp are
/// its rules, not re-implemented ones.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldEdit {
    /// Typed or pasted text replaces the selection. A single-line field drops
    /// line breaks, as an `<input>` strips them from a paste.
    Insert(String),
    /// Retour arrière: the selection, else the character before the caret.
    Backspace,
    /// Ctrl+Retour arrière: the word before the caret.
    BackspaceWord,
    /// Suppr: the selection, else the character after the caret.
    Delete,
    /// ← / →, Maj to extend; Ctrl to jump a word.
    Left { extend: bool, word: bool },
    Right { extend: bool, word: bool },
    /// Origine / Fin, Maj to extend.
    Home { extend: bool },
    End { extend: bool },
    /// Ctrl+A.
    SelectAll,
}

impl FieldEdit {
    /// Decodes a key-down into an edit, or `None` for a key the field does not
    /// own (Entrée, Échap, Tab — the dialog's; Ctrl+C / X / V — the caller's,
    /// because the clipboard is the host's). Maj+Entrée in a multiline field
    /// is a line break.
    pub fn from_key(key: u16, mods: Modifiers, multiline: bool) -> Option<Self> {
        if mods.alt {
            return None;
        }
        let (ctrl, shift) = (mods.ctrl, mods.shift);
        Some(match key {
            vk::BACK if ctrl => FieldEdit::BackspaceWord,
            vk::BACK => FieldEdit::Backspace,
            vk::DELETE if !ctrl => FieldEdit::Delete,
            vk::LEFT => FieldEdit::Left { extend: shift, word: ctrl },
            vk::RIGHT => FieldEdit::Right { extend: shift, word: ctrl },
            vk::HOME => FieldEdit::Home { extend: shift },
            vk::END => FieldEdit::End { extend: shift },
            k if ctrl && !shift && k == vk::letter('a') => FieldEdit::SelectAll,
            vk::ENTER if multiline && shift && !ctrl => FieldEdit::Insert("\n".to_string()),
            _ => return None,
        })
    }
}

/// The character index of the word boundary before `at` in `text` — what
/// Ctrl+← and Ctrl+Retour arrière jump to: back over spaces, then over the
/// word.
fn word_start(text: &str, at: i32) -> i32 {
    let chars: Vec<char> = text.chars().collect();
    let mut i = (at.max(0) as usize).min(chars.len());
    while i > 0 && chars[i - 1].is_whitespace() {
        i -= 1;
    }
    while i > 0 && !chars[i - 1].is_whitespace() {
        i -= 1;
    }
    i as i32
}

/// The character index of the word boundary after `at`: over the word, then
/// over the spaces after it (Windows' Ctrl+→ lands on the next word's start).
fn word_end(text: &str, at: i32) -> i32 {
    let chars: Vec<char> = text.chars().collect();
    let mut i = (at.max(0) as usize).min(chars.len());
    while i < chars.len() && !chars[i].is_whitespace() {
        i += 1;
    }
    while i < chars.len() && chars[i].is_whitespace() {
        i += 1;
    }
    i as i32
}

impl PromptDialog {
    /// The web's defaults: « OK » / « Annuler », single line, empty rejected.
    pub fn new(title: impl Into<String>) -> Self {
        let mut window = FloatingWindow::new(title)
            .modal()
            .with_width(WIDTH_PROMPT)
            .with_actions(Actions::pair("OK", "Annuler"));
        // `el.focus()` on open: the field holds the focus from the start.
        window.focus = Some(DialogPart::Field);
        let mut p = Self {
            window,
            message: String::new(),
            field: TextField::new(),
            allow_empty: false,
            anchor: 0,
            caret: 0,
        };
        p.sync_actions();
        p
    }

    /// The line above the field (`message`).
    pub fn with_message(mut self, message: impl Into<String>) -> Self {
        self.message = message.into();
        self
    }

    /// `defaultValue`, with the selection the web makes on open (`el.select()`).
    pub fn with_value(mut self, value: &str) -> Self {
        self.field.set_text(value);
        let len = value.chars().count() as i32;
        self.field.select(0, len);
        self.anchor = 0;
        self.caret = len;
        self.sync_actions();
        self
    }

    // ── Keyboard ─────────────────────────────────────────────────────────

    /// The tab order: the ✕, the field, then the enabled actions — a
    /// disabled « OK » (empty value) is skipped, as a disabled `<button>` is.
    pub fn focus_order(&self) -> Vec<DialogPart> {
        let mut order = self.window.focus_order();
        order.insert(1.min(order.len()), DialogPart::Field);
        order
    }

    /// The field — `el.focus(); el.select()` on open.
    pub fn initial_focus(&self) -> Option<DialogPart> {
        Some(DialogPart::Field)
    }

    /// Resolves a key — see [`resolve_key`]. Entrée submits only when
    /// `canConfirm`; in a multiline prompt Maj+Entrée belongs to the field
    /// (`e.key === 'Enter' && !(multiline && e.shiftKey)`), so it answers
    /// nothing here and [`FieldEdit::from_key`] turns it into a line break.
    pub fn key_command(&self, key: DialogKey) -> Option<DialogCommand> {
        if key == DialogKey::ShiftEnter && self.field.multiline && self.window.focus == Some(DialogPart::Field) {
            return None;
        }
        let confirm = self.window.actions.confirm.as_ref().map(|_| self.can_confirm());
        resolve_key(key, self.window.focus, &self.focus_order(), confirm)
    }

    /// The caret (the selection's moving end), in characters.
    pub fn caret(&self) -> i32 {
        self.caret
    }

    /// The selected text — what Ctrl+C copies.
    pub fn selected_text(&self) -> String {
        self.field.selected_text()
    }

    /// Puts the caret at `at` (clamped into the text); `extend` keeps the
    /// anchor, so the selection runs from it to the caret — Maj+click, a drag.
    pub fn set_caret(&mut self, at: i32, extend: bool) {
        let n = self.field.text().chars().count() as i32;
        self.caret = at.clamp(0, n);
        if !extend {
            self.anchor = self.caret;
        }
        self.anchor = self.anchor.clamp(0, n);
        let (lo, hi) = (self.anchor.min(self.caret), self.anchor.max(self.caret));
        self.field.select(lo, hi - lo);
    }

    /// Applies one edit and answers whether the TEXT changed (a caret move is
    /// not a change). Keeps the confirming action in step with the value.
    pub fn edit(&mut self, e: FieldEdit) -> bool {
        let before = self.field.text().to_string();
        let has_selection = self.field.selection_length() > 0;
        let (lo, hi) = (self.anchor.min(self.caret), self.anchor.max(self.caret));
        match e {
            FieldEdit::Insert(s) => {
                let s = if self.field.multiline {
                    s.replace("\r\n", "\n").replace('\r', "\n")
                } else {
                    s.replace(['\r', '\n'], "")
                };
                self.field.select(lo, hi - lo);
                self.field.set_selected_text(&s);
                let at = self.field.selection_start();
                self.set_caret(at, false);
            }
            FieldEdit::Backspace | FieldEdit::BackspaceWord | FieldEdit::Delete => {
                if !has_selection {
                    let n = before.chars().count() as i32;
                    let (from, to) = match e {
                        FieldEdit::Backspace => ((self.caret - 1).max(0), self.caret),
                        FieldEdit::BackspaceWord => (word_start(&before, self.caret), self.caret),
                        _ => (self.caret, (self.caret + 1).min(n)),
                    };
                    self.field.select(from, to - from);
                } else {
                    self.field.select(lo, hi - lo);
                }
                self.field.set_selected_text("");
                let at = self.field.selection_start();
                self.set_caret(at, false);
            }
            FieldEdit::Left { extend, word } => {
                let at = if !extend && has_selection && !word {
                    lo
                } else if word {
                    word_start(&before, self.caret)
                } else {
                    self.caret - 1
                };
                self.set_caret(at, extend);
            }
            FieldEdit::Right { extend, word } => {
                let at = if !extend && has_selection && !word {
                    hi
                } else if word {
                    word_end(&before, self.caret)
                } else {
                    self.caret + 1
                };
                self.set_caret(at, extend);
            }
            FieldEdit::Home { extend } => self.set_caret(0, extend),
            FieldEdit::End { extend } => self.set_caret(i32::MAX, extend),
            FieldEdit::SelectAll => {
                self.anchor = 0;
                self.set_caret(i32::MAX, true);
            }
        }
        self.sync_actions();
        self.field.text() != before
    }

    /// The character index nearest to canvas-space `x` in the field painted in
    /// `bounds` — where a click puts the caret. Measured against the live font
    /// prefix by prefix; a single-line field whose text is scrolled (longer
    /// than the box) is measured from its unscrolled start, which is a known
    /// limit of this port (the scroll offset is the predecessor's, private to
    /// `edit_box`).
    pub fn caret_at(&self, canvas: &dyn Canvas, bounds: Rect, x: f32) -> i32 {
        let field = self.field_rect(canvas, bounds);
        let content = self.field.content(field);
        let text = self.field.display();
        let line = text.split('\n').next().unwrap_or("");
        let dx = x - content.left;
        let mut best = 0;
        let mut prev_w = 0.0;
        for (i, (b, ch)) in line.char_indices().enumerate() {
            let w = canvas.measure(&line[..b + ch.len_utf8()], &canvas.formats().body);
            if dx < (prev_w + w) / 2.0 {
                return i as i32;
            }
            prev_w = w;
            best = i as i32 + 1;
        }
        best
    }

    pub fn with_placeholder(mut self, placeholder: impl Into<String>) -> Self {
        self.field.placeholder_text = placeholder.into();
        self
    }

    /// `multiline` — a `rows={3}` text area instead of an input.
    pub fn multiline(mut self, on: bool) -> Self {
        self.field.multiline = on;
        self
    }

    pub fn allow_empty(mut self, on: bool) -> Self {
        self.allow_empty = on;
        self
    }

    /// `canConfirm = allowEmpty || value.trim() !== ''`.
    pub fn can_confirm(&self) -> bool {
        self.allow_empty || !self.field.text().trim().is_empty()
    }

    /// The field's height: `h-9` for one line, `rows={3}` when multiline —
    /// [`crate::text::field_height`], so the two families cannot disagree.
    pub fn field_height(&self) -> f32 {
        field_height(self.field.multiline, 3)
    }

    /// **CANVAS space.** Where the input goes.
    pub fn field_rect(&self, canvas: &dyn Canvas, bounds: Rect) -> Rect {
        let body = self.window.body_rect(bounds);
        let top = body.top + BODY_PAD + self.message_height(canvas, bounds.right - bounds.left);
        Rect::new(body.left + BODY_PAD, top, body.right - BODY_PAD, top + self.field_height())
    }

    /// The message, wrapped — empty when there is none.
    pub fn lines(&self, canvas: &dyn Canvas, width: f32) -> Vec<String> {
        if self.message.is_empty() {
            return Vec::new();
        }
        wrap_on(canvas, &self.message, width - 2.0 * BODY_PAD, &canvas.formats().body)
    }

    /// The message block, `gap-4` included — zero when there is no message, so
    /// the field sits directly under the padding.
    fn message_height(&self, canvas: &dyn Canvas, width: f32) -> f32 {
        let lines = self.lines(canvas, width).len() as f32;
        if lines == 0.0 {
            0.0
        } else {
            lines * MESSAGE_LINE + CONFIRM_GAP
        }
    }

    fn content_height(&self, canvas: &dyn Canvas, width: f32) -> f32 {
        2.0 * BODY_PAD + self.message_height(canvas, width) + self.field_height()
    }

    /// See the module docs.
    pub fn measure_at(&self, canvas: &dyn Canvas, max_width: f32) -> Size {
        let w = self.window.width_for(max_width);
        self.window.size_for(self.content_height(canvas, w), max_width)
    }

    /// Keeps the confirming action's `enabled` in step with [`can_confirm`].
    ///
    /// Called by [`Widget::paint`], and public because a caller that hit-tests
    /// before painting needs the same answer.
    ///
    /// [`can_confirm`]: PromptDialog::can_confirm
    pub fn sync_actions(&mut self) {
        let ok = self.can_confirm();
        if let Some(confirm) = self.window.actions.confirm.as_mut() {
            confirm.enabled = ok;
        }
    }

    /// The veil, then the dialog.
    pub fn paint_modal(&self, canvas: &dyn Canvas, host: Rect, bounds: Rect, state: WidgetState) {
        if self.window.backdrop {
            canvas.fill_rounded(&host, 0.0, &canvas.theme().dialog_scrim);
        }
        self.paint(canvas, bounds, state);
    }
}

impl Deref for PromptDialog {
    type Target = FloatingWindow;
    fn deref(&self) -> &FloatingWindow {
        &self.window
    }
}

impl DerefMut for PromptDialog {
    fn deref_mut(&mut self) -> &mut FloatingWindow {
        &mut self.window
    }
}

impl Widget for PromptDialog {
    fn model(&self) -> &dyn Control {
        self.window.model()
    }

    fn measure(&self, canvas: &dyn Canvas) -> Size {
        self.measure_at(canvas, f32::INFINITY)
    }

    fn paint(&self, canvas: &dyn Canvas, bounds: Rect, state: WidgetState) {
        self.window.paint(canvas, bounds, state);

        let body = self.window.body_rect(bounds);
        canvas.push_clip(&body);
        let lines = self.lines(canvas, bounds.right - bounds.left);
        paint_lines(
            canvas,
            &lines,
            Rect::new(body.left + BODY_PAD, body.top + BODY_PAD, body.right - BODY_PAD, 0.0),
            MESSAGE_LINE,
            &canvas.formats().body,
            &canvas.theme().text_secondary,
        );

        // The field carries the focus from the start: the web focuses and
        // selects it on open, which is what makes Entrée confirm without a
        // click. A text field shows its focus however it got it
        // (`FocusOpts::TEXT`), so `focus_visible` follows `focused`.
        let on = self.window.focus == Some(DialogPart::Field);
        let st = WidgetState { focused: on, focus_visible: on, disabled: state.disabled, ..WidgetState::REST };
        self.field.paint(canvas, self.field_rect(canvas, bounds), st);
        canvas.pop_clip();
    }

    fn type_name(&self) -> &'static str {
        "PromptDialog"
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// ConflictDialog
// ═════════════════════════════════════════════════════════════════════════════

/// What the user chose — `ConflictChoice`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictChoice {
    /// « Écraser » for a file, « Fusionner » for a folder.
    Overwrite,
    KeepBoth,
    Cancel,
}

/// What is in conflict — `type: 'file' | 'folder'`, which decides the first
/// option's label and wording.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ConflictKind {
    #[default]
    File,
    Folder,
}

/// « Un fichier nommé « … » existe déjà. » — the three-way choice Drive shows
/// on a name collision.
///
/// A [`FloatingWindow`] at `defaultWidth={400}` whose body is `p-6 gap-5`: the
/// sentence, then two option rows, each `p-3 rounded-xl border border-border`
/// with a 32 DIP tinted icon box, a title and a two-line explanation. The third
/// choice, « Annuler », is the window's own cancel action — the reference draws
/// it as a bare text button inside the body, which is the shape
/// `ConfirmDialog.tsx` moved away from, so the port puts it in the one footer
/// the product has.
pub struct ConflictDialog {
    window: FloatingWindow,
    /// The colliding name.
    pub name: String,
    pub kind: ConflictKind,
    /// Which option row the pointer is over, if any.
    ///
    /// A [`WidgetState`] describes ONE widget, and this dialog paints two
    /// targets: the caller that tracks hover says which, exactly as
    /// [`crate::views::ListView`] does with its own hot index.
    pub hot: Option<usize>,
}

impl ConflictDialog {
    pub fn new(name: impl Into<String>, kind: ConflictKind) -> Self {
        let window = FloatingWindow::new("Conflit de nom")
            .modal()
            .with_width(WIDTH_CONFLICT)
            .with_actions(Actions::only_cancel("Annuler"));
        let mut d = Self { window, name: name.into(), kind, hot: None };
        d.window.focus = d.initial_focus();
        d
    }

    /// The tab order: the ✕, the two option rows (they are `<button>`s), then
    /// « Annuler ».
    pub fn focus_order(&self) -> Vec<DialogPart> {
        let mut order = self.window.focus_order();
        let at = 1.min(order.len());
        order.insert(at, DialogPart::Option(1));
        order.insert(at, DialogPart::Option(0));
        order
    }

    /// « Annuler ». The web autofocuses nothing here; WAI-ARIA's advice for a
    /// dialog whose choices are destructive is to start on the least
    /// destructive one, and the first row overwrites a file — so a stray
    /// Entrée must not land on it.
    pub fn initial_focus(&self) -> Option<DialogPart> {
        Some(DialogPart::Action(ActionId::Cancel))
    }

    /// Resolves a key: Échap and Entrée on « Annuler » cancel, Entrée or
    /// Espace on a row chooses it, Tab cycles — see [`resolve_key`].
    pub fn key_command(&self, key: DialogKey) -> Option<DialogCommand> {
        resolve_key(key, self.window.focus, &self.focus_order(), None)
    }

    /// The choice a part stands for — a row's, or « Annuler » / the ✕.
    pub fn choice_of(&self, part: DialogPart) -> Option<ConflictChoice> {
        match part {
            DialogPart::Option(i) => self.options().get(i).map(|o| o.0),
            DialogPart::Close | DialogPart::Action(ActionId::Cancel) => Some(ConflictChoice::Cancel),
            _ => None,
        }
    }

    /// The two options, in order — `(choice, title, explanation, icon)`.
    ///
    /// The wording is the component's, verbatim, because it is what the product
    /// says today and a dialog that says something else is a different dialog.
    pub fn options(&self) -> [(ConflictChoice, String, String, &'static str); 2] {
        let folder = self.kind == ConflictKind::Folder;
        let first = if folder { "Fusionner" } else { "Écraser" };
        let explain = if folder {
            "Les deux dossiers seront fusionnés. Les fichiers en conflit seront remplacés."
                .to_string()
        } else {
            "Le fichier existant sera remplacé par le nouveau.".to_string()
        };
        [
            (ConflictChoice::Overwrite, first.to_string(), explain, "Layers"),
            (
                ConflictChoice::KeepBoth,
                "Conserver les deux".to_string(),
                format!(
                    "Le nouvel élément sera renommé automatiquement (ex. : « {} (2) »).",
                    self.name
                ),
                // The lucide `Copy` this row draws on the web is unreachable by
                // that name: `themed-icons.txt` is searched first and already
                // publishes a `Copy` (Material Symbols' `content_copy`), which
                // is also the copy glyph the shipping file explorer draws. Two
                // sections with one name would make the new one dead code, so
                // the existing geometry is used rather than shadowed.
                "Copy",
            ),
        ]
    }

    /// The sentence above the options.
    pub fn message(&self) -> String {
        let what = if self.kind == ConflictKind::Folder { "dossier" } else { "fichier" };
        format!("Un {what} nommé « {} » existe déjà à cet emplacement.", self.name)
    }

    /// The message, wrapped to the body's content width.
    pub fn lines(&self, canvas: &dyn Canvas, width: f32) -> Vec<String> {
        wrap_on(canvas, &self.message(), width - 2.0 * BODY_PAD, &canvas.formats().body)
    }

    /// An option row's explanation, wrapped to the width its icon column
    /// leaves.
    fn option_lines(&self, canvas: &dyn Canvas, width: f32, text: &str) -> Vec<String> {
        let inner = width - 2.0 * BODY_PAD - 2.0 * OPTION_PAD - OPTION_ICON_BOX - OPTION_GAP;
        wrap_on(canvas, text, inner, &canvas.formats().caption)
    }

    /// One row's height: `p-3` around whichever is taller — the icon box, or
    /// the title line plus `mt-0.5` plus the explanation.
    fn option_height(&self, canvas: &dyn Canvas, width: f32, text: &str) -> f32 {
        let lines = self.option_lines(canvas, width, text).len() as f32;
        let text_h = Role::Body.line_height() + space::XXS + lines * Role::Meta.line_height();
        2.0 * OPTION_PAD + text_h.max(OPTION_ICON_BOX)
    }

    /// **CANVAS space.** The two option rows, in order.
    pub fn option_rects(&self, canvas: &dyn Canvas, bounds: Rect) -> Vec<Rect> {
        let body = self.window.body_rect(bounds);
        let width = bounds.right - bounds.left;
        let mut y = body.top
            + BODY_PAD
            + self.lines(canvas, width).len() as f32 * MESSAGE_LINE
            + CONFLICT_GAP;
        let mut out = Vec::new();
        for (_, _, explain, _) in self.options() {
            let h = self.option_height(canvas, width, &explain);
            out.push(Rect::new(body.left + BODY_PAD, y, body.right - BODY_PAD, y + h));
            y += h + CONFLICT_GAP;
        }
        out
    }

    /// Which choice a canvas-space point lands on. The two rows are buttons on
    /// the web (`<button type="button">`), so this is their hit-testing.
    pub fn choice_at(
        &self,
        canvas: &dyn Canvas,
        bounds: Rect,
        x: f32,
        y: f32,
    ) -> Option<ConflictChoice> {
        self.option_rects(canvas, bounds)
            .into_iter()
            .zip(self.options())
            .find(|(r, _)| r.contains(x, y))
            .map(|(_, (choice, _, _, _))| choice)
    }

    fn content_height(&self, canvas: &dyn Canvas, width: f32) -> f32 {
        let message = self.lines(canvas, width).len() as f32 * MESSAGE_LINE;
        let rows: f32 = self
            .options()
            .iter()
            .map(|(_, _, explain, _)| self.option_height(canvas, width, explain) + CONFLICT_GAP)
            .sum();
        2.0 * BODY_PAD + message + rows
    }

    /// See the module docs.
    pub fn measure_at(&self, canvas: &dyn Canvas, max_width: f32) -> Size {
        let w = self.window.width_for(max_width);
        self.window.size_for(self.content_height(canvas, w), max_width)
    }

    /// The veil, then the dialog.
    pub fn paint_modal(&self, canvas: &dyn Canvas, host: Rect, bounds: Rect, state: WidgetState) {
        if self.window.backdrop {
            canvas.fill_rounded(&host, 0.0, &canvas.theme().dialog_scrim);
        }
        self.paint(canvas, bounds, state);
    }

    /// One option row: its frame, the tinted icon box, the title and the
    /// explanation.
    fn paint_option(&self, canvas: &dyn Canvas, rect: Rect, i: usize, hot: bool, width: f32) {
        let t = canvas.theme();
        let f = canvas.formats();
        let options = self.options();
        let Some((choice, title, explain, icon)) = options.get(i) else {
            return;
        };

        // `hover:border-primary hover:bg-primary/5` — the accent tint the web
        // washes the row with is `accent_light`, the token that already is
        // « the accent at a fraction of its strength ».
        if hot {
            canvas.fill_rounded(&rect, radius::XL, &t.accent_light);
        }
        canvas.stroke_rounded(&rect, radius::XL, if hot { &t.accent } else { &t.card_stroke });

        // `bg-danger/10` on the destructive option, `bg-primary/10` on the
        // other — the same two tinted grounds the palette carries as
        // `danger_light` and `accent_light`.
        let (ground, glyph) = if *choice == ConflictChoice::Overwrite {
            (t.danger_light, t.danger)
        } else {
            (t.accent_light, t.accent)
        };
        // `mt-0.5` on the icon box: it sits a hair under the title's top.
        let box_top = rect.top + OPTION_PAD + space::XXS;
        let box_ = Rect::new(
            rect.left + OPTION_PAD,
            box_top,
            rect.left + OPTION_PAD + OPTION_ICON_BOX,
            box_top + OPTION_ICON_BOX,
        );
        canvas.fill_rounded(&box_, radius::LG, &ground);
        canvas.vector_icon(icon, &box_, OPTION_ICON_GLYPH, &glyph);

        let left = box_.right + OPTION_GAP;
        let right = rect.right - OPTION_PAD;
        let line = Rect::new(left, rect.top + OPTION_PAD, right, rect.top + OPTION_PAD + Role::Body.line_height());
        // `text-sm font-medium text-text-primary`.
        canvas.text_ellipsis(title, &line, &f.body_strong, &t.text_primary);
        paint_lines(
            canvas,
            &self.option_lines(canvas, width, explain),
            Rect::new(left, line.bottom + space::XXS, right, 0.0),
            Role::Meta.line_height(),
            &f.caption,
            &t.text_tertiary,
        );

        if self.window.focus_state(DialogPart::Option(i)).show_focus_ring() {
            // A `<button>` without a ring class: the browser's
            // `:focus-visible` outline, in the product's one ring colour.
            paint_focus_ring(canvas, rect, radius::XL, &t.accent);
        }
    }
}

impl Deref for ConflictDialog {
    type Target = FloatingWindow;
    fn deref(&self) -> &FloatingWindow {
        &self.window
    }
}

impl DerefMut for ConflictDialog {
    fn deref_mut(&mut self) -> &mut FloatingWindow {
        &mut self.window
    }
}

impl Widget for ConflictDialog {
    fn model(&self) -> &dyn Control {
        self.window.model()
    }

    fn measure(&self, canvas: &dyn Canvas) -> Size {
        self.measure_at(canvas, f32::INFINITY)
    }

    fn paint(&self, canvas: &dyn Canvas, bounds: Rect, state: WidgetState) {
        self.window.paint(canvas, bounds, state);

        let body = self.window.body_rect(bounds);
        let width = bounds.right - bounds.left;
        // The rows' focus rings reach 3 DIP past them, i.e. into the body's
        // padding, which the clip keeps.
        canvas.push_clip(&body);
        paint_lines(
            canvas,
            &self.lines(canvas, width),
            Rect::new(body.left + BODY_PAD, body.top + BODY_PAD, body.right - BODY_PAD, 0.0),
            MESSAGE_LINE,
            &canvas.formats().body,
            &canvas.theme().text_secondary,
        );

        for (i, rect) in self.option_rects(canvas, bounds).into_iter().enumerate() {
            self.paint_option(canvas, rect, i, self.hot == Some(i), width);
        }
        canvas.pop_clip();
    }

    fn type_name(&self) -> &'static str {
        "ConflictDialog"
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// Popover — the anchored floating panel.
// ═════════════════════════════════════════════════════════════════════════════

/// Which of the popover's edges lines up with the anchor's — `align: 'left' |
/// 'right'` in `AnchoredPopover.tsx`, generalised to the cross axis of
/// whichever side it settles on.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Align {
    /// Leading edges flush: the panel's left with the anchor's left (or its top
    /// with the anchor's top, on a horizontal side).
    #[default]
    Start,
    /// Trailing edges flush.
    End,
    /// Centred on the anchor — what a tooltip does, offered so a caller can ask
    /// for it without leaving this family.
    Center,
}

/// Every alignment, for the gallery and the tests.
pub const ALIGNMENTS: [Align; 3] = [Align::Start, Align::End, Align::Center];

/// Where an anchored panel of `size` goes — **the tooltip's placement, reused**.
///
/// [`crate::display::place`] already solved this problem: prefer a side, flip
/// to the opposite one when it does not fit, take the roomier of the two when
/// neither does, and clamp the result inside the viewport with a
/// [`crate::display::Tooltip::MARGIN`] keep-off. `AnchoredPopover.tsx` does the
/// same thing on one axis (« below the anchor by default; flip above if it
/// would go off the bottom »), with the same 8 px margin. So the flip decision
/// and the clamp are that function's, called here, not a second copy.
///
/// Two things a popover does that a tooltip does not, and neither can be passed
/// to `place` as an argument — which is why they are applied afterwards rather
/// than by forking it:
///
/// 1. **A different gap.** `TOOLTIP_GAP` is 14 (« clears the mouse cursor
///    itself »); a popover hangs 4 off its anchor because it is anchored to a
///    *control*, not to the pointer. `place` bakes its gap in as a constant, so
///    the rectangle is pulled back toward the anchor by the difference — a move
///    INTO the viewport, which cannot undo the clamp.
/// 2. **Edge alignment instead of centring.** A menu's left edge lines up with
///    its button's; a tooltip is centred. The cross axis is therefore
///    recomputed from [`Align`] and re-clamped — four lines, the only ones in
///    this function that are not reused.
///
/// The returned [`Placement`] carries the tooltip's vocabulary. Its `tip` is
/// the midpoint of the anchor-facing edge: a popover paints no arrow, and a
/// meaningful point is better than a stale one.
pub fn place_anchored(
    anchor: Rect,
    size: Size,
    preferred: Side,
    align: Align,
    viewport: Size,
) -> Placement {
    let base = display::place(anchor, size, preferred, viewport);
    let side = base.side;
    let pull = display::Tooltip::GAP - POPOVER_GAP;

    // (1) The gap. Moving toward the anchor is moving away from the edge that
    // forced the flip, so the clamp `place` applied still holds.
    let mut rect = match side {
        Side::Top => Rect::new(base.rect.left, base.rect.top + pull, base.rect.right, base.rect.bottom + pull),
        Side::Bottom => Rect::new(base.rect.left, base.rect.top - pull, base.rect.right, base.rect.bottom - pull),
        Side::Left => Rect::new(base.rect.left + pull, base.rect.top, base.rect.right + pull, base.rect.bottom),
        Side::Right => Rect::new(base.rect.left - pull, base.rect.top, base.rect.right - pull, base.rect.bottom),
    };

    // (2) The cross axis.
    if side.is_vertical() {
        let left = match align {
            Align::Start => anchor.left,
            Align::End => anchor.right - size.width,
            Align::Center => rect.left,
        };
        let left = clamp_cross(left, size.width, viewport.width);
        rect = Rect::new(left, rect.top, left + size.width, rect.bottom);
    } else {
        let top = match align {
            Align::Start => anchor.top,
            Align::End => anchor.bottom - size.height,
            Align::Center => rect.top,
        };
        let top = clamp_cross(top, size.height, viewport.height);
        rect = Rect::new(rect.left, top, rect.right, top + size.height);
    }

    let tip = match side {
        Side::Top => ((rect.left + rect.right) / 2.0, rect.bottom),
        Side::Bottom => ((rect.left + rect.right) / 2.0, rect.top),
        Side::Left => (rect.right, (rect.top + rect.bottom) / 2.0),
        Side::Right => (rect.left, (rect.top + rect.bottom) / 2.0),
    };
    Placement { rect, side, tip }
}

/// Keeps an `extent`-long box inside `[0, limit]`, a
/// [`crate::display::Tooltip::MARGIN`] off both ends — `M = 8` in
/// `AnchoredPopover.tsx`, the same number, taken from the same place.
fn clamp_cross(v: f32, extent: f32, limit: f32) -> f32 {
    let lo = display::Tooltip::MARGIN;
    let hi = (limit - extent - display::Tooltip::MARGIN).max(lo);
    v.clamp(lo, hi)
}

/// [`place_anchored`] against an `area` that does not start at the origin —
/// the monitor's work area in the window's client coordinates
/// (`Frame::screen_area`), which is what a popover shown in a
/// [`kubuno_desktop_controls::host::popup`] is placed against: it may hang past the
/// window's edges, never past the screen's. The web places against the
/// viewport; on the desktop the viewport of a floating surface is the screen.
pub fn place_anchored_in(anchor: Rect, size: Size, preferred: Side, align: Align, area: Rect) -> Placement {
    let (ox, oy) = (area.left, area.top);
    let local = Rect::new(anchor.left - ox, anchor.top - oy, anchor.right - ox, anchor.bottom - oy);
    let viewport = Size::new((area.right - area.left).max(0.0), (area.bottom - area.top).max(0.0));
    let p = place_anchored(local, size, preferred, align, viewport);
    Placement {
        rect: Rect::new(p.rect.left + ox, p.rect.top + oy, p.rect.right + ox, p.rect.bottom + oy),
        side: p.side,
        tip: (p.tip.0 + ox, p.tip.1 + oy),
    }
}

/// The rectangle a floating surface's popup must cover: the panel plus its
/// shadow ([`SHADOW_PAD`] all round).
pub fn surface_bounds(panel: Rect) -> Rect {
    panel.inflate(SHADOW_PAD, SHADOW_PAD)
}

/// `rect` rebased into the local space of a popup whose bounds are `popup`
/// (the popup's paint closure draws with its top-left at the origin).
pub fn rebase(rect: Rect, popup: Rect) -> Rect {
    Rect::new(rect.left - popup.left, rect.top - popup.top, rect.right - popup.left, rect.bottom - popup.top)
}

/// A floating panel pinned to a control — `AnchoredPopover`, which portals a
/// menu or a picker out of a clipping toolbar and positions it against its
/// button.
///
/// The web component paints **nothing**: it is a positioner, and its child
/// carries the chrome. On the desktop there is no DOM to inherit a surface
/// from, so this one paints the float surface every Kubuno popup wears —
/// `layer_background` at [`radius::FLOAT`] under [`SHADOW_MENU`], exactly what
/// `lists`' menu panel draws, so a popover and a menu cannot drift apart.
///
/// Its content is a [`Panel`]'s children: a caller pushes them, this type
/// places and paints them.
pub struct Popover {
    panel: Panel,
    /// The side the caller prefers. Honoured when it fits — see
    /// [`place_anchored`].
    pub side: Side,
    pub align: Align,
    /// What the content asks for. The web reads it off the DOM
    /// (`p.offsetWidth || 232`), which a desktop caller does by measuring its
    /// content; the fallback pair is the component's own.
    pub size: Size,
}

impl Default for Popover {
    fn default() -> Self {
        Self {
            panel: Panel::new().with_padding(Padding::all(space::XS)),
            // `top = r.bottom + gap` — below the anchor, like the component.
            side: Side::Bottom,
            align: Align::default(),
            size: Size::new(POPOVER_WIDTH, POPOVER_HEIGHT),
        }
    }
}

impl Popover {
    pub fn new() -> Self {
        Self::default()
    }

    /// A popover that asks for a given size.
    pub fn sized(size: Size) -> Self {
        Self { size, ..Self::default() }
    }

    pub fn side(mut self, side: Side) -> Self {
        self.side = side;
        self
    }

    pub fn align(mut self, align: Align) -> Self {
        self.align = align;
        self
    }

    /// The content panel, for a caller that places children in it.
    pub fn panel_mut(&mut self) -> &mut Panel {
        &mut self.panel
    }

    /// Where this panel lands against `anchor`. Thin wrapper over the pure
    /// [`place_anchored`].
    pub fn place(&self, anchor: Rect, viewport: Size) -> Placement {
        place_anchored(anchor, self.size, self.side, self.align, viewport)
    }

    /// Where this panel lands against `anchor` inside an arbitrary `area` —
    /// the screen, for a popover shown in a popup. See [`place_anchored_in`].
    pub fn place_in(&self, anchor: Rect, area: Rect) -> Placement {
        place_anchored_in(anchor, self.size, self.side, self.align, area)
    }

    /// Whether a click at `(x, y)` dismisses it.
    ///
    /// The component renders a full-viewport `onMouseDown={onClose}` catcher
    /// under the panel, so anything outside the panel closes it — and unlike a
    /// dialog's backdrop there is no modal flag to check: an anchored popover
    /// is always light-dismissed.
    pub fn dismisses(&self, bounds: Rect, x: f32, y: f32) -> bool {
        !bounds.contains(x, y)
    }
}

impl Deref for Popover {
    type Target = Panel;
    fn deref(&self) -> &Panel {
        &self.panel
    }
}

impl DerefMut for Popover {
    fn deref_mut(&mut self) -> &mut Panel {
        &mut self.panel
    }
}

impl Widget for Popover {
    fn model(&self) -> &dyn Control {
        self.panel.model()
    }

    fn measure(&self, _canvas: &dyn Canvas) -> Size {
        self.size
    }

    fn paint(&self, canvas: &dyn Canvas, bounds: Rect, state: WidgetState) {
        let t = canvas.theme();
        canvas.draw_shadow(&bounds, radius::FLOAT, &SHADOW_MENU, SHADOW_GREY);
        // OPAQUE, like the menu panel: `flyout_background` is half-opaque and
        // is meant to sit over a real acrylic blur, which a popover painted
        // in-window does not have.
        canvas.fill_rounded(&bounds, radius::FLOAT, &t.layer_background);
        // Content clipped to the rounded panel: a child row painting its own
        // hover ground must not square off the corners.
        canvas.push_clip_rounded(&bounds, radius::FLOAT);
        self.panel.paint(canvas, bounds, state);
        canvas.pop_clip_rounded();
        canvas.stroke_rounded(&bounds, radius::FLOAT, &t.card_stroke);
    }

    fn type_name(&self) -> &'static str {
        "Popover"
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// Toast — the transient notification, and its stack.
// ═════════════════════════════════════════════════════════════════════════════

/// How loud a toast is — `ToastVariant`, and its `SKIN` map.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ToastVariant {
    #[default]
    Info,
    Success,
    Warning,
    Danger,
}

/// Every variant, for the gallery and the tests.
pub const TOAST_VARIANTS: [ToastVariant; 4] = [
    ToastVariant::Info,
    ToastVariant::Success,
    ToastVariant::Warning,
    ToastVariant::Danger,
];

impl ToastVariant {
    /// `SKIN`: `Info`, `CheckCircle2`, `AlertTriangle`, `AlertCircle`.
    pub fn icon(self) -> &'static str {
        match self {
            ToastVariant::Info => "Info",
            ToastVariant::Success => "CheckCircle2",
            ToastVariant::Warning => "AlertTriangle",
            ToastVariant::Danger => "AlertCircle",
        }
    }

    /// `text-primary` / `text-success` / `text-warning` / `text-danger`.
    fn ink(self, t: &kubuno_drive_desktop_app_controls::Theme) -> D2D1_COLOR_F {
        match self {
            ToastVariant::Info => t.accent,
            ToastVariant::Success => t.success,
            ToastVariant::Warning => t.warning,
            ToastVariant::Danger => t.danger,
        }
    }

    /// `duration ?? (variant === 'danger' ? 6000 : 4000)`.
    pub fn duration_ms(self) -> u32 {
        if self == ToastVariant::Danger {
            TOAST_MS_DANGER
        } else {
            TOAST_MS
        }
    }
}

/// Where the stack is anchored — `ToastProviderProps['placement']`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ToastPlacement {
    #[default]
    BottomRight,
    BottomLeft,
    TopRight,
    TopCenter,
}

/// Every placement, for the gallery and the tests.
pub const TOAST_PLACEMENTS: [ToastPlacement; 4] = [
    ToastPlacement::BottomRight,
    ToastPlacement::BottomLeft,
    ToastPlacement::TopRight,
    ToastPlacement::TopCenter,
];

impl ToastPlacement {
    fn anchored_at_bottom(self) -> bool {
        matches!(self, ToastPlacement::BottomRight | ToastPlacement::BottomLeft)
    }
}

/// Lays a stack of toasts out inside `host` — **pure**, so « one », « three »
/// and « more than fits » are unit tests.
///
/// The rules, from `ToastProvider`:
///
/// * the anchor is `bottom-4 right-4` (or its three variants), i.e.
///   [`TOAST_INSET`] off both edges;
/// * the cards are a `flex flex-col gap-2`, so they stack along the anchor's
///   axis with [`TOAST_STACK_GAP`] between them;
/// * past `max = 4` the **oldest** is dropped, « the newest message is the
///   relevant one ». `sizes` is oldest-first, so this places the LAST
///   [`TOAST_MAX`] and returns one rectangle per placed toast;
/// * a card never exceeds `min(24rem, calc(100vw - 2rem))`, which is why the
///   width is clamped to the host rather than taken on trust.
pub fn stack(host: Rect, placement: ToastPlacement, sizes: &[Size]) -> Vec<Rect> {
    let shown = &sizes[sizes.len().saturating_sub(TOAST_MAX)..];
    let room = (host.right - host.left - 2.0 * TOAST_INSET).max(0.0);
    let mut out = vec![Rect::new(0.0, 0.0, 0.0, 0.0); shown.len()];

    // Bottom-anchored stacks grow UPWARD from the last card, so they are built
    // backwards; top-anchored ones grow downward from the first.
    let mut cursor = if placement.anchored_at_bottom() {
        host.bottom - TOAST_INSET
    } else {
        host.top + TOAST_INSET
    };
    let order: Vec<usize> = if placement.anchored_at_bottom() {
        (0..shown.len()).rev().collect()
    } else {
        (0..shown.len()).collect()
    };

    for i in order {
        let w = shown[i].width.min(room);
        let h = shown[i].height;
        let left = match placement {
            ToastPlacement::BottomRight | ToastPlacement::TopRight => host.right - TOAST_INSET - w,
            ToastPlacement::BottomLeft => host.left + TOAST_INSET,
            ToastPlacement::TopCenter => host.left + (host.right - host.left - w) / 2.0,
        };
        out[i] = if placement.anchored_at_bottom() {
            let r = Rect::new(left, cursor - h, left + w, cursor);
            cursor -= h + TOAST_STACK_GAP;
            r
        } else {
            let r = Rect::new(left, cursor, left + w, cursor + h);
            cursor += h + TOAST_STACK_GAP;
            r
        };
    }
    out
}

/// A transient notification — `ToastCard`.
///
/// `rounded-lg border border-border bg-surface-0 px-3 py-2.5` under
/// `--kb-shadow-float`, with three columns: the variant glyph, the text (an
/// optional bold title, the message, an optional inline action), and the ✕.
///
/// Its model is an empty [`kubuno_desktop_controls::labels::Label`], as `Badge` and
/// `Tooltip` do: WinForms has no toast, so there is a property surface to
/// borrow (`text`, `padding`, `enabled`, `minimum_size`) and no state machine
/// to inherit. The message IS the label's `text`.
///
/// The **timing** (a 100 ms tick, paused under the pointer) is the host's, not
/// this type's: it is a message loop concern with no painted result. What is
/// ported is the number a host counts down — [`ToastVariant::duration_ms`].
pub struct Toast {
    inner: kc::Label,
    /// The optional bold line above the message.
    pub title: String,
    pub variant: ToastVariant,
    /// One inline action: « Annuler », « Voir », « Réessayer ».
    pub action: Option<String>,
    /// Lifetime in ms. `0` keeps it until dismissed — « for failures the user
    /// must acknowledge ».
    pub duration_ms: u32,
    /// The pointer is on the ✕ (`hover:bg-surface-2 hover:text-text-primary`).
    /// Painting with `WidgetState::hot` still lights it too — the original
    /// convention, kept for callers that have one target only.
    pub close_hot: bool,
    /// The pointer is on the inline action (`hover:bg-primary-light`).
    pub action_hot: bool,
    /// Which of the card's two buttons holds the keyboard focus.
    pub focus: Option<ToastPart>,
    /// Whether that focus shows its ring (`focus-visible:ring-2`).
    pub focus_visible: bool,
}

/// The two buttons a toast card carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastPart {
    /// The inline action (« Annuler », « Réessayer »).
    Action,
    /// The ✕.
    Close,
}

impl Toast {
    /// A toast in a variant, carrying its message.
    pub fn new(variant: ToastVariant, message: impl Into<String>) -> Self {
        let mut inner = kc::Label::new();
        inner.text = message.into();
        Self {
            inner,
            title: String::new(),
            variant,
            action: None,
            duration_ms: variant.duration_ms(),
            close_hot: false,
            action_hot: false,
            focus: None,
            focus_visible: false,
        }
    }

    /// The card's buttons in tab order: the action (inside the text column,
    /// first in the DOM), then the ✕.
    pub fn focus_order(&self) -> Vec<ToastPart> {
        let mut out = Vec::new();
        if self.action.is_some() {
            out.push(ToastPart::Action);
        }
        out.push(ToastPart::Close);
        out
    }

    pub fn info(message: impl Into<String>) -> Self {
        Self::new(ToastVariant::Info, message)
    }

    pub fn success(message: impl Into<String>) -> Self {
        Self::new(ToastVariant::Success, message)
    }

    pub fn warning(message: impl Into<String>) -> Self {
        Self::new(ToastVariant::Warning, message)
    }

    pub fn error(message: impl Into<String>) -> Self {
        Self::new(ToastVariant::Danger, message)
    }

    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = title.into();
        self
    }

    pub fn with_action(mut self, label: impl Into<String>) -> Self {
        self.action = Some(label.into());
        self
    }

    /// Keeps it until dismissed (`duration: 0`).
    pub fn sticky(mut self) -> Self {
        self.duration_ms = 0;
        self
    }

    /// **CANVAS space.** The variant glyph's column.
    pub fn icon_rect(&self, bounds: Rect) -> Rect {
        let left = bounds.left + TOAST_PAD_X;
        let top = bounds.top + TOAST_PAD_Y;
        Rect::new(left, top, left + TOAST_GLYPH, top + Role::Body.line_height())
    }

    /// **CANVAS space.** The ✕, `-mr-1 -mt-0.5` off the padding box.
    pub fn close_rect(&self, bounds: Rect) -> Rect {
        let right = bounds.right - TOAST_PAD_X + space::XS;
        let top = bounds.top + TOAST_PAD_Y - space::XXS;
        Rect::new(right - TOAST_CLOSE_BOX, top, right, top + TOAST_CLOSE_BOX)
    }

    /// The text column, between the glyph and the ✕.
    fn text_rect(&self, bounds: Rect) -> Rect {
        Rect::new(
            bounds.left + TOAST_PAD_X + TOAST_GLYPH + TOAST_GAP,
            bounds.top + TOAST_PAD_Y,
            self.close_rect(bounds).left - TOAST_GAP,
            bounds.bottom - TOAST_PAD_Y,
        )
    }

    /// How wide the text column is in a card `width` wide — everything the
    /// glyph, the ✕ and the paddings leave.
    fn text_width(&self, width: f32) -> f32 {
        let r = self.text_rect(Rect::new(0.0, 0.0, width, 0.0));
        (r.right - r.left).max(0.0)
    }

    /// The message, wrapped to the text column of a card `width` wide.
    pub fn lines(&self, canvas: &dyn Canvas, width: f32) -> Vec<String> {
        wrap_on(canvas, &self.inner.text, self.text_width(width), &canvas.formats().body)
    }

    /// The card's size.
    ///
    /// **Width: shrink-to-fit, capped.** The stack is a `flex-col items-end`
    /// with `maxWidth: min(24rem, calc(100vw - 2rem))`, so a card is as wide
    /// as its content wants — the longest of its title, its message on one
    /// line and its action — and never wider than [`TOAST_WIDTH`] nor
    /// `max_width`. « Enregistré » is a small card; a sentence reaches the cap
    /// and wraps. **Height:** as tall as its rows.
    pub fn measure_at(&self, canvas: &dyn Canvas, max_width: f32) -> Size {
        let f = canvas.formats();
        let chrome = TOAST_WIDTH - self.text_width(TOAST_WIDTH);
        let mut natural = self
            .inner
            .text
            .split('\n')
            .map(|l| canvas.measure(l, &f.body))
            .fold(0.0_f32, f32::max);
        if !self.title.is_empty() {
            natural = natural.max(canvas.measure(&self.title, &f.body_strong));
        }
        if let Some(label) = self.action.as_ref() {
            // The button pulls itself back by its own padding (`-ml-1.5`), so
            // it asks for its label plus the trailing `px-1.5`.
            natural = natural.max(canvas.measure(label, &f.body) + TOAST_ACTION_PAD_X);
        }
        let width = (natural.ceil() + chrome).min(TOAST_WIDTH).min(max_width).max(0.0);
        let line = Role::Body.line_height();
        let mut h = self.lines(canvas, width).len() as f32 * line;
        if !self.title.is_empty() {
            // `<p className="font-medium">` then `mt-0.5` on the message.
            h += line + space::XXS;
        }
        if self.action.is_some() {
            // `mt-1.5` over a `px-1.5 py-0.5` text button.
            h += TOAST_ACTION_GAP + line + 2.0 * space::XXS;
        }
        Size::new(width, 2.0 * TOAST_PAD_Y + h.max(line))
    }

    /// **CANVAS space.** The inline action's button — `mt-1.5 -ml-1.5 px-1.5
    /// py-0.5 rounded-md` under the message — or `None` without an action.
    pub fn action_rect(&self, canvas: &dyn Canvas, bounds: Rect) -> Option<Rect> {
        let label = self.action.as_ref()?;
        let text = self.text_rect(bounds);
        let line = Role::Body.line_height();
        let mut y = text.top;
        if !self.title.is_empty() {
            y += line + space::XXS;
        }
        y += self.lines(canvas, bounds.right - bounds.left).len() as f32 * line + TOAST_ACTION_GAP;
        let left = text.left - TOAST_ACTION_PAD_X;
        let w = canvas.measure(label, &canvas.formats().body).ceil() + 2.0 * TOAST_ACTION_PAD_X;
        Some(Rect::new(left, y, (left + w).min(text.right), y + line + 2.0 * space::XXS))
    }

    /// Which of the card's buttons a point is on.
    pub fn part_at(&self, canvas: &dyn Canvas, bounds: Rect, x: f32, y: f32) -> Option<ToastPart> {
        if self.close_hit(bounds, x, y) {
            return Some(ToastPart::Close);
        }
        self.action_rect(canvas, bounds).filter(|r| r.contains(x, y)).map(|_| ToastPart::Action)
    }

    /// Whether the point is on the ✕.
    pub fn close_hit(&self, bounds: Rect, x: f32, y: f32) -> bool {
        self.close_rect(bounds).contains(x, y)
    }
}

impl Deref for Toast {
    type Target = kc::Label;
    fn deref(&self) -> &kc::Label {
        &self.inner
    }
}

impl DerefMut for Toast {
    fn deref_mut(&mut self) -> &mut kc::Label {
        &mut self.inner
    }
}

impl Widget for Toast {
    fn model(&self) -> &dyn Control {
        &self.inner
    }

    fn measure(&self, canvas: &dyn Canvas) -> Size {
        self.measure_at(canvas, f32::INFINITY)
    }

    fn paint(&self, canvas: &dyn Canvas, bounds: Rect, state: WidgetState) {
        let t = canvas.theme();
        let f = canvas.formats();

        canvas.draw_shadow(&bounds, radius::LG, &SHADOW_MENU, SHADOW_GREY);
        canvas.fill_rounded(&bounds, radius::LG, &t.layer_background);
        canvas.stroke_rounded(&bounds, radius::LG, &t.card_stroke);

        canvas.vector_icon(
            self.variant.icon(),
            &self.icon_rect(bounds),
            TOAST_GLYPH,
            &self.variant.ink(t),
        );

        let text = self.text_rect(bounds);
        let line = Role::Body.line_height();
        let mut y = text.top;
        if !self.title.is_empty() {
            // `<p className="font-medium text-text-primary">`.
            canvas.text_ellipsis(
                &self.title,
                &Rect::new(text.left, y, text.right, y + line),
                &f.body_strong,
                &t.text_primary,
            );
            y += line + space::XXS;
        }
        paint_lines(
            canvas,
            &self.lines(canvas, bounds.right - bounds.left),
            Rect::new(text.left, y, text.right, 0.0),
            line,
            &f.body,
            &t.text_secondary,
        );
        let ring = |r: Rect, part: ToastPart| {
            if self.focus == Some(part) && self.focus_visible {
                paint_focus_ring(canvas, r, radius::SM, &t.accent);
            }
        };
        if let (Some(label), Some(button)) = (self.action.as_ref(), self.action_rect(canvas, bounds)) {
            if self.action_hot {
                // `hover:bg-primary-light`, `rounded-md`.
                canvas.fill_rounded(&button, radius::SM, &t.accent_light);
            }
            let inner = Rect::new(button.left + TOAST_ACTION_PAD_X, button.top, button.right, button.bottom);
            canvas.text_ellipsis(label, &inner, &f.body, &t.accent);
            ring(button, ToastPart::Action);
        }

        let close = self.close_rect(bounds);
        let close_hot = self.close_hot || state.hot;
        if close_hot {
            // `hover:bg-surface-2 hover:text-text-primary`, `rounded-md`.
            canvas.fill_rounded(&close, radius::SM, &t.surface_2);
        }
        let ink = if close_hot { t.text_primary } else { t.text_tertiary };
        canvas.vector_icon("X", &close, TOAST_CLOSE_GLYPH, &ink);
        ring(close, ToastPart::Close);
    }

    fn type_name(&self) -> &'static str {
        "Toast"
    }
}

/// One toast in a [`ToastQueue`]: its id, the card, and the lifetime it has
/// left (`None` = sticky, `duration: 0`).
pub struct QueuedToast {
    pub id: u64,
    pub toast: Toast,
    pub left_ms: Option<u32>,
}

/// The toasts on screen — `ToastProvider`'s state and timer, without React.
///
/// * [`ToastQueue::push`] appends; past [`ToastQueue::max`] the OLDEST is
///   dropped (« the newest message is the relevant one »);
///   [`ToastQueue::push_with_id`] REPLACES a toast carrying the same id
///   instead of stacking a duplicate (`opts.id`, « a save indicator fired on
///   every keystroke »).
/// * [`ToastQueue::tick`] counts the lifetimes down and removes the expired
///   ones — except while `paused`, which the host sets while the pointer is
///   over the stack or the focus is in it (« otherwise a toast carrying an
///   Undo can expire under the cursor on its way to the button »).
/// * [`ToastQueue::display_order`] is the order the cards stack in: the web
///   renders two live regions, `polite` (info, success) then `assertive`
///   (warning, danger), so a failure sits BELOW every « Enregistré » in a
///   bottom-anchored stack, closest to the corner.
pub struct ToastQueue {
    items: Vec<QueuedToast>,
    next_id: u64,
    /// `max = 4`.
    pub max: usize,
}

impl Default for ToastQueue {
    fn default() -> Self {
        Self { items: Vec::new(), next_id: 1, max: TOAST_MAX }
    }
}

impl ToastQueue {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a toast and returns its id.
    pub fn push(&mut self, toast: Toast) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.push_with_id(id, toast)
    }

    /// Adds a toast under `id`, replacing any toast that already carries it
    /// (the replacement goes to the END, as the web's `[...without, entry]`).
    pub fn push_with_id(&mut self, id: u64, toast: Toast) -> u64 {
        self.items.retain(|q| q.id != id);
        let left_ms = (toast.duration_ms > 0).then_some(toast.duration_ms);
        self.items.push(QueuedToast { id, toast, left_ms });
        let max = self.max.max(1);
        if self.items.len() > max {
            let extra = self.items.len() - max;
            self.items.drain(..extra);
        }
        self.next_id = self.next_id.max(id.saturating_add(1));
        id
    }

    /// Removes one toast; whether it was there.
    pub fn dismiss(&mut self, id: u64) -> bool {
        let n = self.items.len();
        self.items.retain(|q| q.id != id);
        self.items.len() != n
    }

    /// `dismissAll`.
    pub fn clear(&mut self) {
        self.items.clear();
    }

    /// Counts `elapsed_ms` off every timed toast unless `paused`, removes the
    /// expired ones and returns their ids.
    pub fn tick(&mut self, elapsed_ms: u64, paused: bool) -> Vec<u64> {
        if paused || elapsed_ms == 0 {
            return Vec::new();
        }
        let step = u32::try_from(elapsed_ms).unwrap_or(u32::MAX);
        let mut expired = Vec::new();
        for q in &mut self.items {
            if let Some(left) = q.left_ms.as_mut() {
                *left = left.saturating_sub(step);
                if *left == 0 {
                    expired.push(q.id);
                }
            }
        }
        self.items.retain(|q| !expired.contains(&q.id));
        expired
    }

    /// The toasts, oldest first.
    pub fn items(&self) -> &[QueuedToast] {
        &self.items
    }

    pub fn items_mut(&mut self) -> &mut [QueuedToast] {
        &mut self.items
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Whether any toast is still counting down — a host keeps its timer
    /// running only then.
    pub fn is_timed(&self) -> bool {
        self.items.iter().any(|q| q.left_ms.is_some())
    }

    /// Indexes into [`ToastQueue::items`] in stacking order: the polite ones
    /// (info, success) oldest first, then the assertive ones (warning,
    /// danger) oldest first.
    pub fn display_order(&self) -> Vec<usize> {
        let assertive = |v: ToastVariant| matches!(v, ToastVariant::Warning | ToastVariant::Danger);
        let mut out: Vec<usize> = (0..self.items.len()).filter(|&i| !assertive(self.items[i].toast.variant)).collect();
        out.extend((0..self.items.len()).filter(|&i| assertive(self.items[i].toast.variant)));
        out
    }

    /// Lays the stack out in `host` — each card measured against the room the
    /// stack leaves (`calc(100vw - 2rem)`), so a narrow host wraps the text
    /// instead of clipping it — and returns `(index into items, rect)` in
    /// stacking order.
    pub fn layout(&self, canvas: &dyn Canvas, host: Rect, placement: ToastPlacement) -> Vec<(usize, Rect)> {
        let room = (host.right - host.left - 2.0 * TOAST_INSET).max(0.0);
        let order = self.display_order();
        let sizes: Vec<Size> = order.iter().map(|&i| self.items[i].toast.measure_at(canvas, room)).collect();
        order.into_iter().zip(stack(host, placement, &sizes)).collect()
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// Tests
// ═════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    /// A stand-in for DirectWrite: seven DIP a character, which is close enough
    /// to the 14 DIP UI face to make the wrap arithmetic readable in a test.
    fn width_of(s: &str) -> f32 {
        s.chars().count() as f32 * 7.0
    }

    fn lines(text: &str, max: f32) -> Vec<String> {
        wrap(text, max, &width_of)
    }

    // ── Wrapping, i.e. how tall a dialog is ───────────────────────────────

    #[test]
    fn a_short_message_is_one_line() {
        assert_eq!(lines("Supprimer ?", 400.0), vec!["Supprimer ?".to_string()]);
    }

    #[test]
    fn a_long_message_wraps_at_the_width_it_is_given() {
        // 12 words of 4 characters: three of them and their two spaces are 14
        // glyphs, i.e. 98 DIP — a fourth would not fit in 100.
        let text = "abcd abcd abcd abcd abcd abcd abcd abcd abcd abcd abcd abcd";
        assert_eq!(lines(text, 100.0).len(), 4, "three words a line → 4 lines");
        // 19 glyphs = 133 DIP is a fourth word, and 24 is not a fifth.
        assert_eq!(lines(text, 140.0).len(), 3, "a wider box takes fewer lines");
        assert_eq!(lines(text, 1000.0).len(), 1, "it all fits on one");
    }

    #[test]
    fn an_explicit_newline_always_breaks() {
        // `whitespace-pre-line`: the caller's breaks are content, and an empty
        // line still costs a line box.
        assert_eq!(lines("un\ndeux", 1000.0).len(), 2);
        assert_eq!(lines("un\n\ndeux", 1000.0).len(), 3);
    }

    #[test]
    fn a_word_wider_than_the_box_is_broken_so_no_line_overflows() {
        // `overflow-wrap: anywhere`: the audit's defect was this word running
        // past the dialog's edge.
        let l = lines("court incassablementtreslong court", 100.0);
        for s in &l {
            assert!(width_of(s) <= 100.0, "« {s} » overflows: {l:?}");
        }
        assert_eq!(l.concat().replace(' ', ""), "courtincassablementtreslongcourt", "no glyph lost: {l:?}");
        assert_eq!(l[0], "court", "the short word before it keeps its own line");
    }

    #[test]
    fn a_long_file_name_breaks_at_its_separators_first() {
        // The audit's own case (composition page): a hyphenated file name in
        // a sentence. 14 characters a line at 7 DIP each.
        let name = "Compte-rendu-comité-de-pilotage-v4-final.docx";
        let l = lines(&format!("Supprimer « {name} » ?"), 98.0);
        for s in &l {
            assert!(width_of(s) <= 98.0, "« {s} » overflows: {l:?}");
        }
        // Breaks land after a separator, not mid-word, whenever one is near.
        let inner: Vec<&String> = l.iter().filter(|s| s.contains('-') && !s.starts_with("Supprimer")).collect();
        assert!(
            inner.iter().any(|s| s.ends_with('-') || s.ends_with('.')),
            "expected a break after a separator: {l:?}"
        );
        assert_eq!(l.concat().replace(' ', ""), format!("Supprimer«{name}»?"), "no glyph lost: {l:?}");
    }

    #[test]
    fn a_hyphen_is_a_break_opportunity_after_other_words() {
        // « peut-être » does not fit after « abcd » in 70 DIP (10 glyphs), but
        // « abcd peut- » does: the browser breaks after the hyphen.
        let l = lines("abcd peut-être", 70.0);
        assert_eq!(l, vec!["abcd peut-".to_string(), "être".to_string()]);
    }

    #[test]
    fn a_box_narrower_than_a_glyph_still_terminates() {
        let l = lines("abc", 1.0);
        assert_eq!(l, vec!["a".to_string(), "b".to_string(), "c".to_string()]);
        // A CRLF paragraph break is one break, not a stray glyph.
        assert_eq!(lines("un\r\ndeux", 1000.0), vec!["un".to_string(), "deux".to_string()]);
    }

    #[test]
    fn a_very_long_title_does_not_change_the_measurement() {
        // The band ellipsises rather than wrapping (`text_ellipsis`), so a long
        // title costs exactly the same height as a short one — which is what
        // makes a dialog's height a function of its BODY alone.
        let short = FloatingWindow::new("Ok");
        let long = FloatingWindow::new("Réinitialiser le mot de passe de Marie Dupont, sans faute");
        assert_eq!(short.chrome_height(), long.chrome_height());
    }

    // ── The window's own arithmetic ───────────────────────────────────────

    #[test]
    fn a_window_without_actions_has_no_footer_band() {
        let mut w = FloatingWindow::new("Panneau");
        assert_eq!(w.footer_height(), 0.0);
        assert_eq!(w.chrome_height(), kubuno_desktop_controls::window_chrome::TITLEBAR_HEIGHT);
        let bounds = Rect::new(0.0, 0.0, 400.0, 300.0);
        assert!(w.footer_rect(bounds).is_none());
        // The body then runs to the bottom edge.
        assert_eq!(w.body_rect(bounds).bottom, 300.0);

        w.actions = Actions::pair("OK", "Annuler");
        assert_eq!(w.footer_height(), 2.0 * FOOTER_PAD_Y + height::BUTTON_MD);
        assert_eq!(w.body_rect(bounds).bottom, 300.0 - w.footer_height());
    }

    #[test]
    fn the_three_bands_tile_the_window_without_a_gap() {
        let w = FloatingWindow::new("t").with_actions(Actions::pair("OK", "Annuler"));
        let bounds = Rect::new(100.0, 50.0, 500.0, 400.0);
        let band = w.titlebar_rect(bounds);
        let body = w.body_rect(bounds);
        let footer = w.footer_rect(bounds).expect("actions were set");

        assert_eq!(band.top, bounds.top);
        assert_eq!(band.bottom - band.top, kubuno_desktop_controls::window_chrome::TITLEBAR_HEIGHT);
        assert_eq!(body.top, band.bottom);
        assert_eq!(body.bottom, footer.top);
        assert_eq!(footer.bottom, bounds.bottom);
        for r in [band, body, footer] {
            assert_eq!((r.left, r.right), (bounds.left, bounds.right));
        }
    }

    #[test]
    fn the_close_button_sits_at_the_end_of_the_band() {
        let w = FloatingWindow::new("t");
        let bounds = Rect::new(0.0, 0.0, 400.0, 300.0);
        let c = w.close_rect(bounds);
        use kubuno_desktop_controls::window_chrome as wc;
        // The standard band's compact button: 24 DIP, 8 from the right edge.
        assert_eq!(c.right, 400.0 - wc::TOOL_PAD_X);
        assert_eq!(c.right - c.left, wc::TOOL_BUTTON);
        // Centred in the band, whatever the band's height.
        let band = w.titlebar_rect(bounds);
        assert_eq!((c.top + c.bottom) / 2.0, (band.top + band.bottom) / 2.0);
        assert!(w.close_hit(bounds, c.left + 1.0, c.top + 1.0));
        assert!(!w.close_hit(bounds, c.left - 4.0, c.top + 1.0));
    }

    // ── The action bar: order, position, minimum width ────────────────────

    #[test]
    fn the_confirming_action_is_left_of_the_cancel_and_both_hug_the_right_edge() {
        let footer = Rect::new(0.0, 200.0, 400.0, 260.0);
        // A wide confirm and a narrow cancel, so « same width » cannot hide a
        // wrong order.
        let r = action_rects(footer, &[140.0, 60.0]);
        assert_eq!(r.len(), 2);
        assert!(r[0].right <= r[1].left, "confirm must be LEFT of cancel");
        assert_eq!(r[1].right, footer.right - FOOTER_PAD_X, "the group hangs off the right");
        assert_eq!(r[1].left - r[0].right, FOOTER_GAP);
        assert_eq!(r[0].right - r[0].left, 140.0, "a wide button keeps its width");
        assert_eq!(r[1].right - r[1].left, ACTION_MIN_WIDTH, "a narrow one is floored");
        assert_eq!(r[0].top, footer.top + FOOTER_PAD_Y);
        assert_eq!(r[0].bottom - r[0].top, height::BUTTON_MD);
    }

    #[test]
    fn a_lone_cancel_still_sits_on_the_right() {
        let footer = Rect::new(0.0, 0.0, 400.0, 60.0);
        let r = action_rects(footer, &[80.0]);
        assert_eq!(r[0].right, footer.right - FOOTER_PAD_X);
    }

    #[test]
    fn the_actions_are_listed_in_paint_order() {
        let a = Actions::pair("Renommer", "Annuler");
        let ids: Vec<ActionId> = a.list().into_iter().map(|(id, _)| id).collect();
        assert_eq!(ids, vec![ActionId::Confirm, ActionId::Cancel]);
        assert!(Actions::only_cancel("Fermer").confirm.is_none());
        assert!(Actions::default().is_empty(), "no actions means no footer");
    }

    // ── Placement in a host ───────────────────────────────────────────────

    #[test]
    fn a_dialog_is_centred_horizontally_and_a_third_of_the_way_down() {
        let host = Rect::new(0.0, 0.0, 1000.0, 900.0);
        let r = place(host, Size::new(380.0, 300.0));
        assert_eq!(r.left, (1000.0 - 380.0) / 2.0);
        // `top: 33%` with `translateY(-33%)`.
        assert_eq!(r.top, 0.33 * 900.0 - 0.33 * 300.0);
        assert_eq!(r.right - r.left, 380.0);
        assert_eq!(r.bottom - r.top, 300.0);
    }

    #[test]
    fn a_dialog_never_leaves_a_host_smaller_than_itself() {
        // The case the CSS `maxWidth`/`maxHeight` clamps exist for.
        let host = Rect::new(40.0, 20.0, 340.0, 220.0); // 300 × 200
        let r = place(host, Size::new(380.0, 300.0));
        assert!(r.left >= host.left + HOST_MARGIN, "left edge escaped");
        assert!(r.top >= host.top + HOST_MARGIN, "top edge escaped");
        assert!(r.right <= host.right - HOST_MARGIN, "right edge escaped");
        assert!(r.bottom <= host.bottom - HOST_MARGIN, "bottom edge escaped");
        assert_eq!(r.right - r.left, 300.0 - 2.0 * HOST_MARGIN);
        assert_eq!(r.bottom - r.top, 200.0 - 2.0 * HOST_MARGIN);
    }

    #[test]
    fn a_host_narrower_than_the_margins_still_answers_a_rectangle() {
        let host = Rect::new(0.0, 0.0, 6.0, 6.0);
        let r = place(host, Size::new(380.0, 300.0));
        assert!(r.right >= r.left && r.bottom >= r.top, "the clamps crossed");
    }

    #[test]
    fn the_window_width_honours_the_minimum_and_the_host() {
        let w = FloatingWindow::new("t").with_width(WIDTH_CONFIRM);
        assert_eq!(w.width_for(f32::INFINITY), WIDTH_CONFIRM);
        // A host of 200 leaves 184 once the margins are taken — less than
        // `minWidth`, and `place` is what then shrinks the rectangle.
        assert_eq!(w.width_for(200.0), 200.0 - 2.0 * HOST_MARGIN);
        let narrow = FloatingWindow::new("t").with_width(100.0);
        assert_eq!(narrow.width_for(f32::INFINITY), MIN_WIDTH, "minWidth = 280");
    }

    // ── The veil, and the click that closes a modal ───────────────────────

    #[test]
    fn clicking_outside_a_modal_dismisses_it_and_clicking_inside_does_not() {
        let host = Rect::new(0.0, 0.0, 1000.0, 900.0);
        let w = FloatingWindow::new("t").modal();
        let r = place(host, Size::new(380.0, 300.0));
        assert!(w.dismisses(host, r, 10.0, 10.0), "a click on the veil closes");
        assert!(!w.dismisses(host, r, r.left + 5.0, r.top + 5.0), "a click inside must not");
        assert!(!w.dismisses(host, r, -50.0, -50.0), "outside the host is not the veil");

        // A non-modal window has no veil, so nothing dismisses it.
        let panel = FloatingWindow::new("t");
        assert!(!panel.dismisses(host, r, 10.0, 10.0));
    }

    // ── Confirm / Prompt / Conflict, without a canvas ─────────────────────

    #[test]
    fn the_danger_variant_reddens_the_confirming_action_too() {
        let d = ConfirmDialog::danger("Supprimer", "…");
        assert_eq!(d.variant.icon(), "Trash2");
        assert!(d.actions.confirm.as_ref().is_some_and(|a| a.danger));
        assert!(d.actions.cancel.as_ref().is_some_and(|a| !a.danger));

        let n = ConfirmDialog::new("Renommer", "…");
        assert_eq!(n.variant.icon(), "AlertTriangle");
        assert!(n.actions.confirm.as_ref().is_some_and(|a| !a.danger));

        // …and the pairing survives a label change.
        let relabelled = ConfirmDialog::danger("Supprimer", "…").labels("Supprimer", "Annuler");
        assert!(relabelled.actions.confirm.as_ref().is_some_and(|a| a.danger));
    }

    #[test]
    fn hiding_the_cancel_leaves_one_button() {
        let d = ConfirmDialog::new("Information", "…").hide_cancel();
        assert_eq!(d.actions.list().len(), 1);
        assert!(!d.actions.is_empty(), "a one-button dialog still has a footer");
    }

    #[test]
    fn an_empty_prompt_cannot_be_confirmed_unless_it_may_be() {
        let mut p = PromptDialog::new("Nouveau dossier");
        assert!(!p.can_confirm(), "empty is refused by default");
        p.field.set_text("   ");
        assert!(!p.can_confirm(), "`value.trim() !== ''`");
        p.field.set_text("Photos");
        assert!(p.can_confirm());
        p.sync_actions();
        assert!(p.actions.confirm.as_ref().is_some_and(|a| a.enabled));

        let mut empty_ok = PromptDialog::new("t").allow_empty(true);
        assert!(empty_ok.can_confirm());
        empty_ok.sync_actions();
        assert!(empty_ok.actions.confirm.as_ref().is_some_and(|a| a.enabled));
    }

    #[test]
    fn a_multiline_prompt_asks_for_three_rows() {
        let one = PromptDialog::new("t");
        let many = PromptDialog::new("t").multiline(true);
        assert_eq!(one.field_height(), field_height(false, 1));
        assert_eq!(many.field_height(), field_height(true, 3));
        assert!(many.field_height() > one.field_height());
    }

    #[test]
    fn a_prompt_opens_with_its_default_value_selected() {
        let p = PromptDialog::new("Renommer").with_value("Rapport.pdf");
        assert_eq!(p.field.text(), "Rapport.pdf");
        assert_eq!(p.field.selection_start(), 0);
        assert_eq!(p.field.selection_length(), "Rapport.pdf".chars().count() as i32);
    }

    #[test]
    fn a_folder_conflict_offers_to_merge_and_a_file_conflict_to_overwrite() {
        let file = ConflictDialog::new("Rapport.pdf", ConflictKind::File);
        let folder = ConflictDialog::new("Photos", ConflictKind::Folder);
        assert_eq!(file.options()[0].1, "Écraser");
        assert_eq!(folder.options()[0].1, "Fusionner");
        assert!(file.message().contains("fichier"));
        assert!(folder.message().contains("dossier"));
        // The rename hint quotes the name it will rename.
        assert!(file.options()[1].2.contains("Rapport.pdf (2)"));
        assert_eq!(file.options()[0].0, ConflictChoice::Overwrite);
        assert_eq!(file.options()[1].0, ConflictChoice::KeepBoth);
        // The third choice is the window's cancel, not a third row.
        assert!(file.actions.confirm.is_none());
        assert!(file.actions.cancel.is_some());
    }

    // ── Popover: the four edges, and the two differences from a tooltip ────

    const VIEW: Size = Size { width: 1000.0, height: 800.0 };
    const PANEL: Size = Size { width: 240.0, height: 200.0 };

    #[test]
    fn a_popover_sits_four_dip_off_its_anchor_not_fourteen() {
        let anchor = Rect::new(400.0, 300.0, 500.0, 340.0);
        let p = place_anchored(anchor, PANEL, Side::Bottom, Align::Start, VIEW);
        assert_eq!(p.side, Side::Bottom);
        assert_eq!(p.rect.top, anchor.bottom + POPOVER_GAP);
        // …and it is the tooltip's own gap, minus the pull.
        assert_eq!(p.rect.top, anchor.bottom + display::Tooltip::GAP - (display::Tooltip::GAP - POPOVER_GAP));

        let above = place_anchored(anchor, PANEL, Side::Top, Align::Start, VIEW);
        assert_eq!(above.rect.bottom, anchor.top - POPOVER_GAP);
        let right = place_anchored(anchor, PANEL, Side::Right, Align::Start, VIEW);
        assert_eq!(right.rect.left, anchor.right + POPOVER_GAP);
        let left = place_anchored(anchor, PANEL, Side::Left, Align::Start, VIEW);
        assert_eq!(left.rect.right, anchor.left - POPOVER_GAP);
    }

    #[test]
    fn a_popover_aligns_its_edge_with_the_anchors() {
        let anchor = Rect::new(400.0, 300.0, 500.0, 340.0);
        let start = place_anchored(anchor, PANEL, Side::Bottom, Align::Start, VIEW);
        assert_eq!(start.rect.left, anchor.left, "align='left'");
        let end = place_anchored(anchor, PANEL, Side::Bottom, Align::End, VIEW);
        assert_eq!(end.rect.right, anchor.right, "align='right'");
        let centre = place_anchored(anchor, PANEL, Side::Bottom, Align::Center, VIEW);
        assert_eq!(
            (centre.rect.left + centre.rect.right) / 2.0,
            (anchor.left + anchor.right) / 2.0,
            "the tooltip's own centring, still reachable"
        );
    }

    #[test]
    fn a_popover_flips_at_each_of_the_four_edges() {
        let cases = [
            (Rect::new(480.0, 0.0, 520.0, 20.0), Side::Top, Side::Bottom),
            (Rect::new(480.0, 780.0, 520.0, 800.0), Side::Bottom, Side::Top),
            (Rect::new(0.0, 380.0, 20.0, 420.0), Side::Left, Side::Right),
            (Rect::new(980.0, 380.0, 1000.0, 420.0), Side::Right, Side::Left),
        ];
        for (anchor, asked, want) in cases {
            let p = place_anchored(anchor, PANEL, asked, Align::Start, VIEW);
            assert_eq!(p.side, want, "anchor against the {asked:?} edge");
        }
    }

    #[test]
    fn a_popover_stays_inside_the_viewport_at_every_corner() {
        let corners = [
            Rect::new(0.0, 0.0, 20.0, 20.0),
            Rect::new(980.0, 0.0, 1000.0, 20.0),
            Rect::new(0.0, 780.0, 20.0, 800.0),
            Rect::new(980.0, 780.0, 1000.0, 800.0),
        ];
        let margin = display::Tooltip::MARGIN;
        for anchor in corners {
            for side in display::SIDES {
                for align in ALIGNMENTS {
                    let p = place_anchored(anchor, PANEL, side, align, VIEW);
                    assert!(p.rect.left >= margin - 0.01, "left escaped: {}", p.rect.left);
                    assert!(p.rect.top >= margin - 0.01, "top escaped: {}", p.rect.top);
                    assert!(
                        p.rect.right <= VIEW.width - margin + 0.01,
                        "right escaped: {}",
                        p.rect.right
                    );
                    assert!(
                        p.rect.bottom <= VIEW.height - margin + 0.01,
                        "bottom escaped: {}",
                        p.rect.bottom
                    );
                }
            }
        }
    }

    #[test]
    fn a_popover_keeps_the_size_it_asked_for() {
        let anchor = Rect::new(400.0, 300.0, 500.0, 340.0);
        for side in display::SIDES {
            let p = place_anchored(anchor, PANEL, side, Align::Start, VIEW);
            assert_eq!(p.rect.right - p.rect.left, PANEL.width);
            assert_eq!(p.rect.bottom - p.rect.top, PANEL.height);
        }
    }

    #[test]
    fn a_click_outside_a_popover_closes_it() {
        let p = Popover::new();
        let r = Rect::new(100.0, 100.0, 340.0, 400.0);
        assert!(p.dismisses(r, 10.0, 10.0));
        assert!(!p.dismisses(r, 120.0, 120.0));
    }

    // ── The toast stack ───────────────────────────────────────────────────

    const HOST: Rect = Rect { left: 0.0, top: 0.0, right: 1000.0, bottom: 800.0 };
    const CARD: Size = Size { width: 384.0, height: 64.0 };

    #[test]
    fn one_toast_sits_in_the_anchors_corner() {
        let r = stack(HOST, ToastPlacement::BottomRight, &[CARD]);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].right, HOST.right - TOAST_INSET);
        assert_eq!(r[0].bottom, HOST.bottom - TOAST_INSET);
        assert_eq!(r[0].right - r[0].left, CARD.width);

        let left = stack(HOST, ToastPlacement::BottomLeft, &[CARD]);
        assert_eq!(left[0].left, HOST.left + TOAST_INSET);
        let top = stack(HOST, ToastPlacement::TopRight, &[CARD]);
        assert_eq!(top[0].top, HOST.top + TOAST_INSET);
        let centre = stack(HOST, ToastPlacement::TopCenter, &[CARD]);
        assert_eq!(
            (centre[0].left + centre[0].right) / 2.0,
            (HOST.left + HOST.right) / 2.0
        );
    }

    #[test]
    fn three_toasts_stack_with_one_gap_between_them() {
        let r = stack(HOST, ToastPlacement::BottomRight, &[CARD, CARD, CARD]);
        assert_eq!(r.len(), 3);
        // Bottom-anchored: the NEWEST (last) is the lowest.
        assert_eq!(r[2].bottom, HOST.bottom - TOAST_INSET);
        assert_eq!(r[1].bottom, r[2].top - TOAST_STACK_GAP);
        assert_eq!(r[0].bottom, r[1].top - TOAST_STACK_GAP);
        assert!(r[0].top < r[1].top && r[1].top < r[2].top);

        // Top-anchored: the OLDEST is the highest, and they grow downward.
        let t = stack(HOST, ToastPlacement::TopRight, &[CARD, CARD, CARD]);
        assert_eq!(t[0].top, HOST.top + TOAST_INSET);
        assert_eq!(t[1].top, t[0].bottom + TOAST_STACK_GAP);
        assert_eq!(t[2].top, t[1].bottom + TOAST_STACK_GAP);
    }

    #[test]
    fn a_stack_past_its_maximum_drops_the_oldest() {
        let six = [CARD; 6];
        let r = stack(HOST, ToastPlacement::BottomRight, &six);
        assert_eq!(r.len(), TOAST_MAX, "only the newest four are placed");
        assert_eq!(r[TOAST_MAX - 1].bottom, HOST.bottom - TOAST_INSET);
        // …and they still fit above one another without overlapping.
        for w in r.windows(2) {
            assert!(w[0].bottom <= w[1].top, "two toasts overlapped");
        }
        assert!(stack(HOST, ToastPlacement::BottomRight, &[]).is_empty());
    }

    #[test]
    fn a_toast_never_grows_wider_than_its_host() {
        let narrow = Rect::new(0.0, 0.0, 300.0, 800.0);
        let r = stack(narrow, ToastPlacement::BottomRight, &[CARD]);
        assert_eq!(r[0].right - r[0].left, 300.0 - 2.0 * TOAST_INSET);
        assert!(r[0].left >= narrow.left);
    }

    #[test]
    fn a_danger_toast_lives_longer_than_the_others() {
        assert_eq!(Toast::error("échec").duration_ms, TOAST_MS_DANGER);
        assert_eq!(Toast::info("ok").duration_ms, TOAST_MS);
        assert_eq!(Toast::success("ok").duration_ms, TOAST_MS);
        assert_eq!(Toast::warning("hm").duration_ms, TOAST_MS);
        assert_eq!(Toast::error("échec").sticky().duration_ms, 0);
        // The message is the replica's `text`, not a field of our own.
        assert_eq!(Toast::info("Enregistré").text, "Enregistré");
    }

    #[test]
    fn a_toasts_close_button_is_hit_testable_at_its_corner() {
        let t = Toast::info("Enregistré");
        let bounds = Rect::new(0.0, 0.0, TOAST_WIDTH, 64.0);
        let c = t.close_rect(bounds);
        assert!(t.close_hit(bounds, c.left + 2.0, c.top + 2.0));
        assert!(!t.close_hit(bounds, bounds.left + 4.0, bounds.top + 4.0), "that is the glyph");
        assert!(c.right <= bounds.right, "the ✕ left the card");
    }

    // ── The keyboard: trap, Échap, Entrée, Espace ─────────────────────────

    const CONFIRM: DialogPart = DialogPart::Action(ActionId::Confirm);
    const CANCEL: DialogPart = DialogPart::Action(ActionId::Cancel);

    #[test]
    fn tab_cycles_inside_the_dialog_and_wraps_at_both_ends() {
        let d = ConfirmDialog::new("t", "m");
        let order = d.focus_order();
        assert_eq!(order, vec![DialogPart::Close, CONFIRM, CANCEL], "document order");
        assert_eq!(cycle_focus(&order, Some(CANCEL), true), Some(DialogPart::Close), "last → first");
        assert_eq!(cycle_focus(&order, Some(DialogPart::Close), false), Some(CANCEL), "first → last");
        assert_eq!(cycle_focus(&order, None, true), Some(DialogPart::Close));
        assert_eq!(cycle_focus(&order, None, false), Some(CANCEL));
        assert_eq!(cycle_focus(&[], None, true), None);
        // …and through the resolver, starting from the autofocused confirm.
        assert_eq!(d.key_command(DialogKey::Tab), Some(DialogCommand::Focus(CANCEL)));
        assert_eq!(d.key_command(DialogKey::ShiftTab), Some(DialogCommand::Focus(DialogPart::Close)));
    }

    #[test]
    fn a_confirmation_opens_on_its_confirming_action_without_a_ring() {
        let d = ConfirmDialog::danger("Supprimer", "…");
        assert_eq!(d.focus, Some(CONFIRM), "`autoFocus: true` on the confirm");
        assert!(!d.focus_visible, "a programmatic focus shows no ring");
        assert_eq!(d.key_command(DialogKey::Enter), Some(DialogCommand::Activate(CONFIRM)));
        assert_eq!(d.key_command(DialogKey::Escape), Some(DialogCommand::Dismiss));
    }

    #[test]
    fn entree_presses_the_focused_cancel_rather_than_confirming() {
        let mut d = ConfirmDialog::new("t", "m");
        d.set_focus(Some(CANCEL), true);
        assert_eq!(d.key_command(DialogKey::Enter), Some(DialogCommand::Activate(CANCEL)));
        assert_eq!(d.key_command(DialogKey::Space), Some(DialogCommand::Activate(CANCEL)));
        d.set_focus(Some(DialogPart::Close), true);
        assert_eq!(d.key_command(DialogKey::Enter), Some(DialogCommand::Activate(DialogPart::Close)));
    }

    #[test]
    fn keys_decode_and_foreign_chords_are_left_alone() {
        assert_eq!(DialogKey::from_vk(vk::ENTER, Modifiers::NONE), Some(DialogKey::Enter));
        assert_eq!(DialogKey::from_vk(vk::ENTER, Modifiers::SHIFT), Some(DialogKey::ShiftEnter));
        assert_eq!(DialogKey::from_vk(vk::ENTER, Modifiers::CTRL), None);
        assert_eq!(DialogKey::from_vk(vk::ESCAPE, Modifiers::SHIFT), Some(DialogKey::Escape));
        assert_eq!(DialogKey::from_vk(vk::TAB, Modifiers::SHIFT), Some(DialogKey::ShiftTab));
        assert_eq!(DialogKey::from_vk(vk::letter('a'), Modifiers::NONE), None);
    }

    #[test]
    fn a_prompt_opens_on_its_field_and_enter_submits_only_a_valid_value() {
        let mut p = PromptDialog::new("Nouveau dossier");
        assert_eq!(p.focus, Some(DialogPart::Field));
        assert_eq!(p.focus_order(), vec![DialogPart::Close, DialogPart::Field, CANCEL], "a disabled OK is skipped");
        assert_eq!(p.key_command(DialogKey::Enter), None, "empty: `submit` refuses");
        p.edit(FieldEdit::Insert("Photos".into()));
        assert_eq!(p.focus_order(), vec![DialogPart::Close, DialogPart::Field, CONFIRM, CANCEL]);
        assert_eq!(p.key_command(DialogKey::Enter), Some(DialogCommand::Activate(CONFIRM)));
        assert_eq!(p.key_command(DialogKey::Escape), Some(DialogCommand::Dismiss));
        // Single line: Maj+Entrée submits too; multiline: it is a new line.
        assert_eq!(p.key_command(DialogKey::ShiftEnter), Some(DialogCommand::Activate(CONFIRM)));
        let multi = PromptDialog::new("t").multiline(true).with_value("x");
        assert_eq!(multi.key_command(DialogKey::ShiftEnter), None);
        assert_eq!(
            FieldEdit::from_key(vk::ENTER, Modifiers::SHIFT, true),
            Some(FieldEdit::Insert("\n".into()))
        );
        assert_eq!(FieldEdit::from_key(vk::ENTER, Modifiers::SHIFT, false), None);
    }

    #[test]
    fn typing_replaces_the_initial_selection_then_appends() {
        let mut p = PromptDialog::new("Renommer").with_value("Rapport.pdf");
        assert!(p.edit(FieldEdit::Insert("Bilan".into())), "the selected default is replaced");
        assert_eq!(p.field.text(), "Bilan");
        p.edit(FieldEdit::Insert(" 2026".into()));
        assert_eq!(p.field.text(), "Bilan 2026");
        assert_eq!(p.caret(), 10);
        assert!(p.edit(FieldEdit::Backspace));
        assert_eq!(p.field.text(), "Bilan 202");
        assert!(p.edit(FieldEdit::BackspaceWord));
        assert_eq!(p.field.text(), "Bilan ");
        // A single-line field strips pasted line breaks.
        p.edit(FieldEdit::Insert("a\r\nb".into()));
        assert_eq!(p.field.text(), "Bilan ab");
    }

    #[test]
    fn arrows_move_and_extend_the_selection() {
        let mut p = PromptDialog::new("t").with_value("abcdef");
        assert!(!p.edit(FieldEdit::Left { extend: false, word: false }), "a move is not a change");
        assert_eq!((p.caret(), p.field.selection_length()), (0, 0), "← collapses to the start");
        p.edit(FieldEdit::Right { extend: true, word: false });
        p.edit(FieldEdit::Right { extend: true, word: false });
        assert_eq!(p.selected_text(), "ab");
        p.edit(FieldEdit::End { extend: true });
        assert_eq!(p.selected_text(), "abcdef");
        p.edit(FieldEdit::Home { extend: false });
        p.edit(FieldEdit::Delete);
        assert_eq!(p.field.text(), "bcdef");
        p.edit(FieldEdit::SelectAll);
        assert_eq!(p.selected_text(), "bcdef");
        p.edit(FieldEdit::Backspace);
        assert!(!p.can_confirm(), "emptied: OK disables again");
        assert!(p.actions.confirm.as_ref().is_some_and(|a| !a.enabled));
    }

    #[test]
    fn ctrl_arrows_jump_words() {
        assert_eq!(word_start("un deux trois", 13), 8);
        assert_eq!(word_start("un deux trois", 8), 3);
        assert_eq!(word_end("un deux trois", 0), 3);
        assert_eq!(word_end("un deux trois", 3), 8);
        assert_eq!(word_start("", 5), 0);
    }

    #[test]
    fn a_conflict_starts_on_cancel_and_its_rows_are_buttons() {
        let d = ConflictDialog::new("a.txt", ConflictKind::File);
        assert_eq!(d.focus, Some(CANCEL), "the least destructive choice");
        assert_eq!(
            d.focus_order(),
            vec![DialogPart::Close, DialogPart::Option(0), DialogPart::Option(1), CANCEL]
        );
        assert_eq!(d.key_command(DialogKey::Enter), Some(DialogCommand::Activate(CANCEL)));
        assert_eq!(d.choice_of(DialogPart::Option(1)), Some(ConflictChoice::KeepBoth));
        assert_eq!(d.choice_of(CANCEL), Some(ConflictChoice::Cancel));
        let mut d = d;
        d.set_focus(Some(DialogPart::Option(0)), true);
        assert_eq!(d.key_command(DialogKey::Space), Some(DialogCommand::Activate(DialogPart::Option(0))));
    }

    #[test]
    fn focus_ids_round_trip_and_are_distinct() {
        let d = ConflictDialog::new("a", ConflictKind::File);
        let order = d.focus_order();
        for p in &order {
            assert_eq!(DialogPart::from_focus_id(p.focus_id(), &order), Some(*p));
        }
        let mut ids: Vec<FocusId> = order.iter().map(|p| p.focus_id()).collect();
        ids.dedup();
        assert_eq!(ids.len(), order.len());
    }

    // ── Popover against the screen ────────────────────────────────────────

    #[test]
    fn a_popover_placed_in_an_offset_area_stays_inside_it() {
        // The monitor's work area in client coordinates starts left of and
        // above the window when the window is not at the screen's corner.
        let area = Rect::new(-300.0, -120.0, 1300.0, 780.0);
        let anchor = Rect::new(-280.0, 740.0, -200.0, 770.0); // bottom-left corner
        let p = place_anchored_in(anchor, PANEL, Side::Bottom, Align::Start, area);
        assert_eq!(p.side, Side::Top, "no room below: flips up");
        let m = display::Tooltip::MARGIN;
        assert!(p.rect.left >= area.left + m - 0.01 && p.rect.bottom <= area.bottom - m + 0.01);
        assert_eq!(p.rect.bottom, anchor.top - POPOVER_GAP);
        // In an area at the origin it is exactly `place_anchored`.
        let origin = Rect::new(0.0, 0.0, VIEW.width, VIEW.height);
        let a = Rect::new(400.0, 300.0, 500.0, 340.0);
        let (x, y) = (
            place_anchored_in(a, PANEL, Side::Bottom, Align::End, origin).rect,
            place_anchored(a, PANEL, Side::Bottom, Align::End, VIEW).rect,
        );
        assert_eq!((x.left, x.top, x.right, x.bottom), (y.left, y.top, y.right, y.bottom));
        // A popup's bounds carry the shadow, and rebasing undoes the offset.
        let b = surface_bounds(p.rect);
        assert_eq!(b.left, p.rect.left - SHADOW_PAD);
        let local = rebase(p.rect, b);
        assert_eq!((local.left, local.top), (SHADOW_PAD, SHADOW_PAD));
    }

    // ── The toast queue ───────────────────────────────────────────────────

    #[test]
    fn the_queue_drops_the_oldest_past_its_maximum_and_replaces_by_id() {
        let mut q = ToastQueue::new();
        let ids: Vec<u64> = (0..6).map(|i| q.push(Toast::info(format!("n°{i}")))).collect();
        assert_eq!(q.len(), TOAST_MAX);
        assert_eq!(q.items()[0].id, ids[2], "the two oldest went");
        // Same id: replaced, moved to the end, not stacked.
        q.push_with_id(ids[3], Toast::success("remplacé"));
        assert_eq!(q.len(), TOAST_MAX);
        assert_eq!(q.items().last().map(|t| t.id), Some(ids[3]));
        assert_eq!(q.items().iter().filter(|t| t.id == ids[3]).count(), 1);
        assert!(q.dismiss(ids[3]));
        assert!(!q.dismiss(ids[3]));
        let fresh = q.push(Toast::info("x"));
        assert!(!ids.contains(&fresh), "ids are never reused");
    }

    #[test]
    fn the_queue_expires_by_duration_pauses_under_the_pointer_and_keeps_sticky_ones() {
        let mut q = ToastQueue::new();
        let info = q.push(Toast::info("ok"));
        let danger = q.push(Toast::error("échec"));
        let sticky = q.push(Toast::error("à acquitter").sticky());
        assert!(q.tick(3900, false).is_empty());
        assert!(q.tick(5000, true).is_empty(), "paused: nothing counts down");
        assert_eq!(q.tick(100, false), vec![info], "4000 ms for info");
        assert_eq!(q.tick(2000, false), vec![danger], "6000 ms for danger");
        assert!(q.tick(u64::MAX, false).is_empty());
        assert_eq!(q.items().iter().map(|t| t.id).collect::<Vec<_>>(), vec![sticky]);
        assert!(!q.is_timed());
    }

    #[test]
    fn assertive_toasts_stack_after_the_polite_ones() {
        let mut q = ToastQueue::new();
        q.push(Toast::error("a"));
        q.push(Toast::success("b"));
        q.push(Toast::warning("c"));
        q.push(Toast::info("d"));
        let variants: Vec<ToastVariant> = q.display_order().iter().map(|&i| q.items()[i].toast.variant).collect();
        assert_eq!(
            variants,
            vec![ToastVariant::Success, ToastVariant::Info, ToastVariant::Danger, ToastVariant::Warning]
        );
    }

    #[test]
    fn a_toast_lists_its_action_before_its_close() {
        assert_eq!(Toast::info("x").focus_order(), vec![ToastPart::Close]);
        assert_eq!(Toast::info("x").with_action("Annuler").focus_order(), vec![ToastPart::Action, ToastPart::Close]);
    }

    // ── The trap every family in this crate is tested against ─────────────

    /// **No paint path here may read the model's own rectangle.** Reading
    /// `self.bounds` (through the deref to the replica) instead of the `bounds`
    /// ARGUMENT has been shipped three times in this codebase — `Label`,
    /// `PictureBox`, `LinkLabel` — and each time it painted the control near
    /// the window origin. `display.rs` greps itself for it; so does this file,
    /// because a dialog is the widget most likely to be painted somewhere other
    /// than where its model thinks it lives.
    #[test]
    fn no_paint_path_reads_the_models_own_rectangle() {
        const SOURCE: &str = include_str!("dialogs.rs");
        for (n, line) in SOURCE.lines().enumerate() {
            if line.trim_start().starts_with("#[cfg(test)]") {
                break;
            }
            let code = line.split("//").next().unwrap_or("");
            for needle in [".bounds", "control().bounds"] {
                assert!(
                    !code.contains(needle),
                    "line {}: `{}` reads the model's own rectangle — paint into the \
                     `bounds` ARGUMENT",
                    n + 1,
                    code.trim()
                );
            }
        }
    }
}
