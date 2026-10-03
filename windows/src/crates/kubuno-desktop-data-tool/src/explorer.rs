//! The Data Explorer's connection list (`explorer.*`).
//!
//! The list (name, provider, store, a redacted description) is `connections.json` under the tool's
//! home; the connection string itself goes ONLY to the chosen secret store: a Windows Credential
//! Manager generic credential `Kubuno:DataExplorer:ConnectionStrings:<name>`, or the user secrets
//! file `UserSecrets\DataExplorer\secrets.json` key `ConnectionStrings:<name>` — the same stores
//! (and the same chain) a project's connection is resolved from.

use std::sync::Mutex;

use kubuno_desktop_data::secrets::SecretSource;
use kubuno_desktop_data_model::ProviderName;
use serde::{Deserialize, Serialize};

use crate::connstr::ConnInfo;
use crate::error::{ToolError, ToolResult};
use crate::home::Home;

/// The user secrets id of the Data Explorer.
pub const EXPLORER_ID: &str = "DataExplorer";

/// Serialises the read-modify-write cycles of the list file (requests run concurrently).
static LIST_LOCK: Mutex<()> = Mutex::new(());

/// Where a connection string is kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Store {
    CredMan,
    UserSecrets,
}

impl Store {
    pub fn parse(text: &str) -> ToolResult<Self> {
        match text {
            "credman" => Ok(Store::CredMan),
            "usersecrets" => Ok(Store::UserSecrets),
            other => Err(ToolError::validation(format!("unknown store `{other}` (credman or usersecrets)"))),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Store::CredMan => "credman",
            Store::UserSecrets => "usersecrets",
        }
    }
}

/// An entry of the list, as `explorer.list` returns it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub name: String,
    pub provider: String,
    pub store: String,
    pub display: String,
}

#[derive(Serialize, Deserialize)]
struct ListFile {
    version: u32,
    connections: Vec<Entry>,
}

/// `explorer.add`'s parameters.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddRequest {
    pub name: String,
    pub provider: String,
    pub connection_string: String,
    pub store: String,
    #[serde(default)]
    pub overwrite: bool,
}

/// The secret key of a Data Explorer connection.
pub fn secret_key(name: &str) -> String {
    format!("ConnectionStrings:{name}")
}

/// 1-64 characters of `[A-Za-z0-9 _.-]`, not starting or ending with a space.
pub fn validate_name(name: &str) -> ToolResult<()> {
    let ok = !name.is_empty()
        && name.chars().count() <= 64
        && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, ' ' | '_' | '.' | '-'))
        && name.trim() == name;
    if ok {
        Ok(())
    } else {
        Err(ToolError::validation("a connection name is 1 to 64 characters: letters, digits, space, '_', '.' and '-'"))
    }
}

// ---- secret stores ---------------------------------------------------------------------------

/// Reads the secret `key` of the store of the application `id`.
pub fn read_secret(home: &Home, store: Store, id: &str, key: &str) -> ToolResult<Option<String>> {
    match store {
        Store::UserSecrets => Ok(home.user_secrets(id)?.get(key)?),
        Store::CredMan => credman_get(id, key),
    }
}

/// Writes the secret `key` into the store of the application `id`.
pub fn write_secret(home: &Home, store: Store, id: &str, key: &str, value: &str) -> ToolResult<()> {
    match store {
        Store::UserSecrets => Ok(home.user_secrets(id)?.set(key, value)?),
        Store::CredMan => credman_set(id, key, value),
    }
}

/// Removes the secret `key`; `Ok(false)` when there was none.
pub fn remove_secret(home: &Home, store: Store, id: &str, key: &str) -> ToolResult<bool> {
    match store {
        Store::UserSecrets => Ok(home.user_secrets(id)?.remove(key)?),
        Store::CredMan => credman_remove(id, key),
    }
}

