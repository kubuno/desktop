//! The data of a Kubuno header for the desktop apps that are not the shell (Documents, Chat, Drive…): the
//! [`kubuno_desktop_shell_controls::LauncherService`] and [`kubuno_desktop_shell_controls::AccountService`] the header's
//! `WaffleButton`, `AccountButton` and `HeaderActions` read, fetched as the shell's current account through
//! the shell's token broker (`kubuno-desktop-account`), off the UI thread, kept on disk (offline-first).
//!
//! - [`modules`]: `/api/v1/modules` read as the web's launcher reads it (`parse_modules`, `module_label`,
//!   `migrate_favorites`, `initials_of`, `tile`) — moved from the shell.
//! - [`logos`]: the content-addressed picture cache and the web's logos embedded at build time — moved from
//!   the shell, its directory and its downloads made the caller's.
//! - [`snapshot`]: what the header shows at one moment ([`HeaderSnapshot`]), and the offline sample.
//! - [`feed`]: the worker thread (broker, server, disk, the broker's events).
//! - [`services`]: the UI-thread services, and what a pick does (focus this app, start another desktop app,
//!   open a web route, switch account, ask the shell).
//!
//! An app wires it in its window's `Load` (see the README):
//!
//! ```ignore
//! let mut config = kubuno_desktop_header_data::FeedConfig::new("kubuno-chat");
//! config.proxy = kubuno_desktop_sync::get_proxy();
//! kubuno_desktop_header_data::start(kubuno_desktop_header_data::HeaderOptions::for_app(&["chat"]), config, sample, self.dispatcher());
//! ```

pub mod feed;
pub mod logos;
pub mod modules;
pub mod services;
pub mod snapshot;

pub use feed::{FeedCommand, FeedConfig, HeaderFeed};
pub use modules::AppEntry;
pub use services::{apply, current, install, plan_account, plan_launch, refresh, set_performer, Action, HeaderOptions, Performer, ShellRequest};
pub use snapshot::{HeaderSnapshot, Status};

/// Whether this run shows the offline sample instead of the account's data: `--sample` on the command line,
/// or a Debug build started under a debugger without `--live` (the shell's rule: a debugging session never
/// reads the real profile nor asks the real broker).
pub fn sample_requested(args: &[String]) -> bool {
    let has = |flag: &str| args.iter().any(|a| a == flag);
    let debugging = cfg!(debug_assertions) && kubuno_desktop::controls::host::diagnostics::is_debugger_attached();
    has("--sample") || (debugging && !has("--live"))
}

/// Installs the header's services on this UI thread and fills them: the offline sample at once (`sample`), or
/// the worker described by `config` whose snapshots `dispatcher` brings back to the UI thread. Call it once the
/// window has a dispatcher (its `Load`); without one (`None`), the header shows the sample.
pub fn start<V: 'static>(options: HeaderOptions, config: FeedConfig, sample: bool, dispatcher: Option<kubuno_desktop::views::events::UiDispatcher<V>>) {
    match dispatcher {
        Some(dispatcher) if !sample => {
            let feed = HeaderFeed::start(config, move |snapshot| {
                drop(dispatcher.begin_invoke(move |_view: &mut V| apply(snapshot)));
            });
            install(options, feed);
        }
        _ => {
            if !sample {
                tracing::warn!("[header] the window has no dispatcher: the header shows the offline sample");
            }
            install(options, HeaderFeed::none());
            apply(HeaderSnapshot::sample());
        }
    }
}
