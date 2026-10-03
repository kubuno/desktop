//! In-process LSP round-trip harness: `initialize` → `initialized` →
//! `didOpen` → `completion`/`hover`/diagnostics → `shutdown`/`exit`, all over
//! an in-memory [`lsp_server::Connection::memory`] pair — no child process,
//! no real stdio, the whole exchange driven and observed inside one test
//! process. This is exactly what `Cargo.toml`'s doc comment on the
//! `lsp-server` dependency choice promised: "`Connection::memory()` is what
//! makes the in-process round-trip test harness … straightforward".

use std::thread;
use std::time::Duration;

use lsp_server::{Connection, Message, Notification, Request, RequestId, Response};
use lsp_types::{
    CompletionParams, CompletionResponse, DidOpenTextDocumentParams, HoverContents, HoverParams,
    PartialResultParams, Position, PublishDiagnosticsParams, TextDocumentIdentifier, TextDocumentItem,
    TextDocumentPositionParams, Uri, WorkDoneProgressParams,
};

/// The view registry is process-wide and every server declares its project's controls into it
/// (EVT-7b): each test holds this lock so one server's scan never changes another's registry.
static REGISTRY_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn serial() -> std::sync::MutexGuard<'static, ()> {
    REGISTRY_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Spawns the server on one end of an in-memory connection, returns the
/// client-side [`Connection`] plus a join handle. The caller drives the
/// client side exactly like a real editor would, then must send
/// `shutdown`/`exit` and join the handle to avoid leaking the server thread.
fn spawn_server() -> (Connection, thread::JoinHandle<()>) {
    let (client, server) = Connection::memory();
    let handle = thread::spawn(move || {
        kubuno_desktop_views_ls::server::run(&server).expect("server loop should not error");
    });
    (client, handle)
}

fn send_request(client: &Connection, id: i32, method: &str, params: impl serde::Serialize) {
    client.sender.send(Message::Request(Request::new(RequestId::from(id), method.to_string(), params))).unwrap();
}

fn send_notification(client: &Connection, method: &str, params: impl serde::Serialize) {
    client.sender.send(Message::Notification(Notification::new(method.to_string(), params))).unwrap();
}

/// Reads messages off `client` until a [`Response`] with the given `id`
/// arrives, returning it. Notifications received while waiting (e.g.
/// `publishDiagnostics` racing a subsequent request) are handed to
/// `on_notification` rather than discarded, so a caller can still observe
/// them without a second, separate receive loop.
fn recv_response(client: &Connection, id: i32, mut on_notification: impl FnMut(Notification)) -> Response {
    loop {
        let msg = client.receiver.recv_timeout(Duration::from_secs(10)).expect("server did not respond in time");
        match msg {
            Message::Response(resp) if resp.id == RequestId::from(id) => return resp,
            Message::Notification(n) => on_notification(n),
            other => panic!("unexpected message while waiting for response {id}: {other:?}"),
        }
    }
}

/// Waits for a `textDocument/publishDiagnostics` notification, ignoring any
/// other notification in between (there should be none in this harness, but
/// being lenient here keeps the test robust to ordering).
fn recv_diagnostics(client: &Connection) -> PublishDiagnosticsParams {
    loop {
        let msg = client.receiver.recv_timeout(Duration::from_secs(10)).expect("no diagnostics notification arrived");
        if let Message::Notification(n) = msg {
            if n.method == "textDocument/publishDiagnostics" {
                return serde_json::from_value(n.params).expect("valid PublishDiagnosticsParams");
            }
        }
    }
}

fn initialize(client: &Connection) {
    send_request(client, 1, "initialize", serde_json::json!({ "capabilities": {} }));
    let resp = recv_response(client, 1, |_| {});
    assert!(resp.response_result.is_ok(), "initialize failed: {:?}", resp.response_result);
    send_notification(client, "initialized", serde_json::json!({}));
}

fn shutdown(client: &Connection, handle: thread::JoinHandle<()>) {
    send_request(client, 999, "shutdown", serde_json::json!(null));
    let resp = recv_response(client, 999, |_| {});
    assert!(resp.response_result.is_ok());
    send_notification(client, "exit", serde_json::json!(null));
    handle.join().expect("server thread panicked");
}

fn url() -> Uri {
    "file:///settings_view.kbview".parse().unwrap()
}

