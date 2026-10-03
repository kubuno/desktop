//! DevToolsSettingsService (mirror of `DevToolsSettingsService.cs`).
//!
//! Helper resolving the effective IDE; the state itself remains carried by
//! the single `AppSettings` store (see `mod.rs`).

use super::AppSettings;

/// Effective IDE (path, name), port of `DevToolsSettingsService.IDEPath/IDEName`
/// whose defaults are `code` / "Visual Studio Code" when VS Code is installed
/// (SoftwareHelpers.IsVSCodeInstalled — approximated here by a PATH lookup).
pub fn ide_display(s: &AppSettings) -> (String, String) {
    if !s.ide_path.is_empty() || !s.ide_name.is_empty() {
        return (s.ide_path.clone(), s.ide_name.clone());
    }
    let vscode = std::env::var_os("PATH").is_some_and(|path| {
        std::env::split_paths(&path)
            .any(|dir| dir.join("code.cmd").exists() || dir.join("code.exe").exists())
    });
    if vscode {
        ("code".into(), drive_localization::tr("VisualStudioCode").into())
    } else {
        (String::new(), String::new())
    }
}
