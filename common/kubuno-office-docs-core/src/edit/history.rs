//! Undo and redo, grouped like the web.
//!
//! The web body's history is Yjs's `UndoManager` (y-prosemirror's `yUndoPlugin`; ProseMirror's own
//! history is disabled under collaboration): changes less than `captureTimeout` = **500 ms** after
//! the previous one merge into the same undo step, whatever they are — a sentence typed without
//! pausing is one step, a pause of half a second starts the next one. Nothing in the web editor
//! calls `stopCapturing`, so a caret move does not split a group either; the desktop keeps that.
//! Undo restores the selection from before the group, redo the one after it.
//!
//! The core never reads a clock: the caller passes the time of each change (milliseconds, any
//! monotonic origin), so the rule is testable and identical on every platform.
//!
//! Undo does not touch the save state: a document undone back to what the server holds is still
//! dirty, because the server does not know that.

use super::step::Step;
use crate::model::Node;

/// `captureTimeout` of Yjs's `UndoManager`.
pub const CAPTURE_MS: u64 = 500;
/// How many undo steps are kept.
pub const MAX_DEPTH: usize = 500;

/// A text selection: `anchor` stays, `head` moves.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Selection {
    pub anchor: usize,
    pub head: usize,
}

impl Selection {
    pub fn caret(pos: usize) -> Self {
        Self { anchor: pos, head: pos }
    }

    pub fn new(anchor: usize, head: usize) -> Self {
        Self { anchor, head }
    }

    pub fn from(&self) -> usize {
        self.anchor.min(self.head)
    }

    pub fn to(&self) -> usize {
        self.anchor.max(self.head)
    }

    pub fn is_empty(&self) -> bool {
        self.anchor == self.head
    }
}

/// One undo step: the forward steps, their inverses (application order), the selections around
/// it and when it last changed.
#[derive(Clone, Debug)]
pub struct Group {
    pub steps: Vec<Step>,
    pub inverses: Vec<Step>,
    pub before: Selection,
    pub after: Selection,
    pub last_ms: u64,
    /// A change made with `merge: false` (a structural command the caller wants on its own) refuses
    /// to absorb the next one.
    pub closed: bool,
}

impl Group {
    fn undo_into(&self, doc: &mut Node) -> bool {
        let mut applied = Vec::new();
        for inv in self.inverses.iter().rev() {
            match inv.apply(doc) {
                Ok(redo) => applied.push(redo),
                Err(_) => {
                    for r in applied.iter().rev() {
                        let _ = Step::apply(r, doc);
                    }
                    return false;
                }
            }
        }
        true
    }

    fn redo_into(&self, doc: &mut Node) -> bool {
        let mut applied = Vec::new();
        for step in &self.steps {
            match step.apply(doc) {
                Ok(inv) => applied.push(inv),
                Err(_) => {
                    for r in applied.iter().rev() {
                        let _ = r.apply(doc);
                    }
                    return false;
                }
            }
        }
        true
    }
}

#[derive(Debug, Default)]
pub struct History {
    done: Vec<Group>,
    undone: Vec<Group>,
    /// Set by [`History::stop_capturing`]: the next change starts a new group.
    stopped: bool,
}

impl History {
    pub fn new() -> Self {
        Self::default()
    }

    /// Records an applied change. `steps`/`inverses` as a [`super::step::Tr`] produced them.
    pub fn record(&mut self, steps: Vec<Step>, inverses: Vec<Step>, before: Selection, after: Selection, now_ms: u64, merge: bool) {
        if steps.is_empty() {
            return;
        }
        self.undone.clear();
        let joins = !self.stopped
            && merge
            && self.done.last().is_some_and(|g| !g.closed && now_ms.saturating_sub(g.last_ms) < CAPTURE_MS);
        self.stopped = false;
        if joins {
            if let Some(g) = self.done.last_mut() {
                // Coalesce a replace of the node the group's last step already replaced: keep the
                // group's inverse (it restores the state before the group), keep only the newest
                // forward step. A typed sentence costs two copies of its paragraph, not two per key.
                if steps.len() == 1 && g.steps.last().is_some_and(|last| last.same_single_target(&steps[0])) {
                    let n = g.steps.len();
                    g.steps[n - 1] = steps.into_iter().next().unwrap_or_else(|| g.steps[n - 1].clone());
                } else {
                    g.steps.extend(steps);
                    g.inverses.extend(inverses);
                }
                g.after = after;
                g.last_ms = now_ms;
                return;
            }
        }
        self.done.push(Group { steps, inverses, before, after, last_ms: now_ms, closed: !merge });
        if self.done.len() > MAX_DEPTH {
            self.done.remove(0);
        }
    }