fn did_open(client: &Connection, text: &str) -> PublishDiagnosticsParams {
    send_notification(
        client,
        "textDocument/didOpen",
        DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: url(),
                language_id: "kbview".to_string(),
                version: 1,
                text: text.to_string(),
            },
        },
    );
    recv_diagnostics(client)
}

#[test]
fn full_round_trip_initialize_open_complete_hover_diagnostics() {
    let _serial = serial();
    let (client, handle) = spawn_server();
    initialize(&client);

    // A view with one deliberate mistake (`Colour` is not a real attribute)
    // so this single round trip exercises diagnostics, hover and completion
    // together on the same small document.
    let text = r#"<Card Title="Réglages"><Button Text="Ok" Colour="red"/></Card>"#;
    let diagnostics = did_open(&client, text);
    assert_eq!(diagnostics.uri, url());
    assert_eq!(diagnostics.diagnostics.len(), 1, "{:?}", diagnostics.diagnostics);
    assert!(diagnostics.diagnostics[0].message.contains("unknown attribute `Colour`"));

    // Hover over `Button`'s name (character 24 is the element's own `<`,
    // "Button" spans characters 24..30) should return the component's doc
    // from the registry.
    send_request(
        &client,
        2,
        "textDocument/hover",
        HoverParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: url() },
                position: Position { line: 0, character: 27 }, // Inside "Button".
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
        },
    );
    let hover_resp = recv_response(&client, 2, |_| {});
    let hover: Option<lsp_types::Hover> =
        serde_json::from_value(hover_resp.response_result.expect("hover result")).unwrap();
    let hover = hover.expect("hover over an element name should return something");
    match hover.contents {
        HoverContents::Markup(m) => assert!(m.value.contains("push button")),
        other => panic!("unexpected hover contents: {other:?}"),
    }

    // Completion right after "<Button " (character 30 is the space, 31 is
    // the gap before "Text") should offer the component's real properties.
    send_request(
        &client,
        3,
        "textDocument/completion",
        CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: url() },
                position: Position { line: 0, character: 31 }, // Just after "<Button ".
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        },
    );
    let completion_resp = recv_response(&client, 3, |_| {});
    let completion: CompletionResponse =
        serde_json::from_value(completion_resp.response_result.expect("completion result")).unwrap();
    let items = match completion {
        CompletionResponse::Array(items) => items,
        CompletionResponse::List(list) => list.items,
    };
    let labels: Vec<_> = items.iter().map(|i| i.label.as_str()).collect();
    assert!(labels.contains(&"Variant"), "{labels:?}");
    assert!(labels.contains(&"Dock"), "{labels:?}");
    assert!(labels.contains(&"x:Name"), "{labels:?}");

    shutdown(&client, handle);
}

