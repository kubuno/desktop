//! Localization (port of `StringsPropertyGenerator` + `AppLocalizationService`).
//!
//! # Approach
//!
//! ~49 cultures × ~1450 keys (~8.9 MB of .resw): embedding 49 static arrays
//! would blow up compile time. Hybrid approach:
//!
//! - **en-US** (reference/fallback) is parsed at build time (`build.rs`)
//!   into a static array sorted by key (`EN_US`) → binary search, zero cost
//!   at startup.
//! - Other cultures are embedded **raw** into the binary via
//!   `include_str!` (`LOCALES` table) and parsed **lazily** on the first
//!   `set_culture`; the result (sorted, leaked as `'static`) is memoized in
//!   a cache, so a culture is only parsed once per process (in practice:
//!   only the user's culture).
//!
//! # Example
//!
//! ```
//! kubuno_drive_desktop_localization::set_culture("fr-FR");
//! assert_eq!(kubuno_drive_desktop_localization::tr("Home"), "Accueil");
//! kubuno_drive_desktop_localization::set_culture(&kubuno_drive_desktop_localization::detect_system_culture());
//! ```

mod resw;

// Generates `EN_US` (sorted table) and `LOCALES` (tag → raw .resw).
include!(concat!(env!("OUT_DIR"), "/locales_gen.rs"));

use std::collections::HashMap;
use std::sync::{OnceLock, RwLock};

/// Translation table: (key, value) pairs sorted by key.
type Table = &'static [(&'static str, &'static str)];

struct Current {
    tag: &'static str,
    table: Table,
}

/// Current culture (en-US by default).
static CURRENT: OnceLock<RwLock<Current>> = OnceLock::new();
/// Cache of already-parsed cultures (tag → table leaked as `'static`).
static PARSED: OnceLock<RwLock<HashMap<&'static str, Table>>> = OnceLock::new();

fn current() -> &'static RwLock<Current> {
    CURRENT.get_or_init(|| {
        RwLock::new(Current {
            tag: "en-US",
            table: EN_US,
        })
    })
}

/// Normalizes a BCP-47 tag to an embedded culture:
/// 1. exact match (case-insensitive), e.g. `fr-FR`;
/// 2. exact match on the language alone, e.g. `vi`, `zh-Hans` for `zh`;
/// 3. first culture whose language matches, e.g. `fr-CA` → `fr-FR`;
/// 4. otherwise `en-US`.
fn normalize_culture(tag: &str) -> &'static str {
    let tag = tag.trim();
    for (culture, _) in LOCALES {
        if culture.eq_ignore_ascii_case(tag) {
            return culture;
        }
    }
    let lang = tag.split(['-', '_']).next().unwrap_or(tag);
    if !lang.is_empty() {
        for (culture, _) in LOCALES {
            if culture.eq_ignore_ascii_case(lang) {
                return culture;
            }
        }
        for (culture, _) in LOCALES {
            let culture_lang = culture.split('-').next().unwrap_or(culture);
            if culture_lang.eq_ignore_ascii_case(lang) {
                return culture;
            }
        }
    }
    "en-US"
}

/// Returns an embedded culture's table, parsing it (once) if necessary.
fn load_table(culture: &'static str) -> Table {
    if culture == "en-US" {
        return EN_US;
    }
    let cache = PARSED.get_or_init(|| RwLock::new(HashMap::new()));
    if let Some(table) = cache.read().unwrap().get(culture) {
        return table;
    }

    let raw = LOCALES
        .iter()
        .find(|(c, _)| *c == culture)
        .map(|(_, xml)| *xml)
        .unwrap_or("");
    let mut pairs = resw::parse_resw(raw);
    pairs.sort_by(|a, b| a.0.cmp(&b.0));
    pairs.dedup_by(|a, b| a.0 == b.0);
    let leaked: Vec<(&'static str, &'static str)> = pairs
        .into_iter()
        .map(|(k, v)| (&*k.leak(), &*v.leak()))
        .collect();
    let table: Table = Vec::leak(leaked);

    // On a race between two threads, the first table inserted wins (the
    // second, identical one, is simply dropped — bounded leak).
    let mut cache = cache.write().unwrap();
    cache.entry(culture).or_insert(table)
}

