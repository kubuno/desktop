//! The window's pages (user controls, `.kbcontrol`), each next to the item template its list repeats
//! (`accounts_page` and `account_row`, `activity_page` and `activity_row`, `labels_page` and `label_row`).
//! The administration console has a folder of its own (`crate::admin`).

pub mod account_row;
pub mod accounts_page;
pub mod activity_page;
pub mod activity_row;
pub mod label_row;
pub mod labels_page;
pub mod launcher_page;
pub mod login_page;
pub mod settings_page;
