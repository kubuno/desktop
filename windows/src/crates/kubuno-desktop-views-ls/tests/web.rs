//! Round trips of the web profile (`vskubuno/docs/WEB-VIEWS.md` §5, WV-7) through the real server loop: a web project
//! is written to a temporary folder (registry, view, TypeScript code-behind), the server is driven over an in-memory
//! connection like an editor, and every edit it returns is applied to the files to check the result byte for byte.
//!
//! `web_pilot_views` runs the same checks on the core's pilot user controls (`AccountMenu`, `WaffleMenu`), copied
//! from the core checkout next to this repository (`KUBUNO_CORE_FRONTEND` overrides where it is); it is skipped when
//! that checkout is absent.

use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use lsp_server::{Connection, Message, Notification, Request, RequestId, Response};
use lsp_types::{DidOpenTextDocumentParams, Position, PublishDiagnosticsParams, TextDocumentItem, Uri, WorkspaceEdit};
use serde_json::{json, Value};

fn spawn_server() -> (Connection, thread::JoinHandle<()>) {
    let (client, server) = Connection::memory();
    let handle = thread::spawn(move || {
        kubuno_desktop_views_ls::server::run(&server).expect("server loop should not error");
    });
    (client, handle)
}

struct Client {
    conn: Connection,
    handle: Option<thread::JoinHandle<()>>,
    next: i32,
}

impl Client {
    fn start() -> Self {
        let (conn, handle) = spawn_server();
        let mut c = Client { conn, handle: Some(handle), next: 1 };
        let r = c.request("initialize", json!({ "capabilities": {} }));
        assert!(r.is_object());
        c.notify("initialized", json!({}));
        c
    }

    fn notify(&self, method: &str, params: impl serde::Serialize) {
        self.conn.sender.send(Message::Notification(Notification::new(method.to_string(), params))).unwrap();
    }

    fn request(&mut self, method: &str, params: impl serde::Serialize) -> Value {
        let id = self.next;
        self.next += 1;
        self.conn.sender.send(Message::Request(Request::new(RequestId::from(id), method.to_string(), params))).unwrap();
        loop {
            match self.conn.receiver.recv_timeout(Duration::from_secs(20)).expect("no response in time") {
                Message::Response(Response { id: rid, response_result }) if rid == RequestId::from(id) => {
                    return response_result.unwrap_or_else(|e| panic!("{method} failed: {e:?}"));
                }
                _ => {}
            }
        }
    }

    fn open(&self, path: &Path) -> PublishDiagnosticsParams {
        let text = std::fs::read_to_string(path).expect("view");
        self.notify(
            "textDocument/didOpen",
            DidOpenTextDocumentParams { text_document: TextDocumentItem { uri: uri(path), language_id: "kbview".into(), version: 1, text } },
        );
        loop {
            if let Message::Notification(n) = self.conn.receiver.recv_timeout(Duration::from_secs(20)).expect("no diagnostics") {
                if n.method == "textDocument/publishDiagnostics" {
                    let p: PublishDiagnosticsParams = serde_json::from_value(n.params).expect("diagnostics");
                    if p.uri == uri(path) {
                        return p;
                    }
                }
            }
        }
    }

    fn change(&self, path: &Path, text: &str, version: i32) {
        self.notify(
            "textDocument/didChange",
            json!({ "textDocument": { "uri": uri(path).as_str(), "version": version }, "contentChanges": [{ "text": text }] }),
        );
    }

