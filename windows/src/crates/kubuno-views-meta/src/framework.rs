//! The Kubuno framework's own crates, by name.
//!
//! The tools look for an application's controls in its folder dependencies (a library of controls such as
//! `kubuno-shell-controls = { path = "…" }`): the `#[kubuno::view]` macro, the language server's project scan and,
//! in Visual Studio, the design build's Toolbox tab and its "out of date" watch. The framework's crates are not such
//! a library — their elements are the built-in and library ones — so they are skipped, **by this explicit list and
//! never by a `kubuno` prefix**: third-party and application crates are named `kubuno-…` too.
//!
//! The same list exists in C# (vskubuno `src/Views/Kubuno.Views.Logic/FrameworkCrates.cs`); a vskubuno test reads
//! this file and fails when the two differ. Keep the `FRAMEWORK_CRATES` block one quoted name per line.
//!
//! What is listed: the framework layers of `desktop/windows/src/crates` (the facade, the toolkit, the views, data,
//! printing, resources and storage-component layers, and their tools). What is not: the application crates
//! (`kubuno-shell-controls`, `drive-*`, `chat`…), and desktop/common's service crates (`kubuno-sync*`,
//! `kubuno-account`, `kubuno-api-client`, `kubuno-secrets`, `kubuno-app-storage`, `kubuno-docs-core`): they declare
//! no control, so reading them finds nothing, and they are not a layer of the views framework.

/// The framework's crates (package names, `-` form). Compare with [`is_framework_crate`] (`-` ≡ `_`).
pub const FRAMEWORK_CRATES: &[&str] = &[
    "kubuno",
    "kubuno-ui",
    "kubuno-controls",
    "kubuno-views",
    "kubuno-views-macros",
    "kubuno-views-meta",
    "kubuno-views-model",
    "kubuno-views-syntax",
    "kubuno-views-ls",
    "kubuno-views-web",
    "kubuno-data",
    "kubuno-data-model",
    "kubuno-data-macros",
    "kubuno-data-tool",
    "kubuno-print",
    "kubuno-resources",
    "kubuno-resources-model",
    "kubuno-resources-macros",
    "kubuno-resources-tool",
    "kubuno-app-storage-components",
];

/// The framework crates that declare library components (`<PrintDocument>`, `<BindingSource>`, `<Settings>`…) and
/// the `kubuno` facade that reaches them through its workspace dependencies. The `#[kubuno::view]` macro knows these
/// elements by name (`LIBRARY_ELEMENTS`) and skips the crates; the language server has no such table and reads their
/// sources like a control library's.
pub const LIBRARY_COMPONENT_CRATES: &[&str] = &["kubuno", "kubuno-data", "kubuno-print", "kubuno-app-storage-components"];

/// A crate name in one form (`kubuno_views` and `kubuno-views` are one crate).
fn normalized(name: &str) -> String {
    name.trim().trim_matches('"').replace('_', "-")
}

/// Whether `name` (a package or extern crate name, `-` or `_`) is one of [`FRAMEWORK_CRATES`].
pub fn is_framework_crate(name: &str) -> bool {
    let name = normalized(name);
    FRAMEWORK_CRATES.contains(&name.as_str())
}

/// Whether `name` (`-` or `_`) is one of [`LIBRARY_COMPONENT_CRATES`].
pub fn is_library_component_crate(name: &str) -> bool {
    let name = normalized(name);
    LIBRARY_COMPONENT_CRATES.contains(&name.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_framework_is_an_explicit_list_not_a_prefix() {
        for name in ["kubuno", "kubuno-views", "kubuno_views", "kubuno_ui", "kubuno-views-ls", "kubuno_app_storage_components"] {
            assert!(is_framework_crate(name), "{name} is the framework");
        }
        for name in ["kubuno-shell-controls", "kubuno_shell_controls", "kubuno-acme-widgets", "kubuno-sync", "kubuno-app-storage", "kubunoish", "kubuno-shell-controls", "drive-app-controls"] {
            assert!(!is_framework_crate(name), "{name} is not the framework");
        }
    }

    #[test]
    fn the_library_component_crates_are_framework_crates() {
        for name in LIBRARY_COMPONENT_CRATES {
            assert!(is_framework_crate(name), "{name}");
        }
        assert!(is_library_component_crate("kubuno_print") && !is_library_component_crate("kubuno-views"));
    }

    #[test]
    fn the_list_matches_the_framework_crates_of_the_workspace() {
        // Every listed crate is a folder of `desktop/windows/src/crates`, and every `kubuno*` folder there is listed:
        // a new framework crate must be added (to this list and the C# one), a new application library renamed out.
        let crates = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let Ok(entries) = std::fs::read_dir(&crates) else { return };
        let folders: Vec<String> = entries.flatten().filter(|e| e.path().join("Cargo.toml").is_file()).map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        for name in FRAMEWORK_CRATES {
            assert!(folders.iter().any(|f| f == name), "{name} is not a crate of {}", crates.display());
        }
        for folder in folders.iter().filter(|f| f.as_str() == "kubuno" || f.starts_with("kubuno-")) {
            // The one application library that lives in this folder (WaffleMenu, AccountMenu…), whatever its name.
            if folder.contains("kubuno-shell-controls") {
                continue;
            }
            assert!(is_framework_crate(folder), "`{folder}` is neither listed in FRAMEWORK_CRATES nor an application library");
        }
    }
}
