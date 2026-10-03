//! Port of `Files.App/Services/SizeProvider` (UserSizeProvider →
//! DrivesSizeProvider → CachedSizeProvider): RECURSIVE folder size
//! computation in the background when "Calculate folder sizes" is
//! enabled. Like the original: symbolic links and junctions ignored
//! (ReparsePoint), subfolder sizes (≤ 3 levels) cached along the
//! way, intermediate result for the top-level folder at most every
//! 500 ms, then the final result.
//!
//! The C# chain is flattened into a single `SizeProvider` type: the
//! `DrivesSizeProvider` layer (per-drive split) is unnecessary here and thus has
//! no dedicated file (see `deferred`).

pub mod cached_size_provider;
pub mod size_changed_event_args;
pub mod user_size_provider;

pub use cached_size_provider::SizeProvider;
pub use user_size_provider::WM_APP_FOLDER_SIZE;
