//! The layout engine: `Dock`, then `Anchor` — in that order, because that is
//! the order the toolkit resolves them.
//!
//! WinForms lays a container out in two passes:
//!
//! 1. **Dock** — children with a `DockStyle` other than `None` are placed
//!    against the container's remaining display rectangle, in **reverse
//!    z-order** (the last docked child gets the outermost band), each one
//!    consuming the edge it took. `Fill` takes whatever is left.
//! 2. **Anchor** — every other child keeps its distance to the edges it is
//!    anchored to; anchoring to two opposite edges stretches it.
//!
//! Both passes are pure functions of `(container rect, children)` here, so the
//! behaviour is testable without a window, a device or a message loop.
//!
//! ## Coordinate space
//!
//! This engine is **space-agnostic**: it returns rectangles in whatever space
//! `display` was given in. Callers must give it the container's display
//! rectangle in the container's OWN local space — that is, with its top-left at
//! the origin — because [`ControlBase::bounds`] is defined, as in WinForms,
//! *relative to the parent's client area*.
//!
//! Passing a canvas-absolute rectangle "works" and is the tempting shortcut,
//! but it silently redefines every child's `bounds` as absolute: moving the
//! container then requires shifting every descendant by hand, and a child's
//! `bounds` no longer means what its documentation says. The container
//! translates into canvas space once, when it paints.
//!
//! [`ControlBase::bounds`]: crate::control::ControlBase::bounds

use kubuno_drive_desktop_app_controls::Rect;

use crate::control::ControlBase;
use crate::enums::{AnchorStyles, DockStyle, Size};

/// One child's layout input: what the engine needs, without borrowing a whole
/// control. `bounds` is the child's current rectangle (its design position).
// No `Debug`: `Rect` comes from the drawing layer and does not implement it.
#[derive(Clone, Copy)]
pub struct Item {
    pub bounds:  Rect,
    pub dock:    DockStyle,
    pub anchor:  AnchorStyles,
    pub visible: bool,
    pub min:     Size,
    pub max:     Size,
}

impl From<&ControlBase> for Item {
    fn from(c: &ControlBase) -> Self {
        Self {
            bounds: c.bounds,
            dock: c.dock,
            anchor: c.anchor,
            visible: c.visible,
            min: c.minimum_size,
            max: c.maximum_size,
        }
    }
}

/// Lays the children out inside `display` (the container's client rect already
/// deflated by its padding) and returns each child's new rectangle, in the same
/// order as the input.
///
/// `previous` is the container's display rect from the last pass — anchoring is
/// defined against the SIZE CHANGE, so a first pass (where `previous` equals
/// `display`) legitimately moves nothing.
/// Applies a `MinimumSize`/`MaximumSize` bound to ONE axis. `0.0` means unset on
/// both, which is why neither is treated as a literal zero.
///
/// The dock pass needs this as much as the anchor pass does: WinForms clamps in
/// `SetBoundsCore`, on whichever axis the engine computes. Applying it only to
/// anchored children left a docked band free to ignore its own maximum.
fn clamp_axis(v: f32, min: f32, max: f32) -> f32 {
    let mut v = v;
    if min > 0.0 {
        v = v.max(min);
    }
    if max > 0.0 {
        v = v.min(max);
    }
    v
}

/// How far a child anchored to NEITHER edge of an axis moves when the container
/// grows from `prev` to `cur`.
///
/// `GetAnchorDestination` recentres with **integer** division, so this is
/// `⌊cur/2⌋ − ⌊prev/2⌋` and not `(cur − prev)/2`: growing 201 → 300 moves the
/// child by 50, not 49.5. Flooring is what makes the port land on the toolkit's
/// pixel instead of half a pixel away from it.
fn recentre(prev: f32, cur: f32) -> f32 {
    (cur / 2.0).floor() - (prev / 2.0).floor()
}

