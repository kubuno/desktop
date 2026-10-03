//! Element ids: the stable path of an element in its view (`"2.0.3"` = the fourth child of the first
//! child of the third child of the root; `""` = the root element itself). An element's id is
//! [`crate::ast::Element::stable_id`], resolved back with [`crate::ast::Document::resolve_id`]; the
//! designer, the language server's `kubuno/applyEdit` bridge and the runtime's event router all speak
//! in these ids. The pure id arithmetic lives here (`kubuno_desktop_views::design` re-exports it).

/// The parent half of `id`: `"2.0.3"` → `Some("2.0")`, `"3"` → `Some("")` (the root), `""` (the
/// root itself) → `None`.
pub fn parent_id_of(id: &str) -> Option<String> {
    if id.is_empty() {
        return None;
    }
    match id.rsplit_once('.') {
        Some((parent, _)) => Some(parent.to_string()),
        None => Some(String::new()),
    }
}

/// Whether `ancestor` is a strict ancestor of `id` in the stable-id scheme (`""` is everyone's).
pub fn is_ancestor_id(ancestor: &str, id: &str) -> bool {
    if ancestor == id {
        return false;
    }
    ancestor.is_empty() || id.starts_with(&format!("{ancestor}."))
}

/// The selected elements a group gesture (move, nudge, delete, format) applies to: the root dropped,
/// and every element one of whose ancestors is also selected dropped too (it follows its container),
/// in selection order - WinForms' own rule for a multi-selection spanning a container and its children.
pub fn top_level_ids(ids: &[String]) -> Vec<String> {
    ids.iter()
        .filter(|id| !id.is_empty())
        .filter(|id| !ids.iter().any(|other| is_ancestor_id(other, id) && !other.is_empty()))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn id_arithmetic() {
        assert_eq!(parent_id_of("2.0.3").as_deref(), Some("2.0"));
        assert_eq!(parent_id_of("3").as_deref(), Some(""));
        assert_eq!(parent_id_of(""), None);
        assert!(is_ancestor_id("", "1"));
        assert!(is_ancestor_id("1", "1.2"));
        assert!(!is_ancestor_id("1", "10.2"));
        assert!(!is_ancestor_id("1", "1"));
        let ids: Vec<String> = ["1.2", "1", "", "3.0"].iter().map(|s| s.to_string()).collect();
        assert_eq!(top_level_ids(&ids), vec!["1".to_string(), "3.0".to_string()]);
    }
}
