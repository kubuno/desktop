//! "General" tab of the properties sheet — mirror of
//! `Views/Properties/GeneralPage.xaml` driven by `BaseProperties` /
//! `FileProperties` / `FolderProperties` / `DriveProperties`.
//!
//! The port is single-window immediate D2D: instead of a `SelectedItemsProperties
//! ViewModel` bound in XAML, we gather the same fields in [`GeneralModel`]
//! (name, type, location, size, dates, attributes, or drive capacity) then
//! paint them by hand via [`ui::Painter`]. The layout follows the page's
//! `StackPanel Padding="12" Spacing="4"`: "Preview" card (56 icon + name +
//! type), "Details" expander (Location, Size, Created/Modified/Accessed) and
//! "Attributes" expander (Read-only / Hidden). For a drive, the "File
//! system" row and the disk space gauge replace the details, like
//! `DriveProperties`.

use windows::core::PCWSTR;

use crate::styles::theme::Theme;
use crate::ui::{Painter, Rect};
use crate::services::storage::IconCache;

use super::PropertiesTarget;

/// The preview's shell icon: 56 DIP box, 48 DIP icon, like `GeneralPage.xaml`'s
/// `uc:FileIcon ItemSize="48"`.
pub const ICON_BOX: f32 = 56.0;
pub const ICON_SIZE: f32 = 48.0;
/// `Constants.ShellIconSizes.ExtraLarge` — the size requested for the icon
/// (`FileThumbnailHelper.GetIconAsync(..., ExtraLarge, ...)`).
pub const SHELL_ICON_EXTRA_LARGE: f32 = 48.0;

/// The General tab's data, gathered once on opening (the equivalent of
/// `GetBaseProperties` + the start of `GetSpecialPropertiesAsync`).
pub struct GeneralModel {
    /// Path used to retrieve the shell icon (file/folder/drive root).
    /// `None` in multi-selection (generic folder glyph).
    pub icon_path: Option<String>,
    /// Editable name (`ViewModel.ItemName`).
    pub name: String,
    /// Localized type (`ViewModel.ItemType`) — the file's shell description,
    /// or "DriveType{Type}" for a drive.
    pub item_type: String,
    /// Parent location (`ViewModel.ItemLocation`).
    pub location: Option<String>,
    /// Is the Size row shown (`ItemSizeVisibility`).
    pub show_size: bool,
    /// Is the recursive folder size computation in progress
    /// (`ItemSizeProgressVisibility` → indeterminate ProgressBar).
    pub size_computing: bool,
    /// Known byte count (immediate for a file ; a folder once computed).
    pub size_bytes: u64,
    /// Formatted dates (`ToLongLabel`) — `None` hides the row.
    pub created: Option<String>,
    pub modified: Option<String>,
    pub accessed: Option<String>,
    /// "Attributes" expander visible (`ItemAttributesVisibility`: hidden for
    /// a drive).
    pub show_attributes: bool,
    /// "Read-only" checkbox active (`IsReadOnlyEnabled`: folders don't carry
    /// this attribute reliably → greyed out).
    pub read_only_enabled: bool,
    pub is_read_only: bool,
    pub is_hidden: bool,
    // --- Drive (`DriveProperties`) ---
    pub is_drive: bool,
    /// File system (`ViewModel.DriveFileSystem`, e.g. "NTFS").
    pub file_system: Option<String>,
    pub drive_total: u64,
    pub drive_free: u64,
    pub drive_used: u64,
}

