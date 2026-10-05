//! The editor features of a web view (`vskubuno/docs/WEB-VIEWS.md` §5, WV-7): diagnostics, completion, hover,
//! go-to-definition, code actions, binding paths, the registry as the designer reads it, and the generated `.d.ts`.
//!
//! **Diagnostics** are the web compiler's own (`kubuno-web-views-compiler-core`: the same codes and messages as
//! `@kubuno/views-compiler` and `kbview-tsc`, module isolation included), plus what only an editor can add:
//!
//! | Code | Severity | What |
//! |---|---|---|
//! | `module-isolation` | error | an element rendered by another module (neither a host singleton nor a file of this project), at the element; an import of another module in the code-behind |
//! | `missing-handler` / `incompatible-handler` | warning | an `On…` naming no method of the code-behind, or one whose parameters cannot take the event |
//! | `binding-member` | warning | `{Binding x…}` whose `x` is neither a member of the code-behind class nor an `x:Name` (outside item templates, when the class declares no `dataContext`) |
//! | `unknown-resource` | warning | `{Res key}` found in none of the project's locale bundles and `.kbres` files |
//! | `web-class` | warning | the web-only `Class` attribute (tolerated during the migration, counted) |
//! | `colour-literal` | warning | a colour property set to a literal (`#fff`, `rgb(…)`) instead of a theme token |
//! | `tab-index` | warning | a positive `TabIndex` (the DOM order is the tab order on the web) |
//! | `absolute-only` | warning | `X` / `Y` / `Anchor` outside a `Layout="Absolute"` container |
//! | `accessible-name` | warning | an `IconButton` without `AccessibleName` (nor `ToolTip`) |
//! | `query-key` | warning | a `Query` without `Key` |
//! | `xmlns` | information | an undeclared `x:` / `d:` prefix (shared with the desktop, quick fix included) |
//!
//! Types stay with TypeScript: wrong binding and handler types come from `kbview-tsc` (the generated check files),
//! not from this server.

use std::path::Path;

use kubuno_desktop_views::ast::{AstNode, Attribute, Document as AstDocument, Element};
use kubuno_desktop_views::syntax::{SyntaxKind, SyntaxToken};
use kubuno_desktop_views_model::schema::ChildrenModelJson;
use kubuno_desktop_views_model::{PropKindEntry, PropertyEntry};
use kubuno_desktop_views_syntax::binding::{binding_parts, is_binding_expr, parse_binding_syntax, BINDING_KEYS};
use kubuno_desktop_views_syntax::res::{parse_res_path, res_reference};
use kubuno_web_views_compiler_core::compile::RESERVED_MEMBERS;
use kubuno_web_views_compiler_core::registry::{is_project_module, Element as WebElement, Origin, WebRegistry, HOST_MODULES};
use lsp_types::{
    CodeAction, CodeActionKind, CompletionItem, CompletionItemKind, Diagnostic, DiagnosticSeverity, Documentation, Hover, HoverContents, Location, MarkupContent,
    MarkupKind, NumberOrString, Position, Range, TextEdit, Uri, WorkspaceEdit,
};
use rowan::TextSize;

use super::handlers::{self, CodeBehind};
use super::project::{self, WebProject};
use super::res;
use super::{lsp_range, WebView};
use crate::documents::Document;
use crate::tree;

const SOURCE: &str = "kubuno-web-views";

/// The `@kubuno/*` specifiers a code-behind may import (the host singletons, and the compiler's own runtime types).
const HOST_PACKAGES: &[&str] = &["@kubuno/ui", "@kubuno/sdk", "@kubuno/drive", "@kubuno/views", "@kubuno/views-compiler"];

fn french() -> bool {
    std::env::var("KUBUNO_UI_LANG").is_ok_and(|l| l.eq_ignore_ascii_case("fr"))
}

fn doc_of<'a>(doc: &'a str, doc_fr: &'a Option<String>) -> &'a str {
    match doc_fr {
        Some(fr) if french() && !fr.is_empty() => fr,
        _ => doc,
    }
}

fn markdown(value: String) -> Documentation {
    Documentation::MarkupContent(MarkupContent { kind: MarkupKind::Markdown, value })
}

fn diag(range: Range, severity: DiagnosticSeverity, code: &str, message: String) -> Diagnostic {
    Diagnostic { range, severity: Some(severity), code: Some(NumberOrString::String(code.to_string())), source: Some(SOURCE.to_string()), message, ..Default::default() }
}

fn text_range(doc: &Document, r: rowan::TextRange) -> Range {
    lsp_range(&doc.text, u32::from(r.start()) as usize, u32::from(r.end()) as usize)
}

fn element_name_range(doc: &Document, el: &Element) -> Range {
    el.name_range().map(|r| text_range(doc, r)).unwrap_or_else(|| text_range(doc, el.syntax().text_range()))
}

fn is_root(element: &Element) -> bool {
    element.syntax().parent().is_none_or(|p| p.kind() != SyntaxKind::ELEMENT)
}

fn elements(doc: &Document) -> Vec<Element> {
    AstDocument::cast(doc.parse.syntax()).and_then(|d| d.root_element()).map(|r| r.syntax().descendants().filter_map(Element::cast).collect()).unwrap_or_default()
}

/// Whether `el` sits inside an item template (a `Repeater`'s children): its bindings resolve against the row first.
fn in_template(registry: &WebRegistry, el: &Element) -> bool {
    el.syntax().ancestors().skip(1).filter_map(Element::cast).any(|a| a.name().and_then(|n| registry.get(&n).map(|m| m.web.template)).unwrap_or(false))
}

