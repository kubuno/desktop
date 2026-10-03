//! Secrets of the Kubuno desktop (vskubuno `docs/DESKTOP-OFFLINE-SYNC.md` §10): the refresh token of each account
//! and the key of each local database live in the **OS credential store**, never in a plain file.
//!
//! | OS | Back-end | Notes |
//! |---|---|---|
//! | Windows | Credential Manager, generic credentials, `CRED_PERSIST_LOCAL_MACHINE` | DPAPI-protected per user; never roams with a roaming profile (two machines presenting one refresh token would revoke its family); blob <= 2560 bytes |
//! | macOS | Keychain generic password (`keyring`, `apple-native`) | device-local login keychain, not iCloud-synced |
//! | Linux / BSD | freedesktop Secret Service over D-Bus (`keyring`, `zbus`) | GNOME Keyring, KWallet; **absent on headless or minimal sessions**: see [`OsSecretStore::probe`] |
//!
//! Every secret is addressed by a [`SecretName`] (`Kubuno/<account_key>/<item>`). The [`SecretStore`] trait is
//! synchronous: the OS calls block (D-Bus round trips, a Keychain prompt), so async callers wrap them in
//! `spawn_blocking`. [`MemorySecretStore`] backs the tests; [`FileSecretStore`] is the **opt-in** fallback for a
//! Linux session without a Secret Service (a `0600` file, which the user must accept explicitly with a warning).
//!
//! Secret values are [`Secret`]s: zeroed on drop, `Debug` prints `Secret(<redacted>)`. Errors name the secret,
//! never its value.

use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use zeroize::Zeroizing;

#[cfg(all(feature = "os", windows))]
mod windows_cred;
#[cfg(all(feature = "os", unix))]
mod keyring_store;

/// The service part of every name: what the Credential Manager / Keychain shows.
pub const SERVICE: &str = "Kubuno";

/// Item of an account's refresh token.
pub const ITEM_REFRESH_TOKEN: &str = "refresh";
/// Item of an account's local database key (one key per account, every app database of the account).
pub const ITEM_DB_KEY: &str = "dbkey";

/// Errors of a secret store. They name the secret, never its value.
#[derive(Debug, thiserror::Error)]
pub enum SecretError {
    /// The OS store does not exist or cannot be reached (no Secret Service on a headless Linux, locked keychain).
    #[error("the OS credential store is unavailable: {0}")]
    Unavailable(String),
    /// The name is not acceptable (empty part, forbidden character).
    #[error("invalid secret name: {0}")]
    InvalidName(String),
    /// The value does not fit the back-end (Windows: 2560 bytes).
    #[error("the secret {name} is too large for {backend} ({len} bytes, at most {max})")]
    TooLarge { name: String, backend: &'static str, len: usize, max: usize },
    /// The stored bytes are not what the caller expected (not UTF-8...).
    #[error("the secret {0} is corrupted")]
    Corrupted(String),
    /// Any other failure of the back-end (its message, never the value).
    #[error("{backend}: {message}")]
    Backend { backend: &'static str, message: String },
}

/// A secret value: zeroed when dropped, never printed.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(Zeroizing<Vec<u8>>);

impl Secret {
    pub fn new(bytes: Vec<u8>) -> Self {
        Self(Zeroizing::new(bytes))
    }

    pub fn from_string(s: String) -> Self {
        Self::new(s.into_bytes())
    }

    pub fn from_str_value(s: &str) -> Self {
        Self::new(s.as_bytes().to_vec())
    }

    /// The raw bytes. Keep the borrow short; never log them.
    pub fn expose(&self) -> &[u8] {
        &self.0
    }

    /// The value as text (tokens, hex keys).
    pub fn expose_str(&self) -> Result<&str, SecretError> {
        std::str::from_utf8(&self.0).map_err(|_| SecretError::Corrupted("<utf-8>".to_string()))
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// A fresh random 256-bit key, hex-encoded (64 characters): the form a database key is stored in.
    pub fn random_key_hex() -> Result<Self, SecretError> {
        let mut raw = Zeroizing::new([0u8; 32]);
        getrandom::getrandom(raw.as_mut_slice())
            .map_err(|e| SecretError::Backend { backend: "getrandom", message: e.to_string() })?;
        Ok(Self::from_string(hex::encode(raw.as_slice())))
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(<redacted>)")
    }
}

/// The address of a secret: `Kubuno/<scope>/<item>`, `scope` being an account key (or `app` for app-wide items).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SecretName {
    scope: String,
    item: String,
}

fn valid_part(s: &str) -> bool {
    !s.is_empty() && s.len() <= 64 && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

impl SecretName {
    pub fn new(scope: &str, item: &str) -> Result<Self, SecretError> {
        if !valid_part(scope) {
            return Err(SecretError::InvalidName(format!("scope '{scope}'")));
        }
        if !valid_part(item) {
            return Err(SecretError::InvalidName(format!("item '{item}'")));
        }
        Ok(Self { scope: scope.to_string(), item: item.to_string() })
    }

    /// The refresh token of an account.
    pub fn refresh_token(account_key: &str) -> Result<Self, SecretError> {
        Self::new(account_key, ITEM_REFRESH_TOKEN)
    }

    /// The database key of an account.
    pub fn db_key(account_key: &str) -> Result<Self, SecretError> {
        Self::new(account_key, ITEM_DB_KEY)
    }

    pub fn scope(&self) -> &str {
        &self.scope
    }

    pub fn item(&self) -> &str {
        &self.item
    }

    /// The target name stored in the OS store: `Kubuno/<scope>/<item>`.
    pub fn target(&self) -> String {
        format!("{SERVICE}/{}/{}", self.scope, self.item)
    }
}

impl fmt::Display for SecretName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.target())
    }
}

