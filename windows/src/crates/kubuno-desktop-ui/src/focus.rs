//! Keyboard focus: which control receives the keys, how Tab moves it, and
//! whether its ring shows.
//!
//! Web counterpart: the browser itself — sequential focus navigation (Tab /
//! Shift+Tab in DOM order), focus on `mousedown`, blur when clicking something
//! unfocusable, and the `:focus-visible` heuristic every `@kubuno/ui`
//! primitive styles its ring with (`focus-visible:ring-2` in
//! `core/frontend/src/ui/*.tsx`).
//!
//! The desktop has no DOM, so the "document order" is the **paint order**:
//! each frame, every focusable control calls [`FocusRing::register`] with a
//! stable id and the rectangle it was painted in. The manager keeps the
//! previous frame's list and resolves input against it at the START of the
//! next frame — the same "route against last frame's geometry" rule the
//! gallery's menus use, so a click is decided before anyone paints.
//!
//! ```ignore
//! use kubuno_desktop_ui::focus::{FocusRing, FocusOpts};
//! // Once per frame, before painting (the gallery does this for you):
//! ring.begin_frame(f);
//! // While painting, in visual order:
//! let st = ring.register("name", name_rect);            // -> FocusState
//! field.paint(c, name_rect, live.state(name_rect).focused(st.focused).focus_visible(st.visible));
//! if st.focused { let typed = kubuno_desktop_controls::host::take_text(); /* … */ }
//! // After painting:
//! ring.end_frame();
//! ```
//!
//! The keys it consumes from the host queue (see `kubuno_desktop_controls::host`):
//! `Tab` and `Shift+Tab` (unless the focused control registered with
//! [`FocusOpts::wants_tab`]), and `Escape` only when someone calls
//! [`FocusRing::take_escape`] (or when [`FocusRing::set_blur_on_escape`] is
//! on and nobody took it by [`FocusRing::end_frame`]).

use kubuno_desktop_controls::host::{self, vk, Frame, InputEvent, Modifiers};

use crate::{Rect, WidgetState};

/// A focusable control's identity: stable across frames, cheap to compare.
///
/// Built from a string (`"name".into()`), a string plus an index for rows of
/// a list (`("row", 3).into()`), or a raw `u64`. Strings are hashed (FNV-1a),
/// so ids must be unique within a page, not globally meaningful.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FocusId(pub u64);

impl FocusId {
    /// FNV-1a of `s` — `const`, so an id can be a `const`.
    pub const fn of(s: &str) -> Self {
        let b = s.as_bytes();
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut i = 0;
        while i < b.len() {
            h ^= b[i] as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
            i += 1;
        }
        FocusId(h)
    }

    /// `base` followed by an index — one id per row, cell or item.
    pub const fn indexed(base: &str, index: usize) -> Self {
        let FocusId(h) = Self::of(base);
        // Mix the index in with a second FNV round over its bytes.
        let mut h = h ^ 0xff;
        let mut n = index as u64;
        let mut k = 0;
        while k < 8 {
            h ^= n & 0xff;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
            n >>= 8;
            k += 1;
        }
        FocusId(h)
    }
}

impl From<&str> for FocusId {
    fn from(s: &str) -> Self {
        FocusId::of(s)
    }
}

impl From<(&str, usize)> for FocusId {
    fn from((s, i): (&str, usize)) -> Self {
        FocusId::indexed(s, i)
    }
}

impl From<u64> for FocusId {
    fn from(v: u64) -> Self {
        FocusId(v)
    }
}

/// How a control takes part in focus navigation.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FocusOpts {
    /// The control handles `Tab` itself while focused (a code editor
    /// indenting): the manager then leaves `Tab`/`Shift+Tab` in the queue.
    pub wants_tab: bool,
    /// The ring shows even when the focus came from the pointer — the web's
    /// `:focus-visible` matches text inputs (anything taking typed text) on
    /// click. Set it for text fields, text areas, editable combos.
    pub always_visible: bool,
    /// Reachable by click and programmatically, but skipped by Tab — the
    /// web's `tabindex="-1"` (a menu item managed by arrow keys, a
    /// roving-tabindex row that is not the current one).
    pub skip_tab: bool,
}