    /// `UndoManager.stopCapturing()`: the next change starts a new undo step.
    pub fn stop_capturing(&mut self) {
        self.stopped = true;
    }

    pub fn can_undo(&self) -> bool {
        !self.done.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.undone.is_empty()
    }

    pub fn depth(&self) -> usize {
        self.done.len()
    }

    /// Throws both stacks away (the document was replaced, e.g. after a conflict reload).
    pub fn clear(&mut self) {
        self.done.clear();
        self.undone.clear();
        self.stopped = false;
    }

    /// Undoes the newest group into `doc`; returns the selection to restore and the steps applied
    /// (for the layout's revision counters). `None` when nothing to undo or the undo failed (the
    /// document is then untouched).
    pub fn undo(&mut self, doc: &mut Node) -> Option<(Selection, Vec<Step>)> {
        let g = self.done.last()?;
        if !g.undo_into(doc) {
            return None;
        }
        let g = self.done.pop()?;
        let applied: Vec<Step> = g.inverses.iter().rev().cloned().collect();
        let sel = g.before;
        self.undone.push(g);
        self.stopped = true;
        Some((sel, applied))
    }

    /// Redoes the newest undone group.
    pub fn redo(&mut self, doc: &mut Node) -> Option<(Selection, Vec<Step>)> {
        let g = self.undone.last()?;
        if !g.redo_into(doc) {
            return None;
        }
        let g = self.undone.pop()?;
        let applied = g.steps.clone();
        let sel = g.after;
        self.done.push(g);
        self.stopped = true;
        Some((sel, applied))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edit::step::Tr;

    fn doc() -> Node {
        Node::from_slice(br#"{"content":[{"content":[{"text":"a","type":"text"}],"type":"paragraph"}],"type":"doc"}"#).expect("parses")
    }

    fn type_into(d: &mut Node, h: &mut History, text: &str, at: u64) {
        let mut tr = Tr::new(d);
        let mut p = tr.doc.children()[0].clone();
        let mut t = p.children()[0].clone();
        let cur = t.text().unwrap_or_default();
        t.set_text(&(cur + text));
        p.set_children(vec![t]);
        tr.replace_node(&[0], p).expect("ok");
        let (steps, inv) = (tr.steps, tr.inverses);
        h.record(steps, inv, Selection::caret(1), Selection::caret(2), at, true);
    }

    fn text(d: &Node) -> String {
        d.children()[0].children()[0].text().unwrap_or_default()
    }

    #[test]
    fn changes_less_than_500_ms_apart_are_one_undo_step() {
        let mut d = doc();
        let mut h = History::new();
        type_into(&mut d, &mut h, "b", 1000);
        type_into(&mut d, &mut h, "c", 1300);
        type_into(&mut d, &mut h, "d", 1700);
        assert_eq!(h.depth(), 1);
        assert_eq!(text(&d), "abcd");
        let (sel, _) = h.undo(&mut d).expect("undoes");
        assert_eq!(text(&d), "a");
        assert_eq!(sel, Selection::caret(1));
        h.redo(&mut d).expect("redoes");
        assert_eq!(text(&d), "abcd");
    }

    #[test]
    fn a_pause_of_half_a_second_starts_a_new_step() {
        let mut d = doc();
        let mut h = History::new();
        type_into(&mut d, &mut h, "b", 1000);
        type_into(&mut d, &mut h, "c", 1500);
        assert_eq!(h.depth(), 2);
        h.undo(&mut d);
        assert_eq!(text(&d), "ab");
    }

    #[test]
    fn a_new_change_discards_the_redo_stack_and_stop_capturing_splits() {
        let mut d = doc();
        let mut h = History::new();
        type_into(&mut d, &mut h, "b", 1000);
        h.undo(&mut d);
        assert!(h.can_redo());
        type_into(&mut d, &mut h, "x", 1100);
        assert!(!h.can_redo());
        h.stop_capturing();
        type_into(&mut d, &mut h, "y", 1150);
        assert_eq!(h.depth(), 2);
    }

    #[test]
    fn coalescing_keeps_two_copies_not_one_per_keystroke() {
        let mut d = doc();
        let mut h = History::new();
        for i in 0..50 {
            type_into(&mut d, &mut h, "z", 1000 + i);
        }
        assert_eq!(h.done[0].steps.len(), 1);
        assert_eq!(h.done[0].inverses.len(), 1);
        h.undo(&mut d);
        assert_eq!(text(&d), "a");
    }
}
