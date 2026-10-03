//! `textDocument/completion` — four contexts, resolved purely from what node
//! the cursor's token belongs to in the `kubuno_desktop_views::syntax` tree (no
//! separate re-lexing of the prefix: the error-tolerant parser already
//! produces a sensible tree for a file mid-edit, per `syntax::parser`'s own
//! recovery strategy):
//!
//! 1. **Element name** — cursor right after `<` (a brand new child), or
//!    inside an element name being typed — offers every registered
//!    component name, gated by whether the *containing* element's
//!    [`kubuno_desktop_views::registry::ChildrenModel`] still accepts a(nother) child
//!    at all.
//! 2. **Attribute name** — cursor in the gap between the tag name/a previous
//!    attribute and the tag's closer, or mid-typing an attribute name —
//!    offers the element's registered properties/events plus the
//!    always-available [`crate::common_attrs`] and `x:Name`.
//! 3. **Attribute value** — cursor inside a quoted value whose attribute is
//!    [`kubuno_desktop_views::registry::PropKind::Enum`] — offers the closed set of
//!    valid values.
//! 4. **Closing tag** — cursor right after `</` (with or without a partial
//!    name already typed) — offers the one name that actually closes the
//!    innermost open element.

use kubuno_desktop_views::ast::{AstNode, Attribute, Element};
use kubuno_desktop_views::registry::{self, ChildrenModel, ComponentMeta, PropKind};
use kubuno_desktop_views::syntax::{SyntaxKind, SyntaxNode};
use lsp_types::{CompletionItem, CompletionItemKind, Documentation, MarkupContent, MarkupKind, Position};

use crate::common_attrs;
use crate::documents::Document;
use crate::tree;

pub fn completion(doc: &Document, pos: Position) -> Vec<CompletionItem> {
    let offset = doc.position_index.position_to_offset(&doc.text, pos);
    let root = doc.parse.syntax();
    let Some(token) = tree::token_at_offset(&root, offset) else { return Vec::new() };

    // 4. Closing tag: `</` already produces an END_TAG node even before any
    // name follows (see `syntax::parser::parse_end_tag`) — the L_ANGLE_SLASH
    // token itself, or a partially typed IDENT, both have an END_TAG parent.
    if token.kind() == SyntaxKind::L_ANGLE_SLASH || (token.kind() == SyntaxKind::IDENT && parent_kind(&token) == Some(SyntaxKind::END_TAG)) {
        if let Some(end_tag) = token.parent() {
            if let Some(element_node) = end_tag.parent() {
                if let Some(element) = Element::cast(element_node) {
                    if let Some(name) = element.name() {
                        return vec![closing_tag_item(&name)];
                    }
                }
            }
        }
        return Vec::new();
    }

    // 3. Attribute value (enum): cursor inside a STRING token belonging to
    // an ATTRIBUTE, strictly between the quotes.
    if token.kind() == SyntaxKind::STRING && parent_kind(&token) == Some(SyntaxKind::ATTRIBUTE) {
        if is_strictly_inside_quotes(&token, offset) {
            return enum_value_items(&token);
        }
        return Vec::new();
    }

    // 2. Attribute name: mid-typing an attribute's own name, or sitting in
    // the whitespace gap of a start tag (after the tag name or a previous
    // attribute, before the closer).
    if tree::is_attribute_name(&token) {
        if let Some(attribute_node) = token.parent() {
            if let Some(element) = tree::enclosing_element(&attribute_node) {
                return attribute_items(&element);
            }
        }
        return Vec::new();
    }
    if token.kind() == SyntaxKind::WHITESPACE && parent_kind(&token) == Some(SyntaxKind::START_TAG) {
        if let Some(start_tag) = token.parent() {
            if let Some(element) = tree::enclosing_element(&start_tag) {
                return attribute_items(&element);
            }
        }
        return Vec::new();
    }

    // 1. Element name: right after `<` (new child), or mid-typing the tag
    // name itself.
    if token.kind() == SyntaxKind::L_ANGLE || tree::is_start_tag_name(&token) {
        // `L_ANGLE`'s parent is the new element's own START_TAG; the tag-name
        // IDENT's parent is that same START_TAG — either way, one `.parent()`
        // reaches the START_TAG and a second reaches the ELEMENT being typed.
        if let Some(start_tag) = token.parent() {
            if let Some(new_element) = start_tag.parent() {
                return element_name_items(new_element.parent().as_ref(), &new_element);
            }
        }
        return Vec::new();
    }

    Vec::new()
}

