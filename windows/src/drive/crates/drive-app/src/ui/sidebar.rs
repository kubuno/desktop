#![allow(unused_imports)]
use windows::core::Result;
use windows::Win32::Graphics::Direct2D::Common::{D2D1_COLOR_F, D2D_RECT_F};
use windows::Win32::Graphics::Direct2D::{
    ID2D1Bitmap1, ID2D1DeviceContext, ID2D1SolidColorBrush, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
    D2D1_INTERPOLATION_MODE_LINEAR, D2D1_ROUNDED_RECT,
};
use windows::Win32::Graphics::DirectWrite::{
    IDWriteTextFormat, DWRITE_MEASURING_MODE_NATURAL, DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
    DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_LEADING,
};

use crate::graphics::Renderer;
use crate::services::storage::IconCache;
use crate::data::items::{HomeModel, QuickAccessKind};
use crate::view_models::shell_view_model::{Location, Tab, TabGroup};
use crate::styles::theme::Theme;
use super::*;

#[derive(Clone)]
pub enum SidebarEntry {
    Home,
    SectionPinned,
    /// Libraries (`SectionType::Library`).
    SectionLibraries,
    /// Index into `model.libraries`.
    Library(usize),
    SectionDrives,
    /// Cloud drives (`SectionType::CloudDrives`).
    SectionCloudDrives,
    /// Index into `model.cloud_drives`.
    CloudDrive(usize),
    /// Network (`SectionType::Network`): the `DRIVE_REMOTE` drives.
    SectionNetwork,
    /// Tags (`SectionType::FileTag`).
    SectionTags,
    Pinned(usize),
    Drive(usize),
    /// Index into `model.drives` (a network drive).
    NetworkDrive(usize),
    /// Index into `settings.file_tags`.
    Tag(usize),
    /// A folder in an expanded drive's tree (hierarchical `SidebarItem`):
    /// full path + indentation depth.
    Folder(String, u8),
    Settings,
}

/// The rendered form of a sidebar row: fold glyph, label, icon indent,
/// section header, and selection. Shared by the layout calculation (natural
/// width, for horizontal scrolling) and the drawing.
pub(crate) struct SidebarVisual {
    pub glyph: &'static str,
    pub label: String,
    pub indent: f32,
    pub is_section: bool,
    pub selected: bool,
}

/// The path a sidebar entry represents (None: Home, sections…).
fn sidebar_entry_path(entry: &SidebarEntry, model: &HomeModel) -> Option<String> {
    match entry {
        SidebarEntry::Pinned(idx) => model.quick_access.get(*idx).map(|p| p.path.clone()),
        SidebarEntry::Drive(idx) | SidebarEntry::NetworkDrive(idx) => {
            model.drives.get(*idx).map(|d| format!("{}:\\", d.letter))
        }
        SidebarEntry::CloudDrive(idx) => {
            model.cloud_drives.get(*idx).map(|c| c.sync_folder.clone())
        }
        SidebarEntry::Folder(path, _) => Some(path.clone()),
        // A library highlights when we're in its default save folder
        // (the v1 navigation target).
        SidebarEntry::Library(idx) => model
            .libraries
            .get(*idx)
            .and_then(|l| l.default_save_folder.clone().or_else(|| l.folders.first().cloned())),
        _ => None,
    }
}

/// The sidebar's selected entry during navigation: the one whose path is
/// the LONGEST prefix of the current location (the original
/// `UpdateSidebarSelectedItemFromArgs` of SidebarViewModel) — a folder
/// under C:\ highlights « Disque système 1 (C:) » for lack of a closer pin;
/// a deeper ancestor pin wins over the drive.
pub(crate) fn sidebar_selected_entry(
    entries: &[SidebarEntry],
    model: &HomeModel,
    active_location: &Location,
) -> Option<usize> {
    let Location::Dir(current) = active_location else { return None };
    let current = current.to_string_lossy().to_lowercase();
    let mut best: Option<(usize, usize)> = None; // (index, prefix length)
    for (i, entry) in entries.iter().enumerate() {
        let Some(path) = sidebar_entry_path(entry, model) else { continue };
        let path = path.to_lowercase();
        let trimmed = path.trim_end_matches('\\');
        let is_prefix = current == trimmed
            || (current.starts_with(trimmed)
                && current.as_bytes().get(trimmed.len()) == Some(&b'\\'));
        if is_prefix && best.is_none_or(|(_, len)| trimmed.len() > len) {
            best = Some((i, trimmed.len()));
        }
    }
    best.map(|(i, _)| i)
}

