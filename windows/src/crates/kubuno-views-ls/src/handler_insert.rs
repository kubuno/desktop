//! `kubuno/createHandler` (`vskubuno/docs/DESIGNER.md`, work package DSG-10):
//! double-click on a control/event on the design surface -> create the Rust
//! handler.
//!
//! Extends [`crate::definition`]'s existing *read* half ("`Click="handler"`
//! -> `fn handler` in a sibling `.rs` file, text search") with the *write*
//! half `DESIGNER.md` §1 asks for: (1) set the `On*="…"` attribute on the
//! `.kbview` element via [`kubuno_views::edit::set_attribute`] (the same
//! surgical splice API `crate::edit_bridge` already bridges, DSG-2), and (2)
//! insert a matching Rust handler stub into the code-behind `.rs` file this
//! module locates the same way [`crate::definition::sibling_rs_files`] does.
//! Both edits travel back to the client as one [`lsp_types::WorkspaceEdit`]
//! (`changes: { uri -> [TextEdit] }`) — the client applies each file's edits
//! through its own buffer, one undo unit per file (`vskubuno`'s
//! `Editing/` services + the new `Handlers/HandlerCreationService.cs`, DSG-10's
//! C# half).
//!
//! ## The stub shape, and why it is a real `fn` *and* a table entry
//!
//! [`kubuno_views::handlers!`] only ever accepts an inline closure literal
//! (`"name" => |vm, value| { … }`) — there is no way to register a bare `fn`
//! item in the table directly. But [`crate::definition::goto_definition`]
//! (the *read* half this package extends) only ever finds a literal `fn
//! <name>` — it cannot see into a `handlers!` table's closure bodies (its own
//! doc explains why: a real resolution needs the interpreter/binding layer
//! this crate deliberately does not link). Generating *only* a closure entry
//! would create a handler "go to definition" could never find; generating
//! *only* a free `fn` would leave it undispatched. So this module always
//! creates a standalone `fn <name>(vm: &mut dyn ViewModel, value: Value)`
//! (the one payload shape [`kubuno_views::registry::EventMeta`] models today
//! — see that type's own doc, "No payload type is modelled yet"), and, only
//! when the target file already uses the `handlers!` table convention, also
//! inserts one forwarding entry `"<name>" => |vm, value| <name>(vm, value),`
//! registering it — "plus registering it in the `handlers!` table if the
//! convention requires it" is conditional on that convention actually being
//! present in the file, exactly this way.
//!
//! ## Typed code-behinds (`vskubuno/docs/EVENTS.md` §5.4, EVT-4)
//!
//! When the code-behind has a `#[kubuno_views::event_handlers]` impl, the stub is a typed
//! method appended to it instead — `fn on_ok_click(&mut self, sender: &Sender<Button>, e:
//! &MouseEventArgs)`: the sender typed with the element's control type
//! (`kubuno_views::controls`, `&ElementRef` for a structural element), the args with the
//! event's Rust type from the registry (`EventMeta::args_rust`, `&dyn EventArgs` for the root
//! args, `&mut` when the handler can set `handled`/`cancel`) — plus
//! `use kubuno_views::prelude::*;` when the file lacks it. No table entry: the macro
//! dispatches by method name. So each file keeps a single style: typed when it already is,
//! legacy otherwise ([`crate::convert_handlers`] migrates one).
//!
//! ## Never regenerates or reformats
//!
//! Every edit this module produces is a **pure, zero-length insertion** at a
//! computed offset — never a replace, never a whole-block rewrite. The two
//! insertion points (the new `fn`, and, when applicable, its `handlers!`
//! table entry) are computed with a small brace-depth scanner
//! ([`find_handlers_block`]) rather than a Rust parser, deliberately mirroring
//! [`crate::definition`]'s own "plain text search, not name resolution"
//! simplification (see that module's doc for why a real resolution needs
//! work another agent owns).
//!
//! ## A known limitation shared with `crate::definition`
//!
//! Like [`crate::definition::goto_definition`], this module reads the
//! code-behind `.rs` file from **disk**, not from a live VS buffer — this
//! server has no LSP document for it (only `.kbview` documents are tracked by
//! [`crate::documents::DocumentStore`]). If that file is open in Visual
//! Studio with unsaved changes, the computed byte offsets are only as fresh
//! as the last save. The C# side is expected to reconcile this the same way
//! any `kubuno/applyEdit` consumer already must (`docs/DESIGNER.md` §2's
//! "reject if the buffer changed since the request snapshot").

use std::path::{Path, PathBuf};
use std::collections::HashMap;

use kubuno_views::ast::{AstNode, Document as AstDocument};
use kubuno_views::edit;
use lsp_types::{Location, Range, TextEdit, Uri, WorkspaceEdit};
use serde::{Deserialize, Serialize};

use crate::code_behind;
use crate::definition;
use crate::documents::Document;
use crate::fs_uri;
use crate::position::PositionIndex;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateHandlerParams {
    pub uri: Uri,
    pub element_id: String,
    /// The declared event name exactly as the registry carries it (e.g.
    /// `"OnClick"` — [`kubuno_views::registry::EventMeta::name`] already
    /// includes the `On` prefix; this is also the `.kbview` attribute name).
    pub event: String,
    #[serde(default)]
    pub suggested_name: Option<String>,
    /// The client's open documents (`uri -> text`), read instead of the files (see `crate::sources`).
    #[serde(default)]
    pub open_files: HashMap<String, String>,
}

/// Exactly one of `location`/`edit` is `Some` on a successful call: `location`
/// when the event already names a handler (a "go to definition" — no edit is
/// produced), `edit` when a new one was created. Both `None` only in the same
/// "degrade to no-op" cases every other bridge method in this crate uses (no
/// open document, an id/name that does not resolve — see the module doc of
/// [`crate::edit_bridge`] for the precedent this follows).
#[derive(Debug, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CreateHandlerResult {
    pub handler_name: String,
    pub location: Option<Location>,
    pub edit: Option<WorkspaceEdit>,
}

