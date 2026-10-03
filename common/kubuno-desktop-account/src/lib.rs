//! Accounts of the Kubuno desktop (vskubuno `docs/DESKTOP-OFFLINE-SYNC.md` §5, §9, §10).
//!
//! - [`AccountKey`]: the identity of an account = normalized server URL + user id;
//! - [`AccountStore`]: `<data>/accounts/<key>/account.json` (no secret);
//! - [`TokenOwner`]: the only holder of refresh tokens (in the OS store, `kubuno-desktop-secrets`), run by the shell:
//!   single-flight rotation, adopt-fresh, persist-before-use, rotation grace, genuine/transient failures, revoked
//!   sessions, account switch, events;
//! - the token **broker** ([`broker`]): a local named pipe / Unix socket over which apps borrow access tokens
//!   ([`broker::BrokerServer`] in the shell, [`broker::BrokerClient`] and its `TokenSource` in the apps);
//! - [`login`]: native sign-in including the TOTP step;
//! - [`migrate`]: the one-time move of the legacy plaintext `creds.json` into the OS store.
//!
//! # How the shell wires it (after its own migration lot)
//!
//! ```text
//! let secrets: Arc<dyn SecretStore> = Arc::new(kubuno_desktop_secrets::OsSecretStore::new());   // probe() on Linux first
//! let accounts = AccountStore::new(kubuno_desktop_sync_engine::paths::user_data_dir()?);
//! let report = migrate::adopt_legacy_instances(&legacy_root, &accounts, secrets.as_ref(), &|_| None);
//! let owner = TokenOwner::new(secrets, accounts, OwnerConfig { proxy, ..Default::default() });
//! owner.load().await?;
//! let endpoint = BrokerEndpoint::for_current_user(&kubuno_desktop_sync_engine::paths::user_runtime_dir()?)?;
//! tokio::spawn(BrokerServer::new(endpoint, owner.clone(), ClientPolicy::ImagesUnder(vec![install_dir])).serve(shutdown));
//! // Sign-in page: login::login(...) -> TotpRequired? -> login::login_totp(...) -> owner.sign_in(server, tokens)
//! // Its own API calls: ApiClient::builder(server).tokens(Arc::new(OwnerTokenSource { owner, account })).build()
//! ```
//!
//! An app: [`app::AppBroker::for_app`]`("kubuno-chat")` (verifies that the server is the installed shell, starts it
//! with `--background` when it is not running) and [`app::AppTokenSource`] as the `ApiClient`'s token source.

#[cfg(feature = "client")]
pub mod app;
pub mod broker;
pub mod key;
pub mod login;
pub mod migrate;
pub mod owner;
pub mod paths;
pub mod store;

pub use key::{normalize_server_url, AccountKey, KeyError};
pub use owner::{
    AccountEvent, AccountSummary, Borrowed, BrokerBackend, OwnerConfig, OwnerTokenSource, SessionStatus, TokenOwner,
};
pub use store::{AccountError, AccountInfo, AccountStore};