fn parent_kind(token: &kubuno_desktop_views::syntax::SyntaxToken) -> Option<SyntaxKind> {
    token.parent().map(|p| p.kind())
}

/// Whether `offset` sits strictly between a `STRING` token's opening and
/// closing quote (not on the quote itself) — so completion is not offered
/// while the cursor is technically still on the boundary token but visually
/// outside the value (e.g. right before the opening `"`).
fn is_strictly_inside_quotes(token: &kubuno_desktop_views::syntax::SyntaxToken, offset: rowan::TextSize) -> bool {
    let range = token.text_range();
    let len: u32 = range.len().into();
    if len < 2 {
        return false; // A lone/unterminated quote — nothing sane to offer inside.
    }
    let start: u32 = range.start().into();
    let end: u32 = range.end().into();
    let offset: u32 = offset.into();
    offset > start && offset < end
}

/// Every registered component name — [`element_name_items`]'s payload once
/// the containing element's children model has been checked.
fn all_component_items() -> Vec<CompletionItem> {
    registry::all().iter().map(component_item).collect()
}

fn component_item(meta: &ComponentMeta) -> CompletionItem {
    CompletionItem {
        label: meta.name.to_string(),
        kind: Some(CompletionItemKind::CLASS),
        documentation: Some(doc_markdown(meta.doc)),
        ..Default::default()
    }
}

/// `container`: the node the new/being-typed element itself sits directly
/// inside — `None` (or a `DOCUMENT`) means it is the file's root, which is
/// always offered every component (no parent `ChildrenModel` to gate
/// against). Otherwise, resolve the nearest ancestor `Element` and only
/// offer components when its registry entry still accepts a(nother) child.
///
/// `new_element` is the (possibly still nameless/malformed) `ELEMENT` node
/// the parser already created for whatever the cursor is sitting in — the
/// error-tolerant grammar starts an `ELEMENT`/`START_TAG` the moment it sees
/// `<`, before any name follows (`syntax::parser::parse_start_tag`), so it
/// already counts as one of `container`'s children by the time this function
/// runs. It must be excluded from the "does the parent already have a
/// child" count below, or a `SingleWidget` parent with *no other* child yet
/// would incorrectly look already-full because of the very child being
/// named.
fn element_name_items(container: Option<&SyntaxNode>, new_element: &SyntaxNode) -> Vec<CompletionItem> {
    let Some(container) = container else { return all_component_items() };
    if container.kind() == SyntaxKind::DOCUMENT {
        return all_component_items();
    }
    let Some(parent_element) = tree::enclosing_element(container) else { return all_component_items() };
    let Some(parent_name) = parent_element.name() else { return all_component_items() };
    let Some(meta) = registry::lookup(&parent_name) else { return all_component_items() };

    let other_children = parent_element.children().filter(|c| c.syntax() != new_element).count();
    match meta.children {
        ChildrenModel::None => Vec::new(),
        ChildrenModel::SingleWidget if other_children >= 1 => Vec::new(),
        // `List`'s payload is the "gated" child names `crate::validate`
        // enforces (`ChildrenModel::List`'s own doc) — completion here does
        // not yet filter by it, so every registered component is still
        // offered, same as before that payload existed.
        ChildrenModel::SingleWidget | ChildrenModel::List(_) => all_component_items(),
    }
}

