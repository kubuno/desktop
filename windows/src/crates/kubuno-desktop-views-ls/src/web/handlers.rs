//! Event handlers of web views, in the TypeScript code-behind (`vskubuno/docs/WEB-VIEWS.md` §5, WV-7): the same
//! LSP methods and payloads as the desktop (`kubuno/compatibleHandlers`, `kubuno/createHandler`,
//! `kubuno/renameHandler`, `kubuno/removeHandler`, F2), answered from the view's class instead of Rust.
//!
//! - **Handler set**: the instance methods of the class extending `ViewBase` (not private, not static, not the
//!   runtime's own members such as `use`).
//! - **Compatibility** (the rule the generated `abstract handler(sender, e)` of `ViewBase` implies, kept strict like the
//!   desktop's): at most two required parameters; the first (the sender) untyped, `any`, `unknown`, `object`,
//!   `ElementHandle` or the element's handle type (`Button`; `ElementHandle` for project controls); the second (the
//!   arguments) untyped, `any`, `unknown`, or one of the event's `args_chain` (`MouseEventArgs`, `EventArgs`).
//! - **Edits** are insertions and identifier replacements only, never a reprint of the user's code: a new method is
//!   inserted before the class's closing brace with the indentation and line endings the file already uses, and its
//!   types are added to the existing `@kubuno/views` import (`, type X`) or by one new `import type` line.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use kubuno_desktop_views_model::EventEntry;
use kubuno_desktop_views_syntax::ast::{AstNode, Attribute, Document as AstDocument, Element};
use kubuno_desktop_views_syntax::syntax::{parse, Parse};
use kubuno_web_views_compiler_core::compile::RESERVED_MEMBERS;
use kubuno_web_views_compiler_core::registry::{Origin, WebRegistry};
use lsp_types::{CodeAction, CodeActionKind, Diagnostic, DiagnosticSeverity, Location, NumberOrString, Position, PrepareRenameResponse, Range, TextEdit, Uri, WorkspaceEdit};

use super::project::{self, WebProject};
use super::ts::{self, ClassInfo, Member, TsFile};
use super::{lsp_range, text_edits, WebView};
use crate::documents::{Document, DocumentStore};
use crate::handler_insert::{CreateHandlerParams, CreateHandlerResult};
use crate::handlers::{CompatibleHandlersParams, CompatibleHandlersResult, RemoveHandlerParams, RemoveHandlerResult, RenameHandlerParams, RenameHandlerResult};

/// The `source` of the web handler diagnostics.
pub const SOURCE: &str = "kubuno-web-views";

// ── the code-behind ─────────────────────────────────────────────────────

/// A view's code-behind file and its class.
pub struct CodeBehind {
    pub file: TsFile,
    class: Option<usize>,
}

impl CodeBehind {
    /// The code-behind of `view` (`X.ts` / `X.tsx` next to it), read through the client's open buffers.
    pub fn load(view: &WebView) -> Option<Self> {
        let path = project::code_behind_of(&view.path)?;
        let file = TsFile::load(&path)?;
        let class = file.classes.iter().position(|c| c.extends.as_deref() == Some("ViewBase")).or_else(|| file.classes.iter().position(|c| c.name == view.stem));
        Some(Self { file, class })
    }

    pub fn class(&self) -> Option<&ClassInfo> {
        self.class.map(|i| &self.file.classes[i])
    }

    /// The method named `name` (any member kind for "already taken" checks: see [`Self::taken`]).
    pub fn method(&self, name: &str) -> Option<&Member> {
        self.class()?.methods().find(|m| m.name == name)
    }

    /// Whether `name` is used by a member of the class or the runtime's view base.
    pub fn taken(&self, name: &str) -> bool {
        RESERVED_MEMBERS.contains(&name) || self.class().is_some_and(|c| c.member(name).is_some())
    }

    pub fn path(&self) -> &Path {
        &self.file.path
    }
}

/// Whether `m` is a handler candidate (an instance method that is not the view's own runtime member).
fn is_handler_candidate(m: &Member) -> bool {
    m.is_method() && !RESERVED_MEMBERS.contains(&m.name.as_str())
}

/// The type names of a type annotation: its union members, without generics, parentheses or `readonly`.
fn type_names(t: &str) -> Vec<String> {
    let mut depth = 0i32;
    let mut parts = Vec::new();
    let mut current = String::new();
    for c in t.chars() {
        match c {
            '<' | '(' | '[' | '{' => {
                depth += 1;
                current.push(c);
            }
            '>' | ')' | ']' | '}' => {
                depth -= 1;
                current.push(c);
            }
            '|' if depth == 0 => parts.push(std::mem::take(&mut current)),
            _ => current.push(c),
        }
    }
    parts.push(current);
    parts
        .into_iter()
        .map(|p| {
            let p = p.trim().trim_start_matches("readonly ").trim();
            let p = p.trim_start_matches('(').trim_end_matches(')').trim();
            p.split('<').next().unwrap_or(p).trim().to_string()
        })
        .filter(|p| !p.is_empty())
        .collect()
}

