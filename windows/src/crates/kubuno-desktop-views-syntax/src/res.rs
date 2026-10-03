//! `{Res key}` — the grammar of resource references (`vskubuno/docs/RESOURCES.md`, `VIEWS-SPEC.md`
//! §6.2): `{Res key}`, `{Res key, Source=set}`, `{Res Key=key, Set=set}`.
//!
//! A reference is carried like a one-way binding whose path starts with [`RES_PREFIX`] — a path no
//! view model can hold. Resolving it (the `.kbres` lookup, the UI culture) is the runtime's
//! (`kubuno_desktop_views::resources`, which re-exports this module's items).

/// The path prefix of a resource reference's binding.
pub const RES_PREFIX: &str = "@res:";

/// Parses the inside of `{Res key[, Source=set]}` (without the braces) into its binding path
/// (`@res:set/key`, `@res:key`); `None` when it is not a `Res` expression. `Key=` may name the key
/// explicitly; `Source=` is the set (the `.kbres` file stem).
pub fn parse_res_path(inner: &str) -> Option<String> {
    let rest = inner.trim().strip_prefix("Res")?;
    if let Some(c) = rest.chars().next() {
        if !c.is_whitespace() && c != ',' {
            return None;
        }
    }
    let mut key: Option<String> = None;
    let mut set: Option<String> = None;
    for (i, part) in rest.split(',').enumerate() {
        let part = part.trim();
        match part.split_once('=').map(|(k, v)| (k.trim(), v.trim())) {
            Some(("Key" | "Path" | "ResourceKey", v)) if !v.is_empty() => key = Some(v.to_string()),
            Some(("Source" | "Set" | "File", v)) if !v.is_empty() => set = Some(v.trim_end_matches(".kbres").to_string()),
            Some(_) => {}
            None if i == 0 && !part.is_empty() => key = Some(part.to_string()),
            None => {}
        }
    }
    let key = key?;
    Some(match set {
        Some(set) => format!("{RES_PREFIX}{set}/{key}"),
        None => format!("{RES_PREFIX}{key}"),
    })
}

/// `(set, key)` of a resource reference's binding path; `None` for an ordinary binding path.
pub fn res_reference(path: &str) -> Option<(Option<&str>, &str)> {
    let rest = path.strip_prefix(RES_PREFIX)?;
    Some(match rest.split_once('/') {
        Some((set, key)) => (Some(set), key),
        None => (None, rest),
    })
}

/// Whether `raw` (an attribute value as written) is a `{Res …}` expression.
pub fn is_res_expr(raw: &str) -> bool {
    raw.trim().strip_prefix('{').and_then(|r| r.strip_suffix('}')).and_then(parse_res_path).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_key_and_the_set() {
        assert_eq!(parse_res_path("Res title").as_deref(), Some("@res:title"));
        assert_eq!(parse_res_path("Res title, Source=strings.kbres").as_deref(), Some("@res:strings/title"));
        assert_eq!(parse_res_path("Res Key=save, Set=icons").as_deref(), Some("@res:icons/save"));
        assert_eq!(parse_res_path("Resource title"), None);
        assert_eq!(parse_res_path("Res"), None);
        assert_eq!(res_reference("@res:strings/title"), Some((Some("strings"), "title")));
        assert_eq!(res_reference("@res:title"), Some((None, "title")));
        assert_eq!(res_reference("Title"), None);
        assert!(is_res_expr(" {Res title} "));
        assert!(!is_res_expr("{Binding title}"));
    }
}
