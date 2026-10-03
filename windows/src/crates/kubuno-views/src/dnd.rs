//! Drag and drop (EVT-8 of `vskubuno/docs/EVENTS.md`): starting a drag from a view or a control.
//!
//! The target side is routed by the view runtime: an element with `AllowDrop="true"` receives
//! `DragEnter` → `DragOver`* → `DragDrop` or `DragLeave` ([`crate::events::DragEventArgs`]; its
//! handler sets `effect` to accept), whether the data comes from this window, another Kubuno window,
//! the Explorer (files) or any other application (text, registered formats) — OLE underneath.
//!
//! The source side is [`do_drag_drop`] (WinForms `Control.DoDragDrop`): the drag starts right after
//! the current frame, and the returned [`DragOperation`] completes with the effect the target chose.
//!
//! ```no_run
//! use kubuno_views::prelude::*;
//!
//! async fn drag_name(_ui: UiHandle<()>) {
//!     let effect = do_drag_drop(DataObject::from_text("Ada"), DragDropEffects::COPY | DragDropEffects::MOVE).await;
//!     if effect == DragDropEffects::MOVE {
//!         // remove the item from the source list
//!     }
//! }
//! ```

use std::cell::RefCell;
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::task::{Context, Poll, Waker};

pub use kubuno_controls::host::dnd::{DataObject, DragDropEffects};

#[derive(Default)]
struct State {
    result: Option<DragDropEffects>,
    waker: Option<Waker>,
}

/// A drag started by [`do_drag_drop`]: completes with the effect the drop target chose (`NONE`
/// when the drag was cancelled or refused). Poll it with [`DragOperation::effect`], or `.await` it.
#[derive(Clone, Default)]
pub struct DragOperation {
    state: Rc<RefCell<State>>,
}

impl DragOperation {
    /// The effect, once the drag is over.
    pub fn effect(&self) -> Option<DragDropEffects> {
        self.state.borrow().result
    }

    /// Whether the drag is over.
    pub fn is_completed(&self) -> bool {
        self.effect().is_some()
    }

    /// Completes the operation (what the host's drag loop calls when it ends).
    fn complete(&self, effect: DragDropEffects) {
        let waker = {
            let mut s = self.state.borrow_mut();
            s.result = Some(effect);
            s.waker.take()
        };
        if let Some(w) = waker {
            w.wake();
        }
    }
}

impl std::fmt::Debug for DragOperation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DragOperation").field("effect", &self.effect()).finish()
    }
}

impl Future for DragOperation {
    type Output = DragDropEffects;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<DragDropEffects> {
        let mut s = self.state.borrow_mut();
        match s.result {
            Some(e) => Poll::Ready(e),
            None => {
                s.waker = Some(cx.waker().clone());
                Poll::Pending
            }
        }
    }
}

/// Starts dragging `data`, allowing `allowed` (`Control.DoDragDrop`): right after the current
/// frame. A second request in the same frame replaces the first (which completes with `NONE`).
pub fn do_drag_drop(data: DataObject, allowed: DragDropEffects) -> DragOperation {
    let op = DragOperation::default();
    let done = op.clone();
    kubuno_controls::host::dnd::do_drag_drop(data, allowed, move |effect| done.complete(effect));
    op
}

/// Whether a drag started by this window is running.
pub fn is_dragging() -> bool {
    kubuno_controls::host::dnd::is_dragging()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_operation_completes_once_and_wakes_its_task() {
        struct Flag(std::sync::atomic::AtomicBool);
        impl std::task::Wake for Flag {
            fn wake(self: std::sync::Arc<Self>) {
                self.0.store(true, std::sync::atomic::Ordering::SeqCst);
            }
        }
        let flag = std::sync::Arc::new(Flag(std::sync::atomic::AtomicBool::new(false)));
        let waker = Waker::from(flag.clone());
        let mut cx = Context::from_waker(&waker);
        let mut op = DragOperation::default();
        assert!(Pin::new(&mut op).poll(&mut cx).is_pending());
        assert!(!op.is_completed());
        op.complete(DragDropEffects::MOVE);
        assert!(flag.0.load(std::sync::atomic::Ordering::SeqCst), "the awaiting task is woken");
        assert_eq!(Pin::new(&mut op).poll(&mut cx), Poll::Ready(DragDropEffects::MOVE));
        assert_eq!(op.effect(), Some(DragDropEffects::MOVE));
    }

    #[test]
    fn a_replaced_request_completes_with_none() {
        let first = do_drag_drop(DataObject::from_text("a"), DragDropEffects::COPY);
        let second = do_drag_drop(DataObject::from_text("b"), DragDropEffects::COPY);
        assert_eq!(first.effect(), Some(DragDropEffects::NONE));
        assert!(!second.is_completed());
    }
}
