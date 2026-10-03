//! Code-behind of Kubuno Desktop's window (`shell_window.kbview`) — Windows Forms' `Form1.cs`.
//!
//! The window owns the data ([`ShellState`]) and fills its pages (user controls) through typed
//! access (`self.launcher_page.with(…)`) from what `view_model` computes. The background work
//! (the launcher's fetch, the labels, the identities, a sign-in) posts its results to the UI thread
//! with the view's `UiDispatcher` (Windows Forms' `BeginInvoke`). The sync loop, the tray icon and
//! the system reach the window as messages, through the form's message hooks (`Form::on_message`):
//! `WM_SYNC_DONE`, `WM_TRAY`, `TaskbarCreated`, `WM_SETTINGCHANGE` (the theme follows the system
//! live), and `WM_SHELL_WORK` (a modal system dialog run outside any frame).
//!
//! Closing the window only hides it (a cancelled `FormClosing`): this is a tray application, the
//! sync keeps running, and « Quitter » in the tray is the way out.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;
use std::time::Instant;

use kubuno::prelude::*;
use kubuno::views::events::{CloseReason, Key};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, RegisterWindowMessageW, WM_APP, WM_SETTINGCHANGE};

use crate::pages::accounts_page::AccountsPage;
use crate::pages::activity_page::ActivityPage;
use crate::services::apps::{Conn, Refreshed};
use crate::services::backend;
use crate::views::confirm_dialog::{ConfirmAction, ConfirmDialog, Confirmation};
use crate::model::events::{CommandEventArgs, ItemCommandEventArgs, SettingEventArgs};
use crate::admin::admin_page::AdminPage;
use crate::model::events::AdminCommandEventArgs;
use crate::pages::labels_page::LabelsPage;
use crate::pages::launcher_page::LauncherPage;
use crate::pages::login_page::LoginPage;
use crate::services::options::{Options, StartPage};
use crate::services::settings::{self, ThemeSetting};
use crate::pages::settings_page::{SettingsPage, SettingsValues};
use kubuno_shell_controls::{AccountAction, AccountButton, AccountService, WaffleButton};
use crate::controls::storage_gauge::StorageGauge;
use crate::model::view_model::{self, AccountInfo, LoginState, ShellState};
use crate::Resources;

/// Which page the window shows. The shell is small enough that a page is a state, not a window:
/// no second window to keep in step.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Page {
    #[default]
    Launcher,
    Settings,
    Accounts,
    Activity,
    Login,
    /// Cross-module labels, reached from the account panel.
    Labels,
    /// The administration console, reached from the account panel when the account may enter it.
    Admin,
}

/// Private message: a modal system dialog to run outside any frame (it runs its own message loop).
pub const WM_SHELL_WORK: u32 = WM_APP + 3;

/// A modal system dialog waiting for the window procedure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Deferred {
    /// The sign-in page's « Parcourir… ».
    PickLoginFolder { current: String },
    /// The accounts page's « Déplacer… » for account `id`, currently synced into `folder`.
    MoveFolder { id: String, folder: String },
}

/// The header's width with the storage gauge, and without it.
const HEADER_ACTIONS_WIDE: f32 = 460.0;
const HEADER_ACTIONS_NARROW: f32 = 216.0;
/// The room the gauge takes in the cluster (its width and the gap after it): what every button
/// after it moves left by when it is hidden.
const GAUGE_ROOM: f32 = HEADER_ACTIONS_WIDE - HEADER_ACTIONS_NARROW;
/// The brand's width with its name, and with its mark alone.
const BRAND_WIDE: f32 = 204.0;
const BRAND_NARROW: f32 = 32.0;

/// Kubuno Desktop's main window.
#[kubuno::view("shell_window.kbview")]
pub struct ShellWindow {
    #[control]
    storage_gauge: Custom<StorageGauge>,
    /// The header's waffle and avatar: they open the launcher and the account panel in popups of
    /// their own (`kubuno-shell-controls`), fed by [`ShellWindow::refresh_header_menus`].
    #[control]
    apps_button: Custom<WaffleButton>,
    #[control]
    account_avatar: Custom<AccountButton>,
    #[control]
    launcher_page: Custom<LauncherPage>,
    #[control]
    settings_page: Custom<SettingsPage>,
    #[control]
    accounts_page: Custom<AccountsPage>,
    #[control]
    activity_page: Custom<ActivityPage>,
    #[control]
    login_page: Custom<LoginPage>,
    #[control]
    labels_page: Custom<LabelsPage>,
    #[control]
    admin_page: Custom<AdminPage>,
    // ── What the window's own view binds to ───────────────────────────────────────────────
    #[bind]
    rail_visible: bool,
    #[bind]
    rail_expanded: bool,
    #[bind]
    nav_key: String,
    #[bind]
    nav_mode: String,
    #[bind]
    connection_color: String,
    #[bind]
    connection_text: String,
    #[bind]
    has_quota: bool,
    /// The header has room for the storage gauge and the brand's name (not under the compact width).
    #[bind]
    show_gauge: bool,
    #[bind]
    header_wide: bool,
    #[bind]
    unread_text: String,
    #[bind]
    has_unread: bool,
    #[bind]
    user_name: String,
    #[bind]
    initials: String,
    #[bind]
    avatar_path: String,
    #[bind]
    on_launcher: bool,
    #[bind]
    on_settings: bool,
    #[bind]
    on_accounts: bool,
    #[bind]
    on_activity: bool,
    #[bind]
    on_login: bool,
    #[bind]
    on_labels: bool,
    #[bind]
    on_admin: bool,
    /// The rail's rows: the pages, and the console's tree while it is open.
    #[bind]
    nav_rows: Rows,
    // ── Data ───────────────────────────────────────────────────────────────────────────────
    state: ShellState,
    options: Options,
    /// Where the background work posts its results (once the window is open).
    ui: Option<UiDispatcher<ShellWindow>>,
    /// The window's handle, once open (what the sync loop posts to).
    hwnd: isize,
    /// An administration section asked for on the command line, held until the identity says
    /// the account may enter the console.
    pending_admin: Option<String>,
    /// The confirmation on screen, and what a « yes » runs.
    confirm: Option<ConfirmAction>,
    /// Modal system dialogs waiting for the window procedure (`WM_SHELL_WORK`).
    deferred: Rc<RefCell<VecDeque<Deferred>>>,
    /// The header shows the gauge (its width and the search button's place follow).
    gauge_shown: Option<(bool, bool)>,
    /// The console's section, the groups of its rail tree shown expanded, and whether the rail
    /// currently lists the tree (its rows are rebuilt only when that changes: the rail keeps the
    /// groups the user opened).
    admin_section: String,
    admin_expanded: Vec<String>,
    rail_has_admin: Option<bool>,
    /// What each section was last asked to load (a page, a search).
    admin_requests: std::collections::HashMap<String, crate::admin::SectionRequest>,
}