/// Whether `name` provably names another element's handle (an element of the registry other than `handle`); any
/// other type (`ElementHandle`, `any`, an alias, an object type) is left to TypeScript (`kbview-tsc`).
fn sender_fits(t: Option<&str>, handle: &str, registry: &WebRegistry) -> bool {
    t.is_none_or(|t| type_names(t).iter().any(|n| n == handle || registry.get(n).is_none()))
}

/// Whether the arguments type fits: anything but an `…EventArgs` type outside the event's `args_chain`.
fn args_fit(t: Option<&str>, event: &EventEntry) -> bool {
    t.is_none_or(|t| type_names(t).iter().any(|n| !n.ends_with("EventArgs") || n == "EventArgs" || *n == event.args_type || event.args_chain.iter().any(|c| c == n)))
}

/// Whether the method `m` can handle `event` raised by an element whose handle type is `handle`.
pub fn accepts(m: &Member, handle: &str, event: &EventEntry, registry: &WebRegistry) -> bool {
    if !is_handler_candidate(m) {
        return false;
    }
    if m.params.iter().skip(2).any(|p| !p.optional) {
        return false;
    }
    sender_fits(m.params.first().and_then(|p| p.type_text.as_deref()), handle, registry) && args_fit(m.params.get(1).and_then(|p| p.type_text.as_deref()), event)
}

// ── the view's events ───────────────────────────────────────────────────

/// One event of an element of the view.
pub struct EventSite {
    pub element: Element,
    pub element_name: String,
    pub element_id: String,
    pub is_root: bool,
    pub event: EventEntry,
    /// The handle type of the sender (`Button`, `ElementHandle`): what the generated `ViewBase` declares.
    pub handle: String,
}

/// The handle type of an element: its own name for a host element, `ElementHandle` for project controls (the
/// compiler's rule, `compile.rs`).
pub fn handle_of(registry: &WebRegistry, element: &str) -> String {
    match registry.get(element) {
        Some(e) if e.origin == Origin::Host => element.to_string(),
        _ => "ElementHandle".to_string(),
    }
}

fn is_root(element: &Element) -> bool {
    element.syntax().parent().is_none_or(|p| p.kind() != kubuno_desktop_views_syntax::syntax::SyntaxKind::ELEMENT)
}

/// The event `event_name` (canonical or alias) of the element `element_id`.
pub fn resolve_event(registry: &WebRegistry, parse: &Parse, element_id: &str, event_name: &str) -> Option<EventSite> {
    let element = AstDocument::cast(parse.syntax())?.resolve_id(element_id)?;
    site(registry, element, event_name)
}

fn site(registry: &WebRegistry, element: Element, event_name: &str) -> Option<EventSite> {
    let name = element.name()?;
    let meta = registry.get(&name)?;
    let root = is_root(&element);
    let event = meta.entry.event(event_name).filter(|e| root || !e.root_only)?.clone();
    Some(EventSite { element_id: element.stable_id(), element_name: name.clone(), is_root: root, handle: handle_of(registry, &name), event, element })
}

/// Every event attribute of `parse`: the site and the attribute.
pub fn event_attributes(registry: &WebRegistry, parse: &Parse) -> Vec<(EventSite, Attribute)> {
    let Some(root) = AstDocument::cast(parse.syntax()).and_then(|d| d.root_element()) else { return Vec::new() };
    let mut out = Vec::new();
    for element in root.syntax().descendants().filter_map(Element::cast) {
        for attr in element.attributes() {
            let Some(name) = attr.name() else { continue };
            if !name.starts_with("On") {
                continue;
            }
            if let Some(site) = site(registry, element.clone(), &name) {
                out.push((site, attr));
            }
        }
    }
    out
}

// ── kubuno/compatibleHandlers ───────────────────────────────────────────

pub fn compatible_handlers(doc: Option<&Document>, view: &WebView, p: &CompatibleHandlersParams) -> CompatibleHandlersResult {
    let Some(doc) = doc else { return CompatibleHandlersResult::default() };
    project::with_project(&view.root, |project| {
        let Some(site) = resolve_event(&project.session.registry, &doc.parse, &p.element_id, &p.event) else { return CompatibleHandlersResult::default() };
        let Some(code) = CodeBehind::load(view) else { return CompatibleHandlersResult::default() };
        let handlers = code.class().map(|c| c.members.iter().filter(|m| accepts(m, &site.handle, &site.event, &project.session.registry)).map(|m| m.name.clone()).collect()).unwrap_or_default();
        CompatibleHandlersResult { handlers }
    })
}

// ── naming ──────────────────────────────────────────────────────────────

