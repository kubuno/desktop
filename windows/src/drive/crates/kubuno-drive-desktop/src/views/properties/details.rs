//! "Details" tab of the properties sheet — mirror of
//! `Views/Properties/DetailsPage.xaml` driven by
//! `FileProperties.GetSystemFilePropertiesAsync`.
//!
//! The original reads the file's SHELL properties via
//! `FileProperty.RetrieveAndInitializePropertiesAsync` (table
//! `Assets/Resources/PropertiesInformation.json`), groups them by SECTION
//! (`GroupBy(SectionResource)`), hides entirely empty sections (except
//! Music/Photo/Video, cf. `IsSectionApplicableForEmpty`) and places the
//! "Core" section last (`Priority`). Each row is a Name(140)/Value pair, the
//! value going through `PSFormatForDisplayAlloc` (Explorer's rendering)
//! except DATES which go through the app's formatter instead.
//!
//! This slice is READ-ONLY: editing (the `TextBox` for non-`IsReadOnly`
//! properties) and clearing (`ClearPropertiesButton`) are TODOs.

use windows::core::PCWSTR;
use windows::Win32::Foundation::PROPERTYKEY;
use windows::Win32::System::Com::StructuredStorage::PropVariantToFileTime;
use windows::Win32::System::Variant::{PSTF_UTC, VT_EMPTY, VT_NULL};
use windows::Win32::UI::Shell::PropertiesSystem::{
    IPropertyStore, PSFormatForDisplayAlloc, PSGetPropertyKeyFromName,
    SHGetPropertyStoreFromParsingName, GPS_BESTEFFORT, PDFF_DEFAULT,
};

use crate::styles::theme::Theme;
use crate::ui::{Painter, Rect};

use super::PropertiesTarget;

/// An entry of `PropertiesInformation.json` (the SAME schema as
/// `PreviewPanePropertiesInformation.json`, but a different FILE: it carries
/// all the "Details" sections, not just the preview pane).
#[derive(serde::Deserialize)]
#[serde(rename_all = "PascalCase")]
struct PropertyEntry {
    name_resource: String,
    section_resource: String,
    property: Option<String>,
    #[serde(rename = "ID")]
    id: Option<String>,
    enumerated_list: Option<std::collections::HashMap<String, String>>,
}

fn entries() -> &'static [PropertyEntry] {
    static ENTRIES: std::sync::OnceLock<Vec<PropertyEntry>> = std::sync::OnceLock::new();
    ENTRIES.get_or_init(|| {
        // The file (copied as-is from the original) starts with a UTF-8 BOM
        // that serde_json rejects — strip it, like `file_properties.rs` does.
        let json = include_str!("../../../assets/PropertiesInformation.json");
        serde_json::from_str(json.trim_start_matches('\u{feff}')).unwrap_or_else(|e| {
            tracing::error!("PropertiesInformation.json invalide: {e}");
            Vec::new()
        })
    })
}

/// A "Details" section (header + Name/Value rows), mirror of
/// `FilePropertySection`.
pub struct DetailsSection {
    /// The localized title (`Key.GetLocalizedResource()`).
    pub title: String,
    /// The section's non-empty (label, value) rows.
    pub rows: Vec<(String, String)>,
}

/// The tab's data, gathered once on opening.
pub struct DetailsModel {
    pub sections: Vec<DetailsSection>,
    /// Could the property store be opened (otherwise empty page, like
    /// `GetSystemFilePropertiesAsync` which bails out if `file is null`).
    pub loaded: bool,
}

