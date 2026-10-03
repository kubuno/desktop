//! The Windows integration: the Cloud Files API (`cloudfiles`, status overlays), the Explorer
//! navigation pane (`explorer`), the notification-area icon (`tray`), the system folder picker
//! (`folder_picker`) and what the shell opens outside itself (`actions`: the browser, Explorer).

pub mod actions;
pub mod cloudfiles;
pub mod explorer;
pub mod folder_picker;
pub mod tray;