/// The handler name the designer gives an event (`VIEWS-SPEC.md` §8): `<x:Name>_<event>` in snake_case, the element's
/// name without `x:Name`, the view's file stem for the root's own events (`account_menu_load`).
pub fn default_name(x_name: Option<&str>, element: &str, event: &EventEntry, is_root: bool, stem: &str) -> String {
    let snake = crate::handler_insert::to_snake_case;
    let subject = x_name
        .filter(|s| !s.is_empty())
        .map(snake)
        .filter(|s| !s.is_empty())
        .or_else(|| if is_root { Some(snake(stem)).filter(|s| !s.is_empty()) } else { None })
        .unwrap_or_else(|| snake(element));
    let display = if event.display_name.is_empty() { event.name.strip_prefix("On").unwrap_or(&event.name).to_string() } else { event.display_name.clone() };
    format!("{subject}_{}", snake(&display))
}

fn unique(code: &CodeBehind, base: &str) -> String {
    if !code.taken(base) {
        return base.to_string();
    }
    (2..).map(|n| format!("{base}_{n}")).find(|c| !code.taken(c)).unwrap_or_else(|| base.to_string())
}

// ── inserting a method and its type imports ─────────────────────────────

/// Whether the project type-checks with `noUnusedParameters` (a generated handler then names its parameters `_sender`
/// / `_e`, as the core's own code-behinds do).
fn no_unused_parameters(root: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(root) else { return false };
    entries.flatten().any(|e| {
        let name = e.file_name().to_string_lossy().to_lowercase();
        name.starts_with("tsconfig") && name.ends_with(".json") && std::fs::read_to_string(e.path()).is_ok_and(|t| {
            let compact: String = t.chars().filter(|c| !c.is_whitespace()).collect();
            compact.contains("\"noUnusedParameters\":true")
        })
    })
}

/// The edits adding `types` (type-only) to the file's imports from `@kubuno/views`, skipping names it already binds.
pub fn import_edits(file: &TsFile, types: &[&str]) -> Vec<(usize, usize, String)> {
    let mut missing: Vec<&str> = Vec::new();
    for t in types {
        if !file.binds(t) && !missing.contains(t) {
            missing.push(t);
        }
    }
    if missing.is_empty() {
        return Vec::new();
    }
    let text = &file.text;
    let existing = file.imports.iter().rfind(|i| i.source == "@kubuno/views" && i.braces.is_some() && !i.names.iter().any(|n| n.imported == "*"));
    if let Some(import) = existing {
        let (open, close) = import.braces.unwrap_or_default();
        let item = |t: &str| if import.type_only { t.to_string() } else { format!("type {t}") };
        let items: Vec<String> = missing.iter().map(|t| item(t)).collect();
        return match import.names.iter().rfind(|n| n.span.0 > open && n.span.1 <= close) {
            Some(last) => {
                // After the last name, or after its trailing comma (a multi-line list keeps one name per line).
                let between = &text[last.span.1..close];
                match between.find(',') {
                    Some(comma) => {
                        let at = last.span.1 + comma + 1;
                        let indent = ts::indentation_at(text, last.span.0).to_string();
                        let multiline = between.contains('\n');
                        let joined = if multiline {
                            items.iter().map(|i| format!("{}{indent}{i},", file.newline())).collect::<String>()
                        } else {
                            items.iter().map(|i| format!(" {i},")).collect()
                        };
                        vec![(at, at, joined)]
                    }
                    None => vec![(last.span.1, last.span.1, items.iter().map(|i| format!(", {i}")).collect())],
                }
            }
            None => vec![(open + 1, open + 1, format!(" {} ", items.join(", ")))],
        };
    }
    let q = file.quote();
    let semi = if file.semicolons() { ";" } else { "" };
    let nl = file.newline();
    let line = format!("import type {{ {} }} from {q}@kubuno/views{q}{semi}", missing.join(", "));
    match file.imports.last() {
        Some(last) => {
            let end = text[last.span.1..].find('\n').map_or(text.len(), |n| last.span.1 + n + 1);
            if end == text.len() && !text.ends_with('\n') {
                vec![(end, end, format!("{nl}{line}"))]
            } else {
                vec![(end, end, format!("{line}{nl}"))]
            }
        }
        None => vec![(0, 0, format!("{line}{nl}{nl}"))],
    }
}