impl DetailsModel {
    /// `GetSystemFilePropertiesAsync`: reads the file's system properties and
    /// groups them into sections. FILE only (the Details tab isn't offered
    /// for a folder/drive/multi-selection).
    pub fn gather(target: &PropertiesTarget) -> Self {
        let path = match target {
            PropertiesTarget::Path(p) if !std::path::Path::new(p).is_dir() => p.clone(),
            _ => return DetailsModel { sections: Vec::new(), loaded: false },
        };
        let ext = std::path::Path::new(&path)
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default();

        let wide: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
        let store: IPropertyStore = match unsafe {
            SHGetPropertyStoreFromParsingName(PCWSTR(wide.as_ptr()), None, GPS_BESTEFFORT)
        } {
            Ok(store) => store,
            Err(e) => {
                tracing::warn!("détails: magasin de propriétés indisponible pour {path}: {e}");
                return DetailsModel { sections: Vec::new(), loaded: false };
            }
        };

        let date_format = crate::services::settings::get().date_time_format;

        // Grouping by section, preserving the FIRST-APPEARANCE ORDER of the
        // keys (LINQ's `GroupBy` preserves this order) — including empty
        // sections, to faithfully reproduce `OrderBy`'s order.
        let mut order: Vec<&'static str> = Vec::new();
        let mut rows_by_section: std::collections::HashMap<&'static str, Vec<(String, String)>> =
            std::collections::HashMap::new();

        for entry in entries() {
            let key: &'static str = section_key(&entry.section_resource);
            if !order.contains(&key) {
                order.push(key);
            }
            // `address`/entries with an `ID`: dedicated handling in the
            // original (geocoding) — out of scope, we only read system properties.
            let Some(prop) = &entry.property else { continue };
            if entry.id.is_some() {
                continue;
            }
            if let Some(text) = read_value(&store, prop, entry.enumerated_list.as_ref(), date_format) {
                let label = kubuno_drive_desktop_localization::tr_opt(&entry.name_resource)
                    .unwrap_or(&entry.name_resource)
                    .to_string();
                rows_by_section.entry(key).or_default().push((label, text));
            }
        }

        // Sections kept: non-empty, OR applicable-when-empty (Music/Photo/
        // Video depending on extension), like `IsSectionApplicableForEmpty`.
        let mut sections: Vec<(&'static str, DetailsSection)> = Vec::new();
        for key in &order {
            let rows = rows_by_section.remove(key).unwrap_or_default();
            let keep = !rows.is_empty() || is_section_applicable_for_empty(key, &ext);
            if keep {
                let title = kubuno_drive_desktop_localization::tr_opt(key).unwrap_or(key).to_string();
                sections.push((key, DetailsSection { title, rows }));
            }
        }
        // STABLE `OrderBy(Priority)`: "Core" always last.
        sections.sort_by_key(|(k, _)| if *k == "PropertySectionCore" { 1 } else { 0 });

        DetailsModel {
            sections: sections.into_iter().map(|(_, s)| s).collect(),
            loaded: true,
        }
    }
}

/// Reads a system property and formats it (mirror of `file_properties.rs`'s
/// mechanics). Returns `None` if empty/absent.
fn read_value(
    store: &IPropertyStore,
    prop: &str,
    enumerated: Option<&std::collections::HashMap<String, String>>,
    date_format: crate::services::settings::DateTimeFormat,
) -> Option<String> {
    let prop_wide: Vec<u16> = prop.encode_utf16().chain(std::iter::once(0)).collect();
    let mut key = PROPERTYKEY::default();
    if unsafe { PSGetPropertyKeyFromName(PCWSTR(prop_wide.as_ptr()), &mut key) }.is_err() {
        return None;
    }
    let value = unsafe { store.GetValue(&key) }.ok()?;
    let vt = unsafe { value.Anonymous.Anonymous.vt };
    if vt == VT_EMPTY || vt == VT_NULL {
        return None;
    }

    // Dates follow the app's format (`ToLongLabel`), not the shell's.
    let text = if matches!(
        prop,
        "System.DateModified" | "System.DateCreated" | "System.Photo.DateTaken"
    ) {
        let ft = unsafe { PropVariantToFileTime(&value, PSTF_UTC) }.ok()?;
        let intervals = ((ft.dwHighDateTime as u64) << 32) | ft.dwLowDateTime as u64;
        let unix_ns = (intervals.saturating_sub(116_444_736_000_000_000)) * 100;
        let time = std::time::UNIX_EPOCH + std::time::Duration::from_nanos(unix_ns);
        crate::services::date_time_formatter::to_long_label(time.into(), date_format)
    } else {
        let formatted = unsafe { PSFormatForDisplayAlloc(&key, &value, PDFF_DEFAULT) }.ok()?;
        let text = unsafe { formatted.to_string() }.unwrap_or_default();
        unsafe {
            windows::Win32::System::Com::CoTaskMemFree(Some(formatted.0 as _));
        }
        match enumerated {
            Some(map) => {
                let res = map.get(text.trim())?;
                kubuno_drive_desktop_localization::tr_opt(res)?.to_string()
            }
            None => text,
        }
    };
    if text.trim().is_empty() {
        None
    } else {
        Some(text)
    }
}

/// `IsSectionApplicableForEmpty`: an empty section is only kept for an audio
/// (Music), image (Photo), or video (Video) file.
fn is_section_applicable_for_empty(section: &str, ext: &str) -> bool {
    match section {
        "PropertySectionMusic" => is_audio(ext),
        "PropertySectionPhoto" => is_image(ext),
        "PropertySectionVideo" => is_video(ext),
        _ => false,
    }
}

fn is_audio(ext: &str) -> bool {
    matches!(ext, "mp3" | "m4a" | "wav" | "flac" | "aac" | "wma" | "ogg" | "opus" | "aiff" | "alac")
}
fn is_image(ext: &str) -> bool {
    matches!(
        ext,
        "png" | "jpg" | "jpeg" | "bmp" | "gif" | "tiff" | "tif" | "webp" | "heic" | "heif"
            | "raw" | "cr2" | "nef" | "arw" | "dng" | "ico"
    )
}
fn is_video(ext: &str) -> bool {
    matches!(ext, "mp4" | "mkv" | "avi" | "mov" | "wmv" | "flv" | "webm" | "m4v" | "mpg" | "mpeg" | "3gp")
}

/// Converts a JSON section key to a `&'static str` (the keys are a known,
/// finite set ; default "Core").
fn section_key(s: &str) -> &'static str {
    match s {
        "PropertySectionImage" => "PropertySectionImage",
        "PropertySectionGPS" => "PropertySectionGPS",
        "PropertySectionPhoto" => "PropertySectionPhoto",
        "PropertySectionAudio" => "PropertySectionAudio",
        "PropertySectionMusic" => "PropertySectionMusic",
        "PropertySectionMedia" => "PropertySectionMedia",
        "PropertySectionVideo" => "PropertySectionVideo",
        "Document" => "Document",
        "PropertySectionCore" => "PropertySectionCore",
        _ => "PropertySectionCore",
    }
}