/// Binary search in a sorted table.
fn lookup(table: Table, key: &str) -> Option<&'static str> {
    table
        .binary_search_by(|(k, _)| (*k).cmp(key))
        .ok()
        .map(|idx| table[idx].1)
}

/// Changes the current culture. `tag` is normalized (see [`culture`]):
/// exact match, then by language (`"fr"` → `"fr-FR"`), otherwise falls
/// back to `en-US`. Returns the tag actually chosen.
pub fn set_culture(tag: &str) -> &'static str {
    let culture = normalize_culture(tag);
    let table = load_table(culture);
    let mut cur = current().write().unwrap();
    cur.tag = culture;
    cur.table = table;
    culture
}

/// Current culture tag (e.g. `"fr-FR"`; `"en-US"` by default).
pub fn culture() -> &'static str {
    current().read().unwrap().tag
}

/// List of embedded cultures, sorted by tag.
pub fn available_cultures() -> impl Iterator<Item = &'static str> {
    LOCALES.iter().map(|(c, _)| *c)
}

/// Windows user culture (`GetUserDefaultLocaleName`), falls back to
/// `"en-US"` on failure.
pub fn detect_system_culture() -> String {
    #[cfg(windows)]
    {
        use windows::Win32::Globalization::GetUserDefaultLocaleName;
        // LOCALE_NAME_MAX_LENGTH (winnt.h) — not exposed by the windows crate.
        const LOCALE_NAME_MAX_LENGTH: usize = 85;
        let mut buf = [0u16; LOCALE_NAME_MAX_LENGTH];
        // Returns the length **including** the trailing NUL, 0 on failure.
        let len = unsafe { GetUserDefaultLocaleName(&mut buf) };
        if len > 1 {
            return String::from_utf16_lossy(&buf[..(len - 1) as usize]);
        }
    }
    "en-US".to_string()
}

/// Translates `key`: current culture, then falls back to en-US, otherwise
/// returns the key as-is (behavior of `AppLocalizationService`/
/// `GetLocalizedResource`).
///
/// `key` must be `'static` (in practice a literal) so it can be returned
/// for an unknown key; for a dynamic key, use [`tr_opt`].
pub fn tr(key: &'static str) -> &'static str {
    tr_opt(key).unwrap_or(key)
}

/// Like [`tr`] but returns `None` for an unknown key (accepts a non-
/// `'static` key).
pub fn tr_opt(key: &str) -> Option<&'static str> {
    let table = current().read().unwrap().table;
    lookup(table, key).or_else(|| lookup(EN_US, key))
}