/// A place secrets are kept. Implementations are blocking; see the crate doc.
pub trait SecretStore: Send + Sync + fmt::Debug {
    /// For logs and diagnostics (`"windows-credential-manager"`, `"memory"`...).
    fn backend(&self) -> &'static str;
    /// The secret stored under `name`, `None` when there is none.
    fn get(&self, name: &SecretName) -> Result<Option<Secret>, SecretError>;
    /// Creates or replaces the secret. Returns only once the store has it (persist-before-use).
    fn set(&self, name: &SecretName, value: &Secret) -> Result<(), SecretError>;
    /// Removes the secret; `Ok(false)` when there was none.
    fn delete(&self, name: &SecretName) -> Result<bool, SecretError>;

    /// Removes every known item of a scope (an account's sign-out wipe).
    fn delete_scope(&self, scope: &str) -> Result<(), SecretError> {
        for item in [ITEM_REFRESH_TOKEN, ITEM_DB_KEY] {
            self.delete(&SecretName::new(scope, item)?)?;
        }
        Ok(())
    }
}

/// Returns the secret under `name`, creating it with `make` first when there is none (a database key at the first
/// open of an account). The value is read back after writing, so a store that silently drops writes is caught here
/// rather than when the database cannot be opened any more.
pub fn get_or_create(
    store: &dyn SecretStore,
    name: &SecretName,
    make: impl FnOnce() -> Result<Secret, SecretError>,
) -> Result<Secret, SecretError> {
    if let Some(existing) = store.get(name)? {
        return Ok(existing);
    }
    let value = make()?;
    store.set(name, &value)?;
    match store.get(name)? {
        Some(read) if read == value => Ok(read),
        _ => Err(SecretError::Backend {
            backend: store.backend(),
            message: format!("{name} was not kept by the store after writing it"),
        }),
    }
}

/// An in-memory store: tests, `--sample` runs, and session-only sign-in when the user refused the file fallback.
#[derive(Default)]
pub struct MemorySecretStore {
    map: Mutex<HashMap<String, Secret>>,
}

impl MemorySecretStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of secrets held (tests).
    pub fn len(&self) -> usize {
        self.map.lock().unwrap_or_else(PoisonError::into_inner).len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl fmt::Debug for MemorySecretStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MemorySecretStore").field("len", &self.len()).finish()
    }
}

impl SecretStore for MemorySecretStore {
    fn backend(&self) -> &'static str {
        "memory"
    }

    fn get(&self, name: &SecretName) -> Result<Option<Secret>, SecretError> {
        Ok(self.map.lock().unwrap_or_else(PoisonError::into_inner).get(&name.target()).cloned())
    }

    fn set(&self, name: &SecretName, value: &Secret) -> Result<(), SecretError> {
        self.map.lock().unwrap_or_else(PoisonError::into_inner).insert(name.target(), value.clone());
        Ok(())
    }

    fn delete(&self, name: &SecretName) -> Result<bool, SecretError> {
        Ok(self.map.lock().unwrap_or_else(PoisonError::into_inner).remove(&name.target()).is_some())
    }
}

/// A store whose scopes all carry a fixed prefix (`<prefix>.<scope>`): a sandboxed profile (tests, demos,
/// `KUBUNO_SANDBOX_DIR`) keeps its secrets apart from the real ones of the same user in the same OS store, so a
/// sandboxed run can never read, overwrite or delete a real account's refresh token.
#[derive(Debug)]
pub struct PrefixedSecretStore<S: SecretStore> {
    inner: S,
    prefix: String,
}

