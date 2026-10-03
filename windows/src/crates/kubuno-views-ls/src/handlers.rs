//! The designer's handler commands beyond creation (`vskubuno/docs/EVENTS.md` §5.3/§5.5, EVT-5):
//!
//! | Request | What it does |
//! |---|---|
//! | `kubuno/compatibleHandlers { uri, elementId, event }` | the handlers of the code-behind an event can be bound to (the ⚡ row's dropdown) |
//! | `kubuno/renameHandler { uri, old?, new, position?, rustRenamed? }` | renames a handler everywhere: every `On*="old"` of the folder's views, the Rust method (or legacy `fn`), its `handlers!` string, its `#[handler(name)]` |
//! | `kubuno/removeHandler { uri, elementId, event }` | clears an event: removes the attribute, and deletes the handler when it is an untouched stub nothing else references |
//! | `textDocument/prepareRename` / `rename` | F2 on a handler name in the XML: the same rename |
//! | diagnostics | an `On*` naming a handler the code-behind does not have (or one that cannot take the event's args): a warning |
//! | code actions | *Create handler `x`*, *Use `closest`* on that warning; *Use `OnCheckedChanged`* on an older event name |
//!
//! Like the rest of the crate this reads the code-behind as text ([`crate::code_behind`]): the
//! handler set of a view is the union, over the `.rs` files of its folder, of the methods of the
//! `#[event_handlers]` impls and of the `handlers!` table entries. When a file builds its table in
//! a way this cannot read (`HandlerTable::new()`, `insert_typed`, a table not in the
//! `"name" => |a, b| body` shape), the set is unknown and no handler is reported missing.
//!
//! Every request may carry `openFiles` (see [`crate::sources`]) so offsets match the editors'
//! buffers; every edit is a surgical splice, one `TextEdit` list per file.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use kubuno_views::ast::{AstNode, Attribute, Document as AstDocument, Element};
use kubuno_views::registry::EventMeta;
use kubuno_views::syntax::{parse, Parse};
use lsp_types::{
    CodeAction, CodeActionKind, Diagnostic, DiagnosticSeverity, NumberOrString, Position, PrepareRenameResponse, Range, TextEdit, Uri,
    WorkspaceEdit,
};
use serde::{Deserialize, Serialize};

use crate::code_behind::{self, HandlersTable, ImplBlock, Method, Param};
use crate::definition;
use crate::documents::{Document, DocumentStore};
use crate::fs_uri;
use crate::position::PositionIndex;
use crate::sources;

/// The `source` of the handler diagnostics.
const SOURCE: &str = "kubuno-views";
/// Diagnostic codes.
pub const MISSING_HANDLER: &str = "missing-handler";
pub const INCOMPATIBLE_HANDLER: &str = "incompatible-handler";

// ── the code-behind's handlers ──────────────────────────────────────────

/// One `.rs` file of a view's folder, as far as handlers go.
pub(crate) struct CodeFile {
    pub path: PathBuf,
    pub text: String,
    pub typed: Option<ImplBlock>,
    pub methods: Vec<Method>,
    pub table: Option<HandlersTable>,
    /// Builds handlers in a way the scanner cannot read.
    pub opaque: bool,
}

/// The handlers of a folder's code-behind files.
pub(crate) struct CodeBehind {
    pub files: Vec<CodeFile>,
}

impl CodeBehind {
    pub fn load(dir: &Path) -> Self {
        let mut paths = definition::sibling_rs_files(dir);
        paths.sort();
        let files = paths
            .into_iter()
            .filter_map(|path| {
                let text = sources::read(&path)?;
                let typed = code_behind::find_typed_impl(&text);
                let methods = typed.as_ref().map(|imp| code_behind::impl_methods(&text, imp)).unwrap_or_default();
                let table = code_behind::find_handlers_table(&text);
                let has_macro = code_behind::ident_occurrences(&text, "handlers").iter().any(|&at| text[at + "handlers".len()..].starts_with('!'));
                let opaque = (table.is_none() && has_macro)
                    || text.contains("insert_typed(")
                    || text.contains("HandlerTable::new(");
                Some(CodeFile { path, text, typed, methods, table, opaque })
            })
            .collect();
        Self { files }
    }

    /// Whether the handler set is statically known: some typed impl or table was found, and no
    /// file builds handlers in a way the scanner cannot read.
    pub fn is_known(&self) -> bool {
        self.files.iter().any(|f| f.typed.is_some() || f.table.is_some()) && !self.files.iter().any(|f| f.opaque)
    }

    fn typed_handlers(&self) -> impl Iterator<Item = (&CodeFile, &Method)> {
        self.files.iter().flat_map(|f| f.methods.iter().filter(|m| m.is_handler).map(move |m| (f, m)))
    }

    fn table_names(&self) -> impl Iterator<Item = &str> {
        self.files.iter().flat_map(|f| f.table.iter().flat_map(|t| t.entries.iter().map(|e| e.name.as_str())))
    }

    pub fn exists(&self, name: &str) -> bool {
        self.typed_handlers().any(|(_, m)| m.handler_name == name) || self.table_names().any(|n| n == name)
    }

    /// Whether `name` is taken by any `fn` of the folder (a handler or not) — a rename target must be free.
    fn name_taken(&self, name: &str) -> bool {
        self.exists(name) || self.files.iter().any(|f| !code_behind::fns_named(&f.text, name).is_empty())
    }

    /// The handlers `event` of `element` can be bound to: typed methods whose sender and args
    /// fit, and every legacy table entry (they take the legacy value of any event).
    pub fn compatible(&self, element: &str, event: &EventMeta) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        let typed = self.typed_handlers().filter(|(_, m)| accepts(m, element, event)).map(|(_, m)| m.handler_name.as_str());
        for name in typed.chain(self.table_names()) {
            if !out.iter().any(|n| n == name) {
                out.push(name.to_string());
            }
        }
        out
    }

    /// Whether some declaration of `name` accepts the event (a legacy entry always does).
    fn accepts(&self, name: &str, element: &str, event: &EventMeta) -> bool {
        self.table_names().any(|n| n == name) || self.typed_handlers().any(|(_, m)| m.handler_name == name && accepts(m, element, event))
    }
}

