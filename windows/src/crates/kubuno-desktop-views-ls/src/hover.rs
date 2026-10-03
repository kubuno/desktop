//! `textDocument/hover` — component doc on an element name, property/event/
//! common-attribute doc on an attribute name (`XML_VIEWS.md` §4: "a doc
//! string lifted from the existing `///` doc comment" is exactly what
//! [`kubuno_desktop_views::registry::PropertyMeta::doc`]/[`kubuno_desktop_views::registry::
//! EventMeta::doc`] carry — this module is that registry's first consumer
//! outside `kubuno-desktop-views` itself, per §4's own framing: "the language server
//! reads it for hover and completion").

use kubuno_desktop_views::ast::{AstNode, Attribute};
use kubuno_desktop_views::registry;
use kubuno_desktop_views::syntax::SyntaxKind;
use lsp_types::{Hover, HoverContents, MarkupContent, MarkupKind, Position, Range};

use crate::common_attrs;
use crate::documents::Document;
use crate::tree;

pub fn hover(doc: &Document, pos: Position) -> Option<Hover> {
    let offset = doc.position_index.position_to_offset(&doc.text, pos);
    let root = doc.parse.syntax();
    let token = tree::token_at_offset(&root, offset)?;

    let (markdown, range) = if tree::is_start_tag_name(&token) {
        let name = token.text();
        let meta = registry::lookup(name)?;
        let mut markdown = format!("**`<{}>`**\n\n{}", meta.name, meta.doc);
        // A class of the project (EVT-7b): where it comes from.
        if let Some(info) = registry::project_info(name) {
            let what = match info.kind {
                registry::ClassKind::Control => "Control",
                registry::ClassKind::UserControl => "User control",
                registry::ClassKind::Component => "Component",
            };
            let krate = info.crate_name.map(|c| format!(" of `{c}`")).unwrap_or_default();
            markdown.push_str(&format!("\n\n*{what}{krate}, extends `{}`*", info.extends));
        }
        (markdown, token.text_range())
    } else if tree::is_attribute_name(&token) {
        let attr_name = token.text().to_string();
        let attribute_node = token.parent()?; // ATTRIBUTE
        let element = tree::enclosing_element(&attribute_node)?;
        let element_name = element.name()?;

        let meta = registry::lookup(&element_name);
        let is_root = element.syntax().parent().is_none_or(|p| p.kind() != SyntaxKind::ELEMENT);
        // The element's own or inherited property, or on the root element a view property.
        let registered = meta
            .and_then(|m| m.own_property(&attr_name).map(|p| (None, p)).or_else(|| m.inherited_property(&attr_name).map(|(l, p)| (Some(l), p))))
            .or_else(|| is_root.then(|| kubuno_desktop_views::registry::view_property(&attr_name).map(|p| (Some("View"), p))).flatten());
        if attr_name == "x:Name" {
            (format!("**`x:Name`**\n\n{}", common_attrs::X_NAME_DOC), token.text_range())
        } else if let Some((level, prop)) = registered {
            let from = level.map(|l| format!(" — inherited from `{l}`")).unwrap_or_default();
            let alias = if prop.name == attr_name { String::new() } else { format!("\n\n*Older name for `{}`.*", prop.name) };
            (
                format!("**`{attr_name}`** on `<{element_name}>`\n\n{}\n\n*Default: `{}`*{from}{alias}", prop.doc, prop.default),
                token.text_range(),
            )
        } else if let Some(common) = common_attrs::lookup(&attr_name) {
            (format!("**`{attr_name}`** (common attribute)\n\n{}", common.doc), token.text_range())
        } else {
            // An event, or an unknown attribute (`diagnostics` already flags it; no doc to show).
            let event = meta?.event(&attr_name).or_else(|| kubuno_desktop_views::registry::view_event(&attr_name))?;
            let alias = if event.name == attr_name { String::new() } else { format!("\n\n*Older name for `{}`.*", event.name) };
            (
                format!(
                    "**`{attr_name}`** event on `<{element_name}>`\n\n{}\n\n*{} — `{}`*{alias}",
                    event.doc,
                    event.category.name(),
                    event.args_type
                ),
                token.text_range(),
            )
        }
    } else if token.kind() == SyntaxKind::STRING {
        // Hovering an enum value: show the same property's doc plus the full
        // set of valid values, so the tooltip doubles as a reminder of what
        // else is valid here.
        let attribute_node = token.parent()?; // ATTRIBUTE
        let attr = Attribute::cast(attribute_node.clone())?;
        let attr_name = attr.name()?;
        let element = tree::enclosing_element(&attribute_node)?;
        let element_name = element.name()?;
        let prop = registry::lookup(&element_name)
            .and_then(|meta| meta.property(&attr_name))
            .or_else(|| kubuno_desktop_views::registry::view_property(&attr_name))?;
        if let registry::PropKind::Enum(variants) = prop.kind {
            (
                format!("**`{attr_name}`** on `<{element_name}>`\n\n{}\n\nValid values: {}", prop.doc, variants.join(", ")),
                token.text_range(),
            )
        } else {
            return None;
        }
    } else {
        return None;
    };

    Some(Hover {
        contents: HoverContents::Markup(MarkupContent { kind: MarkupKind::Markdown, value: markdown }),
        range: Some(Range {
            start: doc.position_index.offset_to_position(&doc.text, range.start()),
            end: doc.position_index.offset_to_position(&doc.text, range.end()),
        }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use kubuno_desktop_views::syntax::parse;

    fn open(text: &str) -> Document {
        Document {
            position_index: crate::position::PositionIndex::new(text),
            parse: parse(text),
            text: text.to_string(),
            version: 1,
        }
    }

    fn markdown(h: &Hover) -> String {
        match &h.contents {
            HoverContents::Markup(m) => m.value.clone(),
            _ => panic!("expected markup contents"),
        }
    }

    #[test]
    fn hovering_the_element_name_shows_the_component_doc() {
        let d = open(r#"<Button Text="Ok"/>"#);
        let h = hover(&d, Position { line: 0, character: 3 }).expect("hover over 'Button'");
        assert!(markdown(&h).contains("push button"));
    }

    #[test]
    fn hovering_a_property_name_shows_its_doc_and_default() {
        let d = open(r#"<Button Text="Ok"/>"#);
        let h = hover(&d, Position { line: 0, character: 9 }).expect("hover over 'Text'");
        let md = markdown(&h);
        assert!(md.contains("Text displayed on the button"));
        assert!(md.contains("Default:"));
    }

    #[test]
    fn hovering_an_event_name_shows_its_doc() {
        let d = open(r#"<Button OnClick="go"/>"#);
        let h = hover(&d, Position { line: 0, character: 10 }).expect("hover over 'OnClick'");
        assert!(markdown(&h).contains("activated"));
    }

    #[test]
    fn hovering_an_inherited_property_shows_its_doc_and_level() {
        let d = open(r#"<Button Dock="Left"/>"#);
        let h = hover(&d, Position { line: 0, character: 9 }).expect("hover over 'Dock'");
        assert!(markdown(&h).contains("inherited from `Control`"), "{}", markdown(&h));
    }

    #[test]
    fn hovering_a_view_property_on_the_root_shows_its_doc() {
        let d = open(r#"<Panel Title="Demo"/>"#);
        let h = hover(&d, Position { line: 0, character: 8 }).expect("hover over 'Title'");
        assert!(markdown(&h).contains("inherited from `View`"), "{}", markdown(&h));
    }

    #[test]
    fn hovering_x_name_shows_its_doc() {
        let d = open(r#"<Button x:Name="go"/>"#);
        let h = hover(&d, Position { line: 0, character: 10 }).expect("hover over 'x:Name'");
        assert!(markdown(&h).contains(common_attrs::X_NAME_DOC));
    }

    #[test]
    fn hovering_an_enum_value_lists_the_valid_values() {
        let d = open(r#"<Button Variant="Primary"/>"#);
        let h = hover(&d, Position { line: 0, character: 20 }).expect("hover over the value");
        assert!(markdown(&h).contains("Secondary"));
    }

    #[test]
    fn hovering_unknown_element_returns_none() {
        let d = open("<Frobnicator/>");
        assert!(hover(&d, Position { line: 0, character: 3 }).is_none());
    }

    #[test]
    fn hovering_whitespace_returns_none() {
        let d = open(r#"<Button  Text="Ok"/>"#);
        assert!(hover(&d, Position { line: 0, character: 8 }).is_none());
    }
}
