//! Port of `Files.App/ViewModels/Settings/SettingsSearchIndexer` (+
//! `SettingsSearchResult`): the settings search index used by the Omnibar
//! in search mode when on the Settings page
//! (`NavigationToolbarViewModel.PopulateOmnibarSuggestionsForSettingsSearch`
//! → `SettingsSearchIndexer.BuildIndex()`).
//!
//! The C# `BuildIndex` instantiates each settings page then walks its
//! visual tree (`Walk`), recording one entry per `SettingsExpander` header
//! and per `SettingsCard`, with the optional parent header:
//!
//! ```csharp
//! case SettingsExpander expander when expander.Header is string groupHeader ...:
//!     results.Add(new SettingsSearchResult(kind, pageName, groupHeader));
//!     foreach (var item in expander.Items)
//!         Walk(item, kind, pageName, groupHeader, results);
//! case SettingsCard card when card.Header is string cardHeader ...:
//!     results.Add(new SettingsSearchResult(kind, pageName, cardHeader, parentHeader));
//! ```
//!
//! and matching is done on a `Haystack` = "page name + [parent header +]
//! header", with every query term required to appear in it:
//!
//! ```csharp
//! Haystack = parentHeaderText is null
//!     ? $"{pageDisplayName} {headerText}"
//!     : $"{pageDisplayName} {parentHeaderText} {headerText}";
//! ...
//! if (terms.All(term => entry.Haystack.Contains(term, StringComparison.CurrentCultureIgnoreCase)))
//! ```
//!
//! Here, the equivalent of `SettingsCard`/`SettingsExpander` is the
//! `SettingsRow` line produced by `views::settings::page_rows()`: we reuse
//! that same source (faithful and without duplication) to build the index.
//! Only the Appearance page doesn't expose a `rows()` (it draws itself
//! line by line): its settings are enumerated explicitly, in XAML order.

// NOT WIRED YET. Ported from Files' settings search; waits for the search box of the Settings dialog, which is not ported yet.
// The allow is scoped to this file and must go once it is wired.
#![allow(dead_code)]

use crate::services::settings::AppSettings;
use crate::views::settings::controls::{RowKind, SettingId, SettingsRow};

/// Settings pages in the port (order of `SETTINGS_SECTIONS`, without the
/// Actions page which doesn't contain `.resw` settings but the shortcut
/// editor). Mirrors `Data/Enums/SettingsPageKind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsPage {
    General,
    Appearance,
    Layout,
    FilesFolders,
    Tags,
    DevTools,
    Advanced,
    About,
}

impl SettingsPage {
    /// All indexed pages, in `BuildIndex` order.
    pub const ALL: [SettingsPage; 8] = [
        SettingsPage::General,
        SettingsPage::Appearance,
        SettingsPage::Layout,
        SettingsPage::FilesFolders,
        SettingsPage::Tags,
        SettingsPage::DevTools,
        SettingsPage::Advanced,
        SettingsPage::About,
    ];

    /// `.resw` key of the page's display name (the same as `BuildIndex`:
    /// `Strings.General`, `Strings.FilesAndFolders`, `Strings.FileTags`…).
    pub fn display_name_key(self) -> &'static str {
        match self {
            SettingsPage::General => "General",
            SettingsPage::Appearance => "Appearance",
            SettingsPage::Layout => "Layout",
            SettingsPage::FilesFolders => "FilesAndFolders",
            SettingsPage::Tags => "FileTags",
            SettingsPage::DevTools => "DevTools",
            SettingsPage::Advanced => "Advanced",
            SettingsPage::About => "About",
        }
    }

    /// Index into `ui::SETTINGS_SECTIONS`: the navigation target
    /// (`state.settings_section`) to jump to the page from a result.
    pub fn section_index(self) -> usize {
        match self {
            SettingsPage::General => 0,
            SettingsPage::Appearance => 1,
            SettingsPage::Layout => 2,
            SettingsPage::FilesFolders => 3,
            // 4 = Actions page (not indexed).
            SettingsPage::Tags => 5,
            SettingsPage::DevTools => 6,
            SettingsPage::Advanced => 7,
            SettingsPage::About => 8,
        }
    }
}