/// The modules an element is rendered by (its own and its alternates').
fn modules_of(meta: &WebElement) -> Vec<String> {
    std::iter::once(meta.web.module.clone()).chain(meta.web.alternates.iter().map(|a| a.module.clone())).flatten().collect()
}

// ── diagnostics ─────────────────────────────────────────────────────────

/// Every diagnostic of the web view `doc`.
pub fn diagnostics(doc: &Document, view: &WebView) -> Vec<Diagnostic> {
    project::with_project(&view.root, |project| {
        let mut out = Vec::new();
        let root_el = AstDocument::cast(doc.parse.syntax()).and_then(|d| d.root_element());
        let root_range = root_el.as_ref().map(|r| element_name_range(doc, r)).unwrap_or_default();
        for problem in &project.load_problems {
            out.push(diag(root_range, DiagnosticSeverity::ERROR, "registry", problem.clone()));
        }
        let compiled = project.compile(&view.path, &doc.text);
        for d in &compiled.diagnostics {
            let pos = |line: u32, col: u32| Position { line: line.saturating_sub(1), character: col.saturating_sub(1) };
            let mut range = Range { start: pos(d.line, d.column), end: pos(d.end_line, d.end_column) };
            // View-level problems (a registry the project loads) are reported at the root's name.
            if d.line == 1 && d.column == 1 && d.end_line == 1 && d.end_column == 1 {
                range = root_range;
            }
            let severity = match d.severity {
                "error" => DiagnosticSeverity::ERROR,
                "warning" => DiagnosticSeverity::WARNING,
                _ => DiagnosticSeverity::INFORMATION,
            };
            out.push(diag(range, severity, d.code, d.message.clone()));
        }
        let registry = &project.session.registry;
        out.extend(isolation(registry, doc));
        out.extend(rules(registry, doc));
        out.extend(handlers::diagnostics(registry, doc, view));
        if let Some(code) = CodeBehind::load(view) {
            out.extend(code_behind_isolation(&code, project, root_range));
            out.extend(binding_members(registry, doc, &code));
        }
        out.extend(resource_diagnostics(doc, project));
        out.extend(crate::namespaces::diagnostics(doc));
        out
    })
}

/// Module isolation at the elements (`VIEWS-SPEC.md` "Module isolation", rule 1 — the compiler's own predicate).
fn isolation(registry: &WebRegistry, doc: &Document) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for el in elements(doc) {
        let Some(name) = el.name() else { continue };
        let Some(meta) = registry.get(&name) else { continue };
        let host = meta.origin == Origin::Host;
        for module in modules_of(meta) {
            if !(HOST_MODULES.contains(&module.as_str()) || (!host && is_project_module(&module))) {
                out.push(diag(
                    element_name_range(doc, &el),
                    DiagnosticSeverity::ERROR,
                    "module-isolation",
                    format!(
                        "`<{name}>` is rendered by `{module}`, another module: a view may only use host elements ({}) and its own module's controls (module isolation)",
                        HOST_MODULES.join(", ")
                    ),
                ));
            }
        }
    }
    out
}

/// The code-behind's imports of another module: an `@kubuno/…` package that is not a host singleton, or a relative
/// path leaving the project.
fn code_behind_isolation(code: &CodeBehind, project: &WebProject, at: Range) -> Vec<Diagnostic> {
    let dir = code.path().parent().map(Path::to_path_buf).unwrap_or_default();
    let mut out = Vec::new();
    for import in &code.file.imports {
        let s = import.source.as_str();
        let foreign_package = s.starts_with("@kubuno/") && !HOST_PACKAGES.contains(&s);
        let outside = (s.starts_with("./") || s.starts_with("../")) && {
            let mut p = dir.clone();
            for part in s.split('/') {
                match part {
                    "." | "" => {}
                    ".." => {
                        p.pop();
                    }
                    other => p.push(other),
                }
            }
            !p.starts_with(&project.root)
        };
        if foreign_package || outside {
            out.push(diag(
                at,
                DiagnosticSeverity::ERROR,
                "module-isolation",
                format!("the code-behind imports `{s}`, which is not a file of this project nor a host singleton (module isolation: modules never import one another; use the core's extension points)"),
            ));
        }
    }
    out
}