impl FocusOpts {
    /// For a control that takes typed text: `always_visible`.
    pub const TEXT: Self = Self { wants_tab: false, always_visible: true, skip_tab: false };
}

/// Why the focus moved — what an event layer on top of the ring needs to raise the
/// WinForms focus sequences in the right order (`Enter → GotFocus` then, on the old
/// control, `Leave → Validating → Validated → LostFocus` for the keyboard, or
/// `LostFocus → Leave → Validating → Validated` for the pointer).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FocusCause {
    /// A pointer press (left or right) on a control, or on nothing (blur).
    Pointer,
    /// Tab / Shift+Tab, or [`FocusRing::step`].
    Keyboard,
    /// [`FocusRing::focus`], [`FocusRing::focus_visibly`], [`FocusRing::blur`],
    /// or an Escape that blurred.
    Program,
    /// The focused control stopped registering (it was removed or hidden).
    Removed,
}

/// One focus move, recorded by the ring in the order it happened. Read (and
/// cleared) with [`FocusRing::take_changes`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FocusChange {
    pub from: Option<FocusId>,
    pub to: Option<FocusId>,
    pub cause: FocusCause,
}

/// What [`FocusRing::register`] tells the control about itself this frame.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FocusState {
    /// It holds the focus — it reads the keys.
    pub focused: bool,
    /// Its ring must show (`focused && :focus-visible`).
    pub visible: bool,
    /// It GAINED the focus at the start of this frame (click, Tab or
    /// [`FocusRing::focus`]) — where a field selects its text on Tab-in.
    pub gained: bool,
}

impl FocusState {
    /// Folds this into a [`WidgetState`]: sets `focused` and `focus_visible`.
    pub fn apply(self, s: WidgetState) -> WidgetState {
        s.focused(self.focused).focus_visible(self.visible)
    }
}

#[derive(Clone)]
struct Entry {
    id:   FocusId,
    rect: Rect,
    opts: FocusOpts,
    /// The tab indexes of the containers it is in, outermost first, then its own — empty unless a
    /// page declares tab indexes ([`FocusRing::push_tab_index`]). Tab follows these keys.
    tab_key: Vec<i32>,
}

/// The focus manager of one page (or one window). Plain data — keep it in the
/// page's state, or a `thread_local!`, and drive it with
/// [`FocusRing::begin_frame`] / [`FocusRing::end_frame`] around the paint.
#[derive(Default)]
pub struct FocusRing {
    focused: Option<FocusId>,
    /// The last interaction was the keyboard (`:focus-visible` modality).
    keyboard: bool,
    /// `focused` changed during this frame's `begin_frame` / `focus`.
    gained: bool,
    /// Last frame's registrations, in paint order.
    prev: Vec<Entry>,
    /// This frame's registrations so far.
    cur: Vec<Entry>,
    /// Last frame's / this frame's regions where a click keeps the focus.
    keep_prev: Vec<Rect>,
    keep_cur: Vec<Rect>,
    /// An Escape key-down is waiting in this frame's queue.
    escape_pending: bool,
    blur_on_escape: bool,
    prev_down: bool,
    prev_right: bool,
    window_active: bool,
    /// Every focus move since the last [`FocusRing::take_changes`], in order.
    changes: Vec<FocusChange>,
    /// The tab indexes in force while painting (see [`FocusRing::push_tab_index`]).
    tab_path: Vec<i32>,
}

impl FocusRing {
    pub fn new() -> Self {
        Self { window_active: true, ..Self::default() }
    }