/// An index entry: an indexable setting, the page it lives on and the
/// keywords used for filtering (page name + parent header, mirroring
/// `SettingsSearchResult`'s `Haystack`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsSearchEntry {
    /// Setting label (the `SettingsCard`/`SettingsExpander` header).
    pub label: String,
    /// The page to open when this result is chosen.
    pub page: SettingsPage,
    /// Search context: the page's display name, then, if the setting is
    /// inside an expander, that expander's header.
    pub keywords: Vec<String>,
}

impl SettingsSearchEntry {
    /// Displayed result path: "Page › Group" (like
    /// `SettingsSearchResult.DisplayPath`). The first keyword is the page
    /// name, the second (optional) the parent header.
    pub fn display_path(&self) -> String {
        match self.keywords.as_slice() {
            [page, parent, ..] => format!("{page} \u{203a} {parent}"),
            [page] => page.clone(),
            [] => String::new(),
        }
    }

    /// The normalized "haystack" (label + keywords) against which query
    /// terms are tested.
    fn haystack(&self) -> String {
        let mut buf = normalize(&self.label);
        for kw in &self.keywords {
            buf.push(' ');
            buf.push_str(&normalize(kw));
        }
        buf
    }
}

/// Builds the index from the settings actually present in the port,
/// grouped by page. Port of `SettingsSearchIndexer.BuildIndex()`.
pub fn build_index() -> Vec<SettingsSearchEntry> {
    let settings = AppSettings::default();
    // Expand all expanders to also index the settings they contain (the C#
    // `Walk` descends into `expander.Items` unconditionally).
    let expanded = [true; 4];
    let mut index = Vec::new();

    for page in SettingsPage::ALL {
        let page_name = kubuno_drive_desktop_localization::tr(page.display_name_key());
        match page {
            // The Appearance page has no `rows()`: explicit list.
            SettingsPage::Appearance => append_appearance(page, page_name, &mut index),
            _ => {
                let rows =
                    crate::views::settings::page_rows(page.section_index(), &settings, &expanded);
                append_rows(page, page_name, &rows, &mut index);
            }
        }
    }

    index
}

/// Walks the `SettingsRow`s of a generic page (like `Walk`): each
/// card / expander header becomes an entry; a expander's lines carry its
/// header as parent. Section headers (plain `TextBlock`), `InfoBar`s and
/// custom panels are not `SettingsCard`/`SettingsExpander`: they aren't
/// indexed — same as tag rows (a `ListView`, not cards).
fn append_rows(
    page: SettingsPage,
    page_name: &str,
    rows: &[SettingsRow],
    index: &mut Vec<SettingsSearchEntry>,
) {
    let mut parent: Option<String> = None;
    for row in rows {
        match row.kind {
            RowKind::ExpanderHeader { .. } => {
                push_entry(index, page, page_name, &row.label, None);
                parent = Some(row.label.clone());
            }
            RowKind::Card => {
                push_entry(index, page, page_name, &row.label, None);
                parent = None;
            }
            RowKind::ExpanderItem => {
                // Tags (`TagRow`) are ListView items, not settings: don't
                // index them (faithful to the C# `Walk`).
                if !matches!(row.id, SettingId::TagRow(_)) {
                    push_entry(index, page, page_name, &row.label, parent.as_deref());
                }
            }
            // Section TextBlock / InfoBar / panel: nothing to index, and we
            // leave the previous expander's context.
            RowKind::GroupHeader | RowKind::InfoBar | RowKind::Custom => parent = None,
        }
    }
}

/// `AppearancePage.xaml` settings in XAML order (the page draws itself
/// line by line, without `rows()`). Pairs of (label key, optional parent
/// header key).
fn append_appearance(page: SettingsPage, page_name: &str, index: &mut Vec<SettingsSearchEntry>) {
    const ENTRIES: [(&str, Option<&str>); 15] = [
        ("SettingsAppearanceTheme", None),
        ("BackdropMaterial", None),
        ("BackgroundColor", None),
        ("BackgroundImage", None),
        ("Opacity", Some("BackgroundImage")),
        ("ImageFit", Some("BackgroundImage")),
        ("VerticalAlignment", Some("BackgroundImage")),
        ("HorizontalAlignment", Some("BackgroundImage")),
        ("Font", None),
        ("ShowTabActions", None),
        ("AddressBar", None),
        ("ShowStatusCenterButton", Some("AddressBar")),
        ("Toolbar", None),
        ("CustomizeToolbarDescription", Some("Toolbar")),
        ("ShowStatusBar", None),
    ];
    for (label_key, parent_key) in ENTRIES {
        let parent = parent_key.map(kubuno_drive_desktop_localization::tr);
        push_entry(
            index,
            page,
            page_name,
            kubuno_drive_desktop_localization::tr(label_key),
            parent,
        );
    }
}

