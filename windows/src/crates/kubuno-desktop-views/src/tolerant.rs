//! Tolerant compilation for the designer (vskubuno `docs/DESIGNER.md` §17): the design surface
//! always shows the best preview of a view it can, never a blank page.
//!
//! [`crate::compile`] is a gate: one unknown element, one bad attribute value, and the whole view is
//! refused. That is right for an application (a broken view must not run), not for a designer, where
//! the view is broken half of the time while it is being typed. [`compile_tolerant`] takes the same
//! text and gets past every finding of [`crate::validate`] with the smallest change that removes it
//! ([`crate::validate::Repair`]), then builds the view with the very same strict pipeline:
//!
//! - an unknown element (a typo, a control of a newer runtime than the preview's, a project control
//!   not built yet) and an element its parent cannot hold become a **placeholder** at their place: a
//!   hatched, dashed box with the element's name, selectable and movable like any element, with its
//!   children still built inside it ([`PLACEHOLDER_ELEMENT`]);
//! - an unknown attribute or a value that does not parse is **ignored** (the property keeps its
//!   default) and the element gets a warning marker ([`DesignIssue`]);
//! - a binding or a value a component's builder refuses is dropped the same way, and retried;
//! - children an element cannot hold are not shown (only trailing ones: no other element moves).
//!
//! Every change keeps the element tree's shape, so the element ids (paths of child ordinals,
//! [`crate::ast::Element::stable_id`]) of the preview stay those of the text Visual Studio holds:
//! selection, drags and edits keep working on a view with errors.
//!
//! A text that is not even well-formed XML (an unterminated tag while typing) is the one case where
//! nothing is rebuilt: the caller keeps showing its last good preview ([`crate::runtime::Runtime`]).
//! When there is none yet (a broken file opened), the elements the error-tolerant parser recovered
//! are rebuilt into a well-formed text first ([`recover_text`]).

use std::borrow::Cow;
use std::collections::HashSet;

use kubuno_desktop_ui::{Canvas, Rect, Size};
use rowan::{NodeOrToken, TextRange, TextSize};

use crate::ast::{AstNode, Document, Element};
use crate::binding::ViewModel;
use crate::compile::{self, CompiledView};
use crate::node::{PaintCx, ViewNode};
use crate::props::{BuildCx, BuildError, Props};
use crate::registry::{self, ChildrenModel, ComponentMeta, LayoutKind};
use crate::syntax::{self, Diagnostic, Parse, SyntaxKind, SyntaxNode};
use crate::validate::{self, Repair};

/// The element name of a placeholder whose children are placed by Dock/Anchor (`X`/`Y`), like a
/// `<Panel>`. Reserved: it only ever appears in a text this module rewrote, never in a listing.
pub const PLACEHOLDER_ELEMENT: &str = "__DesignPlaceholder";
/// The element name of a placeholder whose children flow one after another, like a `<Stack>`.
pub const PLACEHOLDER_FLOW_ELEMENT: &str = "__DesignPlaceholderFlow";
/// The placeholder's attribute holding the name of the element it stands for.
pub const PLACEHOLDER_NAME_ATTRIBUTE: &str = "__Element";
/// The placeholder's attribute holding why the element is not shown.
pub const PLACEHOLDER_REASON_ATTRIBUTE: &str = "__Reason";

/// How many rounds of repairs a view gets: one is the common case (validation finds everything at
/// once); a builder refusing a value, or a child its new placeholder parent gates, take one more.
const MAX_ROUNDS: usize = 12;

/// The attributes a placeholder keeps: where its parent places it, and its name.
const PLACEMENT_ATTRIBUTES: &[&str] = &["x:Name", "X", "Y", "Width", "Height", "Dock", "Anchor", "DesignWidth", "DesignHeight"];

pub(crate) static PLACEHOLDER_META: ComponentMeta = ComponentMeta {
    name: PLACEHOLDER_ELEMENT,
    doc: "",
    properties: &[],
    events: &[],
    children: ChildrenModel::List(&[]),
    layout: LayoutKind::DockAnchor,
    open_attributes: true,
    default_event: None,
    build: placeholder_build,
};

pub(crate) static PLACEHOLDER_FLOW_META: ComponentMeta = ComponentMeta {
    name: PLACEHOLDER_FLOW_ELEMENT,
    doc: "",
    properties: &[],
    events: &[],
    children: ChildrenModel::List(&[]),
    layout: LayoutKind::Flow,
    open_attributes: true,
    default_event: None,
    build: placeholder_build,
};

