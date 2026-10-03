//! "Convert the `handlers!` table to typed handlers" (`vskubuno/docs/EVENTS.md` §5.4 point 4,
//! EVT-4): rewrites a view's legacy code-behind into the typed form, mechanically.
//!
//! Each `"name" => |vm, value| body` entry of the `handlers!` table becomes a method of the
//! view model in a `#[kubuno_views::event_handlers]` impl (appended to an existing one), with
//! the legacy parameters bound the way the table bound them — `vm` to `self` (the concrete
//! view model, still a `ViewModel`), `value` to `e.legacy_value()` (exactly the value the event
//! produced for the table):
//!
//! ```text
//! handlers! {                                      #[kubuno_views::event_handlers]
//!     "save_clicked" => |vm, _v| {          →      impl MainViewModel {
//!         vm.set("Status", …);                         fn save_clicked(&mut self) {
//!     },                                                   let vm = self;
//! }                                                        vm.set("Status", …);
//!                                                      }
//!                                                  }
//! ```
//!
//! The table is left empty (`handlers! {}`) so the function returning it keeps compiling, and
//! every `runtime.frame(canvas, frame, &mut vm, &mut handlers, bounds)` call of the sibling
//! files becomes `runtime.frame_typed_with(…)` with the same arguments, which dispatches to the
//! typed methods (and to whatever the table still holds). `use kubuno_views::prelude::*;` is
//! added when missing. The action is offered only when the code-behind has exactly one
//! `impl ViewModel for …` and a table whose entries all have the `"name" => |a, b| body` shape.
//!
//! Offered two ways: as a `refactor.rewrite` code action on the `.kbview` (the light bulb of
//! its XML editor) and as the `kubuno/convertHandlers { uri }` request (the designer's
//! command); both return the same [`WorkspaceEdit`], one edit list per file.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use lsp_types::{CodeAction, CodeActionKind, Range, TextEdit, Uri, WorkspaceEdit};
use serde::{Deserialize, Serialize};

use crate::code_behind::{self, TableEntry};
use crate::definition;
use crate::fs_uri;
use crate::handler_insert;
use crate::position::PositionIndex;

/// The code action's title.
pub const TITLE: &str = "Convert the handlers! table to typed handlers";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConvertHandlersParams {
    /// The `.kbview` whose code-behind is converted (or the code-behind `.rs` itself).
    pub uri: Uri,
    /// The client's open documents (`uri -> text`), see `crate::sources`.
    #[serde(default)]
    pub open_files: HashMap<String, String>,
}

#[derive(Debug, Serialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ConvertHandlersResult {
    /// The edits, `None` when there is nothing to convert (see `reason`).
    pub edit: Option<WorkspaceEdit>,
    /// The handler names that became methods.
    pub converted: Vec<String>,
    /// Why nothing was converted.
    pub reason: Option<String>,
}

fn not_converted(reason: impl Into<String>) -> ConvertHandlersResult {
    ConvertHandlersResult { reason: Some(reason.into()), ..Default::default() }
}

/// `kubuno/convertHandlers`.
pub fn convert_handlers(uri: &Uri) -> ConvertHandlersResult {
    let Some(path) = fs_uri::to_path(uri) else { return not_converted("not a file URI") };
    let Some(dir) = path.parent() else { return not_converted("no folder") };
    let code_behind = if path.extension().and_then(|e| e.to_str()) == Some("rs") {
        Some(path.clone())
    } else {
        handler_insert::pick_code_behind_file(dir, &path)
    };
    let Some(code_behind) = code_behind else { return not_converted("no code-behind .rs file next to the view") };
    let Some(text) = crate::sources::read(&code_behind) else { return not_converted("the code-behind could not be read") };
    convert_file(dir, &code_behind, &text)
}

/// The `refactor.rewrite` code action for the `.kbview` at `uri`, when its code-behind can be
/// converted.
pub fn code_actions(uri: &Uri) -> Vec<CodeAction> {
    let result = convert_handlers(uri);
    match result.edit {
        Some(edit) => vec![CodeAction {
            title: TITLE.to_string(),
            kind: Some(CodeActionKind::REFACTOR_REWRITE),
            edit: Some(edit),
            ..Default::default()
        }],
        None => Vec::new(),
    }
}

/// Method names a handler cannot take as they are: the view-model and sink methods a legacy
/// body calls on `vm` (an inherent `set` would shadow `ViewModel::set`).
const RESERVED: &[&str] = &["get", "set", "dispatch_event", "handle_event", "handler_info"];

