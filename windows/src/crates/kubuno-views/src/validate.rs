//! The validator — phase 2a's brief: "given a parsed view + the registry,
//! report unknown elements/attributes, wrong value types/enum values, and
//! invalid children — with line/column". `XML_VIEWS.md` §5 is explicit this
//! is meant to be reused as-is by the language server and hot reload's
//! in-app error banner, so it produces the same [`crate::syntax::Diagnostic`]
//! shape the parser itself does — one diagnostic type, two producers.

use crate::ast::{AstNode, Document, Element};
use crate::registry::{ChildrenModel, ComponentMeta, PropKind};
use crate::syntax::{Diagnostic, Parse};
use rowan::TextRange;
// The registry-independent core (literal checks, the attributes every element accepts, typo
// suggestions) lives in the platform-neutral `kubuno-views-syntax` (`validate`, WV-1).
use kubuno_views_syntax::validate::{check_value, closest, is_binding_expression, is_root_element, with_suggestion, COMMON_ATTRIBUTES};

/// Validates a parsed `.kbview` file against `registry`. Never panics; an
/// unparseable file (see `parse.diagnostics`) simply has fewer elements to
/// walk — the two diagnostic sources are meant to be shown together, not one
/// gating the other (§5: "a parse or a registry-lookup error must not blank
/// the screen").
pub fn validate(parse: &Parse, registry: &[ComponentMeta]) -> Vec<Diagnostic> {
    validate_with_repairs(parse, registry).into_iter().map(|(d, _)| d).collect()
}

/// How the designer's tolerant compilation (`crate::tolerant`) gets past one diagnostic of
/// [`validate`] instead of refusing the whole view: every finding comes with the smallest change to
/// the text that removes it while keeping every other element where it is (element ids are paths of
/// child ordinals, so no element before or above a kept one is ever removed).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Repair {
    /// The attribute (its whole `ATTRIBUTE` node) is ignored: the property keeps its default.
    DropAttribute(TextRange),
    /// The element (its whole `ELEMENT` node) is drawn as a labelled placeholder box at its place,
    /// its children still built inside it.
    Placeholder(TextRange),
    /// The element (its whole `ELEMENT` node) and every element sibling after it are not shown: the
    /// children its parent cannot hold. Only ever trailing children of one parent, so the ids of the
    /// elements kept do not change.
    DropElement(TextRange),
    /// The closing tag's name (its token range) is rewritten to the element's own name.
    RenameEndTag(TextRange, String),
}

/// [`validate`], each diagnostic paired with the [`Repair`] that gets past it.
pub(crate) fn validate_with_repairs(parse: &Parse, registry: &[ComponentMeta]) -> Vec<(Diagnostic, Repair)> {
    let root = parse.syntax();
    let Some(doc) = Document::cast(root) else { return Vec::new() };
    let mut out = Vec::new();
    if let Some(element) = doc.root_element() {
        walk(&element, registry, parse, &mut out);
    }
    out
}

/// The registry entry of `name` for the validator: the registry's own, or the designer's
/// placeholder element (`crate::tolerant`), which only ever appears in a text the designer rewrote.
fn meta_of<'r>(registry: &'r [ComponentMeta], name: &str) -> Option<&'r ComponentMeta> {
    registry.iter().find(|c| c.name == name).or_else(|| crate::tolerant::placeholder_meta(name))
}