/// `kubuno/createHandler`: see the module doc for the two-part edit this
/// produces and the "already exists" short-circuit.
pub fn create_handler(doc: Option<&Document>, uri: &Uri, params: &CreateHandlerParams) -> CreateHandlerResult {
    let Some(doc) = doc else { return CreateHandlerResult::default() };
    let Some(ast_doc) = AstDocument::cast(doc.parse.syntax()) else { return CreateHandlerResult::default() };
    let Some(element) = ast_doc.resolve_id(&params.element_id) else { return CreateHandlerResult::default() };
    let Some(element_name) = element.name() else { return CreateHandlerResult::default() };
    let Some(meta) = kubuno_views::registry::lookup(&element_name) else { return CreateHandlerResult::default() };
    // The canonical event (an older alias such as `OnToggled` resolves to `OnCheckedChanged`);
    // the view's own events (`OnLoad`…) exist on the root element only.
    let view_event = || if params.element_id.is_empty() { kubuno_views::registry::view_event(&params.event) } else { None };
    let Some(event) = meta.event(&params.event).or_else(view_event) else {
        return CreateHandlerResult::default(); // Not a declared event of this component.
    };

    let Some(kbview_path) = fs_uri::to_path(uri) else { return CreateHandlerResult::default() };
    let Some(dir) = kbview_path.parent() else { return CreateHandlerResult::default() };

    // Already wired under its canonical name or an older alias: navigate, never add a second attribute.
    let existing = std::iter::once(event.name)
        .chain(event.aliases.iter().copied())
        .find_map(|name| element.attribute(name).and_then(|a| a.value()).filter(|v| !v.is_empty()));
    if let Some(existing) = existing {
        let location = definition::sibling_rs_files(dir)
            .iter()
            .find_map(|path| definition::find_fn_in_file(path, &existing).into_iter().next());
        return CreateHandlerResult { handler_name: existing, location, edit: None };
    }

    let x_name = element.attribute("x:Name").and_then(|a| a.value());
    let base_name = params
        .suggested_name
        .clone()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| match view_code_behind(dir, &kbview_path) {
            // A `#[kubuno::view]` form class: Windows Forms' `button1_Click` → `hello_click`, and
            // `Form1_Load` → `main_view_load` for the view's own events.
            true => view_handler_name(x_name.as_deref(), &element_name, &params.event, params.element_id.is_empty(), &kbview_path),
            false => default_handler_name(x_name.as_deref(), &element_name, &params.event),
        });
    // A name picked from the ⚡ row's dropdown (EVT-5): an existing handler, the event is bound to it
    // (WinForms), no second stub.
    if let Some(existing) = params.suggested_name.as_deref().filter(|s| !s.is_empty()) {
        if crate::handlers::CodeBehind::load(dir).exists(existing) {
            let edits: Vec<TextEdit> = edit::set_attribute(&element, &params.event, existing).into_iter().map(|e| to_text_edit(doc, e)).collect();
            #[allow(clippy::mutable_key_type)] // See below.
            let changes: HashMap<Uri, Vec<TextEdit>> = HashMap::from([(uri.clone(), edits)]);
            return CreateHandlerResult {
                handler_name: existing.to_string(),
                location: None,
                edit: Some(WorkspaceEdit { changes: Some(changes), ..Default::default() }),
            };
        }
    }
    let handler_name = unique_name(dir, &base_name);

    // Written under the name asked for (the ⚡ tab asks for the canonical one).
    let attribute_edits = edit::set_attribute(&element, &params.event, &handler_name);
    if attribute_edits.is_empty() {
        return CreateHandlerResult::default();
    }
    let kbview_edits: Vec<TextEdit> = attribute_edits.into_iter().map(|e| to_text_edit(doc, e)).collect();

    let Some((code_behind_path, code_behind_edits)) = insert_handler_stub(dir, &kbview_path, &handler_name, &element_name, event, params.element_id.is_empty()) else {
        return CreateHandlerResult::default();
    };
    let Some(code_behind_uri) = fs_uri::from_path(&code_behind_path) else {
        return CreateHandlerResult::default();
    };

    // `lsp_types::Uri` wraps a `fluent-uri` type with an internal `Cell`
    // memoizing a lazily-parsed component, which trips clippy's interior-
    // mutability-as-map-key lint — same as `crate::documents::DocumentStore`'s
    // own `HashMap<Uri, Document>`. Never mutated after insertion here (the
    // map is built once and handed straight to `WorkspaceEdit`), so this is
    // exactly the lint's documented false-positive case.
    #[allow(clippy::mutable_key_type)]
    let mut changes: HashMap<Uri, Vec<TextEdit>> = HashMap::new();
    changes.insert(uri.clone(), kbview_edits);
    changes.insert(code_behind_uri, code_behind_edits);

    CreateHandlerResult {
        handler_name,
        location: None,
        edit: Some(WorkspaceEdit { changes: Some(changes), ..Default::default() }),
    }
}

fn to_text_edit(doc: &Document, e: edit::Edit) -> TextEdit {
    let start = doc.position_index.offset_to_position(&doc.text, e.range.start());
    let end = doc.position_index.offset_to_position(&doc.text, e.range.end());
    TextEdit { range: Range { start, end }, new_text: e.new_text }
}

// ── name picking ───────────────────────────────────────────────────────

/// `on_<xname or element>_<event>`, all `snake_case` — `DESIGNER.md` §6,
/// DSG-10's own naming rule.
fn default_handler_name(x_name: Option<&str>, element_name: &str, event: &str) -> String {
    let subject = x_name
        .filter(|s| !s.is_empty())
        .map(to_snake_case)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| to_snake_case(element_name));
    format!("on_{subject}_{}", event_base_name(event))
}

/// Whether the view's code-behind is a `#[kubuno::view]` form class or a user control's (`#[derive(UserControl)]`):
/// its handlers are then named the Windows Forms way (`new_message_click`, `address_editor_load`).
fn view_code_behind(dir: &Path, kbview_path: &Path) -> bool {
    pick_code_behind_file(dir, kbview_path).and_then(|p| crate::sources::read(&p)).is_some_and(|t| {
        code_behind::find_view_struct(&t).is_some() || kubuno_views_meta::scan_source(&t).components.iter().any(|c| c.kind == kubuno_views_meta::DeclKind::UserControl)
    })
}

/// The handler name of a `#[kubuno::view]` form class, the Windows Forms way: `<x:Name>_<event>`
/// (`hello_click`), the element's name for an unnamed one, and the view's file name for the view's
/// own events (`main_view_load`, like `Form1_Load`).
fn view_handler_name(x_name: Option<&str>, element_name: &str, event: &str, root: bool, kbview_path: &Path) -> String {
    let stem = kbview_path.file_stem().and_then(|s| s.to_str()).map(to_snake_case).filter(|s| !s.is_empty());
    let subject = x_name
        .filter(|s| !s.is_empty())
        .map(to_snake_case)
        .filter(|s| !s.is_empty())
        .or(if root { stem } else { None })
        .unwrap_or_else(|| to_snake_case(element_name));
    format!("{subject}_{}", event_base_name(event))
}

/// Strips a registry event name's leading `On` (`"OnClick"` -> `"Click"`,
/// leaving a name that does not follow that convention untouched) before
/// `snake_case`-ing it, so the generated handler name reads `on_button_click`
/// rather than the doubled `on_button_on_click`.
fn event_base_name(event: &str) -> String {
    let base = event
        .strip_prefix("On")
        .filter(|rest| rest.chars().next().is_some_and(char::is_uppercase))
        .unwrap_or(event);
    to_snake_case(base)
}

pub(crate) fn to_snake_case(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 4);
    let mut prev_was_lower_or_digit = false;
    for c in s.chars() {
        if c.is_ascii_uppercase() {
            if prev_was_lower_or_digit {
                out.push('_');
            }
            out.push(c.to_ascii_lowercase());
            prev_was_lower_or_digit = false;
        } else if c.is_ascii_alphanumeric() {
            out.push(c);
            prev_was_lower_or_digit = true;
        } else if !out.is_empty() && !out.ends_with('_') {
            out.push('_');
            prev_was_lower_or_digit = false;
        }
    }
    out.trim_matches('_').to_string()
}

