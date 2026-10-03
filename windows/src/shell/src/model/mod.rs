//! What the views show and what they raise, without any UI: `view_model` (the rows and texts,
//! computed from the shell's state — pure, unit-tested) and `events` (the event arguments the user
//! controls raise to the window).

pub mod events;
pub mod view_model;
