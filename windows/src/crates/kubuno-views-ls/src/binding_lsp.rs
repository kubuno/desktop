//! `{Binding …}` in the editor and the Properties window (`vskubuno/docs/DESIGNER.md`, "Data
//! bindings"): diagnostics (an unknown key, mode, trigger, converter or path, a type that does not
//! fit the property, a two-way binding of a read-only member), the « Mettre à jour les liaisons »
//! quick fix after a member was renamed, completion of keys, paths, sources, modes, triggers and
//! converters, hover, and go-to-definition to the Rust member — all answered from the schema of
//! [`crate::binding_sources`].

use std::collections::HashMap;

use kubuno_views::ast::{AstNode, Attribute, Element};
use kubuno_views::binding::{binding_parts, parse_binding, parse_binding_report, BindingMode, BindingPart, UpdateSourceTrigger, BINDING_KEYS};
use kubuno_views::syntax::SyntaxKind;
use lsp_types::{
    CodeAction, CodeActionKind, CompletionItem, CompletionItemKind, CompletionTextEdit, Diagnostic, DiagnosticSeverity, Documentation, Hover, HoverContents, Location, MarkupContent,
    MarkupKind, NumberOrString, Position, Range, TextEdit, Uri, WorkspaceEdit,
};
use serde::Serialize;

use crate::binding_sources::{Member, Resolution, Schema, Shape, ViewSources};
use crate::documents::Document;

const SOURCE: &str = "kubuno-bindings";

/// The diagnostic code of an unknown path (its `data` carries the rename the quick fix makes).
pub const UNKNOWN_PATH: &str = "binding-unknown-path";

/// A binding problem of one attribute, for the Properties window's markers.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AttributeIssue {
    pub element_id: String,
    pub attribute: String,
    /// `"error"`, `"warning"` or `"information"`.
    pub severity: &'static str,
    pub code: String,
    pub message: String,
}

/// One problem of a binding, its range relative to the attribute's raw value (inside the quotes).
#[derive(Debug, Clone, PartialEq)]
pub struct Issue {
    pub range: std::ops::Range<usize>,
    pub severity: DiagnosticSeverity,
    pub code: &'static str,
    pub message: String,
    /// `(old path, new path)` of a probable rename.
    pub rename: Option<(String, String)>,
}

/// The shape a property of `element` wants (`None`: anything, or unknown).
pub fn expected_shape(element: &str, attribute: &str) -> Option<Shape> {
    if attribute == "ItemsSource" {
        return Some(Shape::List);
    }
    // A column's `Binding` names a field of the row: any shape.
    if attribute == "Binding" {
        return None;
    }
    let meta = kubuno_views::registry::lookup(element).and_then(|m| m.property(attribute)).or_else(|| kubuno_views::registry::view_property(attribute));
    let meta = meta?;
    match meta.editor {
        Some("list") => return Some(Shape::List),
        Some("object") => return Some(Shape::Object),
        _ => {}
    }
    Some(match meta.kind {
        kubuno_views::registry::PropKind::Bool => Shape::Bool,
        kubuno_views::registry::PropKind::F32 => Shape::Number,
        _ => Shape::Text,
    })
}

/// Whether a value of shape `have` fits a property of shape `want`: `Some(true)` yes, `Some(false)`
/// never, `None` only when the text converts (a number or a boolean written as text).
pub fn fits(have: Shape, want: Shape) -> Option<bool> {
    use Shape::*;
    match (have, want) {
        (Any, _) | (_, Any) => Some(true),
        (a, b) if a == b => Some(true),
        (List, _) | (_, List) => Some(false),
        (Object, _) | (_, Object) => Some(false),
        (Text, Bool) | (Text, Number) => None,
        _ => Some(true),
    }
}

/// The path part of a binding: the bare first part or `Path=`.
fn path_part(parts: &[BindingPart]) -> Option<&BindingPart> {
    parts.iter().enumerate().find(|(i, p)| (*i == 0 && p.key.is_none()) || p.key.as_deref() == Some("Path")).map(|(_, p)| p)
}

fn value_of<'a>(parts: &'a [BindingPart], key: &str) -> Option<&'a BindingPart> {
    parts.iter().find(|p| p.key.as_deref() == Some(key))
}

/// A crude edit distance.
fn distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut prev = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cur = row[j + 1];
            row[j + 1] = if ca == *cb { prev } else { 1 + prev.min(row[j]).min(row[j + 1]) };
            prev = cur;
        }
    }
    row[b.len()]
}

/// The member a renamed path probably became: the closest member the view does not bind yet.
fn rename_candidate(schema: &Schema, old: &str, bound: &[String]) -> Option<String> {
    let norm = |s: &str| s.replace('_', "").to_lowercase();
    let candidates: Vec<&Member> = schema.item.iter().flat_map(|i| i.members.iter()).chain(schema.context.members.iter()).filter(|m| !bound.contains(&m.path)).collect();
    if let Some(m) = candidates.iter().find(|m| norm(&m.path) == norm(old)) {
        return Some(m.path.clone());
    }
    // `Status` → `StatusText`, `UserName` → `Name`: one name holds the other.
    let (o, n) = (norm(old), |m: &&Member| norm(&m.path));
    let mut holding: Vec<&&Member> = candidates.iter().filter(|m| o.len() >= 3 && n(m).len() >= 3 && (n(m).contains(&o) || o.contains(&n(m)))).collect();
    if holding.len() == 1 {
        return holding.pop().map(|m| m.path.clone());
    }
    // `Statut` → `StatusText`: a long common start.
    let common = |a: &str, b: &str| a.chars().zip(b.chars()).take_while(|(x, y)| x == y).count();
    let mut prefixed: Vec<&&Member> = candidates
        .iter()
        .filter(|m| {
            let c = common(&o, &n(m));
            c >= 4 && c * 5 >= o.chars().count().min(n(m).chars().count()) * 3
        })
        .collect();
    if prefixed.len() == 1 {
        return prefixed.pop().map(|m| m.path.clone());
    }
    let limit = (old.chars().count() / 2).max(2);
    candidates.iter().map(|m| (distance(&old.to_lowercase(), &m.path.to_lowercase()), m)).filter(|(d, _)| *d <= limit).min_by_key(|(d, _)| *d).map(|(_, m)| m.path.clone())
}

