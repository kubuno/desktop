//! Errors of the engine. Messages carry codes and ids, never a payload, a token or a key.

use kubuno_desktop_api_client::ApiError;

#[derive(Debug, Clone, thiserror::Error)]
pub enum SyncError {
    #[error("local database: {0}")]
    Db(String),
    #[error("the database key is wrong (or the file is not an encrypted Kubuno database)")]
    WrongKey,
    #[error("this build has no SQLCipher: an encrypted database cannot be opened (build with the `sqlcipher` feature)")]
    EncryptionUnavailable,
    #[error("this database belongs to another account")]
    AccountMismatch,
    #[error("migration failed: {0}")]
    Migration(String),
    #[error("the local database is read-only after a failed migration: {0}")]
    ReadOnly(String),
    #[error("configuration: {0}")]
    Config(String),
    #[error("i/o: {0}")]
    Io(String),
    #[error(transparent)]
    Api(#[from] ApiError),
    #[error("no adapter for entity '{0}'")]
    UnknownEntity(String),
    #[error("unknown feed '{0}'")]
    UnknownFeed(String),
    #[error("the session has expired: sign in again")]
    SessionExpired,
    #[error("offline")]
    Offline,
}

impl SyncError {
    /// A mapper for `sqlx` errors that logs them (rule: every DB error is logged before being returned).
    pub fn db(context: &'static str) -> impl Fn(sqlx::Error) -> SyncError {
        move |e| {
            tracing::error!(context, error = %e, "local database error");
            SyncError::Db(format!("{context}: {e}"))
        }
    }
}
