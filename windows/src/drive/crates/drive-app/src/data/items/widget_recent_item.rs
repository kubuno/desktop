//! RecentFileItem (mirror of WidgetRecentItem.cs)

use super::known_folder_path;

#[derive(Debug, Clone)]
pub struct RecentFileItem {
    pub name: String,
    pub path: String,
}

pub fn load_recent_files(max: usize) -> Vec<RecentFileItem> {
    // Reads shortcut names from the Recent folder. Resolving .lnk targets
    // requires IShellLinkW and is ported with drive-app-storage.
    let Some(recent) = known_folder_path(&windows::Win32::UI::Shell::FOLDERID_Recent) else {
        return Vec::new();
    };

    let Ok(entries) = std::fs::read_dir(&recent) else {
        return Vec::new();
    };

    let mut files: Vec<(std::time::SystemTime, RecentFileItem)> = entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            if !path.extension().is_some_and(|e| e.eq_ignore_ascii_case("lnk")) {
                return None;
            }
            let name = path.file_stem()?.to_string_lossy().into_owned();
            let modified = entry.metadata().ok()?.modified().ok()?;
            Some((
                modified,
                RecentFileItem {
                    name,
                    path: path.to_string_lossy().into_owned(),
                },
            ))
        })
        .collect();

    files.sort_by_key(|f| std::cmp::Reverse(f.0));
    files.into_iter().take(max).map(|(_, item)| item).collect()
}
