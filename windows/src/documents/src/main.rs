//! Kubuno Documents — `Program.cs`: the splash screen, the document named on the command line,
//! then `Application::run(DocumentWindow)`. Everything else is in the library (`lib.rs`).
//!
//! `kubuno-documents.exe [--dark] [--culture fr|en] [--no-splash] [--zoom 50] [--doc <id> | content.json]`

// A GUI application: no console window, in Debug too (like a Windows Forms `WinExe`). Its logs,
// `println!`s and panics go to the debugger's Output window or to %LOCALAPPDATA%\Kubuno\logs.
#![windows_subsystem = "windows"]

use kubuno::View;
use kubuno_documents::{DocumentWindow, Options};

fn main() -> kubuno::Result {
    kubuno::ui::diagnostics::set_display_name("Kubuno Documents");
    let options = Options::from_args();
    if let Some(culture) = &options.culture {
        kubuno::resources::set_culture(culture);
    }
    // A document of the server (`--doc <id>`): the access tokens are borrowed from the Kubuno shell's
    // token broker (verified to be the installed shell, or the sandbox's under KUBUNO_SANDBOX_DIR);
    // Documents never holds a password or a refresh token.
    if options.doc.is_some() {
        match kubuno_sync::tokens::BrokerProvider::for_app("kubuno-documents") {
            Ok(p) => kubuno_sync::tokens::install(std::sync::Arc::new(p)),
            Err(e) => kubuno::tracing::error!("[documents] no token broker: {e}"),
        }
    }
    if options.dark {
        kubuno::Application::set_theme(kubuno::ui::Theme::dark());
    }

    // The splash screen, first of all: it paints on its own thread while the document opens and
    // the window is built, and fades out once the window is on screen (`--no-splash` /
    // KUBUNO_NO_SPLASH=1 turn it off).
    let splash = kubuno::SplashScreen::new()
        .artwork(kubuno::Artwork::Documents)
        .product("Kubuno Documents")
        .version(env!("CARGO_PKG_VERSION"))
        .license(env!("CARGO_PKG_LICENSE"))
        .show();

    // A `content_json` file given on the command line opens instead of the built-in sample. A file
    // that cannot be read is reported and the sample opens — a word processor that exits with no
    // window because one argument was wrong is worse than one that tells you.
    splash.step("Ouverture du document…", 0.25);
    let document = options.open_document();

    // The window: the ribbon, the fonts of the document, the first layout of its pages.
    splash.step("Chargement des polices et mise en page…", 0.6);
    let window = DocumentWindow::new(options, document);
    splash.close_when(window.form());
    kubuno::Application::run(window)
}
