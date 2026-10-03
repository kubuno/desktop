//! Kubuno Desktop — `Program.cs`: the splash screen, then `Application::run(ShellWindow)`.
//! Everything else is in the library (`lib.rs`).
//!
//! `kubuno-desktop.exe [--background] [--sample|--live] [--light|--dark] [--culture fr|en] [--no-splash]
//! [--page launcher|settings|accounts|activity|labels|login|admin[:<section>]]`
//!
//! `--background` is the start at logon (the `Run` key): hidden in the notification area, no
//! splash screen. `--sample` shows the offline sample: fixed data, no server, nothing read from or
//! written to the configuration, nothing registered with the system. A Debug build started under a
//! debugger (F5) runs the sample by default; `--live` opts out.

// A GUI application: no console window, in Debug too (like a Windows Forms `WinExe`). Its logs,
// `println!`s and panics go to the debugger's Output window or to %LOCALAPPDATA%\Kubuno\logs.
#![windows_subsystem = "windows"]

use kubuno::View;
use kubuno_shell::{Options, Resources, ShellWindow};

fn main() -> kubuno::Result {
    kubuno::ui::diagnostics::set_display_name("Kubuno Desktop");
    let options = Options::from_args();
    if options.sample {
        kubuno::tracing::info!("[shell] offline sample (--sample, or a Debug build under a debugger; --live opts out)");
    }
    if let Some(culture) = &options.culture {
        kubuno::resources::set_culture(culture);
    }

    // The splash screen, first of all: it paints on its own thread while the rest starts, and fades
    // out once the window is on screen. None at logon, when the shell starts hidden in the
    // notification area.
    let splash = kubuno::SplashScreen::new()
        .artwork(kubuno::Artwork::Kubuno)
        .product("Kubuno Desktop")
        .version(env!("CARGO_PKG_VERSION"))
        .license(env!("CARGO_PKG_LICENSE"))
        .enabled(!options.background)
        .show();
    kubuno_shell::set_splash(splash.clone());

    // COM is initialised here too because the engine may reach the shell's COM objects (the
    // folder picker, WinRT) before the window exists.
    // SAFETY: initialising COM on this thread has no memory-safety requirement; a second
    // initialisation by the host is a harmless no-op.
    unsafe {
        let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
    }

    splash.step(Resources::splash_accounts(), 0.12);
    if !options.sample {
        if let Some(dir) = kubuno_account::paths::sandbox_dir() {
            kubuno::tracing::info!("[shell] sandboxed profile under {} (no system registration)", dir.display());
        }
        // Legacy single-instance layouts move under instances/<id>/ before anything reads them.
        let _ = kubuno_sync::migrate_legacy();
        // The accounts: plaintext creds.json moved into the OS credential store, the token owner, the
        // file sync's tokens, and the token broker the apps borrow from.
        match kubuno_shell::services::session::start() {
            Ok(s) if s.client_mode => kubuno::tracing::warn!("[shell] another Kubuno Desktop owns the accounts: borrowing its tokens"),
            Ok(_) => {}
            Err(e) => kubuno::tracing::error!("[shell] the accounts could not be started: {e}"),
        }
        // A `Run` entry written before the logon start had its flag gets it now.
        kubuno_shell::services::settings::refresh_autostart_command();
    }

    splash.step(Resources::splash_window(), 0.3);
    let window = ShellWindow::new(options);
    kubuno::Application::set_theme(kubuno_shell::services::settings::theme());
    splash.step(Resources::splash_open(), 0.55);
    splash.close_when(window.form());
    kubuno::Application::run(window)
}