impl GeneralModel {
    /// Gathers the tab's fields for the target (`GetBaseProperties`).
    /// Folder size is NOT computed here: the window requests it from the
    /// `SizeProvider` in the background (cf. `GetFolderSizeAsync`).
    pub fn gather(target: &PropertiesTarget) -> Self {
        let date_format = crate::services::settings::get().date_time_format;
        let fmt = |t: std::time::SystemTime| -> String {
            let dt: chrono::DateTime<chrono::Local> = t.into();
            crate::services::date_time_formatter::to_long_label(dt, date_format)
        };

        match target {
            PropertiesTarget::Path(path) => {
                let p = std::path::Path::new(path);
                let is_dir = p.is_dir();
                let name = p
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| path.clone());
                let location = p.parent().map(|d| d.to_string_lossy().into_owned());
                let meta = std::fs::metadata(path).ok();
                let (created, modified, accessed) = match &meta {
                    Some(m) => (
                        m.created().ok().map(fmt),
                        m.modified().ok().map(fmt),
                        m.accessed().ok().map(fmt),
                    ),
                    None => (None, None, None),
                };
                let (is_read_only, is_hidden) = read_attributes(path);
                GeneralModel {
                    icon_path: Some(path.clone()),
                    name,
                    item_type: shell_type_name(path, is_dir),
                    location,
                    show_size: true,
                    // A folder: size unknown until the worker is done
                    // (indeterminate progress). A file: immediate size.
                    size_computing: is_dir,
                    size_bytes: if is_dir { 0 } else { meta.map(|m| m.len()).unwrap_or(0) },
                    created,
                    modified,
                    accessed,
                    show_attributes: true,
                    // `IsReadOnlyEnabled`: the original enables the checkbox
                    // for a file ; for a folder the ReadOnly attribute has no
                    // real meaning (it drives customization) → greyed out.
                    read_only_enabled: !is_dir,
                    is_read_only,
                    is_hidden,
                    is_drive: false,
                    file_system: None,
                    drive_total: 0,
                    drive_free: 0,
                    drive_used: 0,
                }
            }
            PropertiesTarget::Drive(letter) => {
                let root = format!("{letter}:\\");
                let (total, free, fs) = drive_info(*letter);
                GeneralModel {
                    icon_path: Some(root.clone()),
                    name: drive_display_name(*letter),
                    item_type: drive_localization::tr("DriveTypeFixed").to_string(),
                    location: None,
                    show_size: false,
                    size_computing: false,
                    size_bytes: 0,
                    created: None,
                    modified: None,
                    accessed: None,
                    // `DriveProperties`: `ItemAttributesVisibility = false`.
                    show_attributes: false,
                    read_only_enabled: false,
                    is_read_only: false,
                    is_hidden: false,
                    is_drive: true,
                    file_system: fs,
                    drive_total: total,
                    drive_free: free,
                    drive_used: total.saturating_sub(free),
                }
            }
            PropertiesTarget::Multi(paths) => {
                // Multi-selection: `CombinedProperties` — for this slice we
                // stick to the name ("N items") ; the detail is a TODO.
                GeneralModel {
                    icon_path: None,
                    name: format!("{} éléments", paths.len()),
                    item_type: String::new(),
                    location: None,
                    show_size: false,
                    size_computing: false,
                    size_bytes: 0,
                    created: None,
                    modified: None,
                    accessed: None,
                    show_attributes: false,
                    read_only_enabled: false,
                    is_read_only: false,
                    is_hidden: false,
                    is_drive: false,
                    file_system: None,
                    drive_total: 0,
                    drive_free: 0,
                    drive_used: 0,
                }
            }
        }
    }

    /// The text of the Size row (`ItemSize`, `ToLongSizeString`), empty as
    /// long as a folder's computation hasn't produced a value.
    pub fn size_text(&self) -> String {
        if self.size_computing && self.size_bytes == 0 {
            String::new()
        } else {
            crate::data::items::format_bytes_fr(self.size_bytes)
        }
    }
}

/// The tab's clickable areas, collected by the draw (like the info panel's
/// `info_*_rect`: their position depends on the painted layout).
#[derive(Default, Clone, Copy)]
pub struct GeneralHits {
    pub details_header: Option<Rect>,
    pub attributes_header: Option<Rect>,
    pub read_only: Option<Rect>,
    pub hidden: Option<Rect>,
}

