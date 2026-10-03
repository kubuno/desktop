//! Port of `Files.App/Views/Properties/` : the properties sheet.
//!
//! In the original this is a separate non-modal WINDOW
//! (`FilePropertiesHelpers.OpenPropertiesWindow`: an 800x500-DPI `WindowEx`,
//! acrylic/Mica `SystemBackdrop`, non-maximizable, positioned at the cursor),
//! hosting `MainPropertiesPage` (tab sidebar + `Frame`). The port reproduces
//! this 2nd top-level window ([`window::FilesPropertiesWindow`]) on the shared
//! UI thread — the same `GetMessage` loop pumps it, like the
//! `flyout_window`/`tab_ghost` popups.
//!
//! This slice delivers the shell + the **General** tab (`general`). The other
//! tabs are listed (per `PropertiesNavigationItemsFactory`) but
//! greyed-out/non-clickable.
//!
//! The [`open_properties_window`] entry point is NOT yet called (see the
//! report for the planned wiring in `actions::open::OpenProperties` and on
//! `Hot::CmdProperties`) : hence the `dead_code` tolerated below until the
//! window is wired up.
#![allow(dead_code)]

pub mod compatibility;
pub mod customization;
pub mod details;
pub mod general;
pub mod hashes;
pub mod security;
pub mod shortcut;
pub mod window;

use windows::Win32::Foundation::HWND;

/// The target of a properties sheet (the `item` parameter of
/// `OpenPropertiesWindow`). `Path` covers both file AND folder (distinguished
/// on the fly), `Drive` a drive, `Multi` a multi-selection.
#[derive(Debug, Clone)]
pub enum PropertiesTarget {
    Path(String),
    Drive(char),
    Multi(Vec<String>),
}

/// The tabs of the sheet (`PropertiesNavigationViewItemType`).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PropTab {
    General,
    Signatures,
    Security,
    Hashes,
    Shortcut,
    Library,
    Details,
    Customization,
    Compatibility,
}

impl PropTab {
    /// The resource key of the label (`Strings.*.GetLocalizedResource`).
    pub fn label_key(self) -> &'static str {
        match self {
            PropTab::General => "General",
            PropTab::Signatures => "Signatures",
            PropTab::Security => "Security",
            PropTab::Hashes => "Hashes",
            PropTab::Shortcut => "Shortcut",
            PropTab::Library => "Library",
            PropTab::Details => "Details",
            PropTab::Customization => "Customization",
            PropTab::Compatibility => "Compatibility",
        }
    }

    /// Is the tab IMPLEMENTED (clickable) in the port. The others stay
    /// greyed-out (`IsEnabled=false`). Ported: General, Details, Hashes,
    /// Security, Shortcut, Compatibility and Customization ; Library and
    /// Signatures remain out of scope.
    pub fn is_implemented(self) -> bool {
        matches!(
            self,
            PropTab::General
                | PropTab::Details
                | PropTab::Hashes
                | PropTab::Security
                | PropTab::Shortcut
                | PropTab::Compatibility
                | PropTab::Customization
        )
    }

    /// The Segoe Fluent Icons glyph of the tab (approximation of the
    /// factory's `App.ThemedIcons.Properties.*` — not ported as geometry).
    /// Name of the vector ThemedIcon (`App.ThemedIcons.Properties.*`) ported in
    /// `themed-icons.txt` — the tab's real artwork, not an approximate Segoe
    /// glyph.
    pub fn vector_name(self) -> &'static str {
        match self {
            PropTab::General => "Properties.General",
            PropTab::Signatures => "Properties.Signatures",
            PropTab::Security => "Properties.Security",
            PropTab::Hashes => "Properties.Hashes",
            PropTab::Shortcut => "Properties.Shortcut",
            PropTab::Library => "Properties.Library",
            PropTab::Details => "Properties.Info",
            PropTab::Customization => "Properties.CustomizeFolder",
            PropTab::Compatibility => "Properties.Compatability",
        }
    }
}

/// Mirror of `PropertiesNavigationItemsFactory.Initialize` : the full order
/// of the tabs, then the removals depending on the target. Non-implemented
/// tabs (Library, Signatures) are removed as soon as they don't apply ; the
/// implemented ones appear under the same conditions as the factory.
fn navigation_items(target: &PropertiesTarget) -> Vec<PropTab> {
    use PropTab::*;
    // Addition order from the factory.
    let mut items = vec![
        General, Signatures, Security, Hashes, Shortcut, Library, Details,
        Customization, Compatibility,
    ];
    let remove = |items: &mut Vec<PropTab>, t: PropTab| items.retain(|x| *x != t);

    match target {
        PropertiesTarget::Multi(_) => {
            // Multi-selection: only General is ported (partial
            // `CombinedProperties`) → remove everything else.
            for t in [Signatures, Security, Hashes, Shortcut, Library, Details, Customization, Compatibility] {
                remove(&mut items, t);
            }
        }
        PropertiesTarget::Drive(_) => {
            for t in [Signatures, Hashes, Shortcut, Library, Details, Customization, Compatibility] {
                remove(&mut items, t);
            }
        }
        PropertiesTarget::Path(p) => {
            let path = std::path::Path::new(p);
            let is_folder = path.is_dir();
            let ext = path
                .extension()
                .map(|e| e.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            // `IsShortcutFile` covers `.lnk` ; `.url` is an `IsLinkItem`.
            // Both are `IShortcutItem` → Shortcut tab applicable.
            let is_shortcut = matches!(ext.as_str(), "lnk" | "url");

            // `compatibilityItemEnabled = IsExecutableFile(shortcut ? target :
            // ext, exeOnly:true)` → only the `.exe` extension (or an `.exe` target).
            let compat_source = if is_shortcut {
                shortcut::shortcut_target(p).unwrap_or_default()
            } else {
                p.clone()
            };
            let compat_enabled = std::path::Path::new(&compat_source)
                .extension()
                .map(|e| e.eq_ignore_ascii_case("exe"))
                .unwrap_or(false);

            // `customizationItemEnabled = (isFolder && !isArchive) || isShortcut`
            // (archives are treated as files here → a folder as seen by the
            // port is non-archive).
            let customization_enabled = is_folder || is_shortcut;

            // Signatures/Library not ported: always removed.
            remove(&mut items, Signatures);
            remove(&mut items, Library);
            // Hashes/Details: removed for a folder (non-archive).
            if is_folder {
                remove(&mut items, Hashes);
                remove(&mut items, Details);
            }
            if !is_shortcut {
                remove(&mut items, Shortcut);
            }
            if !customization_enabled {
                remove(&mut items, Customization);
            }
            if !compat_enabled {
                remove(&mut items, Compatibility);
            }
        }
    }
    items
}

/// Opens the properties sheet for `item`, anchored to `parent` (the main
/// window). Public entry point — the equivalent of
/// `FilePropertiesHelpers.OpenPropertiesWindow(item, associatedInstance)`.
///
/// TO WIRE UP from:
/// * `actions::open::OpenProperties::execute` (context menu "Properties") ;
/// * `MainWindow` on `Hot::CmdProperties` / `MenuCommand::OpenProperties`
///   (command bar) — see the report for `parent`'s signature.
pub fn open_properties_window(parent: HWND, item: PropertiesTarget) {
    if let Err(e) = window::FilesPropertiesWindow::open(parent, item) {
        tracing::error!("échec ouverture fenêtre Propriétés: {e}");
    }
}
