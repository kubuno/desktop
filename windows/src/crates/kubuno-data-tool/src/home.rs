//! Where the tool keeps its files: `%APPDATA%\Kubuno`, or the folder named by the environment
//! variable `KUBUNO_DATA_TOOL_HOME` (tests, sandboxes). The Data Explorer list and the user secrets
//! live under it; the Windows Credential Manager is machine-wide and is never redirected.

use std::path::{Path, PathBuf};

use kubuno_data::secrets::{is_valid_user_secrets_id, UserSecrets};

use crate::error::{ToolError, ToolResult};

/// The environment variable overriding the root folder.
pub const HOME_VARIABLE: &str = "KUBUNO_DATA_TOOL_HOME";

#[derive(Debug, Clone)]
pub struct Home {
    root: PathBuf,
}

impl Home {
    /// The override, else `%APPDATA%\Kubuno`.
    pub fn from_env() -> ToolResult<Self> {
        if let Some(root) = std::env::var_os(HOME_VARIABLE).filter(|v| !v.is_empty()) {
            return Ok(Self { root: PathBuf::from(root) });
        }
        let appdata = std::env::var_os("APPDATA").ok_or_else(|| ToolError::config("APPDATA is not set: no Kubuno folder for the Data Explorer"))?;
        Ok(Self { root: PathBuf::from(appdata).join("Kubuno") })
    }

    pub fn at(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// `<root>\DataExplorer\connections.json`.
    pub fn connections_file(&self) -> PathBuf {
        self.root.join("DataExplorer").join("connections.json")
    }

    /// The user secrets store of `id`: `<root>\UserSecrets\<id>\secrets.json`.
    pub fn user_secrets(&self, id: &str) -> ToolResult<UserSecrets> {
        if !is_valid_user_secrets_id(id) {
            return Err(ToolError::validation("the user secrets id may only contain letters, digits, '-', '_' and '.'"));
        }
        Ok(UserSecrets::at(self.root.join("UserSecrets").join(id).join("secrets.json")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_are_under_the_root() {
        let h = Home::at("C:\\x");
        assert!(h.connections_file().ends_with("DataExplorer\\connections.json") || h.connections_file().ends_with("DataExplorer/connections.json"));
        let s = h.user_secrets("DataExplorer").expect("id");
        assert!(s.path().starts_with("C:\\x") && s.path().ends_with("secrets.json"));
        assert!(h.user_secrets("../evil").is_err());
    }
}