/// Whether the typed method `m` can handle `event` raised by an `element` (the rules of
/// `kubuno_views::events::with_args` and `HandlerContext::typed_sender`).
pub(crate) fn accepts(m: &Method, element: &str, event: &EventMeta) -> bool {
    m.params.iter().all(|p| match p {
        Param::AnySender | Param::AnyArgs => true,
        Param::Sender(control) => control == element,
        Param::Args { ty, generic } => args_accept(ty, generic.as_deref(), event),
    })
}

fn args_accept(ty: &str, generic: Option<&str>, event: &EventMeta) -> bool {
    // `ValueChangedEventArgs<String>` is `TextChangedEventArgs`, and so on (`IntoLegacyValue::ARGS_TYPE`).
    let alias = match (ty, generic.map(|g| g.replace(' ', ""))) {
        ("ValueChangedEventArgs", Some(g)) => match g.as_str() {
            "String" => "TextChangedEventArgs",
            "bool" => "CheckedChangedEventArgs",
            "f32" => "NumericValueChangedEventArgs",
            "Option<usize>" => "SelectionChangedEventArgs",
            _ => "",
        },
        _ => ty,
    };
    alias == "EmptyEventArgs"
        // `kubuno::prelude::EventArgs`, the root under its Windows Forms name.
        || alias == "EventArgs"
        || alias == event.args_rust
        || (alias == "CancelEventArgs" && event.cancelable)
        || (alias == "HandledEventArgs" && event.args_mut && !event.cancelable)
}

// ── the folder's views ──────────────────────────────────────────────────

/// A view of the folder (`.kbview` or `.kbcontrol`): the open document's text when the server has it, else the file.
pub(crate) struct View {
    pub uri: Uri,
    pub text: String,
    pub parse: Parse,
}

fn load_views(dir: &Path, documents: &DocumentStore) -> Vec<View> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|entries| entries.flatten().map(|e| e.path()).filter(|p| crate::view_kind::is_view_file(p)).collect())
        .unwrap_or_default();
    // An open view not saved yet counts too.
    let dir_key = sources::key(dir);
    for (uri, _) in documents.iter() {
        if let Some(path) = fs_uri::to_path(uri) {
            let new = path.parent().is_some_and(|p| sources::key(p) == dir_key) && !paths.iter().any(|p| sources::key(p) == sources::key(&path));
            if new {
                paths.push(path);
            }
        }
    }
    paths.sort();
    paths
        .into_iter()
        .filter_map(|path| {
            let key = sources::key(&path);
            let open = documents.iter().find(|(uri, _)| fs_uri::to_path(uri).is_some_and(|p| sources::key(&p) == key));
            match open {
                Some((uri, doc)) => Some(View { uri: uri.clone(), text: doc.text.clone(), parse: doc.parse.clone() }),
                None => {
                    let text = sources::read(&path)?;
                    Some(View { uri: fs_uri::from_path(&path)?, parse: parse(&text), text })
                }
            }
        })
        .collect()
}

/// The event attributes of every element of `parse`: (element, attribute, event, is root).
fn event_attributes(parse: &Parse) -> Vec<(Element, Attribute, &'static EventMeta)> {
    let Some(root) = AstDocument::cast(parse.syntax()).and_then(|d| d.root_element()) else { return Vec::new() };
    let mut out = Vec::new();
    for element in root.syntax().descendants().filter_map(Element::cast) {
        let Some(meta) = element.name().and_then(|n| kubuno_views::registry::lookup(&n)) else { continue };
        let is_root = element.syntax() == root.syntax();
        for attr in element.attributes() {
            let Some(name) = attr.name() else { continue };
            let event = meta.event(&name).or_else(|| if is_root { kubuno_views::registry::view_event(&name) } else { None });
            if let Some(event) = event {
                out.push((element.clone(), attr, event));
            }
        }
    }
    out
}

/// The event `event_name` (canonical or alias, view events on the root) of element `id`.
fn resolve_event(parse: &Parse, element_id: &str, event_name: &str) -> Option<(Element, &'static EventMeta)> {
    let element = AstDocument::cast(parse.syntax())?.resolve_id(element_id)?;
    let meta = kubuno_views::registry::lookup(&element.name()?)?;
    let view_event = || if element_id.is_empty() { kubuno_views::registry::view_event(event_name) } else { None };
    let event = meta.event(event_name).or_else(view_event)?;
    Some((element, event))
}

// ── edits ───────────────────────────────────────────────────────────────

/// Byte-range replacements of one file, deduplicated by start, turned into `TextEdit`s at the end.
#[derive(Default)]
struct FileEdits {
    spans: BTreeMap<usize, (usize, String)>,
}

impl FileEdits {
    fn replace(&mut self, start: usize, end: usize, text: impl Into<String>) {
        self.spans.entry(start).or_insert((end, text.into()));
    }

    fn into_text_edits(self, text: &str) -> Vec<TextEdit> {
        let index = PositionIndex::new(text);
        let pos = |o: usize| index.offset_to_position(text, rowan::TextSize::from(o as u32));
        self.spans.into_iter().map(|(start, (end, new_text))| TextEdit { range: Range { start: pos(start), end: pos(end) }, new_text }).collect()
    }
}

#[allow(clippy::mutable_key_type)] // `Uri`'s memoizing `Cell`, never mutated here: see `handler_insert::create_handler`.
fn workspace_edit(changes: HashMap<Uri, Vec<TextEdit>>) -> Option<WorkspaceEdit> {
    let changes: HashMap<Uri, Vec<TextEdit>> = changes.into_iter().filter(|(_, e)| !e.is_empty()).collect();
    (!changes.is_empty()).then(|| WorkspaceEdit { changes: Some(changes), ..Default::default() })
}

fn to_text_edits(text: &str, edits: Vec<kubuno_views::edit::Edit>) -> Vec<TextEdit> {
    let index = PositionIndex::new(text);
    edits
        .into_iter()
        .map(|e| TextEdit {
            range: Range { start: index.offset_to_position(text, e.range.start()), end: index.offset_to_position(text, e.range.end()) },
            new_text: e.new_text,
        })
        .collect()
}

fn folder_of(uri: &Uri) -> Option<(PathBuf, PathBuf)> {
    let path = fs_uri::to_path(uri)?;
    let dir = path.parent()?.to_path_buf();
    Some((path, dir))
}