/// The problems of the binding `raw` (the attribute's value inside its quotes) of `attribute` on
/// `element`, against `schema`; `bound` lists every path the view binds (for rename candidates).
pub fn check(schema: &Schema, element: &str, attribute: &str, raw: &str, bound: &[String]) -> Vec<Issue> {
    let mut out = Vec::new();
    let Some(parts) = binding_parts(raw) else { return out };
    let (spec, report) = parse_binding_report(raw);
    for i in report {
        out.push(Issue { range: i.range, severity: DiagnosticSeverity::WARNING, code: "binding-syntax", message: i.message, rename: None });
    }
    let Some(spec) = spec else {
        out.push(Issue { range: 0..raw.len(), severity: DiagnosticSeverity::WARNING, code: "binding-no-path", message: "the binding names no path (`{Binding Path}`)".into(), rename: None });
        return out;
    };
    // The converter.
    let converter = value_of(&parts, "Converter").filter(|p| !p.value.is_empty());
    if let Some(c) = converter {
        if schema.converter(&c.value).is_none() {
            let known: Vec<&str> = schema.converters.iter().map(|c| c.name.as_str()).collect();
            out.push(Issue {
                range: c.value_range.clone(),
                severity: DiagnosticSeverity::WARNING,
                code: "binding-unknown-converter",
                message: format!("unknown converter `{}`: the value is shown unconverted. Known: {} (a project converter is declared with `#[value_converter]`)", c.value, known.join(", ")),
                rename: None,
            });
        }
    }
    // The path.
    let source = value_of(&parts, "Source");
    let path_range = path_part(&parts).map(|p| p.value_range.clone()).or_else(|| source.map(|s| s.value_range.clone())).unwrap_or(0..raw.len());
    let member = match schema.resolve(&spec.path) {
        Resolution::Found(m) => Some(m),
        Resolution::Unknown => None,
        Resolution::Missing => {
            let (range, what) = match source {
                Some(s) if !schema.components.iter().any(|c| c.path == s.value) => (s.value_range.clone(), format!("no data component named `{}` in the view", s.value)),
                _ => (path_range.clone(), format!("`{}` is not a member of {}", spec.path, scope_label(schema))),
            };
            let rename = if source.is_none() { rename_candidate(schema, &spec.path, bound) } else { None };
            let message = match &rename {
                Some(new) => format!("{what}: was it renamed `{new}`? (quick fix « Mettre à jour les liaisons »)"),
                None => what,
            };
            out.push(Issue { range, severity: DiagnosticSeverity::WARNING, code: UNKNOWN_PATH, message, rename: rename.map(|n| (spec.path.clone(), n)) });
            None
        }
    };
    let Some(member) = member else { return out };
    // The shape.
    let have = match spec.converter.as_deref().and_then(|c| schema.converter(c)) {
        Some(c) => c.output,
        None if spec.converter.is_some() => Shape::Any,
        None => member.shape,
    };
    if let Some(want) = expected_shape(element, attribute) {
        match fits(have, want) {
            Some(false) => out.push(Issue {
                range: path_range.clone(),
                severity: DiagnosticSeverity::WARNING,
                code: "binding-type",
                message: format!("`{attribute}` takes {}, `{}` is {}{}", shape_name(want), spec.path, shape_name(have), hint(have, want)),
                rename: None,
            }),
            None => out.push(Issue {
                range: path_range.clone(),
                severity: DiagnosticSeverity::INFORMATION,
                code: "binding-type",
                message: format!("`{attribute}` takes {}, `{}` is text: it shows only when the text converts", shape_name(want), spec.path),
                rename: None,
            }),
            Some(true) => {}
        }
    }
    if spec.mode.writes_back() && !member.writable {
        let range = value_of(&parts, "Mode").map(|p| p.value_range.clone()).unwrap_or(path_range);
        out.push(Issue {
            range,
            severity: DiagnosticSeverity::WARNING,
            code: "binding-read-only",
            message: format!("`{}` is read-only: Mode={} writes nothing back", spec.path, spec.mode.name()),
            rename: None,
        });
    }
    out
}

fn shape_name(s: Shape) -> &'static str {
    match s {
        Shape::Bool => "a boolean",
        Shape::Number => "a number",
        Shape::Text => "text",
        Shape::List => "a list",
        Shape::Object => "an object",
        Shape::Any => "a value",
    }
}

fn hint(have: Shape, want: Shape) -> &'static str {
    match (have, want) {
        (Shape::List, Shape::Number | Shape::Text) => " (Converter=Count gives its size)",
        (Shape::List, Shape::Bool) => " (Converter=IsNotEmpty tells whether it has rows)",
        _ => "",
    }
}

fn scope_label(schema: &Schema) -> String {
    match (&schema.item, schema.context.label.is_empty()) {
        (Some(item), _) if !item.label.is_empty() => format!("the row ({}) nor of `{}`", item.label, schema.context.label),
        (_, false) => format!("`{}`", schema.context.label),
        _ => "the view model".to_string(),
    }
}