/// `base`, or `base_2`/`base_3`/… — the first name for which no sibling `.rs`
/// file (the same set [`definition::sibling_rs_files`] scans) already
/// declares a matching `fn`.
fn unique_name(dir: &Path, base: &str) -> String {
    if !name_exists(dir, base) {
        return base.to_string();
    }
    for suffix in 2.. {
        let candidate = format!("{base}_{suffix}");
        if !name_exists(dir, &candidate) {
            return candidate;
        }
    }
    unreachable!("an unbounded suffix search always finds a free name");
}

fn name_exists(dir: &Path, name: &str) -> bool {
    definition::sibling_rs_files(dir).iter().any(|path| !definition::find_fn_in_file(path, name).is_empty())
}

// ── code-behind file + insertion point ────────────────────────────────

/// Picks the code-behind `.rs` file to insert into, from the same sibling set
/// [`definition::sibling_rs_files`] already scans: the file matching the
/// `.kbview` file's own stem (`settings_view.kbview` -> `settings_view.rs`,
/// `XML_VIEWS.md` §7's convention) when present; otherwise the one sibling
/// `.rs` file that already contains a `handlers!` table, when exactly one
/// does; otherwise the only sibling `.rs` file, when there is exactly one.
/// `None` when none of these narrows to a single, unambiguous file — this
/// package does not guess between several equally-plausible targets.
pub(crate) fn pick_code_behind_file(dir: &Path, kbview_path: &Path) -> Option<PathBuf> {
    let files = definition::sibling_rs_files(dir);
    if files.is_empty() {
        return None;
    }

    if let Some(stem) = kbview_path.file_stem().and_then(|s| s.to_str()) {
        if let Some(matched) = files.iter().find(|p| p.file_stem().and_then(|s| s.to_str()) == Some(stem)) {
            return Some(matched.clone());
        }
    }

    let with_handlers: Vec<&PathBuf> =
        files.iter().filter(|p| crate::sources::read(p).is_some_and(|t| t.contains("handlers!") || t.contains("event_handlers"))).collect();
    if with_handlers.len() == 1 {
        return Some(with_handlers[0].clone());
    }

    if files.len() == 1 {
        return Some(files[0].clone());
    }

    None
}

/// The full `{path, edits}` this module inserts into the code-behind file for
/// `name` — see the module doc for the "real `fn` + conditional table entry"
/// shape. `None` when [`pick_code_behind_file`] cannot settle on one file, or
/// it cannot be read.
pub(crate) fn insert_handler_stub(
    dir: &Path,
    kbview_path: &Path,
    name: &str,
    element: &str,
    event: &kubuno_views::registry::EventMeta,
    root: bool,
) -> Option<(PathBuf, Vec<TextEdit>)> {
    let path = pick_code_behind_file(dir, kbview_path)?;
    let text = crate::sources::read(&path)?;
    let index = PositionIndex::new(&text);

    // Keep the file's own line endings (a CRLF file stays CRLF).
    let nl = if text.contains("\r\n") { "\r\n" } else { "\n" };

    // A typed code-behind (EVT-4): the stub is a method of its `#[event_handlers]` impl — or, for a
    // `#[kubuno::view]` form class, a plain method of the struct's `impl`.
    if let Some(imp) = code_behind::find_typed_impl(&text) {
        return Some((path, typed_stub_edits(&index, &text, &imp, name, element, event, root, nl)));
    }
    // A form class without an `impl` yet: one is added after the struct.
    if let Some((struct_name, struct_end)) = code_behind::find_view_struct(&text) {
        let signature = typed_signature_for(name, element, event, true, root);
        let block = format!("{nl}{nl}impl {struct_name} {{{nl}    {signature} {{{nl}        // TODO: implement {name}{nl}    }}{nl}}}");
        return Some((path, vec![insertion_at(&index, &text, struct_end, block)]));
    }

    let mut edits = Vec::with_capacity(3);
    match find_handlers_block(&text) {
        Some((handlers_at, open, close)) => {
            let line_start = line_start_of(&text, handlers_at);
            let indent = leading_whitespace(&text[line_start..]);
            let fn_text = format!(
                "{indent}fn {name}(vm: &mut dyn kubuno_views::binding::ViewModel, value: kubuno_views::binding::Value) {{{nl}\
                 {indent}    // TODO: implement {name}{nl}\
                 {indent}    let _ = (vm, value);{nl}\
                 {indent}}}{nl}{nl}"
            );
            edits.push(insertion_at(&index, &text, line_start, fn_text));
            edits.extend(table_entry_edits(&index, &text, open, close, &indent, name, nl));
        }
        None => {
            // No `handlers!` table in this file yet — a top-level stub only,
            // no registration entry ("if the convention requires it").
            let mut fn_text = String::new();
            if !text.is_empty() && !text.ends_with('\n') {
                fn_text.push_str(nl);
            }
            if !text.is_empty() && !text.ends_with("\n\n") && !text.ends_with("\r\n\r\n") {
                fn_text.push_str(nl);
            }
            fn_text.push_str(&format!(
                "fn {name}(vm: &mut dyn kubuno_views::binding::ViewModel, value: kubuno_views::binding::Value) {{{nl}"
            ));
            fn_text.push_str(&format!("    // TODO: implement {name}{nl}"));
            fn_text.push_str(&format!("    let _ = (vm, value);{nl}"));
            fn_text.push_str(&format!("}}{nl}"));
            edits.push(insertion_at(&index, &text, text.len(), fn_text));
        }
    }

    Some((path, edits))
}

/// The typed signature of a new handler (`vskubuno/docs/EVENTS.md` §5.4): the sender typed
/// with the element's control type (`&ElementRef` for a structural element, which has none),
/// and the event's Rust args type — `&dyn EventArgs` for the root args (WinForms'
/// `EventArgs e`), `&mut` when a handler writes back into them (`handled`, `cancel`) —
/// or, for a `#[kubuno::view]` form class (`view`): the sender typed with the
/// control's handle (`&Button`, `&Control` for an element without one, `&Form` for the view's own
/// events on its root) and the root args under their Windows Forms name (`&EventArgs`) —
/// `fn hello_click(&mut self, _sender: &Button, _e: &MouseEventArgs)`.
/// The path a form's code-behind names an args type of a control of the project with: `crate::<module>::<Args>`
/// when the args struct is declared in the file of the control (`AddressValidatedEventArgs` next to
/// `AddressEditor`), so the generated handler compiles without an import; the bare name otherwise.
fn project_args_path(element: &str, args: &str) -> String {
    let Some(info) = kubuno_views::registry::project::project_info(element) else { return args.to_string() };
    let Some(file) = info.source_file.map(Path::new).filter(|f| f.is_absolute()) else { return args.to_string() };
    let declares = crate::sources::read(file).is_some_and(|t| t.contains(&format!("struct {args}")));
    match (declares, file.file_stem().and_then(|s| s.to_str())) {
        (true, Some(stem)) if stem != "main" && stem != "lib" => format!("crate::{}::{args}", to_snake_case(stem)),
        (true, Some(_)) => format!("crate::{args}"),
        _ => args.to_string(),
    }
}

