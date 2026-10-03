//! The tool's error type: a protocol `kind` (the `DataError` variant name, `Protocol`, `Io`) and a
//! message that never contains a connection string or a password.

use kubuno_data::DataError;

/// What a failed request answers: `{"kind": "...", "message": "..."}`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolError {
    pub kind: &'static str,
    pub message: String,
}

pub type ToolResult<T> = Result<T, ToolError>;

impl ToolError {
    pub fn new(kind: &'static str, message: impl Into<String>) -> Self {
        Self { kind, message: message.into() }
    }

    pub fn config(message: impl Into<String>) -> Self {
        Self::new("Config", message)
    }

    pub fn secret(message: impl Into<String>) -> Self {
        Self::new("Secret", message)
    }

    pub fn validation(message: impl Into<String>) -> Self {
        Self::new("Validation", message)
    }

    pub fn database(message: impl Into<String>) -> Self {
        Self::new("Database", message)
    }

    pub fn protocol(message: impl Into<String>) -> Self {
        Self::new("Protocol", message)
    }

    pub fn io(what: &str, e: &std::io::Error) -> Self {
        tracing::error!(what, error = %e, "file operation failed");
        Self::new("Io", format!("{what}: {e}"))
    }

    pub fn cancelled() -> Self {
        Self::new("Cancelled", "the operation was cancelled")
    }
}

impl std::fmt::Display for ToolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.kind, self.message)
    }
}

impl std::error::Error for ToolError {}

impl From<DataError> for ToolError {
    fn from(e: DataError) -> Self {
        match e {
            DataError::Config(m) => Self::new("Config", m),
            DataError::Secret(m) => Self::new("Secret", m),
            DataError::Validation(m) => Self::new("Validation", m),
            DataError::Database { message, code: Some(code) } => Self::new("Database", format!("{message} ({code})")),
            DataError::Database { message, code: None } => Self::new("Database", message),
            DataError::Concurrency(m) => Self::new("Concurrency", m),
            DataError::Cancelled => Self::cancelled(),
            DataError::Closed => Self::new("Closed", "the connection was closed"),
        }
    }
}

/// Replaces the secrets a request touched with `***` in every message it may return: the driver's
/// own errors are trusted not to quote them, this is the second line of defence.
#[derive(Debug, Default)]
pub struct Redactor {
    needles: std::sync::Mutex<Vec<String>>,
}

impl Redactor {
    /// Registers a secret. Values shorter than 4 characters would garble unrelated text and are
    /// ignored (a whole connection string is always longer).
    pub fn add(&self, secret: &str) {
        if secret.chars().count() < 4 {
            return;
        }
        let mut needles = self.needles.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if !needles.iter().any(|n| n == secret) {
            needles.push(secret.to_string());
            // Longest first, so a connection string goes before the password it contains.
            needles.sort_by_key(|n| std::cmp::Reverse(n.len()));
        }
    }

    pub fn scrub(&self, text: &str) -> String {
        let needles = self.needles.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut out = text.to_string();
        for n in needles.iter() {
            if out.contains(n.as_str()) {
                out = out.replace(n.as_str(), "***");
            }
        }
        out
    }

    pub fn error(&self, e: ToolError) -> ToolError {
        ToolError { kind: e.kind, message: self.scrub(&e.message) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_follow_the_data_error_variants() {
        assert_eq!(ToolError::from(DataError::Config("x".into())).kind, "Config");
        assert_eq!(ToolError::from(DataError::Cancelled).kind, "Cancelled");
        let db = ToolError::from(DataError::Database { message: "boom".into(), code: Some("42".into()) });
        assert_eq!((db.kind, db.message.as_str()), ("Database", "boom (42)"));
    }

    #[test]
    fn redaction_removes_the_longest_secret_first() {
        let r = Redactor::default();
        r.add("Password=S3cr3t!");
        r.add("S3cr3t!");
        r.add("ab");
        assert_eq!(r.scrub("bad Password=S3cr3t! then S3cr3t! ab"), "bad *** then *** ab");
        assert_eq!(r.error(ToolError::database("x S3cr3t!")).message, "x ***");
    }
}
