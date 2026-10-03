//! The resource-file format of Kubuno desktop applications — the equivalent of Windows Forms'
//! `.resx` files (`vskubuno/docs/RESOURCES.md`).
//!
//! | Module | What |
//! |---|---|
//! | [`format`] | `.kbres` entries (String, Image, Icon, Audio, File, Color, Font; linked or embedded), lenient reading with positioned diagnostics, canonical writing |
//! | [`culture`] | satellite file names (`resources.fr.kbres`) and the culture fallback chain |
//! | [`set`] | a neutral file and its satellites on disk, cross-file checks |
//! | [`names`] | valid resource names and the generated Rust names |
//! | [`import`] | `.resx`/`.resw` → `.kbres` |
//! | [`settings`] | `.kbsettings` files: an app's declared settings (`Settings.settings`), read by `settings!`, the language server and the Visual Studio settings editor (`docs/STORAGE-COMPONENTS.md`) |
//! | [`xml`] | the small position-tracking XML reader behind them |
//!
//! No UI and no Windows API: used by the runtime (`kubuno-desktop-resources`), the `resources!` macro, the
//! `.kbview` language server and the conversion tool alike.

pub mod culture;
pub mod format;
pub mod import;
pub mod names;
pub mod set;
pub mod settings;
pub mod xml;

pub use format::{Diagnostic, Entry, Kind, ResourceFile, Severity, Value};