/// The `{Binding …}` attributes of the document: element, attribute, raw value (inside the quotes)
/// and the offset of that value in the document.
fn binding_attributes(doc: &Document) -> Vec<(Element, Attribute, String, usize)> {
    doc.parse
        .syntax()
        .descendants()
        .filter_map(Attribute::cast)
        .filter_map(|a| {
            let range = a.value_range()?;
            let start = usize::from(range.start());
            let raw = doc.text.get(start..usize::from(range.end()))?.to_string();
            binding_parts(&raw)?;
            let element = crate::tree::enclosing_element(a.syntax())?;
            Some((element, a, raw, start))
        })
        .collect()
}

/// Every path the document binds.
fn bound_paths(doc: &Document) -> Vec<String> {
    binding_attributes(doc).into_iter().filter_map(|(_, _, raw, _)| parse_binding(&raw).map(|s| s.path)).collect()
}

fn range_in(doc: &Document, start: usize, r: &std::ops::Range<usize>) -> Range {
    Range {
        start: doc.position_index.offset_to_position(&doc.text, rowan::TextSize::from(u32::try_from(start + r.start).unwrap_or(u32::MAX))),
        end: doc.position_index.offset_to_position(&doc.text, rowan::TextSize::from(u32::try_from(start + r.end).unwrap_or(u32::MAX))),
    }
}

/// The diagnostics of the document's bindings.
pub fn diagnostics(doc: &Document, uri: &Uri) -> Vec<Diagnostic> {
    let attrs = binding_attributes(doc);
    if attrs.is_empty() {
        return Vec::new();
    }
    let Some(view) = crate::fs_uri::to_path(uri) else { return Vec::new() };
    let mut sources = ViewSources::read(&view, doc);
    let bound: Vec<String> = attrs.iter().filter_map(|(_, _, raw, _)| parse_binding(raw).map(|s| s.path)).collect();
    let mut out = Vec::new();
    for (element, attr, raw, start) in attrs {
        let schema = sources.schema_for(Some(&element), false);
        let (Some(el), Some(name)) = (element.name(), attr.name()) else { continue };
        for issue in check(&schema, &el, &name, &raw, &bound) {
            out.push(Diagnostic {
                range: range_in(doc, start, &issue.range),
                severity: Some(issue.severity),
                code: Some(NumberOrString::String(issue.code.to_string())),
                source: Some(SOURCE.to_string()),
                message: issue.message,
                data: issue.rename.map(|(old, new)| serde_json::json!({ "oldPath": old, "newPath": new })),
                ..Default::default()
            });
        }
    }
    out
}

/// The binding problems of `element`'s attributes (the Properties window's markers).
pub fn element_issues(doc: &Document, sources: &mut ViewSources, element: &Element) -> Vec<AttributeIssue> {
    let bound = bound_paths(doc);
    let schema = sources.schema_for(Some(element), false);
    let el = element.name().unwrap_or_default();
    let id = element.stable_id();
    let mut out = Vec::new();
    for attr in element.attributes() {
        let (Some(name), Some(range)) = (attr.name(), attr.value_range()) else { continue };
        let Some(raw) = doc.text.get(usize::from(range.start())..usize::from(range.end())) else { continue };
        for issue in check(&schema, &el, &name, raw, &bound) {
            out.push(AttributeIssue {
                element_id: id.clone(),
                attribute: name.clone(),
                severity: match issue.severity {
                    DiagnosticSeverity::ERROR => "error",
                    DiagnosticSeverity::WARNING => "warning",
                    _ => "information",
                },
                code: issue.code.to_string(),
                message: issue.message,
            });
        }
    }
    out
}

/// « Mettre à jour les liaisons » for the unknown-path diagnostics in `diagnostics` (the ones the
/// client sends with the code action request): every binding of the old path becomes the new one.
pub fn code_actions(doc: &Document, uri: &Uri, diagnostics: &[Diagnostic]) -> Vec<CodeAction> {
    let mut out = Vec::new();
    for d in diagnostics {
        if d.code != Some(NumberOrString::String(UNKNOWN_PATH.to_string())) {
            continue;
        }
        let Some(data) = &d.data else { continue };
        let (Some(old), Some(new)) = (data.get("oldPath").and_then(|v| v.as_str()), data.get("newPath").and_then(|v| v.as_str())) else { continue };
        let edits = rename_edits(doc, old, new);
        if edits.is_empty() {
            continue;
        }
        #[allow(clippy::mutable_key_type)] // `Uri` hashes its text; it is never mutated here.
        let changes = HashMap::from([(uri.clone(), edits)]);
        out.push(CodeAction {
            title: format!("Mettre à jour les liaisons : `{old}` → `{new}`"),
            kind: Some(CodeActionKind::QUICKFIX),
            diagnostics: Some(vec![d.clone()]),
            is_preferred: Some(true),
            edit: Some(WorkspaceEdit { changes: Some(changes), ..Default::default() }),
            ..Default::default()
        });
    }
    out
}

/// The edits renaming every binding path `old` of the document to `new`.
pub fn rename_edits(doc: &Document, old: &str, new: &str) -> Vec<TextEdit> {
    let mut out = Vec::new();
    for (_, _, raw, start) in binding_attributes(doc) {
        let Some(parts) = binding_parts(&raw) else { continue };
        if value_of(&parts, "Source").is_some() {
            continue;
        }
        if let Some(p) = path_part(&parts).filter(|p| p.value == old) {
            out.push(TextEdit { range: range_in(doc, start, &p.value_range), new_text: new.to_string() });
        }
    }
    out
}

// ── `kubuno/bindingPreview` ────────────────────────────────────────────

