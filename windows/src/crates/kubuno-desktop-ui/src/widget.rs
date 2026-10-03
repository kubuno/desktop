//! What every Kubuno primitive is, and what it may assume.

use kubuno_drive_desktop_app_controls::{Canvas, Rect};
use kubuno_desktop_controls::{enums::Size, Control, ControlState};

/// The interaction state a primitive is painted in.
///
/// The replica layer has [`ControlState`], and this is deliberately a superset
/// of it rather than a rival: `hot` / `pressed` / `focused` mean exactly what
/// they mean there and convert both ways, so a primitive can hand its state
/// straight to the replica it owns. Kubuno adds the two states its own design
/// system distinguishes and .NET does not paint itself: `selected` (a nav row,
/// a tab, a list item) and `disabled`, which WinForms carries on the control
/// (`Control.Enabled`) rather than in the paint state.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WidgetState {
    /// The pointer is over it.
    pub hot: bool,
    /// The pointer is down on it.
    pub pressed: bool,
    /// It holds the keyboard focus.
    pub focused: bool,
    /// It is the current choice — an active nav row, the selected tab.
    pub selected: bool,
    /// It cannot be interacted with. Kept here (and not only on the model) so
    /// a container can grey a whole subtree without mutating its children.
    pub disabled: bool,
    /// The focus ring should SHOW — the web's `:focus-visible`, as opposed to
    /// `:focus` ([`WidgetState::focused`]). True when the focus came from the
    /// keyboard (Tab, arrows) or the control always shows it (a text field);
    /// false after a pointer click on a button. Meaningful only with
    /// `focused`; see [`WidgetState::show_focus_ring`] and
    /// [`crate::focus::FocusRing`], which computes both.
    pub focus_visible: bool,
}

impl WidgetState {
    pub const REST: Self = Self {
        hot: false,
        pressed: false,
        focused: false,
        selected: false,
        disabled: false,
        focus_visible: false,
    };

    pub fn hot(mut self, v: bool) -> Self {
        self.hot = v;
        self
    }

    pub fn pressed(mut self, v: bool) -> Self {
        self.pressed = v;
        self
    }

    pub fn focused(mut self, v: bool) -> Self {
        self.focused = v;
        self
    }

    pub fn selected(mut self, v: bool) -> Self {
        self.selected = v;
        self
    }

    pub fn disabled(mut self, v: bool) -> Self {
        self.disabled = v;
        self
    }

    pub fn focus_visible(mut self, v: bool) -> Self {
        self.focus_visible = v;
        self
    }

    /// Whether to paint the focus ring: focused AND focus-visible — the web's
    /// `:focus-visible` selector. What a primitive tests before drawing its
    /// ring, so a mouse click on a button does not leave a ring behind.
    pub fn show_focus_ring(&self) -> bool {
        self.focused && self.focus_visible
    }
}

impl From<WidgetState> for ControlState {
    fn from(s: WidgetState) -> Self {
        ControlState { hot: s.hot, pressed: s.pressed, focused: s.focused, default: false }
    }
}

impl From<ControlState> for WidgetState {
    fn from(s: ControlState) -> Self {
        WidgetState {
            hot: s.hot,
            pressed: s.pressed,
            focused: s.focused,
            selected: false,
            disabled: false,
            // The replica does not know how the focus arrived; showing the
            // ring whenever it is focused is the conservative WinForms reading
            // (it draws its focus cue on any focus).
            focus_visible: s.focused,
        }
    }
}

/// A Kubuno primitive.
///
/// The contract is short on purpose: a primitive owns a replica, so almost
/// everything a caller needs (`text`, `enabled`, `padding`, `dock`, `anchor`,
/// `min_size`…) is reached through [`Widget::model`] and through the
/// [`Deref`](std::ops::Deref) each primitive implements. What is left here is
/// only what the *Kubuno* layer decides: how big it wants to be, and how it
/// looks.
pub trait Widget {
    /// The replica underneath — the property surface, the defaults and the
    /// state machine, all of it checked against the real toolkit.
    ///
    /// This is what makes a primitive layout-able: a caller builds a
    /// [`kubuno_desktop_controls::layout::Item`] from the model's `dock`, `anchor`,
    /// `bounds` and `min_size` without the primitive having to restate them.
    fn model(&self) -> &dyn Control;

    /// The size this primitive wants, in DIP, in the **Kubuno** metrics.
    ///
    /// It is not the replica's `preferred_size`: WinForms measures a button as
    /// text plus a system-metric border, Kubuno measures it as a token height
    /// and a token padding. What *is* reused is the structure — which parts
    /// are stacked, in what order, with which gaps — because that is where the
    /// mistakes are, not in the numbers.
    fn measure(&self, canvas: &dyn Canvas) -> Size;

    /// Paints it into `bounds`.
    ///
    /// `bounds` is the rectangle the caller assigned, in the canvas' own
    /// coordinates — a primitive never reads its model's `bounds` to paint
    /// (doing so is a bug this codebase has already shipped once: a control
    /// painted where it *thought* it was rather than where it was put).
    fn paint(&self, canvas: &dyn Canvas, bounds: Rect, state: WidgetState);

    /// Whether `(x, y)`, in the same space as the `bounds` it was painted
    /// into, lands on it. The default is the rectangle; a primitive with a
    /// non-rectangular target (a round icon button) narrows it.
    fn hit_test(&self, bounds: Rect, x: f32, y: f32) -> bool {
        x >= bounds.left && x < bounds.right && y >= bounds.top && y < bounds.bottom
    }

    /// For diagnostics and for the parity harness' captions.
    fn type_name(&self) -> &'static str;
}
