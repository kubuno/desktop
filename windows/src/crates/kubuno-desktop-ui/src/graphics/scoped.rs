//! [`GraphicsSlot`]: a [`Graphics`] lent to event args for the duration of one raise.
//!
//! Event args are `'static` values (they are `Any`), a `Graphics` borrows its canvas for one
//! paint. The slot bridges the two the way WinForms' `PaintEventArgs` does (its `Graphics` is
//! disposed when the paint ends): [`GraphicsSlot::lend`] puts a lifetime-erased pointer to the
//! `Graphics` in a shared cell and clears it when the raise returns — or unwinds. Reading the slot
//! afterwards yields a null `Graphics` that draws nothing, never a dangling one.
//!
//! Only shared references are ever handed out (every `Graphics` method takes `&self`), so a
//! handler cannot swap the lent `Graphics` for another or move it out; and a `Graphics` is
//! covariant in its lifetime, so shortening it to the borrow of the slot is sound.

use std::cell::{Cell, OnceCell};
use std::ptr::NonNull;
use std::rc::Rc;

use super::Graphics;

type Shared = Rc<Cell<Option<NonNull<Graphics<'static>>>>>;

/// A `Graphics` lent for the duration of a raise (see the module doc).
pub struct GraphicsSlot {
    lent: Shared,
    null: OnceCell<Graphics<'static>>,
}

impl Default for GraphicsSlot {
    fn default() -> Self {
        Self::empty()
    }
}

impl std::fmt::Debug for GraphicsSlot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GraphicsSlot").field("lent", &self.is_lent()).finish()
    }
}

/// Clears the shared cell when the raise ends, however it ends.
struct Guard(Shared);

impl Drop for Guard {
    fn drop(&mut self) {
        self.0.set(None);
    }
}

impl GraphicsSlot {
    /// A slot with nothing lent: [`GraphicsSlot::get`] answers a null `Graphics`.
    pub fn empty() -> Self {
        Self { lent: Rc::new(Cell::new(None)), null: OnceCell::new() }
    }

    /// Lends `g` to `f` through a slot; once `f` returns (or unwinds) the slot is empty again.
    pub fn lend<R>(g: &Graphics<'_>, f: impl FnOnce(GraphicsSlot) -> R) -> R {
        let erased = NonNull::from(g).cast::<Graphics<'static>>();
        let lent: Shared = Rc::new(Cell::new(Some(erased)));
        let _guard = Guard(lent.clone());
        f(GraphicsSlot { lent, null: OnceCell::new() })
    }

    /// Whether a `Graphics` is lent right now.
    pub fn is_lent(&self) -> bool {
        self.lent.get().is_some()
    }

    /// The lent `Graphics`, or a null one (draws nothing) outside the raise.
    pub fn get(&self) -> &Graphics<'_> {
        match self.lent.get() {
            // SAFETY: the pointer is set only while `lend` runs, from a live `&Graphics` that
            // `lend` keeps borrowed for that whole time, and cleared by `Guard` before `lend`
            // returns; only a shared reference is produced, shortened to the borrow of `self`
            // (`Graphics` is covariant in its lifetime).
            Some(p) => unsafe { &*(p.as_ptr() as *const Graphics<'_>) },
            None => self.null.get_or_init(Graphics::null),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graphics::{Color, RectExt};
    use kubuno_drive_desktop_app_controls::Rect;

    #[test]
    fn a_lent_graphics_is_reachable_during_the_raise_only() {
        let g = Graphics::recorder();
        let mut kept = None;
        GraphicsSlot::lend(&g, |slot| {
            assert!(slot.is_lent());
            slot.get().fill_rectangle(Color::RED, Rect::from_xywh(0.0, 0.0, 10.0, 10.0));
            kept = Some(slot);
        });
        let slot = kept.expect("slot moved out of the raise");
        assert!(!slot.is_lent(), "cleared when the raise returned");
        // Drawing afterwards reaches a null graphics, not the recorder.
        slot.get().fill_rectangle(Color::BLUE, Rect::from_xywh(0.0, 0.0, 10.0, 10.0));
        assert!(slot.get().is_null());
        assert_eq!(g.recorded().map(|l| l.describe()), Some(vec!["FillRect".to_string()]));
    }

    #[test]
    fn the_slot_is_cleared_even_when_the_raise_panics() {
        let g = Graphics::recorder();
        let escaped = std::rc::Rc::new(std::cell::RefCell::new(None));
        let e2 = escaped.clone();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            GraphicsSlot::lend(&g, |slot| {
                *e2.borrow_mut() = Some(slot);
                panic!("handler failed");
            })
        }));
        assert!(result.is_err());
        let slot = escaped.borrow_mut().take().expect("slot");
        assert!(!slot.is_lent());
    }
}