/// Non-blocking findings (the language server shows them as warnings; the view still compiles and
/// runs): a `Dock`/`Anchor` on an element whose parent does not lay out by Dock/Anchor (anything but a
/// `<Panel>`-like container, or the view's root element) — the attribute is simply ignored there.
pub fn warnings(parse: &Parse) -> Vec<Diagnostic> {
    let Some(doc) = Document::cast(parse.syntax()) else { return Vec::new() };
    let Some(root) = doc.root_element() else { return Vec::new() };
    let mut out = Vec::new();
    // The ribbon's own checks (`vskubuno/docs/RIBBON.md` §5): scaling policies and KeyTips.
    out.extend(crate::registry::families::ribbon::warnings(parse, &root));
    // The menus' own checks (`vskubuno/docs/MENUS.md` §6): shortcuts, access keys, references.
    out.extend(crate::menus::warnings(parse, &root));
    // The title bar's standard items need the `HeaderActions` user control (vskubuno docs/SHELL-CONTROLS.md §5).
    if crate::window::HeaderSpec::read(&root).has_cluster() && !crate::window::header_class_registered() {
        let first = crate::window::HEADER_CLUSTER_PROPERTIES.iter().find_map(|n| root.attribute(n)).and_then(|a| a.name_range());
        if let Some(range) = first {
            let lc = parse.line_col(range.start());
            out.push(Diagnostic { range, line: lc.line, column: lc.column, message: crate::window::header_class_missing_message() });
        }
    }
    for element in root.syntax().descendants().filter_map(Element::cast) {
        let parent_layout = element
            .syntax()
            .parent()
            .and_then(Element::cast)
            .and_then(|p| p.name())
            .and_then(|name| crate::registry::lookup(&name))
            .map(|meta| meta.layout);
        // An icon that names nothing of the icon set nor an image file draws nothing: a warning (an
        // unknown name was always ignored at run time, so the view still compiles).
        let meta = element.name().and_then(|n| crate::registry::lookup(&n));
        let is_root = element.syntax().parent().and_then(Element::cast).is_none();
        for attr in element.attributes() {
            let Some(attr_name) = attr.name() else { continue };
            let icon = meta.and_then(|m| m.property(&attr_name)).or_else(|| if is_root { crate::registry::view_property(&attr_name) } else { None });
            if !icon.is_some_and(|p| p.is_icon()) {
                continue;
            }
            let value = attr.value().unwrap_or_default();
            if let (Err(message), Some(range)) = (crate::icon::check(&value), attr.value_range()) {
                let lc = parse.line_col(range.start());
                out.push(Diagnostic { range, line: lc.line, column: lc.column, message: format!("attribute `{attr_name}`: {message}") });
            }
        }
        if parent_layout == Some(crate::registry::LayoutKind::DockAnchor) {
            continue;
        }
        for name in ["Dock", "Anchor"] {
            let Some(attr) = element.attribute(name) else { continue };
            let value = attr.value().unwrap_or_default();
            if value.trim().is_empty() || (name == "Dock" && value.trim() == "None") {
                continue;
            }
            if let Some(range) = attr.name_range() {
                let lc = parse.line_col(range.start());
                out.push(Diagnostic {
                    range,
                    line: lc.line,
                    column: lc.column,
                    message: format!("`{name}` only applies to a child of a Dock/Anchor container such as `<Panel>`; it is ignored here"),
                });
            }
        }
    }
    out
}

/// Warnings for the icon files a view names that do not exist beside it (`Icon="images/save.svg"`
/// with no such file under `base_dir`, the view's folder): the icon would draw nothing.
pub fn icon_file_warnings(parse: &Parse, base_dir: &std::path::Path) -> Vec<Diagnostic> {
    let Some(doc) = Document::cast(parse.syntax()) else { return Vec::new() };
    let Some(root) = doc.root_element() else { return Vec::new() };
    let mut out = Vec::new();
    for element in root.syntax().descendants().filter_map(Element::cast) {
        let meta = element.name().and_then(|n| crate::registry::lookup(&n));
        let is_root = element.syntax().parent().and_then(Element::cast).is_none();
        for attr in element.attributes() {
            let Some(attr_name) = attr.name() else { continue };
            let icon = meta.and_then(|m| m.property(&attr_name)).or_else(|| if is_root { crate::registry::view_property(&attr_name) } else { None });
            let value = attr.value().unwrap_or_default();
            let v = value.trim();
            if !icon.is_some_and(|p| p.is_icon()) || v.starts_with('{') || !drive_app_controls::icon_source::is_image_path(v) || drive_app_controls::icon_source::is_resource(v) {
                continue;
            }
            let path = base_dir.join(v.replace('/', std::path::MAIN_SEPARATOR_STR));
            if !path.is_file() {
                if let Some(range) = attr.value_range() {
                    let lc = parse.line_col(range.start());
                    out.push(Diagnostic { range, line: lc.line, column: lc.column, message: format!("attribute `{attr_name}`: the file `{v}` is not found beside the view") });
                }
            }
        }
    }
    out
}

/// Hints (the language server shows them as hints, with the new name): an event attribute
/// written under an older alias, `OnToggled="x"` on a `<Switch>` whose event is now
/// `OnCheckedChanged` (`vskubuno/docs/EVENTS.md` §3). The old name keeps working: aliases are
/// never removed within 1.x. The message names the new attribute between backquotes.
pub fn hints(parse: &Parse) -> Vec<Diagnostic> {
    let Some(doc) = Document::cast(parse.syntax()) else { return Vec::new() };
    let Some(root) = doc.root_element() else { return Vec::new() };
    let mut out = Vec::new();
    for element in root.syntax().descendants().filter_map(Element::cast) {
        let Some(meta) = element.name().and_then(|n| crate::registry::lookup(&n)) else { continue };
        for attr in element.attributes() {
            let Some(name) = attr.name() else { continue };
            let target = meta.alias_target(&name).map(|e| e.name).or_else(|| meta.property_alias_target(&name).map(|p| p.name));
            let Some(target_name) = target else { continue };
            if let Some(range) = attr.name_range() {
                let lc = parse.line_col(range.start());
                out.push(Diagnostic {
                    range,
                    line: lc.line,
                    column: lc.column,
                    message: format!("`{name}` is an older name for `{target_name}`"),
                });
            }
        }
    }
    out
}