#[test]
fn document_symbol_round_trip() {
    let _serial = serial();
    let (client, handle) = spawn_server();
    initialize(&client);
    did_open(&client, r#"<Stack><Switch x:Name="notifications" On="true"/></Stack>"#);

    send_request(
        &client,
        2,
        "textDocument/documentSymbol",
        lsp_types::DocumentSymbolParams {
            text_document: TextDocumentIdentifier { uri: url() },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        },
    );
    let resp = recv_response(&client, 2, |_| {});
    let symbols: lsp_types::DocumentSymbolResponse =
        serde_json::from_value(resp.response_result.expect("documentSymbol result")).unwrap();
    let lsp_types::DocumentSymbolResponse::Nested(symbols) = symbols else { panic!("expected nested symbols") };
    assert_eq!(symbols.len(), 1);
    assert_eq!(symbols[0].name, "Stack");
    let children = symbols[0].children.as_ref().unwrap();
    assert_eq!(children[0].name, "notifications");

    shutdown(&client, handle);
}

#[test]
fn well_formed_document_publishes_no_diagnostics() {
    let _serial = serial();
    let (client, handle) = spawn_server();
    initialize(&client);
    let diagnostics = did_open(&client, r#"<Button Text="Ok" Variant="Primary"/>"#);
    assert!(diagnostics.diagnostics.is_empty(), "{:?}", diagnostics.diagnostics);
    shutdown(&client, handle);
}

#[test]
fn apply_edit_set_attribute_round_trip() {
    let _serial = serial();
    let (client, handle) = spawn_server();
    initialize(&client);
    did_open(&client, r#"<Stack><Button Text="Ok"/></Stack>"#);

    send_request(
        &client,
        2,
        "kubuno/applyEdit",
        serde_json::json!({
            "uri": url(),
            "op": { "kind": "setAttribute", "elementId": "0", "name": "Text", "value": "Annuler" }
        }),
    );
    let resp = recv_response(&client, 2, |_| {});
    let result = resp.response_result.expect("applyEdit result");
    let edits = result["edits"].as_array().expect("edits array");
    assert_eq!(edits.len(), 1, "{result:?}");
    assert_eq!(edits[0]["newText"], "Annuler");
    assert_eq!(edits[0]["range"]["start"]["line"], 0);

    shutdown(&client, handle);
}

#[test]
fn apply_edit_insert_child_round_trip() {
    let _serial = serial();
    let (client, handle) = spawn_server();
    initialize(&client);
    did_open(&client, r#"<Stack><A/></Stack>"#);

    send_request(
        &client,
        2,
        "kubuno/applyEdit",
        serde_json::json!({
            "uri": url(),
            "op": { "kind": "insertChild", "parentId": "", "index": 1, "xml": "<B/>" }
        }),
    );
    let resp = recv_response(&client, 2, |_| {});
    let result = resp.response_result.expect("applyEdit result");
    let edits = result["edits"].as_array().expect("edits array");
    assert_eq!(edits.len(), 1, "{result:?}");
    assert_eq!(edits[0]["newText"], "<B/>");

    shutdown(&client, handle);
}

#[test]
fn apply_edit_insert_fragment_round_trip() {
    let _serial = serial();
    let (client, handle) = spawn_server();
    initialize(&client);
    did_open(&client, "<Stack>\n  <Button x:Name=\"go\"/>\n</Stack>");

    send_request(
        &client,
        2,
        "kubuno/applyEdit",
        serde_json::json!({
            "uri": url(),
            "op": { "kind": "insertFragment", "parentId": "", "index": 1, "xml": "<Button x:Name=\"go\"/>" }
        }),
    );
    let resp = recv_response(&client, 2, |_| {});
    let result = resp.response_result.expect("applyEdit result");
    let edits = result["edits"].as_array().expect("edits array");
    assert_eq!(edits.len(), 1, "{result:?}");
    assert_eq!(edits[0]["newText"], "\n  <Button x:Name=\"go2\"/>");

    shutdown(&client, handle);
}

#[test]
fn apply_edit_wrap_element_round_trip() {
    let _serial = serial();
    let (client, handle) = spawn_server();
    initialize(&client);
    did_open(&client, "<Card>\n  <Stack/>\n</Card>");

    send_request(
        &client,
        2,
        "kubuno/applyEdit",
        serde_json::json!({
            "uri": url(),
            "op": { "kind": "wrapElement", "elementId": "0", "wrapper": "GroupBox" }
        }),
    );
    let resp = recv_response(&client, 2, |_| {});
    let result = resp.response_result.expect("applyEdit result");
    let edits = result["edits"].as_array().expect("edits array");
    assert_eq!(edits.len(), 1, "{result:?}");
    assert_eq!(edits[0]["newText"], "<GroupBox>\n    <Stack/>\n  </GroupBox>");

    shutdown(&client, handle);
}

#[test]
fn apply_edit_move_element_across_parents_round_trip() {
    let _serial = serial();
    let (client, handle) = spawn_server();
    initialize(&client);
    did_open(
        &client,
        r#"<Stack><Panel x:Name="left"><A/></Panel><Panel x:Name="right"></Panel></Stack>"#,
    );

    send_request(
        &client,
        2,
        "kubuno/applyEdit",
        serde_json::json!({
            "uri": url(),
            "op": { "kind": "moveElement", "elementId": "0.0", "newParentId": "1", "index": 0 }
        }),
    );
    let resp = recv_response(&client, 2, |_| {});
    let result = resp.response_result.expect("applyEdit result");
    let edits = result["edits"].as_array().expect("edits array");
    assert_eq!(edits.len(), 2, "{result:?}"); // Delete from the old parent, insert into the new one.

    shutdown(&client, handle);
}

#[test]
fn apply_edit_on_unknown_element_id_returns_no_edits() {
    let _serial = serial();
    let (client, handle) = spawn_server();
    initialize(&client);
    did_open(&client, r#"<Button Text="Ok"/>"#);

    send_request(
        &client,
        2,
        "kubuno/applyEdit",
        serde_json::json!({
            "uri": url(),
            "op": { "kind": "removeElement", "elementId": "9.9" }
        }),
    );
    let resp = recv_response(&client, 2, |_| {});
    let result = resp.response_result.expect("applyEdit result");
    assert!(result["edits"].as_array().expect("edits array").is_empty(), "{result:?}");

    shutdown(&client, handle);
}

#[test]
fn element_at_offset_and_range_of_element_round_trip() {
    let _serial = serial();
    let (client, handle) = spawn_server();
    initialize(&client);
    did_open(&client, r#"<Stack><A/><Button Text="Ok"/></Stack>"#);

    // Character 25 sits inside `Text="Ok"`'s value, within `Button` (id "1").
    send_request(
        &client,
        2,
        "kubuno/elementAtOffset",
        serde_json::json!({ "uri": url(), "position": { "line": 0, "character": 25 } }),
    );
    let resp = recv_response(&client, 2, |_| {});
    let found = resp.response_result.expect("elementAtOffset result");
    assert_eq!(found["elementId"], "1", "{found:?}");

    send_request(
        &client,
        3,
        "kubuno/rangeOfElement",
        serde_json::json!({ "uri": url(), "elementId": "1" }),
    );
    let resp2 = recv_response(&client, 3, |_| {});
    let range_result = resp2.response_result.expect("rangeOfElement result");
    assert_eq!(range_result["range"], found["range"], "{range_result:?} vs {found:?}");

    shutdown(&client, handle);
}

#[test]
fn range_of_element_on_unknown_id_returns_null() {
    let _serial = serial();
    let (client, handle) = spawn_server();
    initialize(&client);
    did_open(&client, r#"<Button Text="Ok"/>"#);

    send_request(&client, 2, "kubuno/rangeOfElement", serde_json::json!({ "uri": url(), "elementId": "3.3" }));
    let resp = recv_response(&client, 2, |_| {});
    let result = resp.response_result.expect("rangeOfElement result");
    assert!(result.is_null(), "{result:?}");

    shutdown(&client, handle);
}

#[test]
fn registry_round_trip() {
    let _serial = serial();
    // Work package DSG-1 (`vskubuno/docs/DESIGNER.md` §5/§8): `kubuno/registry`
    // needs no open document, so this exercises it straight after
    // `initialize`, before any `didOpen` — the same way the VS designer's
    // toolbox would query it once at editor start.
    let (client, handle) = spawn_server();
    initialize(&client);

    send_request(&client, 2, "kubuno/registry", serde_json::json!(null));
    let resp = recv_response(&client, 2, |_| {});
    let result = resp.response_result.expect("kubuno/registry result");

    let version = result["version"].as_str().expect("a version string");
    assert!(!version.is_empty());

    let components = result["components"].as_array().expect("a components array");
    assert!(!components.is_empty());

    let button = components
        .iter()
        .find(|c| c["name"] == "Button")
        .expect("Button should be in the exported registry");
    assert_eq!(button["family"], "core");
    assert_eq!(button["children"], "None");
    assert!(button["layout_kind"].is_null());
    let variant = button["properties"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["name"] == "Variant")
        .expect("Button should expose Variant");
    assert!(variant["kind"]["Enum"].as_array().is_some(), "{variant:?}");

    // A second call against the same server returns the same version: the
    // registry only changes when the project's own controls do (EVT-7b), and none are open here.
    send_request(&client, 3, "kubuno/registry", serde_json::json!(null));
    let resp2 = recv_response(&client, 3, |_| {});
    let result2 = resp2.response_result.expect("kubuno/registry result");
    assert_eq!(result2["version"], result["version"]);

    shutdown(&client, handle);
}

#[test]
fn unknown_method_returns_a_method_not_found_error_instead_of_hanging() {
    let _serial = serial();
    let (client, handle) = spawn_server();
    initialize(&client);
    send_request(&client, 42, "workspace/frobnicate", serde_json::json!({}));
    let resp = recv_response(&client, 42, |_| {});
    assert!(resp.response_result.is_err(), "expected an error response for an unhandled method");
    shutdown(&client, handle);
}

// ── `kubuno/createHandler` (DSG-10, `vskubuno/docs/DESIGNER.md`) ─────────
//
// Unlike every other round trip above, this needs a *real* directory on disk:
// `handler_insert` locates and reads the code-behind `.rs` file the same way
// `definition::goto_definition` does (a plain sibling-directory scan), so the
// `.kbview` URI opened here must resolve to a real path with a real sibling
// file next to it.

/// Creates a fresh temp directory (`kubuno-views-ls-roundtrip-<tag>-<pid>`),
/// returning it alongside the `file://` URI for `<dir>/<kbview_name>` (which
/// need not itself exist on disk — only its directory and siblings do).
fn temp_kbview(tag: &str, kbview_name: &str) -> (std::path::PathBuf, Uri) {
    let dir = std::env::temp_dir().join(format!("kubuno-views-ls-roundtrip-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    let uri_string = url::Url::from_file_path(dir.join(kbview_name)).expect("a valid file URL").to_string();
    (dir, uri_string.parse().expect("a valid lsp Uri"))
}

fn did_open_at(client: &Connection, uri: &Uri, text: &str) -> PublishDiagnosticsParams {
    send_notification(
        client,
        "textDocument/didOpen",
        DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "kbview".to_string(),
                version: 1,
                text: text.to_string(),
            },
        },
    );
    recv_diagnostics(client)
}

#[test]
fn create_handler_round_trip_inserts_fn_stub_and_table_entry() {
    let _serial = serial();
    let (dir, uri) = temp_kbview("create", "settings_view.kbview");
    std::fs::write(
        dir.join("settings_view.rs"),
        "pub struct SettingsView;\n\nfn handlers() -> HandlerTable {\n    kubuno_desktop_views::handlers! {\n        \"existing\" => |vm, v| { let _ = (vm, v); },\n    }\n}\n",
    )
    .expect("write code-behind fixture");

    let (client, handle) = spawn_server();
    initialize(&client);
    did_open_at(&client, &uri, r#"<Button Text="Save"/>"#);

    send_request(
        &client,
        2,
        "kubuno/createHandler",
        serde_json::json!({ "uri": uri, "elementId": "", "event": "OnClick" }),
    );
    let resp = recv_response(&client, 2, |_| {});
    let result = resp.response_result.expect("createHandler result");

    assert_eq!(result["handlerName"], "on_button_click", "{result:?}");
    assert!(result["location"].is_null(), "{result:?}");
    let changes = result["edit"]["changes"].as_object().expect("a changes map");
    assert_eq!(changes.len(), 2, "{changes:?}");

    let kbview_edits = changes.get(uri.as_str()).expect("an edit for the .kbview file").as_array().unwrap();
    assert_eq!(kbview_edits.len(), 1);
    assert_eq!(kbview_edits[0]["newText"], " OnClick=\"on_button_click\"");

    let rs_uri = url::Url::from_file_path(dir.join("settings_view.rs")).unwrap().to_string();
    let rs_edits = changes.get(&rs_uri).expect("an edit for the code-behind file").as_array().unwrap();
    assert_eq!(rs_edits.len(), 2, "{rs_edits:?}");
    assert!(rs_edits[0]["newText"].as_str().unwrap().contains("fn on_button_click("));
    assert!(rs_edits[1]["newText"].as_str().unwrap().contains("\"on_button_click\" => |vm, value| on_button_click(vm, value),"));

    shutdown(&client, handle);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn create_handler_on_an_already_wired_event_returns_only_a_location() {
    let _serial = serial();
    let (dir, uri) = temp_kbview("existing", "settings_view.kbview");
    std::fs::write(
        dir.join("settings_view.rs"),
        "fn offline_toggled(vm: &mut dyn kubuno_desktop_views::binding::ViewModel, value: kubuno_desktop_views::binding::Value) {}\n",
    )
    .expect("write code-behind fixture");

    let (client, handle) = spawn_server();
    initialize(&client);
    did_open_at(&client, &uri, r#"<Switch OnToggled="offline_toggled"/>"#);

    send_request(
        &client,
        2,
        "kubuno/createHandler",
        serde_json::json!({ "uri": uri, "elementId": "", "event": "OnToggled" }),
    );
    let resp = recv_response(&client, 2, |_| {});
    let result = resp.response_result.expect("createHandler result");

    assert_eq!(result["handlerName"], "offline_toggled", "{result:?}");
    assert!(result["edit"].is_null(), "{result:?}");
    assert!(!result["location"].is_null(), "{result:?}");

    shutdown(&client, handle);
    std::fs::remove_dir_all(&dir).ok();
}

/// EVT-4: a typed code-behind gets a typed method from `kubuno/createHandler`, and a legacy one
/// is offered the "convert to typed handlers" code action (also as `kubuno/convertHandlers`).
#[test]
fn typed_create_handler_and_convert_code_action_round_trip() {
    let _serial = serial();
    let (dir, uri) = temp_kbview("typed", "main_view.kbview");
    std::fs::write(
        dir.join("main_view.rs"),
        "use kubuno_desktop_views::prelude::*;\n\npub struct Vm;\n\n#[kubuno_desktop_views::event_handlers]\nimpl Vm {\n}\n",
    )
    .expect("write typed code-behind");

    let (client, handle) = spawn_server();
    initialize(&client);
    did_open_at(&client, &uri, r#"<Panel><Button x:Name="hello" Text="Hi"/></Panel>"#);

    send_request(&client, 2, "kubuno/createHandler", serde_json::json!({ "uri": uri, "elementId": "0", "event": "OnClick" }));
    let result = recv_response(&client, 2, |_| {}).response_result.expect("createHandler result");
    assert_eq!(result["handlerName"], "on_hello_click", "{result:?}");
    let edits = result["edit"]["changes"].as_object().expect("changes");
    let rs_edit = edits.iter().find(|(k, _)| k.ends_with("main_view.rs")).expect("code-behind edit").1;
    assert!(
        rs_edit[0]["newText"].as_str().unwrap().contains("fn on_hello_click(&mut self, sender: &Sender<Button>, e: &MouseEventArgs) {"),
        "{rs_edit:?}"
    );

    // No legacy table: no code action.
    send_request(
        &client,
        3,
        "textDocument/codeAction",
        serde_json::json!({ "textDocument": { "uri": uri }, "range": { "start": { "line": 0, "character": 0 }, "end": { "line": 0, "character": 0 } }, "context": { "diagnostics": [] } }),
    );
    assert!(recv_response(&client, 3, |_| {}).response_result.expect("codeAction result").is_null());

    std::fs::write(
        dir.join("main_view.rs"),
        "use kubuno_desktop_views::binding::{HandlerTable, Value, ViewModel};\nuse kubuno_desktop_views::handlers;\n\npub struct Vm;\n\nimpl ViewModel for Vm {\n    fn get(&self, _: &str) -> Option<Value> { None }\n    fn set(&mut self, _: &str, _: Value) {}\n}\n\npub fn handler_table() -> HandlerTable {\n    handlers! {\n        \"hi\" => |vm, _v| { vm.set(\"S\", Value::Bool(true)); },\n    }\n}\n",
    )
    .expect("write legacy code-behind");
    send_request(
        &client,
        4,
        "textDocument/codeAction",
        serde_json::json!({ "textDocument": { "uri": uri }, "range": { "start": { "line": 0, "character": 0 }, "end": { "line": 0, "character": 0 } }, "context": { "diagnostics": [] } }),
    );
    let actions = recv_response(&client, 4, |_| {}).response_result.expect("codeAction result");
    assert_eq!(actions[0]["title"], "Convert the handlers! table to typed handlers", "{actions:?}");
    assert_eq!(actions[0]["kind"], "refactor.rewrite");
    assert!(actions[0]["edit"]["changes"].is_object());

    send_request(&client, 5, "kubuno/convertHandlers", serde_json::json!({ "uri": uri }));
    let converted = recv_response(&client, 5, |_| {}).response_result.expect("convertHandlers result");
    assert_eq!(converted["converted"], serde_json::json!(["hi"]), "{converted:?}");

    shutdown(&client, handle);
    std::fs::remove_dir_all(&dir).ok();
}

/// EVT-5 (`vskubuno/docs/EVENTS.md` §5.3/§5.5): the ⚡ dropdown, rename (by name, and after
/// rust-analyzer renamed the Rust side), clearing an event, the missing-handler warning and its
/// quick fix, F2 in the XML, and the warning following a code-behind saved on disk.
#[test]
fn handler_commands_round_trip() {
    let _serial = serial();
    let (dir, uri) = temp_kbview("evt5", "main_view.kbview");
    let rs_path = dir.join("main_view.rs");
    let rs_uri = url::Url::from_file_path(&rs_path).unwrap().to_string();
    let code = "use kubuno_desktop_views::prelude::*;\n\npub struct Vm;\n\n#[kubuno_desktop_views::event_handlers]\nimpl Vm {\n    fn on_ok_click(&mut self, sender: &Sender<Button>, e: &MouseEventArgs) {\n        // TODO: implement on_ok_click\n    }\n\n    fn toggled(&mut self, sender: &Sender<Switch>, e: &CheckedChangedEventArgs) {}\n}\n";
    std::fs::write(&rs_path, code).expect("write code-behind");

    let (client, handle) = spawn_server();
    initialize(&client);
    // Declared namespaces (VIEWS-SPEC.md §3): no undeclared-prefix note beside the warning under test.
    let view = "<Panel xmlns=\"https://kubuno.com/views\" xmlns:x=\"https://kubuno.com/views/x\">\n    <Button x:Name=\"ok\" OnClick=\"on_ok_click\"/>\n    <Button x:Name=\"other\" OnClick=\"missing_one\"/>\n</Panel>\n";
    let diagnostics = did_open_at(&client, &uri, view);
    assert_eq!(diagnostics.diagnostics.len(), 1, "{:?}", diagnostics.diagnostics);
    assert!(diagnostics.diagnostics[0].message.contains("`missing_one` not found"));

    // The dropdown, computed on the editor's buffer (a method only the buffer has).
    let buffer = code.replace("    fn toggled", "    fn on_any(&mut self) {}\n\n    fn toggled");
    let mut open_files = serde_json::Map::new();
    open_files.insert(rs_uri.clone(), buffer.into());
    send_request(&client, 2, "kubuno/compatibleHandlers", serde_json::json!({ "uri": uri, "elementId": "0", "event": "OnClick", "openFiles": open_files }));
    let result = recv_response(&client, 2, |_| {}).response_result.expect("compatibleHandlers result");
    assert_eq!(result["handlers"], serde_json::json!(["on_ok_click", "on_any"]), "{result:?}");

    // The quick fix creates the missing handler in the code-behind.
    send_request(
        &client,
        3,
        "textDocument/codeAction",
        serde_json::json!({ "textDocument": { "uri": uri }, "range": diagnostics.diagnostics[0].range, "context": { "diagnostics": [] } }),
    );
    let actions = recv_response(&client, 3, |_| {}).response_result.expect("codeAction result");
    assert_eq!(actions[0]["title"], "Create handler `missing_one`", "{actions:?}");
    assert_eq!(actions[0]["kind"], "quickfix");
    assert!(actions[0]["edit"]["changes"][&rs_uri][0]["newText"].as_str().unwrap().contains("fn missing_one(&mut self, sender: &Sender<Button>, e: &MouseEventArgs)"));

    // Rename from the ⚡ tab: the view and the method.
    send_request(&client, 4, "kubuno/renameHandler", serde_json::json!({ "uri": uri, "old": "on_ok_click", "new": "save" }));
    let result = recv_response(&client, 4, |_| {}).response_result.expect("renameHandler result");
    assert_eq!(result["oldName"], "on_ok_click");
    assert_eq!(result["edit"]["changes"][uri.as_str()][0]["newText"], "save", "{result:?}");
    assert_eq!(result["edit"]["changes"][&rs_uri][0]["newText"], "save", "{result:?}");

    // Rename from the Rust editor after rust-analyzer: only the view.
    send_request(
        &client,
        5,
        "kubuno/renameHandler",
        serde_json::json!({ "uri": rs_uri, "position": { "line": 6, "character": 10 }, "new": "save", "rustRenamed": true }),
    );
    let result = recv_response(&client, 5, |_| {}).response_result.expect("renameHandler result");
    assert_eq!(result["oldName"], "on_ok_click");
    assert!(result["edit"]["changes"].get(&rs_uri).is_none(), "{result:?}");

    // F2 on the handler name in the XML.
    send_request(&client, 6, "textDocument/prepareRename", serde_json::json!({ "textDocument": { "uri": uri }, "position": { "line": 1, "character": 35 } }));
    let result = recv_response(&client, 6, |_| {}).response_result.expect("prepareRename result");
    assert_eq!(result["placeholder"], "on_ok_click", "{result:?}");
    send_request(
        &client,
        7,
        "textDocument/rename",
        serde_json::json!({ "textDocument": { "uri": uri }, "position": { "line": 1, "character": 35 }, "newName": "toggled" }),
    );
    let error = recv_response(&client, 7, |_| {}).response_result.expect_err("a taken name is refused");
    assert!(error.message.contains("already has"), "{error:?}");

    // Clearing the event removes the attribute and the untouched stub.
    send_request(&client, 8, "kubuno/removeHandler", serde_json::json!({ "uri": uri, "elementId": "0", "event": "OnClick" }));
    let result = recv_response(&client, 8, |_| {}).response_result.expect("removeHandler result");
    assert_eq!(result["removedStub"], true, "{result:?}");
    assert_eq!(result["handlerName"], "on_ok_click");
    assert!(result["edit"]["changes"][&rs_uri][0]["newText"] == "");

    // The code-behind saved with the missing handler: the warning goes away without touching the view.
    thread::sleep(Duration::from_secs(2)); // past the server's first look at the folder
    std::fs::write(&rs_path, code.replace("    fn toggled", "    fn missing_one(&mut self) {}\n\n    fn toggled")).expect("save code-behind");
    let republished = recv_diagnostics(&client);
    assert!(republished.diagnostics.is_empty(), "{:?}", republished.diagnostics);

    shutdown(&client, handle);
    std::fs::remove_dir_all(&dir).ok();
}

/// EVT-7b: a view of a package whose sources declare a control knows it at once — validation,
/// completion, hover, go-to-definition, the registry export and `kubuno/crateComponents`.
#[test]
fn project_controls_round_trip() {
    let _serial = serial();
    let (dir, uri) = temp_kbview("project", "src/main_view.kbview");
    std::fs::create_dir_all(dir.join("src")).expect("src");
    std::fs::write(dir.join("Cargo.toml"), "[package]\nname = \"proj-app\"\nversion = \"0.1.0\"\n").expect("manifest");
    std::fs::write(
        dir.join("src/proj_round.rs"),
        "use kubuno_desktop_views::prelude::*;\n\n/// A pill.\n#[derive(Component, Default)]\n#[kubuno(extends = Button)]\npub struct ProjRound {\n    base: Button,\n    /// The radius.\n    #[property]\n    #[category(\"Appearance\")]\n    pub corner_radius: f32,\n}\n",
    )
    .expect("control");
    let (client, handle) = spawn_server();
    initialize(&client);

    let text = "<Panel>\n  <ProjRound CornerRadius=\"3\"/>\n  <\n</Panel>";
    let mut diagnostics = did_open_at(&client, &uri, text);
    // Published once before the scan, then again once the project's controls are known.
    if diagnostics.diagnostics.iter().any(|d| d.message.contains("ProjRound")) {
        diagnostics = recv_diagnostics(&client);
    }
    assert!(!diagnostics.diagnostics.iter().any(|d| d.message.contains("ProjRound") || d.message.contains("CornerRadius")), "{:?}", diagnostics.diagnostics);

    send_request(&client, 2, "textDocument/completion", serde_json::json!({ "textDocument": { "uri": uri }, "position": { "line": 2, "character": 3 } }));
    let items = recv_response(&client, 2, |_| {}).response_result.expect("completion");
    assert!(items.as_array().is_some_and(|a| a.iter().any(|i| i["label"] == "ProjRound")), "{items:?}");

    send_request(&client, 3, "textDocument/hover", serde_json::json!({ "textDocument": { "uri": uri }, "position": { "line": 1, "character": 5 } }));
    let hover = recv_response(&client, 3, |_| {}).response_result.expect("hover");
    let value = hover["contents"]["value"].as_str().unwrap_or_default().to_string();
    assert!(value.contains("A pill.") && value.contains("Control of `proj_app`, extends `Button`"), "{value}");

    send_request(&client, 4, "textDocument/definition", serde_json::json!({ "textDocument": { "uri": uri }, "position": { "line": 1, "character": 5 } }));
    let def = recv_response(&client, 4, |_| {}).response_result.expect("definition");
    assert!(def[0]["uri"].as_str().is_some_and(|u| u.ends_with("proj_round.rs")), "{def:?}");
    assert_eq!(def[0]["range"]["start"]["line"], 5, "the struct's line");

    send_request(&client, 5, "kubuno/registry", serde_json::json!(null));
    let registry = recv_response(&client, 5, |_| {}).response_result.expect("registry");
    let entry = registry["components"].as_array().and_then(|a| a.iter().find(|c| c["name"] == "ProjRound")).cloned().expect("exported");
    assert_eq!(entry["origin"], "project");
    assert_eq!(entry["linked"], false);
    assert_eq!(entry["crate_name"], "proj_app");
    assert_eq!(entry["properties"][0]["category"], "Appearance");

    send_request(&client, 6, "kubuno/crateComponents", serde_json::json!({ "manifestPath": dir.join("Cargo.toml").to_string_lossy() }));
    let crate_components = recv_response(&client, 6, |_| {}).response_result.expect("crateComponents");
    assert_eq!(crate_components["crateName"], "proj_app");
    assert_eq!(crate_components["components"][0]["name"], "ProjRound");

    shutdown(&client, handle);
    std::fs::remove_dir_all(&dir).ok();
}
