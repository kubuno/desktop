//! The registry-independent core of the validator (`XML_VIEWS.md` §5): literal value checks against
//! a [`PropKind`], the attributes every element accepts, typo suggestions and the shape tests both
//! targets share. The registry walk itself (unknown elements and attributes, children models,
//! formats of colours and fonts…) is `kubuno_desktop_views::validate`, which builds on these.

use kubuno_desktop_views_model::PropKind;

use crate::ast::{AstNode, Element};

/// Attributes every element accepts regardless of its registry entry: the attached Dock/Anchor
/// layout properties any child of a `<Panel>` carries, and the absolute position and size. They are
/// not modelled per component because they are read by the *parent* panel/layout engine, the same
/// way WinForms' `Dock`/`Anchor` are read off `Control.Dock` regardless of the control's own type.
/// (`x:Name` and the other markup attributes are [`crate::markup`]'s.)
pub const COMMON_ATTRIBUTES: &[(&str, PropKind)] = &[
    ("Dock", PropKind::Enum(&["None", "Top", "Bottom", "Left", "Right", "Fill"])),
    ("Anchor", PropKind::String), // A comma-combination (`"Top,Left,Right"`) — not a single closed enum.
    ("X", PropKind::F32),
    ("Y", PropKind::F32),
    ("Width", PropKind::F32),
    ("Height", PropKind::F32),
];

/// Checks a literal attribute value against its kind: `true`/`false`, a number, one of an enum's
/// values (case-sensitive), any string. The message is what the diagnostic says after the attribute.
pub fn check_value(kind: &PropKind, value: &str) -> Result<(), String> {
    match kind {
        PropKind::Bool => {
            if value == "true" || value == "false" {
                Ok(())
            } else {
                Err(format!("expected `true` or `false`, found `{value}`"))
            }
        }
        PropKind::F32 => value.parse::<f32>().map(|_| ()).map_err(|_| format!("expected a number, found `{value}`")),
        PropKind::String => Ok(()),
        PropKind::Enum(variants) => {
            if variants.contains(&value) {
                Ok(())
            } else {
                Err(format!("`{value}` is not valid here; expected one of: {}", variants.join(", ")))
            }
        }
    }
}

/// Whether an attribute value is a markup expression (`{Binding …}`, `{Res …}`) rather than a
/// literal: resolved at run time, so never checked against its kind.
pub fn is_binding_expression(value: &str) -> bool {
    value.starts_with('{') && value.ends_with('}')
}

/// Whether `element` is the view's root element (its parent is not an element).
pub fn is_root_element(element: &Element) -> bool {
    element.syntax().parent().is_none_or(|p| p.kind() != crate::syntax::SyntaxKind::ELEMENT)
}

/// The edit distance between `a` and `b`, ignoring case (a difference of case alone costs nothing).
pub fn distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.to_lowercase().chars().collect();
    let b: Vec<char> = b.to_lowercase().chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut previous = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let substitution = previous + usize::from(ca != cb);
            previous = row[j + 1];
            row[j + 1] = substitution.min(row[j] + 1).min(row[j + 1] + 1);
        }
    }
    row[b.len()]
}

/// The candidate closest to `name` when it is close enough to be a typo of it (at most a third of its
/// letters off, and at least one); ties go to the first candidate.
pub fn closest<'c>(name: &str, candidates: impl Iterator<Item = &'c str>) -> Option<&'c str> {
    let limit = (name.chars().count() / 3).max(1);
    candidates.filter(|c| *c != name).map(|c| (distance(name, c), c)).filter(|(d, _)| *d <= limit).min_by_key(|(d, _)| *d).map(|(_, c)| c)
}

/// `message` with its suggestion: `…; did you mean `X`?`.
pub fn with_suggestion(message: String, suggestion: Option<String>) -> String {
    match suggestion {
        Some(s) => format!("{message}; did you mean `{s}`?"),
        None => message,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checks_literals_against_their_kind() {
        assert!(check_value(&PropKind::Bool, "true").is_ok());
        assert_eq!(check_value(&PropKind::Bool, "yes"), Err("expected `true` or `false`, found `yes`".to_string()));
        assert!(check_value(&PropKind::F32, "12.5").is_ok());
        assert!(check_value(&PropKind::F32, "wide").is_err());
        assert!(check_value(&PropKind::Enum(&["Top", "Fill"]), "Fill").is_ok());
        assert!(check_value(&PropKind::Enum(&["Top", "Fill"]), "fill").is_err());
        assert!(check_value(&PropKind::String, "").is_ok());
        assert!(is_binding_expression("{Binding Name}"));
        assert!(!is_binding_expression("Name"));
    }

    #[test]
    fn suggests_close_names_only() {
        assert_eq!(distance("Buton", "button"), 1);
        assert_eq!(closest("Buton", ["Button", "Label"].into_iter()), Some("Button"));
        assert_eq!(closest("Xyz", ["Button", "Label"].into_iter()), None);
        assert_eq!(with_suggestion("unknown".into(), Some("Button".into())), "unknown; did you mean `Button`?");
    }

    #[test]
    fn finds_the_root_element() {
        let parse = crate::syntax::parse("<Panel><Button/></Panel>");
        let root = crate::ast::Document::cast(parse.syntax()).and_then(|d| d.root_element()).expect("root");
        assert!(is_root_element(&root));
        let child = root.children().next().expect("child");
        assert!(!is_root_element(&child));
    }
}