/// What the account panel asks the window for (its `AccountAction`s the shell acts on).
#[derive(Debug, Clone, PartialEq)]
pub enum UserAction {
    /// « Gérer votre compte »: the settings page.
    Manage,
    /// Switch to the account (instance) `id`.
    Switch(String),
    AddAccount,
    Labels,
    Admin,
    Logout,
    /// The camera on the avatar: the photo is changed on the web (its crop dialog), in the browser.
    ChangeAvatar,
}

impl UserAction {
    /// The shell's action for a pick of the account panel; `None` for what this panel does not offer
    /// (removing an account: the desktop lists live accounts only).
    pub fn of(action: AccountAction) -> Option<Self> {
        Some(match action {
            AccountAction::ManageAccount => UserAction::Manage,
            AccountAction::OpenAccount(id) => UserAction::Switch(id),
            AccountAction::AddAccount => UserAction::AddAccount,
            AccountAction::OpenLabels => UserAction::Labels,
            AccountAction::OpenAdmin => UserAction::Admin,
            AccountAction::SignOut => UserAction::Logout,
            AccountAction::ChangeAvatar => UserAction::ChangeAvatar,
            // The panel lists live accounts of configured instances only: no « Supprimer » shows.
            AccountAction::RemoveAccount(_) => return None,
        })
    }
}

/// The account panel's data ([`view_model::AccountPanel`]) and where its picks go.
struct ShellAccounts {
    panel: view_model::AccountPanel,
    act: Box<dyn Fn(AccountAction)>,
}

impl AccountService for ShellAccounts {
    fn user(&self) -> kubuno_shell_controls::AccountUser {
        self.panel.user()
    }

    fn accounts(&self) -> Vec<kubuno_shell_controls::AccountEntry> {
        self.panel.accounts()
    }

    fn can_administer(&self) -> bool {
        self.panel.can_administer()
    }

    fn act(&self, action: AccountAction) {
        (self.act)(action);
    }
}

impl ShellWindow {
    pub fn new(options: Options) -> Self {
        backend::set_sample(options.sample);
        if let Some(theme) = options.theme {
            settings::force_theme(theme);
        }
        let mut window = Self { options, nav_mode: "Expanded".into(), rail_visible: true, rail_expanded: true, ..Self::default() };
        window.initialize_component();
        window.state.unread = crate::services::activity::count();
        window.state.offline = backend::is_offline();
        window.state.accounts = accounts();
        // With no account configured there is nothing to launch, so sign-in IS the first screen —
        // and it cannot be cancelled, having nowhere to go back to.
        let start = if window.state.accounts.is_empty() { Some(StartPage::Page(Page::Login)) } else { None };
        let requested = window.options.page.clone();
        match requested.or(start) {
            Some(StartPage::Page(Page::Login)) => window.open_login(!window.state.accounts.is_empty()),
            Some(StartPage::Page(page)) => window.go(page),
            Some(StartPage::Admin(section)) => window.pending_admin = Some(section),
            None => window.go(Page::Launcher),
        }
        if window.options.background {
            // Started at logon: hidden in the notification area until the user opens it.
            let _ = window.form().clone().start_hidden(true);
        }
        window.refresh_all();
        window
    }

    /// The data shown (tests, diagnostics).
    pub fn state(&self) -> &ShellState {
        &self.state
    }

    // ── Pages ────────────────────────────────────────────────────────────────────────────────

    /// Switches page.
    pub fn go(&mut self, page: Page) {
        // Leaving the settings page saves the proxy field, however it is left.
        if self.state.page == Page::Settings && page != Page::Settings {
            self.settings_page.with(|p| p.commit_proxy());
        }
        if page != Page::Login {
            self.state.login = None;
        }
        self.state.page = page;
        match page {
            // Who is signed in is a network call, so it is fetched as the page opens.
            Page::Accounts => self.fetch_identities(),
            Page::Labels => self.fetch_labels(),
            _ => {}
        }
        self.refresh_chrome();
    }

    /// Opens the sign-in page (`cancellable`: there is an account to go back to).
    fn open_login(&mut self, cancellable: bool) {
        self.state.login = Some(LoginState { cancellable, ..LoginState::default() });
        self.login_page.with(|p| p.reset(cancellable));
        self.go(Page::Login);
    }

    /// Pushes everything to the views.
    fn refresh_all(&mut self) {
        self.refresh_chrome();
        self.refresh_launcher();
        self.refresh_settings();
        self.refresh_accounts();
        self.refresh_activity();
        self.refresh_labels();
    }

    /// The header, the rail and which page shows.
    fn refresh_chrome(&mut self) {
        let s = &self.state;
        let page = s.page;
        self.on_launcher = page == Page::Launcher;
        self.on_settings = page == Page::Settings;
        self.on_accounts = page == Page::Accounts;
        self.on_activity = page == Page::Activity;
        self.on_login = page == Page::Login;
        self.on_labels = page == Page::Labels;
        self.on_admin = page == Page::Admin;
        // The rail frames every page except sign-in, which owns the whole window because there is
        // nothing to navigate to yet.
        self.rail_visible = page != Page::Login;
        self.rail_expanded = !s.compact;
        self.nav_mode = if s.compact { "Compact" } else { "Expanded" }.to_string();
        self.nav_key = if page == Page::Admin { format!("{}{}", crate::admin::KEY_PREFIX, self.admin_section) } else { view_model::nav_key(page).to_string() };
        let admin = page == Page::Admin;
        if self.rail_has_admin != Some(admin) {
            self.rail_has_admin = Some(admin);
            self.nav_rows = view_model::nav_rows(admin.then_some(self.admin_expanded.as_slice()));
        }
        let (color, text) = view_model::connection(s.conn);
        self.connection_color = color.to_string();
        self.connection_text = text.to_string();
        // The header: a zero quota hides the gauge, as on the web.
        let id = &s.identity;
        self.has_quota = id.quota > 0;
        // Under the compact width the header keeps only its buttons: the gauge and the brand's name would
        // run under them (the caption buttons take 138 DIP of a 468-DIP window).
        self.header_wide = !s.compact;
        self.show_gauge = self.has_quota && self.header_wide;
        let (used, quota) = (id.used, id.quota);
        self.storage_gauge.with(|g| g.set_usage(used, quota));
        self.unread_text = view_model::unread_text(s.unread);
        self.has_unread = s.unread > 0;
        self.user_name = id.name.clone();
        self.initials = if id.initials.is_empty() { "?".into() } else { id.initials.clone() };
        self.avatar_path = id.avatar.as_ref().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
        self.place_header_actions();
        self.refresh_header_menus();
    }