    fn completion_labels(&mut self, path: &Path, line: u32, character: u32) -> Vec<String> {
        let r = self.request("textDocument/completion", json!({ "textDocument": { "uri": uri(path).as_str() }, "position": { "line": line, "character": character } }));
        r.as_array().map(|a| a.iter().filter_map(|i| i["label"].as_str().map(str::to_string)).collect()).unwrap_or_default()
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        let id = 9999;
        let _ = self.conn.sender.send(Message::Request(Request::new(RequestId::from(id), "shutdown".into(), Value::Null)));
        let _ = self.conn.receiver.recv_timeout(Duration::from_secs(5));
        let _ = self.conn.sender.send(Message::Notification(Notification::new("exit".into(), Value::Null)));
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

fn uri(path: &Path) -> Uri {
    url::Url::from_file_path(path).expect("absolute").as_str().parse().expect("uri")
}

/// The line/UTF-16 column of the first occurrence of `needle` in `text`, plus `delta` characters.
fn pos_of(text: &str, needle: &str, delta: usize) -> Position {
    let at = text.find(needle).unwrap_or_else(|| panic!("`{needle}` not found"));
    let before = &text[..at];
    let line = before.matches('\n').count() as u32;
    let line_start = before.rfind('\n').map_or(0, |i| i + 1);
    Position { line, character: (text[line_start..at].encode_utf16().count() + delta) as u32 }
}

fn offset_of(text: &str, p: &Value) -> usize {
    let line = p["line"].as_u64().unwrap() as usize;
    let character = p["character"].as_u64().unwrap() as usize;
    let mut offset = 0;
    for (i, l) in text.split_inclusive('\n').enumerate() {
        if i == line {
            let mut units = 0;
            for (b, c) in l.char_indices() {
                if units >= character {
                    return offset + b;
                }
                units += c.len_utf16();
            }
            return offset + l.len();
        }
        offset += l.len();
    }
    offset
}

/// Applies the edits of `edit` for `path` to the file and returns the new text.
fn apply(edit: &Value, path: &Path) -> String {
    let text = std::fs::read_to_string(path).expect("file");
    let key = uri(path).as_str().to_string();
    let changes = edit["changes"].as_object().expect("changes");
    let Some((_, edits)) = changes.iter().find(|(k, _)| k.eq_ignore_ascii_case(&key)) else { return text };
    let mut spans: Vec<(usize, usize, String)> = edits
        .as_array()
        .unwrap()
        .iter()
        .map(|e| (offset_of(&text, &e["range"]["start"]), offset_of(&text, &e["range"]["end"]), e["newText"].as_str().unwrap().to_string()))
        .collect();
    spans.sort_by_key(|s| std::cmp::Reverse(s.0));
    let mut out = text;
    for (s, e, t) in spans {
        out.replace_range(s..e, &t);
    }
    std::fs::write(path, &out).expect("write");
    out
}

fn fixture_registry() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("kubuno-web-views-compiler-core").join("tests").join("fixtures").join("registry.web.json")
}

fn temp_project(tag: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("kubuno-webls-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src")).expect("dirs");
    root
}

const VIEW: &str = r#"<Stack Direction="TopDown" OnLoad="settings_load">
  <!-- The settings. -->
  <Switch x:Name="dark" On="{Binding dark, Mode=TwoWay}" OnCheckedChanged="dark_changed"/>
  <Button x:Name="save" Text="{Binding title}" Loading="{Binding busy}" OnClick="save_click"/>
  <Button Text="Cancel" Variant="Ghost" OnClick="missing_click"/>
  <MailList/>
  <Buton/>
</Stack>
"#;

const CODE: &str = r#"import { bind, type Button, type MouseEventArgs } from '@kubuno/views'
import { ViewBase } from './Settings.kbview'

export class Settings extends ViewBase {
  @bind accessor dark = false
  @bind accessor busy = false

  get title(): string {
    return 'Save'
  }

  settings_load(): void {
    this.busy = false
  }

  save_click(sender: Button, e: MouseEventArgs): void {
    this.busy = true
  }

  dark_changed(_sender: unknown, e: { value: boolean }) {}
}

export default Settings.component()
"#;

/// A web project with the fixture registry and a project registry declaring another module's control.
fn settings_project(tag: &str) -> (PathBuf, PathBuf, PathBuf) {
    let root = temp_project(tag);
    std::fs::write(
        root.join("kubuno.views.json"),
        json!({ "target": "web", "hostRegistry": fixture_registry().to_string_lossy(), "registries": ["src/controls.json"], "sources": ["src"] }).to_string(),
    )
    .expect("config");
    std::fs::write(
        root.join("src/controls.json"),
        r#"{"version":"1","components":[{"name":"MailList","kind":"control","origin":"project","family":"project","children":"None","web":{"module":"@kubuno/mail","export":"MailList","dom_root":"wrapper"}}]}"#,
    )
    .expect("controls");
    let view = root.join("src/Settings.kbview");
    let code = root.join("src/Settings.ts");
    std::fs::write(&view, VIEW).expect("view");
    std::fs::write(&code, CODE).expect("code");
    (root, view, code)
}

fn codes(d: &PublishDiagnosticsParams) -> Vec<String> {
    d.diagnostics.iter().map(|d| match &d.code {
        Some(lsp_types::NumberOrString::String(s)) => s.clone(),
        _ => String::new(),
    }).collect()
}