/// The editor's web rules (see the module doc).
fn rules(registry: &WebRegistry, doc: &Document) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    let all = elements(doc);
    let class_count = all.iter().filter(|e| e.attribute("Class").is_some()).count();
    for el in &all {
        let Some(name) = el.name() else { continue };
        let meta = registry.get(&name);
        for attr in el.attributes() {
            let Some(aname) = attr.name() else { continue };
            let value = attr.value().unwrap_or_default();
            let name_range = attr.name_range().map(|r| text_range(doc, r)).unwrap_or_default();
            let value_range = attr.value_range().map(|r| text_range(doc, r)).unwrap_or(name_range);
            if aname == "Class" {
                out.push(diag(
                    name_range,
                    DiagnosticSeverity::WARNING,
                    "web-class",
                    format!("`Class` (Tailwind) is tolerated during the migration only: express it with properties and theme tokens ({class_count} in this view)"),
                ));
                continue;
            }
            if is_binding_expr(value.trim()) {
                continue;
            }
            let property = meta.and_then(|m| m.property(&aname));
            if property.and_then(|p| p.editor.as_deref()) == Some("color") {
                let v = value.trim().to_ascii_lowercase();
                if v.starts_with('#') || v.starts_with("rgb") || v.starts_with("hsl") {
                    out.push(diag(value_range, DiagnosticSeverity::WARNING, "colour-literal", format!("`{aname}`: a colour literal; use a theme token (`TextSecondary`, `Primary`…) so the view follows the theme")));
                }
            }
            if aname == "TabIndex" && value.trim().parse::<f64>().is_ok_and(|n| n > 0.0) {
                out.push(diag(value_range, DiagnosticSeverity::WARNING, "tab-index", "a positive `TabIndex` reorders the keyboard focus on the web; keep the document order (0 or -1)".into()));
            }
            if matches!(aname.as_str(), "X" | "Y" | "Anchor") {
                let parent_absolute = el.syntax().parent().and_then(Element::cast).is_some_and(|p| p.attribute("Layout").and_then(|a| a.value()).as_deref() == Some("Absolute"));
                if !parent_absolute {
                    out.push(diag(name_range, DiagnosticSeverity::WARNING, "absolute-only", format!("`{aname}` only applies inside a `Layout=\"Absolute\"` container on the web (layout is flow by default)")));
                }
            }
        }
        if name == "IconButton" && el.attribute("AccessibleName").is_none() && el.attribute("ToolTip").is_none() {
            out.push(diag(element_name_range(doc, el), DiagnosticSeverity::WARNING, "accessible-name", "an icon-only button needs an `AccessibleName` (read by screen readers)".into()));
        }
        if name == "Query" && el.attribute("Key").is_none() {
            out.push(diag(element_name_range(doc, el), DiagnosticSeverity::WARNING, "query-key", "a `Query` needs a `Key` (its React Query cache key)".into()));
        }
    }
    out
}

/// `(attribute, path, byte range of the path)` of every `{Binding …}` of the document, with its element.
fn binding_paths_of(doc: &Document) -> Vec<(Element, Attribute, String, (usize, usize))> {
    let mut out = Vec::new();
    for el in elements(doc) {
        for attr in el.attributes() {
            let Some(raw) = attr.raw_value() else { continue };
            let Some(vr) = attr.value_range() else { continue };
            if raw.len() < 2 {
                continue;
            }
            let inner = &raw[1..raw.len() - 1];
            if !inner.trim_start().starts_with("{Binding") {
                continue;
            }
            let Some(spec) = parse_binding_syntax(inner.trim()) else { continue };
            let Some(parts) = binding_parts(inner) else { continue };
            let start = u32::from(vr.start()) as usize;
            let part = parts.iter().enumerate().find(|(i, p)| (p.key.is_none() && *i == 0) || p.key.as_deref() == Some("Path")).map(|(_, p)| p);
            if let Some(part) = part.filter(|p| p.value == spec.path) {
                out.push((el.clone(), attr.clone(), spec.path.clone(), (start + part.value_range.start, start + part.value_range.end)));
            }
        }
    }
    out
}

/// `{Binding x…}` whose root member is unknown (see the module doc).
fn binding_members(registry: &WebRegistry, doc: &Document, code: &CodeBehind) -> Vec<Diagnostic> {
    let Some(class) = code.class() else { return Vec::new() };
    if class.member("dataContext").is_some() || class.extends.as_deref() != Some("ViewBase") {
        return Vec::new();
    }
    let names: Vec<String> = elements(doc).iter().filter_map(|e| e.attribute("x:Name").and_then(|a| a.value())).collect();
    let mut out = Vec::new();
    for (el, _, path, (s, e)) in binding_paths_of(doc) {
        // `ItemsSource` of the template itself reads the page; everything inside reads the row first.
        if in_template(registry, &el) || path.is_empty() {
            continue;
        }
        let first = path.split('.').next().unwrap_or_default();
        if class.member(first).is_some() || RESERVED_MEMBERS.contains(&first) || names.iter().any(|n| n == first) {
            continue;
        }
        let end = s + first.len().min(e - s);
        out.push(diag(
            lsp_range(&doc.text, s, end),
            DiagnosticSeverity::WARNING,
            "binding-member",
            format!("`{first}` is not a member of `{}` (the code-behind) nor an `x:Name` of the view", class.name),
        ));
    }
    out
}

/// `(key, set, value range)` of every `{Res …}` of the document.
fn res_references(doc: &Document) -> Vec<(Attribute, String, Option<String>, rowan::TextRange)> {
    let mut out = Vec::new();
    for el in elements(doc) {
        for attr in el.attributes() {
            let (Some(value), Some(range)) = (attr.value(), attr.value_range()) else { continue };
            let v = value.trim();
            let Some(inner) = v.strip_prefix('{').and_then(|r| r.strip_suffix('}')) else { continue };
            let Some(path) = parse_res_path(inner) else { continue };
            if let Some((set, key)) = res_reference(&path) {
                out.push((attr.clone(), key.to_string(), set.map(str::to_string), range));
            }
        }
    }
    out
}

fn resource_diagnostics(doc: &Document, project: &WebProject) -> Vec<Diagnostic> {
    let refs = res_references(doc);
    if refs.is_empty() {
        return Vec::new();
    }
    let index = res::index(&project.root, &project.config.sources);
    if index.items.is_empty() {
        return Vec::new();
    }
    refs.into_iter()
        .filter(|(_, key, set, _)| index.find(key, set.as_deref()).is_none())
        .map(|(_, key, set, range)| {
            let where_ = set.map(|s| format!("`{s}`")).unwrap_or_else(|| "the project's locale bundles and .kbres files".into());
            diag(text_range(doc, range), DiagnosticSeverity::WARNING, "unknown-resource", format!("no resource `{key}` in {where_}"))
        })
        .collect()
}