#[cfg(all(windows, feature = "credential-manager"))]
fn credman_get(id: &str, key: &str) -> ToolResult<Option<String>> {
    Ok(kubuno_desktop_data::secrets::CredentialManager::for_id(id)?.get(key)?)
}
#[cfg(all(windows, feature = "credential-manager"))]
fn credman_set(id: &str, key: &str, value: &str) -> ToolResult<()> {
    Ok(kubuno_desktop_data::secrets::CredentialManager::for_id(id)?.set(key, value)?)
}
#[cfg(all(windows, feature = "credential-manager"))]
fn credman_remove(id: &str, key: &str) -> ToolResult<bool> {
    Ok(kubuno_desktop_data::secrets::CredentialManager::for_id(id)?.remove(key)?)
}

#[cfg(not(all(windows, feature = "credential-manager")))]
fn credman_get(_id: &str, _key: &str) -> ToolResult<Option<String>> {
    Err(ToolError::config("the Windows Credential Manager is not available in this build"))
}
#[cfg(not(all(windows, feature = "credential-manager")))]
fn credman_set(_id: &str, _key: &str, _value: &str) -> ToolResult<()> {
    Err(ToolError::config("the Windows Credential Manager is not available in this build"))
}
#[cfg(not(all(windows, feature = "credential-manager")))]
fn credman_remove(_id: &str, _key: &str) -> ToolResult<bool> {
    Err(ToolError::config("the Windows Credential Manager is not available in this build"))
}

// ---- the list file ---------------------------------------------------------------------------

fn read_list(home: &Home) -> ToolResult<ListFile> {
    let path = home.connections_file();
    match std::fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str::<ListFile>(text.trim_start_matches('\u{feff}')).map_err(|e| {
            tracing::error!(error = %e, "the Data Explorer list is not valid JSON");
            ToolError::new("Io", format!("the Data Explorer list ({}) is not valid: {e}", path.display()))
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(ListFile { version: 1, connections: Vec::new() }),
        Err(e) => Err(ToolError::io("cannot read the Data Explorer list", &e)),
    }
}

/// Writes through a temporary file and a rename: a crash never leaves half a list.
fn write_list(home: &Home, list: &ListFile) -> ToolResult<()> {
    let path = home.connections_file();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| ToolError::io("cannot create the Data Explorer folder", &e))?;
    }
    let text = serde_json::to_string_pretty(list).map_err(|e| ToolError::new("Io", format!("cannot serialize the Data Explorer list: {e}")))?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text).map_err(|e| ToolError::io("cannot write the Data Explorer list", &e))?;
    std::fs::rename(&tmp, &path).map_err(|e| ToolError::io("cannot replace the Data Explorer list", &e))
}

fn lock() -> std::sync::MutexGuard<'static, ()> {
    LIST_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

// ---- operations ------------------------------------------------------------------------------

pub fn list(home: &Home) -> ToolResult<Vec<Entry>> {
    let _guard = lock();
    Ok(read_list(home)?.connections)
}

/// One entry, `Validation` error when the name is unknown.
pub fn find(home: &Home, name: &str) -> ToolResult<Entry> {
    let _guard = lock();
    read_list(home)?
        .connections
        .into_iter()
        .find(|e| e.name.eq_ignore_ascii_case(name))
        .ok_or_else(|| ToolError::validation(format!("no Data Explorer connection named `{name}`")))
}

/// The connection string of an entry, from its store.
pub fn connection_string(home: &Home, entry: &Entry) -> ToolResult<String> {
    let store = Store::parse(&entry.store)?;
    read_secret(home, store, EXPLORER_ID, &secret_key(&entry.name))?
        .ok_or_else(|| ToolError::secret(format!("the connection string of `{}` is no longer in the {} store: remove the connection and add it again", entry.name, entry.store)))
}

