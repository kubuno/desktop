//! The work and the state behind the views, which the views do not draw: the sync engine's door
//! (`backend`, or the offline sample), the accounts (`session`), the background sync loop (`sync`), the
//! launcher's fetch (`apps`) with its offline cache of logos, avatar and module list (`logos`), the activity log (`activity`), the preferences (`settings`), the waffle's
//! favourites (`favorites`) and the command line (`options`).

pub mod activity;
pub mod apps;
pub mod backend;
pub mod favorites;
pub mod logos;
pub mod options;
pub mod session;
pub mod settings;
pub mod sync;