// ── cursor contexts ─────────────────────────────────────────────────────

/// What the cursor is on.
enum At {
    /// An element's name (start or end tag).
    ElementName(Element),
    /// An attribute's name.
    AttributeName(Element, Attribute),
    /// Inside an attribute's value: the attribute and the byte offset inside the raw value (after the quote).
    Value(Element, Attribute, usize),
}

fn at(doc: &Document, pos: Position) -> Option<(At, SyntaxToken, TextSize)> {
    let offset = doc.position_index.position_to_offset(&doc.text, pos);
    let token = tree::token_at_offset(&doc.parse.syntax(), offset)?;
    let parent = token.parent()?;
    let found = match (token.kind(), parent.kind()) {
        (SyntaxKind::IDENT, SyntaxKind::START_TAG) | (SyntaxKind::IDENT, SyntaxKind::END_TAG) => At::ElementName(tree::enclosing_element(&parent)?),
        (SyntaxKind::IDENT, SyntaxKind::ATTRIBUTE) => {
            let attr = Attribute::cast(parent.clone())?;
            At::AttributeName(tree::enclosing_element(&parent)?, attr)
        }
        (SyntaxKind::STRING, SyntaxKind::ATTRIBUTE) => {
            let attr = Attribute::cast(parent.clone())?;
            let start: usize = u32::from(token.text_range().start()) as usize;
            let inside = (u32::from(offset) as usize).checked_sub(start + 1)?;
            At::Value(tree::enclosing_element(&parent)?, attr, inside)
        }
        _ => return None,
    };
    Some((found, token, offset))
}

/// The binding path under the cursor of a `{Binding …}` value: the path and the segment index under the cursor.
fn binding_path_at(attr: &Attribute, inside: usize) -> Option<(String, usize)> {
    let raw = attr.raw_value()?;
    let inner = raw.get(1..raw.len().checked_sub(1)?)?;
    if !inner.trim_start().starts_with("{Binding") {
        return None;
    }
    let parts = binding_parts(inner)?;
    let (i, part) = parts.iter().enumerate().find(|(i, p)| (p.key.is_none() && *i == 0) || p.key.as_deref() == Some("Path"))?;
    let _ = i;
    if inside < part.value_range.start || inside > part.value_range.end {
        return None;
    }
    let before = &inner[part.value_range.start..inside];
    Some((part.value.clone(), before.matches('.').count()))
}

fn res_at(attr: &Attribute) -> Option<(String, Option<String>)> {
    let value = attr.value()?;
    let inner = value.trim().strip_prefix('{')?.strip_suffix('}')?;
    let path = parse_res_path(inner)?;
    let (set, key) = res_reference(&path)?;
    Some((key.to_string(), set.map(str::to_string)))
}

// ── completion ──────────────────────────────────────────────────────────

pub fn completion(doc: &Document, view: &WebView, pos: Position) -> Vec<CompletionItem> {
    project::with_project(&view.root, |project| completion_in(project, doc, pos, &view.stem)).unwrap_or_default()
}

fn completion_in(project: &WebProject, doc: &Document, pos: Position, stem: &str) -> Option<Vec<CompletionItem>> {
    let registry = &project.session.registry;
    let offset = doc.position_index.position_to_offset(&doc.text, pos);
    let token = tree::token_at_offset(&doc.parse.syntax(), offset)?;
    let parent_kind = token.parent().map(|p| p.kind());
    // Closing tag.
    if token.kind() == SyntaxKind::L_ANGLE_SLASH || (token.kind() == SyntaxKind::IDENT && parent_kind == Some(SyntaxKind::END_TAG)) {
        let element = token.parent().and_then(|t| t.parent()).and_then(Element::cast)?;
        let name = element.name()?;
        return Some(vec![CompletionItem { label: format!("{name}>"), kind: Some(CompletionItemKind::CLASS), insert_text: Some(format!("{name}>")), ..Default::default() }]);
    }
    // Attribute value.
    if token.kind() == SyntaxKind::STRING && parent_kind == Some(SyntaxKind::ATTRIBUTE) {
        let range = token.text_range();
        if !(offset > range.start() && offset < range.end()) {
            return Some(Vec::new());
        }
        let attr = token.parent().and_then(Attribute::cast)?;
        let element = tree::enclosing_element(&token.parent()?)?;
        let inside = (u32::from(offset) - u32::from(range.start()) - 1) as usize;
        return Some(value_items(project, doc, &element, &attr, &token.text()[1..1 + inside]));
    }
    // Attribute name.
    if tree::is_attribute_name(&token) || (token.kind() == SyntaxKind::WHITESPACE && parent_kind == Some(SyntaxKind::START_TAG)) {
        let element = tree::enclosing_element(&token.parent()?)?;
        return Some(attribute_items(registry, &element));
    }
    // Element name.
    if token.kind() == SyntaxKind::L_ANGLE || tree::is_start_tag_name(&token) {
        let new_element = token.parent()?.parent()?;
        let container = new_element.parent().and_then(|c| tree::enclosing_element(&c));
        // A user control never contains itself.
        return Some(element_items(registry, container.as_ref(), &new_element).into_iter().filter(|i| i.label != stem).collect());
    }
    Some(Vec::new())
}