/// The edit inserting the method `name(sender: handle, e: args): void { … }` at the end of `class`.
pub fn method_edit(file: &TsFile, class: &ClassInfo, name: &str, handle: &str, args: &str, underscore: bool) -> (usize, usize, String) {
    let text = &file.text;
    let nl = file.newline();
    let class_indent = ts::indentation_at(text, class.start).to_string();
    let member_indent = class.members.first().map(|m| ts::indentation_at(text, m.span.0).to_string()).filter(|i| i.len() > class_indent.len());
    let step = match &member_indent {
        Some(m) => m[class_indent.len()..].to_string(),
        None => if class_indent.contains('\t') { "\t".to_string() } else { "  ".to_string() },
    };
    let indent = member_indent.unwrap_or_else(|| format!("{class_indent}{step}"));
    let (s, e) = if underscore { ("_sender", "_e") } else { ("sender", "e") };
    let method = format!("{indent}{name}({s}: {handle}, {e}: {args}): void {{{nl}{indent}{step}// TODO: implement {name}{nl}{indent}}}{nl}");
    let close = class.body_close;
    let line_start = ts::line_start(text, close);
    let alone = text[line_start..close].trim().is_empty();
    let empty_body = text[class.body_open + 1..close].trim().is_empty();
    if alone {
        let blank = if empty_body { "" } else { nl };
        (line_start, line_start, format!("{blank}{method}"))
    } else {
        // `class X extends ViewBase {}` on one line: open the body.
        (close, close, format!("{nl}{method}{class_indent}"))
    }
}

/// The edits of the code-behind adding the handler `name` for `site` (method + imports), as LSP edits.
fn stub_edits(code: &CodeBehind, root: &Path, name: &str, site: &EventSite) -> Option<Vec<TextEdit>> {
    let class = code.class()?;
    let mut edits = import_edits(&code.file, &[site.handle.as_str(), site.event.args_type.as_str()]);
    edits.push(method_edit(&code.file, class, name, &site.handle, &site.event.args_type, no_unused_parameters(root)));
    Some(text_edits(&code.file.text, edits))
}

fn view_edits(doc_text: &str, edits: Vec<kubuno_desktop_views_syntax::edit::Edit>) -> Vec<TextEdit> {
    text_edits(doc_text, edits.into_iter().map(|e| (u32::from(e.range.start()) as usize, u32::from(e.range.end()) as usize, e.new_text)))
}

#[allow(clippy::mutable_key_type)] // `Uri`'s memoizing `Cell`, never mutated here (see `handler_insert::create_handler`).
fn workspace_edit(changes: Vec<(Uri, Vec<TextEdit>)>) -> Option<WorkspaceEdit> {
    let mut map: HashMap<Uri, Vec<TextEdit>> = HashMap::new();
    for (uri, edits) in changes.into_iter().filter(|(_, e)| !e.is_empty()) {
        map.entry(uri).or_default().extend(edits);
    }
    (!map.is_empty()).then(|| WorkspaceEdit { changes: Some(map), ..Default::default() })
}

fn member_location(code: &CodeBehind, m: &Member) -> Option<Location> {
    super::location(code.path(), &code.file.text, m.name_span.0, m.name_span.1)
}

// ── kubuno/createHandler ────────────────────────────────────────────────

pub fn create_handler(doc: Option<&Document>, uri: &Uri, view: &WebView, p: &CreateHandlerParams) -> CreateHandlerResult {
    let Some(doc) = doc else { return CreateHandlerResult::default() };
    project::with_project(&view.root, |project| create_in(project, doc, uri, view, p)).unwrap_or_default()
}

fn create_in(project: &WebProject, doc: &Document, uri: &Uri, view: &WebView, p: &CreateHandlerParams) -> Option<CreateHandlerResult> {
    let site = resolve_event(&project.session.registry, &doc.parse, &p.element_id, &p.event)?;
    let code = CodeBehind::load(view)?;
    code.class()?;
    // Already wired (under its canonical name or an older alias): navigate, never a second attribute.
    let existing = std::iter::once(site.event.name.as_str())
        .chain(site.event.aliases.iter().map(String::as_str))
        .find_map(|n| site.element.attribute(n).and_then(|a| a.value()).filter(|v| !v.trim().is_empty()));
    if let Some(existing) = existing {
        let location = code.method(existing.trim()).and_then(|m| member_location(&code, m));
        return Some(CreateHandlerResult { handler_name: existing.trim().to_string(), location, edit: None });
    }
    let suggested = p.suggested_name.as_deref().map(str::trim).filter(|s| !s.is_empty());
    // A name picked from the ⚡ dropdown: an existing method is only bound (WinForms).
    if let Some(name) = suggested.filter(|n| code.method(n).is_some()) {
        let edits = view_edits(&doc.text, kubuno_desktop_views_syntax::edit::set_attribute(&site.element, &p.event, name));
        return Some(CreateHandlerResult { handler_name: name.to_string(), location: None, edit: workspace_edit(vec![(uri.clone(), edits)]) });
    }
    let x_name = site.element.attribute("x:Name").and_then(|a| a.value());
    let base = suggested.map(str::to_string).unwrap_or_else(|| default_name(x_name.as_deref(), &site.element_name, &site.event, site.is_root, &view.stem));
    if !kubuno_web_views_compiler_core::compile::is_identifier(&base) {
        return None;
    }
    let name = unique(&code, &base);
    let attribute = kubuno_desktop_views_syntax::edit::set_attribute(&site.element, &p.event, &name);
    if attribute.is_empty() {
        return None;
    }
    let ts_edits = stub_edits(&code, &view.root, &name, &site)?;
    let ts_uri = crate::fs_uri::from_path(code.path())?;
    Some(CreateHandlerResult {
        handler_name: name,
        location: None,
        edit: workspace_edit(vec![(uri.clone(), view_edits(&doc.text, attribute)), (ts_uri, ts_edits)]),
    })
}