/// `kubuno/bindingPreview`: what a property shows for a sample source value through a binding —
/// computed by the runtime's own converters and formats (the « Liaison de données » dialog's live
/// sample).
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BindingPreviewParams {
    /// The `{Binding …}` expression.
    pub expression: String,
    /// The sample source value, as text (`""` and `null` preview the null value).
    #[serde(default)]
    pub value: Option<String>,
    /// The sample's shape (`Bool`, `Number`, `Text`); text by default.
    #[serde(default)]
    pub shape: Option<String>,
    /// The property's shape (`Bool`, `Number`, `Text`); text by default.
    #[serde(default)]
    pub want: Option<String>,
}

#[derive(Debug, Serialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BindingPreviewResult {
    /// The value the property shows (`None`: it falls back to its default).
    pub text: Option<String>,
    /// Why the preview may differ at run time (a project converter the server cannot run).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// `kubuno/bindingPreview` (see [`BindingPreviewParams`]).
pub fn binding_preview(p: &BindingPreviewParams) -> BindingPreviewResult {
    use kubuno_views::binding::{MapViewModel, PropSource, Value};
    let Some(spec) = parse_binding(&p.expression) else { return BindingPreviewResult::default() };
    let note = spec
        .converter
        .as_deref()
        .filter(|c| kubuno_views::binding::converter(c).is_none())
        .map(|c| format!("`{c}` is a converter of the project: the value is shown unconverted here"));
    let mut vm = MapViewModel::new();
    if let Some(text) = p.value.as_deref() {
        let value = match p.shape.as_deref() {
            Some("Number") => text.trim().parse::<f32>().map(Value::F32).unwrap_or_else(|_| Value::Str(text.to_string())),
            Some("Bool") => match text.trim() {
                "true" => Value::Bool(true),
                "false" => Value::Bool(false),
                _ => Value::Str(text.to_string()),
            },
            _ => Value::Str(text.to_string()),
        };
        vm = vm.with(spec.path.clone(), value);
    }
    let text = match p.want.as_deref() {
        Some("Bool") => {
            let p: PropSource<bool> = PropSource::Bound { spec, fallback: false };
            Some(p.resolve(&vm).to_string())
        }
        Some("Number") => {
            let p: PropSource<f32> = PropSource::Bound { spec, fallback: f32::NAN };
            let v = p.resolve(&vm);
            (!v.is_nan()).then(|| v.to_string())
        }
        _ => {
            let fallback = "\u{0}".to_string();
            let p: PropSource<String> = PropSource::Bound { spec, fallback: fallback.clone() };
            let v = p.resolve(&vm);
            (v != fallback).then_some(v)
        }
    };
    BindingPreviewResult { text, note }
}

// ── completion, hover, definition ──────────────────────────────────────

/// Where the cursor is in a `{Binding …}` value being typed.
struct Cursor {
    /// The part's key (`None`: the path, or a key being typed).
    key: Option<String>,
    /// Whether the cursor is on a key (after a comma, before any `=`).
    on_key: bool,
    /// What is typed of the word under the cursor.
    prefix: String,
    /// Where that word starts in the document.
    word_start: usize,
    /// The parts typed so far (for `Source=` when completing the path).
    before: String,
    element: Element,
    attribute: String,
}

fn cursor_at(doc: &Document, pos: Position) -> Option<Cursor> {
    let offset = doc.position_index.position_to_offset(&doc.text, pos);
    let token = crate::tree::token_at_offset(&doc.parse.syntax(), offset)?;
    if token.kind() != SyntaxKind::STRING {
        return None;
    }
    let attr = token.parent().and_then(Attribute::cast)?;
    let start = usize::from(token.text_range().start()) + 1;
    let cursor = usize::from(offset);
    let before = doc.text.get(start..cursor)?;
    let at = before.rfind("{Binding")?;
    let typed = &before[at + "{Binding".len()..];
    if typed.contains('}') || !(typed.is_empty() || typed.starts_with([' ', ','])) {
        return None;
    }
    // The current part: after the last comma outside quotes.
    let mut quoted = false;
    let mut part_start = 0;
    let mut index = 0;
    for (i, c) in typed.char_indices() {
        match c {
            '\'' => quoted = !quoted,
            ',' if !quoted => {
                part_start = i + 1;
                index += 1;
            }
            _ => {}
        }
    }
    let part = &typed[part_start..];
    let (key, value_rel, on_key) = match part.split_once('=') {
        Some((k, _)) => (Some(k.trim().to_string()), part_start + k.len() + 1, false),
        None => (None, part_start, index > 0),
    };
    let value = &typed[value_rel..];
    let lead = value.len() - value.trim_start().len();
    let word_start = start + at + "{Binding".len() + value_rel + lead;
    Some(Cursor { key, on_key, prefix: value.trim_start().to_string(), word_start, before: typed.to_string(), element: crate::tree::enclosing_element(attr.syntax())?, attribute: attr.name()? })
}

fn item(label: &str, kind: CompletionItemKind, detail: String, doc_text: &str, sort: String, edit_range: Range, insert: String) -> CompletionItem {
    CompletionItem {
        label: label.to_string(),
        kind: Some(kind),
        detail: (!detail.is_empty()).then_some(detail),
        documentation: (!doc_text.is_empty()).then(|| Documentation::MarkupContent(MarkupContent { kind: MarkupKind::Markdown, value: doc_text.to_string() })),
        sort_text: Some(sort),
        filter_text: Some(label.to_string()),
        text_edit: Some(CompletionTextEdit::Edit(TextEdit { range: edit_range, new_text: insert })),
        ..Default::default()
    }
}

fn shape_label(s: Shape) -> &'static str {
    match s {
        Shape::Bool => "Bool",
        Shape::Number => "Number",
        Shape::Text => "Text",
        Shape::List => "List",
        Shape::Object => "Object",
        Shape::Any => "Any",
    }
}

