//! The designer surface Visual Studio bundles (vskubuno `tools\surface\view_embed.exe`), with the
//! library components linked: `kubuno-desktop-views`' own `examples/view_embed.rs`, unchanged, plus the static
//! constructors of the data components (`kubuno-desktop-data`) and of the printing components (this crate),
//! so a view holding `<PrintDocument>`, `<PrintPreviewControl>`, `<PrintPreviewDialog>`,
//! `<PrintDialog>`, `<PageSetupDialog>` or a data component renders before the project has been built
//! for the designer — and of the storage components (`kubuno-desktop-app-storage-components`: `<Settings>`,
//! `<SecretStore>`, `<RegistryKey>`, vskubuno docs/STORAGE-COMPONENTS.md), which in the designer keep
//! to memory and never touch the developer's profile.
//!
//! ```text
//! cargo build --release -p kubuno-desktop-print --example view_embed     (CARGO_TARGET_DIR=C:\kubuno-build\agent-dsgint)
//! ```
//!
//! It writes the same `examples\view_embed.exe` as `kubuno-desktop-views`' and `kubuno-desktop-data`'s examples:
//! build this one for the VSIX.

extern crate kubuno_desktop_app_storage_components as _;
extern crate kubuno_desktop_data as _;
extern crate kubuno_desktop_print as _;

#[path = "../../kubuno-desktop-views/examples/view_embed.rs"]
mod embed;

fn main() -> std::process::ExitCode {
    embed::main()
}