/// What a misspelt element name was probably meant to be: the closest element of `registry`.
pub fn element_suggestion(registry: &[ComponentMeta], name: &str) -> Option<&'static str> {
    closest(name, registry.iter().map(|m| m.name)).and_then(|n| registry.iter().find(|m| m.name == n)).map(|m| m.name)
}

/// What an unknown attribute `name` of an element of `meta` was probably meant to be: for `Binding`,
/// the element's text property bound (`Text="{Binding …}"`, the XAML habit of a `Binding` attribute);
/// else the closest of its properties, events and common attributes. The suggestion as it would be
/// written (`Text="{Binding …}"`, or a bare attribute name).
pub fn attribute_suggestion(meta: &ComponentMeta, name: &str, is_root: bool) -> Option<String> {
    let properties = meta.all_properties();
    if name.eq_ignore_ascii_case("Binding") {
        let target = properties
            .iter()
            .map(|(_, p)| *p)
            .find(|p| p.name == "Text")
            .or_else(|| properties.iter().map(|(_, p)| *p).find(|p| matches!(p.kind, PropKind::String) && !p.is_bound_only()))?;
        return Some(format!("{}=\"{{Binding …}}\"", target.name));
    }
    let mut names: Vec<&str> = properties.iter().map(|(_, p)| p.name).collect();
    names.extend(meta.all_events().iter().map(|e| e.name));
    names.extend(COMMON_ATTRIBUTES.iter().map(|(n, _)| *n));
    if is_root {
        names.extend(crate::registry::common::VIEW_PROPERTIES.iter().map(|p| p.name));
        names.extend(crate::registry::VIEW_EVENTS.iter().map(|e| e.name));
    }
    closest(name, names.into_iter()).map(str::to_string)
}

/// [`validate`] against [`crate::registry::all`] — the common case.
pub fn validate_with_default_registry(parse: &Parse) -> Vec<Diagnostic> {
    validate(parse, crate::registry::all())
}