pub fn layout(display: Rect, previous: Rect, items: &[Item]) -> Vec<Rect> {
    let mut out = vec![Rect::new(0.0, 0.0, 0.0, 0.0); items.len()];
    let mut free = display;

    // ── Pass 1: dock, in reverse z-order ────────────────────────────────
    // The toolkit walks the child collection backwards, so the child added
    // LAST ends up against the container edge and the earlier ones stack
    // inside it. Getting this order wrong silently inverts every toolbar.
    for (i, it) in items.iter().enumerate().rev() {
        if it.dock == DockStyle::None {
            continue;
        }
        // An invisible child takes no band — but it KEEPS its bounds. Writing a
        // zero rectangle here destroyed the geometry of anything hidden and
        // shown again, because the caller stores what this returns.
        if !it.visible {
            out[i] = it.bounds;
            continue;
        }
        let w = (it.bounds.right - it.bounds.left).max(0.0);
        let h = (it.bounds.bottom - it.bounds.top).max(0.0);
        out[i] = match it.dock {
            // A band takes its own thickness, NOT the space that happens to be
            // left: `DefaultLayout` never clamps it, so a band taller than its
            // container legitimately overflows (and the remaining space simply
            // runs out). Clamping looked tidier and disagreed with the toolkit.
            // Both axes are clamped, not just the docking one: the toolkit
            // clamps in `SetBoundsCore`, which knows nothing about which axis
            // the layout chose. A Top band with `MaximumSize.Width` really is
            // narrower than its container (parity case `D24`).
            DockStyle::Top => {
                let h = clamp_axis(h, it.min.height, it.max.height);
                let cw = clamp_axis(free.right - free.left, it.min.width, it.max.width);
                let r = Rect::new(free.left, free.top, free.left + cw, free.top + h);
                free = Rect::new(free.left, r.bottom, free.right, free.bottom);
                r
            }
            DockStyle::Bottom => {
                let h = clamp_axis(h, it.min.height, it.max.height);
                let cw = clamp_axis(free.right - free.left, it.min.width, it.max.width);
                let r = Rect::new(free.left, free.bottom - h, free.left + cw, free.bottom);
                free = Rect::new(free.left, free.top, free.right, r.top);
                r
            }
            DockStyle::Left => {
                let w = clamp_axis(w, it.min.width, it.max.width);
                let ch = clamp_axis(free.bottom - free.top, it.min.height, it.max.height);
                let r = Rect::new(free.left, free.top, free.left + w, free.top + ch);
                free = Rect::new(free.left + w, free.top, free.right, free.bottom);
                r
            }
            DockStyle::Right => {
                let w = clamp_axis(w, it.min.width, it.max.width);
                let ch = clamp_axis(free.bottom - free.top, it.min.height, it.max.height);
                let r = Rect::new(free.right - w, free.top, free.right, free.top + ch);
                free = Rect::new(free.left, free.top, free.right - w, free.bottom);
                r
            }
            // `Fill` is resolved IN PLACE, at its turn in the reverse walk, and
            // consumes nothing. That is why the toolkit requires a Fill child to
            // sit at the back of the z-order: a Fill added last takes the whole
            // remaining rectangle and the bands docked "after" it (earlier in z)
            // then overlap it. Deferring Fill to the end made every z-order
            // "work", which is friendlier and is not what WinForms does — and it
            // silently disagreed with the reference on nineteen rectangles.
            DockStyle::Fill => Rect::new(
                free.left,
                free.top,
                free.left + clamp_axis(free.right - free.left, it.min.width, it.max.width),
                free.top + clamp_axis(free.bottom - free.top, it.min.height, it.max.height),
            ),
            DockStyle::None => unreachable!("filtered above"),
        };
    }

    // ── Pass 2: anchor ──────────────────────────────────────────────────
    let dw = (display.right - display.left) - (previous.right - previous.left);
    let dh = (display.bottom - display.top) - (previous.bottom - previous.top);
    for (i, it) in items.iter().enumerate() {
        if it.dock != DockStyle::None {
            continue;
        }
        // Unlike docking, an invisible ANCHORED child is still anchored: the
        // toolkit moves it so that showing it again finds it where it belongs.
        let (l, t, r, b) = (it.bounds.left, it.bounds.top, it.bounds.right, it.bounds.bottom);
        let (left_a, right_a) = (
            it.anchor.contains(AnchorStyles::LEFT),
            it.anchor.contains(AnchorStyles::RIGHT),
        );
        let (top_a, bottom_a) = (
            it.anchor.contains(AnchorStyles::TOP),
            it.anchor.contains(AnchorStyles::BOTTOM),
        );

        // Both edges → stretch; the far edge only → move; the near edge only →
        // stay put; NEITHER → recentre. That last case is `GetAnchorDestination`
        // doing `left += displayRect.Width / 2` with INTEGER division, so it is
        // `old − ⌊prev/2⌋ + ⌊cur/2⌋` and not `old + ⌊Δ/2⌋` — the two differ by a
        // pixel whenever one extent is odd.
        let (nl, nr) = match (left_a, right_a) {
            (true, true) => (l, r + dw),
            (false, true) => (l + dw, r + dw),
            (true, false) => (l, r),
            (false, false) => {
                let d = recentre(previous.right - previous.left, display.right - display.left);
                (l + d, r + d)
            }
        };
        let (nt, nb) = match (top_a, bottom_a) {
            (true, true) => (t, b + dh),
            (false, true) => (t + dh, b + dh),
            (true, false) => (t, b),
            (false, false) => {
                let d = recentre(previous.bottom - previous.top, display.bottom - display.top);
                (t + d, b + d)
            }
        };

        // A stretched child still obeys its own size constraints.
        let mut rect = Rect::new(nl, nt, nr.max(nl), nb.max(nt));
        if it.min.width > 0.0 && rect.right - rect.left < it.min.width {
            rect = Rect::new(rect.left, rect.top, rect.left + it.min.width, rect.bottom);
        }
        if it.max.width > 0.0 && rect.right - rect.left > it.max.width {
            rect = Rect::new(rect.left, rect.top, rect.left + it.max.width, rect.bottom);
        }
        if it.min.height > 0.0 && rect.bottom - rect.top < it.min.height {
            rect = Rect::new(rect.left, rect.top, rect.right, rect.top + it.min.height);
        }
        if it.max.height > 0.0 && rect.bottom - rect.top > it.max.height {
            rect = Rect::new(rect.left, rect.top, rect.right, rect.top + it.max.height);
        }
        out[i] = rect;
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const DISPLAY: Rect = Rect { left: 0.0, top: 0.0, right: 200.0, bottom: 100.0 };

    fn item(bounds: Rect, dock: DockStyle, anchor: AnchorStyles) -> Item {
        Item { bounds, dock, anchor, visible: true, min: Size::EMPTY, max: Size::EMPTY }
    }

    fn sized(w: f32, h: f32, dock: DockStyle) -> Item {
        item(Rect::new(0.0, 0.0, w, h), dock, AnchorStyles::default())
    }

    #[test]
    fn a_top_dock_takes_the_full_width_and_its_own_height() {
        let out = layout(DISPLAY, DISPLAY, &[sized(50.0, 24.0, DockStyle::Top)]);
        assert_eq!((out[0].left, out[0].top, out[0].right, out[0].bottom), (0.0, 0.0, 200.0, 24.0));
    }

    /// `Fill` is resolved at its own turn in the reverse walk and consumes
    /// nothing — so a Fill added LAST (highest z) takes the whole display and
    /// the bands docked before it overlap it. This is why the toolkit tells you
    /// to send a Fill control to the back.
    ///
    /// Measured against real WinForms (parity case `D06`): the port used to
    /// defer Fill to the end and hand it the leftover, which reported
    /// `(60,24,200,80)` where the toolkit reports the full rectangle.
    #[test]
    fn fill_is_resolved_in_place_and_consumes_nothing() {
        let items = [
            sized(0.0, 24.0, DockStyle::Top),
            sized(0.0, 20.0, DockStyle::Bottom),
            sized(60.0, 0.0, DockStyle::Left),
            sized(0.0, 0.0, DockStyle::Fill),
        ];
        let out = layout(DISPLAY, DISPLAY, &items);
        assert_eq!((out[3].left, out[3].top, out[3].right, out[3].bottom), (0.0, 0.0, 200.0, 100.0));
        // The bands still take their own edges out of what remains.
        assert_eq!((out[2].left, out[2].right), (0.0, 60.0), "Left band");
        assert_eq!((out[0].top, out[0].bottom), (0.0, 24.0), "Top band");
    }

    /// A Fill placed FIRST (lowest z) is resolved last and therefore receives
    /// only what the bands left — the arrangement the toolkit actually
    /// recommends. Parity case `D08`, which agreed even before the fix.
    #[test]
    fn a_fill_at_the_back_of_the_z_order_gets_the_remainder() {
        let items = [
            sized(0.0, 0.0, DockStyle::Fill),
            sized(0.0, 30.0, DockStyle::Top),
        ];
        let out = layout(DISPLAY, DISPLAY, &items);
        assert_eq!((out[0].top, out[0].bottom), (30.0, 100.0));
    }

    /// Reverse z-order: the LAST docked child gets the outermost band.
    #[test]
    fn docking_resolves_in_reverse_z_order() {
        let items = [sized(0.0, 30.0, DockStyle::Top), sized(0.0, 20.0, DockStyle::Top)];
        let out = layout(DISPLAY, DISPLAY, &items);
        // Child 1 (added last) is outermost, at the very top.
        assert_eq!(out[1].top, 0.0);
        assert_eq!(out[1].bottom, 20.0);
        // Child 0 stacks below it.
        assert_eq!(out[0].top, 20.0);
        assert_eq!(out[0].bottom, 50.0);
    }

    #[test]
    fn anchoring_both_edges_stretches_and_one_edge_moves() {
        let grown = Rect::new(0.0, 0.0, 300.0, 100.0);   // +100 wide
        let stretch = item(Rect::new(10.0, 10.0, 100.0, 30.0), DockStyle::None,
                           AnchorStyles::LEFT.union(AnchorStyles::RIGHT).union(AnchorStyles::TOP));
        let move_right = item(Rect::new(10.0, 10.0, 100.0, 30.0), DockStyle::None,
                              AnchorStyles::RIGHT.union(AnchorStyles::TOP));
        let pinned = item(Rect::new(10.0, 10.0, 100.0, 30.0), DockStyle::None, AnchorStyles::default());
        let out = layout(grown, DISPLAY, &[stretch, move_right, pinned]);
        assert_eq!((out[0].left, out[0].right), (10.0, 200.0), "stretches");
        assert_eq!((out[1].left, out[1].right), (110.0, 200.0), "moves, keeps width");
        assert_eq!((out[2].left, out[2].right), (10.0, 100.0), "top-left stays put");
    }

    #[test]
    fn a_hidden_child_keeps_its_bounds_and_consumes_no_edge() {
        let mut hidden = sized(0.0, 40.0, DockStyle::Top);
        hidden.visible = false;
        let out = layout(DISPLAY, DISPLAY, &[hidden, sized(0.0, 0.0, DockStyle::Fill)]);
        assert_eq!(out[1].top, 0.0, "the hidden band must not push Fill down");
    }

    /// A docked band keeps its own thickness even when the container cannot
    /// hold it: `DefaultLayout` never clamps the docking axis. Clamping read as
    /// defensive and was simply wrong — parity case `D12` has WinForms report
    /// `(0,0,200,400)` in a 100-tall container where the port reported
    /// `(0,0,200,100)`.
    #[test]
    fn a_band_larger_than_its_container_overflows_rather_than_being_clamped() {
        let out = layout(DISPLAY, DISPLAY, &[sized(0.0, 400.0, DockStyle::Top)]);
        assert_eq!(out[0].bottom, 400.0);
        // The CROSS axis is still the container's, which both agree on (`C06`).
        assert_eq!((out[0].left, out[0].right), (0.0, 200.0));
    }

    /// The dock pass honours `MinimumSize`/`MaximumSize` on the axis it
    /// computes — the toolkit clamps in `SetBoundsCore` regardless of how the
    /// size was arrived at. Parity case `D24`.
    #[test]
    fn a_docked_band_obeys_its_own_max_on_the_docking_axis() {
        let mut it = sized(0.0, 120.0, DockStyle::Top);
        it.max = Size::new(0.0, 50.0);
        let out = layout(DISPLAY, DISPLAY, &[it]);
        assert_eq!(out[0].bottom - out[0].top, 50.0);
    }

    /// An unanchored axis RECENTRES, and with integer halving. Parity case
    /// `A20`: 201 → 300 moves by 50, not 49.5 — the comment this replaces
    /// claimed the child simply stayed put.
    #[test]
    fn an_unanchored_axis_recentres_with_integer_halving() {
        assert_eq!(recentre(200.0, 300.0), 50.0);
        assert_eq!(recentre(201.0, 300.0), 50.0);
        assert_eq!(recentre(100.0, 100.0), 0.0);

        let child = item(Rect::new(10.0, 10.0, 90.0, 40.0), DockStyle::None, AnchorStyles::NONE);
        let grown = Rect::new(0.0, 0.0, 300.0, 160.0);
        let out = layout(grown, DISPLAY, &[child]);
        assert_eq!((out[0].left, out[0].right), (60.0, 140.0));
        assert_eq!((out[0].top, out[0].bottom), (40.0, 70.0));
    }

    /// A hidden ANCHORED child is still anchored — showing it again must find
    /// it where it belongs. Parity case `A15`, where the toolkit stretches it
    /// exactly like its visible twin.
    #[test]
    fn a_hidden_anchored_child_is_still_anchored() {
        let mut hidden = item(
            Rect::new(10.0, 10.0, 90.0, 40.0),
            DockStyle::None,
            AnchorStyles::LEFT.union(AnchorStyles::RIGHT).union(AnchorStyles::TOP),
        );
        hidden.visible = false;
        let grown = Rect::new(0.0, 0.0, 300.0, 100.0);
        let out = layout(grown, DISPLAY, &[hidden]);
        assert_eq!((out[0].left, out[0].right), (10.0, 190.0));
    }
}