fn attribute_items(element: &Element) -> Vec<CompletionItem> {
    let mut items = Vec::new();

    items.push(CompletionItem {
        label: "x:Name".to_string(),
        kind: Some(CompletionItemKind::FIELD),
        documentation: Some(doc_markdown(common_attrs::X_NAME_DOC)),
        ..Default::default()
    });

    let is_root = element.syntax().parent().is_none_or(|p| p.kind() != kubuno_desktop_views::syntax::SyntaxKind::ELEMENT);
    let meta = element.name().and_then(|name| registry::lookup(&name));
    let mut seen: std::collections::HashSet<&'static str> = std::collections::HashSet::new();
    // The element's own properties, then the ones it inherits (`Control`'s `BackColor`, `Enabled`,
    // `ToolTip`...), then on the root element the view's (`Title`, `StartPosition`...).
    let mut properties: Vec<(Option<&'static str>, &'static registry::PropertyMeta)> = meta.map(|m| m.all_properties()).unwrap_or_default();
    if is_root {
        properties.extend(kubuno_desktop_views::registry::VIEW_PROPERTIES.iter().map(|p| (Some("View"), p)));
    }
    for (level, prop) in properties {
        if !seen.insert(prop.name) {
            continue;
        }
        let from = level.map(|l| format!(" (`{l}`)")).unwrap_or_default();
        items.push(CompletionItem {
            label: prop.name.to_string(),
            kind: Some(CompletionItemKind::PROPERTY),
            detail: prop.category.map(str::to_string),
            documentation: Some(doc_markdown(&format!("{}\n\n*Default: `{}`*{from}", prop.doc, prop.default))),
            ..Default::default()
        });
    }
    for common in common_attrs::COMMON_ATTRIBUTES {
        // Design-time attributes only make sense on the view's root element.
        if !is_root && registry::design_time_attribute(common.name).is_some() {
            continue;
        }
        if !seen.insert(common.name) {
            continue;
        }
        items.push(CompletionItem {
            label: common.name.to_string(),
            kind: Some(CompletionItemKind::PROPERTY),
            documentation: Some(doc_markdown(common.doc)),
            ..Default::default()
        });
    }

    if let Some(name) = element.name() {
        if let Some(meta) = registry::lookup(&name) {
            // Own and common events (older aliases are accepted but not offered), plus the
            // view's own events on the root element.
            let is_root = element.syntax().parent().is_none_or(|p| p.kind() != kubuno_desktop_views::syntax::SyntaxKind::ELEMENT);
            let view_events = kubuno_desktop_views::registry::VIEW_EVENTS.iter().filter(|_| is_root);
            for event in meta.all_events().into_iter().chain(view_events) {
                items.push(CompletionItem {
                    label: event.name.to_string(),
                    kind: Some(CompletionItemKind::EVENT),
                    detail: Some(format!("{} ({})", event.category.name(), event.args_type)),
                    documentation: Some(doc_markdown(event.doc)),
                    ..Default::default()
                });
            }
        }
    }

    // On the root element: the namespace declarations it does not carry yet (`xmlns`, `xmlns:x`, `xmlns:d`).
    items.extend(crate::namespaces::attribute_items(element));
    items
}

fn enum_value_items(string_token: &kubuno_desktop_views::syntax::SyntaxToken) -> Vec<CompletionItem> {
    let Some(attribute_node) = string_token.parent() else { return Vec::new() };
    let Some(attr) = Attribute::cast(attribute_node.clone()) else { return Vec::new() };
    let Some(attr_name) = attr.name() else { return Vec::new() };
    // `xmlns:x="…"`: the standard URI of the declaration.
    if let Some(items) = crate::namespaces::value_items(&attr_name) {
        return items;
    }
    let Some(element) = tree::enclosing_element(&attribute_node) else { return Vec::new() };
    let Some(element_name) = element.name() else { return Vec::new() };

    let property = registry::lookup(&element_name).and_then(|meta| meta.property(&attr_name)).or_else(|| kubuno_desktop_views::registry::view_property(&attr_name));
    // A reference to an element of the view (`ContextMenu`, `Target`, `DropDownMenu`…): the names
    // of the elements of that kind; a user control (`ItemTemplate`): the project's user controls; a
    // list or an object: a binding, the only way to set one.
    match property.and_then(|p| p.editor) {
        Some(editor) if editor.starts_with("reference:") => {
            let kind = &editor["reference:".len()..];
            let root = attribute_node.ancestors().last().unwrap_or_else(|| attribute_node.clone());
            let mut names: Vec<String> = root
                .descendants()
                .filter_map(kubuno_desktop_views::ast::Element::cast)
                .filter(|e| e.name().is_some_and(|n| n == kind || registry::lookup(&n).is_some_and(|m| m.is_a(kind))))
                .filter_map(|e| e.attribute("x:Name").and_then(|a| a.value()).filter(|n| !n.is_empty()))
                .collect();
            names.sort();
            names.dedup();
            return names.into_iter().map(|n| CompletionItem { label: n, kind: Some(CompletionItemKind::REFERENCE), ..Default::default() }).collect();
        }
        Some(editor) if editor.starts_with("class:") => {
            return kubuno_desktop_views::registry::project_components()
                .iter()
                .filter(|c| c.kind == kubuno_desktop_views::registry::ClassKind::UserControl)
                .map(|c| CompletionItem { label: c.name.to_string(), kind: Some(CompletionItemKind::CLASS), ..Default::default() })
                .collect();
        }
        // An icon: the glyphs of the Kubuno icon set (with a picture of each), its short aliases.
        Some("icon") => return icon_items(),
        Some("list") | Some("object") => {
            return vec![CompletionItem { label: "{Binding }".to_string(), kind: Some(CompletionItemKind::SNIPPET), ..Default::default() }];
        }
        _ => {}
    }
    // A window's corner radius: the usual radii, Windows 11's first.
    if property.and_then(|p| p.type_converter) == Some("CornerRadius") {
        return corner_radius_items();
    }
    let kind = property.map(|p| p.kind).or_else(|| common_attrs::lookup(&attr_name).map(|c| c.kind));

    match kind {
        Some(PropKind::Enum(variants)) => variants
            .iter()
            .map(|v| CompletionItem { label: v.to_string(), kind: Some(CompletionItemKind::ENUM_MEMBER), ..Default::default() })
            .collect(),
        _ => Vec::new(),
    }
}