fn walk(element: &Element, registry: &[ComponentMeta], parse: &Parse, out: &mut Vec<(Diagnostic, Repair)>) {
    let mut push = |range: TextRange, message: String, repair: Repair| {
        let lc = parse.line_col(range.start());
        out.push((Diagnostic { range, line: lc.line, column: lc.column, message }, repair));
    };
    let element_range = element.syntax().text_range();

    let Some(name) = element.name() else {
        // The parser already reported "expected an element name" — nothing
        // new for the validator to add.
        return walk_children(element, registry, parse, out);
    };

    if let Some(end_name) = element.end_name_token() {
        if end_name.text() != name {
            push(
                end_name.text_range(),
                format!("mismatched closing tag: expected `</{name}>`, found `</{}>`", end_name.text()),
                Repair::RenameEndTag(end_name.text_range(), name.clone()),
            );
        }
    }

    // A property element (`<RibbonTab.ScalingPolicy>` directly inside its `<RibbonTab>`): the
    // owner's node reads it; it is not an element of its own.
    if let Some((owner, _)) = name.split_once('.') {
        let parent = element.syntax().parent().and_then(<Element as crate::ast::AstNode>::cast).and_then(|p| p.name());
        if parent.as_deref() == Some(owner) {
            return;
        }
    }
    let meta = meta_of(registry, &name);
    let Some(meta) = meta else {
        if let Some(range) = element.name_range() {
            let suggestion = element_suggestion(registry, &name).map(|s| format!("<{s}>"));
            push(range, with_suggestion(format!("unknown element `<{name}>`"), suggestion), Repair::Placeholder(element_range));
        }
        return walk_children(element, registry, parse, out);
    };

    for attr in element.attributes() {
        let Some(attr_name) = attr.name() else { continue };
        let drop = Repair::DropAttribute(attr.syntax().text_range());
        // `d:Text="…"`: a design-time value (`kubuno_views_meta::inherit::apply_design_attributes`), free too.
        if kubuno_views_syntax::markup::is_markup(&attr_name) {
            continue; // §1: reserved, free — not a component property.
        }

        // Design-time attributes (`DesignWidth`/`DesignHeight`): the designer's canvas size, only
        // meaningful on the view's root element.
        if let Some(design) = crate::registry::design_time_attribute(&attr_name) {
            let is_root = element.syntax().parent().is_none_or(|p| p.kind() != crate::syntax::SyntaxKind::ELEMENT);
            if !is_root {
                if let Some(range) = attr.name_range() {
                    push(range, format!("`{attr_name}` is a design-time attribute, only valid on the root element"), drop);
                }
            } else if let Some(value) = attr.value() {
                if let Err(message) = check_value(&design.kind, &value) {
                    if let Some(range) = attr.value_range() {
                        push(range, format!("attribute `{attr_name}`: {message}"), drop);
                    }
                }
            }
            continue;
        }

        // An event name (`OnToggled="offline_toggled"`, §2) is also written
        // as a plain XML attribute; its value is a handler name, not a typed
        // property, so it validates as a free-form string.
        let property = meta
            .property(&attr_name)
            // The view's own properties (`Title`, `Opacity`…, the form's) belong to its root element.
            .or_else(|| if is_root_element(element) { crate::registry::view_property(&attr_name) } else { None });
        let kind = property
            .map(|p| p.kind)
            .or_else(|| meta.event(&attr_name).map(|_| PropKind::String))
            // The view's own events (`OnLoad`…) belong to its root element.
            .or_else(|| (is_root_element(element) && crate::registry::view_event(&attr_name).is_some()).then_some(PropKind::String))
            .or_else(|| COMMON_ATTRIBUTES.iter().find(|(n, _)| *n == attr_name).map(|(_, k)| *k));

        let Some(kind) = kind else {
            if meta.open_attributes {
                // See `ComponentMeta::open_attributes`'s own doc — this
                // element's real fields are named by its PARENT, not by a
                // closed `properties` list (`<Item Role="Admin"/>`, `Role`
                // matching a `<Column Binding="{Binding Role}">` elsewhere).
                continue;
            }
            if let Some(range) = attr.name_range() {
                let suggestion = attribute_suggestion(meta, &attr_name, is_root_element(element));
                push(range, with_suggestion(format!("unknown attribute `{attr_name}` on `<{name}>`"), suggestion), drop);
            }
            continue;
        };

        let Some(value) = attr.value() else { continue }; // Malformed string — the parser already flagged it.
        if is_binding_expression(&value) {
            continue; // Resolved at runtime against live state (§3) — not a static value to type-check.
        }
        // A list or an object (a custom control's `Rows` / `Shared<T>` property) comes from a binding
        // only: no XML literal spells one.
        let bound_only = property.is_some_and(|p| p.is_bound_only());
        let checked = if bound_only && !value.trim().is_empty() {
            Err(format!("a {} is set with a binding (`{{Binding Path}}`), not a literal value", if property.is_some_and(|p| p.editor == Some("list")) { "list" } else { "value of this type" }))
        } else {
            check_value(&kind, &value).and_then(|()| property.map_or(Ok(()), |p| check_format(p, &value)))
        };
        if let Err(message) = checked {
            if let Some(range) = attr.value_range() {
                push(range, format!("attribute `{attr_name}`: {message}"), drop);
            }
        }
    }

    let children: Vec<Element> = element.children().collect();
    match meta.children {
        ChildrenModel::None if !children.is_empty() => {
            if let Some(range) = children[0].name_range().or_else(|| element.name_range()) {
                push(range, format!("`<{name}>` does not accept children (found {})", children.len()), Repair::DropElement(children[0].syntax().text_range()));
            }
        }
        ChildrenModel::SingleWidget if children.len() > 1 => {
            if let Some(range) = children[1].name_range() {
                push(range, format!("`<{name}>` accepts at most one child, found {}", children.len()), Repair::DropElement(children[1].syntax().text_range()));
            }
        }
        _ => {}
    }

    // Gated children (`<TabItem>`, `<Item>`, `<Column>`, `<Option>`, `<Step>`,
    // `<AccordionSection>`, `<BreadcrumbItem>`, `<ToolbarItem>` — each a
    // `ChildrenModel::List` component's own `allowed` name somewhere in the
    // registry) are only valid directly under a parent whose own `allowed`
    // names them, wherever they are actually nested — not just when the
    // wrong parent also happens to be another restrictive `List`.
    let allowed_here: &[&str] = match meta.children {
        ChildrenModel::List(allowed) => allowed,
        _ => &[],
    };
    for child in &children {
        let Some(child_name) = child.name() else { continue };
        if !is_gated(registry, &child_name) || allowed_here.contains(&child_name.as_str()) {
            continue;
        }
        if let Some(range) = child.name_range() {
            let parents = gated_parents(registry, &child_name);
            let hint = if parents.is_empty() {
                String::new()
            } else {
                format!("; valid only directly inside {}", parents.iter().map(|p| format!("`<{p}>`")).collect::<Vec<_>>().join(", "))
            };
            push(range, format!("`<{child_name}>` is not valid inside `<{name}>` here{hint}"), Repair::Placeholder(child.syntax().text_range()));
        }
    }

    walk_children(element, registry, parse, out);
}

fn walk_children(element: &Element, registry: &[ComponentMeta], parse: &Parse, out: &mut Vec<(Diagnostic, Repair)>) {
    for child in element.children() {
        walk(&child, registry, parse, out);
    }
}

