//! The accounts of the machine, owned by the shell (vskubuno `docs/DESKTOP-OFFLINE-SYNC.md` §5.2, §9, §10, §19.4).
//!
//! At start ([`start`]), before any window:
//!
//! 1. the plaintext `creds.json` of the file-sync instances are moved into the OS credential store
//!    (`kubuno_desktop_account::migrate::adopt_legacy_instances`: written and read back before the file is deleted;
//!    idempotent, crash-safe);
//! 2. the `TokenOwner` loads the accounts: the shell is the **only** holder of refresh tokens (rotation with the
//!    server's grace window, revoked session, account switch);
//! 3. the file sync (`kubuno_desktop_sync`) is given the owner as its token provider: it never reads a token file again;
//! 4. the **token broker** is bound (named pipe restricted to the current user, remote clients refused, clients
//!    filtered by executable: only programs installed next to the shell) and served: the apps borrow access
//!    tokens from it. If another shell already serves it, this one becomes a broker client instead of a second
//!    token owner, and runs no file sync.
//!
//! Sign-in ([`sign_in`], [`sign_in_code`]) includes the two-factor step. Sign-out ([`sign_out_instance`]) asks
//! first when changes were not sent (« Envoyer d'abord » / « Exporter » / « Supprimer quand même »). A session
//! revoked remotely only pauses the sync (nothing is deleted) and is reported through [`set_event_handler`].
//!
//! A sandboxed profile (`KUBUNO_SANDBOX_DIR`, see `kubuno_desktop_account::paths`) moves every file under that directory,
//! keeps its secrets under a prefix of their own and gets its own broker pipe.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use kubuno_desktop::tracing;
use kubuno_desktop_account::broker::{BrokerEndpoint, BrokerServer, ClientPolicy};
use kubuno_desktop_account::login::{self, LoginOutcome, NativeTokens, TotpCode};
use kubuno_desktop_account::{paths, AccountEvent, AccountInfo, AccountKey, AccountStore, OwnerConfig, TokenOwner};
use kubuno_desktop_api_client::ApiClient;
use kubuno_desktop_secrets::{OsSecretStore, PrefixedSecretStore, SecretStore};

/// How this process holds the accounts.
enum Mode {
    /// The token owner and the broker's server.
    Owner(Arc<TokenOwner>),
    /// Another shell owns the tokens: this one borrows from its broker.
    Client,
}

struct Session {
    runtime: tokio::runtime::Runtime,
    mode: Mode,
}

static SESSION: OnceLock<Session> = OnceLock::new();

type EventHandler = Box<dyn Fn(AccountEvent) + Send>;
static HANDLER: Mutex<Option<EventHandler>> = Mutex::new(None);

/// What the start found, for the log and the window.
#[derive(Debug, Default, Clone)]
pub struct Startup {
    /// `creds.json` files moved into the OS store this time.
    pub migrated: usize,
    /// `creds.json` files that could not be moved (left untouched, the account shows "session expired").
    pub migration_failed: usize,
    /// Another shell owns the accounts: this one is a broker client.
    pub client_mode: bool,
}

/// The secret store of this run: the OS store, under a prefix of its own in a sandbox.
fn secret_store() -> Result<Arc<dyn SecretStore>> {
    match paths::sandbox_tag() {
        Some(tag) => Ok(Arc::new(PrefixedSecretStore::new(OsSecretStore::new(), &format!("sandbox-{tag}"))?)),
        None => Ok(Arc::new(OsSecretStore::new())),
    }
}

fn normalized(server: &str) -> String {
    kubuno_desktop_account::normalize_server_url(server).unwrap_or_else(|_| server.trim().trim_end_matches('/').to_ascii_lowercase())
}

/// Links the file-sync instances that belong to no account to the only account of their server, if there is
/// exactly one (an instance added by `kubuno-sync add`, or one whose migration could not read the user id).
fn link_orphans(accounts: &AccountStore) {
    let Ok(all) = accounts.list() else { return };
    for inst in kubuno_desktop_sync::list_instances() {
        if all.iter().any(|a| a.linked_instances.contains(&inst.id)) {
            continue;
        }
        let server = normalized(&inst.server_url);
        let candidates: Vec<&AccountInfo> = all.iter().filter(|a| a.server_url == server).collect();
        if let [only] = candidates.as_slice() {
            match accounts.link_instance(&only.key, &inst.id) {
                Ok(_) => tracing::info!("[session] sync folder {} linked to its account {}", inst.id, only.key),
                Err(e) => tracing::warn!("[session] cannot link {}: {e}", inst.id),
            }
        }
    }
}

