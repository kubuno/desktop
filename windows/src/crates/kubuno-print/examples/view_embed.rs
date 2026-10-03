//! The designer surface Visual Studio bundles (vskubuno `tools\surface\view_embed.exe`), with the
//! library components linked: `kubuno-views`' own `examples/view_embed.rs`, unchanged, plus the static
//! constructors of the data components (`kubuno-data`) and of the printing components (this crate),
//! so a view holding `<PrintDocument>`, `<PrintPreviewControl>`, `<PrintPreviewDialog>`,
//! `<PrintDialog>`, `<PageSetupDialog>` or a data component renders before the project has been built
//! for the designer — and of the storage components (`kubuno-app-storage-components`: `<Settings>`,
//! `<SecretStore>`, `<RegistryKey>`, vskubuno docs/STORAGE-COMPONENTS.md), which in the designer keep
//! to memory and never touch the developer's profile.
//!
//! ```text
//! cargo build --release -p kubuno-print --example view_embed     (CARGO_TARGET_DIR=C:\kubuno-build\agent-dsgint)
//! ```
//!
//! It writes the same `examples\view_embed.exe` as `kubuno-views`' and `kubuno-data`'s examples:
//! build this one for the VSIX.

extern crate kubuno_app_storage_components as _;
extern crate kubuno_data as _;
extern crate kubuno_print as _;

#[path = "../../kubuno-views/examples/view_embed.rs"]
mod embed;

fn main() -> std::process::ExitCode {
    embed::main()
}