/// Draws the tab in `content` (the inner area of the panel, above the
/// Save/Cancel bar) and returns its clickable areas. `anim` drives the
/// indeterminate progress bar.
#[allow(clippy::too_many_arguments)]
pub fn draw_general(
    p: &Painter,
    theme: &Theme,
    content: &Rect,
    model: &GeneralModel,
    details_expanded: bool,
    attr_expanded: bool,
    icons: &IconCache,
    scale: f32,
    anim: std::time::Duration,
) -> GeneralHits {
    let f = &p.renderer.formats;
    let mut hits = GeneralHits::default();
    let pad = 12.0;
    let left = content.left + pad;
    let right = content.right - pad;
    let mut y = content.top + pad;

    // ---- "Item preview" card --------------------------------------------
    // Grid Padding=12, 56 icon + (name / type).
    let card_h = 12.0 + ICON_BOX + 12.0;
    let card = Rect::new(left, y, right, y + card_h);
    p.fill_rounded(&card, 4.0, &theme.card_background);
    p.stroke_rounded(&card, 4.0, &theme.card_stroke);

    // 56/48 shell icon in a bordered box (the 56x56 `Grid` CornerRadius=4).
    let icon_box = Rect::new(card.left + 12.0, card.top + 12.0, card.left + 12.0 + ICON_BOX, card.top + 12.0 + ICON_BOX);
    p.stroke_rounded(&icon_box, 4.0, &theme.card_stroke);
    let mut drew_icon = false;
    if let Some(path) = &model.icon_path {
        if let Some(bmp) = icons.get(path, (ICON_SIZE * scale).round() as i32) {
            p.image(bmp, &icon_box, ICON_SIZE);
            drew_icon = true;
        }
    }
    if !drew_icon {
        // Fallback: folder/drive/file glyph (F0 Segoe Fluent Icons).
        let glyph = if model.is_drive {
            "\u{EDA2}"
        } else if model.icon_path.as_deref().map(|p| std::path::Path::new(p).is_dir()).unwrap_or(true) {
            "\u{E8B7}"
        } else {
            "\u{E8A5}"
        };
        p.text(glyph, &icon_box, &f.icon_large, &theme.text_secondary, true);
    }

    // Right column: name (TextBox — displayed ; editing = TODO) then type.
    let col_x = icon_box.right + 12.0;
    let name_rect = Rect::new(col_x, card.top + 12.0, right - 12.0, card.top + 12.0 + 32.0);
    // The name lives in a `TextBox`: paint its frame (background + border)
    // then the text. Editing/renaming (`ItemName` TwoWay) stays a TODO.
    p.fill_rounded(&name_rect, 4.0, &theme.toolbar_background);
    p.stroke_rounded(&name_rect, 4.0, &theme.card_stroke);
    let name_text = Rect::new(name_rect.left + 10.0, name_rect.top, name_rect.right - 10.0, name_rect.bottom);
    p.text_ellipsis(&model.name, &name_text, &f.body, &theme.text_primary);

    // Type row (or File system for a drive): in the RIGHT COLUMN, under the
    // name (to the right of the icon), to fit within the 80 DIP card without
    // overflowing into the "More details" expander.
    let type_y = name_rect.bottom + 6.0;
    let label_w = 96.0;
    if model.is_drive {
        draw_kv(p, theme, tr("PropertiesDriveFileSystem.Text"), model.file_system.as_deref().unwrap_or(""), col_x, type_y, label_w, right - 12.0);
    } else if !model.item_type.is_empty() {
        draw_kv(p, theme, tr("ItemType"), &model.item_type, col_x, type_y, label_w, right - 12.0);
    }
    y = card.bottom + 4.0;

    // ---- Drive: disk space gauge -----------------------------------------
    if model.is_drive {
        draw_drive_capacity(p, theme, left, right, y, model);
        return hits;
    }

    // ---- "Details" expander (MoreDetails) --------------------------------
    let header = Rect::new(left, y, right, y + 40.0);
    draw_expander_header(p, theme, &header, tr("MoreDetails"), details_expanded);
    hits.details_header = Some(header);
    y = header.bottom;

    if details_expanded {
        let ix = left + 16.0;
        let iright = right - 16.0;
        let lw = 120.0;
        let mut ry = y + 8.0;
        // Location.
        if let Some(loc) = &model.location {
            ry = draw_kv(p, theme, tr("Location"), loc, ix, ry, lw, iright) + 8.0;
        }
        // Size (+ indeterminate ProgressBar during folder computation).
        if model.show_size {
            let row = Rect::new(ix, ry, iright, ry + 22.0);
            p.text(tr("SizeLabel"), &Rect::new(row.left, row.top, row.left + lw, row.bottom), &f.body_strong, &theme.text_secondary, false);
            let vx = row.left + lw;
            let size = model.size_text();
            if !size.is_empty() {
                p.text(&size, &Rect::new(vx, row.top, vx + 120.0, row.bottom), &f.body, &theme.text_primary, false);
            }
            if model.size_computing {
                let bar = Rect::new(vx + 124.0, row.top + 8.0, (vx + 124.0 + 100.0).min(iright), row.bottom - 8.0);
                draw_indeterminate_bar(p, theme, &bar, anim);
            }
            ry = row.bottom + 8.0;
        }
        // Separator (`DividerStrokeColorDefaultBrush`, Margin -16,0).
        let sep = Rect::new(left, ry, right, ry + 1.0);
        p.fill_rounded(&sep, 0.0, &theme.divider);
        ry += 1.0 + 8.0;
        // Created / Modified / Accessed dates.
        for (key, val) in [
            ("PropertiesCreated.Text", &model.created),
            ("PropertiesModified.Text", &model.modified),
            ("Accessed", &model.accessed),
        ] {
            if let Some(v) = val {
                ry = draw_kv(p, theme, tr_key(key), v, ix, ry, lw, iright) + 8.0;
            }
        }
        y = ry;
    }
    y += 4.0;

    // ---- "Attributes" expander --------------------------------------------
    if model.show_attributes {
        let header = Rect::new(left, y, right, y + 40.0);
        draw_expander_header(p, theme, &header, tr("Attributes"), attr_expanded);
        hits.attributes_header = Some(header);
        y = header.bottom;
        if attr_expanded {
            let ix = left + 16.0;
            let iright = right - 16.0;
            let mut ry = y + 8.0;
            hits.read_only = Some(draw_checkbox_row(
                p, theme, ix, iright, ry, tr_key("PropertiesDialogReadOnly.Text"), model.is_read_only, model.read_only_enabled,
            ));
            ry += 32.0;
            let sep = Rect::new(left, ry, right, ry + 1.0);
            p.fill_rounded(&sep, 0.0, &theme.divider);
            ry += 1.0 + 8.0;
            hits.hidden = Some(draw_checkbox_row(
                p, theme, ix, iright, ry, tr("Hidden"), model.is_hidden, true,
            ));
        }
    }

    hits
}