    /// Resolves the frame's input against LAST frame's registrations, in this
    /// order: pointer press (left or right, rising edge) → focus the topmost
    /// registered control under it, or blur when the press lands on nothing
    /// focusable and outside every [`FocusRing::keep_focus_in`] region;
    /// `Tab` / `Shift+Tab` → next / previous in paint order (wrapping; from
    /// nothing, the first / last), consumed from the host queue; any other
    /// key-down without Ctrl/Alt switches to keyboard modality (the ring
    /// shows). Then starts collecting this frame's registrations.
    pub fn begin_frame(&mut self, f: &Frame) {
        // Last frame's registrations become the geometry input is resolved
        // against; this frame's start empty.
        std::mem::swap(&mut self.prev, &mut self.cur);
        self.cur.clear();
        std::mem::swap(&mut self.keep_prev, &mut self.keep_cur);
        self.keep_cur.clear();

        self.gained = false;
        self.window_active = f.window_focused;
        let pressed = (f.mouse_down && !self.prev_down) || (f.right_down && !self.prev_right);
        self.prev_down = f.mouse_down;
        self.prev_right = f.right_down;

        if pressed {
            self.keyboard = false;
            let (x, y) = f.mouse;
            // Topmost = painted last.
            let hit = self.prev.iter().rev().find(|e| e.rect.contains(x, y)).map(|e| e.id);
            match hit {
                Some(id) => self.set(Some(id), FocusCause::Pointer),
                None => {
                    if !self.keep_prev.iter().any(|r| r.contains(x, y)) {
                        self.set(None, FocusCause::Pointer);
                    }
                }
            }
        }

        // Tab navigation, unless the focused control keeps Tab for itself.
        let focused_wants_tab = self
            .focused
            .and_then(|id| self.prev.iter().find(|e| e.id == id))
            .is_some_and(|e| e.opts.wants_tab);
        if !focused_wants_tab {
            let fwd = host::take_key(vk::TAB, Modifiers::NONE);
            let back = host::take_key(vk::TAB, Modifiers::SHIFT);
            for _ in 0..fwd {
                self.step(true);
            }
            for _ in 0..back {
                self.step(false);
            }
        }

        // Keyboard modality: any real key (not a lone modifier, not a
        // Ctrl/Alt shortcut) shows the ring, like Chromium's heuristic.
        self.escape_pending = false;
        for e in host::events() {
            if let InputEvent::Key { vk: k, down: true, mods, .. } = e {
                if k == vk::ESCAPE {
                    self.escape_pending = true;
                }
                let modifier_key = matches!(k, vk::SHIFT | vk::CONTROL | vk::MENU | vk::LWIN | vk::RWIN);
                if !modifier_key && !mods.ctrl && !mods.alt {
                    self.keyboard = true;
                }
            }
        }

    }


    /// Registers a focusable control painted at `rect` this frame, in paint
    /// order, and returns its focus state. Do not register a disabled control
    /// (the web skips disabled elements too).
    pub fn register(&mut self, id: impl Into<FocusId>, rect: Rect) -> FocusState {
        self.register_with(id, rect, FocusOpts::default())
    }

    /// [`FocusRing::register`] with options.
    pub fn register_with(&mut self, id: impl Into<FocusId>, rect: Rect, opts: FocusOpts) -> FocusState {
        let id = id.into();
        // Kept in CLIENT coordinates: next frame's presses are resolved against
        // the raw pointer, while `rect` is in the (possibly scrolled) content's.
        let rect = to_client(rect);
        self.cur.push(Entry { id, rect, opts, tab_key: self.tab_path.clone() });
        self.state_with(id, opts)
    }

    /// How many controls registered so far this frame — a mark for [`FocusRing::remove_since`] and
    /// [`FocusRing::skip_tab_since`], taken before painting a part of the page.
    pub fn mark(&self) -> usize {
        self.cur.len()
    }

    /// Unregisters every control registered since `mark`: a disabled part of the page (WinForms'
    /// `Enabled = false` on a container). A focused one among them loses the focus at
    /// [`FocusRing::end_frame`], like a removed control.
    pub fn remove_since(&mut self, mark: usize) {
        self.cur.truncate(mark.min(self.cur.len()));
    }

    /// Cuts the rectangles of the controls registered since `mark` to `clip` (client coordinates): a part of the
    /// page its container clips (WinForms clips a control to its parent), whose hidden part a click must not focus.
    pub fn clip_since(&mut self, mark: usize, clip: Rect) {
        for e in self.cur.iter_mut().skip(mark) {
            let left = e.rect.left.max(clip.left);
            let top = e.rect.top.max(clip.top);
            e.rect = Rect::new(left, top, e.rect.right.min(clip.right).max(left), e.rect.bottom.min(clip.bottom).max(top));
        }
    }