#[test]
fn web_diagnostics_completion_hover_definition() {
    let (root, view, code) = settings_project("lang");
    let mut c = Client::start();
    let d = c.open(&view);
    let all = codes(&d);
    // Module isolation: the project registry naming `@kubuno/mail` (the compiler's rule) and the element itself.
    let isolation: Vec<_> = d.diagnostics.iter().filter(|x| matches!(&x.code, Some(lsp_types::NumberOrString::String(s)) if s == "module-isolation")).collect();
    assert_eq!(isolation.len(), 1, "{all:?}");
    assert_eq!(isolation[0].range.start.line, 5, "at <MailList/>");
    assert!(all.contains(&"registry".to_string()), "the compiler's own isolation error: {all:?}");
    // The compiler's « did you mean ».
    let unknown = d.diagnostics.iter().find(|x| x.message.contains("unknown element `Buton`")).expect("unknown element");
    assert!(unknown.message.contains("did you mean `Button`"), "{}", unknown.message);
    // A handler the class does not have; an undeclared `x:` prefix.
    let missing = d.diagnostics.iter().find(|x| x.message.contains("missing_click")).expect("missing handler");
    assert_eq!(missing.severity, Some(lsp_types::DiagnosticSeverity::WARNING));
    assert!(all.contains(&"undeclared-prefix".to_string()), "{all:?}");
    assert!(!d.diagnostics.iter().any(|x| x.message.contains("save_click") || x.message.contains("settings_load") || x.message.contains("dark_changed")), "{:?}", d.diagnostics);

    // Completion: `<` (elements), a space (attributes), `="` (enum values).
    std::fs::write(&view, VIEW.replace("<Buton/>", "<")).expect("edit");
    let text = std::fs::read_to_string(&view).unwrap();
    c.change(&view, &text, 2);
    let p = pos_of(&text, "<\n</Stack>", 1);
    let labels = c.completion_labels(&view, p.line, p.character);
    assert!(labels.contains(&"Button".into()) && labels.contains(&"Repeater".into()) && labels.contains(&"MailList".into()), "{labels:?}");
    assert!(!labels.contains(&"Option".into()), "an item only under its parent: {labels:?}");
    let p = pos_of(&text, "Variant=\"Ghost\"", 0);
    let labels = c.completion_labels(&view, p.line, p.character);
    assert!(labels.contains(&"OnClick".into()) && labels.contains(&"Size".into()) && labels.contains(&"x:Name".into()), "{labels:?}");
    assert!(!labels.contains(&"OnLoad".into()), "a view event only on the root");
    let p = pos_of(&text, "Variant=\"Ghost\"", 9);
    let labels = c.completion_labels(&view, p.line, p.character);
    assert!(labels.contains(&"Primary".into()) && labels.contains(&"Danger".into()), "{labels:?}");

    // Hover on a handler: its TypeScript signature; on an element: its doc.
    let p = pos_of(&text, "save_click\"", 2);
    let h = c.request("textDocument/hover", json!({ "textDocument": { "uri": uri(&view).as_str() }, "position": p }));
    assert!(h["contents"]["value"].as_str().unwrap().contains("save_click(sender: Button, e: MouseEventArgs): void"), "{h}");

    // F12: handler → TS method, binding → member.
    let p = pos_of(&text, "save_click\"", 2);
    let loc = c.request("textDocument/definition", json!({ "textDocument": { "uri": uri(&view).as_str() }, "position": p }));
    let loc = &loc[0];
    assert!(loc["uri"].as_str().unwrap().to_lowercase().ends_with("settings.ts"), "{loc}");
    assert_eq!(loc["range"]["start"]["line"], 15, "save_click's line");
    let p = pos_of(&text, "{Binding title}", 10);
    let loc = c.request("textDocument/definition", json!({ "textDocument": { "uri": uri(&view).as_str() }, "position": p }));
    assert_eq!(loc[0]["range"]["start"]["line"], 7, "the getter");

    // Binding paths of the class (the cross-language completion of `{Binding `).
    let paths = c.request("kubuno/bindingPaths", json!({ "uri": uri(&view).as_str() }));
    assert_eq!(paths["paths"], json!(["dark", "busy", "title"]));

    // The generated declarations are the compiler's.
    let dts = root.join(".kubuno").join("views").join("src").join("Settings.kbview.d.ts");
    for _ in 0..40 {
        if dts.is_file() {
            break;
        }
        thread::sleep(Duration::from_millis(100));
    }
    let written = std::fs::read_to_string(&dts).expect("the .d.ts is written on open");
    assert!(written.contains("abstract save_click(sender: __Button, e: __MouseEventArgs): void | Promise<void>"), "{written}");
    assert!(root.join(".kubuno/views/src/Settings.kbview.check.ts").is_file());
    assert!(root.join(".kubuno/views/src/Settings.kbview.check.json").is_file());
    let _ = code;
    drop(c);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn web_handlers_create_rename_remove() {
    let (root, view, code) = settings_project("handlers");
    let mut c = Client::start();
    c.open(&view);
    let u = uri(&view);
    // ⚡ dropdown of the Switch's OnCheckedChanged: only the methods that can take it.
    let compatible = c.request("kubuno/compatibleHandlers", json!({ "uri": u.as_str(), "elementId": "0", "event": "OnCheckedChanged" }));
    let names: Vec<&str> = compatible["handlers"].as_array().unwrap().iter().map(|v| v.as_str().unwrap()).collect();
    assert_eq!(names, vec!["settings_load", "dark_changed"], "a Button sender or MouseEventArgs does not fit a Switch's change");
    let compatible = c.request("kubuno/compatibleHandlers", json!({ "uri": u.as_str(), "elementId": "1", "event": "OnClick" }));
    assert_eq!(compatible["handlers"], json!(["settings_load", "save_click", "dark_changed"]));

    // ⚡ double-click on the « Cancel » button's OnDoubleClick: a new method, the attribute, the import.
    let created = c.request("kubuno/createHandler", json!({ "uri": u.as_str(), "elementId": "2", "event": "OnDoubleClick" }));
    assert_eq!(created["handlerName"], "button_double_click");
    let new_code = apply(&created["edit"], &code);
    let new_view = apply(&created["edit"], &view);
    assert!(new_view.contains(r#"<Button Text="Cancel" Variant="Ghost" OnClick="missing_click" OnDoubleClick="button_double_click"/>"#), "{new_view}");
    let expected = CODE.replace(
        "  dark_changed(_sender: unknown, e: { value: boolean }) {}\n}",
        "  dark_changed(_sender: unknown, e: { value: boolean }) {}\n\n  button_double_click(sender: Button, e: MouseEventArgs): void {\n    // TODO: implement button_double_click\n  }\n}",
    );
    assert_eq!(new_code, expected, "inserted, nothing else touched (the types were already imported)");
    c.change(&view, &new_view, 2);

    // Again on the same event: no new code, the location of the method.
    let again = c.request("kubuno/createHandler", json!({ "uri": u.as_str(), "elementId": "2", "event": "OnDoubleClick" }));
    assert!(again["edit"].is_null() && again["location"]["range"]["start"]["line"] == 21, "{again}");

    // The root's view event names after the file stem; a type missing from the import is added to it.
    let created = c.request("kubuno/createHandler", json!({ "uri": u.as_str(), "elementId": "", "event": "OnShown" }));
    assert_eq!(created["handlerName"], "settings_shown");
    let code_text = apply(&created["edit"], &code);
    assert!(code_text.starts_with("import { bind, type Button, type MouseEventArgs, type Stack, type EventArgs } from '@kubuno/views'\n"), "{code_text}");
    assert!(code_text.contains("  settings_shown(sender: Stack, e: EventArgs): void {\n    // TODO: implement settings_shown\n  }\n}\n\nexport default"), "{code_text}");
    let view_text = apply(&created["edit"], &view);
    c.change(&view, &view_text, 3);

    // Rename everywhere: the attribute, the method, `this.` calls.
    let renamed = c.request("kubuno/renameHandler", json!({ "uri": u.as_str(), "old": "save_click", "new": "save_now" }));
    assert_eq!(renamed["oldName"], "save_click");
    let code_text = apply(&renamed["edit"], &code);
    let view_text = apply(&renamed["edit"], &view);
    assert!(code_text.contains("  save_now(sender: Button, e: MouseEventArgs): void {") && !code_text.contains("save_click"));
    assert!(view_text.contains("OnClick=\"save_now\""));
    c.change(&view, &view_text, 4);
    let taken = c.request("kubuno/renameHandler", json!({ "uri": u.as_str(), "old": "save_now", "new": "busy" }));
    assert!(taken["reason"].as_str().unwrap().contains("already has"), "{taken}");

    // Clear the event: the untouched stub goes too; a method with code stays.
    let removed = c.request("kubuno/removeHandler", json!({ "uri": u.as_str(), "elementId": "2", "event": "OnDoubleClick" }));
    assert_eq!(removed["removedStub"], true);
    let code_text = apply(&removed["edit"], &code);
    let view_text = apply(&removed["edit"], &view);
    assert!(!code_text.contains("button_double_click") && !view_text.contains("button_double_click"), "{code_text}");
    assert!(code_text.contains("dark_changed(_sender: unknown, e: { value: boolean }) {}\n\n  settings_shown("), "one blank line kept: {code_text}");
    c.change(&view, &view_text, 5);
    let kept = c.request("kubuno/removeHandler", json!({ "uri": u.as_str(), "elementId": "1", "event": "OnClick" }));
    assert_eq!(kept["removedStub"], false);
    assert_eq!(kept["handlerName"], "save_now");

    // The quick fix « Create handler » on a missing one.
    let d = c.request("textDocument/codeAction", json!({ "textDocument": { "uri": u.as_str() }, "range": { "start": pos_of(&view_text, "missing_click", 1), "end": pos_of(&view_text, "missing_click", 1) }, "context": { "diagnostics": [] } }));
    let titles: Vec<&str> = d.as_array().unwrap().iter().map(|a| a["title"].as_str().unwrap()).collect();
    assert!(titles.contains(&"Create handler `missing_click`"), "{titles:?}");
    drop(c);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_view_without_web_markers_keeps_the_desktop_profile() {
    let root = temp_project("desktop");
    let view = root.join("src/main_view.kbview");
    std::fs::write(&view, "<Panel><Button Text=\"Ok\" Colour=\"red\"/></Panel>\n").expect("view");
    let c = Client::start();
    let d = c.open(&view);
    assert!(d.diagnostics.iter().any(|x| x.message.contains("unknown attribute `Colour`")), "the desktop validator: {:?}", d.diagnostics);
    assert!(!root.join(".kubuno").exists(), "nothing generated for a desktop view");
    drop(c);
    let _ = std::fs::remove_dir_all(&root);
}

/// The core checkout's frontend, when present.
fn core_frontend() -> Option<PathBuf> {
    let dir = std::env::var_os("KUBUNO_CORE_FRONTEND").map(PathBuf::from).unwrap_or_else(|| {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..").join("..").join("..").join("..").join("core").join("frontend")
    });
    dir.join("src/core/shell/menus/AccountMenu.kbcontrol").is_file().then_some(dir)
}

fn copy(from: &Path, to: &Path) {
    std::fs::create_dir_all(to.parent().unwrap()).unwrap();
    std::fs::copy(from, to).unwrap_or_else(|e| panic!("{}: {e}", from.display()));
}

#[test]
fn web_pilot_views() {
    let Some(core) = core_frontend() else {
        eprintln!("skipped: no core checkout next to this repository (set KUBUNO_CORE_FRONTEND)");
        return;
    };
    let root = temp_project("pilot");
    let menus = "src/core/shell/menus";
    for f in ["AccountMenu.kbcontrol", "AccountMenu.ts", "WaffleMenu.kbcontrol", "WaffleMenu.ts", "model.ts", "kbview-controls.json"] {
        copy(&core.join(menus).join(f), &root.join(menus).join(f));
    }
    copy(&core.join("packages/ui/kbview-registry.web.json"), &root.join("packages/ui/kbview-registry.web.json"));
    copy(&core.join("kubuno.views.json"), &root.join("kubuno.views.json"));
    copy(&core.join("tsconfig.app.json"), &root.join("tsconfig.app.json"));
    for lang in ["en", "fr"] {
        let bundle = format!("src/core/i18n/locales/{lang}/core.json");
        copy(&core.join(&bundle), &root.join(&bundle));
    }
    let account = root.join(menus).join("AccountMenu.kbcontrol");
    let waffle = root.join(menus).join("WaffleMenu.kbcontrol");
    let account_code = root.join(menus).join("AccountMenu.ts");

    let mut c = Client::start();
    for v in [&account, &waffle] {
        let d = c.open(v);
        if std::env::var_os("KUBUNO_WEBLS_PRINT").is_some() {
            for x in &d.diagnostics {
                eprintln!("{}:{}:{} {:?} {:?} {}", v.file_name().unwrap().to_string_lossy(), x.range.start.line + 1, x.range.start.character + 1, x.severity, x.code, x.message);
            }
        }
        let errors: Vec<_> = d.diagnostics.iter().filter(|x| x.severity == Some(lsp_types::DiagnosticSeverity::ERROR)).collect();
        assert!(errors.is_empty(), "{}: {errors:?}", v.display());
        assert!(!d.diagnostics.iter().any(|x| matches!(&x.code, Some(lsp_types::NumberOrString::String(s)) if s == "missing-handler" || s == "incompatible-handler" || s == "binding-member" || s == "unknown-resource")), "{:?}", d.diagnostics);
    }
    let d = c.open(&account);
    assert!(codes(&d).contains(&"web-class".to_string()), "the Class budget is reported");

    let text = std::fs::read_to_string(&account).unwrap();
    let u = uri(&account);
    // Element, attribute and value completion at the places VS triggers it.
    let p = pos_of(&text, "<Avatar Image=\"{Binding user.avatar}\" DisplayName", 8);
    let labels = c.completion_labels(&account, p.line, p.character);
    assert!(labels.contains(&"AvatarSize".into()) && labels.contains(&"OnClick".into()), "{labels:?}");
    let p = pos_of(&text, "Tint=\"Accent\"", 6);
    let labels = c.completion_labels(&account, p.line, p.character);
    assert!(labels.len() > 1 && labels.contains(&"Accent".into()), "{labels:?}");
    let p = pos_of(&text, "{Res shell.change_photo}", 5);
    let labels = c.completion_labels(&account, p.line, p.character);
    assert!(labels.contains(&"shell.change_photo".into()) && labels.contains(&"shell.add_account".into()), "the core's i18n keys: {}", labels.len());

    // The camera button: IconButton handlers, ElementHandle ones too.
    let id = c.request("kubuno/elementAtOffset", json!({ "uri": u.as_str(), "position": pos_of(&text, "x:Name=\"camera_button\"", 0) }));
    let element_id = id["elementId"].as_str().unwrap().to_string();
    let compatible = c.request("kubuno/compatibleHandlers", json!({ "uri": u.as_str(), "elementId": element_id, "event": "OnClick" }));
    let names: Vec<&str> = compatible["handlers"].as_array().unwrap().iter().map(|v| v.as_str().unwrap()).collect();
    assert!(names.contains(&"camera_button_click") && names.contains(&"close_button_click") && names.contains(&"account_click"), "{names:?}");
    assert!(names.contains(&"escape"), "a method without parameters fits any event");

    // F12 on `OnClick="camera_button_click"`.
    let p = pos_of(&text, "camera_button_click\"", 3);
    let loc = c.request("textDocument/definition", json!({ "textDocument": { "uri": u.as_str() }, "position": p }));
    let code_text = std::fs::read_to_string(&account_code).unwrap();
    assert_eq!(loc[0]["range"]["start"], serde_json::to_value(pos_of(&code_text, "camera_button_click(_sender", 0)).unwrap());

    // ⚡ on the hero's Avatar OnClick: `avatar_click(_sender: Avatar, _e: MouseEventArgs)` (noUnusedParameters).
    let id = c.request("kubuno/elementAtOffset", json!({ "uri": u.as_str(), "position": pos_of(&text, "AvatarSize=\"96\"", 0) }));
    let element_id = id["elementId"].as_str().unwrap().to_string();
    let created = c.request("kubuno/createHandler", json!({ "uri": u.as_str(), "elementId": element_id, "event": "OnClick" }));
    assert_eq!(created["handlerName"], "avatar_click");
    let new_code = apply(&created["edit"], &account_code);
    assert!(new_code.contains("import { bind, type ElementHandle, type IconButton, type MouseEventArgs, type Avatar } from '@kubuno/views'"), "{new_code}");
    assert!(new_code.contains("  sign_out_click(_sender: ElementHandle): void {\n    this.props.onSignOut?.()\n  }\n\n  avatar_click(_sender: Avatar, _e: MouseEventArgs): void {\n    // TODO: implement avatar_click\n  }\n}\n"), "{new_code}");
    assert_eq!(new_code.len() - code_text.len(), "\n  avatar_click(_sender: Avatar, _e: MouseEventArgs): void {\n    // TODO: implement avatar_click\n  }\n".len() + ", type Avatar".len(), "insertions only");
    drop(c);
    let _ = std::fs::remove_dir_all(&root);
    let _: Option<WorkspaceEdit> = None;
}