/// Draws the tab in `content`, offset by `scroll` (vertical scroll), and
/// returns the TOTAL HEIGHT of the content (to bound the scroll). The
/// content is clipped to `content` by the caller.
pub fn draw(p: &Painter, theme: &Theme, content: &Rect, model: &DetailsModel, scroll: f32) -> f32 {
    let f = &p.renderer.formats;
    let pad = 12.0;
    let left = content.left + pad;
    let right = content.right - pad;
    let mut y = content.top + pad - scroll;
    let top0 = content.top + pad - scroll;

    if !model.loaded {
        // `file is null`: unable to read the properties → empty page, like
        // `GetSystemFilePropertiesAsync` which silently bails out.
        return 0.0;
    }

    if model.sections.is_empty() {
        p.text(
            kubuno_drive_desktop_localization::tr("Loading"),
            &Rect::new(left, y, right, y + 24.0),
            &f.body,
            &theme.text_secondary,
            false,
        );
        return 24.0 + pad * 2.0;
    }

    for section in &model.sections {
        // `Expander` header (always expanded, `IsExpanded=True`).
        let header = Rect::new(left, y, right, y + 40.0);
        p.fill_rounded(&header, 4.0, &theme.card_background);
        p.stroke_rounded(&header, 4.0, &theme.card_stroke);
        p.text(
            &section.title,
            &Rect::new(header.left + 16.0, header.top, header.right - 40.0, header.bottom),
            &f.body,
            &theme.text_primary,
            false,
        );
        // Down chevron (expanded).
        p.text(
            "\u{E70E}",
            &Rect::new(header.right - 36.0, header.top, header.right - 8.0, header.bottom),
            &f.icon_small,
            &theme.text_secondary,
            true,
        );
        y = header.bottom;

        // Name(140)/Value rows.
        let ix = left + 4.0;
        let iright = right - 4.0;
        let lw = 140.0;
        let mut ry = y + 6.0;
        for (name, value) in &section.rows {
            let row_h = 24.0;
            let row = Rect::new(ix, ry, iright, ry + row_h);
            p.text(
                name,
                &Rect::new(row.left, row.top, row.left + lw, row.bottom),
                &f.body_strong,
                &theme.text_secondary,
                false,
            );
            let vrect = Rect::new(row.left + lw + 8.0, row.top, row.right, row.bottom);
            p.text_ellipsis(value, &vrect, &f.body, &theme.text_primary);
            ry = row.bottom + 2.0;
        }
        y = ry + 8.0;
    }

    // TODO (later slice): editing of non-`IsReadOnly` properties
    // (TextBox) and "Clear properties" button (`ClearPropertiesButton`).
    y - top0 + pad
}