// ── kubuno/compatibleHandlers ───────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompatibleHandlersParams {
    pub uri: Uri,
    pub element_id: String,
    pub event: String,
    #[serde(default)]
    pub open_files: HashMap<String, String>,
}

#[derive(Debug, Serialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CompatibleHandlersResult {
    /// The handler names, typed methods first (in declaration order), then legacy table entries.
    pub handlers: Vec<String>,
}

pub fn compatible_handlers(doc: Option<&Document>, p: &CompatibleHandlersParams) -> CompatibleHandlersResult {
    let Some(doc) = doc else { return CompatibleHandlersResult::default() };
    let Some((element, event)) = resolve_event(&doc.parse, &p.element_id, &p.event) else { return CompatibleHandlersResult::default() };
    let Some((_, dir)) = folder_of(&p.uri) else { return CompatibleHandlersResult::default() };
    let element_name = element.name().unwrap_or_default();
    CompatibleHandlersResult { handlers: CodeBehind::load(&dir).compatible(&element_name, event) }
}

// ── kubuno/renameHandler ────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenameHandlerParams {
    /// A `.kbview` of the folder, or the `.rs` file `position` is in.
    pub uri: Uri,
    /// The handler to rename; found from `position` when absent.
    #[serde(default)]
    pub old: Option<String>,
    pub new: String,
    /// In a `.rs` file: the method (or legacy `fn`) name under the cursor.
    #[serde(default)]
    pub position: Option<Position>,
    /// The Rust side was already renamed (by rust-analyzer's rename): only the views and the
    /// strings (a `handlers!` entry) are edited.
    #[serde(default)]
    pub rust_renamed: bool,
    #[serde(default)]
    pub open_files: HashMap<String, String>,
}

#[derive(Debug, Serialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RenameHandlerResult {
    pub edit: Option<WorkspaceEdit>,
    /// The handler renamed (as the views name it).
    pub old_name: Option<String>,
    /// Why nothing was renamed.
    pub reason: Option<String>,
}

fn not_renamed(reason: impl Into<String>) -> RenameHandlerResult {
    RenameHandlerResult { reason: Some(reason.into()), ..Default::default() }
}

pub fn rename_handler(documents: &DocumentStore, p: &RenameHandlerParams) -> RenameHandlerResult {
    let Some((path, dir)) = folder_of(&p.uri) else { return not_renamed("not a file URI") };
    let code = CodeBehind::load(&dir);
    let old = match (&p.old, p.position) {
        (Some(old), _) => old.clone(),
        (None, Some(pos)) => match handler_at(&code, &path, pos) {
            Ok(old) => old,
            Err(reason) => return not_renamed(reason),
        },
        (None, None) => return not_renamed("no handler to rename"),
    };
    let new = p.new.trim();
    if new == old {
        return RenameHandlerResult { old_name: Some(old), ..Default::default() };
    }
    if !code_behind::is_method_ident(new) {
        return not_renamed(format!("`{new}` is not a valid Rust method name"));
    }
    if !p.rust_renamed && code.name_taken(new) {
        return not_renamed(format!("the code-behind already has a `{new}`"));
    }

    #[allow(clippy::mutable_key_type)] // See `workspace_edit`.
    let mut changes: HashMap<Uri, Vec<TextEdit>> = HashMap::new();
    for view in load_views(&dir, documents) {
        let mut edits = Vec::new();
        for (element, attr, _) in event_attributes(&view.parse) {
            if attr.value().as_deref() == Some(old.as_str()) {
                if let Some(name) = attr.name() {
                    edits.extend(to_text_edits(&view.text, kubuno_views::edit::set_attribute(&element, &name, new)));
                }
            }
        }
        changes.insert(view.uri, edits);
    }
    for file in &code.files {
        let mut edits = FileEdits::default();
        rust_rename_edits(file, &old, new, p.rust_renamed, &mut edits);
        if let Some(uri) = fs_uri::from_path(&file.path) {
            changes.insert(uri, edits.into_text_edits(&file.text));
        }
    }
    RenameHandlerResult { edit: workspace_edit(changes), old_name: Some(old), reason: None }
}

/// The handler named by the identifier at `pos` of the `.rs` file `path`.
fn handler_at(code: &CodeBehind, path: &Path, pos: Position) -> Result<String, String> {
    let key = sources::key(path);
    let file = code.files.iter().find(|f| sources::key(&f.path) == key).ok_or("not a code-behind file of a view")?;
    let offset = u32::from(PositionIndex::new(&file.text).position_to_offset(&file.text, pos)) as usize;
    let b = file.text.as_bytes();
    let ident = |c: u8| c.is_ascii_alphanumeric() || c == b'_';
    let mut start = offset.min(b.len());
    while start > 0 && ident(b[start - 1]) {
        start -= 1;
    }
    let mut end = offset.min(b.len());
    while end < b.len() && ident(b[end]) {
        end += 1;
    }
    let word = &file.text[start..end];
    if word.is_empty() {
        return Err("no identifier at the cursor".into());
    }
    if let Some(m) = file.methods.iter().find(|m| m.is_handler && m.name == word) {
        // `#[handler(name = "…")]`: the views name the string, which a method rename leaves alone.
        return if m.name_attr.is_some() { Err(format!("`{word}` is bound under its #[handler(name)]")) } else { Ok(word.to_string()) };
    }
    if code.table_names().any(|n| n == word) {
        return Ok(word.to_string());
    }
    Err(format!("`{word}` is not a handler"))
}