/// Adds (or, with `overwrite`, replaces) a connection: the string into its store first, then the
/// list; a list failure removes the secret again.
pub fn add(home: &Home, req: &AddRequest, redact: impl Fn(&str)) -> ToolResult<Entry> {
    validate_name(&req.name)?;
    let provider = ProviderName::parse(&req.provider).ok_or_else(|| ToolError::validation(format!("unknown provider `{}`", req.provider)))?;
    let store = Store::parse(&req.store)?;
    let text = req.connection_string.trim();
    if text.is_empty() {
        return Err(ToolError::validation("the connection string is empty"));
    }
    redact(text);
    let info = ConnInfo::parse(text)?;
    if let Some(p) = &info.password {
        redact(p);
    }
    if !crate::targets::to_runtime(provider).is_available() {
        return Err(ToolError::config(format!("the {} provider is not available in this build", provider.as_str())));
    }
    let entry = Entry { name: req.name.clone(), provider: provider.as_str().to_string(), store: store.as_str().to_string(), display: info.display() };
    let key = secret_key(&entry.name);

    let _guard = lock();
    let mut list = read_list(home)?;
    let existing = list.connections.iter().position(|e| e.name.eq_ignore_ascii_case(&req.name));
    if existing.is_some() && !req.overwrite {
        return Err(ToolError::validation(format!("a connection named `{}` already exists", req.name)));
    }
    write_secret(home, store, EXPLORER_ID, &key, text)?;
    let old = existing.map(|i| list.connections.remove(i));
    list.connections.push(entry.clone());
    list.connections.sort_by_key(|e| e.name.to_ascii_lowercase());
    if let Err(e) = write_list(home, &list) {
        if old.is_none() {
            if let Err(rb) = remove_secret(home, store, EXPLORER_ID, &key) {
                tracing::warn!(error = %rb, "could not remove the secret of a connection that failed to be listed");
            }
        }
        return Err(e);
    }
    // Replaced entry that lived in the other store: its stale secret goes.
    if let Some(old) = old {
        if let Ok(old_store) = Store::parse(&old.store) {
            if old_store != store {
                if let Err(e) = remove_secret(home, old_store, EXPLORER_ID, &secret_key(&old.name)) {
                    tracing::warn!(error = %e, "could not remove the previous secret of a replaced connection");
                }
            }
        }
    }
    Ok(entry)
}

/// Removes the entry and its secret.
pub fn remove(home: &Home, name: &str) -> ToolResult<()> {
    let _guard = lock();
    let mut list = read_list(home)?;
    let Some(pos) = list.connections.iter().position(|e| e.name.eq_ignore_ascii_case(name)) else {
        return Err(ToolError::validation(format!("no Data Explorer connection named `{name}`")));
    };
    let entry = list.connections.remove(pos);
    let store = Store::parse(&entry.store)?;
    remove_secret(home, store, EXPLORER_ID, &secret_key(&entry.name))?;
    write_list(home, &list)
}

