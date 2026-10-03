//! The designer surface Visual Studio bundles (vskubuno `tools\surface\view_embed.exe`), with the data
//! components linked: `kubuno-views`' own `examples/view_embed.rs`, unchanged, plus this crate's static
//! constructors, so a view holding `<DbConnection>`, `<TableAdapter>`, `<BindingSource>`,
//! `<ErrorProvider>` or `<BindingNavigator>` renders before the project has been built for the designer
//! (without them the fallback surface refused such a view as unknown elements).
//!
//! ```text
//! cargo build --release -p kubuno-data --example view_embed     (CARGO_TARGET_DIR=C:\kubuno-build\agent-dsgint)
//! ```
//!
//! It writes the same `examples\view_embed.exe` as `kubuno-views`' example: build this one for the VSIX.

extern crate kubuno_data as _;

#[path = "../../kubuno-views/examples/view_embed.rs"]
mod embed;

fn main() -> std::process::ExitCode {
    embed::main()
}