    /// The header's right cluster: with the gauge, or without it (the search button moves next to
    /// the bell, and the cluster narrows so the band drags where the gauge was).
    fn place_header_actions(&mut self) {
        let shown = self.show_gauge;
        if self.gauge_shown == Some((shown, self.header_wide)) {
            return;
        }
        self.gauge_shown = Some((shown, self.header_wide));
        self.brand.set_size(if self.header_wide { BRAND_WIDE } else { BRAND_NARROW }, 64.0);
        let width = if shown { HEADER_ACTIONS_WIDE } else { HEADER_ACTIONS_NARROW };
        self.header_actions.set_size(width, 64.0);
        // Every button is placed from the design's coordinates, shifted by the gauge's room when it
        // is hidden: resizing the cluster from code does not move its right-anchored children.
        let shift = if shown { 0.0 } else { GAUGE_ROOM };
        self.search_button.set_location(0.0, 14.0);
        self.bell_button.set_location(280.0 - shift, 14.0);
        self.bell_badge.set_location(294.0 - shift, 20.0);
        self.settings_button.set_location(316.0 - shift, 14.0);
        self.help_button.set_location(352.0 - shift, 14.0);
        self.apps_button.set_location(388.0 - shift, 14.0);
        self.account_avatar.set_location(424.0 - shift, 14.0);
    }

    fn refresh_launcher(&mut self) {
        let l = view_model::launcher(&self.state);
        self.launcher_page.with(|p| p.show(&l));
    }

    fn refresh_settings(&mut self) {
        let s = settings::get();
        let values = SettingsValues {
            theme: settings::theme_setting(),
            interval: s.sync_interval_min,
            notifications: s.notifications,
            autostart: settings::autostart_enabled(),
            offline: self.state.offline,
            proxy: backend::get_proxy().unwrap_or_default(),
        };
        self.settings_page.with(|p| p.show(&values));
    }

    fn refresh_accounts(&mut self) {
        let rows = view_model::account_rows(&self.state.accounts);
        self.accounts_page.with(|p| p.show(rows));
        self.refresh_header_menus();
    }

    fn refresh_activity(&mut self) {
        let rows = view_model::activity_rows(&crate::services::activity::events(), Instant::now());
        self.activity_page.with(|p| p.show(rows));
    }

    fn refresh_labels(&mut self) {
        let rows = view_model::label_rows(&self.state.labels);
        let (loading, error) = (self.state.labels_loading, self.state.labels_error.clone());
        self.labels_page.with(|p| p.show(rows, loading, &error));
    }

    // ── Background work ─────────────────────────────────────────────────────────────────────

    /// Reloads the active instance and its module list (the launcher, the header, the waffle).
    pub fn refresh_apps(&mut self) {
        self.state.accounts = accounts();
        let Some(active) = crate::services::apps::active_instance() else {
            self.state.conn = Conn::Offline;
            self.state.status = Resources::no_account().to_string();
            self.refresh_all();
            return;
        };
        self.state.server = active.server_url.clone();
        self.state.folder = active.sync_root.to_string_lossy().into_owned();
        let Some(ui) = self.ui.clone() else { return };
        crate::services::apps::refresh_in_background(active, move |refreshed| {
            drop(ui.begin_invoke(move |w: &mut ShellWindow| w.apply_apps(refreshed)));
        });
        self.refresh_all();
    }

    fn apply_apps(&mut self, r: Refreshed) {
        if let Some(identity) = r.identity {
            self.state.identity = identity;
        }
        self.state.conn = r.conn;
        // Keep the last known tiles when a refresh comes back empty: going offline should dim the
        // launcher, not erase it.
        if !r.apps.is_empty() {
            self.state.apps = r.apps;
        }
        self.open_pending_admin();
        self.refresh_all();
    }

    /// Opens the section `--page admin:<id>` asked for, once the identity says the account may
    /// enter the console (taken, not peeked: a request is honoured once).
    fn open_pending_admin(&mut self) {
        let Some(section) = self.pending_admin.take() else { return };
        if !self.state.identity.is_admin {
            kubuno::tracing::warn!("[shell] --page admin:{section} ignored: this account is not an administrator");
            return;
        }
        self.open_admin(Some(section));
    }

    /// Opens the administration console, at `section` (its landing section when `None`).
    fn open_admin(&mut self, section: Option<String>) {
        // The server enforces every privilege; the console is only offered to who may enter it.
        if !self.state.identity.is_admin {
            return;
        }
        let section = section.filter(|s| crate::admin::is_section(s)).unwrap_or_else(|| crate::admin::DEFAULT_SECTION.to_string());
        if self.state.page != Page::Admin {
            // Entering the console: its tree opens on the section's group.
            self.admin_expanded = crate::admin::groups_of(&section);
            self.rail_has_admin = None;
        }
        self.admin_section = section.clone();
        self.admin_page.with(|p| p.open(&section));
        self.go(Page::Admin);
        self.load_admin(&section);
    }

