//! The XML namespace declarations of a view (`VIEWS-SPEC.md` §3): `xmlns` (the components),
//! `xmlns:x` (the view's directives, `x:Name`…) and `xmlns:d` (the design-time attributes, `d:Text`…),
//! written on the root element like XAML's.
//!
//! Kubuno's own tools never need them: the `x:` and `d:` prefixes are fixed, and a view without any
//! declaration parses, validates and compiles exactly like one with them. They are **recommended**
//! because any generic XML tool (Visual Studio's XML editor, an XSD validator, a browser) treats an
//! undeclared prefix as a well-formedness error. The tools emit them (templates, Add New Item, the
//! designer's new views) and the language server offers to add the missing ones ([`missing_declarations`]).

use crate::ast::{AstNode, Document, Element};
use crate::syntax::Parse;
use rowan::{TextRange, TextSize};

/// The default namespace of a view: its elements (the components) and their properties.
pub const VIEWS_NAMESPACE: &str = "https://kubuno.com/views";

/// The namespace of the `x:` directives (`x:Name`, `x:Inherits`, `x:Props`…).
pub const DIRECTIVE_NAMESPACE: &str = "https://kubuno.com/views/x";

/// The namespace of the `d:` design-time attributes (`d:Text`, `d:Visible`…).
pub const DESIGN_NAMESPACE: &str = "https://kubuno.com/views/design";

/// The prefix of the directives, without its colon.
pub const DIRECTIVE_PREFIX: &str = "x";

/// The prefix of the design-time attributes, without its colon.
pub const DESIGN_PREFIX: &str = "d";

/// Whether `attribute` declares a namespace (`xmlns`, `xmlns:x`…): markup, never a property.
pub fn is_declaration(attribute: &str) -> bool {
    attribute == "xmlns" || attribute.starts_with("xmlns:")
}

/// The attribute that declares `prefix` (`""` = the default namespace): `xmlns` or `xmlns:<prefix>`.
pub fn declaration_attribute(prefix: &str) -> String {
    if prefix.is_empty() {
        "xmlns".to_string()
    } else {
        format!("xmlns:{prefix}")
    }
}

/// The standard namespace of a prefix Kubuno knows (`""`, `x`, `d`).
pub fn standard_namespace(prefix: &str) -> Option<&'static str> {
    match prefix {
        "" => Some(VIEWS_NAMESPACE),
        DIRECTIVE_PREFIX => Some(DIRECTIVE_NAMESPACE),
        DESIGN_PREFIX => Some(DESIGN_NAMESPACE),
        _ => None,
    }
}

/// The declarations a new view's root element carries, each preceded by a space:
/// ` xmlns="…" xmlns:x="…"`, plus ` xmlns:d="…"` when `design` (the view uses `d:` attributes).
pub fn standard_declarations(design: bool) -> String {
    let mut out = format!(" xmlns=\"{VIEWS_NAMESPACE}\" xmlns:x=\"{DIRECTIVE_NAMESPACE}\"");
    if design {
        out.push_str(&format!(" xmlns:d=\"{DESIGN_NAMESPACE}\""));
    }
    out
}

/// A prefix the view uses without declaring it on its root element.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UndeclaredPrefix {
    /// `x` or `d`.
    pub prefix: &'static str,
    /// The first use: the name of the first attribute written with that prefix.
    pub first_use: TextRange,
}

/// The `x:` / `d:` prefixes the view uses (on any element) that its root element does not declare,
/// in that order. Empty for a view that declares them, uses none, or has no root element.
pub fn undeclared_prefixes(parse: &Parse) -> Vec<UndeclaredPrefix> {
    let Some(root) = Document::cast(parse.syntax()).and_then(|d| d.root_element()) else { return Vec::new() };
    let mut out = Vec::new();
    for prefix in [DIRECTIVE_PREFIX, DESIGN_PREFIX] {
        if is_declared(&root, prefix) {
            continue;
        }
        let first_use = std::iter::once(root.clone())
            .chain(root.syntax().descendants().skip(1).filter_map(Element::cast))
            .flat_map(|e| e.attributes().collect::<Vec<_>>())
            .find(|a| a.name().is_some_and(|n| n.strip_prefix(prefix).is_some_and(|rest| rest.starts_with(':'))))
            .and_then(|a| a.name_range());
        if let Some(first_use) = first_use {
            out.push(UndeclaredPrefix { prefix, first_use });
        }
    }
    out
}

/// Whether `root` declares `prefix` (`""` = the default namespace), whatever its namespace.
fn is_declared(root: &Element, prefix: &str) -> bool {
    root.attribute(&declaration_attribute(prefix)).is_some()
}

