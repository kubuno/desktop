//! The UI-thread side: the [`LauncherService`] and [`AccountService`] the header's buttons read
//! (`kubuno_shell_controls::WaffleButton`, `AccountButton`, `HeaderActions`), backed by the latest
//! [`HeaderSnapshot`], and what a pick in their panels does ([`Action`]).
//!
//! The services are installed once per UI thread ([`install`]) and again at every snapshot ([`apply`]): the
//! waffle reads its service when it opens, and `set_default_accounts` makes every avatar take its look again.
//!
//! What a pick does — every choice is a pure function ([`plan_launch`], [`plan_account`]) so it is tested, and
//! carried out by a [`Performer`] (the default one below, or a stub in tests and demos):
//!
//! | Pick | Action |
//! |---|---|
//! | the running app's own tile | [`Action::FocusSelf`]: its window comes forward, nothing else starts |
//! | an app with a desktop build installed next to this program (Chat, Drive) | [`Action::StartApp`]: that program (a single-instance app raises its running window) |
//! | any other app | [`Action::OpenUrl`]: its web route on the account's server, in the browser |
//! | « Gérer votre compte », the avatar's camera | `/settings` on the web (the shell's own « Gérer » opens its settings page, which an app cannot reach) |
//! | « Étiquettes » / « Administration » (admins only) | `/labels` / `/admin` on the web |
//! | another account | [`Action::Switch`]: the broker's `switch_account` (the shell and every app follow) |
//! | « Ajouter un compte », « Se déconnecter », « Supprimer » | [`Action::Shell`]: the shell's job (it owns the accounts and the refresh tokens). No channel to ask it exists yet (the broker has no such request, the shell no single-instance hand-off), so the default performer logs it; see the README |
//!
//! The default performer never opens the browser in a sandboxed profile (`KUBUNO_SANDBOX_DIR`) or in the
//! offline sample: it logs the URL instead (`kubuno_account::paths::system_integration_allowed`).

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use kubuno_shell_controls::{AccountAction, AccountEntry, AccountService, AccountUser, LauncherService, Tile};

use crate::feed::{FeedCommand, HeaderFeed};
use crate::modules;
use crate::snapshot::{HeaderSnapshot, Status};

/// The desktop builds of Kubuno apps, by launcher id: `(app id, program name without extension)`. Documents
/// is not listed: started without a document it opens its built-in sample (it has no home view yet), so its
/// tile opens the web's documents list instead.
pub const DESKTOP_APPS: &[(&str, &str)] = &[("chat", "kubuno-chat"), ("drive", "drive")];

/// What the app tells the header about itself.
#[derive(Debug, Clone, Default)]
pub struct HeaderOptions {
    /// The launcher ids of the running app (`office-documents`, `chat`): their tile brings this window forward.
    pub own_apps: Vec<String>,
    /// The desktop builds a tile may start ([`DESKTOP_APPS`] by default).
    pub desktop_apps: Vec<(String, String)>,
    /// Where those programs are (the running program's directory by default).
    pub install_dir: Option<PathBuf>,
}

impl HeaderOptions {
    /// The options of the app whose own launcher ids are `own_apps`.
    pub fn for_app(own_apps: &[&str]) -> Self {
        Self {
            own_apps: own_apps.iter().map(|s| s.to_string()).collect(),
            desktop_apps: DESKTOP_APPS.iter().map(|(id, exe)| (id.to_string(), exe.to_string())).collect(),
            install_dir: std::env::current_exe().ok().and_then(|p| p.parent().map(PathBuf::from)),
        }
    }

    /// The installed program of desktop app `id`, if it is there.
    fn program_of(&self, id: &str) -> Option<PathBuf> {
        let (_, exe) = self.desktop_apps.iter().find(|(app, _)| app == id)?;
        let path = self.install_dir.as_ref()?.join(format!("{exe}{}", std::env::consts::EXE_SUFFIX));
        path.is_file().then_some(path)
    }
}

/// What the shell must do for an app ([`Action::Shell`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShellRequest {
    AddAccount,
    SignOut,
    RemoveAccount(String),
}

/// What a pick in the header's panels does (see the module doc).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Nothing (an unknown app, a sample, a row for the account already shown…), with why.
    Nothing(&'static str),
    FocusSelf,
    StartApp(PathBuf),
    OpenUrl(String),
    Switch(String),
    Shell(ShellRequest),
}