fn element_item(meta: &WebElement) -> CompletionItem {
    let e = &meta.entry;
    let origin = match meta.origin {
        Origin::Host => meta.web.module.clone().map(|m| format!("{m} {}", meta.web.export.clone().unwrap_or_default())).unwrap_or_else(|| "item".into()),
        Origin::Project => "project control".into(),
        Origin::UserControl => "user control".into(),
    };
    CompletionItem {
        label: e.name.clone(),
        kind: Some(if meta.origin == Origin::Host { CompletionItemKind::CLASS } else { CompletionItemKind::MODULE }),
        detail: Some(origin.trim().to_string()),
        documentation: Some(markdown(doc_of(&e.doc, &e.doc_fr).to_string())),
        ..Default::default()
    }
}

fn element_items(registry: &WebRegistry, container: Option<&Element>, new_element: &kubuno_desktop_views::syntax::SyntaxNode) -> Vec<CompletionItem> {
    let parent_meta = container.and_then(|c| c.name()).and_then(|n| registry.get(&n));
    let items_of_parent: Vec<String> = parent_meta.and_then(|m| m.web.children_to_prop.as_ref().map(|a| a.item.list())).unwrap_or_default();
    if let (Some(parent), Some(meta)) = (container, parent_meta) {
        let others = parent.children().filter(|c| c.syntax() != new_element).count();
        match meta.entry.children {
            ChildrenModelJson::None if items_of_parent.is_empty() => return Vec::new(),
            ChildrenModelJson::SingleWidget if others >= 1 => return Vec::new(),
            _ => {}
        }
    }
    let allowed = parent_meta.map(|m| m.entry.allowed_children.clone()).unwrap_or_default();
    registry
        .elements()
        .filter(|e| {
            let name = &e.entry.name;
            if e.is_item() {
                // An item only under the parents whose adapter consumes it.
                return items_of_parent.contains(name) || parent_meta.is_some_and(|p| p.entry.name == *name && p.web.item_of.contains(name));
            }
            allowed.is_empty() || allowed.contains(name)
        })
        .map(element_item)
        .collect()
}

/// Whether a property can be set on the web (it has a `prop_map` entry; a user control takes any attribute).
fn on_web(meta: &WebElement, p: &PropertyEntry) -> bool {
    meta.web.prop_map.contains_key(&p.name)
}

fn attribute_items(registry: &WebRegistry, element: &Element) -> Vec<CompletionItem> {
    let root = is_root(element);
    let mut items = vec![CompletionItem {
        label: "x:Name".into(),
        kind: Some(CompletionItemKind::FIELD),
        detail: Some("x:".into()),
        documentation: Some(markdown("The element's name: a typed handle field of the code-behind (`this.<name>`).".into())),
        ..Default::default()
    }];
    if root {
        items.push(CompletionItem {
            label: "x:Props".into(),
            kind: Some(CompletionItemKind::FIELD),
            detail: Some("x:".into()),
            documentation: Some(markdown("The TypeScript type of the view's props, exported by the code-behind (`this.props`).".into())),
            ..Default::default()
        });
        for (name, doc) in [("DesignWidth", "Canvas width in the designer only."), ("DesignHeight", "Canvas height in the designer only.")] {
            items.push(CompletionItem { label: name.into(), kind: Some(CompletionItemKind::PROPERTY), detail: Some("Design".into()), documentation: Some(markdown(doc.into())), ..Default::default() });
        }
    }
    let Some(meta) = element.name().and_then(|n| registry.get(&n)) else { return items };
    let mut seen = std::collections::HashSet::new();
    for p in &meta.entry.properties {
        if (p.root_only && !root) || !on_web(meta, p) || !seen.insert(p.name.clone()) {
            continue;
        }
        let from = p.inherited_from.as_ref().map(|l| format!(" (`{l}`)")).unwrap_or_default();
        let kind = match &p.kind {
            PropKindEntry::Enum(v) => v.join(" | "),
            other => format!("{other:?}"),
        };
        items.push(CompletionItem {
            label: p.name.clone(),
            kind: Some(CompletionItemKind::PROPERTY),
            detail: p.category.clone(),
            documentation: Some(markdown(format!("{}\n\n*{kind}* — default `{}`{from}", doc_of(&p.doc, &p.doc_fr), p.default))),
            ..Default::default()
        });
    }
    for ev in &meta.entry.events {
        if (ev.root_only && !root) || !meta.web.event_map.contains_key(&ev.name) {
            continue;
        }
        items.push(CompletionItem {
            label: ev.name.clone(),
            kind: Some(CompletionItemKind::EVENT),
            detail: Some(format!("{} ({})", ev.category, ev.args_type)),
            documentation: Some(markdown(doc_of(&ev.doc, &ev.doc_fr).to_string())),
            ..Default::default()
        });
    }
    items.extend(crate::namespaces::attribute_items(element));
    items
}