    /// Takes the controls registered as `id` since `mark` out of the Tab order (`TabStop = false`):
    /// still focused by a click or programmatically.
    pub fn skip_tab_since(&mut self, mark: usize, id: impl Into<FocusId>) {
        let id = id.into();
        for e in self.cur.iter_mut().skip(mark).filter(|e| e.id == id) {
            e.opts.skip_tab = true;
        }
    }

    /// Enters a part of the page whose position in the Tab order is `index` among its siblings
    /// (WinForms' `TabIndex`): Tab visits the controls registered until the matching
    /// [`FocusRing::pop_tab_index`] in the order of these indexes (a container's index orders its
    /// whole content), paint order breaking ties. A page that never calls it keeps the plain paint
    /// order.
    pub fn push_tab_index(&mut self, index: i32) {
        self.tab_path.push(index);
    }

    /// Leaves the part entered by [`FocusRing::push_tab_index`].
    pub fn pop_tab_index(&mut self) {
        self.tab_path.pop();
    }

    /// A region (an open menu, a popover) where a press keeps the current
    /// focus even though nothing focusable is under it. Valid for the next
    /// frame's `begin_frame`, like a registration.
    pub fn keep_focus_in(&mut self, rect: Rect) {
        self.keep_cur.push(to_client(rect));
    }

    /// The focus state of `id`, without registering it.
    pub fn state(&self, id: impl Into<FocusId>) -> FocusState {
        let id = id.into();
        let opts = self
            .cur
            .iter()
            .chain(self.prev.iter())
            .find(|e| e.id == id)
            .map(|e| e.opts)
            .unwrap_or_default();
        self.state_with(id, opts)
    }

    fn state_with(&self, id: FocusId, opts: FocusOpts) -> FocusState {
        let focused = self.focused == Some(id);
        FocusState {
            focused,
            visible: focused && (self.keyboard || opts.always_visible),
            gained: focused && self.gained,
        }
    }

    /// Whether `id` holds the focus.
    pub fn is_focused(&self, id: impl Into<FocusId>) -> bool {
        self.focused == Some(id.into())
    }

    /// The focused control, if any.
    pub fn focused(&self) -> Option<FocusId> {
        self.focused
    }

    /// Whether the last interaction was the keyboard — the page-wide half of
    /// `:focus-visible`.
    pub fn focus_visible(&self) -> bool {
        self.keyboard
    }

    /// Whether the host window has the keyboard focus. A caret hides (and
    /// stops blinking) when it does not; the focused id is kept, as the
    /// browser keeps `document.activeElement` across a window blur.
    pub fn window_active(&self) -> bool {
        self.window_active
    }

    /// Programmatic focus (`element.focus()`): the ring's visibility follows
    /// the current modality, as in the browser. Takes effect immediately, for
    /// the rest of this frame.
    pub fn focus(&mut self, id: impl Into<FocusId>) {
        self.set(Some(id.into()), FocusCause::Program);
    }

    /// Programmatic focus that always shows the ring (`focus({focusVisible:
    /// true})`) — e.g. returning focus to a trigger after closing its menu
    /// with Escape.
    pub fn focus_visibly(&mut self, id: impl Into<FocusId>) {
        self.keyboard = true;
        self.set(Some(id.into()), FocusCause::Program);
    }

    /// Drops the focus (`element.blur()`).
    pub fn blur(&mut self) {
        self.set(None, FocusCause::Program);
    }

    /// Moves the focus to the next (`forward`) or previous tabbable control of
    /// the last frame's order, wrapping — what Tab does. Shows the ring.
    pub fn step(&mut self, forward: bool) {
        self.keyboard = true;
        let mut tabbable: Vec<&Entry> = self.prev.iter().filter(|e| !e.opts.skip_tab).collect();
        // Stable: equal keys (no tab index anywhere, the usual case) keep the paint order.
        tabbable.sort_by(|a, b| compare_tab_keys(&a.tab_key, &b.tab_key));
        let order: Vec<FocusId> = tabbable.iter().map(|e| e.id).collect();
        if order.is_empty() {
            return;
        }
        let n = order.len();
        let next = match self.focused.and_then(|id| order.iter().position(|&o| o == id)) {
            Some(i) if forward => (i + 1) % n,
            Some(i) => (i + n - 1) % n,
            None if forward => 0,
            None => n - 1,
        };
        self.set(Some(order[next]), FocusCause::Keyboard);
    }

