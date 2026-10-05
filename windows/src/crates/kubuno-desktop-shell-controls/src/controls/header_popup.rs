//! What [`crate::WaffleButton`] and [`crate::AccountButton`] share: where their panel opens, how
//! tall it may be, its tint, and the guard that keeps the click which closed it from reopening it.
//!
//! The panel is a [`kubuno_desktop::popup::Popup`]: a floating window of its own, so it may extend beyond the
//! window that holds the button (a small app window, a window near a screen edge), clamped into the
//! monitor's work area.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Instant;

use kubuno_desktop::popup::{self, Placement};
use kubuno_desktop::ui::Rect;

/// A click on the button within this delay after its panel closed is the click that closed it
/// (the panel closes as soon as it loses the focus, before the click arrives): it does not reopen it.
const REOPEN_GUARD_MS: u128 = 400;

/// What the panel hangs from: the button itself (under it, right edges aligned, `PopupOffset` below
/// it), or the window's top-right corner (`PopupMargin` from its right edge, `PopupOffset` below its
/// client top: the shell's header, whose panels line up with the window rather than the button).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PopupAnchor {
    Button,
    Window,
}

impl PopupAnchor {
    pub fn parse(name: &str) -> Self {
        if name.trim().eq_ignore_ascii_case("Window") {
            PopupAnchor::Window
        } else {
            PopupAnchor::Button
        }
    }
}

/// Where a panel `width` wide opens and the height it may take, in screen DIP: `(x, y, room)`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spot {
    pub x: f32,
    pub y: f32,
    /// The tallest the panel may be from `y`.
    pub room: f32,
}

/// Where a panel of `size` opens for a button at `bounds` (client DIP of the window `owner`):
/// `offset`, `margin` and `bottom_gap` are the button's `PopupOffset`, `PopupMargin` and
/// `PopupBottomGap`. `None` when `owner` is no window.
pub fn spot(owner: isize, anchor: PopupAnchor, bounds: Rect, size: (f32, f32), offset: f32, margin: f32, bottom_gap: f32) -> Option<Spot> {
    match anchor {
        PopupAnchor::Button => {
            let (a, work) = popup::anchor_on_screen(owner, bounds)?;
            let below = popup::room_below(a, work, offset) - bottom_gap;
            let above = a.top - offset - work.top - bottom_gap;
            // Under the button when the panel fits there or has more room there; else above it.
            let (placement, room) = if size.1 <= below || below >= above { (Placement::BottomEnd, below) } else { (Placement::TopEnd, above) };
            let room = room.max(120.0);
            let (x, y) = popup::place(a, (size.0, size.1.min(room)), work, placement, offset);
            Some(Spot { x, y, room })
        }
        PopupAnchor::Window => {
            let g = kubuno_desktop::controls::host::screen_geometry(owner)?;
            let (ox, oy) = g.client_origin;
            let (w, h) = g.client_size;
            Some(Spot { x: ox + w - margin - size.0, y: oy + offset, room: h - offset - bottom_gap })
        }
    }
}

/// The panels' tint over their blur: the theme's `PanelBackground` (the web's `--color-panel-bg`: light
/// `#E9EEF6`, dark `#303134`) at 80 %.
pub fn tint() -> String {
    tint_of(&kubuno_desktop::Application::theme())
}

/// [`tint`] in `theme`: `#RRGGBBCC`.
pub fn tint_of(theme: &kubuno_desktop::ui::Theme) -> String {
    let c = theme.panel_background;
    let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!("#{:02X}{:02X}{:02X}CC", byte(c.r), byte(c.g), byte(c.b))
}

/// The open panel of a button, and when its last one closed (see [`REOPEN_GUARD_MS`]).
#[derive(Default, Clone)]
pub struct PopupState {
    open: Rc<Cell<bool>>,
    closed_at: Rc<Cell<Option<Instant>>>,
}

impl PopupState {
    /// Whether a click now opens the panel: not while one is open (the click closes it, by taking
    /// the focus), nor right after it closed.
    pub fn may_open(&self) -> bool {
        !self.open.get() && !self.closed_at.get().is_some_and(|t| t.elapsed().as_millis() < REOPEN_GUARD_MS)
    }

    /// Follows `form`: open now, closed when its `FormClosed` comes.
    pub fn track(&self, form: &kubuno_desktop::Form) {
        self.open.set(true);
        let (open, closed_at) = (self.open.clone(), self.closed_at.clone());
        form.form_closed().subscribe(move |_form, _e| {
            open.set(false);
            closed_at.set(Some(Instant::now()));
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_anchor_reads_its_name_and_a_fresh_state_may_open() {
        assert_eq!(PopupAnchor::parse("Window"), PopupAnchor::Window);
        assert_eq!(PopupAnchor::parse(""), PopupAnchor::Button);
        let s = PopupState::default();
        assert!(s.may_open());
        s.closed_at.set(Some(Instant::now()));
        assert!(!s.may_open(), "the click that closed it does not reopen it");
    }

    #[test]
    fn the_tint_is_the_panel_token_at_80_percent_in_both_modes() {
        assert_eq!(tint_of(&kubuno_desktop::ui::Theme::light()), "#E9EEF6CC");
        assert_eq!(tint_of(&kubuno_desktop::ui::Theme::dark()), "#303134CC");
    }
}