fn value_items(project: &WebProject, doc: &Document, element: &Element, attr: &Attribute, before: &str) -> Vec<CompletionItem> {
    let registry = &project.session.registry;
    let Some(attr_name) = attr.name() else { return Vec::new() };
    // `{Res …}`: the project's strings.
    if let Some(at) = before.rfind("{Res") {
        let after = &before[at + 4..];
        if !after.contains('}') && !after.contains(',') && (after.is_empty() || after.starts_with(' ')) {
            return res_items(project, after.is_empty());
        }
    }
    // `{Binding …, |`: the binding's keys (the path itself comes from the code-behind, `kubuno/bindingPaths`).
    if let Some(at) = before.rfind("{Binding") {
        let after = &before[at..];
        if !after.contains('}') {
            let last = after.rsplit(',').next().unwrap_or_default();
            if after.contains(',') && !last.contains('=') {
                return BINDING_KEYS.iter().map(|k| CompletionItem { label: format!("{k}="), kind: Some(CompletionItemKind::KEYWORD), ..Default::default() }).collect();
            }
            if let Some(mode) = last.trim().strip_prefix("Mode=") {
                let _ = mode;
                return ["OneWay", "TwoWay", "OneTime", "OneWayToSource"].iter().map(|m| CompletionItem { label: m.to_string(), kind: Some(CompletionItemKind::ENUM_MEMBER), ..Default::default() }).collect();
            }
            return Vec::new();
        }
    }
    if let Some(items) = crate::namespaces::value_items(&attr_name) {
        return items;
    }
    let Some(meta) = element.name().and_then(|n| registry.get(&n)) else { return Vec::new() };
    if meta.entry.event(&attr_name).is_some() {
        // Handler names come from the code-behind (`kubuno/compatibleHandlers`, the editor's cross-language completion).
        return Vec::new();
    }
    let Some(p) = meta.property(&attr_name) else { return Vec::new() };
    match p.editor.as_deref() {
        Some(e) if e.starts_with("reference:") => {
            let kind = &e["reference:".len()..];
            let mut names: Vec<String> = elements(doc)
                .into_iter()
                .filter(|el| el.name().is_some_and(|n| n == kind || registry.get(&n).is_some_and(|m| m.entry.is_a(kind))))
                .filter_map(|el| el.attribute("x:Name").and_then(|a| a.value()))
                .collect();
            names.sort();
            names.dedup();
            return names.into_iter().map(|n| CompletionItem { label: n, kind: Some(CompletionItemKind::REFERENCE), ..Default::default() }).collect();
        }
        Some(e) if e.starts_with("class:") => {
            return registry.elements().filter(|e| e.origin == Origin::UserControl).map(element_item).collect();
        }
        Some("icon") => return icon_items(),
        Some("list") | Some("object") => return vec![CompletionItem { label: "{Binding }".into(), kind: Some(CompletionItemKind::SNIPPET), ..Default::default() }],
        _ => {}
    }
    let web_values = meta.web.prop_map.get(&p.name).and_then(|t| t.values.clone());
    match &p.kind {
        PropKindEntry::Enum(values) => values
            .iter()
            .enumerate()
            .map(|(i, v)| CompletionItem {
                label: v.clone(),
                kind: Some(CompletionItemKind::ENUM_MEMBER),
                detail: web_values.as_ref().and_then(|w| w.get(v)).map(|w| format!("→ {w}")),
                sort_text: Some(format!("{i:03}")),
                ..Default::default()
            })
            .collect(),
        PropKindEntry::Bool => ["true", "false"].iter().map(|v| CompletionItem { label: v.to_string(), kind: Some(CompletionItemKind::KEYWORD), ..Default::default() }).collect(),
        _ => Vec::new(),
    }
}

/// The Lucide icon names (the web's icon set, `lucide-react`) and their short aliases.
fn icon_items() -> Vec<CompletionItem> {
    crate::completion::icon_items().into_iter().filter(|i| i.detail.as_deref().is_none_or(|d| d == "Lucide icon" || d.starts_with("alias of"))).collect()
}

fn res_items(project: &WebProject, right_after_prefix: bool) -> Vec<CompletionItem> {
    let index = res::index(&project.root, &project.config.sources);
    let mut items: Vec<CompletionItem> = index
        .items
        .iter()
        .map(|item| {
            let default = index.default_set.as_deref() == Some(item.set.as_str()) || item.file.extension().is_some_and(|e| e.eq_ignore_ascii_case("kbres"));
            let insert = if default { item.key.clone() } else { format!("{}, Source={}", item.key, item.set) };
            CompletionItem {
                label: item.key.clone(),
                kind: Some(CompletionItemKind::TEXT),
                detail: Some(item.value.clone()),
                documentation: Some(markdown(res_markdown(item))),
                insert_text: Some(if right_after_prefix { format!(" {insert}") } else { insert }),
                filter_text: Some(item.key.clone()),
                ..Default::default()
            }
        })
        .collect();
    items.sort_by(|a, b| a.label.cmp(&b.label));
    items
}

fn res_markdown(item: &res::WebResource) -> String {
    let file = item.file.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
    let mut md = format!("**{}** — `{}` ({file})\n\n{}\n\n", item.key, item.set, item.value.replace('\n', "  \n"));
    for (lang, v) in &item.translations {
        md.push_str(&format!("- `{lang}`: {}\n", v.replace('\n', " ")));
    }
    md
}

// ── hover ───────────────────────────────────────────────────────────────