    /// Consumes this frame's Escape key-down (any modifiers) from the host
    /// queue and returns whether there was one — the hook a focused control
    /// (or an open menu) uses to act on Escape. The first caller wins.
    pub fn take_escape(&mut self) -> bool {
        if !self.escape_pending {
            return false;
        }
        self.escape_pending = false;
        !host::take_key_any(vk::ESCAPE).is_empty()
    }

    /// When on, an Escape that nobody took by [`FocusRing::end_frame`] blurs
    /// the focused control. Off by default (the browser does not blur on
    /// Escape).
    pub fn set_blur_on_escape(&mut self, on: bool) {
        self.blur_on_escape = on;
    }

    /// Ends the frame: a focused control that did not register this frame
    /// (it was removed or hidden) loses the focus, like a removed DOM node;
    /// an untaken Escape blurs when [`FocusRing::set_blur_on_escape`] is on.
    pub fn end_frame(&mut self) {
        if self.blur_on_escape && self.take_escape() {
            self.set(None, FocusCause::Program);
        }
        if let Some(id) = self.focused {
            if !self.cur.iter().any(|e| e.id == id) {
                self.focused = None;
                self.record(FocusChange { from: Some(id), to: None, cause: FocusCause::Removed });
            }
        }
    }

    /// Every focus move since the last call, oldest first, and forgets them —
    /// what an event layer (`kubuno_desktop_views`' input router) reads once per frame,
    /// right after [`FocusRing::begin_frame`], to raise Enter/GotFocus/Leave/
    /// Validating/Validated/LostFocus. Moves made while painting (a control
    /// calling [`FocusRing::focus`]) and the removal [`FocusRing::end_frame`]
    /// detects are reported by the next frame's call. A page that never reads
    /// them loses nothing: the list is capped (the oldest moves are dropped).
    pub fn take_changes(&mut self) -> Vec<FocusChange> {
        std::mem::take(&mut self.changes)
    }

    /// Puts the focus back on `id` WITHOUT recording a move — a validation that
    /// was cancelled (WinForms' `CancelEventArgs.Cancel` on `Validating`) keeps
    /// the focus where it was, as if it had never left.
    pub fn restore(&mut self, id: Option<FocusId>) {
        self.focused = id;
        self.gained = false;
    }

    fn record(&mut self, change: FocusChange) {
        if self.changes.len() >= MAX_PENDING_CHANGES {
            self.changes.remove(0);
        }
        self.changes.push(change);
    }

    /// Forgets everything (a page switch): no focus, no registrations.
    pub fn reset(&mut self) {
        let active = self.window_active;
        *self = Self::new();
        self.window_active = active;
    }

    fn set(&mut self, id: Option<FocusId>, cause: FocusCause) {
        if self.focused != id {
            let from = self.focused;
            self.focused = id;
            self.gained = id.is_some();
            self.record(FocusChange { from, to: id, cause });
        }
    }
}

/// Orders two tab keys (see [`FocusRing::push_tab_index`]) index by index, a missing index counting
/// as 0 (WinForms' default `TabIndex`).
fn compare_tab_keys(a: &[i32], b: &[i32]) -> std::cmp::Ordering {
    for i in 0..a.len().max(b.len()) {
        let (x, y) = (a.get(i).copied().unwrap_or(0), b.get(i).copied().unwrap_or(0));
        if x != y {
            return x.cmp(&y);
        }
    }
    std::cmp::Ordering::Equal
}

/// How many unread [`FocusChange`]s the ring keeps (a page that never calls
/// [`FocusRing::take_changes`] must not grow the list forever).
const MAX_PENDING_CHANGES: usize = 64;

/// Half-period of the text caret's blink, in ms — the Windows default
/// `GetCaretBlinkTime` (Chromium on Windows follows the system setting).
pub const CARET_BLINK_MS: u64 = 530;

