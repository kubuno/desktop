//! Popups: a control — typically a user control — shown in a floating panel window of its own,
//! anchored to a control of another window, so it may extend beyond that window's edges (the web's
//! popover, which is never clipped by its container).
//!
//! ```ignore
//! let menu = Custom::<WaffleMenu>::new().dock(DockStyle::Fill);
//! let popup = Popup::new(&menu, 360.0, 580.0).placement(Placement::BottomEnd).gap(4.0);
//! popup.show(owner_hwnd, anchor_bounds);   // the anchor's bounds in the owner's client DIP
//! ```
//!
//! The window is a `WindowKind::Flyout` form built in code: borderless, its corners rounded at
//! [`Popup::corner_radius`] over a blur of what is behind it (`kubuno-desktop-controls::host::backdrop`), with
//! its shadow margin, owned by the anchor's window (above it, top-most when it is), closing when it
//! loses the focus (a click outside, another window activated) and on Escape. It takes the keyboard
//! focus when it opens; Windows gives it back to the owner when it closes.
//!
//! [`place`] decides where it goes: next to the anchor on the side [`Placement`] names, flipped to the
//! other side when that one has no room for it, then clamped into the work area of the anchor's
//! monitor (the screen less the taskbar), so it never leaves the screen.

use kubuno_desktop_ui::Rect;
use kubuno_desktop_views::events::KeyEventArgs;

use crate::forms::{AsControl, Form, WindowKind};
use crate::view::View;

/// Where a popup goes against its anchor: below or above it, aligned on the anchor's start (left),
/// end (right) or centre.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Placement {
    BottomStart,
    /// Below the anchor, right edges aligned (the web's `align="end"`): a header's menus.
    #[default]
    BottomEnd,
    BottomCenter,
    TopStart,
    TopEnd,
    TopCenter,
}

impl Placement {
    fn below(self) -> bool {
        matches!(self, Placement::BottomStart | Placement::BottomEnd | Placement::BottomCenter)
    }

    /// Its name in a view (`PopupPlacement="BottomEnd"`).
    pub fn parse(name: &str) -> Option<Self> {
        Some(match name.trim() {
            "BottomStart" => Placement::BottomStart,
            "BottomEnd" => Placement::BottomEnd,
            "BottomCenter" => Placement::BottomCenter,
            "TopStart" => Placement::TopStart,
            "TopEnd" => Placement::TopEnd,
            "TopCenter" => Placement::TopCenter,
            _ => return None,
        })
    }
}

/// Where a popup of `size` (DIP) goes against `anchor`, `gap` DIP away from it, inside `work` (all in
/// screen DIP): its top-left corner. The side `placement` names is kept when the popup fits there,
/// else the other side is taken when it has more room; the result is then clamped into `work` (a
/// popup taller or wider than `work` starts at its top or left edge).
pub fn place(anchor: Rect, size: (f32, f32), work: Rect, placement: Placement, gap: f32) -> (f32, f32) {
    let (w, h) = size;
    let x = match placement {
        Placement::BottomStart | Placement::TopStart => anchor.left,
        Placement::BottomEnd | Placement::TopEnd => anchor.right - w,
        Placement::BottomCenter | Placement::TopCenter => (anchor.left + anchor.right - w) / 2.0,
    };
    let below = anchor.bottom + gap;
    let above = anchor.top - gap - h;
    let room_below = work.bottom - below;
    let room_above = anchor.top - gap - work.top;
    let y = if placement.below() {
        if room_below >= h || room_below >= room_above { below } else { above }
    } else if room_above >= h || room_above >= room_below {
        above
    } else {
        below
    };
    let clamp = |v: f32, lo: f32, hi: f32| if hi < lo { lo } else { v.clamp(lo, hi) };
    (clamp(x, work.left, work.right - w), clamp(y, work.top, work.bottom - h))
}

/// The room a popup placed below `anchor` (`gap` away) has down to the bottom of `work` — the
/// height a tall popup is capped at.
pub fn room_below(anchor: Rect, work: Rect, gap: f32) -> f32 {
    (work.bottom - anchor.bottom - gap).max(0.0)
}

/// `anchor` (in the client DIP of the window `owner`, a raw `HWND` value) on screen, with the work
/// area of that window's monitor: `(anchor, work)` in screen DIP. `None` when `owner` is no window.
pub fn anchor_on_screen(owner: isize, anchor: Rect) -> Option<(Rect, Rect)> {
    let g = kubuno_desktop_controls::host::screen_geometry(owner)?;
    let (ox, oy) = g.client_origin;
    let (l, t, r, b) = g.work_area;
    Some((Rect::new(ox + anchor.left, oy + anchor.top, ox + anchor.right, oy + anchor.bottom), Rect::new(l, t, r, b)))
}

/// The window of the frame being handled — the window a control's event handler runs in — as a
/// raw `HWND` value (`0` when there is none).
pub fn current_window() -> isize {
    kubuno_desktop_controls::host::main_window().map_or(0, |h| h.0 as isize)
}

/// What Escape does in a popup: `true` closes it (the default), `false` lets the content keep it
/// open (it abandoned an edit first, say).
pub type EscapeFn = Box<dyn FnMut() -> bool>;

/// A popup (see the module doc). Build it, then [`Popup::show`] it; its [`Popup::form`] stays
/// usable meanwhile (resize it with `set_client_size`, close it with `close`).
pub struct Popup {
    form: Form,
    size: (f32, f32),
    placement: Placement,
    gap: f32,
    escape: Option<EscapeFn>,
}