/// The edits renaming `old` to `new` in one code-behind file: the typed method (or its
/// `#[handler(name)]` string) and its `self.old(…)` calls, the `handlers!` entry string and the
/// `old` it forwards to, the legacy `fn old` stub. With `rust_renamed`, only the strings.
fn rust_rename_edits(file: &CodeFile, old: &str, new: &str, rust_renamed: bool, edits: &mut FileEdits) {
    let text = &file.text;
    let mut renamed_fn = false;
    for m in file.methods.iter().filter(|m| m.is_handler && m.handler_name == old) {
        match m.name_attr {
            Some((s, e)) => edits.replace(s, e, new),
            None if !rust_renamed => {
                edits.replace(m.name_at, m.name_at + m.name.len(), new);
                renamed_fn = true;
            }
            None => {}
        }
    }
    if let Some(table) = &file.table {
        for entry in table.entries.iter().filter(|e| e.name == old) {
            edits.replace(entry.name_at, entry.name_at + old.len(), new);
            if !rust_renamed {
                // `"old" => |vm, value| old(vm, value)`: the forwarding call to the legacy stub.
                let body_end = entry.body_at + entry.body.len();
                for at in code_behind::ident_occurrences(&text[..body_end], old).into_iter().filter(|&at| at >= entry.body_at) {
                    edits.replace(at, at + old.len(), new);
                    renamed_fn = true;
                }
            }
        }
    }
    if rust_renamed {
        return;
    }
    for (_, name_at, _, _) in code_behind::fns_named(text, old) {
        edits.replace(name_at, name_at + old.len(), new);
        renamed_fn = true;
    }
    if renamed_fn {
        // Method calls `self.old(…)` / `vm.old(…)`.
        for at in code_behind::ident_occurrences(text, old) {
            if at > 0 && text.as_bytes()[at - 1] == b'.' && text[at + old.len()..].trim_start().starts_with('(') {
                edits.replace(at, at + old.len(), new);
            }
        }
    }
}

// ── kubuno/removeHandler ────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoveHandlerParams {
    pub uri: Uri,
    pub element_id: String,
    pub event: String,
    #[serde(default)]
    pub open_files: HashMap<String, String>,
}

#[derive(Debug, Serialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RemoveHandlerResult {
    pub edit: Option<WorkspaceEdit>,
    pub handler_name: Option<String>,
    /// The handler was an untouched stub nothing else used: it was deleted too.
    pub removed_stub: bool,
}

pub fn remove_handler(documents: &DocumentStore, p: &RemoveHandlerParams) -> RemoveHandlerResult {
    let Some(doc) = documents.get(&p.uri) else { return RemoveHandlerResult::default() };
    let Some((element, event)) = resolve_event(&doc.parse, &p.element_id, &p.event) else { return RemoveHandlerResult::default() };
    // The attribute as written: the canonical name or an older alias.
    let Some(attr_name) = std::iter::once(event.name).chain(event.aliases.iter().copied()).find(|n| element.attribute(n).is_some()) else {
        return RemoveHandlerResult::default();
    };
    let handler = element.attribute(attr_name).and_then(|a| a.value()).filter(|v| !v.is_empty());
    #[allow(clippy::mutable_key_type)] // See `workspace_edit`.
    let mut changes: HashMap<Uri, Vec<TextEdit>> = HashMap::new();
    changes.insert(p.uri.clone(), to_text_edits(&doc.text, kubuno_views::edit::remove_attribute(&element, attr_name)));

    let mut removed_stub = false;
    if let (Some(name), Some((_, dir))) = (handler.as_deref(), folder_of(&p.uri)) {
        let references: usize = load_views(&dir, documents)
            .iter()
            .map(|v| event_attributes(&v.parse).iter().filter(|(_, a, _)| a.value().as_deref() == Some(name)).count())
            .sum();
        // This attribute is the only reference.
        if references <= 1 {
            let code = CodeBehind::load(&dir);
            for file in &code.files {
                let mut edits = FileEdits::default();
                if delete_stub(file, name, &mut edits) {
                    removed_stub = true;
                    if let Some(uri) = fs_uri::from_path(&file.path) {
                        changes.insert(uri, edits.into_text_edits(&file.text));
                    }
                    break;
                }
            }
        }
    }
    RemoveHandlerResult { edit: workspace_edit(changes), handler_name: handler, removed_stub }
}

/// Whether the body between `open` and `close` is what `createHandler` wrote (or empty). The name in the
/// `TODO` comment is not checked: a renamed stub keeps the comment it was created with.
fn is_untouched_stub(text: &str, open: usize, close: usize) -> bool {
    let body = text[open + 1..close].split_whitespace().collect::<Vec<_>>().join(" ");
    let rest = match body.strip_prefix("// TODO: implement ") {
        Some(rest) => rest,
        None => return body.is_empty(),
    };
    let (name, tail) = rest.split_once(' ').unwrap_or((rest, ""));
    code_behind::is_method_ident(name) && (tail.is_empty() || tail == "let _ = (vm, value);")
}

/// Deletes the untouched stub of handler `name` from `file`: a typed method, or a legacy `fn`
/// and its forwarding `handlers!` entry. False (no edit) when it is not a stub.
fn delete_stub(file: &CodeFile, name: &str, edits: &mut FileEdits) -> bool {
    let text = &file.text;
    if let Some(m) = file.methods.iter().find(|m| m.is_handler && m.handler_name == name) {
        if !is_untouched_stub(text, m.body_open, m.body_close) {
            return false;
        }
        let (start, end) = item_lines(text, m.item_start, m.body_close + 1, true);
        edits.replace(start, end, "");
        return true;
    }
    let fns = code_behind::fns_named(text, name);
    let [(fn_at, _, open, close)] = fns[..] else { return false };
    if !is_untouched_stub(text, open, close) {
        return false;
    }
    let entry = file.table.as_ref().and_then(|t| t.entries.iter().find(|e| e.name == name));
    if let Some(entry) = entry {
        // Only the forwarding entry `createHandler` wrote; anything else is the user's code.
        let forward: String = entry.body.split_whitespace().collect();
        if forward != format!("{name}({},{})", entry.vm_pat, entry.value_pat) {
            return false;
        }
        let (s, e) = item_lines(text, entry.start, entry.end, false);
        edits.replace(s, e, "");
    }
    let (start, end) = item_lines(text, fn_at, close + 1, entry.is_none());
    edits.replace(start, end, "");
    true
}