impl<S: SecretStore> PrefixedSecretStore<S> {
    /// `prefix` must be a valid name part (`[A-Za-z0-9._-]`, at most 40 characters so that
    /// `<prefix>.<account key>` stays within the 64 characters of a scope).
    pub fn new(inner: S, prefix: &str) -> Result<Self, SecretError> {
        if !valid_part(prefix) || prefix.len() > 40 {
            return Err(SecretError::InvalidName(format!("prefix '{prefix}'")));
        }
        Ok(Self { inner, prefix: prefix.to_string() })
    }

    pub fn inner(&self) -> &S {
        &self.inner
    }

    fn map(&self, name: &SecretName) -> Result<SecretName, SecretError> {
        SecretName::new(&format!("{}.{}", self.prefix, name.scope()), name.item())
    }
}

impl<S: SecretStore> SecretStore for PrefixedSecretStore<S> {
    fn backend(&self) -> &'static str {
        self.inner.backend()
    }

    fn get(&self, name: &SecretName) -> Result<Option<Secret>, SecretError> {
        self.inner.get(&self.map(name)?)
    }

    fn set(&self, name: &SecretName, value: &Secret) -> Result<(), SecretError> {
        self.inner.set(&self.map(name)?, value)
    }

    fn delete(&self, name: &SecretName) -> Result<bool, SecretError> {
        self.inner.delete(&self.map(name)?)
    }
}

/// The opt-in file fallback: a JSON map `{target: hex(value)}` in a file only its owner can read (`0600` on Unix).
/// For a Linux session without a Secret Service, after the user accepted the warning; never chosen silently.
/// On Windows the file inherits the profile's ACL only: use the Credential Manager there.
#[derive(Debug)]
pub struct FileSecretStore {
    path: PathBuf,
    lock: Mutex<()>,
}

impl FileSecretStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into(), lock: Mutex::new(()) }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn read(&self) -> Result<HashMap<String, String>, SecretError> {
        match std::fs::read_to_string(&self.path) {
            Ok(text) => serde_json::from_str(&text).map_err(|e| {
                // The parser's message may quote the content: keep only the position.
                SecretError::Backend {
                    backend: "file",
                    message: format!("{} is not valid (line {}, column {})", self.path.display(), e.line(), e.column()),
                }
            }),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(HashMap::new()),
            Err(e) => Err(SecretError::Backend { backend: "file", message: format!("cannot read {}: {e}", self.path.display()) }),
        }
    }

    fn write(&self, map: &HashMap<String, String>) -> Result<(), SecretError> {
        let io = |e: std::io::Error| SecretError::Backend { backend: "file", message: format!("cannot write {}: {e}", self.path.display()) };
        let text = Zeroizing::new(
            serde_json::to_string(map).map_err(|e| SecretError::Backend { backend: "file", message: e.to_string() })?,
        );
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir).map_err(io)?;
        }
        write_private(&self.path, text.as_bytes()).map_err(io)
    }
}

/// Writes `bytes` to `path` atomically (temporary file + rename) with owner-only permissions on Unix.
pub fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let tmp = path.with_extension("tmp");
    {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    std::fs::rename(&tmp, path)
}

impl SecretStore for FileSecretStore {
    fn backend(&self) -> &'static str {
        "file"
    }

    fn get(&self, name: &SecretName) -> Result<Option<Secret>, SecretError> {
        let _guard = self.lock.lock().unwrap_or_else(PoisonError::into_inner);
        let map = self.read()?;
        match map.get(&name.target()) {
            None => Ok(None),
            Some(h) => hex::decode(h).map(|b| Some(Secret::new(b))).map_err(|_| SecretError::Corrupted(name.target())),
        }
    }

    fn set(&self, name: &SecretName, value: &Secret) -> Result<(), SecretError> {
        let _guard = self.lock.lock().unwrap_or_else(PoisonError::into_inner);
        let mut map = self.read()?;
        map.insert(name.target(), hex::encode(value.expose()));
        self.write(&map)
    }

    fn delete(&self, name: &SecretName) -> Result<bool, SecretError> {
        let _guard = self.lock.lock().unwrap_or_else(PoisonError::into_inner);
        let mut map = self.read()?;
        let had = map.remove(&name.target()).is_some();
        if had {
            self.write(&map)?;
        }
        Ok(had)
    }
}

/// Whether the OS store can be used right now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Availability {
    Available,
    /// With the reason (no D-Bus session, no Secret Service, locked keychain...).
    Unavailable(String),
}

/// The OS credential store of the current platform.
#[cfg(feature = "os")]
#[derive(Debug, Default)]
pub struct OsSecretStore {
    #[cfg(windows)]
    inner: windows_cred::WindowsCredentialStore,
    #[cfg(unix)]
    inner: keyring_store::KeyringStore,
}