/// The registry entry of a placeholder element name, `None` for any other name.
pub(crate) fn placeholder_meta(name: &str) -> Option<&'static ComponentMeta> {
    match name {
        PLACEHOLDER_ELEMENT => Some(&PLACEHOLDER_META),
        PLACEHOLDER_FLOW_ELEMENT => Some(&PLACEHOLDER_FLOW_META),
        _ => None,
    }
}

/// Whether `name` is a placeholder element's.
pub fn is_placeholder(name: &str) -> bool {
    placeholder_meta(name).is_some()
}

/// An element the preview shows differently from what its text says: a placeholder, an attribute
/// ignored, children not shown. The design surface marks it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesignIssue {
    /// The element's id ([`crate::ast::Element::stable_id`]).
    pub element_id: String,
    pub message: String,
    /// The element is shown as a placeholder (which says so itself: no marker needed).
    pub placeholder: bool,
    /// The attribute in question, when it is one.
    pub attribute: Option<String>,
    /// Where the finding is in the text as written (the attribute's value or name, else the element's
    /// name): what a click on the element's marker selects in the XML. `None` when the element is not
    /// found there (a view rebuilt from a malformed text).
    pub range: Option<TextRange>,
}

/// What [`compile_tolerant`] made of a text.
pub struct TolerantCompile {
    /// The view, unless nothing could be built (`malformed` without recovery, or no element at all).
    pub view: Option<CompiledView>,
    /// The diagnostics of the text as written (parse and validation, then what a builder refused),
    /// positioned in that text.
    pub diagnostics: Vec<Diagnostic>,
    /// The elements shown differently from their text.
    pub issues: Vec<DesignIssue>,
    /// The text is not well-formed XML.
    pub malformed: bool,
    /// The view was rebuilt from what the parser recovered of a malformed text.
    pub recovered: bool,
    /// The text the view was finally built from (the repaired one).
    pub(crate) built_text: String,
}

/// Compiles `text` for the designer (see the module doc). `reuse` hands the instances of named
/// components a hot reload keeps (it is called once per build attempt). `recover_malformed`: rebuild a
/// text that is not well-formed from what the parser recovered (no previous preview to keep), instead
/// of giving up.
pub(crate) fn compile_tolerant(
    text: &str,
    base_dir: Option<&std::path::Path>,
    reuse: &dyn Fn() -> crate::scope::Reusable,
    recover_malformed: bool,
) -> TolerantCompile {
    let reg = registry::all();
    // What the user sees: the findings of the text as written.
    let written = syntax::parse(text);
    let mut diagnostics = written.diagnostics.clone();
    diagnostics.extend(validate::validate(&written, reg));
    let malformed = !written.diagnostics.is_empty();
    let mut result = TolerantCompile { view: None, diagnostics: Vec::new(), issues: Vec::new(), malformed, recovered: false, built_text: String::new() };

    let prepared = match compile::prepare_text(text, base_dir) {
        Ok(t) => t,
        Err(d) => {
            diagnostics.insert(0, d);
            Cow::Borrowed(text)
        }
    };
    let prepared_parse = syntax::parse(&prepared);
    let mut current = if prepared_parse.diagnostics.is_empty() {
        prepared.into_owned()
    } else if recover_malformed {
        match recover_text(&prepared_parse) {
            Some(t) => {
                result.recovered = true;
                t
            }
            None => {
                result.diagnostics = diagnostics;
                return result;
            }
        }
    } else {
        result.diagnostics = diagnostics;
        return result;
    };

    let written_doc = Document::cast(written.syntax());
    for _ in 0..MAX_ROUNDS {
        let parse = syntax::parse(&current);
        if !parse.diagnostics.is_empty() {
            break; // A repair broke the text (should not happen): give up rather than loop.
        }
        let mut found = validate::validate_with_repairs(&parse, reg);
        if found.is_empty() {
            match compile::compile_prepared(&current, reg, base_dir, reuse()) {
                Ok(view) => {
                    result.view = Some(view);
                    result.built_text = current;
                    break;
                }
                Err(errors) => {
                    for error in errors {
                        let Some(repair) = build_repair(&parse, error.range) else { continue };
                        // A builder's refusal is a finding of its own: shown where it is in the text as written.
                        if let Some(d) = written_position(&parse, written_doc.as_ref(), &written, &error) {
                            if !diagnostics.iter().any(|x| x.range == d.range && x.message == d.message) {
                                diagnostics.push(d);
                            }
                        }
                        found.push((error, repair));
                    }
                }
            }
        }
        let edits = edits_for(&parse, &current, &found, &mut result.issues);
        if edits.is_empty() {
            break;
        }
        current = apply_edits(&current, edits);
    }
    // The issues, located in the text as written (ids are kept by every repair).
    if let Some(doc) = written_doc.as_ref().filter(|_| !result.recovered) {
        for issue in &mut result.issues {
            let Some(element) = doc.resolve_id(&issue.element_id) else { continue };
            issue.range = match issue.attribute.as_deref().and_then(|a| element.attribute(a)) {
                Some(a) => a.value_range().filter(|r| !r.is_empty()).or_else(|| a.name_range()),
                None => element.name_range(),
            };
        }
    }
    result.diagnostics = diagnostics;
    result
}