/// Completion inside a `{Binding …}` value; `None` when the cursor is not in one.
pub fn completion(doc: &Document, uri: &Uri, pos: Position) -> Option<Vec<CompletionItem>> {
    let c = cursor_at(doc, pos)?;
    let range = Range { start: doc.position_index.offset_to_position(&doc.text, rowan::TextSize::from(u32::try_from(c.word_start).unwrap_or(u32::MAX))), end: pos };
    let lead = if c.before.is_empty() { " " } else { "" };
    if c.on_key {
        return Some(
            BINDING_KEYS
                .iter()
                .map(|k| item(k, CompletionItemKind::PROPERTY, String::new(), "", format!("1{k}"), range, format!("{k}=")))
                .collect(),
        );
    }
    match c.key.as_deref() {
        Some("Mode") => return Some(BindingMode::ALL.iter().map(|m| item(m.name(), CompletionItemKind::ENUM_MEMBER, String::new(), "", m.name().into(), range, m.name().into())).collect()),
        Some("UpdateSourceTrigger") => {
            return Some(UpdateSourceTrigger::ALL.iter().map(|t| item(t.name(), CompletionItemKind::ENUM_MEMBER, String::new(), "", t.name().into(), range, t.name().into())).collect())
        }
        _ => {}
    }
    let view = crate::fs_uri::to_path(uri)?;
    let mut sources = ViewSources::read(&view, doc);
    let schema = sources.schema_for(Some(&c.element), false);
    let want = c.element.name().and_then(|e| expected_shape(&e, &c.attribute));
    let rank = |s: Shape| match want.map(|w| fits(s, w)) {
        Some(Some(false)) => "9",
        Some(None) => "5",
        _ => "0",
    };
    let member_item = |m: &Member, label: &str| {
        item(
            label,
            match m.kind {
                crate::binding_sources::MemberKind::Component => CompletionItemKind::MODULE,
                crate::binding_sources::MemberKind::State => CompletionItemKind::PROPERTY,
                _ => CompletionItemKind::FIELD,
            },
            format!("{} · {}", m.rust_type.clone().unwrap_or_else(|| shape_label(m.shape).to_string()), shape_label(m.shape)),
            &m.doc,
            format!("{}{label}", rank(m.shape)),
            range,
            format!("{lead}{label}"),
        )
    };
    match c.key.as_deref() {
        Some("Converter") => Some(
            schema
                .converters
                .iter()
                .map(|cv| item(&cv.name, CompletionItemKind::FUNCTION, format!("→ {}{}", shape_label(cv.output), if cv.project { " · project" } else { "" }), &cv.doc, format!("{}{}", rank(cv.output), cv.name), range, cv.name.clone()))
                .collect(),
        ),
        Some("Source") => Some(schema.components.iter().map(|m| member_item(m, &m.path)).collect()),
        Some("Path") | None => {
            // `Source=x, Path=|`: the members of `x`.
            let source = c.before.split(',').filter_map(|p| p.split_once('=')).find(|(k, _)| k.trim() == "Source").map(|(_, v)| v.trim().to_string());
            if let Some(source) = source {
                let comp = schema.components.iter().find(|m| m.path == source)?;
                return Some(comp.children.iter().map(|m| member_item(m, &m.name)).collect());
            }
            if let Some((head, _)) = c.prefix.rsplit_once('.') {
                let prefix = format!("{head}.");
                let all = schema.all_members();
                return Some(all.into_iter().filter(|m| m.path.starts_with(&prefix)).map(|m| member_item(m, &m.path)).collect());
            }
            let mut out: Vec<CompletionItem> = Vec::new();
            for m in schema.item.iter().flat_map(|i| i.members.iter()).chain(schema.context.members.iter()).chain(schema.components.iter()) {
                if !out.iter().any(|i| i.label == m.path) {
                    out.push(member_item(m, &m.path));
                }
            }
            Some(out)
        }
        Some(_) => Some(Vec::new()),
    }
}

/// The binding part under `pos`: the element, the parts, the part index, and the raw value's offset.
fn part_at(doc: &Document, pos: Position) -> Option<(Element, Vec<BindingPart>, usize, usize, String)> {
    let offset = usize::from(doc.position_index.position_to_offset(&doc.text, pos));
    let token = crate::tree::token_at_offset(&doc.parse.syntax(), rowan::TextSize::from(u32::try_from(offset).ok()?))?;
    let attr = token.parent().and_then(Attribute::cast)?;
    let range = attr.value_range()?;
    let start = usize::from(range.start());
    let raw = doc.text.get(start..usize::from(range.end()))?.to_string();
    let parts = binding_parts(&raw)?;
    let rel = offset.checked_sub(start)?;
    let index = parts.iter().position(|p| p.value_range.start <= rel && rel <= p.value_range.end)?;
    Some((crate::tree::enclosing_element(attr.syntax())?, parts, index, start, raw))
}

