//! The DSG-6 IPC protocol: line-delimited JSON on the design surface's own
//! stdin (host → surface) and stdout (surface → host) — `vskubuno/docs/
//! DESIGNER.md`'s "DSG-6 protocol" section documents the wire shapes this
//! module implements. `stderr` stays exactly what it already was
//! (`RustDesignSurfaceHost.cs` already captures it as plain trace/log lines,
//! `[embed] …` — see `examples/view_embed.rs`); this module never touches
//! it.
//!
//! One JSON object per line, no framing beyond the newline — the same
//! "trivially testable" shape `DESIGNER.md` §3 already chose for the
//! `kubuno-desktop-views-ls` channel, reused here for consistency rather than
//! inventing a second IPC style for the other Rust process.
//!
//! - [`HostMessage`] — what `RustDesignSurfaceHost` sends on the surface's
//!   stdin: `setText` (re-parse+render the buffer's current text — replaces
//!   the temp-file bridge `RustDesignSurfaceHost.SetDocumentText` used
//!   before DSG-6), `setDesignMode` (turn the whole feature on/off), `select`
//!   (host-driven selection, e.g. the XML pane's caret moved), and DSG-9's
//!   toolbox drag/drop quartet `dragEnter`/`dragOver`/`drop`/`dragLeave` (the
//!   host translates OLE/WPF drag-drop from the VS Toolbox into these —
//!   `DESIGNER.md` DSG-9 item 3).
//! - [`SurfaceMessage`] — what the surface sends back on stdout:
//!   `selectionChanged` (a click, or an Esc-to-parent), `editRequest` (a
//!   single [`crate::design::EditOp`] — Delete/nudge, or a DSG-9 toolbox
//!   drop/Flow reorder), `editRequests` (DSG-9's BATCHED form for a move/
//!   resize drag: `{ops, gesture}`, applied by the host as one undo unit —
//!   `DESIGNER.md` DSG-9 item 1), and `dropTargetChanged` (DSG-9's live
//!   toolbox-drop feedback: where a drop would land right now, and whether
//!   it is allowed).
//!
//! A malformed or unrecognised line is never an error — [`parse_host_message`]
//! returns `None` for it, matching DSG-2's own "stale or bogus input, no-op,
//! never a hard failure" rule (`DESIGNER.md` §8's `kubuno/applyEdit`
//! doc: "never an error").

use serde::{Deserialize, Serialize};

use crate::design::{DesignCommand, DropTarget, EditOp, FormatCommand, Gesture};
use kubuno_desktop_ui::Rect;

/// One host → surface message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum HostMessage {
    /// Re-parses and renders `text` as the current document — the buffer's
    /// live text, pushed directly instead of through a temp file
    /// (`DESIGNER.md` §2: "the buffer, not the file on disk, is
    /// authoritative").
    SetText {
        text: String,
        /// The folder of the view file: the relative image paths the view names (`Image`,
        /// `BackgroundImage`, `Icon`) are resolved against it.
        #[serde(default, rename = "baseDir", skip_serializing_if = "Option::is_none")]
        base_dir: Option<String>,
    },
    /// Turns design mode on or off (`DESIGNER.md` §6 scope item 2): on,
    /// clicks/keys select/nudge/delete instead of reaching the compiled
    /// view's own widgets.
    SetDesignMode { on: bool },
    /// Host-driven selection (XML pane → Design surface sync, `DESIGNER.md`
    /// §1) — `id: null` clears it.
    Select { id: Option<String> },
    /// Host-driven multi-selection (`DESIGNER.md` §13) - e.g. resent to a restarted surface. `primary`
    /// is the primary selection (one of `ids`; the first one when absent or not among them).
    SelectMany { ids: Vec<String>, primary: Option<String> },
    /// A Layout toolbar / Format menu command on the current selection (`DESIGNER.md` §13): the
    /// surface answers with ONE `editRequests {gesture: "format"}` (nothing when it does not apply).
    Format { command: FormatCommand },
    /// DSG-9 toolbox drag/drop — a VS Toolbox drag entered the surface's own
    /// window. `component` is a registry element name (`"Button"`).
    DragEnter { component: String },
    /// DSG-9: the toolbox drag moved to `(x, y)`, surface-client DIP — the
    /// surface recomputes its drop target and shows the insertion marker/
    /// position (`DESIGNER.md` DSG-9 item 3).
    DragOver { x: f32, y: f32 },
    /// DSG-9: the toolbox drag was dropped at `(x, y)` — the surface emits
    /// one `editRequest { op: insertChild }` when the current target is
    /// valid, nothing otherwise.
    Drop { x: f32, y: f32 },
    /// DSG-9: the toolbox drag left the surface's own window (cancelled, or
    /// moved onto some other UI) — clears any drop-target adorner.
    DragLeave,
    /// EVT-7b: the project's own controls as the language server knows them (its `kubuno/registry`
    /// entries of `origin: "project"`). The surface registers those its program does not link as
    /// placeholders (`crate::registry::set_declared`) and reloads the view, so a view using a custom
    /// control shows a labelled box until the project is built, instead of failing to compile.
    ProjectComponents { components: Vec<crate::registry::DeclaredComponent> },
    /// The designer options that change what the surface draws (Tools > Options > Kubuno >
    /// Designer). `containerOutlines`: the faint dashed outline around the otherwise-invisible
    /// containers (`crate::design::set_container_outlines`); on when absent.
    SetDesignOptions {
        #[serde(default = "default_true", rename = "containerOutlines")]
        container_outlines: bool,
    },
    /// The IDE's designer background (`#RRGGBB`), sent at start and again when its theme changes:
    /// the canvas around the view (`crate::design::set_canvas_background`) follows it live.
    SetCanvasBackground { color: String },
    /// The designer's zoom (vskubuno docs/RIBBON.md, pass 2): `zoom` is a factor (1.0 = 100 %), or 0 for
    /// « fit »: the surface picks the largest factor up to 1 that shows the whole designed window.
    SetZoom { zoom: f32 },
    /// The project's resource files and the design-time culture (vskubuno docs/RESOURCES.md): the surface
    /// registers `sets` as loaded resource sets (replacing the previous ones) and shows `{Res …}` values in
    /// `culture` (`""`: the neutral values, the designer's "(Default)" language).
    SetResources {
        #[serde(default)]
        culture: String,
        #[serde(default)]
        sets: Vec<ResourceSetWire>,
    },
}

