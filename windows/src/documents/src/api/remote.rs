//! The session's outside world for real: [`Remote`] answers [`session::Server`] through the REST
//! [`Client`], and [`FileJournal`] is the local crash journal ([`session::Journal`]).
//!
//! The journal is the offline-first half: the open document's bytes are written to the user's data
//! folder (`<data>/documents/journal/<id>.kbdoc`, the sandbox's when `KUBUNO_SANDBOX_DIR` is set) at
//! each journal pass of the session, before any server write, and cleared once the server holds them.

use std::path::PathBuf;

use serde_json::{Map, Value};

use super::client::{self, Client};
use super::session::{self, SaveJob, Snapshot};

/// Maps a REST failure onto the four shapes the session tells apart.
fn map(e: client::Error) -> session::Error {
    match e {
        client::Error::Precondition => session::Error::Precondition,
        client::Error::TooLarge => session::Error::TooLarge,
        client::Error::Forbidden | client::Error::NotFound => session::Error::NotOwner,
        other => session::Error::Transport(other.to_string()),
    }
}

/// The office routes, over one account's client.
pub struct Remote {
    pub client: Client,
}

impl Remote {
    pub fn new(client: Client) -> Self {
        Self { client }
    }
}

impl session::Server for Remote {
    fn fetch(&mut self, id: &str) -> Result<Snapshot, session::Error> {
        let f = self.client.fetch_doc(id).map_err(map)?;
        Ok(Snapshot { content: f.content, etag: f.etag, editors: Vec::new() })
    }

    fn patch(&mut self, job: &SaveJob) -> Result<Option<String>, session::Error> {
        let f = self.client.save_content(&job.id, &job.content, job.if_match.as_deref()).map_err(map)?;
        Ok(f.etag)
    }

    fn join(&mut self, id: &str) -> Result<Snapshot, session::Error> {
        let j = self.client.join_editing(id).map_err(map)?;
        let editors = j.editors.iter().map(|e| e.display_name.clone().unwrap_or_else(|| e.user_id.clone())).collect();
        Ok(Snapshot { content: j.document.content, etag: j.document.etag, editors })
    }

    fn ping(&mut self, id: &str) -> Result<(), session::Error> {
        self.client.ping_editing(id).map_err(map)
    }

    fn leave(&mut self, id: &str) -> Result<(), session::Error> {
        self.client.leave_editing(id).map_err(map)
    }

    fn config(&mut self) -> Result<Map<String, Value>, session::Error> {
        self.client.core_config().map_err(map)
    }
}

/// The crash journal: one file per document in the user's data folder.
pub struct FileJournal {
    dir: PathBuf,
}

impl FileJournal {
    /// The journal of this user (or of the sandbox), `None` when no data folder can be resolved.
    pub fn for_user() -> Option<Self> {
        let dir = kubuno_desktop_account::paths::user_data_dir().ok()?.join("documents").join("journal");
        Some(Self { dir })
    }

    pub fn at(dir: PathBuf) -> Self {
        Self { dir }
    }

    /// A file name that is valid on every OS whatever the id holds (ids are UUIDs, but nothing
    /// here relies on it).
    fn path(&self, id: &str) -> PathBuf {
        let safe: String = id.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
        self.dir.join(format!("{safe}.kbdoc"))
    }
}

impl session::Journal for FileJournal {
    fn write(&mut self, id: &str, bytes: &[u8]) -> Result<(), session::Error> {
        let path = self.path(id);
        std::fs::create_dir_all(&self.dir).map_err(|e| session::Error::Transport(format!("journal : {e}")))?;
        // Written beside, then renamed: a crash mid-write never leaves a torn journal.
        let tmp = path.with_extension("kbdoc.tmp");
        std::fs::write(&tmp, bytes).map_err(|e| session::Error::Transport(format!("journal : {e}")))?;
        std::fs::rename(&tmp, &path).map_err(|e| session::Error::Transport(format!("journal : {e}")))
    }

    fn read(&mut self, id: &str) -> Option<Vec<u8>> {
        std::fs::read(self.path(id)).ok()
    }

    fn clear(&mut self, id: &str) {
        let _ = std::fs::remove_file(self.path(id));
    }
}

/// The session host of a real document: the REST routes plus the journal.
pub struct Link {
    pub remote: Remote,
    pub journal: FileJournal,
}

impl session::Server for Link {
    fn fetch(&mut self, id: &str) -> Result<Snapshot, session::Error> {
        self.remote.fetch(id)
    }
    fn patch(&mut self, job: &SaveJob) -> Result<Option<String>, session::Error> {
        self.remote.patch(job)
    }
    fn join(&mut self, id: &str) -> Result<Snapshot, session::Error> {
        self.remote.join(id)
    }
    fn ping(&mut self, id: &str) -> Result<(), session::Error> {
        self.remote.ping(id)
    }
    fn leave(&mut self, id: &str) -> Result<(), session::Error> {
        self.remote.leave(id)
    }
    fn config(&mut self) -> Result<Map<String, Value>, session::Error> {
        self.remote.config()
    }
}

impl session::Journal for Link {
    fn write(&mut self, id: &str, bytes: &[u8]) -> Result<(), session::Error> {
        self.journal.write(id, bytes)
    }
    fn read(&mut self, id: &str) -> Option<Vec<u8>> {
        self.journal.read(id)
    }
    fn clear(&mut self, id: &str) {
        self.journal.clear(id)
    }
}

/// Connects to document `id` as the account the shell shows (its access token is borrowed from the
/// shell's broker: this app never sees a password or a refresh token). Returns the host and the
/// title; offline, the title is the id and the session opens from the journal.
pub fn connect(id: &str) -> Result<(Link, String), String> {
    use session::Journal as _;
    let journal = FileJournal::for_user().ok_or_else(|| "dossier de données introuvable".to_string())?;
    let account = kubuno_desktop_sync::current_account()
        .map_err(|e| format!("compte : {e}"))?
        .ok_or_else(|| "aucun compte connecté : connectez-vous dans Kubuno Desktop".to_string())?;
    let client = Client::new(account.key.clone());
    let mut link = Link { remote: Remote::new(client), journal };
    let title = match link.remote.client.fetch_doc(id) {
        Ok(f) => f.title,
        Err(client::Error::Transport(_)) if link.journal.read(id).is_some() => id.to_string(),
        Err(e) => return Err(e.to_string()),
    };
    Ok((link, title))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::session::Journal;

    #[test]
    fn the_journal_round_trips_and_clears() {
        let dir = std::env::temp_dir().join(format!("kubuno-docs-journal-{}", std::process::id()));
        let mut j = FileJournal::at(dir.clone());
        assert!(j.read("a/b:c").is_none());
        j.write("a/b:c", b"{\"type\":\"doc\"}").expect("written");
        assert_eq!(j.read("a/b:c").as_deref(), Some(&b"{\"type\":\"doc\"}"[..]));
        assert!(j.path("a/b:c").file_name().is_some_and(|n| n == "a_b_c.kbdoc"));
        j.clear("a/b:c");
        assert!(j.read("a/b:c").is_none());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn rest_failures_map_to_the_session_shapes() {
        assert_eq!(map(client::Error::TooLarge), session::Error::TooLarge);
        assert_eq!(map(client::Error::Precondition), session::Error::Precondition);
        assert_eq!(map(client::Error::Forbidden), session::Error::NotOwner);
        assert!(matches!(map(client::Error::Transport("x".into())), session::Error::Transport(_)));
    }
}