/// Whether a blinking caret is in its visible half right now, counting from
/// `last_input_ms` (a [`host::now_ms`] reading taken at the last edit or caret
/// move, so the caret stays solid while the user types). Schedules the repaint
/// of the next toggle through [`host::request_repaint_after`], so calling it
/// every frame the caret shows is all a field has to do. Call it only while
/// the field is focused and the window active.
pub fn caret_visible(last_input_ms: u64) -> bool {
    let elapsed = host::now_ms().saturating_sub(last_input_ms);
    let phase = elapsed % (2 * CARET_BLINK_MS);
    let to_toggle = CARET_BLINK_MS - (phase % CARET_BLINK_MS);
    host::request_repaint_after(to_toggle as u32);
    phase < CARET_BLINK_MS
}

/// `rect`, painted in the current (possibly scrolled) content coordinates, in
/// client coordinates.
fn to_client(rect: Rect) -> Rect {
    let (dx, dy) = host::content_offset();
    Rect::new(rect.left + dx, rect.top + dy, rect.right + dx, rect.bottom + dy)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(mouse: (f32, f32), down: bool) -> Frame {
        Frame {
            size: (800.0, 600.0),
            mouse,
            mouse_down: down,
            right_down: false,
            middle_down: false,
            dismiss: false,
            scale: 1.0,
            client_origin: (0.0, 0.0),
            work_area: (0.0, 0.0, 800.0, 600.0),
            chrome_top: 0.0,
            mods: Modifiers::NONE,
            wheel: (0.0, 0.0),
            click_count: 0,
            window_focused: true,
        }
    }

    fn paint(r: &mut FocusRing) {
        r.register("a", Rect::new(0.0, 0.0, 10.0, 10.0));
        r.register_with("b", Rect::new(20.0, 0.0, 30.0, 10.0), FocusOpts::TEXT);
        r.register_with("skip", Rect::new(40.0, 0.0, 50.0, 10.0), FocusOpts { skip_tab: true, ..Default::default() });
        r.register("c", Rect::new(60.0, 0.0, 70.0, 10.0));
    }

    #[test]
    fn click_focuses_and_hides_ring_except_text() {
        let mut r = FocusRing::new();
        r.begin_frame(&frame((0.0, 0.0), false));
        paint(&mut r);
        r.end_frame();
        r.begin_frame(&frame((5.0, 5.0), true));
        assert!(r.is_focused("a"));
        let s = r.register("a", Rect::new(0.0, 0.0, 10.0, 10.0));
        assert!(s.focused && !s.visible && s.gained);
        // Clicking the text field: focused and visible on pointer.
        r.begin_frame(&frame((5.0, 5.0), false));
        paint(&mut r);
        r.end_frame();
        r.begin_frame(&frame((25.0, 5.0), true));
        paint(&mut r);
        assert!(r.state("b").visible);
        r.end_frame();
        // Clicking nothing blurs.
        r.begin_frame(&frame((500.0, 500.0), false));
        paint(&mut r);
        r.end_frame();
        r.begin_frame(&frame((500.0, 500.0), true));
        assert_eq!(r.focused(), None);
    }

    #[test]
    fn step_skips_skip_tab_and_wraps() {
        let mut r = FocusRing::new();
        r.begin_frame(&frame((0.0, 0.0), false));
        paint(&mut r);
        r.end_frame();
        r.begin_frame(&frame((0.0, 0.0), false));
        r.step(true);
        assert!(r.is_focused("a"));
        r.step(true);
        assert!(r.is_focused("b"));
        r.step(true);
        assert!(r.is_focused("c"));
        r.step(true);
        assert!(r.is_focused("a"));
        r.step(false);
        assert!(r.is_focused("c"));
        assert!(r.focus_visible());
    }

    #[test]
    fn unregistered_focus_is_dropped() {
        let mut r = FocusRing::new();
        r.begin_frame(&frame((0.0, 0.0), false));
        paint(&mut r);
        r.end_frame();
        r.begin_frame(&frame((0.0, 0.0), false));
        r.focus("c");
        r.register("a", Rect::new(0.0, 0.0, 10.0, 10.0));
        r.end_frame();
        assert_eq!(r.focused(), None);
    }

    #[test]
    fn changes_are_recorded_with_their_cause_and_restore_records_nothing() {
        let mut r = FocusRing::new();
        r.begin_frame(&frame((0.0, 0.0), false));
        paint(&mut r);
        r.end_frame();
        assert!(r.take_changes().is_empty());
        // Pointer.
        r.begin_frame(&frame((5.0, 5.0), true));
        assert_eq!(r.take_changes(), vec![FocusChange { from: None, to: Some(FocusId::of("a")), cause: FocusCause::Pointer }]);
        paint(&mut r);
        r.end_frame();
        // Keyboard, then program.
        r.begin_frame(&frame((5.0, 5.0), false));
        r.step(true);
        r.focus("c");
        let changes = r.take_changes();
        assert_eq!(changes.len(), 2);
        assert_eq!(
            (changes[0].from, changes[0].to, changes[0].cause),
            (Some(FocusId::of("a")), Some(FocusId::of("b")), FocusCause::Keyboard)
        );
        assert_eq!((changes[1].to, changes[1].cause), (Some(FocusId::of("c")), FocusCause::Program));
        // A cancelled validation puts the focus back silently.
        r.restore(Some(FocusId::of("b")));
        assert!(r.is_focused("b"));
        assert!(r.take_changes().is_empty());
        // Removal is reported by `end_frame`.
        r.register("a", Rect::new(0.0, 0.0, 10.0, 10.0));
        r.end_frame();
        assert_eq!(r.take_changes(), vec![FocusChange { from: Some(FocusId::of("b")), to: None, cause: FocusCause::Removed }]);
    }

    /// `TabIndex`: Tab follows the indexes (a container's orders its content), paint order breaking
    /// ties; a missing index is 0.
    #[test]
    fn tab_follows_the_tab_indexes() {
        let mut r = FocusRing::new();
        r.begin_frame(&frame((-1.0, -1.0), false));
        // Paint order: a (index 2), then a container (index 1) holding b (0) and c (5), then d (none).
        r.push_tab_index(2);
        r.register("a", Rect::new(0.0, 0.0, 10.0, 10.0));
        r.pop_tab_index();
        r.push_tab_index(1);
        r.push_tab_index(5);
        r.register("c", Rect::new(20.0, 0.0, 30.0, 10.0));
        r.pop_tab_index();
        r.push_tab_index(0);
        r.register("b", Rect::new(40.0, 0.0, 50.0, 10.0));
        r.pop_tab_index();
        r.pop_tab_index();
        r.register("d", Rect::new(60.0, 0.0, 70.0, 10.0));
        r.end_frame();
        r.begin_frame(&frame((-1.0, -1.0), false));
        let mut order = Vec::new();
        for _ in 0..4 {
            r.step(true);
            order.push(r.focused());
        }
        let ids: Vec<_> = ["d", "b", "c", "a"].iter().map(|s| Some(FocusId::of(s))).collect();
        assert_eq!(order, ids);
    }

    /// A disabled part of the page leaves the ring; `TabStop = false` only leaves the Tab order.
    #[test]
    fn a_disabled_part_is_unregistered_and_a_tab_stop_skipped() {
        let paint = |r: &mut FocusRing| {
            r.register("a", Rect::new(0.0, 0.0, 10.0, 10.0));
            let mark = r.mark();
            r.register("b", Rect::new(20.0, 0.0, 30.0, 10.0));
            r.register("c", Rect::new(40.0, 0.0, 50.0, 10.0));
            r.remove_since(mark);
            let mark = r.mark();
            r.register("d", Rect::new(60.0, 0.0, 70.0, 10.0));
            r.skip_tab_since(mark, "d");
            r.end_frame();
        };
        let mut r = FocusRing::new();
        r.begin_frame(&frame((-1.0, -1.0), false));
        paint(&mut r);
        r.begin_frame(&frame((25.0, 5.0), true));
        assert_eq!(r.focused(), None, "a click on a disabled control focuses nothing");
        paint(&mut r);
        r.begin_frame(&frame((65.0, 5.0), false));
        paint(&mut r);
        r.begin_frame(&frame((65.0, 5.0), true));
        assert!(r.is_focused("d"), "a control out of the Tab order still takes a click");
        r.step(true);
        assert!(r.is_focused("a"), "Tab skips it");
    }

    #[test]
    fn ids() {
        assert_eq!(FocusId::from("x"), FocusId::of("x"));
        assert_ne!(FocusId::indexed("row", 1), FocusId::indexed("row", 2));
        assert_ne!(FocusId::indexed("row", 0), FocusId::of("row"));
    }
}