/// Starts the accounts (see the module doc). Called once by `main`, never under the offline sample.
pub fn start() -> Result<Startup> {
    let runtime = tokio::runtime::Builder::new_multi_thread().worker_threads(2).thread_name("kubuno-accounts").enable_all().build()?;
    let mut startup = Startup::default();
    let secrets = secret_store()?;
    let accounts = AccountStore::new(paths::user_data_dir()?);

    // 1. The legacy plaintext tokens move into the OS store (the user id comes from the stored token's `sub`).
    let legacy = paths::legacy_config_dir()?;
    let report = kubuno_desktop_account::migrate::adopt_legacy_instances(&legacy, &accounts, secrets.as_ref(), &|_| None);
    startup.migrated = report.migrated();
    startup.migration_failed = report.failed();
    if startup.migrated > 0 || startup.migration_failed > 0 {
        tracing::info!("[session] legacy credentials: {} moved to the {} store, {} left", startup.migrated, secrets.backend(), startup.migration_failed);
    }
    link_orphans(&accounts);

    // 2. The broker first: it is also what tells whether another shell already owns the accounts.
    let endpoint = BrokerEndpoint::for_current_user(&paths::user_runtime_dir()?)?;
    let install_dir = std::env::current_exe()?.parent().map(Path::to_path_buf).ok_or_else(|| anyhow!("the shell has no directory"))?;
    let owner = TokenOwner::new(secrets, accounts, OwnerConfig { proxy: kubuno_desktop_sync::get_proxy(), ..OwnerConfig::default() });
    let bound = runtime.block_on(BrokerServer::new(endpoint, owner.clone(), ClientPolicy::ImagesUnder(vec![install_dir])).bind());
    let mode = match bound {
        Ok(bound) => {
            // 3. The owner, the file sync's tokens, the events, then the broker.
            runtime.block_on(owner.load())?;
            kubuno_desktop_sync::tokens::install(Arc::new(kubuno_desktop_sync::tokens::OwnerProvider::new(owner.clone(), runtime.handle().clone())));
            let mut events = owner.subscribe();
            runtime.spawn(async move {
                loop {
                    match events.recv().await {
                        Ok(ev) => {
                            if let Some(handler) = HANDLER.lock().unwrap_or_else(PoisonError::into_inner).as_ref() {
                                handler(ev);
                            }
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    }
                }
            });
            runtime.spawn(async move {
                if let Err(e) = bound.serve(std::future::pending::<()>()).await {
                    tracing::error!("[session] the token broker stopped: {e}");
                }
            });
            Mode::Owner(owner)
        }
        Err(e) => {
            // Another shell serves the broker: borrow from it, never become a second token owner.
            tracing::warn!("[session] the token broker is already served ({e}): this shell borrows its tokens");
            match kubuno_desktop_sync::tokens::BrokerProvider::for_app("kubuno-desktop") {
                Ok(p) => kubuno_desktop_sync::tokens::install(Arc::new(p)),
                Err(e) => tracing::error!("[session] no token broker client: {e}"),
            }
            startup.client_mode = true;
            Mode::Client
        }
    };
    let _ = SESSION.set(Session { runtime, mode });
    Ok(startup)
}

/// Whether this process owns the accounts (false under the sample, before [`start`], or as a broker client).
pub fn is_owner() -> bool {
    matches!(SESSION.get().map(|s| &s.mode), Some(Mode::Owner(_)))
}

/// Receives the account events (session expired, restored, account added or removed), on a background thread.
pub fn set_event_handler(handler: impl Fn(AccountEvent) + Send + 'static) {
    *HANDLER.lock().unwrap_or_else(PoisonError::into_inner) = Some(Box::new(handler));
}

fn owner() -> Result<(&'static Session, Arc<TokenOwner>)> {
    let session = SESSION.get().ok_or_else(|| anyhow!("les comptes ne sont pas démarrés"))?;
    match &session.mode {
        Mode::Owner(o) => Ok((session, o.clone())),
        Mode::Client => bail!("Kubuno Desktop est déjà ouvert : utilisez sa fenêtre (zone de notification)."),
    }
}

/// What a sign-in attempt answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignIn {
    /// Signed in: the sync folder's instance id.
    Done { instance: String },
    /// The account has two-factor authentication: ask for the code, then call [`sign_in_code`].
    NeedsCode { totp_session: String },
}

