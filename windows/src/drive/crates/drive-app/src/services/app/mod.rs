//! Port of `Files.App/Services/App/`.
//!
//! (Only the SIDELOAD channel is ported; `AppUpdateStoreService.cs` and
//! `AppUpdateNoneService.cs` remain out of scope — not applicable outside MSIX.)

pub mod app_update_sideload_service;
