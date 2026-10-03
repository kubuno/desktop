//! Kubuno Chat — `Program.cs`: the splash screen, the single instance and the `kubuno://`
//! protocol, then `Application::run(ChatWindow)`. Everything else is in the library (`lib.rs`).
//!
//! `kubuno-chat.exe [--sample] [--dark] [--culture fr|en] [--no-splash] [kubuno://chat/<id>]`

// A GUI application: no console window, in Debug too (like a Windows Forms `WinExe`). Its logs,
// `println!`s and panics go to the debugger's Output window or to %LOCALAPPDATA%\Kubuno\logs.
#![windows_subsystem = "windows"]

use kubuno::View;
use kubuno_chat::platform::protocol;
use kubuno_chat::{ChatWindow, Options, Resources};

fn main() -> kubuno::Result {
    kubuno::ui::diagnostics::set_display_name("Kubuno Chat");
    let args: Vec<String> = std::env::args().collect();
    // `--sample`, or a Debug build under a debugger without `--live`: no broker, no network, no registration.
    let sample = kubuno_header_data::sample_requested(&args);
    // `kubuno://` points at this program, except for the offline sample and a sandboxed profile
    // (`KUBUNO_SANDBOX_DIR`): neither registers anything with the system.
    let integrate = !sample && kubuno_account::paths::system_integration_allowed();
    if let Some(culture) = args.iter().position(|a| a == "--culture").and_then(|i| args.get(i + 1)) {
        kubuno::resources::set_culture(culture);
    }

    // Single instance: a launch carrying a meeting link (from the web) forwards it to the running
    // window and exits, so a hand-off raises the existing chat instead of opening a second one —
    // exactly what Zoom does with `zoommtg://`. Checked before anything else, so a hand-off shows
    // no splash screen.
    if protocol::another_instance_runs() {
        if integrate {
            protocol::register();
        }
        protocol::forward_to_running(protocol::raw_arg().as_deref(), Resources::app_title());
        return Ok(());
    }

    // The splash screen: it paints on its own thread while the rest starts, and fades out once
    // the chat window is on screen (`--no-splash` / KUBUNO_NO_SPLASH=1 turn it off).
    let splash = kubuno::SplashScreen::new()
        .artwork(kubuno::Artwork::Chat)
        .product("Kubuno Chat")
        .version(env!("CARGO_PKG_VERSION"))
        .license(env!("CARGO_PKG_LICENSE"))
        .show();

    // Point `kubuno://` at this binary (idempotent, never fatal).
    if integrate {
        splash.step("Enregistrement des liens kubuno://…", 0.2);
        protocol::register();
    }
    // The access tokens are borrowed from the Kubuno shell's token broker (verified to be the installed
    // shell, started in the background when it is not running): the chat holds no refresh token. The
    // sample needs none.
    if !sample {
        match kubuno_sync::tokens::BrokerProvider::for_app("kubuno-chat") {
            Ok(p) => kubuno_sync::tokens::install(std::sync::Arc::new(p)),
            Err(e) => kubuno::tracing::error!("[chat] no token broker: {e}"),
        }
    }

    if args.iter().any(|a| a == "--dark") {
        kubuno::Application::set_theme(kubuno::ui::Theme::dark());
    }
    splash.step("Préparation de l'affichage…", 0.45);
    let window = ChatWindow::new(Options::from_args());
    splash.step("Chargement des discussions…", 0.85);
    splash.close_when(window.form());
    kubuno::Application::run(window)
}
