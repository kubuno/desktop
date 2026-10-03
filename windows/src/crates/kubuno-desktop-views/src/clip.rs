//! Children clipping: like every Windows Forms control, a container — and a user control — clips its children to
//! its own box. An element of a user control's view never paints, nor takes the pointer, outside the instance using
//! it; a button wider than its panel is cut at the panel's edge.
//!
//! The rule is applied in one place, `crate::design::DesignSlot::paint` (every compiled element is painted
//! through one):
//!
//! - a container's slot (or a user control's: [`crate::node::ViewNode::clips_children`]) paints its node under
//!   [`Children`], which narrows the clip in force to its box for everything painted inside it;
//! - every slot asks [`cut`] whether its own box crosses the clip in force. When it does, its paint (its node and
//!   all it holds) is clipped on the canvas, the pointer outside the visible part is kept from it, and its input
//!   rectangles (input router, focus ring, design-time hit-test, accessibility bounds) are cut to the visible part.
//!   A slot entirely inside the clip pushes nothing: the clip costs nothing in the common case, and what a control
//!   deliberately draws just outside its own box — its focus ring, its shadow — is kept.
//!
//! The floating parts of a view are not clipped by the containers they are declared in: menus, tooltips and
//! drop-downs are surfaces of their own (`kubuno_desktop_controls::host::popup`), and a `<Popover>`'s panel is painted
//! above the view, out of every container's clip ([`detached`]).
//!
//! The clip is kept in client coordinates (what the pointer is tested against): a scroll offset in force when it is
//! read is taken into account.

use std::cell::RefCell;

use kubuno_desktop_controls::host;
use kubuno_desktop_ui::Rect;

thread_local! {
    /// The clips in force, innermost last, each already cut by the ones before it (client coordinates).
    static STACK: RefCell<Vec<Rect>> = const { RefCell::new(Vec::new()) };
}

/// `a` ∩ `b` (empty, not inverted, when they do not meet).
pub(crate) fn intersect(a: Rect, b: Rect) -> Rect {
    let left = a.left.max(b.left);
    let top = a.top.max(b.top);
    Rect::new(left, top, a.right.min(b.right).max(left), a.bottom.min(b.bottom).max(top))
}

/// Whether `inner` lies in `outer` (to a hundredth of a DIP: layout arithmetic is not exact).
fn lies_in(inner: Rect, outer: Rect) -> bool {
    const EPS: f32 = 0.01;
    inner.left >= outer.left - EPS && inner.top >= outer.top - EPS && inner.right <= outer.right + EPS && inner.bottom <= outer.bottom + EPS
}

fn offset(r: Rect, (dx, dy): (f32, f32)) -> Rect {
    Rect::new(r.left + dx, r.top + dy, r.right + dx, r.bottom + dy)
}

/// The clip in force, in client coordinates; `None` when nothing clips what is painting now.
pub(crate) fn current() -> Option<Rect> {
    STACK.with(|s| s.borrow().last().copied())
}

/// What of `client` (a rectangle in client coordinates) the clip in force leaves visible — what the pointer may
/// reach of it (empty when it is entirely clipped away).
pub(crate) fn visible_client(client: Rect) -> Rect {
    match current() {
        Some(clip) => intersect(client, clip),
        None => client,
    }
}

/// The clip in force, in the current content coordinates, when `bounds` (content coordinates) crosses it: `None`
/// when nothing clips `bounds`, `Some(clip)` when part of it — or all of it — is clipped away.
pub(crate) fn cut(bounds: Rect) -> Option<Rect> {
    let (dx, dy) = host::content_offset();
    let clip = offset(current()?, (-dx, -dy));
    (!lies_in(bounds, clip)).then_some(clip)
}

/// The clip of a control's children ([`Children::push`]), popped when dropped (unwinding included).
#[must_use = "the children clip ends when this is dropped"]
pub(crate) struct Children {
    depth: usize,
}

impl Children {
    /// Clips what is painted until the returned guard drops to `bounds` (content coordinates) too.
    pub(crate) fn push(bounds: Rect) -> Self {
        let client = offset(bounds, host::content_offset());
        STACK.with(|s| {
            let mut s = s.borrow_mut();
            let clip = match s.last() {
                Some(outer) => intersect(client, *outer),
                None => client,
            };
            s.push(clip);
            Self { depth: s.len() }
        })
    }
}

impl Drop for Children {
    fn drop(&mut self) {
        STACK.with(|s| {
            if let Ok(mut s) = s.try_borrow_mut() {
                s.truncate(self.depth.saturating_sub(1));
            }
        });
    }
}

/// Runs `f` out of every clip in force: what it paints is not clipped by the containers around (a `<Popover>`'s
/// panel, painted above the whole view).
pub(crate) fn detached<R>(f: impl FnOnce() -> R) -> R {
    struct Restore(Vec<Rect>);
    impl Drop for Restore {
        fn drop(&mut self) {
            let saved = std::mem::take(&mut self.0);
            STACK.with(|s| {
                if let Ok(mut s) = s.try_borrow_mut() {
                    *s = saved;
                }
            });
        }
    }
    let _restore = Restore(STACK.with(|s| std::mem::take(&mut *s.borrow_mut())));
    f()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_clips_intersect_and_pop_in_order() {
        assert_eq!(current(), None);
        assert_eq!(cut(Rect::new(500.0, 500.0, 900.0, 900.0)), None, "nothing clips outside every container");
        {
            let _outer = Children::push(Rect::new(0.0, 0.0, 200.0, 100.0));
            assert_eq!(cut(Rect::new(10.0, 10.0, 50.0, 50.0)), None, "inside: no clip");
            assert_eq!(cut(Rect::new(150.0, 60.0, 290.0, 140.0)), Some(Rect::new(0.0, 0.0, 200.0, 100.0)));
            {
                let _inner = Children::push(Rect::new(100.0, 50.0, 400.0, 400.0));
                assert_eq!(current(), Some(Rect::new(100.0, 50.0, 200.0, 100.0)), "cut by the outer clip");
                assert_eq!(visible_client(Rect::new(150.0, 60.0, 290.0, 140.0)), Rect::new(150.0, 60.0, 200.0, 100.0));
                detached(|| assert_eq!(current(), None, "a popover's panel is out of every clip"));
                assert_eq!(current(), Some(Rect::new(100.0, 50.0, 200.0, 100.0)));
            }
            assert_eq!(current(), Some(Rect::new(0.0, 0.0, 200.0, 100.0)));
        }
        assert_eq!(current(), None);
        assert_eq!(intersect(Rect::new(0.0, 0.0, 10.0, 10.0), Rect::new(20.0, 20.0, 30.0, 30.0)), Rect::new(20.0, 20.0, 20.0, 20.0), "empty, never inverted");
    }
}