#[cfg(feature = "os")]
impl OsSecretStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Checks the store by reading a name that does not exist. On Linux this is how a missing Secret Service is
    /// detected before a sign-in: the caller then offers session-only sign-in or, after a warning, the
    /// [`FileSecretStore`].
    pub fn probe(&self) -> Availability {
        let name = match SecretName::new("probe", "probe") {
            Ok(n) => n,
            Err(e) => return Availability::Unavailable(e.to_string()),
        };
        match self.get(&name) {
            Ok(_) => Availability::Available,
            Err(e) => Availability::Unavailable(e.to_string()),
        }
    }
}

#[cfg(feature = "os")]
impl SecretStore for OsSecretStore {
    fn backend(&self) -> &'static str {
        self.inner.backend()
    }

    fn get(&self, name: &SecretName) -> Result<Option<Secret>, SecretError> {
        self.inner.get(name)
    }

    fn set(&self, name: &SecretName, value: &Secret) -> Result<(), SecretError> {
        self.inner.set(name, value)
    }

    fn delete(&self, name: &SecretName) -> Result<bool, SecretError> {
        self.inner.delete(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_validated_and_formatted() {
        let n = SecretName::refresh_token("0123456789abcdef").expect("name");
        assert_eq!(n.target(), "Kubuno/0123456789abcdef/refresh");
        assert!(SecretName::new("a/b", "refresh").is_err());
        assert!(SecretName::new("", "refresh").is_err());
        assert!(SecretName::new("abc", "re fresh").is_err());
    }

    #[test]
    fn secret_debug_is_redacted() {
        let s = Secret::from_str_value("super-secret-token");
        assert_eq!(format!("{s:?}"), "Secret(<redacted>)");
    }

    #[test]
    fn random_keys_are_hex_and_distinct() {
        let a = Secret::random_key_hex().expect("key");
        let b = Secret::random_key_hex().expect("key");
        assert_eq!(a.len(), 64);
        assert!(a.expose_str().expect("utf8").chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, b);
    }

    fn exercise(store: &dyn SecretStore) {
        let n = SecretName::new("test-scope", "item").expect("name");
        assert!(store.get(&n).expect("get").is_none());
        store.set(&n, &Secret::from_str_value("v1")).expect("set");
        assert_eq!(store.get(&n).expect("get").expect("some").expose(), b"v1");
        store.set(&n, &Secret::from_str_value("v2")).expect("replace");
        assert_eq!(store.get(&n).expect("get").expect("some").expose(), b"v2");
        assert!(store.delete(&n).expect("delete"));
        assert!(!store.delete(&n).expect("delete twice"));
        assert!(store.get(&n).expect("get").is_none());
    }

    #[test]
    fn memory_store_roundtrip() {
        exercise(&MemorySecretStore::new());
    }

    #[test]
    fn file_store_roundtrip_and_permissions() {
        let dir = tempfile::tempdir().expect("tmp");
        let store = FileSecretStore::new(dir.path().join("secrets.json"));
        exercise(&store);
        store.set(&SecretName::new("s", "i").expect("n"), &Secret::from_str_value("x")).expect("set");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(store.path()).expect("meta").permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }

    #[test]
    fn prefixed_store_keeps_its_secrets_apart() {
        let inner = MemorySecretStore::new();
        let real = SecretName::refresh_token("0123456789abcdef").expect("name");
        inner.set(&real, &Secret::from_str_value("real")).expect("set");
        let sandbox = PrefixedSecretStore::new(inner, "sandbox.1a2b3c4d").expect("prefix");
        exercise(&sandbox);
        // The sandbox neither sees, replaces nor deletes the real secret of the same account key.
        assert!(sandbox.get(&real).expect("get").is_none());
        sandbox.set(&real, &Secret::from_str_value("sbx")).expect("set");
        sandbox.delete_scope("0123456789abcdef").expect("wipe");
        assert_eq!(sandbox.inner().get(&real).expect("get").expect("kept").expose(), b"real");
        assert!(PrefixedSecretStore::new(MemorySecretStore::new(), "a/b").is_err());
    }

    #[test]
    fn get_or_create_creates_once() {
        let store = MemorySecretStore::new();
        let n = SecretName::db_key("acc").expect("n");
        let a = get_or_create(&store, &n, Secret::random_key_hex).expect("create");
        let b = get_or_create(&store, &n, || panic!("must not create twice")).expect("read");
        assert_eq!(a, b);
    }

    /// The real OS store. Writes and deletes one throw-away credential (`Kubuno/kubuno-selftest/...`). Ignored by
    /// default: on Linux CI there is usually no Secret Service.
    #[cfg(feature = "os")]
    #[test]
    #[ignore = "touches the real OS credential store; run with --ignored"]
    fn os_store_roundtrip() {
        let store = OsSecretStore::new();
        assert_eq!(store.probe(), Availability::Available);
        let n = SecretName::new("kubuno-selftest", "item").expect("name");
        let _ = store.delete(&n);
        exercise(&store);
    }
}