fn convert_file(dir: &Path, code_behind: &Path, text: &str) -> ConvertHandlersResult {
    let Some(table) = code_behind::find_handlers_table(text) else {
        return not_converted("the code-behind has no handlers! table in the \"name\" => |vm, value| body form");
    };
    if table.entries.is_empty() {
        return not_converted("the handlers! table is empty");
    }
    let Some(vm_impl) = code_behind::find_view_model_impl(text) else {
        return not_converted("the code-behind needs exactly one `impl ViewModel for …`");
    };
    let nl = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let index = PositionIndex::new(text);
    let mut edits = Vec::new();

    if !code_behind::has_prelude(text) {
        let (at, import) = code_behind::prelude_insertion(text, nl);
        edits.push(insertion(&index, text, at, import));
    }

    let typed = code_behind::find_typed_impl(text).filter(|imp| imp.self_ty == vm_impl.self_ty);
    let impl_indent = typed.as_ref().map(|imp| code_behind::leading_whitespace(&text[code_behind::line_start_of(text, imp.open)..])).unwrap_or_default();
    let method_indent = format!("{impl_indent}    ");
    let methods: Vec<String> = table.entries.iter().map(|e| method_text(e, &method_indent, nl)).collect();
    match &typed {
        Some(imp) => {
            let close_line = code_behind::line_start_of(text, imp.close);
            let body_empty = text[imp.open + 1..imp.close].trim().is_empty();
            let joined = methods.join(nl);
            if close_line > imp.open && text[close_line..imp.close].trim().is_empty() {
                let separator = if body_empty { "" } else { nl };
                edits.push(insertion(&index, text, close_line, format!("{separator}{joined}")));
            } else {
                edits.push(insertion(&index, text, imp.close, format!("{nl}{joined}{impl_indent}")));
            }
        }
        None => {
            let vm_ty = &vm_impl.self_ty;
            let block = format!(
                "{nl}{nl}/// The handlers `On*` attributes name (converted from the `handlers!` table: `vm` is `self`,{nl}\
                 /// `value` is `e.legacy_value()`).{nl}#[kubuno_views::event_handlers]{nl}impl {vm_ty} {{{nl}{}}}",
                methods.join(nl)
            );
            edits.push(insertion(&index, text, vm_impl.close + 1, block));
        }
    }

    // The table itself: emptied, so its function keeps compiling (and can be deleted later).
    let inner = &text[table.open + 1..table.close];
    if !inner.is_empty() {
        let start = index.offset_to_position(text, rowan::TextSize::from((table.open + 1) as u32));
        let end = index.offset_to_position(text, rowan::TextSize::from(table.close as u32));
        edits.push(TextEdit { range: Range { start, end }, new_text: String::new() });
    }

    #[allow(clippy::mutable_key_type)] // See `handler_insert::create_handler`'s map.
    let mut changes: HashMap<Uri, Vec<TextEdit>> = HashMap::new();
    let Some(code_behind_uri) = fs_uri::from_path(code_behind) else { return not_converted("not a file path") };
    // `frame(…, &mut handlers, …)` → `frame_typed_with(…)`, in this file and its siblings.
    for file in std::iter::once(code_behind.to_path_buf()).chain(definition::sibling_rs_files(dir).into_iter().filter(|p| !same_file(p, code_behind))) {
        let is_code_behind = same_file(&file, code_behind);
        let owned;
        let file_text = if is_code_behind {
            text
        } else {
            match crate::sources::read(&file) {
                Some(t) => {
                    owned = t;
                    owned.as_str()
                }
                None => continue,
            }
        };
        let calls = code_behind::legacy_frame_calls(file_text);
        if calls.is_empty() {
            continue;
        }
        let file_index = PositionIndex::new(file_text);
        let renames = calls.into_iter().map(|at| {
            let start = file_index.offset_to_position(file_text, rowan::TextSize::from(at as u32));
            let end = file_index.offset_to_position(file_text, rowan::TextSize::from((at + "frame".len()) as u32));
            TextEdit { range: Range { start, end }, new_text: "frame_typed_with".to_string() }
        });
        if is_code_behind {
            edits.extend(renames);
        } else if let Some(file_uri) = fs_uri::from_path(&file) {
            changes.insert(file_uri, renames.collect());
        }
    }
    changes.insert(code_behind_uri, edits);

    ConvertHandlersResult {
        edit: Some(WorkspaceEdit { changes: Some(changes), ..Default::default() }),
        converted: table.entries.iter().map(|e| e.name.clone()).collect(),
        reason: None,
    }
}

fn same_file(a: &Path, b: &Path) -> bool {
    let canon = |p: &Path| fs::canonicalize(p).unwrap_or_else(|_| PathBuf::from(p));
    canon(a) == canon(b)
}

