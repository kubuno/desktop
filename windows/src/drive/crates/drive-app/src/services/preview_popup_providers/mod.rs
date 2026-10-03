//! Port of `Files.App/Services/PreviewPopupProviders/`: the external QUICK
//! PREVIEW, in the style of QuickLook (Explorer's Space key).
//!
//! The original draws NO internal preview at all: it merely controls an
//! already-installed third-party utility — QuickLook (`QuickLookProvider.cs`), or
//! failing that Seer/Seer Pro (`SeerProProvider.cs`), or failing that PowerToys Peek. The
//! `PreviewPopupService.GetProviderAsync()` service keeps the FIRST available one; if
//! there is none, the action is a no-op. We faithfully reproduce this
//! behavior: QuickLook (named pipe) then Seer (WM_COPYDATA) as fallback, and
//! nothing else.
//!
//! Keyboard triggering (VK_SPACE on a single selection outside renaming) and
//! selection tracking (arrow keys → `switch`) still need to be wired into the hub — see
//! the porting report. This module only exposes the service building blocks.
//!
//! (`PowerToysPeekProvider.cs` is intentionally not ported: no dedicated
//! file.)

pub mod preview_popup_service;
pub mod quick_look_provider;
pub mod seer_pro_provider;

// `is_available`/`switch_preview` aren't called yet: the main window's
// keyboard hook still needs to be wired up (see the module doc).
#[allow(unused_imports)]
pub use preview_popup_service::{is_available, switch_preview, toggle_preview};