/// A builder's diagnostic (positioned in the rewritten text) moved onto the text as written: the same
/// attribute (or element name) of the element with the same id.
fn written_position(parse: &Parse, written_doc: Option<&Document>, written: &Parse, error: &Diagnostic) -> Option<Diagnostic> {
    let node = node_at(parse, error.range)?;
    let element = node.ancestors().find_map(Element::cast)?;
    let attribute_name = node.ancestors().find_map(crate::ast::Attribute::cast).and_then(|a| a.name());
    let target = written_doc?.resolve_id(&element.stable_id())?;
    let range = match attribute_name {
        Some(name) => target.attribute(&name).and_then(|a| a.value_range().or_else(|| a.name_range())),
        None => target.name_range(),
    }?;
    let lc = written.line_col(range.start());
    Some(Diagnostic { range, line: lc.line, column: lc.column, message: error.message.clone() })
}

/// The innermost node covering `range` (a token's parent).
fn node_at(parse: &Parse, range: TextRange) -> Option<SyntaxNode> {
    let root = parse.syntax();
    if !root.text_range().contains_range(range) {
        return None;
    }
    match root.covering_element(range) {
        NodeOrToken::Node(n) => Some(n),
        NodeOrToken::Token(t) => t.parent(),
    }
}

/// How to get past a builder's refusal at `range`: drop the attribute it is in, else show its
/// element as a placeholder. `None` when there is nothing sensible to change (the placeholder itself,
/// or no element at all): the caller then gives up.
fn build_repair(parse: &Parse, range: TextRange) -> Option<Repair> {
    let node = node_at(parse, range)?;
    for ancestor in node.ancestors() {
        match ancestor.kind() {
            SyntaxKind::ATTRIBUTE => return Some(Repair::DropAttribute(ancestor.text_range())),
            SyntaxKind::ELEMENT => {
                let element = Element::cast(ancestor.clone())?;
                if element.name().is_some_and(|n| is_placeholder(&n)) {
                    return None;
                }
                return Some(Repair::Placeholder(ancestor.text_range()));
            }
            _ => {}
        }
    }
    None
}

/// The element node whose whole range is `range`.
fn element_with_range(parse: &Parse, range: TextRange) -> Option<Element> {
    node_at(parse, range)?.ancestors().filter_map(Element::cast).find(|e| e.syntax().text_range() == range)
}

/// `range` widened over the whitespace right before it (an attribute removed with its separator).
fn with_leading_space(text: &str, range: TextRange) -> TextRange {
    let start = usize::from(range.start());
    let trimmed = text[..start].trim_end_matches([' ', '\t', '\r', '\n']);
    TextRange::new(TextSize::from(u32::try_from(trimmed.len()).unwrap_or(0)), range.end())
}

/// An attribute value as XML text (quotes, `&`, `<` escaped).
fn escape_attribute(value: &str) -> String {
    value.replace('&', "&amp;").replace('<', "&lt;").replace('"', "&quot;")
}

/// Whether a placeholder keeps `name="value"` (where its parent places it, and only a value its
/// parent can read).
fn keeps_placement(name: &str, value: &str) -> bool {
    if !PLACEMENT_ATTRIBUTES.contains(&name) {
        return false;
    }
    match name {
        "X" | "Y" | "Width" | "Height" | "DesignWidth" | "DesignHeight" => value.trim().parse::<f32>().is_ok(),
        "Dock" => ["None", "Top", "Bottom", "Left", "Right", "Fill"].contains(&value.trim()),
        _ => true,
    }
}