    /// Loads what section `id` shows, off the UI thread, and hands it to the console.
    fn load_admin(&mut self, id: &str) {
        let Some(instance) = crate::services::apps::active_instance().map(|c| c.id) else { return };
        let Some(ui) = self.ui.clone() else { return };
        let request = self.admin_requests.get(id).cloned().unwrap_or_default();
        /// Runs `load` off the UI thread, then hands its outcome to the console with `apply`.
        fn spawn<T: Send + 'static>(
            ui: UiDispatcher<ShellWindow>,
            instance: String,
            load: impl FnOnce(&str) -> anyhow::Result<T> + Send + 'static,
            apply: fn(&mut AdminPage, crate::admin::SectionState<T>),
        ) {
            std::thread::spawn(move || {
                let r = load(&instance).map_err(|e| e.to_string());
                drop(ui.begin_invoke(move |w: &mut ShellWindow| {
                    w.admin_page.with(|p| apply(p, crate::admin::SectionState::done(r)));
                }));
            });
        }
        self.admin_page.with(|p| p.set_loading(id));
        match id {
            "dashboard" => spawn(ui, instance, crate::admin::admin_dashboard::load, AdminPage::set_dashboard),
            "users" => spawn(ui, instance, move |i| crate::admin::admin_users::load(i, &request), AdminPage::set_users),
            "groups" => spawn(ui, instance, crate::admin::admin_groups::load, AdminPage::set_groups),
            "audiences" => spawn(ui, instance, crate::admin::admin_audiences::load, AdminPage::set_audiences),
            "org-units" => spawn(ui, instance, crate::admin::admin_org_units::load, AdminPage::set_org_units),
            "modules" => spawn(ui, instance, crate::admin::admin_modules::load, AdminPage::set_modules),
            "settings" => spawn(ui, instance, crate::admin::admin_settings::load, AdminPage::set_settings),
            "storage" => spawn(ui, instance, crate::admin::admin_storage::load, AdminPage::set_storage),
            _ => {}
        }
    }

    /// Fetches who is signed in on every instance, off the UI thread.
    fn fetch_identities(&mut self) {
        let Some(ui) = self.ui.clone() else { return };
        std::thread::spawn(move || {
            let found: Vec<(String, String)> = backend::list_instances()
                .into_iter()
                .filter_map(|c| {
                    let (u, _) = backend::current_user(&c.id).ok()?;
                    let name = u.display_name.filter(|n| !n.trim().is_empty()).or_else(|| u.username.clone()).unwrap_or_else(|| u.email.clone());
                    Some((c.id, name))
                })
                .collect();
            drop(ui.begin_invoke(move |w: &mut ShellWindow| {
                for a in &mut w.state.accounts {
                    if let Some((_, name)) = found.iter().find(|(id, _)| *id == a.id) {
                        a.user = name.clone();
                    }
                }
                w.refresh_accounts();
            }));
        });
    }

    /// Re-reads the labels from the server, off the UI thread.
    fn fetch_labels(&mut self) {
        let Some(id) = crate::services::apps::active_instance().map(|c| c.id) else { return };
        self.state.labels_loading = true;
        self.state.labels_error.clear();
        self.refresh_labels();
        let Some(ui) = self.ui.clone() else { return };
        std::thread::spawn(move || {
            let result = backend::labels(&id).map_err(|e| e.to_string());
            drop(ui.begin_invoke(move |w: &mut ShellWindow| w.apply_labels(result)));
        });
    }

    fn apply_labels(&mut self, result: Result<Vec<kubuno_sync::Label>, String>) {
        self.state.labels_loading = false;
        match result {
            Ok(list) => {
                self.state.labels = list;
                self.state.labels_error.clear();
            }
            Err(e) => self.state.labels_error = Resources::labels_unavailable().replace("{0}", &e),
        }
        self.refresh_labels();
    }

    /// Runs a label change on the server, then refetches the list.
    fn change_label(&mut self, change: impl FnOnce(&str) -> anyhow::Result<()> + Send + 'static) {
        let Some(id) = crate::services::apps::active_instance().map(|c| c.id) else { return };
        let Some(ui) = self.ui.clone() else { return };
        std::thread::spawn(move || {
            let result = change(&id).and_then(|()| backend::labels(&id)).map_err(|e| e.to_string());
            drop(ui.begin_invoke(move |w: &mut ShellWindow| w.apply_labels(result)));
        });
    }

    /// A sync cycle (or another background step that wakes the window) ended.
    fn on_sync_done(&mut self) {
        let summary = crate::take_summary();
        if !summary.is_empty() {
            self.state.status = summary;
        }
        self.state.syncing = false;
        self.state.unread = crate::services::activity::count();
        self.refresh_all();
    }

    // ── Actions ──────────────────────────────────────────────────────────────────────────────

    /// « Synchroniser maintenant »: one cycle per instance, off the UI thread (a full push+pull can
    /// run for minutes), recorded in the activity log like any other.
    fn sync_now(&mut self) {
        if self.state.syncing {
            return;
        }
        self.state.syncing = true;
        self.state.status = Resources::sync_busy().to_string();
        self.refresh_launcher();
        let hwnd = self.hwnd;
        std::thread::spawn(move || {
            let mut summary = String::new();
            for c in backend::list_instances() {
                match backend::sync_once(&c.id) {
                    Ok(s) => {
                        summary = view_model::sync_summary(&s);
                        crate::services::activity::record("synced", Resources::manual_sync_title(), &summary);
                    }
                    Err(e) => {
                        summary = Resources::sync_failed().replace("{0}", &e.to_string());
                        crate::services::activity::record("error", Resources::sync_failed_title(), &e.to_string());
                    }
                }
            }
            crate::post_sync_done(hwnd, summary);
        });
    }

    fn open_folder(&self) {
        if let Some(c) = backend::list_instances().into_iter().next() {
            crate::platform::actions::open_path(&c.sync_root.to_string_lossy());
        }
    }

    fn open_web(&self) {
        if !self.state.server.is_empty() {
            crate::platform::actions::open_in_browser(&self.state.server);
        }
    }

    fn toggle_offline(&mut self) {
        if let Err(e) = backend::set_offline(!backend::is_offline()) {
            kubuno::tracing::warn!("[shell] mode hors ligne : {e}");
        }
        self.state.offline = backend::is_offline();
        self.refresh_launcher();
        self.refresh_settings();
    }

    /// Applies the theme preference (and the font) to the windows.
    fn apply_theme(&mut self) {
        kubuno::Application::set_theme(settings::theme());
        kubuno::controls::host::set_font_override(settings::font_override());
    }

    /// Asks before something destructive (an in-window dialog over a veil); `action` runs when
    /// the user confirms.
    fn ask(&mut self, confirmation: Confirmation, action: ConfirmAction) {
        self.confirm = Some(action);
        let ui = self.ui.clone();
        ConfirmDialog::new(&confirmation).show_in_window(self.form(), move |result| {
            if let Some(ui) = ui {
                drop(ui.begin_invoke(move |w: &mut ShellWindow| w.confirmed(result == DialogResult::Ok)));
            }
        });
    }

    fn confirmed(&mut self, yes: bool) {
        let Some(action) = self.confirm.take() else { return };
        if !yes {
            return;
        }
        match action {
            ConfirmAction::DeleteLabel { instance, label_id } => {
                let _ = instance;
                self.change_label(move |id| backend::delete_label(id, &label_id));
            }
            ConfirmAction::RemoveAccount { id } => self.disconnect(&id),
            ConfirmAction::Logout => self.logout(),
        }
    }

    /// Drops an account's credentials and local sync state — the files already downloaded stay on
    /// disk, which is what « disconnect » means — and forgets it as the chosen account. Changes not
    /// sent yet are asked about first (« Envoyer d'abord » / « Exporter » / « Supprimer quand même »).
    fn disconnect(&mut self, id: &str) {
        self.begin_sign_out(id.to_string(), false);
    }

    /// Signs out of the current account, once the dialog is confirmed.
    fn logout(&mut self) {
        let Some(id) = crate::services::apps::active_instance().map(|c| c.id) else { return };
        self.begin_sign_out(id, true);
    }

    /// How the sign-out dialog names an account: its label, else its server's host.
    fn account_label(id: &str) -> String {
        backend::list_instances()
            .into_iter()
            .find(|c| c.id == id)
            .map(|c| {
                c.label.filter(|l| !l.trim().is_empty()).unwrap_or_else(|| c.server_url.split("://").last().unwrap_or(&c.server_url).trim_end_matches('/').to_string())
            })
            .unwrap_or_else(|| id.to_string())
    }

    /// Starts signing account `id` out: asks first when some of its changes were not sent.
    /// `navigate`: leave for the launcher (or the sign-in page when no account is left) afterwards.
    fn begin_sign_out(&mut self, id: String, navigate: bool) {
        let unsent = backend::unsent_count(&id);
        if unsent > 0 {
            self.ask_sign_out(id, unsent, false, navigate);
        } else {
            self.run_sign_out(id, crate::services::session::SignOutChoice::Discard, navigate);
        }
    }

    /// The sign-out dialog; `retry`: « Envoyer d'abord » left changes behind.
    fn ask_sign_out(&mut self, id: String, unsent: u32, retry: bool, navigate: bool) {
        let ui = self.ui.clone();
        let label = Self::account_label(&id);
        crate::views::signout_dialog::SignOutDialog::new(unsent, &label, retry).show_in_window(self.form(), move |result| {
            let Some(choice) = crate::views::signout_dialog::choice_of(result) else { return };
            if let Some(ui) = ui {
                drop(ui.begin_invoke(move |w: &mut ShellWindow| w.run_sign_out(id, choice, navigate)));
            }
        });
    }

    /// Runs the sign-out off the UI thread (« Envoyer d'abord » synchronises first).
    fn run_sign_out(&mut self, id: String, choice: crate::services::session::SignOutChoice, navigate: bool) {
        let Some(ui) = self.ui.clone() else { return };
        std::thread::spawn(move || {
            let outcome = backend::sign_out(&id, choice).map_err(|e| e.to_string());
            drop(ui.begin_invoke(move |w: &mut ShellWindow| w.signed_out(id, outcome, navigate)));
        });
    }

    fn signed_out(&mut self, id: String, outcome: Result<crate::services::session::SignOutOutcome, String>, navigate: bool) {
        use crate::services::session::SignOutOutcome;
        match outcome {
            Ok(SignOutOutcome::StillUnsent(n)) => {
                self.ask_sign_out(id, n, true, navigate);
                return;
            }
            Ok(SignOutOutcome::SignedOut { exported }) => {
                if let Some((dir, n)) = exported {
                    let text = Resources::signout_exported().replace("{0}", &n.to_string()).replace("{1}", &dir.display().to_string());
                    crate::services::activity::record("synced", Resources::signout_export(), &text);
                }
            }
            Err(e) => {
                kubuno::tracing::warn!("[comptes] déconnexion : {e}");
                crate::services::activity::record("error", Resources::disconnect_title(), &e);
                return;
            }
        }
        if !backend::is_sample() && kubuno_account::paths::system_integration_allowed() {
            crate::platform::cloudfiles::unregister(&id);
        }
        settings::update(|s| {
            if s.active_instance == id {
                s.active_instance.clear();
            }
        });
        if !backend::is_sample() {
            crate::services::sync::refresh_explorer_nav();
        }
        self.refresh_apps();
        if navigate {
            // Nothing left to show without an account.
            if backend::list_instances().is_empty() {
                self.open_login(false);
            } else {
                self.go(Page::Launcher);
            }
        } else if self.state.page == Page::Accounts {
            self.refresh_accounts();
        }
    }

    /// An account event of the token owner (on the UI thread): a session revoked elsewhere pauses the sync
    /// (nothing is deleted) and asks to sign in again.
    fn account_event(&mut self, event: kubuno_account::AccountEvent) {
        use kubuno_account::AccountEvent;
        if let AccountEvent::SessionExpired { .. } = &event {
            crate::services::activity::record("expired", Resources::session_expired_reconnect(), Resources::session_paused_text());
            if settings::notifications_enabled() {
                crate::services::sync::toast(Resources::session_expired_reconnect(), Resources::session_paused_text());
            }
            self.state.unread = crate::services::activity::count();
        }
        self.refresh_apps();
    }

    /// Switches the launcher to account `id`.
    fn select_account(&mut self, id: &str) {
        crate::services::session::select(id);
        settings::update(|s| s.active_instance = id.to_string());
        self.refresh_apps();
        self.go(Page::Launcher);
    }

    // ── The header's menus ───────────────────────────────────────────────────────────────────

    /// Hands the header's waffle and avatar (`kubuno-shell-controls`' `WaffleButton` / `AccountButton`,
    /// which open their menus in a popup of their own) what they show now: the instance's apps and
    /// the account's favourites, the signed-in user and the other accounts. A confirmed list of
    /// favourites and the account panel's picks come back to the window.
    fn refresh_header_menus(&mut self) {
        let ui = self.ui.clone();
        let launcher = crate::services::favorites::ShellLauncher::new(self.state.apps.clone(), self.state.identity.favorites.clone(), &self.state.server, move |list| {
            if let Some(ui) = &ui {
                drop(ui.begin_invoke(move |w: &mut ShellWindow| w.favorites_saved(list)));
            }
        });
        // An administrator's launcher lists the console, as the web's does; it opens in this window.
        let launcher = if self.state.identity.is_admin {
            let ui = self.ui.clone();
            launcher.with_admin(move || {
                if let Some(ui) = &ui {
                    drop(ui.begin_invoke(|w: &mut ShellWindow| w.open_admin(None)));
                }
            })
        } else {
            launcher
        };
        self.apps_button.with(|b| b.set_service(Rc::new(launcher)));
        let id = &self.state.identity;
        let panel = view_model::AccountPanel {
            user: kubuno_shell_controls::AccountUser {
                name: id.name.clone(),
                email: id.email.clone(),
                initials: self.initials.clone(),
                avatar: Some(self.avatar_path.clone()).filter(|p| !p.is_empty()),
            },
            others: view_model::other_accounts(&self.state.accounts),
            admin: id.is_admin,
        };
        let ui = self.ui.clone();
        let accounts = ShellAccounts {
            panel,
            act: Box::new(move |action| {
                let Some(action) = UserAction::of(action) else { return };
                if let Some(ui) = &ui {
                    drop(ui.begin_invoke(move |w: &mut ShellWindow| w.user_action(action)));
                }
            }),
        };
        self.account_avatar.with(|b| b.set_service(Rc::new(accounts)));
    }

    /// The launcher's favourites, as the user confirmed them: kept for the next opening and sent to
    /// the server.
    fn favorites_saved(&mut self, list: Vec<String>) {
        self.state.identity.favorites = list.clone();
        crate::services::favorites::persist(list);
    }

    /// What the account panel asked for.
    fn user_action(&mut self, action: UserAction) {
        match action {
            UserAction::Manage => self.go(Page::Settings),
            UserAction::Switch(id) => self.select_account(&id),
            UserAction::AddAccount => self.open_login(true),
            UserAction::Labels => self.go(Page::Labels),
            // The row is only listed for an administrator, but the check belongs here too: this is
            // the door to the console, not a decoration.
            UserAction::Admin if self.state.identity.is_admin => self.open_admin(None),
            UserAction::Admin => {}
            UserAction::ChangeAvatar => {
                if !self.state.server.is_empty() {
                    crate::platform::actions::open_in_browser(&format!("{}/settings", self.state.server.trim_end_matches('/')));
                }
            }
            // Signing out is the accounts page's « Déconnecter » applied to the CURRENT account,
            // asked first like it.
            UserAction::Logout => {
                let Some(current) = self.state.accounts.iter().find(|a| a.active).cloned() else { return };
                self.ask(Confirmation::disconnect(&view_model::account_name(&current)), ConfirmAction::Logout);
            }
        }
    }

    /// Moves an account's sync folder to `target`, picked by the user. Relocating moves real
    /// files: the engine holds the sync lock for the whole operation.
    fn move_folder(&mut self, id: &str, folder: &str, target: &str) {
        if target == folder {
            return;
        }
        match backend::move_instance_folder(id, target) {
            Ok(()) => {
                crate::services::activity::record("synced", Resources::folder_moved(), &format!("{folder} → {target}"));
                if !backend::is_sample() {
                    crate::services::sync::refresh_explorer_nav();
                }
                self.refresh_apps();
            }
            Err(e) => {
                kubuno::tracing::warn!("[comptes] déplacement du dossier : {e}");
                crate::services::activity::record("error", Resources::move_failed(), &e.to_string());
            }
        }
        self.state.unread = crate::services::activity::count();
        self.refresh_all();
    }

    /// Queues a modal system dialog for the window procedure (it runs its own message loop, so it
    /// is not run inside a frame).
    fn defer(&mut self, work: Deferred) {
        self.deferred.borrow_mut().push_back(work);
        // SAFETY: posting a message to our own window has no memory-safety requirement; a failure
        // is only a lost request.
        unsafe {
            let _ = PostMessageW(Some(HWND(self.hwnd as *mut _)), WM_SHELL_WORK, WPARAM(0), LPARAM(0));
        }
    }

    /// Signs in with what the page holds, off the UI thread.
    fn submit_login(&mut self) {
        let Some(fields) = self.login_page.with(|p| p.fields()) else { return };
        if !fields.is_complete() || self.login_page.with_ref(|p| p.is_busy()).unwrap_or(false) {
            return;
        }
        self.login_page.with(|p| p.set_busy(true));
        let Some(ui) = self.ui.clone() else { return };
        let totp_session = self.state.login.as_ref().and_then(|l| l.totp_session.clone()).filter(|_| fields.needs_code);
        std::thread::spawn(move || {
            let outcome = match totp_session {
                Some(session) => backend::login_code(fields.server.trim(), &session, &fields.code, fields.folder.trim()),
                None => backend::login(fields.server.trim(), fields.login.trim(), &fields.password, fields.folder.trim()),
            }
            .map_err(|e| e.to_string());
            drop(ui.begin_invoke(move |w: &mut ShellWindow| w.apply_login(outcome)));
        });
    }

    fn apply_login(&mut self, outcome: Result<crate::services::session::SignIn, String>) {
        match outcome {
            Ok(crate::services::session::SignIn::NeedsCode { totp_session }) => {
                // Two-factor authentication: the code row replaces the password row.
                if let Some(login) = self.state.login.as_mut() {
                    login.totp_session = Some(totp_session);
                }
                self.login_page.with(|p| p.ask_code());
            }
            Ok(crate::services::session::SignIn::Done { .. }) => {
                if let Some(login) = self.state.login.as_mut() {
                    login.totp_session = None;
                }
                self.login_page.with(|p| p.set_busy(false));
                // A new instance means a new Explorer entry and a launcher reload.
                if !backend::is_sample() {
                    crate::services::sync::refresh_explorer_nav();
                }
                self.refresh_apps();
                self.go(Page::Launcher);
            }
            Err(e) => {
                self.login_page.with(|p| {
                    p.set_busy(false);
                    p.set_error(&e);
                });
            }
        }
    }

    /// Runs what the tray menu asked for.
    fn run_tray(&mut self, command: crate::platform::tray::Command) {
        use crate::platform::tray::Command;
        match command {
            Command::SyncNow => self.sync_now(),
            Command::OpenFolder => self.open_folder(),
            Command::Show => {
                self.set_visible(true);
                kubuno::controls::host::restore_and_focus();
            }
            Command::Quit => kubuno::Application::exit(),
        }
    }

    // ── The window's events ──────────────────────────────────────────────────────────────────

    fn shell_window_load(&mut self, _sender: &Form, _e: &EventArgs) {
        self.hwnd = self.handle().unwrap_or(0);
        self.ui = self.dispatcher();
        if let Some(font) = settings::font_override() {
            kubuno::controls::host::set_font_override(Some(font));
        }
        if self.options.sample {
            for (kind, title, body) in backend::sample_activity().iter().rev() {
                crate::services::activity::record(kind, title, body);
            }
            self.state.unread = crate::services::activity::count();
        }
        self.install_hooks();
        self.refresh_apps();
        if self.state.page == Page::Accounts {
            self.fetch_identities();
        }
        if self.state.page == Page::Labels {
            self.fetch_labels();
        }
        if let Some(login) = self.state.login.clone() {
            self.login_page.with(|p| p.reset(login.cancellable));
        }
        // The parts that make this a sync client rather than a launcher: the tray icon, one entry
        // per instance in Explorer, then the continuous loop. The sample registers none of them.
        if !self.options.sample {
            // The token owner's events (a session revoked elsewhere, an account added or removed).
            let ui = self.ui.clone();
            crate::services::session::set_event_handler(move |event| {
                if let Some(ui) = &ui {
                    drop(ui.begin_invoke(move |w: &mut ShellWindow| w.account_event(event)));
                }
            });
            crate::platform::tray::add(HWND(self.hwnd as *mut _));
            crate::splash_step(Resources::splash_explorer(), 0.85);
            crate::services::sync::refresh_explorer_nav();
            // Only the shell that owns the accounts synchronises (a second shell borrows its tokens and would
            // only run every cycle twice).
            if crate::services::session::is_owner() {
                crate::services::sync::start(self.hwnd);
            }
        }
    }

    fn shell_window_shown(&mut self, _sender: &Form, _e: &EventArgs) {
        self.shell_window_resize();
    }

    /// The rail collapses to its icons on a narrow window.
    fn shell_window_resize(&mut self) {
        let (width, _) = self.get_client_size();
        let compact = view_model::rail_compact(width);
        if compact != self.state.compact {
            self.state.compact = compact;
            let w = if compact { view_model::RAIL_NARROW } else { view_model::RAIL_WIDE };
            self.rail.set_size(w, 736.0);
            self.refresh_chrome();
        }
    }

    fn shell_window_key_down(&mut self, e: &mut KeyEventArgs) {
        if e.key != Key(kubuno::controls::host::vk::ESCAPE) {
            return;
        }
        match self.state.page {
            Page::Launcher => return,
            // Cancelling sign-in is only an option when there is somewhere to go back to.
            Page::Login if !self.state.login.as_ref().is_some_and(|l| l.cancellable) => return,
            _ => self.go(Page::Launcher),
        }
        e.handled = true;
    }

    fn shell_window_form_closing(&mut self, e: &mut FormClosingEventArgs) {
        // A tray app: closing the window only hides it, the sync keeps running and « Quitter » in
        // the tray is the way out. The sample has no tray icon to come back from: it quits.
        if e.reason == CloseReason::UserClosing && !self.options.sample {
            e.cancel = true;
            self.hide();
        }
    }

    fn shell_window_form_closed(&mut self) {
        if !self.options.sample {
            crate::platform::tray::remove(HWND(self.hwnd as *mut _));
        }
    }

    fn clock_tick(&mut self) {
        if self.state.page == Page::Activity {
            self.refresh_activity();
        }
    }

    /// The message hooks: the tray, the sync loop, the system's settings, the deferred dialogs.
    fn install_hooks(&mut self) {
        let Some(ui) = self.ui.clone() else { return };
        let post = ui.clone();
        self.on_message(crate::platform::tray::WM_TRAY, move |m| {
            if let Some(command) = crate::platform::tray::on_message(m.hwnd, m.lparam.0 as u32) {
                drop(post.begin_invoke(move |w: &mut ShellWindow| w.run_tray(command)));
            }
            Some(0)
        });
        // Explorer restarted: the notification area is new and empty.
        // SAFETY: registering a named message has no memory-safety requirement.
        let taskbar_created = unsafe { RegisterWindowMessageW(windows::core::w!("TaskbarCreated")) };
        if taskbar_created != 0 && !self.options.sample {
            self.on_message(taskbar_created, |m| {
                crate::platform::tray::add(m.hwnd);
                None
            });
        }
        let post = ui.clone();
        self.on_message(crate::WM_SYNC_DONE, move |_| {
            drop(post.begin_invoke(|w: &mut ShellWindow| w.on_sync_done()));
            Some(0)
        });
        // The system theme or a system setting changed: « Système » follows it, live.
        let post = ui.clone();
        self.on_message(WM_SETTINGCHANGE, move |_| {
            drop(post.begin_invoke(|w: &mut ShellWindow| {
                if settings::theme_setting() == ThemeSetting::System {
                    w.apply_theme();
                }
            }));
            None
        });
        let queue = self.deferred.clone();
        self.on_message(WM_SHELL_WORK, move |m| {
            let work = queue.borrow_mut().pop_front();
            match work {
                Some(Deferred::PickLoginFolder { current }) => {
                    if let Some(folder) = crate::platform::folder_picker::pick(m.hwnd, Resources::login_folder_picker_title(), &current) {
                        drop(ui.begin_invoke(move |w: &mut ShellWindow| {
                            w.login_page.with(|p| p.set_folder(&folder));
                        }));
                    }
                }
                Some(Deferred::MoveFolder { id, folder }) => {
                    if let Some(target) = crate::platform::folder_picker::pick(m.hwnd, Resources::move_folder_picker_title(), &folder) {
                        drop(ui.begin_invoke(move |w: &mut ShellWindow| w.move_folder(&id, &folder, &target)));
                    }
                }
                None => {}
            }
            Some(0)
        });
    }

    // ── The header ───────────────────────────────────────────────────────────────────────────

    fn storage_gauge_click(&mut self) {
        // The gauge links to the storage page of the web app.
        if !self.state.server.is_empty() {
            crate::platform::actions::open_in_browser(&format!("{}/drive/storage", self.state.server.trim_end_matches('/')));
        }
    }

    fn bell_button_click(&mut self) {
        self.go(Page::Activity);
    }

    fn settings_button_click(&mut self) {
        self.go(Page::Settings);
    }

    // ── The rail and the pages ───────────────────────────────────────────────────────────────

    fn nav_item_invoked(&mut self, e: &TextChangedEventArgs) {
        if let Some(section) = e.new.strip_prefix(crate::admin::KEY_PREFIX) {
            // A section of the console's tree (its groups only expand).
            if crate::admin::is_section(section) {
                self.open_admin(Some(section.to_string()));
            }
        } else if let Some(page) = view_model::page_of_key(&e.new) {
            self.go(page);
        }
    }

    fn admin_page_command(&mut self, e: &AdminCommandEventArgs) {
        match e.command.as_str() {
            // A section shown only on the web, or a section's item edited there.
            "open-web" => crate::platform::actions::open_in_browser(&format!("{}{}", self.state.server.trim_end_matches('/'), e.value)),
            // Another page of a list, or a search.
            "load" => {
                let request = self.admin_requests.entry(e.section.clone()).or_default();
                match e.id.as_str() {
                    "page" => request.page = e.value.parse().unwrap_or(0),
                    "query" => {
                        request.query = e.value.clone();
                        request.page = 0;
                    }
                    _ => {}
                }
                let section = e.section.clone();
                self.load_admin(&section);
            }
            // A module's service switched for everyone (shown at once by the section), then the list
            // reloaded to confirm what the server holds.
            "set-module" => {
                let Some(instance) = crate::services::apps::active_instance().map(|c| c.id) else { return };
                let Some(ui) = self.ui.clone() else { return };
                let (module, enabled) = (e.id.clone(), e.value == "true");
                std::thread::spawn(move || {
                    if let Err(err) = backend::set_module_enabled(&instance, &module, enabled) {
                        kubuno::tracing::warn!("[admin] module {module}: {err}");
                    }
                    drop(ui.begin_invoke(|w: &mut ShellWindow| w.load_admin("modules")));
                });
            }
            other => kubuno::tracing::warn!("[shell] unknown console command {other}"),
        }
    }

    fn launcher_page_command(&mut self, e: &CommandEventArgs) {
        match e.command.as_str() {
            "sync_now" => self.sync_now(),
            "open_folder" => self.open_folder(),
            "open_web" => self.open_web(),
            "toggle_offline" => self.toggle_offline(),
            other => kubuno::tracing::warn!("[shell] unknown launcher command {other}"),
        }
    }

    fn settings_page_setting_changed(&mut self, e: &SettingEventArgs) {
        let value = e.value.as_str();
        match e.name.as_str() {
            "theme" => {
                settings::update(|s| s.theme = ThemeSetting::from_key(value));
                self.apply_theme();
            }
            "interval" => {
                let minutes = value.parse::<u32>().unwrap_or(5).clamp(settings::INTERVAL_MIN, settings::INTERVAL_MAX);
                settings::update(|s| s.sync_interval_min = minutes);
            }
            "notifications" => settings::update(|s| s.notifications = value == "true"),
            "autostart" => {
                // The Run key is the source of truth here.
                if let Err(e) = settings::set_autostart(value == "true") {
                    kubuno::tracing::warn!("[settings] démarrage automatique : {e}");
                }
            }
            // Lives in the engine's own config, not in the shell's settings.
            "offline" => {
                if (value == "true") != backend::is_offline() {
                    self.toggle_offline();
                }
            }
            "proxy" => {
                let proxy = (!value.trim().is_empty()).then(|| value.trim().to_string());
                if let Err(e) = backend::set_proxy(proxy) {
                    kubuno::tracing::warn!("[settings] proxy : {e}");
                }
            }
            other => kubuno::tracing::warn!("[shell] unknown setting {other}"),
        }
        self.refresh_settings();
    }

    fn accounts_page_account_command(&mut self, e: &ItemCommandEventArgs) {
        let account = self.state.accounts.iter().find(|a| a.id == e.id).cloned();
        match (e.command.as_str(), account) {
            ("select", Some(a)) => self.select_account(&a.id),
            ("move", Some(a)) => self.defer(Deferred::MoveFolder { id: a.id, folder: a.folder }),
            ("disconnect", Some(a)) => {
                // Asked first: the sync of that account stops.
                let name = view_model::account_name(&a);
                self.ask(Confirmation::disconnect(&name), ConfirmAction::RemoveAccount { id: a.id });
            }
            ("add", _) => self.open_login(true),
            _ => {}
        }
    }

    fn login_page_command(&mut self, e: &CommandEventArgs) {
        match e.command.as_str() {
            "submit" => self.submit_login(),
            "browse" => {
                let current = self.login_page.with_ref(|p| p.folder.clone()).unwrap_or_default();
                self.defer(Deferred::PickLoginFolder { current });
            }
            "cancel" => self.go(Page::Launcher),
            _ => {}
        }
    }

    fn labels_page_label_command(&mut self, e: &ItemCommandEventArgs) {
        match e.command.as_str() {
            "create" => {
                let (name, colour) = (e.value.clone(), view_model::next_colour(self.state.labels.len()).to_string());
                self.change_label(move |id| backend::create_label(id, &name, &colour));
            }
            "delete" => {
                let Some(label) = self.state.labels.iter().find(|l| l.id == e.id) else { return };
                let Some(instance) = crate::services::apps::active_instance().map(|c| c.id) else { return };
                // Labels span every module, so removing one unlabels items the user may not have
                // in view: ask first, as the web does.
                self.ask(Confirmation::delete_label(&label.name), ConfirmAction::DeleteLabel { instance, label_id: label.id.clone() });
            }
            "colour" => {
                // Optimistic: the swatch answers at once, and the refetch confirms.
                if let Some(l) = self.state.labels.iter_mut().find(|l| l.id == e.id) {
                    l.color = Some(e.value.clone());
                }
                self.refresh_labels();
                let (label, colour) = (e.id.clone(), e.value.clone());
                self.change_label(move |id| backend::update_label(id, &label, None, Some(&colour)));
            }
            _ => {}
        }
    }
}