/// The values offered for a window's `CornerRadius` (DIP): Windows 11's two radii (drawn by DWM
/// there), square, and the larger radii the host draws itself (`kubuno_desktop_controls::host::frame`).
pub fn corner_radius_items() -> Vec<CompletionItem> {
    const RADII: &[(&str, &str)] = &[
        ("8", "Windows 11's rounded corners (the default)"),
        ("4", "Windows 11's small rounded corners"),
        ("0", "Square corners"),
        ("12", "Larger rounded corners"),
        ("16", "Larger rounded corners"),
        ("24", "Large rounded corners"),
    ];
    RADII
        .iter()
        .enumerate()
        .map(|(i, (value, detail))| CompletionItem {
            label: value.to_string(),
            kind: Some(CompletionItemKind::VALUE),
            detail: Some(detail.to_string()),
            sort_text: Some(format!("{i}")),
            ..Default::default()
        })
        .collect()
}

/// The values of an icon attribute: every glyph of the Kubuno icon set — its set as the detail, a
/// picture of it (an SVG drawn from the glyph's own paths) and its keywords as the documentation,
/// the keywords also matching the filter (`folder-open` finds `FolderOpen`) — then the short
/// aliases (`trash` → `Trash2`). An image file is typed as a path (`resources/save.svg`).
pub fn icon_items() -> Vec<CompletionItem> {
    let catalog = kubuno_desktop_views::icon::catalog_json();
    let mut items: Vec<CompletionItem> = catalog["icons"]
        .as_array()
        .map(|icons| {
            icons
                .iter()
                .filter_map(|icon| {
                    let name = icon["name"].as_str()?;
                    let set = icon["set"].as_str().unwrap_or_default();
                    let keywords: Vec<&str> = icon["keywords"].as_array().map(|k| k.iter().filter_map(|w| w.as_str()).collect()).unwrap_or_default();
                    let picture = icon_svg(icon).map(|svg| format!("![{name}](data:image/svg+xml;base64,{})\n\n", kubuno_desktop_views::icon::base64(svg.as_bytes()))).unwrap_or_default();
                    Some(CompletionItem {
                        label: name.to_string(),
                        kind: Some(CompletionItemKind::CONSTANT),
                        detail: Some(format!("{set} icon")),
                        documentation: Some(Documentation::MarkupContent(MarkupContent {
                            kind: MarkupKind::Markdown,
                            value: format!("{picture}**{name}** — {set}\n\n{}", keywords.join(", ")),
                        })),
                        filter_text: Some(format!("{name} {}", keywords.join(" "))),
                        sort_text: Some(format!("{}{name}", if set == "Lucide" { 0 } else { 1 })),
                        ..Default::default()
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    for (alias, glyph) in catalog["aliases"].as_object().into_iter().flatten() {
        items.push(CompletionItem {
            label: alias.clone(),
            kind: Some(CompletionItemKind::CONSTANT),
            detail: glyph.as_str().map(|g| format!("alias of {g}")),
            sort_text: Some(format!("2{alias}")),
            ..Default::default()
        });
    }
    items
}

/// A glyph of the catalog as a small SVG document (24 px, the theme-neutral grey of a tooltip).
fn icon_svg(icon: &serde_json::Value) -> Option<String> {
    let viewbox = icon["viewBox"].as_f64()?;
    let mut body = String::new();
    for layer in icon["layers"].as_array()? {
        let d = layer["d"].as_str()?;
        let colour = layer["color"].as_str().unwrap_or("#6b7280");
        let opacity = layer["opacity"].as_f64().unwrap_or(1.0);
        let t: Vec<f64> = layer["transform"].as_array().map(|a| a.iter().filter_map(|v| v.as_f64()).collect()).unwrap_or_default();
        let transform = if t.len() == 6 { format!(" transform=\"matrix({} {} {} {} {} {})\"", t[0], t[1], t[2], t[3], t[4], t[5]) } else { String::new() };
        let paint = match layer["stroke"].as_f64() {
            Some(w) => format!("fill=\"none\" stroke=\"{colour}\" stroke-width=\"{w}\" stroke-linecap=\"round\" stroke-linejoin=\"round\""),
            None => format!("fill=\"{colour}\""),
        };
        body.push_str(&format!("<path d=\"{d}\" {paint} opacity=\"{opacity}\"{transform}/>"));
    }
    Some(format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"24\" height=\"24\" viewBox=\"0 0 {viewbox} {viewbox}\">{body}</svg>"))
}

fn closing_tag_item(element_name: &str) -> CompletionItem {
    CompletionItem {
        label: format!("{element_name}>"),
        kind: Some(CompletionItemKind::CLASS),
        insert_text: Some(format!("{element_name}>")),
        documentation: Some(doc_markdown(&format!("Closes the enclosing `<{element_name}>`."))),
        ..Default::default()
    }
}

fn doc_markdown(text: &str) -> Documentation {
    Documentation::MarkupContent(MarkupContent { kind: MarkupKind::Markdown, value: text.to_string() })
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

    fn labels(items: &[CompletionItem]) -> Vec<String> {
        items.iter().map(|i| i.label.clone()).collect()
    }

    #[test]
    fn new_child_right_after_open_angle_offers_every_component() {
        let d = open("<Stack><");
        let items = completion(&d, Position { line: 0, character: 8 });
        let ls = labels(&items);
        assert!(ls.contains(&"Button".to_string()));
        assert!(ls.contains(&"Switch".to_string()));
        assert!(ls.contains(&"Card".to_string()));
    }

    #[test]
    fn childless_component_offers_no_children() {
        let d = open("<Switch><");
        let items = completion(&d, Position { line: 0, character: 9 });
        assert!(items.is_empty());
    }

    #[test]
    fn single_widget_with_existing_child_offers_no_more_children() {
        let d = open(r#"<Card><Button Text="a"/><"#);
        let items = completion(&d, Position { line: 0, character: 26 });
        assert!(items.is_empty());
    }

    #[test]
    fn single_widget_with_no_child_yet_offers_components() {
        let d = open("<Card><");
        let items = completion(&d, Position { line: 0, character: 7 });
        assert!(!items.is_empty());
    }

    #[test]
    fn root_position_offers_every_component() {
        let d = open("<");
        let items = completion(&d, Position { line: 0, character: 1 });
        assert!(labels(&items).contains(&"Button".to_string()));
    }

    #[test]
    fn attribute_name_completion_after_tag_name_space() {
        let d = open(r#"<Button "#);
        let items = completion(&d, Position { line: 0, character: 8 });
        let ls = labels(&items);
        assert!(ls.contains(&"Text".to_string()));
        assert!(ls.contains(&"Variant".to_string()));
        assert!(ls.contains(&"OnClick".to_string()));
        assert!(ls.contains(&"Dock".to_string())); // common attribute
        assert!(ls.contains(&"x:Name".to_string()));
    }

    #[test]
    fn inherited_and_view_properties_are_offered_once() {
        let d = open(r#"<Panel><Button "#);
        let ls = labels(&completion(&d, Position { line: 0, character: 15 }));
        for name in ["BackColor", "Font", "Enabled", "ToolTip", "AccessibleName", "TabIndex", "UseMnemonic", "TextAlign"] {
            assert!(ls.contains(&name.to_string()), "{name}");
        }
        assert_eq!(ls.iter().filter(|l| *l == "Dock").count(), 1, "a registry property is not offered twice");
        assert!(!ls.contains(&"Title".to_string()), "a view property belongs to the root");
        let root = labels(&completion(&open(r#"<Panel "#), Position { line: 0, character: 7 }));
        assert!(root.contains(&"Title".to_string()) && root.contains(&"StartPosition".to_string()) && root.contains(&"Opacity".to_string()));
    }

    #[test]
    fn enum_value_completion_for_a_view_property() {
        let d = open(r#"<Panel StartPosition=""/>"#);
        let ls = labels(&completion(&d, Position { line: 0, character: 22 }));
        assert!(ls.contains(&"CenterScreen".to_string()), "{ls:?}");
    }

    #[test]
    fn attribute_name_completion_mid_typing() {
        let d = open(r#"<Button Te"#);
        let items = completion(&d, Position { line: 0, character: 10 });
        assert!(labels(&items).contains(&"Text".to_string()));
    }

    #[test]
    fn enum_value_completion_inside_quotes() {
        let d = open(r#"<Button Variant=""/>"#);
        // Cursor between the two quotes.
        let items = completion(&d, Position { line: 0, character: 17 });
        let ls = labels(&items);
        assert!(ls.contains(&"Primary".to_string()));
        assert!(ls.contains(&"Danger".to_string()));
    }

    #[test]
    fn a_window_corner_radius_offers_the_usual_radii() {
        // On the view's root (the window) and on an in-view window.
        let d = open(r#"<Panel CornerRadius=""/>"#);
        let ls = labels(&completion(&d, Position { line: 0, character: 21 }));
        assert_eq!(&ls[..3], ["8", "4", "0"], "Windows 11's radius first");
        let d = open(r#"<FloatingWindow CornerRadius=""/>"#);
        assert!(labels(&completion(&d, Position { line: 0, character: 30 })).contains(&"16".to_string()));
    }

    #[test]
    fn enum_value_completion_for_common_attribute() {
        let d = open(r#"<Button Dock=""/>"#);
        let items = completion(&d, Position { line: 0, character: 14 });
        let ls = labels(&items);
        assert!(ls.contains(&"Top".to_string()));
        assert!(ls.contains(&"Fill".to_string()));
    }

    #[test]
    fn reference_values_complete_the_names_of_the_view() {
        let d = open(r#"<Panel><ContextMenu x:Name="edit"/><ContextMenu x:Name="view"/><Button x:Name="ok" DropDownMenu=""/><Popover Target=""/></Panel>"#);
        let ls = labels(&completion(&d, Position { line: 0, character: 97 }));
        assert_eq!(ls, vec!["edit".to_string(), "view".to_string()], "the menus of the view");
        let d = open(r#"<Panel><Button x:Name="ok"/><Popover Target=""/></Panel>"#);
        let ls = labels(&completion(&d, Position { line: 0, character: 45 }));
        assert!(ls.contains(&"ok".to_string()), "{ls:?}");
        let d = open(r#"<Repeater ItemsSource=""/>"#);
        let ls = labels(&completion(&d, Position { line: 0, character: 23 }));
        assert_eq!(ls, vec!["{Binding }".to_string()]);
    }

    #[test]
    fn non_enum_attribute_value_offers_nothing() {
        let d = open(r#"<Button Text=""/>"#);
        let items = completion(&d, Position { line: 0, character: 14 });
        assert!(items.is_empty());
    }

    #[test]
    fn closing_tag_completion_offers_the_matching_name() {
        let d = open("<Card></");
        let items = completion(&d, Position { line: 0, character: 8 });
        assert_eq!(labels(&items), vec!["Card>".to_string()]);
    }

    #[test]
    fn closing_tag_completion_mid_typed_name() {
        let d = open("<Card></Ca");
        let items = completion(&d, Position { line: 0, character: 11 });
        assert_eq!(labels(&items), vec!["Card>".to_string()]);
    }
}

#[cfg(test)]
mod icon_tests {
    use super::*;

    #[test]
    fn icon_values_are_the_set_with_pictures_then_the_aliases() {
        let items = icon_items();
        let save = items.iter().find(|i| i.label == "Save").expect("Save is offered");
        assert_eq!(save.detail.as_deref(), Some("Lucide icon"));
        let Some(Documentation::MarkupContent(doc)) = &save.documentation else { panic!("a picture") };
        assert!(doc.value.contains("data:image/svg+xml;base64,"));
        assert!(save.filter_text.as_deref().is_some_and(|f| f.contains("save")));
        assert!(items.iter().any(|i| i.label == "trash" && i.detail.as_deref() == Some("alias of Trash2")));
        assert!(items.len() > 300, "the whole set: {}", items.len());
    }
}
