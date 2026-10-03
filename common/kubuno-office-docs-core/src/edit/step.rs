//! The one primitive mutation, and transactions built from it.
//!
//! A [`Step`] replaces children `from..to` of the node at `parent` (a [`Path`] from the body) with
//! new nodes. Applying it returns the step that undoes it — the removed nodes, byte-identical —
//! computed against the pre-state while it still exists, so no feature ever writes an inverse by
//! hand and undo restores the stored bytes exactly. Editing inside a paragraph replaces the
//! paragraph; a structural change replaces the range of blocks it touches.

use std::fmt;

use crate::model::Node;
use crate::pm::{node_at_mut, Path};

/// Why an edit could not be applied. The document is left as it was.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EditError {
    /// The path does not lead to a node with a child array.
    NoSuchNode,
    /// The range is outside the parent's children, or runs backwards.
    BadRange,
    /// A position does not resolve, or is not where the command needs it (not in a textblock…).
    BadPosition,
    /// The operation does not apply here (nothing to do, or unsupported structure).
    NotApplicable,
}

impl fmt::Display for EditError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EditError::NoSuchNode => f.write_str("no such node"),
            EditError::BadRange => f.write_str("range outside the node"),
            EditError::BadPosition => f.write_str("position not usable here"),
            EditError::NotApplicable => f.write_str("not applicable"),
        }
    }
}

impl std::error::Error for EditError {}

/// Replace children `from..to` of the node at `parent` with `with`.
#[derive(Clone, Debug)]
pub struct Step {
    pub parent: Path,
    pub from: usize,
    pub to: usize,
    pub with: Vec<Node>,
}

impl Step {
    /// Replaces the single node at `path` (which must not be the body).
    pub fn replace_node(path: &[usize], node: Node) -> Self {
        let (last, parent) = path.split_last().map(|(l, p)| (*l, p.to_vec())).unwrap_or((0, Vec::new()));
        Step { parent, from: last, to: last + 1, with: vec![node] }
    }

    /// Applies the step and returns its inverse.
    pub fn apply(&self, doc: &mut Node) -> Result<Step, EditError> {
        let parent = node_at_mut(doc, &self.parent).ok_or(EditError::NoSuchNode)?;
        if !parent.has_child_array() {
            // A container with no `content` array: only an insertion at 0 can apply, and it would
            // have to create the key (which no step can remove again) — callers replace the
            // parent node itself instead.
            return Err(EditError::NoSuchNode);
        }
        let children = parent.children_mut().ok_or(EditError::NoSuchNode)?;
        if self.from > self.to || self.to > children.len() {
            return Err(EditError::BadRange);
        }
        let removed: Vec<Node> = children.splice(self.from..self.to, self.with.iter().cloned()).collect();
        Ok(Step { parent: self.parent.clone(), from: self.from, to: self.from + self.with.len(), with: removed })
    }

    /// Whether this step replaces exactly one node with exactly one node.
    pub fn is_single_replace(&self) -> bool {
        self.to == self.from + 1 && self.with.len() == 1
    }

    /// Whether `self` and `other` replace the same single node.
    pub fn same_single_target(&self, other: &Step) -> bool {
        self.is_single_replace() && other.is_single_replace() && self.parent == other.parent && self.from == other.from
    }

    /// The index of the top-level block this step touches, or the range of top-level children for
    /// a step on the body (for the layout's revision counters).
    pub fn top_level_touch(&self) -> TopTouch {
        match self.parent.first() {
            Some(&i) => TopTouch::Block(i),
            None => TopTouch::Splice { from: self.from, to: self.to, inserted: self.with.len() },
        }
    }
}

/// What a step does to the body's list of top-level blocks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TopTouch {
    /// Block `i` changed in place.
    Block(usize),
    /// Blocks `from..to` were replaced by `inserted` new ones.
    Splice { from: usize, to: usize, inserted: usize },
}

/// Applies steps one after the other, recording their inverses; rolls everything back on error.
pub struct Tr<'a> {
    pub doc: &'a mut Node,
    pub steps: Vec<Step>,
    /// In application order; undo applies them reversed.
    pub inverses: Vec<Step>,
}

impl<'a> Tr<'a> {
    pub fn new(doc: &'a mut Node) -> Self {
        Self { doc, steps: Vec::new(), inverses: Vec::new() }
    }

    pub fn step(&mut self, step: Step) -> Result<(), EditError> {
        let inverse = step.apply(self.doc)?;
        self.steps.push(step);
        self.inverses.push(inverse);
        Ok(())
    }

    /// Replaces the node at `path`.
    pub fn replace_node(&mut self, path: &[usize], node: Node) -> Result<(), EditError> {
        self.step(Step::replace_node(path, node))
    }

    /// Undoes everything applied so far.
    pub fn rollback(&mut self) {
        while let Some(inv) = self.inverses.pop() {
            let _ = inv.apply(self.doc);
            self.steps.pop();
        }
    }

    pub fn changed(&self) -> bool {
        !self.steps.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(json: &str) -> Node {
        Node::from_slice(json.as_bytes()).expect("parses")
    }

    fn bytes(n: &Node) -> String {
        String::from_utf8(n.to_vec().expect("serialises")).expect("utf-8")
    }

    #[test]
    fn a_step_and_its_inverse_restore_the_bytes() {
        let src = r#"{"content":[{"attrs":{"indent":47.999999999999996},"content":[{"text":"a","type":"text"}],"type":"paragraph"},{"type":"paragraph"}],"type":"doc"}"#;
        let mut d = doc(src);
        let step = Step { parent: vec![], from: 0, to: 2, with: vec![Node::of_type("horizontalRule")] };
        let inv = step.apply(&mut d).expect("applies");
        assert_ne!(bytes(&d), src);
        inv.apply(&mut d).expect("undoes");
        assert_eq!(bytes(&d), src);
    }

    #[test]
    fn a_failed_transaction_rolls_back() {
        let src = r#"{"content":[{"type":"paragraph"}],"type":"doc"}"#;
        let mut d = doc(src);
        let mut tr = Tr::new(&mut d);
        tr.step(Step { parent: vec![], from: 0, to: 0, with: vec![Node::of_type("paragraph")] }).expect("ok");
        assert!(tr.step(Step { parent: vec![9], from: 0, to: 0, with: vec![] }).is_err());
        tr.rollback();
        assert_eq!(bytes(&d), src);
    }
}