/// Whether `name` is a "gated" child element — one that appears in *some*
/// component's [`ChildrenModel::List`] `allowed` list anywhere in the
/// registry, and is therefore only ever valid directly under one of those
/// parents (registry-driven: no separate hand-maintained list of gated
/// names to keep in sync — see [`registry::ChildrenModel::List`]'s doc).
fn is_gated(registry: &[ComponentMeta], name: &str) -> bool {
    registry.iter().any(|c| matches!(c.children, ChildrenModel::List(allowed) if allowed.contains(&name)))
}

/// Every registered component whose `allowed` list names `child_name` — the
/// parent(s) it is actually valid directly inside, used only to phrase the
/// diagnostic (the accept/reject decision itself is the caller's own
/// `allowed_here.contains(...)` check, not this function).
fn gated_parents<'r>(registry: &'r [ComponentMeta], child_name: &str) -> Vec<&'r str> {
    registry
        .iter()
        .filter_map(|c| match c.children {
            ChildrenModel::List(allowed) if allowed.contains(&child_name) => Some(c.name),
            _ => None,
        })
        .collect()
}

/// The value's shape beyond its kind, for the properties whose text has a grammar of its own: a
/// colour, a font, a `Margin`/`Padding`, a size, an opacity, a password character.
fn check_format(property: &crate::registry::PropertyMeta, value: &str) -> Result<(), String> {
    match property.type_converter {
        Some("Color") => crate::style::parse_color(value).map(|_| ()),
        Some("Font") => crate::style::parse_font(value).map(|_| ()),
        Some("Padding") => crate::style::parse_padding(value).map(|_| ()),
        Some("Size") => crate::style::parse_size(value).map(|_| ()),
        Some("IconSize") => crate::icon::parse_icon_size(value).map(|_| ()),
        Some("Opacity") => match value.trim().parse::<f32>() {
            Ok(v) if (0.0..=100.0).contains(&v) => Ok(()),
            _ => Err(format!("`{value}` is not an opacity: write a percentage from 0 to 100")),
        },
        Some("CornerRadius") => match kubuno_controls::host::form::parse_corner_radius(value) {
            Some(_) => Ok(()),
            None => Err(format!("`{value}` is not a corner radius: write a number of pixels, 0 or more (0 for square corners)")),
        },
        _ if property.name == "PasswordChar" && value.chars().count() > 1 => Err(format!("`{value}` is not one character")),
        _ => Ok(()),
    }
}

/// The colour attribute `name` of `element` as written (literal, parsed), `None` when absent, a
/// binding, or not a colour.
fn literal_color(element: &Element, name: &str) -> Option<crate::style::ColorValue> {
    let value = element.attribute(name)?.value()?;
    if is_binding_expression(&value) {
        return None;
    }
    crate::style::parse_color(&value).ok().flatten()
}

/// The ambient colour `name` of `element`: its own, else its nearest ancestor's.
fn ambient_color(element: &Element, name: &str) -> Option<crate::style::ColorValue> {
    std::iter::successors(Some(element.clone()), |e| e.syntax().parent().and_then(Element::cast)).find_map(|e| literal_color(&e, name))
}

