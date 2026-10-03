//! Cultures: the culture part of a satellite file name (`resources.fr.kbres`, `resources.de-DE.kbres`),
//! and the fallback chain used to pick a value (.NET's `ResourceManager` order: specific culture →
//! its neutral parent → the invariant file, with one Kubuno addition, see [`fallback_chain`]).

/// Whether `tag` looks like a BCP-47 culture name: a 2-3 letter language, then optional subtags of
/// 1-8 letters/digits (`fr`, `fr-FR`, `zh-Hans`, `zh-Hans-CN`, `es-419`, `sr-Latn-RS`).
pub fn is_culture(tag: &str) -> bool {
    let mut parts = tag.split('-');
    let Some(lang) = parts.next() else { return false };
    if !(2..=3).contains(&lang.len()) || !lang.chars().all(|c| c.is_ascii_alphabetic()) {
        return false;
    }
    parts.all(|p| (1..=8).contains(&p.len()) && p.chars().all(|c| c.is_ascii_alphanumeric()))
}

/// The canonical spelling of a culture name: language lower case, a 4-letter script title case, a
/// 2-letter region upper case (`FR-fr` → `fr-FR`, `zh-hans` → `zh-Hans`). `_` is accepted for `-`.
pub fn canonical(tag: &str) -> String {
    let tag = tag.trim().replace('_', "-");
    let mut out = Vec::new();
    for (i, part) in tag.split('-').enumerate() {
        out.push(if i == 0 {
            part.to_ascii_lowercase()
        } else if part.len() == 4 && part.chars().all(|c| c.is_ascii_alphabetic()) {
            let mut s = part.to_ascii_lowercase();
            s[..1].make_ascii_uppercase();
            s
        } else if part.len() == 2 && part.chars().all(|c| c.is_ascii_alphabetic()) {
            part.to_ascii_uppercase()
        } else {
            part.to_string()
        });
    }
    out.join("-")
}

/// The parent of a culture (`zh-Hans-CN` → `zh-Hans`, `fr-FR` → `fr`, `fr` → none).
pub fn parent(tag: &str) -> Option<&str> {
    tag.rfind('-').map(|at| &tag[..at])
}

/// `(stem, culture)` of a resource file name: `resources.kbres` → `("resources", None)`,
/// `resources.fr-FR.kbres` → `("resources", Some("fr-FR"))`. A middle part that is not a culture
/// name stays in the stem (`app.icons.kbres` → `("app.icons", None)`). `None` for another extension.
pub fn split_file_name(file_name: &str) -> Option<(String, Option<String>)> {
    let base = file_name.strip_suffix(".kbres").or_else(|| file_name.strip_suffix(".KBRES"))?;
    if let Some(at) = base.rfind('.') {
        let (stem, culture) = (&base[..at], &base[at + 1..]);
        if !stem.is_empty() && is_culture(culture) {
            return Some((stem.to_string(), Some(canonical(culture))));
        }
    }
    Some((base.to_string(), None))
}

/// The cultures to try, in order, for `culture` among the `available` satellite cultures, before the
/// invariant (neutral) file:
///
/// 1. the culture itself and its parents (`fr-CA` → `fr-CA`, `fr`), like .NET;
/// 2. then — a Kubuno addition for sets that only ship specific cultures (drive's `fr-FR` but no
///    `fr`) — the first available culture of the same language (`fr-CA` → `fr-FR`).
///
/// Comparisons ignore case; the returned names are those of `available`.
pub fn fallback_chain<'a>(culture: &str, available: &[&'a str]) -> Vec<&'a str> {
    let mut chain: Vec<&'a str> = Vec::new();
    let wanted = canonical(culture);
    if wanted.is_empty() || wanted.eq_ignore_ascii_case("invariant") {
        return chain;
    }
    let mut cur: Option<&str> = Some(&wanted);
    while let Some(c) = cur {
        if let Some(found) = available.iter().find(|a| a.eq_ignore_ascii_case(c)) {
            if !chain.contains(found) {
                chain.push(found);
            }
        }
        cur = parent(c);
    }
    let lang = wanted.split('-').next().unwrap_or_default();
    let mut siblings: Vec<&'a str> = available.iter().copied().filter(|a| a.split('-').next().is_some_and(|l| l.eq_ignore_ascii_case(lang)) && !chain.contains(a)).collect();
    siblings.sort_unstable();
    chain.extend(siblings);
    chain
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_culture_names() {
        for ok in ["fr", "fr-FR", "zh-Hans", "zh-Hans-CN", "es-419", "sr-Latn-RS", "fil-PH"] {
            assert!(is_culture(ok), "{ok}");
        }
        for bad in ["", "f", "icons", "fr_FR", "fr-", "123", "francais"] {
            assert!(!is_culture(bad), "{bad}");
        }
        assert_eq!(canonical("FR-fr"), "fr-FR");
        assert_eq!(canonical("zh_hans_cn"), "zh-Hans-CN");
    }

    #[test]
    fn splits_file_names() {
        assert_eq!(split_file_name("resources.kbres"), Some(("resources".into(), None)));
        assert_eq!(split_file_name("resources.fr.kbres"), Some(("resources".into(), Some("fr".into()))));
        assert_eq!(split_file_name("resources.de-de.kbres"), Some(("resources".into(), Some("de-DE".into()))));
        assert_eq!(split_file_name("app.icons.kbres"), Some(("app.icons".into(), None)));
        assert_eq!(split_file_name("x.resx"), None);
    }

    #[test]
    fn falls_back_specific_then_neutral_then_sibling() {
        let available = ["de", "fr", "fr-FR", "fr-BE", "zh-Hans"];
        assert_eq!(fallback_chain("fr-FR", &available), vec!["fr-FR", "fr", "fr-BE"]);
        assert_eq!(fallback_chain("fr-CA", &available), vec!["fr", "fr-BE", "fr-FR"]);
        assert_eq!(fallback_chain("de-AT", &available), vec!["de"]);
        assert_eq!(fallback_chain("zh-Hans-CN", &available), vec!["zh-Hans"]);
        assert!(fallback_chain("ja-JP", &available).is_empty());
        assert!(fallback_chain("", &available).is_empty());
        // Only specific cultures (drive): a neutral request finds a sibling.
        assert_eq!(fallback_chain("fr", &["fr-FR", "en-US"]), vec!["fr-FR"]);
    }
}