/// One resource set of [`HostMessage::SetResources`]: the neutral file's text and its satellites', the
/// folder linked files are relative to.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResourceSetWire {
    pub name: String,
    #[serde(rename = "baseDir")]
    pub base_dir: String,
    pub neutral: String,
    #[serde(default)]
    pub satellites: Vec<ResourceSatelliteWire>,
}

/// A satellite of a [`ResourceSetWire`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResourceSatelliteWire {
    pub culture: String,
    pub text: String,
}

impl ResourceSetWire {
    /// The run-time set these files make.
    pub fn to_set(&self) -> kubuno_desktop_resources::LoadedSet {
        let satellites: Vec<(String, String)> = self.satellites.iter().map(|s| (s.culture.clone(), s.text.clone())).collect();
        kubuno_desktop_resources::LoadedSet::from_texts(self.name.clone(), self.base_dir.clone(), &self.neutral, &satellites)
    }
}

/// Applies a [`HostMessage::SetResources`]: the sets replace the previously loaded ones, then the culture
/// is set (which repaints).
pub fn apply_resources(culture: &str, sets: &[ResourceSetWire]) {
    kubuno_desktop_resources::replace_loaded(sets.iter().map(|s| std::sync::Arc::new(s.to_set()) as std::sync::Arc<dyn kubuno_desktop_resources::Source>).collect());
    kubuno_desktop_resources::set_culture(if culture.trim().is_empty() { "invariant" } else { culture });
}

/// `#RRGGBB` (or `RRGGBB`) as `0xRRGGBB`; `None` when it is not one.
pub fn parse_rgb(color: &str) -> Option<u32> {
    let hex = color.trim().trim_start_matches('#');
    (hex.len() == 6).then(|| u32::from_str_radix(hex, 16).ok()).flatten()
}

fn default_true() -> bool {
    true
}

