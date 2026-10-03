//! GeneralSettingsService (mirror of `GeneralSettingsService.cs`).
//!
//! Helper generating/persisting the user identifier; the state
//! itself remains carried by the single `AppSettings` store (see `mod.rs`).

use super::{get, update};

/// `GeneralSettingsService.UserId`: `Get(Guid.NewGuid().ToString())` —
/// generated once and persisted.
pub fn user_id() -> String {
    let existing = get().user_id;
    if !existing.is_empty() {
        return existing;
    }
    let guid = windows::core::GUID::new().unwrap_or_default();
    let id = format!("{guid:?}").to_lowercase();
    update(|s| s.user_id = id.clone());
    id
}