/// What the part under the cursor names: a member, a converter's location, or nothing.
fn target_at(doc: &Document, uri: &Uri, pos: Position) -> Option<(Option<Member>, Option<Location>, Range)> {
    let (element, parts, index, start, raw) = part_at(doc, pos)?;
    let view = crate::fs_uri::to_path(uri)?;
    let mut sources = ViewSources::read(&view, doc);
    let schema = sources.schema_for(Some(&element), false);
    let part = &parts[index];
    let range = range_in(doc, start, &part.value_range);
    match part.key.as_deref() {
        Some("Converter") => {
            let c = schema.converter(&part.value)?;
            Some((None, c.location.clone(), range))
        }
        Some("Source") => {
            let m = schema.components.iter().find(|m| m.path == part.value)?.clone();
            let loc = m.location.clone();
            Some((Some(m), loc, range))
        }
        Some("Path") | None if path_part(&parts).is_some_and(|p| p.value_range == part.value_range) => {
            let spec = parse_binding(&raw)?;
            // The segment under the cursor: `a.b` on `a` names `a`.
            let rel = usize::from(doc.position_index.position_to_offset(&doc.text, pos)) - start - part.value_range.start;
            let segment_end = part.value[rel.min(part.value.len())..].find('.').map_or(part.value.len(), |n| rel + n);
            let in_source = value_of(&parts, "Source").is_some();
            let path = if in_source || segment_end == part.value.len() { spec.path } else { part.value[..segment_end].to_string() };
            let m = match schema.resolve(&path) {
                Resolution::Found(m) => m.clone(),
                _ => return None,
            };
            let loc = m.location.clone();
            Some((Some(m), loc, range))
        }
        _ => None,
    }
}

/// Go to definition on a binding's path (the Rust member), `Source=` (the component) or
/// `Converter=` (a project converter).
pub fn definition(doc: &Document, uri: &Uri, pos: Position) -> Option<Location> {
    target_at(doc, uri, pos)?.1
}

/// `kubuno/bindingDefinition { uri, elementId, attribute }`: the definition of the path (else the
/// converter) of an element's binding — the Properties window's « Aller à la définition ».
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BindingDefinitionParams {
    pub uri: Uri,
    pub element_id: String,
    pub attribute: String,
    #[serde(default)]
    pub open_files: HashMap<String, String>,
}

/// See [`BindingDefinitionParams`] (call inside [`crate::sources::with_overlays`]).
pub fn binding_definition(doc: Option<&Document>, p: &BindingDefinitionParams) -> Option<Location> {
    let doc = doc?;
    let ast = kubuno_views::ast::Document::cast(doc.parse.syntax())?;
    let element = ast.resolve_id(&p.element_id)?;
    let range = element.attribute(&p.attribute)?.value_range()?;
    let start = usize::from(range.start());
    let raw = doc.text.get(start..usize::from(range.end()))?;
    let parts = binding_parts(raw)?;
    let starts: Vec<usize> = [path_part(&parts), value_of(&parts, "Source"), value_of(&parts, "Converter")].into_iter().flatten().map(|part| part.value_range.start).collect();
    starts.into_iter().find_map(|at| {
        let pos = doc.position_index.offset_to_position(&doc.text, rowan::TextSize::from(u32::try_from(start + at).ok()?));
        definition(doc, &p.uri, pos)
    })
}