// ── kubuno/renameHandler ────────────────────────────────────────────────

fn not_renamed(reason: impl Into<String>) -> RenameHandlerResult {
    RenameHandlerResult { reason: Some(reason.into()), ..Default::default() }
}

/// The views of a code-behind (same folder, same stem): their URI, text and parse (the open document when the
/// server has it).
fn views_of(stem_path: &Path, documents: &DocumentStore) -> Vec<(Uri, String, Parse)> {
    let mut out = Vec::new();
    for ext in ["kbview", "kbcontrol"] {
        let path = stem_path.with_extension(ext);
        let key = crate::sources::key(&path);
        let open = documents.iter().find(|(u, _)| crate::fs_uri::to_path(u).is_some_and(|p| crate::sources::key(&p) == key));
        match open {
            Some((u, d)) => out.push((u.clone(), d.text.clone(), d.parse.clone())),
            None => {
                if let (Some(text), Some(u)) = (crate::sources::read(&path), crate::fs_uri::from_path(&path)) {
                    let parse = parse(&text);
                    out.push((u, text, parse));
                }
            }
        }
    }
    out
}

/// `this.<old>` occurrences (not followed by an identifier character) of `text`: byte offsets of `<old>`.
fn this_references(text: &str, old: &str) -> Vec<usize> {
    let needle = format!("this.{old}");
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(i) = text[from..].find(&needle) {
        let at = from + i;
        let end = at + needle.len();
        let before_ok = at == 0 || !(text.as_bytes()[at - 1].is_ascii_alphanumeric() || matches!(text.as_bytes()[at - 1], b'_' | b'$' | b'.'));
        let after_ok = text[end..].chars().next().is_none_or(|c| !(c.is_alphanumeric() || c == '_' || c == '$'));
        if before_ok && after_ok {
            out.push(at + "this.".len());
        }
        from = end;
    }
    out
}

pub fn rename_handler(documents: &DocumentStore, view: &WebView, p: &RenameHandlerParams) -> RenameHandlerResult {
    project::with_project(&view.root, |project| rename_in(project, documents, view, p))
}

fn rename_in(project: &WebProject, documents: &DocumentStore, view: &WebView, p: &RenameHandlerParams) -> RenameHandlerResult {
    // The request may come from the `.ts` (F2 on the method): its view is the same stem's.
    let is_ts = view.path.extension().is_some_and(|e| e.eq_ignore_ascii_case("ts") || e.eq_ignore_ascii_case("tsx"));
    let view_for_code = if is_ts {
        let found = ["kbview", "kbcontrol"].iter().map(|e| view.path.with_extension(e)).find(|p| p.is_file());
        match found {
            Some(path) => WebView { path, ..view.clone() },
            None => return not_renamed("no view uses this code-behind"),
        }
    } else {
        view.clone()
    };
    let Some(code) = CodeBehind::load(&view_for_code) else { return not_renamed("the view has no TypeScript code-behind") };
    let Some(class) = code.class() else { return not_renamed("the code-behind has no class extending ViewBase") };
    let old = match (&p.old, p.position) {
        (Some(old), _) => old.clone(),
        (None, Some(pos)) if is_ts => {
            let offset = u32::from(crate::position::PositionIndex::new(&code.file.text).position_to_offset(&code.file.text, pos)) as usize;
            match class.members.iter().find(|m| m.name_span.0 <= offset && offset <= m.name_span.1) {
                Some(m) if is_handler_candidate(m) => m.name.clone(),
                Some(m) => return not_renamed(format!("`{}` is not a handler", m.name)),
                None => return not_renamed("no method at the cursor"),
            }
        }
        _ => return not_renamed("no handler to rename"),
    };
    let new = p.new.trim();
    if new == old {
        return RenameHandlerResult { old_name: Some(old), ..Default::default() };
    }
    if !kubuno_web_views_compiler_core::compile::is_identifier(new) || RESERVED_MEMBERS.contains(&new) {
        return not_renamed(format!("`{new}` is not a valid method name"));
    }
    if !p.rust_renamed && code.taken(new) {
        return not_renamed(format!("the code-behind already has a `{new}`"));
    }
    let mut changes: Vec<(Uri, Vec<TextEdit>)> = Vec::new();
    let stem_path = view_for_code.path.with_extension("");
    for (uri, text, parse) in views_of(&stem_path, documents) {
        let mut edits = Vec::new();
        for (site, attr) in event_attributes(&project.session.registry, &parse) {
            if attr.value().is_some_and(|v| v.trim() == old) {
                if let Some(name) = attr.name() {
                    edits.extend(view_edits(&text, kubuno_desktop_views_syntax::edit::set_attribute(&site.element, &name, new)));
                }
            }
        }
        changes.push((uri, edits));
    }
    if !p.rust_renamed {
        let mut spans: Vec<(usize, usize, String)> = Vec::new();
        if let Some(m) = class.member(&old) {
            spans.push((m.name_span.0, m.name_span.1, new.to_string()));
        }
        for at in this_references(&code.file.text, &old) {
            spans.push((at, at + old.len(), new.to_string()));
        }
        if let Some(uri) = crate::fs_uri::from_path(code.path()) {
            changes.push((uri, text_edits(&code.file.text, spans)));
        }
    }
    RenameHandlerResult { edit: workspace_edit(changes), old_name: Some(old), reason: None }
}