/// What app `id`'s tile does.
pub fn plan_launch(s: &HeaderSnapshot, options: &HeaderOptions, id: &str) -> Action {
    if options.own_apps.iter().any(|own| own == id) {
        return Action::FocusSelf;
    }
    let Some(app) = s.apps.iter().find(|a| a.id == id) else { return Action::Nothing("unknown app") };
    if let Some(program) = options.program_of(id) {
        return Action::StartApp(program);
    }
    if s.server.is_empty() || app.path.is_empty() {
        return Action::Nothing("no server to open the app on");
    }
    Action::OpenUrl(modules::web_url(&s.server, &app.path))
}

/// What a pick of the account panel does.
pub fn plan_account(s: &HeaderSnapshot, action: &AccountAction) -> Action {
    let web = |route: &str| if s.server.is_empty() { Action::Nothing("no server") } else { Action::OpenUrl(modules::web_url(&s.server, route)) };
    match action {
        AccountAction::ManageAccount | AccountAction::ChangeAvatar => web("/settings"),
        AccountAction::OpenLabels => web("/labels"),
        // Listed for an administrator only, but checked here too: this is the door to the console.
        AccountAction::OpenAdmin if s.is_admin => web("/admin"),
        AccountAction::OpenAdmin => Action::Nothing("not an administrator"),
        AccountAction::OpenAccount(id) if s.account.as_deref() == Some(id.as_str()) => Action::Nothing("already the current account"),
        AccountAction::OpenAccount(id) => Action::Switch(id.clone()),
        AccountAction::AddAccount => Action::Shell(ShellRequest::AddAccount),
        AccountAction::SignOut => Action::Shell(ShellRequest::SignOut),
        AccountAction::RemoveAccount(id) => Action::Shell(ShellRequest::RemoveAccount(id.clone())),
    }
}

/// Carries an [`Action`] out (on the UI thread). Replace the default with [`set_performer`] (tests, demos).
pub type Performer = Rc<dyn Fn(&Action)>;