/// One surface → host message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum SurfaceMessage {
    /// The selection changed on the surface (a click, or Esc-to-parent).
    /// `bounds` is the newly selected element's painted rect, `None` for
    /// `id: None` (nothing selected) or an id the current layout map has no
    /// entry for (a selection that raced a reload).
    ///
    /// `DESIGNER.md` §13: `id` is the PRIMARY selection and `ids` the whole selection (primary
    /// included, selection order) - one element for an ordinary single selection, empty for none.
    SelectionChanged {
        id: Option<String>,
        bounds: Option<WireRect>,
        #[serde(default)]
        ids: Vec<String>,
    },
    /// Delete, a nudging arrow, a DSG-9 Flow reorder, or a DSG-9 toolbox drop
    /// produced this SINGLE edit request — forward it to `kubuno-desktop-views-ls`'s
    /// `kubuno/applyEdit`, exactly shaped like that method's own `op`
    /// parameter (`DESIGNER.md` §8).
    EditRequest { op: EditOp },
    /// DSG-9's batched form: a move or resize drag's mouse-up produces one or
    /// more `setAttribute` ops that must land as a SINGLE undo unit
    /// (`DESIGNER.md` DSG-9 item 1) — the host applies every entry in `ops`
    /// inside one `IOleUndoManager`/`ITextUndoHistory` compound action rather
    /// than one `kubuno/applyEdit` call per op. `gesture` says which kind of
    /// drag produced them (`"move"` or `"resize"` on the wire).
    EditRequests { ops: Vec<EditOp>, gesture: Gesture },
    /// DSG-9's live toolbox-drop feedback (`crate::design::ToolboxController
    /// ::drag_over`'s own result, re-sent after every `dragOver`/on
    /// `dragLeave`): `None` when there is no current target (no active drag,
    /// nothing under the pointer, or the drag just left) — the host clears
    /// its own drop-target adorner/cursor in that case.
    DropTargetChanged { target: Option<WireDropTarget> },
    /// A right-click (or Shift+F10 / the context-menu key) on the surface: the host shows its
    /// context menu at `(screenX, screenY)` (physical screen pixels). `(x, y)` is the same point
    /// in surface-client DIP. `elementId` is the element the menu is for — already selected by the
    /// surface, which sends the `selectionChanged` first — or `null` for the view itself (the
    /// canvas background or the frame's title bar).
    ContextMenu {
        x: f32,
        y: f32,
        #[serde(rename = "screenX")]
        screen_x: i32,
        #[serde(rename = "screenY")]
        screen_y: i32,
        #[serde(rename = "elementId")]
        element_id: Option<String>,
        /// A menu of the element other than its context menu: `"add"` (a ribbon's « + » glyph: what
        /// can be added into the element) or `"tasks"` (its smart tag). Absent for the context menu.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        menu: Option<String>,
    },
    /// A clipboard/duplicate keyboard command on the surface (Ctrl+C / Ctrl+X / Ctrl+V / Ctrl+D)
    /// for `elementId` (the current selection; `null` = the view itself, e.g. a paste into it).
    Command {
        name: DesignCommand,
        #[serde(rename = "elementId")]
        element_id: Option<String>,
    },
    /// A double-click on an element of the surface (`vskubuno/docs/EVENTS.md` §5.2, EVT-3): the
    /// host creates or opens the element's DEFAULT event handler, like the WinForms designer.
    /// `elementId` is the element (`""` = the view's root element, e.g. a double-click on the
    /// frame's title bar); the surface has already selected it and sent `selectionChanged`.
    DoubleClick {
        #[serde(rename = "elementId")]
        element_id: String,
    },
    /// What the preview shows after a `setText` (or a reload by `projectComponents`) — `DESIGNER.md`
    /// §17: the view as written, its valid part with placeholders, the last good preview kept over a
    /// malformed text, or nothing — and the diagnostics of the text, for the designer's error banner.
    /// Sent once per reload, only when it differs from the previous one.
    RenderStatus {
        state: RenderState,
        diagnostics: Vec<WireDiagnostic>,
    },
    /// A click on the warning marker of an element the preview shows differently from its text
    /// (`DESIGNER.md` §17): the host shows the XML pane with the finding's span selected. The element
    /// itself was selected first (`selectionChanged`).
    GoToSource { diagnostic: WireDiagnostic },
}

/// The wire form of [`crate::runtime::DesignRenderState`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RenderState {
    Clean,
    Tolerant,
    Recovered,
    Stale,
    Empty,
}

impl From<crate::runtime::DesignRenderState> for RenderState {
    fn from(state: crate::runtime::DesignRenderState) -> Self {
        use crate::runtime::DesignRenderState as S;
        match state {
            S::Clean => RenderState::Clean,
            S::Tolerant => RenderState::Tolerant,
            S::Recovered => RenderState::Recovered,
            S::Stale => RenderState::Stale,
            S::Empty => RenderState::Empty,
        }
    }
}

/// One diagnostic of the text, positioned for Visual Studio: 1-based lines, 1-based columns in
/// UTF-16 code units (what `IVsTextView.SetSelection` counts, minus one), start and end.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireDiagnostic {
    pub line: u32,
    pub column: u32,
    #[serde(rename = "endLine")]
    pub end_line: u32,
    #[serde(rename = "endColumn")]
    pub end_column: u32,
    pub message: String,
    /// `"unknownElement"` for an element the preview does not know (`element` names it: the host
    /// can tell a misspelt name from a control its own registry knows but this preview's runtime
    /// does not), absent otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub element: Option<String>,
    /// A syntax error (the text is not well-formed XML): listed first by the host.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub syntax: bool,
}

/// The 1-based line and UTF-16 column of byte `offset` of `text` (clamped to the text, and back to
/// a character boundary).
fn utf16_position(text: &str, offset: usize) -> (u32, u32) {
    let mut offset = offset.min(text.len());
    while !text.is_char_boundary(offset) {
        offset -= 1;
    }
    let before = &text[..offset];
    let line_start = before.rfind('\n').map_or(0, |i| i + 1);
    let line = before.matches('\n').count() + 1;
    let column = before[line_start..].encode_utf16().count() + 1;
    (u32::try_from(line).unwrap_or(u32::MAX), u32::try_from(column).unwrap_or(u32::MAX))
}