/// The handler named by the event attribute value under `pos`: its name and range.
fn handler_value_at(registry: &WebRegistry, doc: &Document, pos: Position) -> Option<(String, Range)> {
    let offset = doc.position_index.position_to_offset(&doc.text, pos);
    event_attributes(registry, &doc.parse).into_iter().find_map(|(_, attr)| {
        let r = attr.value_range()?;
        (r.start() <= offset && offset <= r.end()).then(|| (attr.value().unwrap_or_default(), lsp_range(&doc.text, u32::from(r.start()) as usize, u32::from(r.end()) as usize)))
    })
}

pub fn prepare_rename(doc: &Document, view: &WebView, pos: Position) -> Option<PrepareRenameResponse> {
    let (value, range) = project::with_project(&view.root, |p| handler_value_at(&p.session.registry, doc, pos))?;
    (!value.is_empty()).then_some(PrepareRenameResponse::RangeWithPlaceholder { range, placeholder: value })
}

pub fn rename(documents: &DocumentStore, uri: &Uri, view: &WebView, pos: Position, new_name: &str) -> Result<Option<WorkspaceEdit>, String> {
    let Some(doc) = documents.get(uri) else { return Ok(None) };
    let Some((old, _)) = project::with_project(&view.root, |p| handler_value_at(&p.session.registry, doc, pos)) else { return Ok(None) };
    let params = RenameHandlerParams { uri: uri.clone(), old: Some(old), new: new_name.to_string(), position: None, rust_renamed: false, open_files: HashMap::new() };
    let result = rename_handler(documents, view, &params);
    match result.reason {
        Some(reason) => Err(reason),
        None => Ok(result.edit),
    }
}

// ── kubuno/removeHandler ────────────────────────────────────────────────

/// Whether the method body `{ … }` is what `createHandler` wrote (or empty).
fn is_untouched_stub(text: &str, body: (usize, usize)) -> bool {
    let inner = text.get(body.0 + 1..body.1.saturating_sub(1)).unwrap_or_default();
    let words = inner.split_whitespace().collect::<Vec<_>>().join(" ");
    words.is_empty() || words.strip_prefix("// TODO: implement ").is_some_and(|rest| !rest.contains(' ') && kubuno_web_views_compiler_core::compile::is_identifier(rest))
}

/// The byte range deleting the member `m` with its whole lines and one blank line around it.
fn member_lines(text: &str, m: &Member) -> (usize, usize) {
    let start = ts::line_start(text, m.span.0);
    let end = text[m.span.1..].find('\n').map_or(text.len(), |n| m.span.1 + n + 1);
    if !text[start..m.span.0].trim().is_empty() || !text[m.span.1..end].trim().is_empty() {
        return (m.span.0, m.span.1);
    }
    // One blank line before (the one `createHandler` inserted) when there is one.
    if start > 0 {
        let prev = ts::line_start(text, start - 1);
        if text[prev..start].trim().is_empty() {
            return (prev, end);
        }
    }
    (start, end)
}