/// The short reason a placeholder shows under the element's name.
fn placeholder_reason(message: &str) -> String {
    if message.starts_with("unknown element") {
        crate::messages::tr(
            "Not available in this preview (a misspelt name, a newer control, or a project control not built yet)",
            "Indisponible dans cet aperçu (nom mal orthographié, contrôle plus récent ou contrôle du projet pas encore généré)",
        )
    } else {
        crate::messages::localize(message)
    }
}

/// The text edits `(range, replacement)` of `repairs` on `text` (parsed as `parse`), recording the
/// elements they change in `issues`.
fn edits_for(parse: &Parse, text: &str, repairs: &[(Diagnostic, Repair)], issues: &mut Vec<DesignIssue>) -> Vec<(TextRange, String)> {
    let mut edits: Vec<(TextRange, String)> = Vec::new();
    let mut issue = |element: Option<Element>, message: &str, placeholder: bool, attribute: Option<String>| {
        if let Some(e) = element {
            let entry = DesignIssue { element_id: e.stable_id(), message: message.to_string(), placeholder, attribute, range: None };
            if !issues.contains(&entry) {
                issues.push(entry);
            }
        }
    };
    for (diagnostic, repair) in repairs {
        match repair {
            Repair::DropAttribute(range) => {
                let owner = node_at(parse, *range).and_then(|n| n.ancestors().find_map(Element::cast));
                let name = node_at(parse, *range).and_then(|n| n.ancestors().find_map(crate::ast::Attribute::cast)).and_then(|a| a.name());
                issue(owner, &format!("{} (ignored in the preview)", diagnostic.message), false, name);
                edits.push((with_leading_space(text, *range), String::new()));
            }
            Repair::RenameEndTag(range, name) => {
                let owner = node_at(parse, *range).and_then(|n| n.ancestors().find_map(Element::cast));
                issue(owner, &diagnostic.message, false, None);
                edits.push((*range, name.clone()));
            }
            Repair::DropElement(range) => {
                let Some(element) = element_with_range(parse, *range) else { continue };
                let parent = element.syntax().parent().and_then(Element::cast);
                issue(parent, &format!("{} (not shown in the preview)", diagnostic.message), false, None);
                // This element and every element after it under the same parent.
                let mut sibling = Some(element.syntax().clone());
                while let Some(node) = sibling {
                    if node.kind() == SyntaxKind::ELEMENT {
                        edits.push((node.text_range(), String::new()));
                    }
                    sibling = node.next_sibling();
                }
            }
            Repair::Placeholder(range) => {
                let Some(element) = element_with_range(parse, *range) else { continue };
                let (Some(name), Some(name_range)) = (element.name(), element.name_range()) else { continue };
                if is_placeholder(&name) {
                    continue;
                }
                issue(Some(element.clone()), &diagnostic.message, true, None);
                // Children placed by X/Y or Dock/Anchor keep their places; any others flow.
                let positioned = element.children().any(|c| ["X", "Y", "Dock", "Anchor"].iter().any(|a| c.attribute(a).is_some()));
                let tag = if positioned { PLACEHOLDER_ELEMENT } else { PLACEHOLDER_FLOW_ELEMENT };
                edits.push((
                    name_range,
                    format!(
                        "{tag} {PLACEHOLDER_NAME_ATTRIBUTE}=\"{}\" {PLACEHOLDER_REASON_ATTRIBUTE}=\"{}\"",
                        escape_attribute(&name),
                        escape_attribute(&placeholder_reason(&diagnostic.message))
                    ),
                ));
                if let Some(end) = element.end_name_range() {
                    edits.push((end, tag.to_string()));
                }
                for attribute in element.attributes() {
                    let keep = match (attribute.name(), attribute.value()) {
                        (Some(n), Some(v)) => keeps_placement(&n, &v),
                        _ => false,
                    };
                    if !keep {
                        edits.push((with_leading_space(text, attribute.syntax().text_range()), String::new()));
                    }
                }
            }
        }
    }
    edits
}