fn insertion(index: &PositionIndex, text: &str, at: usize, new_text: String) -> TextEdit {
    let pos = index.offset_to_position(text, rowan::TextSize::from(at as u32));
    TextEdit { range: Range { start: pos, end: pos }, new_text }
}

/// One table entry as a method, at `indent`.
fn method_text(entry: &TableEntry, indent: &str, nl: &str) -> String {
    let mut out = String::new();
    let method = if code_behind::is_method_ident(&entry.name) && !RESERVED.contains(&entry.name.as_str()) {
        entry.name.clone()
    } else {
        let mut snake = handler_insert::to_snake_case(&entry.name);
        if snake.is_empty() || snake.starts_with(|c: char| c.is_ascii_digit()) || !code_behind::is_method_ident(&snake) {
            snake = format!("on_{snake}");
        }
        if RESERVED.contains(&snake.as_str()) {
            snake.push_str("_handler");
        }
        out.push_str(&format!("{indent}#[handler(name = \"{}\")]{nl}", entry.name.replace('\\', "\\\\").replace('"', "\\\"")));
        snake
    };
    let binds = |pat: &str| !pat.starts_with('_');
    let args = if binds(&entry.value_pat) { ", e: &dyn EventArgs" } else { "" };
    out.push_str(&format!("{indent}fn {method}(&mut self{args}) {{{nl}"));
    let body_indent = format!("{indent}    ");
    if binds(&entry.vm_pat) {
        out.push_str(&format!("{body_indent}let {} = self;{nl}", entry.vm_pat));
    }
    if binds(&entry.value_pat) {
        out.push_str(&format!("{body_indent}let {} = e.legacy_value();{nl}", entry.value_pat));
    }
    let body = entry.body.trim();
    let block = body.starts_with('{') && code_behind::match_bracket(body, 0) == Some(body.len() - 1);
    let inner = if block { body[1..body.len() - 1].to_string() } else { format!("{body};") };
    for line in reindent(&inner) {
        if line.is_empty() {
            out.push_str(nl);
        } else {
            out.push_str(&format!("{body_indent}{line}{nl}"));
        }
    }
    out.push_str(&format!("{indent}}}{nl}"));
    out
}