/// `diagnostics` (positioned in `text`, byte ranges) in their wire form.
pub fn wire_diagnostics(text: &str, diagnostics: &[crate::syntax::Diagnostic]) -> Vec<WireDiagnostic> {
    let syntax = crate::syntax::parse(text).diagnostics;
    diagnostics
        .iter()
        .map(|d| {
            let (start, end) = (usize::from(d.range.start()), usize::from(d.range.end()));
            let (line, column) = if start == 0 && end == 0 { (d.line.max(1), d.column.max(1)) } else { utf16_position(text, start) };
            let (end_line, end_column) = if end > start { utf16_position(text, end) } else { (line, column) };
            let unknown = d.message.strip_prefix("unknown element `<").and_then(|rest| rest.split_once(">`")).map(|(name, _)| name.to_string());
            WireDiagnostic {
                line,
                column,
                end_line,
                end_column,
                message: crate::messages::localize(&d.message),
                code: unknown.as_ref().map(|_| "unknownElement".to_string()),
                element: unknown,
                syntax: syntax.iter().any(|s| s.range == d.range && s.message == d.message),
            }
        })
        .collect()
}

/// The wire shape of [`crate::design::DropTarget`] — plain, fully
/// serializable fields only (unlike that type, which embeds a [`Rect`] with
/// no `serde` impl of its own), so this struct — not `DropTarget` itself —
/// is what travels on the wire; [`From<DropTarget>`] converts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WireDropTarget {
    pub valid: bool,
    #[serde(rename = "parentId")]
    pub parent_id: String,
    pub index: usize,
    /// The new element's placement `X`/`Y`, parent-local DIP — `None` for a
    /// Flow/other parent (see [`DropTarget::xy`]'s own doc).
    pub xy: Option<(f32, f32)>,
    pub marker: WireRect,
}

impl From<DropTarget> for WireDropTarget {
    fn from(t: DropTarget) -> Self {
        Self { valid: t.valid, parent_id: t.parent_id, index: t.index, xy: t.xy, marker: t.marker.into() }
    }
}