/// The range to delete for the item `start..end`: whole lines when it sits alone on them, plus
/// one blank line next to it (the one before when `prefer_before` and there is one, as a stub
/// appended after a blank line; else the one after, as a stub inserted before a blank line).
fn item_lines(text: &str, start: usize, end: usize, prefer_before: bool) -> (usize, usize) {
    let line_start = code_behind::line_start_of(text, start);
    let line_end = text[end..].find('\n').map_or(text.len(), |n| end + n + 1);
    if !text[line_start..start].trim().is_empty() || !text[end..line_end].trim().is_empty() {
        return (start, end);
    }
    let prev_blank = line_start > 0 && {
        let prev = code_behind::line_start_of(text, line_start - 1);
        text[prev..line_start].trim().is_empty().then_some(prev)
    }
    .is_some();
    let next_end = text[line_end..].find('\n').map(|n| line_end + n + 1);
    let next_blank = next_end.is_some_and(|n| text[line_end..n].trim().is_empty());
    if prev_blank && (prefer_before || !next_blank) {
        (code_behind::line_start_of(text, line_start - 1), line_end)
    } else if next_blank {
        (line_start, next_end.unwrap_or(line_end))
    } else {
        (line_start, line_end)
    }
}

// ── diagnostics and code actions ────────────────────────────────────────

/// A handler finding on one attribute value.
struct Finding {
    range: Range,
    element_id: String,
    element: String,
    event: &'static EventMeta,
    handler: String,
    missing: bool,
}

fn findings(doc: &Document, code: &CodeBehind) -> Vec<Finding> {
    if !code.is_known() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for (element, attr, event) in event_attributes(&doc.parse) {
        let Some(handler) = attr.value().filter(|v| !v.is_empty()) else { continue };
        let Some(range) = attr.value_range() else { continue };
        let element_name = element.name().unwrap_or_default();
        let missing = !code.exists(&handler);
        if missing || !code.accepts(&handler, &element_name, event) {
            let range = Range {
                start: doc.position_index.offset_to_position(&doc.text, range.start()),
                end: doc.position_index.offset_to_position(&doc.text, range.end()),
            };
            out.push(Finding { range, element_id: element.stable_id(), element: element_name, event, handler, missing });
        }
    }
    out
}

/// The handler warnings of the view `doc` at `uri`.
pub fn handler_diagnostics(doc: &Document, uri: &Uri) -> Vec<Diagnostic> {
    let Some((_, dir)) = folder_of(uri) else { return Vec::new() };
    let code = CodeBehind::load(&dir);
    findings(doc, &code)
        .into_iter()
        .map(|f| {
            let (code, message) = if f.missing {
                (MISSING_HANDLER, format!("handler `{}` not found in the code-behind", f.handler))
            } else {
                (INCOMPATIBLE_HANDLER, format!("handler `{}` cannot take the arguments of `{}` (`{}` from a `{}`)", f.handler, f.event.name, f.event.args_rust, f.element))
            };
            Diagnostic {
                range: f.range,
                severity: Some(DiagnosticSeverity::WARNING),
                code: Some(NumberOrString::String(code.to_string())),
                source: Some(SOURCE.to_string()),
                message,
                data: Some(serde_json::json!({ "handler": f.handler, "elementId": f.element_id, "event": f.event.name })),
                ..Default::default()
            }
        })
        .collect()
}

fn overlaps(a: &Range, b: &Range) -> bool {
    let before = |p: &Position, q: &Position| (p.line, p.character) < (q.line, q.character);
    !before(&a.end, &b.start) && !before(&b.end, &a.start)
}

/// A crude edit distance, for "did you mean".
fn distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut prev = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cur = row[j + 1];
            row[j + 1] = (prev + usize::from(ca != *cb)).min(row[j] + 1).min(row[j + 1] + 1);
            prev = cur;
        }
    }
    row[b.len()]
}

/// The quick fixes for the handler warnings and older event names in `range` of the view `doc`.
pub fn quick_fixes(doc: &Document, uri: &Uri, range: &Range) -> Vec<CodeAction> {
    let mut out = Vec::new();
    let Some((kbview_path, dir)) = folder_of(uri) else { return out };
    let code = CodeBehind::load(&dir);
    for f in findings(doc, &code).into_iter().filter(|f| overlaps(&f.range, range)) {
        if f.missing {
            if let Some((path, edits)) = crate::handler_insert::insert_handler_stub(&dir, &kbview_path, &f.handler, &f.element, f.event, f.element_id.is_empty()) {
                if let Some(rs_uri) = fs_uri::from_path(&path) {
                    #[allow(clippy::mutable_key_type)] // See `workspace_edit`.
                    let changes = HashMap::from([(rs_uri, edits)]);
                    out.push(CodeAction {
                        title: format!("Create handler `{}`", f.handler),
                        kind: Some(CodeActionKind::QUICKFIX),
                        is_preferred: Some(true),
                        edit: workspace_edit(changes),
                        ..Default::default()
                    });
                }
            }
        }
        let limit = (f.handler.len() / 3).max(2);
        let closest = code
            .compatible(&f.element, f.event)
            .into_iter()
            .filter(|n| *n != f.handler)
            .map(|n| (distance(&f.handler, &n), n))
            .filter(|(d, _)| *d <= limit)
            .min();
        if let Some((_, name)) = closest {
            #[allow(clippy::mutable_key_type)] // See `workspace_edit`.
            let changes = HashMap::from([(uri.clone(), vec![TextEdit { range: f.range, new_text: name.clone() }])]);
            out.push(CodeAction {
                title: format!("Use `{name}`"),
                kind: Some(CodeActionKind::QUICKFIX),
                edit: workspace_edit(changes),
                ..Default::default()
            });
        }
    }
    // `OnToggled` → `OnCheckedChanged` (the hint of `validate::hints`).
    for (element, attr, event) in event_attributes(&doc.parse) {
        let (Some(name), Some(name_range)) = (attr.name(), attr.name_range()) else { continue };
        if name == event.name || element.attribute(event.name).is_some() {
            continue;
        }
        let r = Range {
            start: doc.position_index.offset_to_position(&doc.text, name_range.start()),
            end: doc.position_index.offset_to_position(&doc.text, name_range.end()),
        };
        if overlaps(&r, range) {
            #[allow(clippy::mutable_key_type)] // See `workspace_edit`.
            let changes = HashMap::from([(uri.clone(), vec![TextEdit { range: r, new_text: event.name.to_string() }])]);
            out.push(CodeAction {
                title: format!("Use `{}`", event.name),
                kind: Some(CodeActionKind::QUICKFIX),
                edit: workspace_edit(changes),
                ..Default::default()
            });
        }
    }
    out
}