/// The Material Symbols Outlined vector icon of a navigation row, when one
/// exists. The pane used to draw Segoe Fluent glyphs here; every row that has
/// a Material counterpart now uses the same outlined family as the toolbar, so
/// the whole window reads as ONE icon set. `None` falls back to the row's
/// glyph (`SidebarVisual::glyph`), so an entry without a match still draws.
pub(crate) fn sidebar_vector_icon(entry: &SidebarEntry, model: &HomeModel) -> Option<&'static str> {
    Some(match entry {
        SidebarEntry::Home => "Home",
        SidebarEntry::SectionPinned => "Star",
        SidebarEntry::SectionLibraries | SidebarEntry::Library(_) => "Library",
        SidebarEntry::SectionDrives | SidebarEntry::Drive(_) => "HardDrive",
        SidebarEntry::SectionCloudDrives | SidebarEntry::CloudDrive(_) => "Cloud",
        SidebarEntry::SectionNetwork | SidebarEntry::NetworkDrive(_) => "Network",
        SidebarEntry::SectionTags => "Label",
        SidebarEntry::Settings => "Settings",
        SidebarEntry::Folder(..) => "Folder",
        SidebarEntry::Pinned(idx) => match model.quick_access.get(*idx).map(|p| p.kind) {
            Some(QuickAccessKind::Downloads) => "Download",
            Some(QuickAccessKind::Desktop) => "Computer",
            Some(QuickAccessKind::Documents) => "File",
            Some(QuickAccessKind::Pictures) => "Image",
            Some(QuickAccessKind::Videos) => "Movie",
            Some(QuickAccessKind::Music) => "MusicNote",
            Some(QuickAccessKind::RecycleBin) => "Delete",
            Some(QuickAccessKind::Generic) | None => "Folder",
        },
        // A tag keeps its own filled mark: there the colour IS the data.
        SidebarEntry::Tag(_) => return None,
    })
}

/// Extracts a row's rendered form (mirrors the `SidebarItem` `DataTemplate`).
/// `path_selected`: the GLOBAL verdict of `sidebar_selected_entry` for
/// path-bearing entries (the longest prefix wins, not local equality).
pub(crate) fn sidebar_visual(
    entry: &SidebarEntry,
    model: &HomeModel,
    active_location: &Location,
    path_selected: bool,
) -> SidebarVisual {
    let (glyph, label, indent, is_section, selected): (&'static str, String, f32, bool, bool) =
        match entry {
            SidebarEntry::Home => (
                GLYPH_HOME,
                drive_localization::tr("Home").into(),
                28.0,
                false,
                *active_location == Location::Home,
            ),
            SidebarEntry::SectionPinned => {
                (GLYPH_STAR, drive_localization::tr("Pinned").into(), 28.0, true, false)
            }
            SidebarEntry::SectionDrives => {
                (GLYPH_DRIVE, drive_localization::tr("Drives").into(), 28.0, true, false)
            }
            SidebarEntry::SectionLibraries => {
                ("\u{E8F1}", drive_localization::tr("SidebarLibraries").into(), 28.0, true, false)
            }
            SidebarEntry::Library(idx) => {
                let name = model
                    .libraries
                    .get(*idx)
                    .map(|l| l.name.clone())
                    .unwrap_or_default();
                ("\u{E8F1}", name, 48.0, false, path_selected)
            }
            SidebarEntry::SectionCloudDrives => {
                ("\u{E753}", drive_localization::tr("SidebarCloudDrives").into(), 28.0, true, false)
            }
            SidebarEntry::CloudDrive(idx) => {
                let cloud = &model.cloud_drives[*idx];
                ("\u{E753}", cloud.name.clone(), 48.0, false, path_selected)
            }
            SidebarEntry::SectionNetwork => {
                ("\u{E968}", drive_localization::tr("Network").into(), 28.0, true, false)
            }
            SidebarEntry::SectionTags => {
                ("\u{E8EC}", drive_localization::tr("FileTags").into(), 28.0, true, false)
            }
            SidebarEntry::NetworkDrive(idx) => {
                let drive = &model.drives[*idx];
                ("\u{E968}", drive.display_name(), 48.0, false, path_selected)
            }
            SidebarEntry::Tag(idx) => {
                let name = crate::services::settings::get()
                    .file_tags
                    .get(*idx)
                    .map(|t| t.name.clone())
                    .unwrap_or_default();
                ("\u{EA3B}", name, 48.0, false, false)
            }
            SidebarEntry::Folder(path, depth) => {
                let name = std::path::Path::new(path)
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| path.clone());
                (GLYPH_FOLDER, name, 48.0 + 16.0 * *depth as f32, false, path_selected)
            }
            SidebarEntry::Pinned(idx) => {
                let item = &model.quick_access[*idx];
                let (g, _) = quick_access_glyph(item.kind);
                (g, item.name.clone(), 48.0, false, path_selected)
            }
            SidebarEntry::Drive(idx) => {
                let drive = &model.drives[*idx];
                (GLYPH_DRIVE, drive.display_name(), 48.0, false, path_selected)
            }
            SidebarEntry::Settings => (
                GLYPH_SETTINGS,
                drive_localization::tr("Settings").into(),
                28.0,
                false,
                *active_location == Location::Settings,
            ),
        };
    SidebarVisual { glyph, label, indent, is_section, selected }
}

/// The natural width of a sidebar row (icon + label, untruncated), used
/// to decide whether the horizontal bar should appear.
pub(crate) fn sidebar_row_width(v: &SidebarVisual) -> f32 {
    // rect.left(8) + indent + icon + gap + text + right margin. The three
    // template metrics come from the row itself so this stays in step with the
    // drawing code (a stale gap here clips the label).
    use drive_app_controls::sidebar::{ROW_ICON_SIZE, ROW_ICON_TEXT_GAP, ROW_TEXT_RIGHT_MARGIN};
    8.0 + v.indent
        + ROW_ICON_SIZE
        + ROW_ICON_TEXT_GAP
        + approx_text_width(&v.label, 14.0)
        + ROW_TEXT_RIGHT_MARGIN
}