impl Popup {
    /// A popup `width × height` DIP holding `content` (docked to fill it).
    pub fn new(content: &impl AsControl, width: f32, height: f32) -> Self {
        let form = Form::new().client_size(width, height).window_kind(WindowKind::Flyout).property("CornerRadius", 28.0).property("KeyPreview", true);
        form.controls().add(content);
        Self { form, size: (width, height), placement: Placement::BottomEnd, gap: 4.0, escape: None }
    }

    /// Its corners' radius (28 by default: the header's panels).
    pub fn corner_radius(self, radius: f32) -> Self {
        self.form.root().set_property("CornerRadius", radius);
        self
    }

    /// Its ground under the content, over the blur (`#E9EEF6CC`: the tint of the header's panels).
    pub fn back_color(self, color: &str) -> Self {
        self.form.root().set_property("BackColor", color);
        self
    }

    /// Its title, as a screen reader names its window.
    pub fn title(self, title: &str) -> Self {
        self.form.set_text(title);
        self
    }

    /// Where it goes against its anchor ([`Placement::BottomEnd`] by default).
    pub fn placement(mut self, placement: Placement) -> Self {
        self.placement = placement;
        self
    }

    /// How far from its anchor it goes (4 DIP by default: the web's `sideOffset={4}`).
    pub fn gap(mut self, gap: f32) -> Self {
        self.gap = gap;
        self
    }

    /// What Escape does (see [`EscapeFn`]); without it Escape closes the popup.
    pub fn on_escape(mut self, escape: impl FnMut() -> bool + 'static) -> Self {
        self.escape = Some(Box::new(escape));
        self
    }

    /// Its window's form (its `FormClosed` event, `set_client_size`, `close`).
    pub fn form(&self) -> &Form {
        &self.form
    }

    /// Opens it against `anchor`, in the client DIP of the window `owner` (a raw `HWND` value: the
    /// window of the frame being handled is [`current_window`]); owned by that window's form when
    /// one of this application's is. Returns its form.
    pub fn show(self, owner: isize, anchor: Rect) -> Form {
        let fallback = (Rect::new(anchor.left, anchor.top, anchor.right, anchor.bottom), Rect::new(0.0, 0.0, 1.0e6, 1.0e6));
        let (anchor, work) = anchor_on_screen(owner, anchor).unwrap_or(fallback);
        let (x, y) = place(anchor, self.size, work, self.placement, self.gap);
        self.show_at(owner, x, y)
    }

    /// Opens it with its top-left corner at `(x, y)` on screen (DIP), owned by the window `owner`.
    pub fn show_at(self, owner: isize, x: f32, y: f32) -> Form {
        let Popup { form, escape, .. } = self;
        if let Some(owner) = crate::Application::open_forms().into_iter().find(|f| f.handle() == Some(owner)) {
            form.set_owner(&owner);
        }
        let mut escape = escape;
        let closing = form.clone();
        form.root().key_down().subscribe(move |_sender, e: &mut KeyEventArgs| {
            if e.key != kubuno_desktop_views::events::Key(kubuno_desktop_controls::host::vk::ESCAPE) {
                return;
            }
            let close = escape.as_mut().is_none_or(|f| f());
            if close {
                closing.close();
            }
            e.handled = true;
        });
        let handle = form.clone();
        form.show_flyout(x, y);
        handle
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WORK: Rect = Rect { left: 0.0, top: 0.0, right: 1920.0, bottom: 1040.0 };

    #[test]
    fn a_popup_goes_under_its_anchor_aligned_on_the_end() {
        let anchor = Rect::new(1000.0, 14.0, 1036.0, 50.0);
        assert_eq!(place(anchor, (360.0, 580.0), WORK, Placement::BottomEnd, 4.0), (676.0, 54.0));
        assert_eq!(place(anchor, (360.0, 580.0), WORK, Placement::BottomStart, 4.0), (1000.0, 54.0));
        assert_eq!(place(anchor, (100.0, 50.0), WORK, Placement::BottomCenter, 4.0), (968.0, 54.0));
    }

    #[test]
    fn it_flips_above_when_there_is_no_room_below_and_more_above() {
        let anchor = Rect::new(500.0, 900.0, 536.0, 936.0);
        let (_, y) = place(anchor, (360.0, 400.0), WORK, Placement::BottomEnd, 4.0);
        assert_eq!(y, 900.0 - 4.0 - 400.0);
        // Room on neither side: it keeps the side with more room, clamped into the screen.
        let (_, y) = place(Rect::new(500.0, 100.0, 536.0, 136.0), (360.0, 1000.0), WORK, Placement::BottomEnd, 4.0);
        assert_eq!(y, 40.0, "clamped so its bottom stays on the screen");
    }

    #[test]
    fn it_is_clamped_into_the_work_area() {
        // An anchor at the screen's left edge: an end-aligned popup would start off screen.
        assert_eq!(place(Rect::new(10.0, 14.0, 46.0, 50.0), (360.0, 300.0), WORK, Placement::BottomEnd, 4.0).0, 0.0);
        // At its right edge, a start-aligned one would leave it.
        assert_eq!(place(Rect::new(1900.0, 14.0, 1936.0, 50.0), (360.0, 300.0), WORK, Placement::BottomStart, 4.0).0, 1560.0);
        // A second monitor left of the first (negative coordinates).
        let left = Rect::new(-1280.0, 0.0, 0.0, 1000.0);
        assert_eq!(place(Rect::new(-40.0, 14.0, -4.0, 50.0), (360.0, 300.0), left, Placement::BottomStart, 4.0), (-360.0, 54.0));
        assert_eq!(room_below(Rect::new(0.0, 14.0, 36.0, 50.0), WORK, 4.0), 986.0);
        assert_eq!(Placement::parse("TopCenter"), Some(Placement::TopCenter));
        assert_eq!(Placement::parse("nope"), None);
    }
}