/// `text` with `edits` applied. Overlapping edits keep the first, widest one (an element removed
/// takes everything inside it with it).
fn apply_edits(text: &str, mut edits: Vec<(TextRange, String)>) -> String {
    edits.sort_by(|a, b| a.0.start().cmp(&b.0.start()).then(b.0.len().cmp(&a.0.len())));
    let mut out = String::with_capacity(text.len());
    let mut at = 0usize;
    for (range, replacement) in edits {
        let (start, end) = (usize::from(range.start()), usize::from(range.end()));
        if start < at || end > text.len() || !text.is_char_boundary(start) || !text.is_char_boundary(end) {
            continue;
        }
        out.push_str(&text[at..start]);
        out.push_str(&replacement);
        at = end;
    }
    out.push_str(&text[at..]);
    out
}

/// A well-formed text of the elements the error-tolerant parser recovered from a malformed one
/// (their names, their attributes with a complete value, their text): what a broken file opened in
/// the designer is rebuilt from. `None` when there is no root element at all.
pub(crate) fn recover_text(parse: &Parse) -> Option<String> {
    let root = Document::cast(parse.syntax())?.root_element()?;
    let mut out = String::new();
    write_element(&root, &mut out).then_some(out)
}

fn write_element(element: &Element, out: &mut String) -> bool {
    let Some(name) = element.name() else { return false };
    out.push('<');
    out.push_str(&name);
    let mut seen = HashSet::new();
    for attribute in element.attributes() {
        if let (Some(n), Some(v)) = (attribute.name(), attribute.value()) {
            if seen.insert(n.clone()) {
                out.push(' ');
                out.push_str(&n);
                out.push_str("=\"");
                out.push_str(&escape_attribute(&v));
                out.push('"');
            }
        }
    }
    let children: Vec<Element> = element.children().collect();
    let body = crate::ast::decode_entities(&element.text());
    if children.is_empty() && body.trim().is_empty() {
        out.push_str("/>");
        return true;
    }
    out.push('>');
    if !body.trim().is_empty() {
        out.push_str(&body.replace('&', "&amp;").replace('<', "&lt;"));
    }
    for child in &children {
        write_element(child, out);
    }
    out.push_str("</");
    out.push_str(&name);
    out.push('>');
    true
}

// ── The placeholder node ────────────────────────────────────────────────────────────────────

/// The `build` of the placeholder elements: the element's name and reason, and its children laid
/// out like a `<Panel>` (Dock/Anchor) or a `<Stack>` (flow).
fn placeholder_build(props: &Props<'_>, cx: &mut BuildCx) -> Result<Box<dyn ViewNode>, BuildError> {
    let element = props.element();
    let read = |name: &str| element.attribute(name).and_then(|a| a.value()).unwrap_or_default();
    let inner = if element.children().next().is_some() {
        let host = if props.meta().layout == LayoutKind::Flow { "Stack" } else { "Panel" };
        let meta = registry::lookup(host).ok_or_else(|| BuildError::new(format!("`<{host}>` is not registered"), element.name_range()))?;
        Some((meta.build)(&Props::new(element, meta), cx)?)
    } else {
        None
    };
    Ok(Box::new(PlaceholderNode { name: read(PLACEHOLDER_NAME_ATTRIBUTE), reason: read(PLACEHOLDER_REASON_ATTRIBUTE), inner }))
}

/// An element the preview cannot show as itself (see the module doc).
struct PlaceholderNode {
    name: String,
    reason: String,
    /// Its children, laid out (`None` without children).
    inner: Option<Box<dyn ViewNode>>,
}

/// The height of a placeholder's name chip, DIP.
const CHIP_HEIGHT: f32 = 20.0;
/// The space between a placeholder's box and its children, DIP.
const CONTENT_PADDING: f32 = 4.0;

impl ViewNode for PlaceholderNode {
    fn measure(&self, c: &dyn Canvas, vm: &dyn ViewModel) -> Size {
        // Never smaller than 120 x 40 when its parent sizes it by its content: its name stays readable.
        let own = Size { width: 160.0, height: 40.0 };
        match &self.inner {
            Some(inner) => {
                let s = inner.measure(c, vm);
                Size { width: (s.width + 2.0 * CONTENT_PADDING).max(own.width), height: (s.height + CHIP_HEIGHT + 2.0 * CONTENT_PADDING).max(own.height) }
            }
            None => own,
        }
    }

