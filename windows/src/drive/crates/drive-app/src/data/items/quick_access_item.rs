//! QuickAccessItem / QuickAccessKind (port-only — no `Data/Items/` counterpart)

use windows::Win32::UI::Shell::{
    FOLDERID_Desktop, FOLDERID_Documents, FOLDERID_Downloads, FOLDERID_Music,
    FOLDERID_Pictures, FOLDERID_Videos,
};

use super::known_folder_path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuickAccessKind {
    Downloads,
    Desktop,
    Documents,
    Pictures,
    Videos,
    Music,
    RecycleBin,
    Generic,
}

#[derive(Debug, Clone)]
pub struct QuickAccessItem {
    pub name: String,
    pub path: String,
    pub kind: QuickAccessKind,
    pub is_pinned: bool,
}

/// The `QuickAccessKind` of a pinned path, by comparison with the known
/// folders (only used for the fallback glyph).
fn kind_of_path(path: &str) -> QuickAccessKind {
    let known: [(&windows::core::GUID, QuickAccessKind); 6] = [
        (&FOLDERID_Downloads, QuickAccessKind::Downloads),
        (&FOLDERID_Desktop, QuickAccessKind::Desktop),
        (&FOLDERID_Documents, QuickAccessKind::Documents),
        (&FOLDERID_Pictures, QuickAccessKind::Pictures),
        (&FOLDERID_Videos, QuickAccessKind::Videos),
        (&FOLDERID_Music, QuickAccessKind::Music),
    ];
    for (id, kind) in known {
        if known_folder_path(id).is_some_and(|p| p.eq_ignore_ascii_case(path)) {
            return kind;
        }
    }
    QuickAccessKind::Generic
}

pub fn load_quick_access() -> Vec<QuickAccessItem> {
    // The REAL Quick Access pins (`GetPinnedFoldersAsync`), like the
    // QuickAccessManager; the known folders are only a fallback when the
    // shell's virtual folder is unavailable.
    let mut items: Vec<QuickAccessItem> =
        match crate::services::windows_quick_access_service::pinned_folders() {
            Some(pins) if !pins.is_empty() => pins
                .into_iter()
                .map(|pin| QuickAccessItem {
                    kind: kind_of_path(&pin.path),
                    name: pin.name,
                    path: pin.path,
                    is_pinned: true,
                })
                .collect(),
            _ => {
                let entries: [(&windows::core::GUID, &str, QuickAccessKind); 6] = [
                    (&FOLDERID_Downloads, "Téléchargements", QuickAccessKind::Downloads),
                    (&FOLDERID_Desktop, "Desktop", QuickAccessKind::Desktop),
                    (&FOLDERID_Documents, "Documents", QuickAccessKind::Documents),
                    (&FOLDERID_Pictures, "Images", QuickAccessKind::Pictures),
                    (&FOLDERID_Videos, "Vidéos", QuickAccessKind::Videos),
                    (&FOLDERID_Music, "Musique", QuickAccessKind::Music),
                ];
                entries
                    .iter()
                    .filter_map(|(id, name, kind)| {
                        known_folder_path(id).map(|path| QuickAccessItem {
                            name: name.to_string(),
                            path,
                            kind: *kind,
                            is_pinned: true,
                        })
                    })
                    .collect()
            }
        };

    items.push(QuickAccessItem {
        name: "Corbeille".to_string(),
        path: "shell:RecycleBinFolder".to_string(),
        kind: QuickAccessKind::RecycleBin,
        is_pinned: true,
    });
    items
}