pub(crate) fn typed_signature_for(name: &str, element: &str, event: &kubuno_views::registry::EventMeta, view: bool, root: bool) -> String {
    if view {
        let sender = if root {
            "Form"
        } else if kubuno_views::controls::ALL.contains(&element) && element != "UserControl" {
            element
        } else {
            "Control"
        };
        let args = if event.args_rust == "EmptyEventArgs" { "EventArgs".to_string() } else { project_args_path(element, event.args_rust) };
        let reference = if event.args_mut { "&mut " } else { "&" };
        // `_`-prefixed: a stub does not use them yet, and a plain method (unlike an
        // `#[event_handlers]` one) gets no help from a macro to silence the unused-variable lint.
        return format!("fn {name}(&mut self, _sender: &{sender}, _e: {reference}{args})");
    }
    let sender = if kubuno_views::controls::ALL.contains(&element) {
        format!("sender: &Sender<{element}>")
    } else {
        "sender: &ElementRef".to_string()
    };
    let args = if event.args_rust == "EmptyEventArgs" { "dyn EventArgs" } else { event.args_rust };
    let reference = if event.args_mut { "&mut " } else { "&" };
    format!("fn {name}(&mut self, {sender}, e: {reference}{args})")
}

/// The edits adding the typed stub `name` at the end of the `#[event_handlers]` impl `imp`,
/// plus `use kubuno_views::prelude::*;` when the file lacks it (the stub names `Sender`, the
/// control type and the args type).
#[allow(clippy::too_many_arguments)]
fn typed_stub_edits(
    index: &PositionIndex,
    text: &str,
    imp: &code_behind::ImplBlock,
    name: &str,
    element: &str,
    event: &kubuno_views::registry::EventMeta,
    root: bool,
    nl: &str,
) -> Vec<TextEdit> {
    let mut edits = Vec::with_capacity(2);
    if !code_behind::has_prelude(text) {
        let (at, import) = code_behind::prelude_insertion(text, nl);
        edits.push(insertion_at(index, text, at, import));
    }
    let impl_indent = leading_whitespace(&text[line_start_of(text, imp.open)..]);
    let indent = format!("{impl_indent}    ");
    let signature = typed_signature_for(name, element, event, imp.view, root);
    let method = format!("{indent}{signature} {{{nl}{indent}    // TODO: implement {name}{nl}{indent}}}{nl}");
    let empty = text[imp.open + 1..imp.close].trim().is_empty();
    let close_line_start = line_start_of(text, imp.close);
    if close_line_start > imp.open && text[close_line_start..imp.close].trim().is_empty() {
        let separator = if empty { "" } else { nl };
        edits.push(insertion_at(index, text, close_line_start, format!("{separator}{method}")));
    } else {
        // `impl X {}` on one line: the method on its own lines, the brace after it.
        edits.push(insertion_at(index, text, imp.close, format!("{nl}{method}{impl_indent}")));
    }
    edits
}

fn insertion_at(index: &PositionIndex, text: &str, byte_offset: usize, new_text: String) -> TextEdit {
    let pos = index.offset_to_position(text, rowan::TextSize::from(byte_offset as u32));
    TextEdit { range: Range { start: pos, end: pos }, new_text }
}

fn leading_whitespace(line_onward: &str) -> String {
    line_onward.chars().take_while(|c| *c == ' ' || *c == '\t').collect()
}

/// Byte offset of the start of the line containing `offset`.
fn line_start_of(text: &str, offset: usize) -> usize {
    text[..offset].rfind('\n').map(|nl| nl + 1).unwrap_or(0)
}

/// The edits registering `name` in the `handlers!` table whose braces are at
/// `open`/`close`: one new entry line, indented like the table's existing
/// entries (or one level deeper than the `handlers!` line when the table is
/// empty), inserted as a whole line *before* the closing brace's own line so
/// that brace keeps its indentation - plus, when the last existing entry has
/// no trailing comma, the `,` the macro's `$(…),*` grammar needs before a new
/// entry. When the closing brace shares its line with other text (a
/// one-line table), the entry goes on its own line and the brace moves to the
/// next line at `indent`.
fn table_entry_edits(
    index: &PositionIndex,
    text: &str,
    open: usize,
    close: usize,
    indent: &str,
    name: &str,
    nl: &str,
) -> Vec<TextEdit> {
    let body = &text[open + 1..close];
    let last = body.trim_end();
    let last_end = open + 1 + last.len();
    let has_entries = !last.trim().is_empty();
    let needs_comma = has_entries && !last.ends_with(',');

    let last_line_start = line_start_of(text, last_end);
    let entry_indent = if has_entries && last_line_start > open {
        leading_whitespace(&text[last_line_start..])
    } else {
        format!("{indent}    ")
    };
    let entry = format!("{entry_indent}\"{name}\" => |vm, value| {name}(vm, value),");

    let close_line_start = line_start_of(text, close);
    let brace_alone_on_its_line = close_line_start > open && text[close_line_start..close].trim().is_empty();

    let mut edits = Vec::with_capacity(2);
    if brace_alone_on_its_line {
        if needs_comma {
            edits.push(insertion_at(index, text, last_end, ",".to_string()));
        }
        edits.push(insertion_at(index, text, close_line_start, format!("{entry}{nl}")));
    } else {
        // One-line table: `handlers! { "a" => … }` -> entry and brace on their own lines. The comma (when
        // needed) is part of the same insertion if nothing but whitespace separates the last entry from
        // the brace, so two edits never share one offset.
        let comma = if needs_comma { "," } else { "" };
        if needs_comma && last_end != close {
            edits.push(insertion_at(index, text, last_end, ",".to_string()));
            edits.push(insertion_at(index, text, close, format!("{nl}{entry}{nl}{indent}")));
        } else {
            edits.push(insertion_at(index, text, close, format!("{comma}{nl}{entry}{nl}{indent}")));
        }
    }
    edits
}