pub fn hover(doc: &Document, view: &WebView, pos: Position) -> Option<Hover> {
    let (found, token, _) = at(doc, pos)?;
    let range = Some(text_range(doc, token.text_range()));
    let value = project::with_project(&view.root, |project| -> Option<String> {
        let registry = &project.session.registry;
        match &found {
            At::ElementName(el) => {
                let meta = registry.get(&el.name()?)?;
                let e = &meta.entry;
                let source = match meta.origin {
                    Origin::Host => format!("`{}` `{}`", meta.web.module.clone().unwrap_or_default(), meta.web.export.clone().unwrap_or_default()),
                    Origin::Project => format!("project control `{}` (`{}`)", meta.web.export.clone().unwrap_or_default(), meta.web.module.clone().unwrap_or_default()),
                    Origin::UserControl => format!("user control (`{}`)", meta.web.module.clone().unwrap_or_default()),
                };
                Some(format!("**{}** — {} · {source}\n\n{}", e.name, e.family, doc_of(&e.doc, &e.doc_fr)))
            }
            At::AttributeName(el, attr) => {
                let meta = registry.get(&el.name()?)?;
                let name = attr.name()?;
                if let Some(ev) = meta.entry.event(&name) {
                    let handle = handlers::handle_of(registry, &meta.entry.name);
                    return Some(format!(
                        "**{}** — event ({})\n\n{}\n\n```ts\nhandler(sender: {handle}, e: {}): void | Promise<void>\n```",
                        ev.name,
                        ev.category,
                        doc_of(&ev.doc, &ev.doc_fr),
                        ev.args_type
                    ));
                }
                let p = meta.property(&name)?;
                let kind = match &p.kind {
                    PropKindEntry::Enum(v) => v.join(" | "),
                    other => format!("{other:?}"),
                };
                let web = match meta.web.prop_map.get(&p.name) {
                    Some(t) => t.prop.clone().map(|pr| format!("React prop `{pr}`")).or_else(|| t.runtime.clone().map(|r| format!("runtime `{r}`"))).unwrap_or_default(),
                    None => "not available on the web".into(),
                };
                Some(format!("**{}** — {kind} (default `{}`)\n\n{}\n\n*{web}*", p.name, p.default, doc_of(&p.doc, &p.doc_fr)))
            }
            At::Value(el, attr, inside) => {
                let name = attr.name()?;
                let meta = el.name().and_then(|n| registry.get(&n));
                if meta.is_some_and(|m| m.entry.event(&name).is_some()) {
                    let handler = attr.value()?.trim().to_string();
                    let code = CodeBehind::load(view)?;
                    let class = code.class()?;
                    return Some(match class.member(&handler) {
                        Some(m) => format!("```ts\n{}\n```\n\n`{}` — {}", m.signature(), class.name, code.path().file_name()?.to_string_lossy()),
                        None => format!("`{handler}`: no such method in `{}`", class.name),
                    });
                }
                if let Some((path, segment)) = binding_path_at(attr, *inside) {
                    let code = CodeBehind::load(view)?;
                    let class = code.class()?;
                    let first = path.split('.').next()?;
                    let m = class.member(first)?;
                    let note = if segment > 0 { format!("\n\n(path `{path}`)") } else { String::new() };
                    return Some(format!("```ts\n{}\n```\n\n`{}`{note}", m.signature(), class.name));
                }
                if let Some((key, set)) = res_at(attr) {
                    let index = res::index(&project.root, &project.config.sources);
                    return Some(index.find(&key, set.as_deref()).map(res_markdown).unwrap_or_else(|| format!("no resource `{key}`")));
                }
                None
            }
        }
    })?;
    Some(Hover { contents: HoverContents::Markup(MarkupContent { kind: MarkupKind::Markdown, value }), range })
}

// ── go to definition ────────────────────────────────────────────────────

pub fn definition(doc: &Document, view: &WebView, pos: Position) -> Option<Location> {
    let (found, _, _) = at(doc, pos)?;
    project::with_project(&view.root, |project| -> Option<Location> {
        let registry = &project.session.registry;
        match &found {
            At::ElementName(el) => element_definition(project, &el.name()?),
            At::AttributeName(..) => None,
            At::Value(el, attr, inside) => {
                let name = attr.name()?;
                let meta = el.name().and_then(|n| registry.get(&n));
                if meta.is_some_and(|m| m.entry.event(&name).is_some()) {
                    let code = CodeBehind::load(view)?;
                    let m = code.class()?.member(attr.value()?.trim())?;
                    return super::location(code.path(), &code.file.text, m.name_span.0, m.name_span.1);
                }
                if name == "x:Props" {
                    let code = CodeBehind::load(view)?;
                    let t = code.file.types.iter().find(|t| t.name == attr.value().unwrap_or_default().trim())?;
                    return super::location(code.path(), &code.file.text, t.span.0, t.span.1);
                }
                if let Some((path, _)) = binding_path_at(attr, *inside) {
                    return member_location(view, doc, path.split('.').next()?);
                }
                if let Some((key, set)) = res_at(attr) {
                    let index = res::index(&project.root, &project.config.sources);
                    let item = index.find(&key, set.as_deref())?;
                    let text = std::fs::read_to_string(&item.file).ok()?;
                    return super::location(&item.file, &text, item.offset, item.offset);
                }
                // A user control or a project control named in a value (`ItemTemplate="MessageRow"`).
                let value = attr.value()?;
                registry.get(value.trim()).filter(|m| m.origin != Origin::Host).and_then(|_| element_definition(project, value.trim()))
            }
        }
    })
}

/// `{Binding member}` / `x:Name` → the class member, else the element of the view named so.
fn member_location(view: &WebView, doc: &Document, first: &str) -> Option<Location> {
    if let Some(code) = CodeBehind::load(view) {
        if let Some(m) = code.class().and_then(|c| c.member(first)) {
            return super::location(code.path(), &code.file.text, m.name_span.0, m.name_span.1);
        }
    }
    let el = elements(doc).into_iter().find(|e| e.attribute("x:Name").and_then(|a| a.value()).as_deref() == Some(first))?;
    let uri_path = &view.path;
    let r = el.name_range()?;
    super::location(uri_path, &doc.text, u32::from(r.start()) as usize, u32::from(r.end()) as usize)
}