/// `secrets.copyToProject`: copies the connection string of `explorer` into a project's store.
pub fn copy_to_project(home: &Home, explorer: &str, user_secrets_id: &str, key: &str, store: Store, redact: impl Fn(&str)) -> ToolResult<()> {
    if key.trim().is_empty() || key.len() > 200 || key.chars().any(char::is_control) {
        return Err(ToolError::validation("the secret key is empty, too long or contains control characters"));
    }
    let entry = find(home, explorer)?;
    let text = connection_string(home, &entry)?;
    redact(&text);
    write_secret(home, store, user_secrets_id, key, &text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_home(tag: &str) -> Home {
        let dir = std::env::temp_dir().join(format!("kubuno-data-tool-explorer-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        Home::at(dir)
    }

    fn request(name: &str, cs: &str) -> AddRequest {
        AddRequest { name: name.into(), provider: "postgres".into(), connection_string: cs.into(), store: "usersecrets".into(), overwrite: false }
    }

    #[test]
    fn names_are_validated() {
        for ok in ["Shop", "my db-1.x_y", "a"] {
            assert!(validate_name(ok).is_ok(), "{ok}");
        }
        for bad in ["", " x", "x ", "a/b", "a:b", &"x".repeat(65)] {
            assert!(validate_name(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn add_list_overwrite_remove_keep_the_secret_out_of_the_list() {
        let home = temp_home("cycle");
        let cs = "Host=db.example;Port=5432;Database=shop;Username=kubuno;Password=S3cr3t!";
        let entry = add(&home, &request("Shop", cs), |_| {}).expect("add");
        assert_eq!(entry.display, "db.example:5432/shop (user kubuno)");
        assert_eq!(list(&home).expect("list"), vec![entry.clone()]);
        let file = std::fs::read_to_string(home.connections_file()).expect("list file");
        assert!(!file.contains("S3cr3t"), "{file}");
        let secrets = std::fs::read_to_string(home.user_secrets(EXPLORER_ID).expect("id").path()).expect("secrets file");
        assert!(secrets.contains("S3cr3t!") && secrets.contains("ConnectionStrings:Shop"));
        assert_eq!(connection_string(&home, &entry).expect("secret"), cs);

        let dup = add(&home, &request("shop", cs), |_| {}).expect_err("duplicate");
        assert_eq!(dup.kind, "Validation");
        let mut again = request("Shop", "Host=other;Database=x;Username=u;Password=n3wSecret");
        again.overwrite = true;
        let replaced = add(&home, &again, |_| {}).expect("overwrite");
        assert_eq!(replaced.display, "other/x (user u)");
        assert_eq!(list(&home).expect("list").len(), 1);

        remove(&home, "Shop").expect("remove");
        assert!(list(&home).expect("list").is_empty());
        assert_eq!(read_secret(&home, Store::UserSecrets, EXPLORER_ID, "ConnectionStrings:Shop").expect("read"), None);
        assert_eq!(remove(&home, "Shop").expect_err("gone").kind, "Validation");
        let _ = std::fs::remove_dir_all(home.root());
    }

    #[test]
    fn bad_requests_are_refused_before_any_write() {
        let home = temp_home("bad");
        assert!(add(&home, &request("x/y", "Host=h"), |_| {}).is_err());
        assert!(add(&home, &request("Shop", "  "), |_| {}).is_err());
        let mut unknown = request("Shop", "Host=h");
        unknown.provider = "oracle".into();
        assert!(add(&home, &unknown, |_| {}).is_err());
        let mut store = request("Shop", "Host=h");
        store.store = "registry".into();
        assert!(add(&home, &store, |_| {}).is_err());
        assert!(!home.root().exists(), "nothing was written");
    }

    #[test]
    fn copy_to_project_uses_the_project_store() {
        let home = temp_home("copy");
        add(&home, &request("Shop", "Host=h;Database=d;Username=u;Password=pw123456"), |_| {}).expect("add");
        copy_to_project(&home, "Shop", "my-app-id", "ConnectionStrings:Shop", Store::UserSecrets, |_| {}).expect("copy");
        assert_eq!(read_secret(&home, Store::UserSecrets, "my-app-id", "ConnectionStrings:Shop").expect("read").as_deref(), Some("Host=h;Database=d;Username=u;Password=pw123456"));
        assert!(copy_to_project(&home, "Nope", "my-app-id", "K", Store::UserSecrets, |_| {}).is_err());
        assert!(copy_to_project(&home, "Shop", "../x", "K", Store::UserSecrets, |_| {}).is_err());
        let _ = std::fs::remove_dir_all(home.root());
    }

    /// Touches the real Windows Credential Manager (clearly named, removed at the end).
    /// Ignored by default: `cargo test -- --ignored`.
    #[cfg(all(windows, feature = "credential-manager"))]
    #[test]
    #[ignore]
    fn credential_manager_store_round_trip() {
        let home = temp_home("credman");
        let name = format!("ToolTest{}", std::process::id());
        let mut req = request(&name, "Host=h;Database=d;Username=u;Password=cm-pass-123");
        req.store = "credman".into();
        let entry = add(&home, &req, |_| {}).expect("add");
        let result = connection_string(&home, &entry);
        let removed = remove(&home, &name);
        assert_eq!(result.expect("read"), "Host=h;Database=d;Username=u;Password=cm-pass-123");
        removed.expect("remove");
        assert_eq!(read_secret(&home, Store::CredMan, EXPLORER_ID, &secret_key(&name)).expect("read"), None);
        let _ = std::fs::remove_dir_all(home.root());
    }
}