/// "Label: value" row of the Details grid (label column fixed at `lw`).
#[allow(clippy::too_many_arguments)] // a paint entry point: the frame's inputs, passed flat
fn draw_kv(p: &Painter, theme: &Theme, label: &str, value: &str, left: f32, top: f32, lw: f32, right: f32) -> f32 {
    let f = &p.renderer.formats;
    let row = Rect::new(left, top, right, top + 22.0);
    p.text(label, &Rect::new(row.left, row.top, row.left + lw, row.bottom), &f.body_strong, &theme.text_secondary, false);
    let vrect = Rect::new(row.left + lw, row.top, row.right, row.bottom);
    p.text_ellipsis(value, &vrect, &f.body, &theme.text_primary);
    row.bottom
}

/// `Expander` header (chevron + title) — a 40 DIP clickable track.
fn draw_expander_header(p: &Painter, theme: &Theme, rect: &Rect, title: &str, expanded: bool) {
    let f = &p.renderer.formats;
    p.fill_rounded(rect, 4.0, &theme.card_background);
    p.stroke_rounded(rect, 4.0, &theme.card_stroke);
    p.text(title, &Rect::new(rect.left + 16.0, rect.top, rect.right - 40.0, rect.bottom), &f.body, &theme.text_primary, false);
    // Chevron `\u{E70D}` (down) / `\u{E70E}` (up) on the right.
    let chevron = if expanded { "\u{E70E}" } else { "\u{E70D}" };
    let cr = Rect::new(rect.right - 36.0, rect.top, rect.right - 8.0, rect.bottom);
    p.text(chevron, &cr, &f.icon_small, &theme.text_secondary, true);
}

/// "Label ...... [checkbox]" row of an attributes expander. Returns the rect
/// of the checkbox (clickable area).
#[allow(clippy::too_many_arguments)] // a paint entry point: the frame's inputs, passed flat
fn draw_checkbox_row(p: &Painter, theme: &Theme, left: f32, right: f32, top: f32, label: &str, checked: bool, enabled: bool) -> Rect {
    let f = &p.renderer.formats;
    let color = if enabled { theme.text_primary } else { theme.text_secondary };
    p.text(label, &Rect::new(left, top, right - 40.0, top + 24.0), &f.body, &color, false);
    let box_rect = Rect::new(right - 20.0, top + 2.0, right, top + 22.0);
    let fill = if checked && enabled { theme.accent } else { theme.toolbar_background };
    p.fill_rounded(&box_rect, 4.0, &fill);
    if checked {
        p.text("\u{E73E}", &box_rect, &f.icon_small, &theme.accent_foreground, true);
    } else {
        p.stroke_rounded(&box_rect, 4.0, &theme.card_stroke);
    }
    box_rect
}