// ── F2 in the XML ───────────────────────────────────────────────────────

/// The handler name under `pos`: the value of an event attribute.
fn handler_value_at(doc: &Document, pos: Position) -> Option<(String, Range)> {
    let offset = doc.position_index.position_to_offset(&doc.text, pos);
    event_attributes(&doc.parse).into_iter().find_map(|(_, attr, _)| {
        let r = attr.value_range()?;
        (r.start() <= offset && offset <= r.end()).then(|| {
            let range = Range {
                start: doc.position_index.offset_to_position(&doc.text, r.start()),
                end: doc.position_index.offset_to_position(&doc.text, r.end()),
            };
            (attr.value().unwrap_or_default(), range)
        })
    })
}

pub fn prepare_rename(doc: &Document, pos: Position) -> Option<PrepareRenameResponse> {
    let (value, range) = handler_value_at(doc, pos)?;
    (!value.is_empty()).then_some(PrepareRenameResponse::RangeWithPlaceholder { range, placeholder: value })
}

pub fn rename(documents: &DocumentStore, uri: &Uri, pos: Position, new_name: &str) -> Result<Option<WorkspaceEdit>, String> {
    let Some(doc) = documents.get(uri) else { return Ok(None) };
    let Some((old, _)) = handler_value_at(doc, pos) else { return Ok(None) };
    let params = RenameHandlerParams {
        uri: uri.clone(),
        old: Some(old),
        new: new_name.to_string(),
        position: None,
        rust_renamed: false,
        open_files: HashMap::new(),
    };
    let result = rename_handler(documents, &params);
    match result.reason {
        Some(reason) => Err(reason),
        None => Ok(result.edit),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("kubuno-views-ls-evt5-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn store_with(dir: &Path, name: &str, text: &str) -> (DocumentStore, Uri) {
        let path = dir.join(name);
        std::fs::write(&path, text).unwrap();
        let uri = fs_uri::from_path(&path).unwrap();
        let mut store = DocumentStore::new();
        store.open(uri.clone(), text.to_string(), 1);
        (store, uri)
    }

    /// Applies `edit`'s changes for the file at `path` to `text`.
    fn apply_for(edit: &WorkspaceEdit, path: &Path, text: &str) -> String {
        let uri = fs_uri::from_path(path).unwrap();
        #[allow(clippy::mutable_key_type)]
        let changes = edit.changes.as_ref().unwrap();
        let Some(edits) = changes.get(&uri) else { return text.to_string() };
        let index = PositionIndex::new(text);
        let mut spans: Vec<(usize, usize, &str)> = edits
            .iter()
            .map(|e| {
                (
                    u32::from(index.position_to_offset(text, e.range.start)) as usize,
                    u32::from(index.position_to_offset(text, e.range.end)) as usize,
                    e.new_text.as_str(),
                )
            })
            .collect();
        spans.sort_by_key(|s| std::cmp::Reverse(s.0));
        let mut out = text.to_string();
        for (s, e, t) in spans {
            out.replace_range(s..e, t);
        }
        out
    }

    const VIEW: &str = r#"<Panel OnLoad="on_load">
    <Button x:Name="ok" OnClick="on_ok_click" OnMouseDown="on_ok_down"/>
    <Switch x:Name="dark" OnToggled="on_dark"/>
    <TextField x:Name="name" OnTextChanged="on_ok_click"/>
</Panel>
"#;

    const CODE: &str = "use kubuno_views::prelude::*;\n\npub struct Vm;\n\n#[kubuno_views::event_handlers]\nimpl Vm {\n    fn on_load(&mut self) {\n        self.on_ok_click_helper();\n    }\n\n    fn on_ok_click(&mut self, sender: &Sender<Button>, e: &MouseEventArgs) {\n        // TODO: implement on_ok_click\n    }\n\n    fn on_ok_down(&mut self, e: &MouseEventArgs) {\n        // TODO: implement on_ok_down\n    }\n\n    fn on_dark(&mut self, sender: &Sender<Switch>, e: &CheckedChangedEventArgs) {\n        let _ = e.new;\n    }\n\n    fn any(&mut self, sender: &ElementRef, e: &dyn EventArgs) {}\n\n    fn cancel(&mut self, e: &mut CancelEventArgs) {}\n\n    #[handler(skip)]\n    fn on_ok_click_helper(&mut self) {}\n}\n";

    #[test]
    fn compatible_handlers_follow_sender_and_args() {
        let dir = temp_dir("compat");
        std::fs::write(dir.join("main_view.rs"), CODE).unwrap();
        let (store, uri) = store_with(&dir, "main_view.kbview", VIEW);
        let ask = |id: &str, event: &str| {
            let p = CompatibleHandlersParams { uri: uri.clone(), element_id: id.into(), event: event.into(), open_files: HashMap::new() };
            compatible_handlers(store.get(&uri), &p).handlers
        };
        assert_eq!(ask("0", "OnClick"), ["on_load", "on_ok_click", "on_ok_down", "any"]);
        assert_eq!(ask("1", "OnCheckedChanged"), ["on_load", "on_dark", "any"]);
        assert_eq!(ask("2", "OnValidating"), ["on_load", "any", "cancel"]);
        assert_eq!(ask("2", "OnClick"), ["on_load", "on_ok_down", "any"], "a Sender<Button> handler does not fit a TextField");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_form_class_handlers_are_its_plain_methods() {
        let dir = temp_dir("form-class");
        let code = "use kubuno::prelude::*;\n\n#[kubuno::view(\"main_view.kbview\")]\n#[derive(Default)]\npub struct MainView {\n    clicks: u32,\n}\n\nimpl MainView {\n    pub fn new() -> Self {\n        Self::default()\n    }\n\n    fn main_view_load(&mut self, sender: &Form, e: &EventArgs) {}\n\n    fn hello_click(&mut self, sender: &Button, e: &MouseEventArgs) {}\n\n    fn any(&mut self, sender: &Control) {}\n\n    fn named(&mut self, sender: &TextField, e: &TextChangedEventArgs) {}\n}\n";
        std::fs::write(dir.join("main_view.rs"), code).unwrap();
        let view = "<Panel OnLoad=\"main_view_load\">\n  <Button x:Name=\"hello\" OnClick=\"hello_click\"/>\n  <TextField x:Name=\"status\" OnTextChanged=\"missing_one\"/>\n</Panel>\n";
        let (store, uri) = store_with(&dir, "main_view.kbview", view);
        let ask = |id: &str, event: &str| {
            let p = CompatibleHandlersParams { uri: uri.clone(), element_id: id.into(), event: event.into(), open_files: HashMap::new() };
            compatible_handlers(store.get(&uri), &p).handlers
        };
        assert_eq!(ask("0", "OnClick"), ["main_view_load", "hello_click", "any"], "`new` has no receiver: not a handler");
        assert_eq!(ask("1", "OnTextChanged"), ["main_view_load", "any", "named"], "a `&Button` sender does not fit a TextField");
        let code_behind = CodeBehind::load(&dir);
        assert!(code_behind.is_known());
        let found = findings(store.get(&uri).expect("open"), &code_behind);
        assert_eq!(found.iter().map(|f| (f.handler.as_str(), f.missing)).collect::<Vec<_>>(), [("missing_one", true)]);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn compatible_handlers_use_the_open_buffer() {
        let dir = temp_dir("compat-open");
        let rs = dir.join("main_view.rs");
        std::fs::write(&rs, CODE).unwrap();
        let (store, uri) = store_with(&dir, "main_view.kbview", VIEW);
        let buffer = CODE.replace("fn any(", "fn renamed_any(");
        let open = HashMap::from([(fs_uri::from_path(&rs).unwrap().as_str().to_string(), buffer)]);
        let p = CompatibleHandlersParams { uri: uri.clone(), element_id: "0".into(), event: "OnClick".into(), open_files: open.clone() };
        let handlers = sources::with_overlays(&open, || compatible_handlers(store.get(&uri), &p).handlers);
        assert!(handlers.contains(&"renamed_any".to_string()) && !handlers.contains(&"any".to_string()), "{handlers:?}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn rename_updates_every_view_attribute_and_the_method() {
        let dir = temp_dir("rename");
        let rs = dir.join("main_view.rs");
        std::fs::write(&rs, CODE).unwrap();
        let other = dir.join("other.kbview");
        std::fs::write(&other, "<Button OnClick=\"on_ok_click\"/>").unwrap();
        let (store, uri) = store_with(&dir, "main_view.kbview", VIEW);
        let p = RenameHandlerParams { uri: uri.clone(), old: Some("on_ok_click".into()), new: "save".into(), position: None, rust_renamed: false, open_files: HashMap::new() };
        let result = rename_handler(&store, &p);
        let edit = result.edit.expect("edit");
        let view = apply_for(&edit, &dir.join("main_view.kbview"), VIEW);
        assert_eq!(view, VIEW.replace("\"on_ok_click\"", "\"save\""));
        assert_eq!(apply_for(&edit, &other, "<Button OnClick=\"on_ok_click\"/>"), "<Button OnClick=\"save\"/>");
        let code = apply_for(&edit, &rs, CODE);
        assert!(code.contains("    fn save(&mut self, sender: &Sender<Button>"), "{code}");
        assert!(code.contains("self.on_ok_click_helper();"), "another identifier is left alone");
        assert!(code.contains("// TODO: implement on_ok_click"), "comments are left alone");

        let taken = RenameHandlerParams { new: "any".into(), ..p };
        assert!(rename_handler(&store, &taken).reason.unwrap().contains("already has"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn rename_from_the_rust_editor_after_rust_analyzer() {
        let dir = temp_dir("rename-rs");
        let rs = dir.join("main_view.rs");
        std::fs::write(&rs, CODE).unwrap();
        let (store, _) = store_with(&dir, "main_view.kbview", VIEW);
        let line = CODE.lines().position(|l| l.contains("fn on_dark(")).unwrap() as u32;
        let p = RenameHandlerParams {
            uri: fs_uri::from_path(&rs).unwrap(),
            old: None,
            new: "dark_changed".into(),
            position: Some(Position { line, character: 10 }),
            rust_renamed: true,
            open_files: HashMap::new(),
        };
        let result = rename_handler(&store, &p);
        assert_eq!(result.old_name.as_deref(), Some("on_dark"));
        let edit = result.edit.expect("edit");
        assert!(apply_for(&edit, &dir.join("main_view.kbview"), VIEW).contains("OnToggled=\"dark_changed\""));
        assert_eq!(apply_for(&edit, &rs, CODE), CODE, "rust-analyzer renamed the Rust side");
        std::fs::remove_dir_all(&dir).ok();
    }

    const LEGACY: &str = "fn table() -> HandlerTable {\n    fn go(vm: &mut dyn kubuno_views::binding::ViewModel, value: kubuno_views::binding::Value) {\n        // TODO: implement go\n        let _ = (vm, value);\n    }\n\n    handlers! {\n        \"a\" => |vm, _v| { vm.set(\"S\", Value::Bool(true)); },\n        \"go\" => |vm, value| go(vm, value),\n    }\n}\n";

    #[test]
    fn rename_a_legacy_handler_renames_the_entry_and_its_stub() {
        let dir = temp_dir("rename-legacy");
        let rs = dir.join("v.rs");
        std::fs::write(&rs, LEGACY).unwrap();
        let (store, uri) = store_with(&dir, "v.kbview", "<Button OnClick=\"go\"/>");
        let p = RenameHandlerParams { uri, old: Some("go".into()), new: "run".into(), position: None, rust_renamed: false, open_files: HashMap::new() };
        let edit = rename_handler(&store, &p).edit.unwrap();
        let code = apply_for(&edit, &rs, LEGACY);
        assert!(code.contains("    fn run(vm") && code.contains("\"run\" => |vm, value| run(vm, value),"), "{code}");
        assert!(code.contains("// TODO: implement go"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn removing_an_untouched_stub_deletes_it_and_its_blank_line() {
        let dir = temp_dir("remove");
        let rs = dir.join("main_view.rs");
        std::fs::write(&rs, CODE).unwrap();
        let (store, uri) = store_with(&dir, "main_view.kbview", VIEW);
        let p = RemoveHandlerParams { uri: uri.clone(), element_id: "0".into(), event: "OnMouseDown".into(), open_files: HashMap::new() };
        let result = remove_handler(&store, &p);
        assert!(result.removed_stub);
        let edit = result.edit.unwrap();
        assert_eq!(apply_for(&edit, &dir.join("main_view.kbview"), VIEW), VIEW.replace(" OnMouseDown=\"on_ok_down\"", ""));
        let code = apply_for(&edit, &rs, CODE);
        assert_eq!(code, CODE.replace("\n    fn on_ok_down(&mut self, e: &MouseEventArgs) {\n        // TODO: implement on_ok_down\n    }\n", ""));

        // A handler still used elsewhere (OnClick of `ok` and OnTextChanged of `name`) stays.
        let shared = RemoveHandlerParams { element_id: "0".into(), event: "OnClick".into(), ..p };
        let result = remove_handler(&store, &shared);
        assert!(!result.removed_stub);
        // A handler with code in it stays (and an older alias is removed as written).
        let edited = RemoveHandlerParams { uri: uri.clone(), element_id: "1".into(), event: "OnCheckedChanged".into(), open_files: HashMap::new() };
        let result = remove_handler(&store, &edited);
        assert!(!result.removed_stub);
        assert_eq!(apply_for(&result.edit.unwrap(), &dir.join("main_view.kbview"), VIEW), VIEW.replace(" OnToggled=\"on_dark\"", ""));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_renamed_stub_is_still_a_stub() {
        let dir = temp_dir("remove-renamed");
        let rs = dir.join("v.rs");
        let code = "#[event_handlers]\nimpl Vm {\n    fn keep(&mut self) {}\n\n    fn renamed(&mut self, e: &MouseEventArgs) {\n        // TODO: implement original_name\n    }\n}\n";
        std::fs::write(&rs, code).unwrap();
        let (store, uri) = store_with(&dir, "v.kbview", "<Button OnClick=\"renamed\"/>");
        let p = RemoveHandlerParams { uri, element_id: String::new(), event: "OnClick".into(), open_files: HashMap::new() };
        let result = remove_handler(&store, &p);
        assert!(result.removed_stub);
        assert_eq!(apply_for(&result.edit.unwrap(), &rs, code), "#[event_handlers]\nimpl Vm {\n    fn keep(&mut self) {}\n}\n");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn removing_a_legacy_stub_deletes_the_fn_and_its_entry() {
        let dir = temp_dir("remove-legacy");
        let rs = dir.join("v.rs");
        std::fs::write(&rs, LEGACY).unwrap();
        let (store, uri) = store_with(&dir, "v.kbview", "<Button OnClick=\"go\"/>");
        let p = RemoveHandlerParams { uri, element_id: String::new(), event: "OnClick".into(), open_files: HashMap::new() };
        let result = remove_handler(&store, &p);
        assert!(result.removed_stub);
        let code = apply_for(&result.edit.unwrap(), &rs, LEGACY);
        assert_eq!(code, "fn table() -> HandlerTable {\n    handlers! {\n        \"a\" => |vm, _v| { vm.set(\"S\", Value::Bool(true)); },\n    }\n}\n");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn missing_and_incompatible_handlers_are_warnings_with_quick_fixes() {
        let dir = temp_dir("diag");
        std::fs::write(dir.join("main_view.rs"), CODE).unwrap();
        let view = "<Panel>\n    <Button OnClick=\"on_ok_clik\" OnMouseUp=\"on_dark\"/>\n    <Switch OnToggled=\"any\"/>\n</Panel>\n";
        let (store, uri) = store_with(&dir, "main_view.kbview", view);
        let doc = store.get(&uri).unwrap();
        let diags = handler_diagnostics(doc, &uri);
        assert_eq!(diags.len(), 2, "{diags:?}");
        assert_eq!(diags[0].code, Some(NumberOrString::String(MISSING_HANDLER.into())));
        assert!(diags[0].message.contains("`on_ok_clik` not found"));
        assert_eq!(diags[1].code, Some(NumberOrString::String(INCOMPATIBLE_HANDLER.into())));

        let fixes = quick_fixes(doc, &uri, &diags[0].range);
        let titles: Vec<_> = fixes.iter().map(|f| f.title.as_str()).collect();
        assert_eq!(titles, ["Create handler `on_ok_clik`", "Use `on_ok_click`"]);
        let created = apply_for(fixes[0].edit.as_ref().unwrap(), &dir.join("main_view.rs"), CODE);
        assert!(created.contains("    fn on_ok_clik(&mut self, sender: &Sender<Button>, e: &MouseEventArgs) {\n        // TODO: implement on_ok_clik\n    }\n}\n"), "{created}");

        // The older event name gets its rename fix.
        let alias_range = Range { start: Position { line: 2, character: 13 }, end: Position { line: 2, character: 13 } };
        let fixes = quick_fixes(doc, &uri, &alias_range);
        assert_eq!(fixes.iter().map(|f| f.title.as_str()).collect::<Vec<_>>(), ["Use `OnCheckedChanged`"]);

        // An unknown handler set (a table built at run time) reports nothing.
        std::fs::write(dir.join("main.rs"), "fn t() { let mut h = HandlerTable::new(); }").unwrap();
        assert!(handler_diagnostics(doc, &uri).is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn f2_on_a_handler_name_renames_it() {
        let dir = temp_dir("f2");
        std::fs::write(dir.join("main_view.rs"), CODE).unwrap();
        let (store, uri) = store_with(&dir, "main_view.kbview", VIEW);
        let doc = store.get(&uri).unwrap();
        let pos = Position { line: 1, character: 35 };
        assert!(matches!(prepare_rename(doc, pos), Some(PrepareRenameResponse::RangeWithPlaceholder { placeholder, .. }) if placeholder == "on_ok_click"));
        assert!(prepare_rename(doc, Position { line: 1, character: 5 }).is_none());
        let edit = rename(&store, &uri, pos, "ok_clicked").unwrap().unwrap();
        assert!(apply_for(&edit, &dir.join("main_view.rs"), CODE).contains("fn ok_clicked(&mut self"));
        assert!(rename(&store, &uri, pos, "not valid").is_err());
        std::fs::remove_dir_all(&dir).ok();
    }
}
