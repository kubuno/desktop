//! SHELL properties of the info pane — port of
//! `FileProperty.RetrieveAndInitializePropertiesAsync` driven by
//! `PreviewPanePropertiesInformation.json` (copied as-is from the
//! original). Each entry names a system property
//! (`System.Image.Dimensions`, `System.Media.Duration`,
//! `System.Music.Artist`…); only those with a value are shown. Values go
//! through `PSFormatForDisplayAlloc` (Explorer's rendering — durations,
//! exposure fractions…), except DATES which go back through the app's
//! formatter (`ToLongLabel`).

use windows::core::PCWSTR;
use windows::Win32::Foundation::PROPERTYKEY;
use windows::Win32::System::Com::StructuredStorage::PropVariantToFileTime;
use windows::Win32::System::Variant::PSTF_UTC;
use windows::Win32::UI::Shell::PropertiesSystem::{
    IPropertyStore, PSFormatForDisplayAlloc, PSGetPropertyKeyFromName,
    SHGetPropertyStoreFromParsingName, GPS_BESTEFFORT, PDFF_DEFAULT,
};

#[derive(serde::Deserialize)]
#[serde(rename_all = "PascalCase")]
struct PropertyEntry {
    name_resource: String,
    property: Option<String>,
    #[serde(rename = "ID")]
    id: Option<String>,
    /// Numeric value → localized resource (SyncStatus, ColorSpace…); a
    /// value missing from the table HIDES the row, like the original.
    enumerated_list: Option<std::collections::HashMap<String, String>>,
}

fn entries() -> &'static [PropertyEntry] {
    static ENTRIES: std::sync::OnceLock<Vec<PropertyEntry>> = std::sync::OnceLock::new();
    ENTRIES.get_or_init(|| {
        // The file (copied as-is from the original) starts with a UTF-8
        // BOM that serde_json rejects.
        let json = include_str!("../../assets/PreviewPanePropertiesInformation.json");
        serde_json::from_str(json.trim_start_matches('\u{feff}')).unwrap_or_else(|e| {
            tracing::error!("PreviewPanePropertiesInformation.json invalide: {e}");
            Vec::new()
        })
    })
}

/// The pane's rows (localized label, value) for a FILE, in JSON order —
/// empty ones excluded, like `Where(i => i.ValueText != null)`. Memoized
/// on the last path: the pane redraws often, opening the property store
/// on every frame would be ruinous.
pub fn preview_pane_properties(path: &str) -> Vec<(String, String)> {
    type Rows = Vec<(String, String)>;
    static LAST: std::sync::Mutex<Option<(String, Rows)>> = std::sync::Mutex::new(None);
    if let Some((cached_path, rows)) = LAST.lock().unwrap().as_ref() {
        if cached_path == path {
            return rows.clone();
        }
    }
    let rows = read_properties(path);
    *LAST.lock().unwrap() = Some((path.to_string(), rows.clone()));
    rows
}

fn read_properties(path: &str) -> Vec<(String, String)> {
    let wide: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
    let store: IPropertyStore = match unsafe {
        SHGetPropertyStoreFromParsingName(PCWSTR(wide.as_ptr()), None, GPS_BESTEFFORT)
    } {
        Ok(store) => store,
        Err(e) => {
            tracing::warn!("property store failed for {path}: {e}");
            return Vec::new();
        }
    };

    let date_format = crate::services::settings::get().date_time_format;
    let mut rows = Vec::new();
    for entry in entries() {
        // `address` (online geocoding) and `filetag` (dedicated badges)
        // have their own handling — only system properties here.
        let Some(prop) = &entry.property else { continue };
        if entry.id.is_some() {
            continue;
        }
        let prop_wide: Vec<u16> = prop.encode_utf16().chain(std::iter::once(0)).collect();
        let mut key = PROPERTYKEY::default();
        if unsafe { PSGetPropertyKeyFromName(PCWSTR(prop_wide.as_ptr()), &mut key) }.is_err() {
            continue;
        }
        let Ok(value) = (unsafe { store.GetValue(&key) }) else { continue };
        let vt = unsafe { value.Anonymous.Anonymous.vt };
        if vt == windows::Win32::System::Variant::VT_EMPTY
            || vt == windows::Win32::System::Variant::VT_NULL
        {
            continue;
        }

        // Dates follow the app's format, not the shell's.
        let text = if matches!(
            prop.as_str(),
            "System.DateModified" | "System.DateCreated" | "System.Photo.DateTaken"
        ) {
            // UTC: the local conversion is left to the formatter (chrono Local).
            let Ok(ft) = (unsafe { PropVariantToFileTime(&value, PSTF_UTC) }) else {
                continue;
            };
            let intervals = ((ft.dwHighDateTime as u64) << 32) | ft.dwLowDateTime as u64;
            let unix_ns = (intervals.saturating_sub(116_444_736_000_000_000)) * 100;
            let time = std::time::UNIX_EPOCH + std::time::Duration::from_nanos(unix_ns);
            crate::services::date_time_formatter::to_long_label(time.into(), date_format)
        } else {
            let Ok(formatted) = (unsafe { PSFormatForDisplayAlloc(&key, &value, PDFF_DEFAULT) })
            else {
                continue;
            };
            let text = unsafe { formatted.to_string() }.unwrap_or_default();
            unsafe {
                windows::Win32::System::Com::CoTaskMemFree(Some(formatted.0 as _));
            }
            match &entry.enumerated_list {
                Some(map) => match map
                    .get(text.trim())
                    .and_then(|res| kubuno_drive_desktop_localization::tr_opt(res))
                {
                    Some(label) => label.to_string(),
                    None => continue,
                },
                None => text,
            }
        };
        if text.trim().is_empty() {
            continue;
        }
        let label = kubuno_drive_desktop_localization::tr_opt(&entry.name_resource)
            .unwrap_or(&entry.name_resource)
            .to_string();
        rows.push((label, text));
    }
    rows
}
