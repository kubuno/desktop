//! The one error type of the crate. Messages name keys, files and Registry paths, **never values** (a setting may
//! hold a user's data; a secret never reaches an error at all).

/// What went wrong storing or reading app data.
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    /// An app id, a setting name, a set name or a Registry path is not acceptable.
    #[error("invalid name: {0}")]
    InvalidName(String),
    /// A setting the schema does not declare (strict access), or a value of the wrong type.
    #[error("setting '{name}': {message}")]
    Setting { name: String, message: String },
    /// The value is larger than the limits of `docs/STORAGE-COMPONENTS.md` §6 (or of the back-end).
    #[error("'{name}' is too large ({len} bytes, at most {max})")]
    TooLarge { name: String, len: usize, max: usize },
    /// Writing a read-only layer (application-scoped settings, the machine layer, a key opened read-only).
    #[error("{0} is read-only")]
    ReadOnly(String),
    /// The OS refused the access (a Registry key of `HKLM` without elevation, a file of another user).
    #[error("access denied: {0}")]
    AccessDenied(String),
    /// The stored data cannot be read (not JSON, an unexpected Registry type). The location, never the content.
    #[error("{0} is corrupted")]
    Corrupted(String),
    /// The medium does not exist on this platform (the Registry outside Windows).
    #[error("{0} is not available on this platform")]
    Unsupported(&'static str),
    /// A secret store failure (it already names the secret, never its value).
    #[error(transparent)]
    Secret(#[from] kubuno_desktop_secrets::SecretError),
    /// Any other failure of a back-end: its name and a message.
    #[error("{backend}: {message}")]
    Backend { backend: &'static str, message: String },
}

impl StorageError {
    /// An I/O error on `path`, mapped to [`StorageError::AccessDenied`] when it is one.
    pub(crate) fn io(path: &std::path::Path, e: std::io::Error) -> Self {
        if e.kind() == std::io::ErrorKind::PermissionDenied {
            StorageError::AccessDenied(path.display().to_string())
        } else {
            StorageError::Backend { backend: "file", message: format!("{}: {e}", path.display()) }
        }
    }
}