/// [`kubuno_desktop_ui::Rect`] has no `Serialize`/`Deserialize` of its own (it is a
/// plain geometry type reused by every control, not a wire type) — this is
/// the field-for-field wire shape, named distinctly so a reader of the
/// protocol never confuses "a rect on the wire" with "a rect mid-layout".
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct WireRect {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

impl From<Rect> for WireRect {
    fn from(r: Rect) -> Self {
        Self { left: r.left, top: r.top, right: r.right, bottom: r.bottom }
    }
}

/// Parses one stdin line as a [`HostMessage`] — `None` for a blank line, one
/// that fails to parse as JSON at all, or JSON that is well-formed but does
/// not match any known `type` (an older/newer host talking to this surface,
/// or a stray line on the pipe): a caller (`examples/view_embed.rs`) simply
/// skips it and reads the next line, never panics or tears down the surface
/// process over one bad line.
///
/// Strips a leading U+FEFF (byte-order mark) before parsing, defence in
/// depth against a host whose first write to this process's stdin prepends
/// one — confirmed live during DSG-9's own visual check: some `.NET`
/// `StreamWriter` configurations emit a UTF-8 BOM preamble on the very FIRST
/// write to a stream and never again, so the FIRST protocol line a freshly
/// launched surface ever receives (typically `setText`) could otherwise be
/// silently dropped here (BOM lost its Unicode `White_Space` property in
/// Unicode 6.3, so a plain `str::trim()` does not remove it). The real fix
/// is on the host side (`vskubuno`'s `RustDesignSurfaceHost.cs` now sets
/// `StandardInputEncoding` explicitly); this is the belt to that braces.
pub fn parse_host_message(line: &str) -> Option<HostMessage> {
    let line = line.trim().trim_start_matches('\u{feff}');
    if line.is_empty() {
        return None;
    }
    serde_json::from_str(line).ok()
}

/// Encodes one [`SurfaceMessage`] as a single JSON line, WITHOUT the
/// trailing newline (the caller's `writeln!`/`println!` supplies that, so
/// this function stays testable as a plain string comparison). A message
/// that somehow fails to serialize (none of this module's own types can —
/// every field is a plain string/bool/enum/float — this is defensive only)
/// degrades to `"{}"`, an empty JSON object the host's own line parser
/// silently ignores, rather than panicking a whole design surface over one
/// bad line out.
pub fn encode_surface_message(msg: &SurfaceMessage) -> String {
    serde_json::to_string(msg).unwrap_or_else(|_| "{}".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// EVT-7b: the project's controls, as the language server exports them (extra keys ignored).
    #[test]
    fn project_components_parse_from_export_entries() {
        let line = r#"{"type":"projectComponents","components":[{"name":"RoundButton","kind":"control","doc":"A pill.","family":"project","extends":"Button","base_chain":["RoundButton","Button","ButtonBase","Control","Component"],"properties":[{"name":"CornerRadius","kind":"F32","default":"18","doc":"","doc_fr":null,"category":"Appearance"},{"name":"Shape","kind":{"Enum":["Pill","Square"]}}],"events":[{"name":"OnLongPress","category":"Mouse","args_type":"MouseEventArgs","inherited_from":null},{"name":"OnClick","inherited_from":"Control","common":true}]}]}"#;
        let Some(HostMessage::ProjectComponents { components }) = parse_host_message(line) else { panic!("not parsed") };
        assert_eq!(components.len(), 1);
        let c = &components[0];
        assert_eq!((c.name.as_str(), c.extends.as_str(), c.base_chain.len()), ("RoundButton", "Button", 5));
        assert_eq!(c.properties[1].kind, crate::registry::DeclaredKind::Enum(vec!["Pill".into(), "Square".into()]));
        assert_eq!(c.properties[0].category.as_deref(), Some("Appearance"));
        assert_eq!(c.events[1].inherited_from.as_deref(), Some("Control"));
        assert!(c.browsable, "absent fields take their defaults");
    }

    #[test]
    fn set_text_round_trips() {
        let msg = HostMessage::SetText { text: "<Button/>".to_string(), base_dir: None };
        let json = serde_json::to_string(&msg).unwrap();
        assert_eq!(json, r#"{"type":"setText","text":"<Button/>"}"#);
        assert_eq!(parse_host_message(&json), Some(msg));
    }

    #[test]
    fn set_text_carries_the_view_folder() {
        let json = r#"{"type":"setText","text":"<Button/>","baseDir":"C:\\app\\src"}"#;
        assert_eq!(parse_host_message(json), Some(HostMessage::SetText { text: "<Button/>".to_string(), base_dir: Some(r"C:\app\src".to_string()) }));
    }

    #[test]
    fn set_design_mode_round_trips() {
        let msg = HostMessage::SetDesignMode { on: true };
        let json = serde_json::to_string(&msg).unwrap();
        assert_eq!(json, r#"{"type":"setDesignMode","on":true}"#);
        assert_eq!(parse_host_message(&json), Some(msg));
    }

    #[test]
    fn set_zoom_round_trips() {
        let msg = HostMessage::SetZoom { zoom: 0.75 };
        let json = serde_json::to_string(&msg).unwrap();
        assert_eq!(json, r#"{"type":"setZoom","zoom":0.75}"#);
        assert_eq!(parse_host_message(&json), Some(msg));
    }

    #[test]
    fn set_design_options_round_trips_and_defaults_to_outlines_on() {
        let msg = HostMessage::SetDesignOptions { container_outlines: false };
        let json = serde_json::to_string(&msg).unwrap();
        assert_eq!(json, r#"{"type":"setDesignOptions","containerOutlines":false}"#);
        assert_eq!(parse_host_message(&json), Some(msg));
        assert_eq!(parse_host_message(r#"{"type":"setDesignOptions"}"#), Some(HostMessage::SetDesignOptions { container_outlines: true }));
    }

    #[test]
    fn select_with_and_without_an_id_round_trips() {
        let with_id = HostMessage::Select { id: Some("0.1".to_string()) };
        assert_eq!(parse_host_message(&serde_json::to_string(&with_id).unwrap()), Some(with_id));

        let clear = HostMessage::Select { id: None };
        let json = serde_json::to_string(&clear).unwrap();
        assert_eq!(json, r#"{"type":"select","id":null}"#);
        assert_eq!(parse_host_message(&json), Some(clear));
    }

    #[test]
    fn parse_host_message_is_none_for_blank_or_garbage_lines() {
        assert_eq!(parse_host_message(""), None);
        assert_eq!(parse_host_message("   \n"), None);
        assert_eq!(parse_host_message("not json at all"), None);
        assert_eq!(parse_host_message(r#"{"type":"somethingUnknown"}"#), None);
    }

    #[test]
    fn parse_host_message_strips_a_leading_byte_order_mark() {
        // A `.NET` `StreamWriter` can prepend a UTF-8 BOM to the FIRST write on a stream
        // (confirmed live, DSG-9's own visual check) - `\u{feff}` is not `char::is_whitespace`
        // (removed from Unicode's `White_Space` property in 6.3), so a plain `.trim()` alone
        // would not have stripped it.
        let msg = HostMessage::SetDesignMode { on: true };
        let json = serde_json::to_string(&msg).unwrap();
        let with_bom = format!("\u{feff}{json}");
        assert_eq!(parse_host_message(&with_bom), Some(msg));
    }

    #[test]
    fn parse_host_message_strips_a_byte_order_mark_even_with_surrounding_whitespace() {
        let msg = HostMessage::Select { id: None };
        let json = serde_json::to_string(&msg).unwrap();
        let with_bom = format!("  \u{feff}{json}\n");
        assert_eq!(parse_host_message(&with_bom), Some(msg));
    }

    #[test]
    fn selection_changed_wire_shape_matches_documented_field_names() {
        let msg = SurfaceMessage::SelectionChanged {
            id: Some("0".to_string()),
            bounds: Some(WireRect { left: 1.0, top: 2.0, right: 3.0, bottom: 4.0 }),
            ids: vec!["0".to_string()],
        };
        let json = encode_surface_message(&msg);
        assert_eq!(
            json,
            r#"{"type":"selectionChanged","id":"0","bounds":{"left":1.0,"top":2.0,"right":3.0,"bottom":4.0},"ids":["0"]}"#
        );
    }

    #[test]
    fn selection_changed_with_no_bounds_omits_nothing_but_nulls_it() {
        let msg = SurfaceMessage::SelectionChanged { id: None, bounds: None, ids: Vec::new() };
        let json = encode_surface_message(&msg);
        assert_eq!(json, r#"{"type":"selectionChanged","id":null,"bounds":null,"ids":[]}"#);
    }

    #[test]
    fn selection_changed_carries_the_whole_multi_selection() {
        let msg = SurfaceMessage::SelectionChanged {
            id: Some("0.1".to_string()),
            bounds: None,
            ids: vec!["0.0".to_string(), "0.1".to_string()],
        };
        assert_eq!(
            encode_surface_message(&msg),
            r#"{"type":"selectionChanged","id":"0.1","bounds":null,"ids":["0.0","0.1"]}"#
        );
        // An older surface's line without `ids` still parses (it defaults to empty).
        let old: SurfaceMessage = serde_json::from_str(r#"{"type":"selectionChanged","id":"0","bounds":null}"#).unwrap();
        assert_eq!(old, SurfaceMessage::SelectionChanged { id: Some("0".to_string()), bounds: None, ids: Vec::new() });
    }

    #[test]
    fn select_many_round_trips() {
        let msg = HostMessage::SelectMany { ids: vec!["0".to_string(), "1".to_string()], primary: Some("1".to_string()) };
        let json = serde_json::to_string(&msg).unwrap();
        assert_eq!(json, r#"{"type":"selectMany","ids":["0","1"],"primary":"1"}"#);
        assert_eq!(parse_host_message(&json), Some(msg));
    }

    #[test]
    fn format_round_trips_with_camel_case_command_names() {
        let msg = HostMessage::Format { command: FormatCommand::HorizontalSpacingEqual };
        let json = serde_json::to_string(&msg).unwrap();
        assert_eq!(json, r#"{"type":"format","command":"horizontalSpacingEqual"}"#);
        assert_eq!(parse_host_message(&json), Some(msg));
        assert_eq!(
            parse_host_message(r#"{"type":"format","command":"alignLefts"}"#),
            Some(HostMessage::Format { command: FormatCommand::AlignLefts })
        );
        assert_eq!(parse_host_message(r#"{"type":"format","command":"alignToGrid"}"#), None);
    }

    #[test]
    fn edit_requests_delete_and_format_gestures_serialize_lowercase() {
        let delete = SurfaceMessage::EditRequests {
            ops: vec![EditOp::RemoveElement { element_id: "0".to_string() }, EditOp::RemoveElement { element_id: "1".to_string() }],
            gesture: Gesture::Delete,
        };
        assert_eq!(
            encode_surface_message(&delete),
            r#"{"type":"editRequests","ops":[{"kind":"removeElement","elementId":"0"},{"kind":"removeElement","elementId":"1"}],"gesture":"delete"}"#
        );
        let format = SurfaceMessage::EditRequests { ops: Vec::new(), gesture: Gesture::Format };
        assert!(encode_surface_message(&format).ends_with(r#""gesture":"format"}"#));
    }

    #[test]
    fn edit_request_set_attribute_matches_the_dsg2_applyedit_op_shape() {
        let msg = SurfaceMessage::EditRequest {
            op: EditOp::SetAttribute { element_id: "0.1".to_string(), name: "X".to_string(), value: "42".to_string() },
        };
        let json = encode_surface_message(&msg);
        assert_eq!(
            json,
            r#"{"type":"editRequest","op":{"kind":"setAttribute","elementId":"0.1","name":"X","value":"42"}}"#
        );
    }

    #[test]
    fn edit_request_remove_element_matches_the_dsg2_applyedit_op_shape() {
        let msg = SurfaceMessage::EditRequest { op: EditOp::RemoveElement { element_id: "0.1".to_string() } };
        let json = encode_surface_message(&msg);
        assert_eq!(json, r#"{"type":"editRequest","op":{"kind":"removeElement","elementId":"0.1"}}"#);
    }

    #[test]
    fn wire_rect_from_rect_carries_every_field() {
        let r = Rect::new(1.0, 2.0, 3.0, 4.0);
        let wire: WireRect = r.into();
        assert_eq!(wire, WireRect { left: 1.0, top: 2.0, right: 3.0, bottom: 4.0 });
    }

    // ── DSG-9: toolbox drag/drop (host → surface) ───────────────────────

    #[test]
    fn drag_enter_round_trips() {
        let msg = HostMessage::DragEnter { component: "Button".to_string() };
        let json = serde_json::to_string(&msg).unwrap();
        assert_eq!(json, r#"{"type":"dragEnter","component":"Button"}"#);
        assert_eq!(parse_host_message(&json), Some(msg));
    }

    #[test]
    fn drag_over_round_trips() {
        let msg = HostMessage::DragOver { x: 10.5, y: 20.0 };
        let json = serde_json::to_string(&msg).unwrap();
        assert_eq!(json, r#"{"type":"dragOver","x":10.5,"y":20.0}"#);
        assert_eq!(parse_host_message(&json), Some(msg));
    }

    #[test]
    fn drop_round_trips() {
        let msg = HostMessage::Drop { x: 10.5, y: 20.0 };
        let json = serde_json::to_string(&msg).unwrap();
        assert_eq!(json, r#"{"type":"drop","x":10.5,"y":20.0}"#);
        assert_eq!(parse_host_message(&json), Some(msg));
    }

    #[test]
    fn drag_leave_round_trips_with_no_fields() {
        let msg = HostMessage::DragLeave;
        let json = serde_json::to_string(&msg).unwrap();
        assert_eq!(json, r#"{"type":"dragLeave"}"#);
        assert_eq!(parse_host_message(&json), Some(msg));
    }

    // ── DSG-9: batched edit requests + drop target (surface → host) ────

    #[test]
    fn edit_requests_batch_matches_the_documented_wire_shape() {
        let msg = SurfaceMessage::EditRequests {
            ops: vec![
                EditOp::SetAttribute { element_id: "0.1".to_string(), name: "X".to_string(), value: "42".to_string() },
                EditOp::SetAttribute { element_id: "0.1".to_string(), name: "Y".to_string(), value: "10".to_string() },
            ],
            gesture: Gesture::Move,
        };
        let json = encode_surface_message(&msg);
        assert_eq!(
            json,
            r#"{"type":"editRequests","ops":[{"kind":"setAttribute","elementId":"0.1","name":"X","value":"42"},{"kind":"setAttribute","elementId":"0.1","name":"Y","value":"10"}],"gesture":"move"}"#
        );
    }

    #[test]
    fn edit_requests_resize_gesture_serializes_lowercase() {
        let msg = SurfaceMessage::EditRequests {
            ops: vec![EditOp::SetAttribute { element_id: "0".to_string(), name: "Width".to_string(), value: "120".to_string() }],
            gesture: Gesture::Resize,
        };
        let json = encode_surface_message(&msg);
        assert!(json.contains(r#""gesture":"resize""#), "{json}");
    }

    #[test]
    fn edit_request_move_element_matches_the_dsg2_applyedit_op_shape() {
        let msg = SurfaceMessage::EditRequest {
            op: EditOp::MoveElement { element_id: "0.0".to_string(), new_parent_id: "0".to_string(), index: 2 },
        };
        let json = encode_surface_message(&msg);
        assert_eq!(
            json,
            r#"{"type":"editRequest","op":{"kind":"moveElement","elementId":"0.0","newParentId":"0","index":2}}"#
        );
    }

    #[test]
    fn edit_request_insert_child_matches_the_dsg2_applyedit_op_shape() {
        let msg = SurfaceMessage::EditRequest {
            op: EditOp::InsertChild { parent_id: "0".to_string(), index: 1, xml: "<Button/>".to_string() },
        };
        let json = encode_surface_message(&msg);
        assert_eq!(json, r#"{"type":"editRequest","op":{"kind":"insertChild","parentId":"0","index":1,"xml":"<Button/>"}}"#);
    }

    #[test]
    fn drop_target_changed_with_a_target_matches_the_documented_wire_shape() {
        let msg = SurfaceMessage::DropTargetChanged {
            target: Some(WireDropTarget {
                valid: true,
                parent_id: "0".to_string(),
                index: 2,
                xy: Some((10.0, 20.0)),
                marker: WireRect { left: 1.0, top: 2.0, right: 3.0, bottom: 4.0 },
            }),
        };
        let json = encode_surface_message(&msg);
        assert_eq!(
            json,
            r#"{"type":"dropTargetChanged","target":{"valid":true,"parentId":"0","index":2,"xy":[10.0,20.0],"marker":{"left":1.0,"top":2.0,"right":3.0,"bottom":4.0}}}"#
        );
    }

    #[test]
    fn drop_target_changed_with_no_target_serializes_a_null() {
        let msg = SurfaceMessage::DropTargetChanged { target: None };
        assert_eq!(encode_surface_message(&msg), r#"{"type":"dropTargetChanged","target":null}"#);
    }

    #[test]
    fn context_menu_matches_the_documented_wire_shape() {
        let msg = SurfaceMessage::ContextMenu {
            x: 10.5,
            y: 20.0,
            screen_x: 300,
            screen_y: -40,
            element_id: Some("0.1".to_string()),
            menu: None,
        };
        assert_eq!(
            encode_surface_message(&msg),
            r#"{"type":"contextMenu","x":10.5,"y":20.0,"screenX":300,"screenY":-40,"elementId":"0.1"}"#
        );
        let view = SurfaceMessage::ContextMenu { x: 1.0, y: 2.0, screen_x: 3, screen_y: 4, element_id: None, menu: None };
        let add = SurfaceMessage::ContextMenu { x: 1.0, y: 2.0, screen_x: 3, screen_y: 4, element_id: Some("0".into()), menu: Some("add".into()) };
        assert!(encode_surface_message(&add).ends_with(r#""elementId":"0","menu":"add"}"#));
        assert!(encode_surface_message(&view).ends_with(r#""elementId":null}"#));
    }

    #[test]
    fn command_matches_the_documented_wire_shape() {
        let msg = SurfaceMessage::Command { name: DesignCommand::Duplicate, element_id: Some("0".to_string()) };
        assert_eq!(encode_surface_message(&msg), r#"{"type":"command","name":"duplicate","elementId":"0"}"#);
        let paste = SurfaceMessage::Command { name: DesignCommand::Paste, element_id: None };
        assert_eq!(encode_surface_message(&paste), r#"{"type":"command","name":"paste","elementId":null}"#);
    }

    #[test]
    fn double_click_serializes_with_the_element_id() {
        let msg = SurfaceMessage::DoubleClick { element_id: "1".to_string() };
        assert_eq!(encode_surface_message(&msg), r#"{"type":"doubleClick","elementId":"1"}"#);
    }

    /// `DESIGNER.md` §17: the render status the host's error banner reads, byte for byte.
    #[test]
    fn render_status_carries_the_state_and_utf16_positions() {
        let text = "<Stack>\n  <Label Text=\"\u{e9}\" Colour=\"red\"/>\n  <Frob/>\n</Stack>";
        let parse = crate::syntax::parse(text);
        let diagnostics = crate::validate::validate_with_default_registry(&parse);
        let wire = wire_diagnostics(text, &diagnostics);
        assert_eq!(wire.len(), 2, "{wire:?}");
        // `Colour` is the 19th UTF-16 unit of line 2 (`é` is two UTF-8 bytes but one UTF-16 unit).
        assert_eq!((wire[0].line, wire[0].column, wire[0].end_line, wire[0].end_column), (2, 19, 2, 25));
        assert_eq!(wire[0].code, None);
        assert_eq!((wire[1].line, wire[1].column), (3, 4));
        assert_eq!(wire[1].code.as_deref(), Some("unknownElement"));
        assert_eq!(wire[1].element.as_deref(), Some("Frob"));
        let msg = SurfaceMessage::RenderStatus { state: RenderState::Tolerant, diagnostics: vec![wire[1].clone()] };
        assert_eq!(
            encode_surface_message(&msg),
            r#"{"type":"renderStatus","state":"tolerant","diagnostics":[{"line":3,"column":4,"endLine":3,"endColumn":8,"message":"unknown element `<Frob>`","code":"unknownElement","element":"Frob"}]}"#
        );
        // A syntax error says so (the host lists them first); a marker's click carries one diagnostic.
        let broken = "<Stack>\n  <Label Text=\"a\n</Stack>";
        let wire = wire_diagnostics(broken, &crate::syntax::parse(broken).diagnostics);
        assert!(!wire.is_empty() && wire.iter().all(|d| d.syntax), "{wire:?}");
        assert!(encode_surface_message(&SurfaceMessage::GoToSource { diagnostic: wire[0].clone() }).starts_with(r#"{"type":"goToSource","diagnostic":{"line":2,"#));
        assert!(encode_surface_message(&SurfaceMessage::RenderStatus { state: RenderState::Stale, diagnostics: wire }).contains(r#""syntax":true"#));
        let clean = SurfaceMessage::RenderStatus { state: RenderState::Clean, diagnostics: Vec::new() };
        assert_eq!(encode_surface_message(&clean), r#"{"type":"renderStatus","state":"clean","diagnostics":[]}"#);
        let stale: SurfaceMessage = serde_json::from_str(r#"{"type":"renderStatus","state":"stale","diagnostics":[{"line":1,"column":2,"endLine":1,"endColumn":2,"message":"m"}]}"#).unwrap();
        assert!(matches!(stale, SurfaceMessage::RenderStatus { state: RenderState::Stale, ref diagnostics } if diagnostics[0].code.is_none()));
    }

    #[test]
    fn wire_drop_target_from_design_drop_target_carries_every_field() {
        let target = DropTarget {
            parent_id: "0".to_string(),
            index: 3,
            xy: Some((10.0, 20.0)),
            valid: true,
            marker: Rect::new(10.0, 20.0, 90.0, 44.0),
        };
        let wire: WireDropTarget = target.into();
        assert!(wire.valid);
        assert_eq!(wire.parent_id, "0");
        assert_eq!(wire.index, 3);
        assert_eq!(wire.xy, Some((10.0, 20.0)));
        assert_eq!(wire.marker, WireRect { left: 10.0, top: 20.0, right: 90.0, bottom: 44.0 });
    }
}

#[cfg(test)]
mod canvas_theme_tests {
    use super::*;

    #[test]
    fn the_ide_theme_colour_reaches_the_canvas() {
        assert_eq!(
            parse_host_message(r##"{"type":"setCanvasBackground","color":"#EEEEF2"}"##),
            Some(HostMessage::SetCanvasBackground { color: "#EEEEF2".into() })
        );
        assert_eq!(parse_rgb("#EEEEF2"), Some(0xEEEEF2));
        assert_eq!(parse_rgb("1e1e1e"), Some(0x1E1E1E));
        assert_eq!(parse_rgb("#FFF"), None);
        assert_eq!(parse_rgb("#GG0000"), None);
    }
}