/// Hover on a binding's path: the member's type, shape and documentation.
pub fn hover(doc: &Document, uri: &Uri, pos: Position) -> Option<Hover> {
    let (member, _, range) = target_at(doc, uri, pos)?;
    let m = member?;
    let mut text = format!("**{}** · {}", m.path, m.rust_type.clone().unwrap_or_else(|| shape_label(m.shape).to_string()));
    if !m.writable {
        text.push_str(" · read-only");
    }
    if !m.doc.is_empty() {
        text.push_str("\n\n");
        text.push_str(&m.doc);
    }
    Some(Hover { contents: HoverContents::Markup(MarkupContent { kind: MarkupKind::Markdown, value: text }), range: Some(range) })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::binding_sources::{Context, ConverterInfo, MemberKind};

    fn member(path: &str, shape: Shape) -> Member {
        Member {
            name: path.into(),
            path: path.into(),
            expression: format!("{{Binding {path}}}"),
            kind: MemberKind::Field,
            rust_type: None,
            shape,
            writable: true,
            doc: String::new(),
            location: None,
            children: Vec::new(),
        }
    }

    fn schema() -> Schema {
        let mut count = member("customers.Count", Shape::Number);
        count.writable = false;
        count.name = "Count".into();
        let mut name = member("customers.Name", Shape::Any);
        name.name = "Name".into();
        name.kind = MemberKind::Column;
        let mut customers = member("customers", Shape::List);
        customers.kind = MemberKind::Component;
        customers.children = vec![count, name];
        Schema {
            context: Context { label: "Vm".into(), members: vec![member("Title", Shape::Text), member("IsBusy", Shape::Bool), member("Items", Shape::List), member("StatusText", Shape::Text)], open: false, file: None },
            item: None,
            components: vec![customers],
            resources: Vec::new(),
            converters: vec![ConverterInfo { name: "Not".into(), output: Shape::Bool, two_way: true, doc: String::new(), project: false, location: None }],
        }
    }

    fn codes(issues: &[Issue]) -> Vec<&str> {
        issues.iter().map(|i| i.code).collect()
    }

    #[test]
    fn a_known_path_of_the_right_shape_is_fine() {
        assert!(check(&schema(), "TextField", "Text", "{Binding Title, Mode=TwoWay}", &[]).is_empty());
        assert!(check(&schema(), "Label", "Text", "{Binding Source=customers, Path=Name}", &[]).is_empty());
        assert!(check(&schema(), "Label", "Text", "{Binding customers.Current.Name}", &[]).is_empty());
    }

    #[test]
    fn unknown_paths_converters_and_keys_are_reported() {
        let raw = "{Binding Titel, Converter=Nope, Mod=TwoWay}";
        let issues = check(&schema(), "Label", "Text", raw, &["Title".to_string()]);
        assert_eq!(codes(&issues), ["binding-syntax", "binding-unknown-converter", UNKNOWN_PATH]);
        assert_eq!(&raw[issues[2].range.clone()], "Titel");
        // `Title` is already bound elsewhere: no rename guess.
        assert!(issues[2].rename.is_none());
        let issues = check(&schema(), "Label", "Text", "{Binding Status}", &[]);
        assert_eq!(issues[0].rename, Some(("Status".to_string(), "StatusText".to_string())));
        let issues = check(&schema(), "Label", "Text", "{Binding Source=orders, Path=Total}", &[]);
        assert!(issues[0].message.contains("no data component named `orders`"));
    }

    #[test]
    fn shapes_and_read_only_members_are_checked() {
        assert_eq!(codes(&check(&schema(), "Label", "Text", "{Binding Items}", &[])), ["binding-type"]);
        assert!(check(&schema(), "Label", "Text", "{Binding Items}", &[])[0].message.contains("Converter=Count"));
        assert!(check(&schema(), "Button", "Enabled", "{Binding IsBusy, Converter=Not}", &[]).is_empty());
        let info = check(&schema(), "Switch", "On", "{Binding Title}", &[]);
        assert_eq!(info[0].severity, DiagnosticSeverity::INFORMATION);
        assert_eq!(codes(&check(&schema(), "NumericField", "Value", "{Binding Source=customers, Path=Count, Mode=TwoWay}", &[])), ["binding-read-only"]);
    }

    #[test]
    fn an_open_context_does_not_report_unknown_paths() {
        let mut s = schema();
        s.context.open = true;
        assert!(check(&s, "Label", "Text", "{Binding Whatever}", &[]).is_empty());
    }

    #[test]
    fn distance_is_an_edit_distance() {
        assert_eq!(distance("kitten", "sitting"), 3);
        assert_eq!(distance("", "ab"), 2);
    }

    #[test]
    fn the_preview_runs_the_runtime_formats_and_converters() {
        let p = |expression: &str, value: Option<&str>, shape: &str, want: &str| {
            binding_preview(&BindingPreviewParams { expression: expression.into(), value: value.map(str::to_string), shape: Some(shape.into()), want: Some(want.into()) })
        };
        assert_eq!(p("{Binding Total, StringFormat=N2, Culture=en-US}", Some("1234.5"), "Number", "Text").text.as_deref(), Some("1,234.50"));
        assert_eq!(p("{Binding Name, Converter=ToUpper}", Some("ada"), "Text", "Text").text.as_deref(), Some("ADA"));
        assert_eq!(p("{Binding Busy, Converter=Not}", Some("true"), "Bool", "Bool").text.as_deref(), Some("false"));
        assert_eq!(p("{Binding Name, FallbackValue='(none)'}", None, "Text", "Text").text.as_deref(), Some("(none)"));
        assert_eq!(p("{Binding Name, TargetNullValue='-'}", Some(""), "Text", "Text").text.as_deref(), Some("-"));
        let mine = p("{Binding Name, Converter=MyOwn}", Some("x"), "Text", "Text");
        assert!(mine.note.is_some());
    }

    /// A package on disk: a form class with `#[bind]` fields, a repeater whose rows come from a
    /// `Row::new().with(…)` chain and a sample file, a binding source, a project converter.
    struct Package {
        dir: std::path::PathBuf,
        store: crate::documents::DocumentStore,
        uri: Uri,
    }

    impl Drop for Package {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.dir).ok();
        }
    }

    const VIEW: &str = r#"<Form x:Class="MainForm" Text="{Binding Title}">
  <Label x:Name="status" Text="{Binding Statut}" Visible="{Binding IsBusy, Converter=Not}"/>
  <TableAdapter x:Name="customers_adapter" SelectCommand="SELECT id, name AS Name FROM customers"/>
  <BindingSource x:Name="customers" DataSource="customers_adapter"/>
  <Label Text="{Binding Source=customers, Path=Name, Converter=Initials}"/>
  <Label Text="{Binding Items}"/>
  <Repeater x:Name="list" ItemsSource="{Binding Items}" d:ItemsSource="sample.json">
    <Label Text="{Binding Caption}" Visible="{Binding Shown}"/>
  </Repeater>
</Form>
"#;

    const CODE: &str = r#"use kubuno::prelude::*;

#[kubuno::view("main_form.kbview")]
pub struct MainForm {
    /// The window's title.
    #[bind]
    title: String,
    #[bind]
    is_busy: bool,
    #[bind("Items")]
    items: Rows,
    #[bind]
    status_text: String,
}

fn rows() -> Rows {
    Rows::from(vec![Row::new().with("Caption", Value::Str(String::new())).with("Shown", Value::Bool(true))])
}

#[derive(Default)]
struct Initials;