    fn paint(&mut self, cx: &mut PaintCx<'_>, bounds: Rect) {
        let c = cx.canvas;
        if bounds.right - bounds.left < 2.0 || bounds.bottom - bounds.top < 2.0 {
            return;
        }
        let theme = c.theme();
        paint_hatch(c, bounds, theme.text_secondary);
        for dash in crate::design::dashed_outline(bounds, 4.0, 3.0, 1.0) {
            c.fill_rect(&dash, &theme.text_secondary);
        }
        // The children: inside the box, under the name chip, padded and clipped to it.
        if let Some(inner) = self.inner.as_mut() {
            let body = Rect::new(
                bounds.left + CONTENT_PADDING,
                (bounds.top + CHIP_HEIGHT + CONTENT_PADDING).min(bounds.bottom),
                (bounds.right - CONTENT_PADDING).max(bounds.left),
                (bounds.bottom - CONTENT_PADDING).max(bounds.top),
            );
            if body.right > body.left && body.bottom > body.top {
                cx.canvas.push_clip(&body);
                inner.paint(cx, body);
                cx.canvas.pop_clip();
            }
        }
        let c = cx.canvas;
        let label = format!("<{}>", self.name);
        let format = &c.formats().caption;
        let label_width = c.measure(&label, format) + 30.0;
        if bounds.right - bounds.left >= label_width && bounds.bottom - bounds.top >= CHIP_HEIGHT {
            // The chip: an icon and the element's name, then why it is not shown (when there is room).
            let chip = Rect::new(bounds.left + 1.0, bounds.top + 1.0, bounds.right - 1.0, bounds.top + CHIP_HEIGHT);
            c.fill_rect(&chip, &theme.window_background);
            c.vector_icon("Puzzle", &Rect::new(chip.left + 4.0, chip.top + 3.0, chip.left + 18.0, chip.top + 17.0), 14.0, &theme.warning);
            c.text(&label, &Rect::new(chip.left + 22.0, chip.top + 1.0, chip.right - 4.0, chip.bottom), format, &theme.text_primary, false);
            if self.inner.is_none() && bounds.bottom - bounds.top >= CHIP_HEIGHT + 16.0 {
                let hint = Rect::new(bounds.left + 8.0, bounds.top + CHIP_HEIGHT, bounds.right - 6.0, bounds.bottom - 2.0);
                c.text_ellipsis(&self.reason, &hint, &c.formats().caption, &theme.text_secondary);
            }
        } else {
            // Too small for its name: the icon in the box, and the whole name on a tag right under it
            // (never cut, whatever the element's own size).
            let side = 14.0f32.min(bounds.right - bounds.left - 2.0).min(bounds.bottom - bounds.top - 2.0).max(0.0);
            let (cx0, cy0) = ((bounds.left + bounds.right - side) / 2.0, (bounds.top + bounds.bottom - side) / 2.0);
            c.vector_icon("Puzzle", &Rect::new(cx0, cy0, cx0 + side, cy0 + side), side, &theme.warning);
            let tag = Rect::new(bounds.left, bounds.bottom + 2.0, bounds.left + label_width - 8.0, bounds.bottom + 2.0 + CHIP_HEIGHT);
            c.fill_rounded(&tag, 3.0, &theme.window_background);
            c.stroke_rounded(&tag, 3.0, &theme.text_secondary);
            c.text(&label, &Rect::new(tag.left + 6.0, tag.top + 1.0, tag.right, tag.bottom), format, &theme.text_primary, false);
        }
    }
}

/// Faint diagonal stripes over `bounds` (the placeholder's hatching).
fn paint_hatch(c: &dyn kubuno_desktop_controls::ControlCanvas, bounds: Rect, color: windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F) {
    let mut stripe = color;
    stripe.a = 0.10;
    c.push_clip(&bounds);
    let (w, h) = (bounds.right - bounds.left, bounds.bottom - bounds.top);
    let step = 10.0;
    let thickness = 3.0;
    let mut x = -h;
    while x < w {
        // One stripe: the parallelogram from (x, bottom) up to (x + h, top), `thickness` wide.
        let (x0, x1) = (bounds.left + x, bounds.left + x + h);
        c.fill_triangle((x0, bounds.bottom), (x1, bounds.top), (x1 + thickness, bounds.top), &stripe);
        c.fill_triangle((x0, bounds.bottom), (x1 + thickness, bounds.top), (x0 + thickness, bounds.bottom), &stripe);
        x += step;
    }
    c.pop_clip();
}

#[cfg(test)]
#[path = "tolerant_tests.rs"]
mod tests;