/// The contrast warnings (`vskubuno/docs/EVENTS.md` §16's colour policy): an element that sets a free
/// `ForeColor` or `BackColor` whose text would not stand out enough from its background (WCAG AA) in
/// the light or the dark theme — a non-blocking warning, the view still compiles.
pub fn contrast_warnings(parse: &Parse) -> Vec<Diagnostic> {
    let Some(doc) = Document::cast(parse.syntax()) else { return Vec::new() };
    let Some(root) = doc.root_element() else { return Vec::new() };
    let mut out = Vec::new();
    for element in root.syntax().descendants().filter_map(Element::cast) {
        let own_fore = literal_color(&element, "ForeColor");
        let own_back = literal_color(&element, "BackColor");
        let free_attr = match (&own_fore, &own_back) {
            (Some(f), _) if f.is_free() => "ForeColor",
            (_, Some(b)) if b.is_free() => "BackColor",
            _ => continue,
        };
        let fore = own_fore.or_else(|| ambient_color(&element, "ForeColor"));
        let back = own_back.or_else(|| ambient_color(&element, "BackColor"));
        let font = element.attribute("Font").and_then(|a| a.value()).and_then(|v| crate::style::parse_font(&v).ok().flatten());
        let size = font.as_ref().and_then(|f| f.size_px).unwrap_or(crate::style::BODY_SIZE_PX);
        let bold = font.as_ref().is_some_and(|f| f.bold);
        let findings = crate::style::contrast_warnings(fore.as_ref(), back.as_ref(), size, bold);
        let Some(worst) = findings.iter().min_by(|a, b| a.ratio.total_cmp(&b.ratio)) else { continue };
        let themes: Vec<&str> = findings.iter().map(|f| f.theme.name()).collect();
        let Some(range) = element.attribute(free_attr).and_then(|a| a.name_range()) else { continue };
        let lc = parse.line_col(range.start());
        out.push(Diagnostic {
            range,
            line: lc.line,
            column: lc.column,
            message: format!(
                "the text colour {} on the background {} has a contrast of {:.1}:1 in the {} theme{}; readable text needs at least {}:1 (WCAG AA). Choose a theme colour, or colours that contrast more",
                worst.foreground.hex(),
                worst.background.hex(),
                worst.ratio,
                themes.join(" and "),
                if themes.len() > 1 { "s" } else { "" },
                worst.required,
            ),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::syntax::parse;

    fn diagnostics(src: &str) -> Vec<Diagnostic> {
        validate_with_default_registry(&parse(src))
    }

    #[test]
    fn the_inherited_and_view_properties_are_checked() {
        let ok = diagnostics(r##"<Panel Title="Demo" Opacity="90" StartPosition="CenterScreen"><Button Text="&amp;Save" BackColor="Primary" ForeColor="#FFFFFF" Font="Segoe UI, 12pt, style=Bold" Padding="4, 2, 4, 2" MinimumSize="80, 24" ToolTip="Saves" Enabled="false" TabIndex="2"/></Panel>"##);
        assert!(ok.is_empty(), "{ok:?}");
        let bad = diagnostics(r#"<Panel Opacity="150"><Button BackColor="Chartreusish" Font="Segoe UI, huge" Padding="1, 2" MinimumSize="wide"/><Button Title="x"/></Panel>"#);
        assert_eq!(bad.len(), 6, "{bad:?}");
    }

    #[test]
    fn a_corner_radius_is_a_non_negative_number_of_pixels() {
        assert!(diagnostics(r#"<Panel CornerRadius="16"><FloatingWindow CornerRadius="0"/></Panel>"#).is_empty());
        let bad = diagnostics(r#"<Panel CornerRadius="-2"><FloatingWindow CornerRadius="round"/></Panel>"#);
        assert_eq!(bad.len(), 2, "{bad:?}");
        assert!(bad.iter().any(|d| d.message.contains("not a corner radius")), "{bad:?}");
    }

    #[test]
    fn a_free_colour_that_does_not_contrast_warns_without_blocking() {
        let p = parse(r##"<Panel><Label Text="Faint" ForeColor="#EEEEEE" BackColor="#FFFFFF"/><Label Text="Fine" ForeColor="#202020" BackColor="#FFFFFF"/><Label Text="Tokens" ForeColor="TextPrimary"/></Panel>"##);
        assert!(validate_with_default_registry(&p).is_empty(), "a warning, not an error");
        let w = contrast_warnings(&p);
        assert_eq!(w.len(), 1, "{w:?}");
        assert!(w[0].message.contains("#EEEEEE") && w[0].message.contains("4.5:1"), "{}", w[0].message);
    }

    #[test]
    fn well_formed_view_has_no_diagnostics() {
        let d = diagnostics(r#"<Button Text="Ok" Variant="Primary" Dock="Left"/>"#);
        assert!(d.is_empty(), "{d:?}");
    }

    #[test]
    fn unknown_element_is_reported() {
        let d = diagnostics(r#"<Frobnicator/>"#);
        assert_eq!(d.len(), 1);
        assert!(d[0].message.contains("unknown element"), "{}", d[0].message);
        assert_eq!((d[0].line, d[0].column), (1, 2)); // right after `<`
    }

    #[test]
    fn close_names_and_a_binding_attribute_get_a_suggestion() {
        let d = diagnostics(r#"<Panel><Labl Text="a"/><Label Binding="Name"/><Button Txt="Ok"/></Panel>"#);
        assert_eq!(d.len(), 3, "{d:?}");
        assert_eq!(d[0].message, "unknown element `<Labl>`; did you mean `<Label>`?");
        assert_eq!(d[1].message, "unknown attribute `Binding` on `<Label>`; did you mean `Text=\"{Binding …}\"`?");
        assert_eq!(d[2].message, "unknown attribute `Txt` on `<Button>`; did you mean `Text`?");
        // Nothing close: no suggestion.
        let d = diagnostics(r#"<Frobnicator/>"#);
        assert_eq!(d[0].message, "unknown element `<Frobnicator>`");
    }

    #[test]
    fn unknown_attribute_is_reported() {
        let d = diagnostics(r#"<Button Colour="red"/>"#);
        assert_eq!(d.len(), 1);
        assert!(d[0].message.contains("unknown attribute `Colour`"), "{}", d[0].message);
    }

    #[test]
    fn enum_typo_is_reported_with_the_value_range() {
        let src = r#"<Button Variant="Primmary"/>"#;
        let d = diagnostics(src);
        assert_eq!(d.len(), 1);
        assert!(d[0].message.contains("Primmary"), "{}", d[0].message);
        assert!(d[0].message.contains("Primary"), "{}", d[0].message); // suggests the real values
        let start: usize = d[0].range.start().into();
        assert_eq!(&src[start..start + 8], "Primmary");
    }

    #[test]
    fn bool_typo_is_reported() {
        let d = diagnostics(r#"<Switch On="yes"/>"#);
        assert_eq!(d.len(), 1);
        assert!(d[0].message.contains("true` or `false`"), "{}", d[0].message);
    }

    #[test]
    fn numeric_typo_is_reported() {
        let d = diagnostics(r#"<Stack Width="wide"><Button Text="Ok"/></Stack>"#);
        assert_eq!(d.len(), 1);
        assert!(d[0].message.contains("expected a number"), "{}", d[0].message);
    }

    #[test]
    fn event_attributes_are_accepted_as_handler_names() {
        let d = diagnostics(r#"<Switch OnToggled="offline_toggled"/>"#);
        assert!(d.is_empty(), "{d:?}");
    }

    #[test]
    fn common_events_and_aliases_are_accepted_and_view_events_only_on_the_root() {
        let d = diagnostics(r#"<Panel OnLoad="loaded" OnShown="shown"><Button OnMouseDown="down" OnKeyPress="key" OnValidating="check"/><TextField OnChanged="old" OnTextChanged="new"/></Panel>"#);
        assert!(d.is_empty(), "{d:?}");
        let d = diagnostics(r#"<Panel><Button OnLoad="nope"/></Panel>"#);
        assert_eq!(d.len(), 1, "{d:?}");
        assert!(d[0].message.contains("unknown attribute `OnLoad`"), "{}", d[0].message);
    }

    #[test]
    fn an_alias_is_a_hint_naming_the_new_event() {
        let p = parse(r#"<Panel><Switch OnToggled="x"/><Switch OnCheckedChanged="y"/></Panel>"#);
        assert!(validate_with_default_registry(&p).is_empty());
        let h = hints(&p);
        assert_eq!(h.len(), 1, "{h:?}");
        assert_eq!(h[0].message, "`OnToggled` is an older name for `OnCheckedChanged`");
    }

    #[test]
    fn x_name_and_dock_are_always_allowed() {
        let d = diagnostics(r#"<Button x:Name="go" Dock="Fill" Anchor="Top,Left"/>"#);
        assert!(d.is_empty(), "{d:?}");
    }

    #[test]
    fn dock_and_anchor_outside_a_panel_are_warnings_not_errors() {
        let p = parse(r#"<Stack><Button Dock="Top" Anchor="Top, Left"/><Panel><Button Dock="Fill"/><Button Dock="None"/></Panel></Stack>"#);
        assert!(validate_with_default_registry(&p).is_empty());
        let w = warnings(&p);
        assert_eq!(w.len(), 2, "{w:?}");
        assert!(w[0].message.contains("`Dock` only applies"), "{}", w[0].message);
        assert!(w[1].message.contains("`Anchor` only applies"), "{}", w[1].message);
    }

    #[test]
    fn design_size_attributes_are_accepted_on_the_root() {
        let d = diagnostics(r#"<Card DesignWidth="640" DesignHeight="480"><Stack/></Card>"#);
        assert!(d.is_empty(), "{d:?}");
    }

    #[test]
    fn design_size_attributes_are_type_checked() {
        let d = diagnostics(r#"<Card DesignWidth="wide"/>"#);
        assert_eq!(d.len(), 1);
        assert!(d[0].message.contains("expected a number"), "{}", d[0].message);
    }

    #[test]
    fn design_size_attributes_are_rejected_below_the_root() {
        let d = diagnostics(r#"<Stack><Button DesignWidth="10"/></Stack>"#);
        assert_eq!(d.len(), 1);
        assert!(d[0].message.contains("only valid on the root element"), "{}", d[0].message);
    }

    #[test]
    fn binding_expressions_skip_static_type_checking() {
        let d = diagnostics(r#"<Switch On="{Binding Notifications, Mode=TwoWay}"/>"#);
        assert!(d.is_empty(), "{d:?}");
    }

    #[test]
    fn childless_component_rejects_children() {
        let d = diagnostics(r#"<Switch><Button Text="no"/></Switch>"#);
        assert_eq!(d.len(), 1);
        assert!(d[0].message.contains("does not accept children"), "{}", d[0].message);
    }

    #[test]
    fn single_widget_rejects_a_second_child() {
        let d = diagnostics(r#"<Card><Button Text="a"/><Button Text="b"/></Card>"#);
        assert_eq!(d.len(), 1);
        assert!(d[0].message.contains("at most one child"), "{}", d[0].message);
    }

    #[test]
    fn list_children_model_allows_many() {
        let d = diagnostics(r#"<Stack><Button Text="a"/><Button Text="b"/><Button Text="c"/></Stack>"#);
        assert!(d.is_empty(), "{d:?}");
    }

    #[cfg(feature = "family-containers")]
    #[test]
    fn tab_item_is_rejected_outside_tabs() {
        // §3's requirement: a gated child (here `<TabItem>`, valid only
        // directly inside `<Tabs>`) is rejected under ANY other parent, not
        // just when that other parent is itself another restrictive `List` —
        // `<Stack>`'s own `allowed` is empty (an ordinary widget container),
        // which is exactly the case that would slip through a check that
        // only compared allow-lists between two `List` parents.
        let d = diagnostics(r#"<Stack><TabItem Header="Oops"><Button Text="x"/></TabItem></Stack>"#);
        assert_eq!(d.len(), 1, "{d:?}");
        assert!(d[0].message.contains("<TabItem>"), "{}", d[0].message);
        assert!(d[0].message.contains("<Tabs>"), "{}", d[0].message);
    }

    #[cfg(feature = "family-containers")]
    #[test]
    fn tab_item_is_accepted_directly_inside_tabs() {
        let d = diagnostics(r#"<Tabs><TabItem Header="Un"><Button Text="x"/></TabItem></Tabs>"#);
        assert!(d.is_empty(), "{d:?}");
    }

    #[cfg(all(feature = "family-containers", feature = "family-text"))]
    #[test]
    fn option_is_rejected_outside_dropdown_or_combo_box() {
        let d = diagnostics(r#"<Stack><Option Value="a" Label="A"/></Stack>"#);
        assert_eq!(d.len(), 1, "{d:?}");
        assert!(d[0].message.contains("<Option>"), "{}", d[0].message);
        assert!(d[0].message.contains("<Dropdown>") || d[0].message.contains("<ComboBox>"), "{}", d[0].message);
    }

    #[cfg(feature = "family-data")]
    #[test]
    fn item_is_rejected_outside_a_list_control() {
        let d = diagnostics(r#"<Stack><Item Text="a"/></Stack>"#);
        assert_eq!(d.len(), 1, "{d:?}");
        assert!(d[0].message.contains("<Item>"), "{}", d[0].message);
    }

    #[cfg(feature = "family-containers")]
    #[test]
    fn accordion_section_is_rejected_as_a_single_widget_body() {
        // Exercises the non-`List` side: `<Card>` is `SingleWidget`, whose
        // own cardinality check ("at most one child") does not fire for
        // exactly one child — the gated check must still catch it.
        let d = diagnostics(r#"<Card><AccordionSection Header="Oops"/></Card>"#);
        assert_eq!(d.len(), 1, "{d:?}");
        assert!(d[0].message.contains("<AccordionSection>"), "{}", d[0].message);
        assert!(d[0].message.contains("<Accordion>"), "{}", d[0].message);
    }

    #[test]
    fn mismatched_closing_tag_is_reported() {
        let d = diagnostics(r#"<Button Text="a"></Switch>"#);
        assert_eq!(d.len(), 1);
        assert!(d[0].message.contains("mismatched closing tag"), "{}", d[0].message);
    }

    #[test]
    fn nested_unknown_elements_are_all_reported() {
        let d = diagnostics(r#"<Stack><Frobnicator/><Whatsit/></Stack>"#);
        assert_eq!(d.len(), 2);
    }

    #[test]
    fn worked_example_settings_view_is_clean() {
        // `XML_VIEWS.md` §7's worked example, trimmed to what phase 2a's
        // five-component registry actually covers (`Panel`, `ScrollArea`,
        // `RadioButtons`/`RadioOption`, `NumericField` are not declared yet
        // — see the crate's top-level docs for why. This still exercises
        // Card/Stack/Switch/TextField together, nested, with bindings, Dock
        // and `x:Name` all at once.
        let src = r#"
            <Card Title="Réglages">
              <Stack Direction="TopDown" Gap="0">
                <Switch Dock="Right" x:Name="notifications"
                        On="{Binding Notifications, Mode=TwoWay}"/>
                <Switch Dock="Right" x:Name="offline" On="{Binding Offline}"
                        OnToggled="offline_toggled"/>
                <TextField Dock="Right" x:Name="proxy" Width="320"
                           Placeholder="http://hôte:port (aucun)"
                           Text="{Binding Proxy, Mode=TwoWay}"/>
              </Stack>
            </Card>
        "#;
        let p = parse(src);
        assert!(p.diagnostics.is_empty(), "{:?}", p.diagnostics);
        let d = validate_with_default_registry(&p);
        assert!(d.is_empty(), "{d:?}");
    }
}