/// The edit that adds the missing declarations to the root element: the default namespace when it
/// has none, then those of the prefixes [`undeclared_prefixes`] reports. The text (each declaration
/// preceded by a space) goes right after the root element's name, on its line, so no other line of
/// the file moves. `None` when nothing is missing — nothing is ever added to a view that uses no
/// prefix at all (no false positive) unless `always` asks for the standard set (a migration).
pub fn missing_declarations(parse: &Parse, always: bool) -> Option<(TextSize, String)> {
    let root = Document::cast(parse.syntax()).and_then(|d| d.root_element())?;
    let at = root.name_range()?.end();
    let undeclared = undeclared_prefixes(parse);
    if undeclared.is_empty() && !always {
        return None;
    }
    let mut text = String::new();
    let mut add = |prefix: &str| {
        if !is_declared(&root, prefix) && !text.contains(&format!("{}=", declaration_attribute(prefix))) {
            if let Some(ns) = standard_namespace(prefix) {
                text.push_str(&format!(" {}=\"{ns}\"", declaration_attribute(prefix)));
            }
        }
    };
    add("");
    if always {
        add(DIRECTIVE_PREFIX);
    }
    for u in &undeclared {
        add(u.prefix);
    }
    (!text.is_empty()).then_some((at, text))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::syntax::parse;

    fn apply(src: &str, always: bool) -> String {
        let p = parse(src);
        match missing_declarations(&p, always) {
            Some((at, text)) => {
                let at = usize::from(at);
                format!("{}{}{}", &src[..at], text, &src[at..])
            }
            None => src.to_string(),
        }
    }

    #[test]
    fn declarations_are_markup_attributes() {
        assert!(is_declaration("xmlns"));
        assert!(is_declaration("xmlns:x"));
        assert!(!is_declaration("xmlnsFoo"));
        assert!(!is_declaration("Text"));
    }

    #[test]
    fn undeclared_x_and_d_are_reported_at_their_first_use() {
        let src = "<Panel>\n  <Button x:Name=\"ok\" d:Text=\"Ok\"/>\n  <Label x:Name=\"l\"/>\n</Panel>";
        let u = undeclared_prefixes(&parse(src));
        assert_eq!(u.iter().map(|u| u.prefix).collect::<Vec<_>>(), vec!["x", "d"]);
        assert_eq!(&src[u[0].first_use], "x:Name");
        assert_eq!(&src[u[1].first_use], "d:Text");
    }

    #[test]
    fn a_declared_prefix_is_not_reported_whatever_its_uri() {
        let src = r#"<Panel xmlns:x="urn:anything"><Button x:Name="ok"/></Panel>"#;
        assert!(undeclared_prefixes(&parse(src)).is_empty());
    }

    #[test]
    fn a_view_without_prefixes_gets_nothing_unless_asked() {
        assert_eq!(apply("<Panel><Button/></Panel>", false), "<Panel><Button/></Panel>");
        assert_eq!(
            apply("<Panel><Button/></Panel>", true),
            format!("<Panel xmlns=\"{VIEWS_NAMESPACE}\" xmlns:x=\"{DIRECTIVE_NAMESPACE}\"><Button/></Panel>")
        );
    }

    #[test]
    fn the_fix_adds_only_what_is_missing_on_the_root_line() {
        let src = "<!-- c -->\r\n<Panel Title=\"T\">\r\n  <Button x:Name=\"ok\" d:Text=\"Ok\"/>\r\n</Panel>\r\n";
        let out = apply(src, false);
        assert_eq!(
            out,
            format!("<!-- c -->\r\n<Panel xmlns=\"{VIEWS_NAMESPACE}\" xmlns:x=\"{DIRECTIVE_NAMESPACE}\" xmlns:d=\"{DESIGN_NAMESPACE}\" Title=\"T\">\r\n  <Button x:Name=\"ok\" d:Text=\"Ok\"/>\r\n</Panel>\r\n")
        );
        // Applying it again changes nothing (idempotent), and a declared default is kept.
        assert_eq!(apply(&out, false), out);
        assert_eq!(apply(&out, true), out);
        let partial = r#"<Panel xmlns="urn:mine"><Button x:Name="ok"/></Panel>"#;
        assert_eq!(apply(partial, false), format!(r#"<Panel xmlns:x="{DIRECTIVE_NAMESPACE}" xmlns="urn:mine"><Button x:Name="ok"/></Panel>"#));
    }

    #[test]
    fn standard_declarations_text() {
        assert_eq!(standard_declarations(false), format!(" xmlns=\"{VIEWS_NAMESPACE}\" xmlns:x=\"{DIRECTIVE_NAMESPACE}\""));
        assert!(standard_declarations(true).ends_with(&format!(" xmlns:d=\"{DESIGN_NAMESPACE}\"")));
    }
}
