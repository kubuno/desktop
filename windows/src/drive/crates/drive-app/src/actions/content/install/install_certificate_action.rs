//! InstallCertificate (mirrors InstallCertificateAction.cs)

// NOT WIRED YET. See the module docs of actions::content::install: not registered yet.
// The allow is scoped to this file and must go once it is wired.
#![allow(dead_code)]

use crate::actions::Action;
use crate::main_window::MainWindow;
use crate::view_models::shell_view_model::shell_verb;
use drive_shared::helpers::file_extensions as fe;

use super::{not_recycle_bin, selected};

/// `InstallCertificateAction.cs`.
///
/// - `Label`: `Strings.InstallCertificate` → "Install certificate".
/// - `Description`: `Strings.InstallCertificateDescription`.
/// - `Glyph`: `new("")` (Segoe MDL2 glyph, not a ThemedIcon — hence no
///   `glyph()` here, the port renders via ThemedIcon).
/// - `IsExecutable`: `SelectedItems.Any() && All(IsCertificateFile) &&
///   !RecycleBin && !ZipFolder`.
/// - `ExecuteAsync`: `ContextMenu.InvokeVerb("add", paths)` — the verb is
///   indeed **"add"** (not "install").
pub struct InstallCertificate;
impl Action for InstallCertificate {
    fn label(&self) -> &'static str {
        "InstallCertificate"
    }
    fn description(&self) -> &'static str {
        "InstallCertificateDescription"
    }
    fn is_executable(&self, w: &MainWindow) -> bool {
        let sel = w.state.active().selected_paths();
        !sel.is_empty()
            && sel.iter().all(|p| fe::is_certificate_file(Some(p)))
            && not_recycle_bin(w)
    }
    fn execute(&self, w: &mut MainWindow, parameter: Option<&str>) {
        // `await ContextMenu.InvokeVerb("add", ...)`: the "add" shell verb.
        for path in selected(w, parameter) {
            shell_verb(&path, "add");
        }
    }
}