/// Finds the first `handlers!` macro invocation's `{ … }` block, returning
/// `(keyword_offset, open_brace_offset, close_brace_offset)` — `keyword_offset`
/// is where the literal `handlers!` starts (used to find *its own line's*
/// indentation, even when other code shares that line, e.g. `let mut h =
/// handlers! {`); `open`/`close` point *at* the `{`/`}` characters themselves.
/// A small brace-depth scan that skips over `"…"` string literals (so a
/// handler body's own braces inside a string never miscount) — deliberately
/// not a Rust parser, mirroring this crate's established "plain text search"
/// simplification (see the module doc). `None` when no `handlers!` invocation
/// is found, or its block is unterminated.
fn find_handlers_block(text: &str) -> Option<(usize, usize, usize)> {
    let handlers_at = text.find("handlers!")?;
    let after = &text[handlers_at + "handlers!".len()..];
    let open = handlers_at + "handlers!".len() + after.find('{')?;

    let bytes = text.as_bytes();
    let mut depth: i32 = 1;
    let mut i = open + 1;
    let mut in_string = false;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' if in_string => {
                i += 2;
                continue;
            }
            b'"' => in_string = !in_string,
            b'{' if !in_string => depth += 1,
            b'}' if !in_string => {
                depth -= 1;
                if depth == 0 {
                    return Some((handlers_at, open, i));
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use kubuno_views::syntax::parse;
    use std::io::Write;
    use std::fs;

    fn open_doc(text: &str) -> Document {
        Document {
            position_index: crate::position::PositionIndex::new(text),
            parse: parse(text),
            text: text.to_string(),
            version: 1,
        }
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("kubuno-views-ls-handler-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_file(dir: &Path, name: &str, contents: &str) -> PathBuf {
        let path = dir.join(name);
        let mut f = fs::File::create(&path).unwrap();
        f.write_all(contents.as_bytes()).unwrap();
        path
    }

    fn kbview_uri(dir: &Path, name: &str) -> Uri {
        fs_uri::from_path(&dir.join(name)).unwrap()
    }

    #[test]
    fn default_handler_name_strips_on_prefix_and_snake_cases() {
        assert_eq!(default_handler_name(Some("saveBtn"), "Button", "OnClick"), "on_save_btn_click");
        assert_eq!(default_handler_name(None, "Button", "OnClick"), "on_button_click");
        assert_eq!(default_handler_name(Some(""), "Switch", "OnToggled"), "on_switch_toggled");
    }

    #[test]
    fn to_snake_case_handles_pascal_and_separators() {
        assert_eq!(to_snake_case("SaveButton"), "save_button");
        assert_eq!(to_snake_case("save-btn 1"), "save_btn_1");
        assert_eq!(to_snake_case(""), "");
    }

    #[test]
    fn find_handlers_block_skips_braces_inside_string_literals() {
        let text = r#"kubuno_views::handlers! {
    "a" => |vm, v| { vm.set("x", Value::Str("{}".to_string())); },
};"#;
        let (_handlers_at, open, close) = find_handlers_block(text).expect("a block");
        assert_eq!(&text[open..=open], "{");
        assert_eq!(&text[close..=close], "}");
        // Everything strictly between the two is the table body.
        assert!(text[open + 1..close].contains("\"a\" =>"));
    }

    #[test]
    fn creates_a_new_handler_with_a_fn_stub_and_table_entry() {
        let dir = temp_dir("new");
        let rs_text = "pub struct View;\n\nfn handlers() -> HandlerTable {\n    kubuno_views::handlers! {\n        \"existing\" => |vm, v| { let _ = (vm, v); },\n    }\n}\n";
        write_file(&dir, "settings_view.rs", rs_text);
        let kbview_text = r#"<Button Text="Save"/>"#;
        let uri = kbview_uri(&dir, "settings_view.kbview");
        let doc = open_doc(kbview_text);

        let params = CreateHandlerParams {
            uri: uri.clone(),
            element_id: String::new(),
            event: "OnClick".to_string(),
            suggested_name: None,
            open_files: HashMap::new(),
        };
        let result = create_handler(Some(&doc), &uri, &params);

        assert_eq!(result.handler_name, "on_button_click");
        assert!(result.location.is_none());
        let workspace_edit = result.edit.expect("a workspace edit");
        #[allow(clippy::mutable_key_type)] // See `create_handler`'s own `changes` map for why this is safe.
        let changes = workspace_edit.changes.expect("changes map");
        assert_eq!(changes.len(), 2);

        let kbview_edits = changes.get(&uri).expect("kbview edit");
        assert_eq!(kbview_edits.len(), 1);
        // No `OnClick` attribute existed yet, so `set_attribute` inserts the
        // whole ` OnClick="…"` — not just a value replacement (see
        // `kubuno_views::edit::set_attribute`'s own doc).
        assert_eq!(kbview_edits[0].new_text, " OnClick=\"on_button_click\"");

        let rs_uri = fs_uri::from_path(&dir.join("settings_view.rs")).unwrap();
        let rs_edits = changes.get(&rs_uri).expect("code-behind edit");
        assert_eq!(rs_edits.len(), 2);
        assert!(rs_edits[0].new_text.contains("fn on_button_click(vm: &mut dyn kubuno_views::binding::ViewModel"));
        assert!(rs_edits[1].new_text.contains("\"on_button_click\" => |vm, value| on_button_click(vm, value),"));

        std::fs::remove_dir_all(&dir).ok();
    }

    /// Applies `edits` (all relative to `text`, as an LSP client does) and returns the result.
    fn apply(text: &str, edits: &[TextEdit]) -> String {
        let index = PositionIndex::new(text);
        let mut spans: Vec<(usize, usize, &str)> = edits
            .iter()
            .map(|e| {
                let start = u32::from(index.position_to_offset(text, e.range.start)) as usize;
                let end = u32::from(index.position_to_offset(text, e.range.end)) as usize;
                (start, end, e.new_text.as_str())
            })
            .collect();
        spans.sort_by_key(|s| std::cmp::Reverse(s.0));
        let mut out = text.to_string();
        for (start, end, new_text) in spans {
            out.replace_range(start..end, new_text);
        }
        out
    }

    fn code_behind_after_create(tag: &str, rs_text: &str) -> String {
        let dir = temp_dir(tag);
        write_file(&dir, "main_view.rs", rs_text);
        let uri = kbview_uri(&dir, "main_view.kbview");
        let doc = open_doc(r#"<Switch x:Name="dark"/>"#);
        let params = CreateHandlerParams {
            uri: uri.clone(),
            element_id: String::new(),
            event: "OnToggled".to_string(),
            suggested_name: None,
            open_files: HashMap::new(),
        };
        let result = create_handler(Some(&doc), &uri, &params);
        #[allow(clippy::mutable_key_type)] // See `create_handler`'s own `changes` map for why this is safe.
        let changes = result.edit.expect("edit").changes.expect("changes");
        let rs_uri = fs_uri::from_path(&dir.join("main_view.rs")).unwrap();
        let out = apply(rs_text, changes.get(&rs_uri).expect("rs edits"));
        std::fs::remove_dir_all(&dir).ok();
        out
    }

    const STUB: &str = "    fn on_dark_toggled(vm: &mut dyn kubuno_views::binding::ViewModel, value: kubuno_views::binding::Value) {\n        // TODO: implement on_dark_toggled\n        let _ = (vm, value);\n    }\n\n";

    #[test]
    fn table_entry_keeps_the_closing_brace_indentation() {
        // The project template's own shape: the new entry lands after the existing one, at the same
        // indentation, and `handlers!`'s closing brace stays at its 4-space indentation.
        let rs = "pub fn handler_table() -> HandlerTable {\n    handlers! {\n        \"say_hello_clicked\" => |vm, _v| {\n            vm.set(\"Status\", Value::Str(\"Hello\".to_string()));\n        },\n    }\n}\n";
        let expected = format!(
            "pub fn handler_table() -> HandlerTable {{\n{STUB}    handlers! {{\n        \"say_hello_clicked\" => |vm, _v| {{\n            vm.set(\"Status\", Value::Str(\"Hello\".to_string()));\n        }},\n        \"on_dark_toggled\" => |vm, value| on_dark_toggled(vm, value),\n    }}\n}}\n"
        );
        assert_eq!(code_behind_after_create("indent", rs), expected);
    }

    #[test]
    fn table_entry_adds_the_missing_comma_after_the_last_entry() {
        let rs = "fn t() -> HandlerTable {\n    handlers! {\n        \"a\" => |vm, v| { let _ = (vm, v); }\n    }\n}\n";
        let out = code_behind_after_create("comma", rs);
        assert!(
            out.contains("        \"a\" => |vm, v| { let _ = (vm, v); },\n        \"on_dark_toggled\" => |vm, value| on_dark_toggled(vm, value),\n    }\n}\n"),
            "{out}"
        );
    }

    #[test]
    fn table_entry_in_an_empty_or_one_line_table() {
        let out = code_behind_after_create("empty", "fn t() -> HandlerTable {\n    handlers! {\n    }\n}\n");
        assert!(out.ends_with("    handlers! {\n        \"on_dark_toggled\" => |vm, value| on_dark_toggled(vm, value),\n    }\n}\n"), "{out}");

        let out = code_behind_after_create("oneline-empty", "fn t() -> HandlerTable {\n    handlers! {}\n}\n");
        assert!(out.ends_with("    handlers! {\n        \"on_dark_toggled\" => |vm, value| on_dark_toggled(vm, value),\n    }\n}\n"), "{out}");

        let out = code_behind_after_create("oneline", "fn t() -> HandlerTable {\n    handlers! { \"a\" => |vm, v| { let _ = (vm, v); } }\n}\n");
        assert!(
            out.ends_with("    handlers! { \"a\" => |vm, v| { let _ = (vm, v); }, \n        \"on_dark_toggled\" => |vm, value| on_dark_toggled(vm, value),\n    }\n}\n"),
            "{out}"
        );
    }

    #[test]
    fn crlf_code_behind_stays_crlf() {
        let rs = "fn t() -> HandlerTable {\r\n    handlers! {\r\n        \"a\" => |vm, v| { let _ = (vm, v); },\r\n    }\r\n}\r\n";
        let out = code_behind_after_create("crlf", rs);
        assert_eq!(out.matches('\n').count(), out.matches("\r\n").count(), "{out:?}");
        assert!(out.contains("        \"on_dark_toggled\" => |vm, value| on_dark_toggled(vm, value),\r\n    }\r\n}\r\n"), "{out:?}");
    }

    /// The typed code-behind of the Kubuno Desktop Application template (EVT-4).
    const TYPED_VIEW: &str = "//! Code-behind.\n\nuse kubuno_views::prelude::*;\n\npub struct MainViewModel {\n    pub status: String,\n}\n\n#[kubuno_views::event_handlers]\nimpl MainViewModel {\n    fn hello_click(&mut self, sender: &Sender<Button>, e: &MouseEventArgs) {\n        self.status = \"Hello\".into();\n    }\n}\n";

    /// Creates the handler of `event` on the element `element_id` of `kbview` against the
    /// code-behind `rs`, applies the edits, and returns (handler name, new code-behind).
    fn typed_round_trip(tag: &str, rs: &str, kbview: &str, element_id: &str, event: &str) -> (String, String) {
        let dir = temp_dir(tag);
        write_file(&dir, "main_view.rs", rs);
        let uri = kbview_uri(&dir, "main_view.kbview");
        let doc = open_doc(kbview);
        let params = CreateHandlerParams { uri: uri.clone(), element_id: element_id.into(), event: event.into(), suggested_name: None, open_files: HashMap::new() };
        let result = create_handler(Some(&doc), &uri, &params);
        #[allow(clippy::mutable_key_type)] // See `create_handler`'s own `changes` map for why this is safe.
        let changes = result.edit.expect("edit").changes.expect("changes");
        let rs_uri = fs_uri::from_path(&dir.join("main_view.rs")).unwrap();
        let out = apply(rs, changes.get(&rs_uri).expect("rs edits"));
        std::fs::remove_dir_all(&dir).ok();
        (result.handler_name, out)
    }

    #[test]
    fn a_typed_code_behind_gets_a_typed_method_in_its_impl() {
        let kbview = r#"<Panel><Button x:Name="second" Text="Again"/></Panel>"#;
        let (name, out) = typed_round_trip("typed-click", TYPED_VIEW, kbview, "0", "OnClick");
        assert_eq!(name, "on_second_click");
        let expected = TYPED_VIEW.replace(
            "        self.status = \"Hello\".into();\n    }\n}\n",
            "        self.status = \"Hello\".into();\n    }\n\n    fn on_second_click(&mut self, sender: &Sender<Button>, e: &MouseEventArgs) {\n        // TODO: implement on_second_click\n    }\n}\n",
        );
        assert_eq!(out, expected);
        assert!(!out.contains("handlers!"), "no legacy table entry in a typed code-behind");
    }

    #[test]
    fn typed_stubs_follow_the_event_args() {
        let kbview = r#"<Panel><Button x:Name="go"/><TextField x:Name="name"/><Switch x:Name="dark"/></Panel>"#;
        let cases = [
            ("0", "OnMouseDown", "fn on_go_mouse_down(&mut self, sender: &Sender<Button>, e: &MouseEventArgs)"),
            ("1", "OnKeyDown", "fn on_name_key_down(&mut self, sender: &Sender<TextField>, e: &mut KeyEventArgs)"),
            ("1", "OnTextChanged", "fn on_name_text_changed(&mut self, sender: &Sender<TextField>, e: &TextChangedEventArgs)"),
            ("1", "OnValidating", "fn on_name_validating(&mut self, sender: &Sender<TextField>, e: &mut CancelEventArgs)"),
            ("2", "OnToggled", "fn on_dark_toggled(&mut self, sender: &Sender<Switch>, e: &CheckedChangedEventArgs)"),
            ("0", "OnGotFocus", "fn on_go_got_focus(&mut self, sender: &Sender<Button>, e: &dyn EventArgs)"),
            ("", "OnLoad", "fn on_panel_load(&mut self, sender: &Sender<Panel>, e: &dyn EventArgs)"),
        ];
        for (id, event, signature) in cases {
            let (_, out) = typed_round_trip(&format!("typed-{}", event.to_lowercase()), TYPED_VIEW, kbview, id, event);
            assert!(out.contains(&format!("    {signature} {{\n")), "{event}: {out}");
        }
    }

    /// The `main_view.rs` of the Kubuno Desktop Application template (`#[kubuno::view]`).
    const FORM_CLASS: &str = "use kubuno::prelude::*;\n\n#[kubuno::view(\"main_view.kbview\")]\n#[derive(Default)]\npub struct MainView {}\n\nimpl MainView {\n    pub fn new() -> Self {\n        let mut view = Self::default();\n        view.initialize_component();\n        view\n    }\n\n    fn main_view_load(&mut self, sender: &Form, e: &EventArgs) {\n        // TODO: implement main_view_load\n    }\n}\n";

    #[test]
    fn a_form_class_gets_windows_forms_named_plain_methods() {
        let kbview = r#"<Panel OnLoad="main_view_load"><TextField x:Name="status"/><Button x:Name="hello" Text="Say hello"/><Label/></Panel>"#;
        let (name, out) = typed_round_trip("view-click", FORM_CLASS, kbview, "1", "OnClick");
        assert_eq!(name, "hello_click");
        assert!(
            out.ends_with("        // TODO: implement main_view_load\n    }\n\n    fn hello_click(&mut self, _sender: &Button, _e: &MouseEventArgs) {\n        // TODO: implement hello_click\n    }\n}\n"),
            "{out}"
        );
        let cases = [
            ("0", "OnTextChanged", "fn status_text_changed(&mut self, _sender: &TextField, _e: &TextChangedEventArgs)"),
            ("0", "OnKeyDown", "fn status_key_down(&mut self, _sender: &TextField, _e: &mut KeyEventArgs)"),
            ("0", "OnGotFocus", "fn status_got_focus(&mut self, _sender: &TextField, _e: &EventArgs)"),
            ("2", "OnClick", "fn label_click(&mut self, _sender: &Label, _e: &MouseEventArgs)"),
            ("", "OnFormClosing", "fn main_view_form_closing(&mut self, _sender: &Form, _e: &mut FormClosingEventArgs)"),
        ];
        for (id, event, signature) in cases {
            let (_, out) = typed_round_trip(&format!("view-{}-{id}", event.to_lowercase()), FORM_CLASS, kbview, id, event);
            assert!(out.contains(&format!("    {signature} {{\n")), "{event}: {out}");
        }
    }

    /// Found by the chat migration: in a user control's code-behind of a `kubuno`-only application (the template's
    /// `#[kubuno::views::event_handlers]`), a double-click wrote a legacy free `fn on_new_message_click(vm, value)`.
    /// The stub is now a typed method of its `#[event_handlers]` impl, named the Windows Forms way.
    const USER_CONTROL_FACADE: &str = "use kubuno::views::prelude::*;

#[derive(UserControl, Default)]
#[user_control(view = \"main_view.kbview\")]
pub struct ListPane {
    base: UserControlCore,
}

#[kubuno::views::event_handlers]
impl ListPane {
    fn list_pane_load(&mut self) {}
}
";

    #[test]
    fn a_user_control_of_a_facade_application_gets_typed_windows_forms_named_methods() {
        let kbview = r#"<UserControl x:Class="ListPane"><IconButton x:Name="new_message"/></UserControl>"#;
        let (name, out) = typed_round_trip("uc-facade", USER_CONTROL_FACADE, kbview, "0", "OnClick");
        assert_eq!(name, "new_message_click");
        assert!(out.contains("impl ListPane {
    fn list_pane_load(&mut self) {}

    fn new_message_click(&mut self, sender: &Sender<IconButton>, e: &MouseEventArgs) {
"), "{out}");
        assert!(!out.contains("kubuno_views::binding::ViewModel") && !out.contains("use kubuno_views::prelude"), "no legacy stub, no second prelude: {out}");
        let (name, _) = typed_round_trip("uc-facade-load", USER_CONTROL_FACADE, r#"<UserControl x:Class="ListPane"/>"#, "", "OnLoad");
        assert_eq!(name, "main_view_load", "the view's own event is named after the view, like Form1_Load");
    }

    /// A double-click on a ribbon button or a menu item in the designer (their default event, OnClick)
    /// creates its handler like a button's: a method of the form class, its sender typed.
    #[test]
    fn ribbon_buttons_and_menu_items_get_their_click_handler() {
        let kbview = r#"<Panel OnLoad="main_view_load"><Ribbon><RibbonTab Header="Accueil"><RibbonGroup Header="Presse-papiers"><RibbonButton x:Name="paste" Label="Coller"/></RibbonGroup></RibbonTab></Ribbon><MenuBar><MenuItem Text="&amp;Fichier"><MenuItem x:Name="open_item" Text="&amp;Ouvrir"/></MenuItem></MenuBar><SplitButton x:Name="export" Text="Exporter"/></Panel>"#;
        let (name, out) = typed_round_trip("ribbon-click", FORM_CLASS, kbview, "0.0.0.0", "OnClick");
        assert_eq!(name, "paste_click");
        assert!(out.contains("fn paste_click(&mut self, "), "{out}");
        let (name, out) = typed_round_trip("menu-click", FORM_CLASS, kbview, "1.0.0", "OnClick");
        assert_eq!(name, "open_item_click");
        assert!(out.contains("fn open_item_click(&mut self, "), "{out}");
        let (name, _) = typed_round_trip("split-click", FORM_CLASS, kbview, "2", "OnClick");
        assert_eq!(name, "export_click");
    }

    #[test]
    fn a_form_class_without_an_impl_gets_one() {
        let rs = "#[kubuno::view(\"main_view.kbview\")]\npub struct MainView;\n";
        let (_, out) = typed_round_trip("view-no-impl", rs, r#"<Panel><Button x:Name="ok"/></Panel>"#, "0", "OnClick");
        assert_eq!(out, "#[kubuno::view(\"main_view.kbview\")]\npub struct MainView;\n\nimpl MainView {\n    fn ok_click(&mut self, _sender: &Button, _e: &MouseEventArgs) {\n        // TODO: implement ok_click\n    }\n}\n");
    }

    #[test]
    fn a_structural_element_gets_an_untyped_sender() {
        let kbview = r#"<Toolbar><ToolbarItem Text="New"/></Toolbar>"#;
        let (_, out) = typed_round_trip("typed-item", TYPED_VIEW, kbview, "0", "OnClick");
        assert!(out.contains("fn on_toolbar_item_click(&mut self, sender: &ElementRef, e: &ItemEventArgs) {"), "{out}");
    }

    #[test]
    fn a_typed_stub_adds_the_prelude_and_fills_an_empty_impl() {
        let rs = "use kubuno_views::binding::{Value, ViewModel};\n\npub struct Vm;\n\n#[event_handlers]\nimpl Vm {}\n";
        let (_, out) = typed_round_trip("typed-empty", rs, r#"<Button/>"#, "", "OnClick");
        assert_eq!(
            out,
            "use kubuno_views::binding::{Value, ViewModel};\nuse kubuno_views::prelude::*;\n\npub struct Vm;\n\n#[event_handlers]\nimpl Vm {\n    fn on_button_click(&mut self, sender: &Sender<Button>, e: &MouseEventArgs) {\n        // TODO: implement on_button_click\n    }\n}\n"
        );
        let multi_line_empty = "use kubuno_views::prelude::*;\n#[kubuno_views::event_handlers]\nimpl Vm {\n}\n";
        let (_, out) = typed_round_trip("typed-empty2", multi_line_empty, r#"<Button/>"#, "", "OnClick");
        assert!(out.ends_with("impl Vm {\n    fn on_button_click(&mut self, sender: &Sender<Button>, e: &MouseEventArgs) {\n        // TODO: implement on_button_click\n    }\n}\n"), "{out}");
    }

    #[test]
    fn creates_a_stub_only_fn_when_the_file_has_no_handlers_table() {
        let dir = temp_dir("nohandlers");
        write_file(&dir, "plain_view.rs", "pub struct View;\n");
        let uri = kbview_uri(&dir, "plain_view.kbview");
        let doc = open_doc(r#"<Button Text="Go"/>"#);

        let params = CreateHandlerParams {
            uri: uri.clone(),
            element_id: String::new(),
            event: "OnClick".to_string(),
            suggested_name: None,
            open_files: HashMap::new(),
        };
        let result = create_handler(Some(&doc), &uri, &params);
        #[allow(clippy::mutable_key_type)] // See `create_handler`'s own `changes` map for why this is safe.
        let changes = result.edit.expect("edit").changes.expect("changes");
        let rs_uri = fs_uri::from_path(&dir.join("plain_view.rs")).unwrap();
        let rs_edits = changes.get(&rs_uri).expect("rs edit");
        assert_eq!(rs_edits.len(), 1, "no handlers! table -> no registration entry");
        assert!(rs_edits[0].new_text.contains("fn on_button_click"));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn an_event_wired_under_its_older_alias_is_found_by_its_new_name() {
        let dir = temp_dir("alias");
        write_file(&dir, "settings_view.rs", "fn offline_toggled(vm: &mut dyn kubuno_views::binding::ViewModel, value: kubuno_views::binding::Value) {}\n");
        let uri = kbview_uri(&dir, "settings_view.kbview");
        let doc = open_doc(r#"<Switch OnToggled="offline_toggled"/>"#);
        let params = CreateHandlerParams {
            uri: uri.clone(),
            element_id: String::new(),
            event: "OnCheckedChanged".to_string(),
            suggested_name: None,
            open_files: HashMap::new(),
        };
        let result = create_handler(Some(&doc), &uri, &params);
        assert_eq!(result.handler_name, "offline_toggled");
        assert!(result.edit.is_none(), "no second attribute next to the alias");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_common_event_and_a_root_view_event_get_a_handler() {
        let dir = temp_dir("common");
        write_file(&dir, "main_view.rs", "pub struct V;\n");
        let uri = kbview_uri(&dir, "main_view.kbview");
        let doc = open_doc(r#"<Panel><Button x:Name="go"/></Panel>"#);
        let mouse_down = CreateHandlerParams { uri: uri.clone(), element_id: "0".into(), event: "OnMouseDown".into(), suggested_name: None, open_files: HashMap::new() };
        let result = create_handler(Some(&doc), &uri, &mouse_down);
        assert_eq!(result.handler_name, "on_go_mouse_down");
        assert!(result.edit.is_some());
        let load = CreateHandlerParams { uri: uri.clone(), element_id: String::new(), event: "OnLoad".into(), suggested_name: None, open_files: HashMap::new() };
        assert_eq!(create_handler(Some(&doc), &uri, &load).handler_name, "on_panel_load");
        let not_root = CreateHandlerParams { uri: uri.clone(), element_id: "0".into(), event: "OnLoad".into(), suggested_name: None, open_files: HashMap::new() };
        assert!(create_handler(Some(&doc), &uri, &not_root).edit.is_none(), "OnLoad is the root's only");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn already_wired_event_returns_only_a_location() {
        let dir = temp_dir("existing");
        write_file(&dir, "settings_view.rs", "fn offline_toggled(vm: &mut dyn kubuno_views::binding::ViewModel, value: kubuno_views::binding::Value) {}\n");
        let uri = kbview_uri(&dir, "settings_view.kbview");
        let doc = open_doc(r#"<Switch OnToggled="offline_toggled"/>"#);

        let params = CreateHandlerParams {
            uri: uri.clone(),
            element_id: String::new(),
            event: "OnToggled".to_string(),
            suggested_name: None,
            open_files: HashMap::new(),
        };
        let result = create_handler(Some(&doc), &uri, &params);

        assert_eq!(result.handler_name, "offline_toggled");
        assert!(result.edit.is_none());
        let location = result.location.expect("a definition location");
        assert_eq!(location.range.start.line, 0);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn suggested_name_is_deduplicated_against_existing_fns() {
        let dir = temp_dir("dedupe");
        write_file(&dir, "settings_view.rs", "fn my_handler() {}\n");
        let uri = kbview_uri(&dir, "settings_view.kbview");
        let doc = open_doc(r#"<Button Text="Go"/>"#);

        let params = CreateHandlerParams {
            uri: uri.clone(),
            element_id: String::new(),
            event: "OnClick".to_string(),
            suggested_name: Some("my_handler".to_string()),
            open_files: HashMap::new(),
        };
        let result = create_handler(Some(&doc), &uri, &params);
        assert_eq!(result.handler_name, "my_handler_2");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn unknown_component_is_a_no_op() {
        let dir = temp_dir("unknown");
        let uri = kbview_uri(&dir, "settings_view.kbview");
        let doc = open_doc(r#"<TotallyMadeUp OnClick="x"/>"#);
        let params = CreateHandlerParams {
            uri: uri.clone(),
            element_id: String::new(),
            event: "OnClick".to_string(),
            suggested_name: None,
            open_files: HashMap::new(),
        };
        let result = create_handler(Some(&doc), &uri, &params);
        assert!(result.edit.is_none());
        assert!(result.location.is_none());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn undeclared_event_is_a_no_op() {
        let dir = temp_dir("undeclared-event");
        let uri = kbview_uri(&dir, "settings_view.kbview");
        let doc = open_doc(r#"<Button Text="Go"/>"#);
        let params = CreateHandlerParams {
            uri: uri.clone(),
            element_id: String::new(),
            event: "OnNotARealEvent".to_string(),
            suggested_name: None,
            open_files: HashMap::new(),
        };
        let result = create_handler(Some(&doc), &uri, &params);
        assert!(result.edit.is_none());
        assert!(result.location.is_none());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn missing_document_is_a_no_op() {
        let uri: Uri = "file:///nope.kbview".parse().unwrap();
        let params = CreateHandlerParams {
            uri: uri.clone(),
            element_id: String::new(),
            event: "OnClick".to_string(),
            suggested_name: None,
            open_files: HashMap::new(),
        };
        let result = create_handler(None, &uri, &params);
        assert_eq!(result.handler_name, "");
        assert!(result.edit.is_none());
        assert!(result.location.is_none());
    }
}