/// Indeterminate progress bar (`ProgressBar IsIndeterminate=True`): an accent
/// segment that sweeps the track. `anim` is the elapsed time.
fn draw_indeterminate_bar(p: &Painter, theme: &Theme, track: &Rect, anim: std::time::Duration) {
    p.fill_rounded(track, 2.0, &theme.drive_bar_track);
    let w = track.right - track.left;
    let seg = w * 0.35;
    // ~1.6 s cycle, back-and-forth of a segment (approximation of the WinUI visual).
    let t = (anim.as_secs_f32() / 1.6).fract();
    let travel = (w + seg) * t - seg;
    let x0 = (track.left + travel).clamp(track.left, track.right);
    let x1 = (track.left + travel + seg).clamp(track.left, track.right);
    if x1 > x0 {
        p.fill_rounded(&Rect::new(x0, track.top, x1, track.bottom), 2.0, &theme.accent);
    }
}

/// Disk space gauge (`DiskDetailsGrid`): used (accent) / free / capacity,
/// with a horizontal used-fraction bar. Deliberate simplification of the
/// original's circular `ProgressRing`.
fn draw_drive_capacity(p: &Painter, theme: &Theme, left: f32, right: f32, top: f32, model: &GeneralModel) -> f32 {
    let f = &p.renderer.formats;
    let tr = drive_localization::tr;
    let card = Rect::new(left, top, right, top + 128.0);
    p.fill_rounded(&card, 4.0, &theme.card_background);
    p.stroke_rounded(&card, 4.0, &theme.card_stroke);
    let ix = card.left + 12.0;
    let iright = card.right - 12.0;

    // Used-fraction bar.
    let frac = if model.drive_total == 0 {
        0.0
    } else {
        (model.drive_used as f64 / model.drive_total as f64) as f32
    };
    let bar = Rect::new(ix, card.top + 12.0, iright, card.top + 20.0);
    p.fill_rounded(&bar, 4.0, &theme.drive_bar_track);
    let used_w = (bar.right - bar.left) * frac.clamp(0.0, 1.0);
    if used_w > 0.0 {
        p.fill_rounded(&Rect::new(bar.left, bar.top, bar.left + used_w, bar.bottom), 4.0, &theme.accent);
    }
    let pct = format!("{:.0}%", frac * 100.0);
    p.text(&pct, &Rect::new(ix, bar.bottom + 4.0, iright, bar.bottom + 26.0), &f.body_strong, &theme.text_primary, true);

    // Used / Free / Capacity rows (`PropertiesDrive*`).
    let fb = crate::data::items::format_bytes_fr;
    let mut ry = bar.bottom + 30.0;
    for (label, value) in [
        (tr("PropertiesDriveUsedSpace.Text"), fb(model.drive_used)),
        (tr("PropertiesDriveFreeSpace.Text"), fb(model.drive_free)),
        (tr("PropertiesDriveCapacity.Text"), fb(model.drive_total)),
    ] {
        ry = draw_kv(p, theme, label, &value, ix, ry, 140.0, iright) + 6.0;
    }
    card.bottom + 4.0
}