/// The On/Off text of a `ToggleSwitch`.
///
/// These two words do NOT come from Files' resources: a `ToggleSwitch`
/// without `OnContent`/`OffContent` (the one in the "Layout" pane, for
/// example) displays WinUI's system strings. They are therefore not in the
/// .resw files ported here; we reproduce them for the shipped cultures,
/// falling back to English — the behavior of WinUI itself when the
/// language isn't installed.
pub fn toggle_on_off() -> (&'static str, &'static str) {
    let tag = culture();
    let lang = tag.split('-').next().unwrap_or(tag);
    match lang {
        "fr" => ("Activé", "Désactivé"),
        "de" => ("Ein", "Aus"),
        "es" => ("Activado", "Desactivado"),
        "it" => ("Sì", "No"),
        "pt" => ("Ativado", "Desativado"),
        "nl" => ("Aan", "Uit"),
        "pl" => ("Wł.", "Wył."),
        "ru" => ("Вкл.", "Откл."),
        "tr" => ("Açık", "Kapalı"),
        "ja" => ("オン", "オフ"),
        "ko" => ("켬", "끔"),
        "zh" => ("开", "关"),
        _ => ("On", "Off"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// The current culture is global state: tests that modify it must be
    /// serialized.
    static LOCK: Mutex<()> = Mutex::new(());

    fn guard() -> std::sync::MutexGuard<'static, ()> {
        LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    #[test]
    fn en_us_known_keys() {
        let _g = guard();
        assert_eq!(set_culture("en-US"), "en-US");
        assert_eq!(culture(), "en-US");
        assert_eq!(tr("Home"), "Home");
        assert_eq!(tr("SortBy"), "Sort by");
        assert_eq!(tr("Settings"), "Settings");
    }

    #[test]
    fn fr_fr_known_keys() {
        let _g = guard();
        assert_eq!(set_culture("fr-FR"), "fr-FR");
        assert_eq!(tr("Home"), "Accueil");
        assert_eq!(tr("SortBy"), "Trier par");
        assert_eq!(tr("Settings"), "Paramètres");
        set_culture("en-US");
    }

    #[test]
    fn culture_normalization() {
        let _g = guard();
        // Language only → first matching culture.
        assert_eq!(set_culture("fr"), "fr-FR");
        // Unknown regional variant → falls back to the language.
        assert_eq!(set_culture("fr-CA"), "fr-FR");
        // Case-insensitive.
        assert_eq!(set_culture("FR-fr"), "fr-FR");
        // Culture with no region embedded as-is.
        assert_eq!(set_culture("vi"), "vi");
        assert_eq!(set_culture("zh"), "zh-Hans");
        set_culture("en-US");
    }

    #[test]
    fn unknown_culture_falls_back_to_en_us() {
        let _g = guard();
        assert_eq!(set_culture("xx-XX"), "en-US");
        assert_eq!(tr("Home"), "Home");
        assert_eq!(set_culture(""), "en-US");
    }

    #[test]
    fn unknown_key_returns_key() {
        let _g = guard();
        set_culture("en-US");
        assert_eq!(tr("ThisKeyDoesNotExist_12345"), "ThisKeyDoesNotExist_12345");
        assert_eq!(tr_opt("ThisKeyDoesNotExist_12345"), None);
    }

    #[test]
    fn missing_key_in_culture_falls_back_to_en_us() {
        let _g = guard();
        // "AutoFitColumns" exists in en-US but not in fr-FR.
        set_culture("fr-FR");
        assert_eq!(tr("AutoFitColumns"), "Auto-fit columns");
        set_culture("en-US");
    }

    #[test]
    fn detect_system_culture_is_non_empty() {
        let tag = detect_system_culture();
        assert!(!tag.is_empty());
        // Must at least normalize without panicking.
        let _ = normalize_culture(&tag);
    }

    #[test]
    fn embedded_cultures_all_parse() {
        // Every embedded .resw must produce a plausible number of keys.
        assert_eq!(available_cultures().count(), 49);
        for (culture, xml) in LOCALES {
            let pairs = resw::parse_resw(xml);
            assert!(
                pairs.len() > 500,
                "culture {culture}: seulement {} clés parsées",
                pairs.len()
            );
        }
    }

    #[test]
    fn en_us_table_is_sorted_and_unique() {
        assert!(EN_US.windows(2).all(|w| w[0].0 < w[1].0));
        assert!(EN_US.len() > 1000);
    }

    #[test]
    fn unescape_entities() {
        assert_eq!(
            resw::unescape_xml("a &amp; b &lt;c&gt; &quot;d&quot; &apos;e&apos; &#233; &#xE9;"),
            "a & b <c> \"d\" 'e' é é"
        );
        assert_eq!(resw::unescape_xml("pas d'entité"), "pas d'entité");
        assert_eq!(resw::unescape_xml("& seul"), "& seul");
    }
}