#[kubuno::views::value_converter]
impl ValueConverter for Initials {
    fn convert(&self, value: Option<Value>, _: Option<&str>) -> Option<Value> { value }
}
"#;

    fn package() -> Package {
        let dir = std::env::temp_dir().join(format!("kubuno-ls-bindings-{}-{:?}", std::process::id(), std::thread::current().id()).replace(['(', ')'], ""));
        let src = dir.join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(dir.join("Cargo.toml"), "[package]\nname = \"app\"\nversion = \"0.1.0\"\n").unwrap();
        std::fs::write(src.join("main_form.rs"), CODE).unwrap();
        std::fs::write(src.join("main_form.kbview"), VIEW).unwrap();
        std::fs::write(src.join("sample.json"), r#"[{"Caption": "One", "Shown": true, "Extra": 3}]"#).unwrap();
        let uri = crate::fs_uri::from_path(&src.join("main_form.kbview")).unwrap();
        let mut store = crate::documents::DocumentStore::new();
        store.open(uri.clone(), VIEW.to_string(), 1);
        Package { dir, store, uri }
    }

    fn pos_of(text: &str, needle: &str, plus: usize) -> Position {
        crate::binding_sources::position_in(text, text.find(needle).unwrap() + plus)
    }

    #[test]
    fn the_schema_of_a_real_package() {
        let p = package();
        let doc = p.store.get(&p.uri).unwrap();
        let ast = kubuno_views::ast::Document::cast(doc.parse.syntax()).unwrap();
        let root = ast.root_element().unwrap();
        let caption = root.syntax().descendants().filter_map(Element::cast).find(|e| e.attribute("Text").and_then(|a| a.value()).as_deref() == Some("{Binding Caption}")).unwrap();
        let params = crate::binding_sources::BindingSourcesParams { uri: p.uri.clone(), element_id: Some(caption.stable_id()), open_files: HashMap::new() };
        let result = crate::binding_sources::binding_sources(Some(doc), &params);
        let s = &result.schema;
        let paths: Vec<&str> = s.context.members.iter().map(|m| m.path.as_str()).collect();
        assert_eq!(paths, ["Title", "IsBusy", "Items", "StatusText"]);
        assert_eq!(s.context.label, "MainForm");
        assert_eq!(s.context.members[0].doc, "The window's title.");
        assert!(s.context.members[0].location.as_ref().is_some_and(|l| l.uri.as_str().ends_with("main_form.rs") && l.range.start.line == 6));
        let item: Vec<(&str, Shape)> = s.item.as_ref().unwrap().members.iter().map(|m| (m.path.as_str(), m.shape)).collect();
        assert_eq!(item, [("Caption", Shape::Text), ("Extra", Shape::Number), ("Shown", Shape::Bool)]);
        let customers = s.components.iter().find(|c| c.path == "customers").unwrap();
        assert!(customers.children.iter().any(|m| m.path == "customers.Name" && m.kind == MemberKind::Column));
        assert!(s.converters.iter().any(|c| c.name == "Initials" && c.project));
        assert!(s.converters.iter().any(|c| c.name == "Not" && !c.project));
        assert!(result.issues.is_empty(), "{:?}", result.issues);
    }

    #[test]
    fn diagnostics_quick_fix_completion_and_definition() {
        let p = package();
        let doc = p.store.get(&p.uri).unwrap();
        let diags = diagnostics(doc, &p.uri);
        let summary: Vec<(String, String)> = diags.iter().map(|d| (format!("{:?}", d.code), d.message.clone())).collect();
        // `Statut` is unknown (renamed `StatusText`?), `Items` (a list) in a text property.
        assert_eq!(diags.len(), 2, "{summary:?}");
        let unknown = diags.iter().find(|d| d.code == Some(NumberOrString::String(UNKNOWN_PATH.into()))).unwrap();
        assert_eq!(unknown.data, Some(serde_json::json!({ "oldPath": "Statut", "newPath": "StatusText" })));
        let actions = code_actions(doc, &p.uri, std::slice::from_ref(unknown));
        assert_eq!(actions.len(), 1);
        assert!(actions[0].title.starts_with("Mettre à jour les liaisons"));
        let edits = &actions[0].edit.as_ref().unwrap().changes.as_ref().unwrap()[&p.uri];
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].new_text, "StatusText");

        // Completion of the path, of the converter, of a source's members.
        let labels = |pos| completion(doc, &p.uri, pos).unwrap().into_iter().map(|i| i.label).collect::<Vec<_>>();
        let at_path = labels(pos_of(VIEW, "{Binding Statut}", "{Binding ".len()));
        assert!(at_path.contains(&"StatusText".to_string()) && at_path.contains(&"customers".to_string()));
        let in_item = labels(pos_of(VIEW, "{Binding Caption}", "{Binding ".len()));
        assert!(in_item.contains(&"Caption".to_string()) && in_item.contains(&"Title".to_string()));
        let converters = labels(pos_of(VIEW, "Converter=Not", "Converter=".len()));
        assert!(converters.contains(&"Initials".to_string()) && converters.contains(&"ToUpper".to_string()));
        let source_members = labels(pos_of(VIEW, "Path=Name", "Path=".len()));
        assert!(source_members.contains(&"Name".to_string()) && source_members.contains(&"Position".to_string()));
        assert!(completion(doc, &p.uri, pos_of(VIEW, "x:Class", 0)).is_none());

        // F12: the `#[bind]` field, the row field, the project converter.
        let def = definition(doc, &p.uri, pos_of(VIEW, "{Binding Title}", "{Binding T".len())).unwrap();
        assert!(def.uri.as_str().ends_with("main_form.rs"));
        assert_eq!(def.range.start.line, 6);
        let def = definition(doc, &p.uri, pos_of(VIEW, "{Binding Shown}", "{Binding S".len())).unwrap();
        assert!(def.uri.as_str().ends_with("sample.json") || def.uri.as_str().ends_with("main_form.rs"));
        let def = definition(doc, &p.uri, pos_of(VIEW, "Converter=Initials", "Converter=I".len())).unwrap();
        assert!(def.uri.as_str().ends_with("main_form.rs"));
        let hover = hover(doc, &p.uri, pos_of(VIEW, "{Binding Title}", "{Binding T".len())).unwrap();
        let HoverContents::Markup(m) = hover.contents else { panic!("markup") };
        assert!(m.value.contains("String") && m.value.contains("The window's title."));

        // « Aller à la définition » from the Properties window: by element and attribute.
        let params = BindingDefinitionParams { uri: p.uri.clone(), element_id: String::new(), attribute: "Text".into(), open_files: HashMap::new() };
        let def = binding_definition(Some(doc), &params).unwrap();
        assert_eq!(def.range.start.line, 6);
        let params = BindingDefinitionParams { uri: p.uri.clone(), element_id: "0".into(), attribute: "Visible".into(), open_files: HashMap::new() };
        assert!(binding_definition(Some(doc), &params).unwrap().uri.as_str().ends_with("main_form.rs"));
    }
}