/// The configured instances, the current one marked.
fn accounts() -> Vec<AccountInfo> {
    let list = backend::list_instances();
    // With nothing chosen yet, the first instance is the current one — the same rule the
    // launcher applies.
    let stored = settings::get().active_instance;
    let current = if list.iter().any(|c| c.id == stored) { stored } else { list.first().map(|c| c.id.clone()).unwrap_or_default() };
    list.into_iter()
        .map(|c| AccountInfo {
            active: c.id == current,
            server_url: c.server_url,
            folder: c.sync_root.to_string_lossy().into_owned(),
            label: c.label,
            user: String::new(),
            id: c.id,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use kubuno::views::events::ChangeSource;

    fn sample_window(args: &[&str]) -> ShellWindow {
        kubuno::resources::set_culture("fr");
        let mut all = vec!["--sample".to_string()];
        all.extend(args.iter().map(|s| s.to_string()));
        ShellWindow::new(Options::parse(all))
    }

    #[test]
    fn a_new_window_opens_on_the_launcher() {
        let w = sample_window(&[]);
        assert_eq!(w.state().page, Page::Launcher);
        assert!(w.on_launcher && !w.on_settings && w.rail_visible);
        assert_eq!(w.nav_key, "launcher");
        assert_eq!(w.state().accounts.len(), 2);
    }

    #[test]
    fn the_command_line_opens_a_page() {
        let w = sample_window(&["--page", "settings"]);
        assert_eq!(w.state().page, Page::Settings);
        assert_eq!(w.nav_key, "settings");
        let w = sample_window(&["--page", "login"]);
        assert!(w.on_login && !w.rail_visible, "sign-in owns the whole window");
        assert!(w.state().login.as_ref().is_some_and(|l| l.cancellable), "there is an account to go back to");
        let w = sample_window(&["--page", "admin:users"]);
        assert_eq!(w.pending_admin.as_deref(), Some("users"), "held until the identity arrives");
    }

    #[test]
    fn pages_switch_and_the_rail_follows() {
        let mut w = sample_window(&[]);
        w.go(Page::Activity);
        assert!(w.on_activity && !w.on_launcher);
        assert_eq!(w.nav_key, "activity");
        w.go(Page::Labels);
        assert_eq!(w.nav_key, "", "the labels have no rail row");
        let mut e = TextChangedEventArgs::new(String::new(), "accounts".into(), ChangeSource::User);
        w.nav_item_invoked(&e);
        assert_eq!(w.state().page, Page::Accounts);
        e.new = "nowhere".into();
        w.nav_item_invoked(&e);
        assert_eq!(w.state().page, Page::Accounts);
    }

    #[test]
    fn the_header_hides_the_gauge_without_a_quota() {
        let mut w = sample_window(&[]);
        w.state.identity.quota = 0;
        w.refresh_chrome();
        assert!(!w.has_quota);
        w.state.identity = crate::model::view_model::ShellState::design().identity;
        w.state.unread = 12;
        w.refresh_chrome();
        assert!(w.has_quota && w.has_unread);
        assert_eq!((w.unread_text.as_str(), w.initials.as_str()), ("9+", "CM"));
        assert!(w.show_gauge && w.header_wide);
    }

    #[test]
    fn the_compact_header_keeps_only_its_buttons() {
        let mut w = sample_window(&[]);
        w.state.identity = crate::model::view_model::ShellState::design().identity;
        w.state.compact = true;
        w.refresh_chrome();
        // The quota is known, but there is no room for the gauge nor the brand's name.
        assert!(w.has_quota && !w.show_gauge && !w.header_wide);
        assert_eq!(w.gauge_shown, Some((false, false)));
    }

    /// The rail of the view links every page the window knows by its key.
    #[test]
    fn the_rail_rows_are_the_pages() {
        let view = include_str!("shell_window.kbview");
        for (_, key, _) in view_model::NAV {
            assert!(view.contains(&format!("Key=\"{key}\"")), "no rail row for {key}");
        }
    }

    #[test]
    fn escape_goes_back_to_the_launcher() {
        let mut w = sample_window(&["--page", "settings"]);
        let mut e = KeyEventArgs::new(Key(kubuno::controls::host::vk::ESCAPE), Default::default());
        w.shell_window_key_down(&mut e);
        assert!(e.handled && w.state().page == Page::Launcher);
    }
}