/// The lines of `inner` without their common indentation (the first line, which follows the
/// block's `{` on the same line, is trimmed on its own), leading and trailing blank lines dropped.
fn reindent(inner: &str) -> Vec<String> {
    let lines: Vec<&str> = inner.lines().collect();
    let common = lines
        .iter()
        .skip(1)
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.len() - l.trim_start().len())
        .min()
        .unwrap_or(0);
    let mut out: Vec<String> = lines
        .iter()
        .enumerate()
        .map(|(i, l)| {
            if i == 0 {
                l.trim().to_string()
            } else if l.trim().is_empty() {
                String::new()
            } else {
                l[common.min(l.len() - l.trim_start().len())..].trim_end().to_string()
            }
        })
        .collect();
    while out.first().is_some_and(String::is_empty) {
        out.remove(0);
    }
    while out.last().is_some_and(String::is_empty) {
        out.pop();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Applies `edits` (all relative to `text`, as an LSP client does).
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

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("kubuno-views-ls-convert-{tag}-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// The code-behind of the previous Kubuno Desktop Application template.
    const LEGACY_VIEW: &str = r#"//! Code-behind for `main_view.kbview`.

use kubuno_views::binding::{HandlerTable, Value, ViewModel};
use kubuno_views::handlers;

pub struct MainViewModel {
    pub status: String,
}

impl ViewModel for MainViewModel {
    fn get(&self, path: &str) -> Option<Value> {
        match path {
            "Status" => Some(Value::Str(self.status.clone())),
            _ => None,
        }
    }

    fn set(&mut self, path: &str, value: Value) {
        if let ("Status", Value::Str(s)) = (path, value) {
            self.status = s;
        }
    }
}

/// Handlers referenced by name from `main_view.kbview`.
pub fn handler_table() -> HandlerTable {
    handlers! {
        "say_hello_clicked" => |vm, _v| {
            vm.set("Status", Value::Str("Hello!".to_string()));
        },
        "on_second_click" => |vm, value| on_second_click(vm, value),
        "save-file" => |_vm, on| { let _ = on; },
    }
}

fn on_second_click(vm: &mut dyn kubuno_views::binding::ViewModel, value: kubuno_views::binding::Value) {
    let _ = (vm, value);
}
"#;

    const LEGACY_MAIN: &str = "fn main() {\n    let events = runtime.frame(canvas, frame, &mut view_model, &mut handlers, body);\n}\n";

    #[test]
    fn converts_the_legacy_template_mechanically() {
        let dir = temp_dir("template");
        fs::write(dir.join("main_view.rs"), LEGACY_VIEW).unwrap();
        fs::write(dir.join("main.rs"), LEGACY_MAIN).unwrap();
        let uri = fs_uri::from_path(&dir.join("main_view.kbview")).unwrap();

        let result = convert_handlers(&uri);
        assert_eq!(result.converted, ["say_hello_clicked", "on_second_click", "save-file"]);
        #[allow(clippy::mutable_key_type)]
        let changes = result.edit.expect("edit").changes.expect("changes");
        let view_uri = fs_uri::from_path(&dir.join("main_view.rs")).unwrap();
        let main_uri = fs_uri::from_path(&dir.join("main.rs")).unwrap();
        let view = apply(LEGACY_VIEW, &changes[&view_uri]);
        let main = apply(LEGACY_MAIN, &changes[&main_uri]);

        assert!(view.contains("use kubuno_views::handlers;\nuse kubuno_views::prelude::*;\n"), "{view}");
        let expected_impl = concat!(
            "\n\n/// The handlers `On*` attributes name (converted from the `handlers!` table: `vm` is `self`,\n",
            "/// `value` is `e.legacy_value()`).\n#[kubuno_views::event_handlers]\nimpl MainViewModel {\n",
            "    fn say_hello_clicked(&mut self) {\n        let vm = self;\n        vm.set(\"Status\", Value::Str(\"Hello!\".to_string()));\n    }\n\n",
            "    fn on_second_click(&mut self, e: &dyn EventArgs) {\n        let vm = self;\n        let value = e.legacy_value();\n        on_second_click(vm, value);\n    }\n\n",
            "    #[handler(name = \"save-file\")]\n    fn save_file(&mut self, e: &dyn EventArgs) {\n        let on = e.legacy_value();\n        let _ = on;\n    }\n}",
        );
        assert!(view.contains(&format!("}}{expected_impl}\n\n/// Handlers referenced")), "{view}");
        assert!(view.contains("    handlers! {}\n"), "{view}");
        assert_eq!(main, "fn main() {\n    let events = runtime.frame_typed_with(canvas, frame, &mut view_model, &mut handlers, body);\n}\n");

        // Converted once: the table is empty now, nothing more to offer.
        fs::write(dir.join("main_view.rs"), &view).unwrap();
        assert_eq!(convert_handlers(&uri).reason.as_deref(), Some("the handlers! table is empty"));
        assert!(code_actions(&uri).is_empty());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn appends_to_an_existing_typed_impl_and_keeps_crlf() {
        let dir = temp_dir("append");
        let text = "use kubuno_views::prelude::*;\r\npub struct Vm;\r\nimpl ViewModel for Vm {\r\n    fn get(&self, _: &str) -> Option<Value> { None }\r\n    fn set(&mut self, _: &str, _: Value) {}\r\n}\r\n#[kubuno_views::event_handlers]\r\nimpl Vm {\r\n    fn a(&mut self) {}\r\n}\r\npub fn t() -> HandlerTable {\r\n    handlers! { \"b\" => |vm, v| vm.set(\"B\", v) }\r\n}\r\n";
        fs::write(dir.join("view.rs"), text).unwrap();
        let uri = fs_uri::from_path(&dir.join("view.kbview")).unwrap();
        let actions = code_actions(&uri);
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].title, TITLE);
        #[allow(clippy::mutable_key_type)]
        let changes = actions[0].edit.clone().unwrap().changes.unwrap();
        let out = apply(text, &changes[&fs_uri::from_path(&dir.join("view.rs")).unwrap()]);
        assert!(out.contains("    fn a(&mut self) {}\r\n\r\n    fn b(&mut self, e: &dyn EventArgs) {\r\n        let vm = self;\r\n        let v = e.legacy_value();\r\n        vm.set(\"B\", v);\r\n    }\r\n}\r\n"), "{out:?}");
        assert!(out.contains("handlers! {}"), "{out:?}");
        assert_eq!(out.matches('\n').count(), out.matches("\r\n").count());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn nothing_to_convert_says_why() {
        let dir = temp_dir("none");
        fs::write(dir.join("v.rs"), "pub struct V;\n").unwrap();
        let uri = fs_uri::from_path(&dir.join("v.kbview")).unwrap();
        assert!(convert_handlers(&uri).reason.unwrap().contains("no handlers! table"));
        fs::write(dir.join("v.rs"), "fn t() { handlers! { \"a\" => |vm, v| {} } }\n").unwrap();
        assert!(convert_handlers(&uri).reason.unwrap().contains("impl ViewModel"));
        fs::remove_dir_all(&dir).ok();
    }
}