pub fn remove_handler(documents: &DocumentStore, view: &WebView, p: &RemoveHandlerParams) -> RemoveHandlerResult {
    let Some(doc) = documents.get(&p.uri) else { return RemoveHandlerResult::default() };
    project::with_project(&view.root, |project| {
        let Some(site) = resolve_event(&project.session.registry, &doc.parse, &p.element_id, &p.event) else { return RemoveHandlerResult::default() };
        let Some(attr_name) = std::iter::once(site.event.name.clone()).chain(site.event.aliases.iter().cloned()).find(|n| site.element.attribute(n).is_some()) else {
            return RemoveHandlerResult::default();
        };
        let handler = site.element.attribute(&attr_name).and_then(|a| a.value()).map(|v| v.trim().to_string()).filter(|v| !v.is_empty());
        let mut changes = vec![(p.uri.clone(), view_edits(&doc.text, kubuno_desktop_views_syntax::edit::remove_attribute(&site.element, &attr_name)))];
        let mut removed_stub = false;
        if let Some(name) = handler.as_deref() {
            let references = event_attributes(&project.session.registry, &doc.parse).iter().filter(|(_, a)| a.value().is_some_and(|v| v.trim() == name)).count();
            if references <= 1 {
                if let Some(code) = CodeBehind::load(view) {
                    let used_elsewhere = !this_references(&code.file.text, name).is_empty();
                    if let Some(m) = code.method(name).filter(|m| !used_elsewhere && m.body.is_some_and(|b| is_untouched_stub(&code.file.text, b))) {
                        let (s, e) = member_lines(&code.file.text, m);
                        if let Some(uri) = crate::fs_uri::from_path(code.path()) {
                            changes.push((uri, text_edits(&code.file.text, [(s, e, String::new())])));
                            removed_stub = true;
                        }
                    }
                }
            }
        }
        RemoveHandlerResult { edit: workspace_edit(changes), handler_name: handler, removed_stub }
    })
}

// ── diagnostics and quick fixes ─────────────────────────────────────────

/// A handler problem on one attribute value.
pub struct Finding {
    pub range: Range,
    pub site: EventSite,
    pub handler: String,
    pub missing: bool,
}

pub fn findings(registry: &WebRegistry, doc: &Document, code: &CodeBehind) -> Vec<Finding> {
    let Some(class) = code.class() else { return Vec::new() };
    if code.file.has_errors && class.members.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for (site, attr) in event_attributes(registry, &doc.parse) {
        let Some(handler) = attr.value().map(|v| v.trim().to_string()).filter(|v| !v.is_empty()) else { continue };
        let Some(r) = attr.value_range() else { continue };
        let member = class.member(&handler);
        let missing = member.is_none();
        if missing || !member.is_some_and(|m| accepts(m, &site.handle, &site.event, registry)) {
            out.push(Finding { range: lsp_range(&doc.text, u32::from(r.start()) as usize, u32::from(r.end()) as usize), site, handler, missing });
        }
    }
    out
}

pub fn diagnostics(registry: &WebRegistry, doc: &Document, view: &WebView) -> Vec<Diagnostic> {
    let Some(code) = CodeBehind::load(view) else { return Vec::new() };
    let class_name = code.class().map(|c| c.name.clone()).unwrap_or_default();
    findings(registry, doc, &code)
        .into_iter()
        .map(|f| {
            let (code_id, message) = if f.missing {
                (crate::handlers::MISSING_HANDLER, format!("handler `{}` not found in the code-behind (`{class_name}`)", f.handler))
            } else {
                (
                    crate::handlers::INCOMPATIBLE_HANDLER,
                    format!("handler `{}` cannot take the arguments of `{}` (`{}` from a `{}`)", f.handler, f.site.event.name, f.site.event.args_type, f.site.handle),
                )
            };
            Diagnostic {
                range: f.range,
                severity: Some(DiagnosticSeverity::WARNING),
                code: Some(NumberOrString::String(code_id.to_string())),
                source: Some(SOURCE.to_string()),
                message,
                data: Some(serde_json::json!({ "handler": f.handler, "elementId": f.site.element_id, "event": f.site.event.name })),
                ..Default::default()
            }
        })
        .collect()
}

fn overlaps(a: &Range, b: &Range) -> bool {
    let before = |p: &Position, q: &Position| (p.line, p.character) < (q.line, q.character);
    !before(&a.end, &b.start) && !before(&b.end, &a.start)
}

/// « Create handler `x` » and « Use `y` » on the handler warnings in `range`.
pub fn quick_fixes(registry: &WebRegistry, doc: &Document, uri: &Uri, view: &WebView, range: &Range) -> Vec<CodeAction> {
    let Some(code) = CodeBehind::load(view) else { return Vec::new() };
    let mut out = Vec::new();
    for f in findings(registry, doc, &code).into_iter().filter(|f| overlaps(&f.range, range)) {
        if f.missing && kubuno_web_views_compiler_core::compile::is_identifier(&f.handler) {
            if let (Some(edits), Some(ts_uri)) = (stub_edits(&code, &view.root, &f.handler, &f.site), crate::fs_uri::from_path(code.path())) {
                out.push(CodeAction {
                    title: format!("Create handler `{}`", f.handler),
                    kind: Some(CodeActionKind::QUICKFIX),
                    is_preferred: Some(true),
                    edit: workspace_edit(vec![(ts_uri, edits)]),
                    ..Default::default()
                });
            }
        }
        let limit = (f.handler.len() / 3).max(2);
        let closest = code
            .class()
            .into_iter()
            .flat_map(|c| c.members.iter())
            .filter(|m| m.name != f.handler && accepts(m, &f.site.handle, &f.site.event, registry))
            .map(|m| (kubuno_desktop_views_syntax::validate::distance(&f.handler, &m.name), m.name.clone()))
            .filter(|(d, _)| *d <= limit)
            .min();
        if let Some((_, name)) = closest {
            out.push(CodeAction {
                title: format!("Use `{name}`"),
                kind: Some(CodeActionKind::QUICKFIX),
                edit: workspace_edit(vec![(uri.clone(), vec![TextEdit { range: f.range, new_text: name }])]),
                ..Default::default()
            });
        }
    }
    out
}