fn api_for(server: &str) -> Result<ApiClient> {
    Ok(ApiClient::builder(server.trim()).timeout(Duration::from_secs(30)).proxy(kubuno_desktop_sync::get_proxy()).build()?)
}

/// A user-facing reason for a failed sign-in (never the server's raw body).
fn sign_in_error(e: kubuno_desktop_api_client::ApiError) -> anyhow::Error {
    match e.status() {
        Some(401) | Some(403) => anyhow!("Identifiant, mot de passe ou code incorrect."),
        Some(429) => anyhow!("Trop de tentatives : réessayez dans une minute."),
        Some(code) => anyhow!("Connexion refusée par le serveur (HTTP {code})."),
        None => anyhow!("Serveur injoignable : {e}"),
    }
}

/// Completes a sign-in: the owner stores the refresh token (OS store) and identifies the user, then the sync
/// folder is registered (or found again: same server and folder) and linked to the account.
fn finish(session: &Session, owner: &Arc<TokenOwner>, server: &str, tokens: NativeTokens, folder: &str) -> Result<SignIn> {
    let o = owner.clone();
    let server_owned = server.trim().to_string();
    let info = session.runtime.block_on(async move { o.sign_in(&server_owned, tokens).await }).map_err(|e| anyhow!("{e}"))?;
    let instance = kubuno_desktop_sync::register_instance(&info.server_url, folder.trim())?;
    owner.account_store().link_instance(&info.key, &instance)?;
    // A plaintext token left by an older version for this folder is abandoned now (the new session replaces it).
    if let Ok(dir) = kubuno_desktop_sync::config::instance_dir(&instance) {
        if let Ok(true) = kubuno_desktop_account::migrate::discard_legacy_creds(&dir) {
            tracing::info!("[session] the leftover plaintext credentials of {instance} were deleted");
        }
    }
    if let Err(e) = owner.switch(&info.key) {
        tracing::warn!("[session] switch to the new account: {e}");
    }
    Ok(SignIn::Done { instance })
}

/// Signs in to `server` with a login and a password (step 1; see [`SignIn`]).
pub fn sign_in(server: &str, user: &str, password: &str, folder: &str) -> Result<SignIn> {
    let (session, owner) = owner()?;
    let api = api_for(server)?;
    let (user, password) = (user.trim().to_string(), password.to_string());
    let outcome = session.runtime.block_on(async move { login::login(&api, &user, &password).await }).map_err(sign_in_error)?;
    match outcome {
        LoginOutcome::SignedIn(tokens) => finish(session, &owner, server, tokens, folder),
        LoginOutcome::TotpRequired { totp_session } => Ok(SignIn::NeedsCode { totp_session }),
    }
}

/// The two-factor step: a 6-8 digit code from the authenticator app, or a backup code.
pub fn sign_in_code(server: &str, totp_session: &str, code: &str, folder: &str) -> Result<SignIn> {
    let (session, owner) = owner()?;
    let api = api_for(server)?;
    let code = code.trim();
    let code = if code.len() >= 6 && code.len() <= 8 && code.chars().all(|c| c.is_ascii_digit()) {
        TotpCode::Code(code.to_string())
    } else {
        TotpCode::Backup(code.to_string())
    };
    let totp_session = totp_session.to_string();
    let tokens = session.runtime.block_on(async move { login::login_totp(&api, &totp_session, &code).await }).map_err(sign_in_error)?;
    finish(session, &owner, server, tokens, folder)
}

/// How many changes of a sync folder are not sent yet.
pub fn unsent_count(instance: &str) -> u32 {
    kubuno_desktop_sync::unsent_changes(instance).map(|ops| u32::try_from(ops.len()).unwrap_or(u32::MAX)).unwrap_or(0)
}

/// What the user chose when changes were not sent (§10, §18.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignOutChoice {
    /// « Envoyer d'abord » (the default): synchronise, then sign out only if nothing is left.
    SendFirst,
    /// « Exporter »: copy the files of the unsent changes into a folder, then sign out.
    Export,
    /// « Supprimer quand même »: sign out, the unsent changes are dropped.
    Discard,
}

/// The end of a sign-out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignOutOutcome {
    /// Signed out; with the export folder and the number of files copied when exporting.
    SignedOut { exported: Option<(PathBuf, u32)> },
    /// « Envoyer d'abord » could not send everything (server unreachable…): still signed in.
    StillUnsent(u32),
}

