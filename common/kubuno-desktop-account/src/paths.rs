//! Per-user locations of the desktop (vskubuno `docs/DESKTOP-OFFLINE-SYNC.md` §11) and the sandboxed profile
//! (`KUBUNO_SANDBOX_DIR`). The rules moved to `kubuno-desktop-app-storage::paths` (vskubuno `docs/STORAGE-COMPONENTS.md`)
//! so that an app keeps its settings without linking the account and network stack; this module re-exports them
//! unchanged (same functions, same variables, same locations on disk).

pub use kubuno_desktop_app_storage::paths::*;