struct State {
    snapshot: HeaderSnapshot,
    options: HeaderOptions,
    feed: HeaderFeed,
    performer: Option<Performer>,
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

fn with_state<R>(f: impl FnOnce(&mut State) -> R) -> Option<R> {
    STATE.with(|s| s.borrow_mut().as_mut().map(f))
}

/// The snapshot the header shows now (`None` before [`install`]).
pub fn current() -> Option<HeaderSnapshot> {
    with_state(|s| s.snapshot.clone())
}

/// Installs the header's services on this UI thread: `feed` takes the commands (favourites saved, account
/// switched), `options` say which tiles are this app's own. Until the first [`apply`], the panels are empty.
pub fn install(options: HeaderOptions, feed: HeaderFeed) {
    STATE.with(|s| {
        let performer = s.borrow_mut().take().and_then(|old| old.performer);
        *s.borrow_mut() = Some(State { snapshot: HeaderSnapshot::empty(Status::Loading), options, feed, performer });
    });
    publish();
}

/// Shows `snapshot` (on the UI thread): the panels read it at their next opening, the avatars take their look
/// again.
pub fn apply(snapshot: HeaderSnapshot) {
    if with_state(|s| s.snapshot = snapshot).is_none() {
        tracing::warn!("[header] a snapshot arrived before the services were installed: dropped");
        return;
    }
    publish();
}

/// Replaces the default performer (see [`Performer`]).
pub fn set_performer(performer: Performer) {
    with_state(|s| s.performer = Some(performer));
}

/// Asks the worker to fetch the data again now.
pub fn refresh() {
    with_state(|s| s.feed.send(FeedCommand::Refresh));
}

fn publish() {
    kubuno_shell_controls::set_default_launcher(Rc::new(BrokerLauncher));
    kubuno_shell_controls::set_default_accounts(Rc::new(BrokerAccounts));
}

/// Carries `action` out with the installed performer, else [`perform`].
fn carry_out(action: Action) {
    let (performer, sample) = with_state(|s| (s.performer.clone(), s.snapshot.status == Status::Sample)).unwrap_or((None, false));
    tracing::info!("[header] {action:?}");
    match performer {
        Some(p) => p(&action),
        None => perform(&action, sample),
    }
}

/// The default performer (see the module doc). `sample`: the offline sample, which opens and starts nothing.
pub fn perform(action: &Action, sample: bool) {
    match action {
        Action::Nothing(_) => {}
        Action::FocusSelf => kubuno::controls::host::restore_and_focus(),
        Action::StartApp(program) if sample => tracing::info!("[header] sample: would start {}", program.display()),
        Action::StartApp(program) => {
            if let Err(e) = start_detached(program) {
                tracing::warn!("[header] {} could not start: {e}", program.display());
            }
        }
        Action::OpenUrl(url) => open_url(url, sample),
        Action::Switch(id) => {
            with_state(|s| s.feed.send(FeedCommand::SwitchAccount(id.clone())));
        }
        Action::Shell(request) => {
            tracing::warn!("[header] {request:?} is the shell's: no channel to ask it yet (open Kubuno Desktop)");
        }
    }
}

/// Opens `url` in the user's browser — never in the offline sample nor in a sandboxed profile (tests,
/// captures), which only log it.
pub fn open_url(url: &str, sample: bool) {
    if url.is_empty() || sample || !kubuno_account::paths::system_integration_allowed() {
        tracing::info!("[header] open {url} (not opened: sample or sandboxed profile)");
        return;
    }
    #[cfg(windows)]
    {
        use windows::core::{HSTRING, PCWSTR};
        use windows::Win32::UI::Shell::ShellExecuteW;
        use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
        let (verb, file) = (HSTRING::from("open"), HSTRING::from(url));
        // SAFETY: the strings outlive the call; ShellExecuteW has no other requirement.
        unsafe {
            ShellExecuteW(None, PCWSTR(verb.as_ptr()), PCWSTR(file.as_ptr()), PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL);
        }
    }
    #[cfg(not(windows))]
    {
        let opener = if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
        if let Err(e) = std::process::Command::new(opener).arg(url).spawn() {
            tracing::warn!("[header] {opener} {url}: {e}");
        }
    }
}

/// Starts `program` detached from this process (it outlives the app, inherits no standard handle). The
/// environment is inherited, so a sandboxed app starts a sandboxed app.
fn start_detached(program: &std::path::Path) -> std::io::Result<()> {
    let mut cmd = std::process::Command::new(program);
    cmd.stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    if let Some(dir) = program.parent() {
        cmd.current_dir(dir);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        cmd.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    cmd.spawn().map(|_| ())
}

/// The waffle's service: the current snapshot's apps and favourites.
struct BrokerLauncher;

impl LauncherService for BrokerLauncher {
    fn apps(&self) -> Vec<Tile> {
        with_state(|s| s.snapshot.apps.iter().map(modules::tile).collect()).unwrap_or_default()
    }

    fn favorites(&self) -> Vec<String> {
        with_state(|s| s.snapshot.favorites.clone()).unwrap_or_default()
    }

    fn launch(&self, id: &str) {
        if let Some(action) = with_state(|s| plan_launch(&s.snapshot, &s.options, id)) {
            carry_out(action);
        }
    }

    fn save_favorites(&self, favorites: &[String]) {
        with_state(|s| {
            // Shown at once; the server gets it in the background (the sample keeps it for the run).
            s.snapshot.favorites = favorites.to_vec();
            s.feed.send(FeedCommand::SaveFavorites(favorites.to_vec()));
        });
    }
}

/// The account panel's service: the current snapshot's user, the shell's other accounts.
struct BrokerAccounts;

impl AccountService for BrokerAccounts {
    fn user(&self) -> AccountUser {
        with_state(|s| s.snapshot.user.clone()).unwrap_or_default()
    }

    fn accounts(&self) -> Vec<AccountEntry> {
        with_state(|s| s.snapshot.others.clone()).unwrap_or_default()
    }

    fn can_administer(&self) -> bool {
        with_state(|s| s.snapshot.is_admin).unwrap_or(false)
    }

    fn act(&self, action: AccountAction) {
        if let Some(planned) = with_state(|s| plan_account(&s.snapshot, &action)) {
            carry_out(planned);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::AppEntry;

    fn snapshot() -> HeaderSnapshot {
        let app = |id: &str, path: &str| AppEntry { id: id.into(), label: id.into(), path: path.into(), icon: "Cloud".into(), module: id.into(), ..AppEntry::default() };
        HeaderSnapshot {
            status: Status::Online,
            account: Some("0123456789abcdef".into()),
            server: "https://cloud.exemple.fr/".into(),
            apps: vec![app("drive", "/drive"), app("chat", "/chat"), app("office-documents", "/office/documents"), app("mail", "/mail")],
            favorites: vec!["mail".into()],
            user: AccountUser { name: "Camille Martin".into(), email: "c@x".into(), initials: "CM".into(), avatar: None },
            is_admin: false,
            others: Vec::new(),
        }
    }

    #[test]
    fn a_tile_focuses_starts_or_opens() {
        let tmp = tempfile::tempdir().expect("tmp");
        let chat = tmp.path().join(format!("kubuno-chat{}", std::env::consts::EXE_SUFFIX));
        std::fs::write(&chat, b"").expect("fake program");
        let mut options = HeaderOptions::for_app(&["office-documents"]);
        options.install_dir = Some(tmp.path().to_path_buf());
        let s = snapshot();
        assert_eq!(plan_launch(&s, &options, "office-documents"), Action::FocusSelf);
        assert_eq!(plan_launch(&s, &options, "chat"), Action::StartApp(chat));
        // Drive's program is not installed here: its web route.
        assert_eq!(plan_launch(&s, &options, "drive"), Action::OpenUrl("https://cloud.exemple.fr/drive".into()));
        assert_eq!(plan_launch(&s, &options, "mail"), Action::OpenUrl("https://cloud.exemple.fr/mail".into()));
        assert_eq!(plan_launch(&s, &options, "nope"), Action::Nothing("unknown app"));
        let signed_out = HeaderSnapshot { server: String::new(), ..s };
        assert!(matches!(plan_launch(&signed_out, &options, "mail"), Action::Nothing(_)));
    }

    #[test]
    fn the_account_panel_s_picks() {
        let mut s = snapshot();
        assert_eq!(plan_account(&s, &AccountAction::ManageAccount), Action::OpenUrl("https://cloud.exemple.fr/settings".into()));
        assert_eq!(plan_account(&s, &AccountAction::OpenLabels), Action::OpenUrl("https://cloud.exemple.fr/labels".into()));
        assert!(matches!(plan_account(&s, &AccountAction::OpenAdmin), Action::Nothing(_)), "not an administrator");
        s.is_admin = true;
        assert_eq!(plan_account(&s, &AccountAction::OpenAdmin), Action::OpenUrl("https://cloud.exemple.fr/admin".into()));
        assert_eq!(plan_account(&s, &AccountAction::OpenAccount("fedcba9876543210".into())), Action::Switch("fedcba9876543210".into()));
        assert!(matches!(plan_account(&s, &AccountAction::OpenAccount("0123456789abcdef".into())), Action::Nothing(_)));
        assert_eq!(plan_account(&s, &AccountAction::SignOut), Action::Shell(ShellRequest::SignOut));
        assert_eq!(plan_account(&s, &AccountAction::AddAccount), Action::Shell(ShellRequest::AddAccount));
    }

    /// The services read the snapshot applied last; a saved list shows at once and goes to the worker; picks
    /// reach the performer, never the browser.
    #[test]
    fn the_services_follow_the_snapshot() {
        let heard = Rc::new(RefCell::new(Vec::new()));
        install(HeaderOptions::for_app(&["chat"]), HeaderFeed::none());
        let sink = heard.clone();
        set_performer(Rc::new(move |a: &Action| sink.borrow_mut().push(a.clone())));
        assert!(BrokerLauncher.apps().is_empty(), "nothing before the first snapshot");
        apply(snapshot());
        assert_eq!(BrokerLauncher.apps().len(), 4);
        assert_eq!(BrokerAccounts.user().initials, "CM");
        BrokerLauncher.save_favorites(&["drive".into(), "unknown".into()]);
        assert_eq!(BrokerLauncher.favorites(), ["drive", "unknown"]);
        BrokerLauncher.launch("chat");
        BrokerLauncher.launch("mail");
        BrokerAccounts.act(AccountAction::SignOut);
        assert_eq!(*heard.borrow(), [Action::FocusSelf, Action::OpenUrl("https://cloud.exemple.fr/mail".into()), Action::Shell(ShellRequest::SignOut)]);
        // A new install keeps the performer (tests and demos install it once).
        install(HeaderOptions::default(), HeaderFeed::none());
        apply(HeaderSnapshot::sample());
        BrokerAccounts.act(AccountAction::ManageAccount);
        assert_eq!(heard.borrow().len(), 4);
    }
}