/// A project element's source: a user control's code-behind class (else its view), a project control's exported name
/// in its module file.
fn element_definition(project: &WebProject, name: &str) -> Option<Location> {
    let meta = project.session.registry.get(name)?;
    if meta.origin == Origin::Host {
        return None;
    }
    let module = meta.web.module.clone()?;
    let base = project.root.join(module.trim_start_matches('/').replace('/', std::path::MAIN_SEPARATOR_STR));
    let candidates: Vec<std::path::PathBuf> = if project::is_view_file(&base) {
        vec![base.clone()]
    } else {
        ["ts", "tsx", "kbcontrol"].iter().map(|e| std::path::PathBuf::from(format!("{}.{e}", base.display()))).collect()
    };
    let file = candidates.into_iter().find(|p| p.is_file())?;
    let text = crate::sources::read(&file)?;
    if project::is_view_file(&file) {
        return super::location(&file, &text, 0, 0);
    }
    let ts = super::ts::TsFile::parse(&file, &text);
    let export = meta.web.export.clone().unwrap_or_default();
    let span = if export == "default" {
        ts.classes.iter().find(|c| c.name == name).map(|c| c.name_span)
    } else {
        ts.exports.iter().find(|e| e.name == export).map(|e| e.span).or_else(|| ts.classes.iter().find(|c| c.name == export).map(|c| c.name_span))
    };
    let (s, e) = span.unwrap_or((0, 0));
    super::location(&file, &text, s, e)
}

/// `kubuno/bindingDefinition { uri, elementId, attribute }` for a web view: the member named by the first segment of
/// that attribute's binding path.
pub fn binding_definition(doc: &Document, view: &WebView, element_id: &str, attribute: &str) -> Option<Location> {
    let element = AstDocument::cast(doc.parse.syntax())?.resolve_id(element_id)?;
    let value = element.attribute(attribute)?.value()?;
    let spec = parse_binding_syntax(value.trim())?;
    member_location(view, doc, spec.path.split('.').next()?)
}

// ── code actions ────────────────────────────────────────────────────────

fn overlaps(a: &Range, b: &Range) -> bool {
    let before = |p: &Position, q: &Position| (p.line, p.character) < (q.line, q.character);
    !before(&a.end, &b.start) && !before(&b.end, &a.start)
}

/// The quick fixes of a web view in `range`: handlers (create, closest), « did you mean » of the compiler's
/// diagnostics, the namespace declarations.
pub fn code_actions(doc: &Document, uri: &Uri, view: &WebView, range: &Range) -> Vec<CodeAction> {
    let mut out = project::with_project(&view.root, |project| handlers::quick_fixes(&project.session.registry, doc, uri, view, range));
    for d in diagnostics(doc, view).into_iter().filter(|d| overlaps(&d.range, range)) {
        let Some(rest) = d.message.split("did you mean `").nth(1) else { continue };
        let Some(suggestion) = rest.split('`').next().filter(|s| !s.is_empty()) else { continue };
        #[allow(clippy::mutable_key_type)] // `Uri`'s memoizing `Cell`, never mutated here.
        let changes = std::collections::HashMap::from([(uri.clone(), vec![TextEdit { range: d.range, new_text: suggestion.to_string() }])]);
        out.push(CodeAction {
            title: format!("Use `{suggestion}`"),
            kind: Some(CodeActionKind::QUICKFIX),
            diagnostics: Some(vec![d.clone()]),
            edit: Some(WorkspaceEdit { changes: Some(changes), ..Default::default() }),
            ..Default::default()
        });
    }
    out.extend(crate::namespaces::quick_fixes(doc, uri, range));
    out
}

// ── binding paths, registry, generated files ────────────────────────────

/// `kubuno/bindingPaths` of a web view: the bindable members of its class (`@bind accessor` fields, fields and
/// getters), in declaration order.
pub fn binding_paths(view: &WebView) -> Vec<String> {
    let Some(code) = CodeBehind::load(view) else { return Vec::new() };
    let mut out: Vec<String> = Vec::new();
    for m in code.class().into_iter().flat_map(|c| c.members.iter()).filter(|m| m.is_bindable()) {
        if !out.contains(&m.name) && !RESERVED_MEMBERS.contains(&m.name.as_str()) {
            out.push(m.name.clone());
        }
    }
    out
}

/// `kubuno/registry` for a web view: the elements the view may use (host, project controls, user controls) in the
/// `kubuno/registry` shape (`VIEWS-SPEC.md` §10), each with its `web` block and `origin`.
pub fn registry_json(view: &WebView) -> serde_json::Value {
    project::with_project(&view.root, |project| {
        let components: Vec<serde_json::Value> = project
            .session
            .registry
            .elements()
            .map(|e| {
                let mut entry = e.entry.clone();
                if e.origin == Origin::UserControl {
                    entry.web = Some(serde_json::json!({ "module": e.web.module, "export": e.web.export, "dom_root": e.web.dom_root }));
                    entry.source_file = Some(e.source.clone());
                }
                serde_json::to_value(entry).unwrap_or_default()
            })
            .collect();
        let body = serde_json::to_string(&components).unwrap_or_default();
        serde_json::json!({ "schema": 1, "target": "web", "version": kubuno_desktop_views_model::schema::fnv1a_hex(body.as_bytes()), "components": components })
    })
}

/// Regenerates the `.d.ts`, check file and span map of the web view `doc` under `.kubuno/views/` (only the files
/// whose content changed are written). Returns how many files were written.
pub fn write_generated(doc: &Document, view: &WebView) -> usize {
    project::with_project(&view.root, |project| {
        let out = project.compile(&view.path, &doc.text);
        project.write_generated(&view.path, &out)
    })
}
