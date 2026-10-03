//! The top-level view (`.kbview`): the document window, next to its code-behind, and the tests
//! over the views and their resources.

pub mod document_window;
pub mod confirm_dialog;
pub mod link_dialog;

#[cfg(test)]
mod view_tests;