/// Where « Exporter » copies the files: the user's Documents (the sandbox's `exports` in a sandbox).
fn export_dir(instance: &str) -> Result<PathBuf> {
    let base = match paths::sandbox_dir() {
        Some(s) => s.join("exports"),
        None => documents_dir().ok_or_else(|| anyhow!("le dossier Documents est introuvable"))?,
    };
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    Ok(base.join(format!("Kubuno - modifications non envoyées - {instance} - {stamp}")))
}

/// The user's Documents known folder.
fn documents_dir() -> Option<PathBuf> {
    use windows::Win32::UI::Shell::{FOLDERID_Documents, SHGetKnownFolderPath, KF_FLAG_DEFAULT};
    // SAFETY: a valid KNOWNFOLDERID; the returned string is freed below.
    let p = unsafe { SHGetKnownFolderPath(&FOLDERID_Documents, KF_FLAG_DEFAULT, None) }.ok()?;
    // SAFETY: on success `p` is a NUL-terminated UTF-16 string allocated by the API.
    let s = unsafe { p.to_string() }.ok();
    // SAFETY: freeing the buffer the API allocated.
    unsafe { windows::Win32::System::Com::CoTaskMemFree(Some(p.0 as *const _)) };
    s.map(PathBuf::from)
}

/// Signs a sync folder out: its local sync state goes (the files already downloaded stay on disk), and when it
/// was the account's last folder the account is signed out (server logout, refresh token and database key
/// deleted from the OS store, account directory removed). With unsent changes, `choice` says what happens first.
pub fn sign_out_instance(instance: &str, choice: SignOutChoice) -> Result<SignOutOutcome> {
    let mut exported = None;
    match choice {
        SignOutChoice::SendFirst => {
            if let Err(e) = kubuno_desktop_sync::sync_once(instance) {
                tracing::warn!("[session] sending before the sign-out failed: {e}");
            }
            let left = unsent_count(instance);
            if left > 0 {
                return Ok(SignOutOutcome::StillUnsent(left));
            }
        }
        SignOutChoice::Export => {
            let dest = export_dir(instance)?;
            let n = kubuno_desktop_sync::export_unsent(instance, &dest).context("export des modifications non envoyées")?;
            exported = Some((dest, n));
        }
        SignOutChoice::Discard => {}
    }
    let account = match SESSION.get().map(|s| &s.mode) {
        Some(Mode::Owner(o)) => o.account_store().account_of_instance(instance).ok().flatten().map(|a| a.key),
        _ => None,
    };
    kubuno_desktop_sync::remove_instance(instance)?;
    if let Some(key) = account {
        sign_out_account_if_unused(&key, instance)?;
    }
    Ok(SignOutOutcome::SignedOut { exported })
}

/// Unlinks `instance` from `key` and signs the account out when no folder is left.
fn sign_out_account_if_unused(key: &AccountKey, instance: &str) -> Result<()> {
    let (session, owner) = owner()?;
    let left = owner.account_store().unlink_instance(key, instance)?.map(|a| a.linked_instances.len()).unwrap_or(0);
    if left > 0 {
        return Ok(());
    }
    let o = owner.clone();
    let k = key.clone();
    if let Err(e) = session.runtime.block_on(async move { o.sign_out(&k).await }) {
        // Not in memory (never loaded): the secrets are still deleted below through a fresh scope wipe.
        tracing::warn!("[session] sign-out of {key}: {e}");
        secret_store()?.delete_scope(key.as_str())?;
    }
    owner.account_store().remove_dir(key)?;
    tracing::info!("[session] account {key} signed out: its secrets and local data are deleted");
    Ok(())
}

/// The account a sync folder belongs to, as the apps see it.
pub fn account_of(instance: &str) -> Option<AccountInfo> {
    match SESSION.get().map(|s| &s.mode) {
        Some(Mode::Owner(o)) => o.account_store().account_of_instance(instance).ok().flatten(),
        _ => None,
    }
}

/// Makes the account of `instance` the current one (the apps follow it through the broker).
pub fn select(instance: &str) {
    if let (Some(info), Ok((_, owner))) = (account_of(instance), owner()) {
        if let Err(e) = owner.switch(&info.key) {
            tracing::warn!("[session] switch: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_urls_are_compared_normalized() {
        assert_eq!(normalized("HTTPS://Cloud.Exemple.fr:443/"), "https://cloud.exemple.fr");
        assert_eq!(normalized("https://cloud.exemple.fr"), normalized("https://cloud.exemple.fr/"));
    }

    #[test]
    fn nothing_is_owned_before_the_start() {
        assert!(!is_owner());
        assert!(owner().is_err());
    }
}