/// The code-behind path of a view, for callers outside this module.
pub fn code_behind_path(view: &WebView) -> Option<PathBuf> {
    project::code_behind_of(&view.path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(text: &str) -> TsFile {
        TsFile::parse(Path::new("X.ts"), text)
    }

    fn apply(text: &str, mut edits: Vec<(usize, usize, String)>) -> String {
        edits.sort_by_key(|e| std::cmp::Reverse(e.0));
        let mut out = text.to_string();
        for (s, e, t) in edits {
            out.replace_range(s..e, &t);
        }
        out
    }

    #[test]
    fn type_imports_join_the_existing_import_or_get_their_own_line() {
        let f = file("import type { Button } from '@kubuno/views'\nimport { ViewBase } from './X.kbview'\n");
        assert_eq!(apply(&f.text, import_edits(&f, &["Button", "MouseEventArgs"])), "import type { Button, MouseEventArgs } from '@kubuno/views'\nimport { ViewBase } from './X.kbview'\n");
        let f = file("import {\n  bind,\n  type Button,\n} from \"@kubuno/views\";\n");
        assert_eq!(apply(&f.text, import_edits(&f, &["Switch"])), "import {\n  bind,\n  type Button,\n  type Switch,\n} from \"@kubuno/views\";\n");
        let f = file("import { ViewBase } from \"./X.kbview\";\r\n\r\nexport class X extends ViewBase {}\r\n");
        assert_eq!(
            apply(&f.text, import_edits(&f, &["Button", "Button"])),
            "import { ViewBase } from \"./X.kbview\";\r\nimport type { Button } from \"@kubuno/views\";\r\n\r\nexport class X extends ViewBase {}\r\n"
        );
        let f = file("import { bind } from '@kubuno/views'\n");
        assert!(import_edits(&f, &["bind"]).is_empty(), "already bound");
    }

    #[test]
    fn a_method_goes_before_the_closing_brace_in_the_class_style() {
        let f = file("export class X extends ViewBase {}\n");
        let c = f.classes[0].clone();
        assert_eq!(
            apply(&f.text, vec![method_edit(&f, &c, "ok_click", "Button", "MouseEventArgs", true)]),
            "export class X extends ViewBase {\n  ok_click(_sender: Button, _e: MouseEventArgs): void {\n    // TODO: implement ok_click\n  }\n}\n"
        );
        let f = file("namespace N {\n\texport class X extends ViewBase {\n\t\ta = 1\n\t}\n}\n");
        let c = f.classes.first().cloned();
        // A class inside a namespace is not a top-level code-behind: not read.
        assert!(c.is_none());
        let f = file("class X extends ViewBase {\n\ta = 1\n}\n");
        let c = f.classes[0].clone();
        assert_eq!(
            apply(&f.text, vec![method_edit(&f, &c, "go", "Stack", "EventArgs", false)]),
            "class X extends ViewBase {\n\ta = 1\n\n\tgo(sender: Stack, e: EventArgs): void {\n\t\t// TODO: implement go\n\t}\n}\n"
        );
    }

    #[test]
    fn handler_names_follow_the_designer_rule() {
        let ev = EventEntry { name: "OnCheckedChanged".into(), display_name: "CheckedChanged".into(), ..Default::default() };
        assert_eq!(default_name(Some("darkMode"), "Switch", &ev, false, "Settings"), "dark_mode_checked_changed");
        assert_eq!(default_name(None, "Switch", &ev, false, "Settings"), "switch_checked_changed");
        let load = EventEntry { name: "OnLoad".into(), display_name: "Load".into(), ..Default::default() };
        assert_eq!(default_name(None, "UserControl", &load, true, "AccountMenu"), "account_menu_load");
    }

    #[test]
    fn stubs_are_recognised_and_this_references_found() {
        let text = "a() {\n    // TODO: implement a\n  }";
        assert!(is_untouched_stub(text, (4, text.len())));
        assert!(!is_untouched_stub("a() { go() }", (4, 12)));
        assert_eq!(this_references("this.a(); this.ab(); x.this.a; this.a", "a"), vec![5, 36]);
        assert_eq!(type_names("(Button) | ValueChangedEventArgs<string | number>"), vec!["Button".to_string(), "ValueChangedEventArgs".to_string()]);
    }
}