/// Adds an entry: keywords = page name (+ parent header), like
/// `SettingsSearchResult`'s `Haystack`.
fn push_entry(
    index: &mut Vec<SettingsSearchEntry>,
    page: SettingsPage,
    page_name: &str,
    label: &str,
    parent: Option<&str>,
) {
    let label = label.trim();
    if label.is_empty() {
        return;
    }
    let mut keywords = vec![page_name.to_string()];
    if let Some(parent) = parent {
        keywords.push(parent.to_string());
    }
    index.push(SettingsSearchEntry {
        label: label.to_string(),
        page,
        keywords,
    });
}

/// Filters the index: returns entries whose label or keywords contain ALL
/// the query terms (faithful to the C# `terms.All(...)`), case- AND
/// accent-insensitive.
pub fn search<'a>(index: &'a [SettingsSearchEntry], query: &str) -> Vec<&'a SettingsSearchEntry> {
    let query = normalize(query);
    let terms: Vec<&str> = query.split_whitespace().collect();
    if terms.is_empty() {
        return Vec::new();
    }
    index
        .iter()
        .filter(|entry| {
            let haystack = entry.haystack();
            terms.iter().all(|term| haystack.contains(term))
        })
        .collect()
}

/// Lowercase + folding of common Latin accents (é→e, ç→c, œ→oe…), for a
/// case- and accent-insensitive comparison without an external dependency.
fn normalize(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars().flat_map(char::to_lowercase) {
        match ch {
            'à' | 'â' | 'ä' | 'á' | 'ã' | 'å' => out.push('a'),
            'ç' => out.push('c'),
            'è' | 'é' | 'ê' | 'ë' => out.push('e'),
            'ì' | 'î' | 'ï' | 'í' => out.push('i'),
            'ñ' => out.push('n'),
            'ò' | 'ô' | 'ö' | 'ó' | 'õ' => out.push('o'),
            'ù' | 'û' | 'ü' | 'ú' => out.push('u'),
            'ý' | 'ÿ' => out.push('y'),
            'œ' => out.push_str("oe"),
            'æ' => out.push_str("ae"),
            'ß' => out.push_str("ss"),
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_index_non_vide_et_groupe_par_page() {
        let index = build_index();
        assert!(!index.is_empty(), "l'index ne doit pas être vide");

        // Every indexed page must contribute at least one entry.
        for page in SettingsPage::ALL {
            assert!(
                index.iter().any(|e| e.page == page),
                "aucune entrée pour {page:?}"
            );
        }

        // The first keyword of each entry is its page name.
        for entry in &index {
            let page_name = kubuno_drive_desktop_localization::tr(entry.page.display_name_key());
            assert_eq!(entry.keywords.first().map(String::as_str), Some(page_name));
        }
    }

    #[test]
    fn search_trouve_un_reglage_connu() {
        let index = build_index();

        // Appearance page setting: "theme" (SettingsAppearanceTheme key).
        let hits = search(&index, "theme");
        assert!(
            hits.iter().any(|e| e.page == SettingsPage::Appearance),
            "la recherche « theme » doit atteindre la page Apparence"
        );

        // Case + accent insensitivity: "THÉME" must give the same results.
        let hits_accent = search(&index, "THÉME");
        assert_eq!(hits.len(), hits_accent.len());

        // Folders page setting: thumbnails (ShowThumbnails).
        let thumbs = search(&index, "thumbnails");
        assert!(
            thumbs.iter().any(|e| e.page == SettingsPage::FilesFolders),
            "la recherche « thumbnails » doit atteindre la page Fichiers et dossiers"
        );

        // All terms must be present (AND logic); a nonsense query returns
        // nothing.
        assert!(search(&index, "zzzzz_inexistant").is_empty());

        // Empty query → no results.
        assert!(search(&index, "   ").is_empty());
    }
}