fn tr(key: &'static str) -> &'static str {
    drive_localization::tr(key)
}

/// `tr` for a dynamic dotted key (`PropertiesModified.Text`…).
fn tr_key(key: &str) -> &str {
    drive_localization::tr_opt(key).unwrap_or(key)
}

/// Shell type of an item (`SHGetFileInfo` `SHGFI_TYPENAME`) — the equivalent
/// of `ListedItem.ItemType` ("Text Document", "File folder"…).
fn shell_type_name(path: &str, is_dir: bool) -> String {
    use windows::Win32::Storage::FileSystem::{FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_NORMAL};
    use windows::Win32::UI::Shell::{
        SHGetFileInfoW, SHFILEINFOW, SHGFI_TYPENAME, SHGFI_USEFILEATTRIBUTES,
    };
    let wide: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
    let mut info = SHFILEINFOW::default();
    let attr = if is_dir { FILE_ATTRIBUTE_DIRECTORY } else { FILE_ATTRIBUTE_NORMAL };
    let ret = unsafe {
        SHGetFileInfoW(
            PCWSTR(wide.as_ptr()),
            attr,
            Some(&mut info as *mut _),
            std::mem::size_of::<SHFILEINFOW>() as u32,
            SHGFI_TYPENAME | SHGFI_USEFILEATTRIBUTES,
        )
    };
    if ret == 0 {
        return if is_dir { "Dossier de fichiers".into() } else { "Fichier".into() };
    }
    String::from_utf16_lossy(&info.szTypeName)
        .trim_end_matches('\0')
        .to_string()
}

/// Reads `ReadOnly` / `Hidden` from `GetFileAttributesW` (like
/// `Win32Helper.GetFileAttributes`).
fn read_attributes(path: &str) -> (bool, bool) {
    use windows::core::HSTRING;
    use windows::Win32::Storage::FileSystem::{
        GetFileAttributesW, FILE_ATTRIBUTE_HIDDEN, FILE_ATTRIBUTE_READONLY, INVALID_FILE_ATTRIBUTES,
    };
    let wide = HSTRING::from(path);
    let a = unsafe { GetFileAttributesW(&wide) };
    if a == INVALID_FILE_ATTRIBUTES {
        return (false, false);
    }
    (a & FILE_ATTRIBUTE_READONLY.0 != 0, a & FILE_ATTRIBUTE_HIDDEN.0 != 0)
}

/// Writes the `ReadOnly` / `Hidden` attributes at Save time
/// (`Win32Helper.SetFileAttribute` / `UnsetFileAttribute`).
pub fn write_attributes(path: &str, read_only: bool, hidden: bool) {
    use windows::core::HSTRING;
    use windows::Win32::Storage::FileSystem::{
        GetFileAttributesW, SetFileAttributesW, FILE_ATTRIBUTE_HIDDEN, FILE_ATTRIBUTE_READONLY,
        FILE_FLAGS_AND_ATTRIBUTES, INVALID_FILE_ATTRIBUTES,
    };
    let wide = HSTRING::from(path);
    let mut a = unsafe { GetFileAttributesW(&wide) };
    if a == INVALID_FILE_ATTRIBUTES {
        return;
    }
    if read_only {
        a |= FILE_ATTRIBUTE_READONLY.0;
    } else {
        a &= !FILE_ATTRIBUTE_READONLY.0;
    }
    if hidden {
        a |= FILE_ATTRIBUTE_HIDDEN.0;
    } else {
        a &= !FILE_ATTRIBUTE_HIDDEN.0;
    }
    unsafe {
        let _ = SetFileAttributesW(&wide, FILE_FLAGS_AND_ATTRIBUTES(a));
    }
}

/// Display name of a drive ("Local Disk (C:)") via its volume label, like
/// `DriveItem.Text`.
fn drive_display_name(letter: char) -> String {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::MAX_PATH;
    use windows::Win32::Storage::FileSystem::GetVolumeInformationW;
    let root: Vec<u16> = format!("{letter}:\\").encode_utf16().chain([0]).collect();
    let mut label_buf = [0u16; MAX_PATH as usize + 1];
    let label = unsafe {
        GetVolumeInformationW(PCWSTR(root.as_ptr()), Some(&mut label_buf), None, None, None, None)
    }
    .ok()
    .map(|_| String::from_utf16_lossy(&label_buf).trim_end_matches('\0').to_string())
    .filter(|s| !s.is_empty())
    .unwrap_or_else(|| "Disque local".to_string());
    format!("{label} ({letter}:)")
}

/// (capacity, free, file system) of a drive
/// (`GetDiskFreeSpaceExW` + `GetVolumeInformationW`).
fn drive_info(letter: char) -> (u64, u64, Option<String>) {
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::{GetDiskFreeSpaceExW, GetVolumeInformationW};
    let root: Vec<u16> = format!("{letter}:\\").encode_utf16().chain([0]).collect();
    let rp = PCWSTR(root.as_ptr());
    let mut free = 0u64;
    let mut total = 0u64;
    unsafe {
        let _ = GetDiskFreeSpaceExW(rp, None, Some(&mut total), Some(&mut free));
    }
    let mut fs_buf = [0u16; 64];
    let fs = unsafe { GetVolumeInformationW(rp, None, None, None, None, Some(&mut fs_buf)) }
        .ok()
        .map(|_| String::from_utf16_lossy(&fs_buf).trim_end_matches('\0').to_string())
        .filter(|s| !s.is_empty());
    (total, free, fs)
}
