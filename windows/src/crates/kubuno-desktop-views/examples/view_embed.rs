//! Embeds a `.kbview` preview INSIDE another process's window — the DSG-7
//! spike of `vskubuno/docs/DESIGNER.md` (the Visual Studio designer surface,
//! hosted by a WPF `HwndHost`), extended by DSG-6 with the actual **design
//! mode** (§6): a frame-local layout map + hit-testing, selection/hover
//! adorners, and Esc/Delete/arrow request generation — all implemented in
//! [`kubuno_desktop_views::design`], this file only wires it to real input and to
//! the DSG-6 IPC protocol ([`kubuno_desktop_views::protocol`]) on this process's own
//! stdin/stdout. See `vskubuno/docs/DESIGNER.md`'s "DSG-6 protocol" section
//! for the wire shapes.
//!
//! ```text
//! view_embed --parent <hwnd> [<file.kbview>]   # child of <hwnd> (decimal or 0x-hex)
//! view_embed [<file.kbview>]                   # plain top-level window, for comparison
//! ```
//!
//! ## Built against the project it previews
//!
//! Visual Studio compiles this file with `rustc` against the dependency graph
//! of the project a view belongs to (`vskubuno/docs/DESIGNER.md` section 15),
//! so that it statically links that project's own `kubuno_desktop_ui` build (and the
//! project's crate, for its controls). Hence two rules: it uses nothing but
//! `std` and the three `kubuno_*` crates (its Win32/OLE calls are declared in
//! [`win32`], no `windows` crate feature is assumed), and its first stdout line
//! is the `surfaceInfo` handshake ([`send_surface_info`]) the host checks
//! before trusting it.
//!
//! `<file.kbview>` is now OPTIONAL: `kubuno/setText` on stdin (DSG-6) pushes
//! the buffer's live text directly, exactly `vskubuno/docs/DESIGNER.md` §2's
//! "the buffer, not the file on disk, is authoritative" — the file argument
//! remains for standalone/manual testing (no host attached) and, when both
//! are used, a `setText` simply reloads on top of whatever the file watcher
//! last loaded.
//!
//! The host runs in its child-window mode ([`HostOptions::parent`]): no
//! caption, sized to the parent's client area, resized by the parent. On top
//! of the view it paints a one-line probe (parent, DPI, focus, last key) and a
//! "Menu" button opening an interactive popup that deliberately overflows the
//! child's bottom-right corner — the floating-surface case (dropdowns) the
//! spike has to prove. Every focus/key/size/DPI message is also traced on
//! stderr (`[embed] …`) so a test harness can assert on it; the DSG-6
//! protocol traffic itself is traced too (`[embed] proto …`), on stderr, same
//! as everything else — stdout is reserved for the protocol's own JSON lines
//! (`vskubuno`'s `RustDesignSurfaceHost` already captures stderr as plain
//! trace text; see that class's own doc).
//!
//! It also owns a tiny [`FocusRing`] of its own (`save_btn`, `menu_btn` — NOT
//! the compiled `.kbview` content's, which is `Runtime`'s internal one and out
//! of reach from here per this task's scope) to exercise DSG-7's "tabOut"
//! keyboard protocol end to end: Tab/Shift+Tab past the last/first of these
//! two demo buttons calls [`host::notify_tab_out`] instead of wrapping in
//! place, so `vskubuno`'s `RustDesignSurfaceHost` can move the WPF focus back
//! out (see `docs/DESIGNER.md` §7). This demo ring, and the Save/Menu buttons
//! it drives, are host CHROME, not the compiled view's own content — design
//! mode (below) never touches them; it only ever suppresses/redirects input
//! for `runtime`'s own tree.
//!
//! ## Design mode (DSG-6)
//!
//! When [`DesignController::enabled`] is on, the compiled view is painted
//! through [`Runtime::frame_with_design`] with a NEUTERED copy of the real
//! [`Frame`] ([`neutralize_frame`]): the pointer is parked off-canvas and no
//! button/wheel state survives, so no leaf can become "hot", start a press,
//! complete a click, or take keyboard focus (a `TextField` only reads typed
//! input once it has focus) — "user input does NOT reach widgets" without
//! this file needing to touch `kubuno_desktop_controls::host` itself (out of scope:
//! another agent owns its keyboard-forwarding code). The REAL [`Frame`]
//! drives design mode's OWN click/hover/keyboard handling instead
//! ([`DesignController::click_select`]/[`update_hover`]/[`handle_keys`]),
//! and [`kubuno_desktop_views::design::paint_adorners`] draws the selection/hover/
//! parent outline on top, using the very [`LayoutMap`] `frame_with_design`
//! just recorded.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::env;
use std::io::{BufRead, Write};
use std::process::ExitCode;
use std::rc::Rc;
use std::sync::mpsc;

use kubuno_desktop_controls::host::{self, vk, Cursor, Frame, HostOptions, Modifiers};
use kubuno_desktop_ui::{FocusId, FocusRing, Rect, Theme};
use kubuno_desktop_views::ast::{AstNode, Document};
use kubuno_desktop_views::binding::{HandlerTable, Value, ViewModel};
use kubuno_desktop_views::design::{
    self, DesignCommand, DesignController, DesignKeyInput, DesignSize, DragOutcome, FrameHandle, FrameLayout,
    EditOp, FormatCommand, FrameResize, Gesture, LayoutMap, PointerModifiers, Selection, ToolboxController, CANVAS_MARGIN,
    SCROLLBAR_SIZE,
};
use kubuno_desktop_views::handlers;
use kubuno_desktop_views::protocol::{self, HostMessage, SurfaceMessage};
use kubuno_desktop_views::runtime::{DesignRenderState, FileWatcher, Runtime};
use kubuno_desktop_views::tolerant::DesignIssue;

// EVT-7b: the Visual Studio design build of a project with its own controls compiles the project's
// crate as a library and names it here (`--cfg kubuno_design_project`, the generated file in
// `KUBUNO_DESIGN_PROJECT_RS` holds `extern crate <project> as _;`): linking it runs the static
// constructors its `#[derive(Component)]` classes emit, so the surface renders them with their real
// `on_paint` (docs/DESIGNER.md section 15, vskubuno docs/EVENTS.md EVT-7b).
#[cfg(kubuno_design_project)]
mod project {
    include!(env!("KUBUNO_DESIGN_PROJECT_RS"));
}

// Raw message ids (this surface does not use the `windows` crate itself - see `win32`).
const WM_SIZE: u32 = 0x0005;
const WM_SETFOCUS: u32 = 0x0007;
const WM_KILLFOCUS: u32 = 0x0008;
const WM_KEYDOWN: u32 = 0x0100;
const WM_SYSKEYDOWN: u32 = 0x0104;
const WM_DPICHANGED_AFTERPARENT: u32 = 0x02E3;
const WM_MOUSEACTIVATE: u32 = 0x0021;
const WM_LBUTTONDOWN: u32 = 0x0201;
const WM_LBUTTONUP: u32 = 0x0202;
const WM_RBUTTONUP: u32 = 0x0205;

/// What the design canvas reads off the view's text (DESIGNER.md §12): its design size and the
/// title its frame shows. Re-read whenever the text changes.
struct ViewInfo {
    size: DesignSize,
    /// The title and caption buttons the frame shows (the view's `Form` properties).
    frame: design::ViewFrameStyle,
}

impl ViewInfo {
    fn of(text: &str) -> Self {
        // An inherited view takes its size and its window from its base (`x:Inherits`).
        let text = kubuno_desktop_views::compile::designed_view_text(text);
        let parse = kubuno_desktop_views::syntax::parse(&text);
        let doc = Document::cast(parse.syntax());
        Self { size: design::design_size(doc.as_ref()), frame: design::ViewFrameStyle::read(doc.as_ref()) }
    }
}

/// A right-button release: client pixels, then screen pixels.
type RightClick = (i32, i32, i32, i32);

/// A drag of one of the canvas' scrollbar thumbs.
#[derive(Clone, Copy)]
struct ThumbDrag {
    vertical: bool,
    start_mouse: f32,
    start_offset: f32,
}

/// `(x, y)` client pixels of the surface window `hwnd` (as an `isize`) in screen pixels.
/// The in-place editor of a menu being designed (`vskubuno/docs/MENUS.md` §5, WinForms' « Type
/// Here »): typing into a slot creates a `<MenuItem>` there (`-` alone, a `<MenuSeparator>`); Enter
/// goes on to the next slot, Tab into the new item's sub-menu, Escape stops. F2 on a menu item edits
/// its `Text` in place.
struct TypeEdit {
    /// The element the new item goes into, and its index among its element children.
    parent_id: String,
    index: usize,
    /// A menu bar's slot (items left to right).
    horizontal: bool,
    /// F2: the item whose `Text` is edited (nothing is inserted).
    rename_of: Option<String>,
    text: String,
    /// Where it is drawn (the slot's box, or the item's).
    rect: Rect,
}

impl TypeEdit {
    fn new(slot: &kubuno_desktop_views::menus::TypeSlot) -> Self {
        Self { parent_id: slot.parent_id.clone(), index: slot.index, horizontal: slot.horizontal, rename_of: None, text: String::new(), rect: slot.rect }
    }

    /// The edit its text makes (`None` for an empty text): the new item (named after its text, unique
    /// in `view`), or the new `Text` of the item renamed.
    fn edit(&self, view: &str) -> Option<EditOp> {
        let text = self.text.trim();
        if text.is_empty() {
            return None;
        }
        if let Some(id) = &self.rename_of {
            return Some(EditOp::SetAttribute { element_id: id.clone(), name: "Text".into(), value: text.to_string() });
        }
        let xml = if text == "-" && !self.horizontal {
            "<MenuSeparator/>".to_string()
        } else {
            let parse = kubuno_desktop_views::syntax::parse(view);
            let taken = Document::cast(parse.syntax()).and_then(|d| d.root_element()).map(|r| kubuno_desktop_views::edit::collect_names(&r)).unwrap_or_default();
            let name = kubuno_desktop_views::menus::item_name(text, &taken);
            let escaped = text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;");
            format!("<MenuItem x:Name=\"{name}\" Text=\"{escaped}\"/>")
        };
        Some(EditOp::InsertChild { parent_id: self.parent_id.clone(), index: self.index, xml })
    }

    /// The id the item it inserts will have.
    fn new_id(&self) -> String {
        if self.parent_id.is_empty() {
            self.index.to_string()
        } else {
            format!("{}.{}", self.parent_id, self.index)
        }
    }
}

/// The in-place editor over its slot: a field with the text typed so far (« Type Here » while
/// empty) and a blinking caret.
fn paint_type_edit(c: &dyn kubuno_desktop_controls::ControlCanvas, theme: &Theme, t: &TypeEdit, now: u64) {
    let r = Rect::new(t.rect.left + 4.0, t.rect.top + 2.0, t.rect.right - 4.0, t.rect.bottom - 2.0);
    c.fill_rounded(&r, 4.0, &theme.layer_background);
    c.stroke_rounded_w(&r, 4.0, &theme.accent, 1.5);
    let f = &c.formats().body;
    let inner = Rect::new(r.left + 8.0, r.top, r.right - 8.0, r.bottom);
    let shown = kubuno_desktop_views::common::mnemonic(&t.text).0;
    if shown.is_empty() {
        c.text(&kubuno_desktop_views::menus::type_here_text(), &inner, f, &theme.text_tertiary, false);
    } else {
        c.text_ellipsis(&shown, &inner, f, &theme.text_primary);
    }
    if (now / 530).is_multiple_of(2) {
        let x = (inner.left + c.measure(&shown, f)).min(inner.right);
        let cy = (r.top + r.bottom) / 2.0;
        c.fill_rect(&Rect::new(x, cy - 8.0, x + 1.0, cy + 8.0), &theme.text_primary);
    }
}

fn client_to_screen(hwnd: isize, x: i32, y: i32) -> (i32, i32) {
    let mut p = win32::Point { x, y };
    // SAFETY: plain Win32 call on this thread's own window handle; a stale handle just fails.
    unsafe {
        let _ = win32::ClientToScreen(hwnd, &mut p);
    }
    (p.x, p.y)
}

/// A schemaless view model: whatever path the view binds, stored by name.
/// Enough for any `.kbview` a designer surface is pointed at.
#[derive(Default)]
struct BagViewModel(HashMap<String, Value>);

impl ViewModel for BagViewModel {
    fn get(&self, path: &str) -> Option<Value> {
        self.0.get(path).cloned()
    }

    fn set(&mut self, path: &str, value: Value) {
        self.0.insert(path.to_string(), value);
    }
}

/// What the probe line shows, fed by the message hooks.
#[derive(Default)]
struct Probe {
    last_key: String,
    focus_events: u32,
    size_px: (u32, u32),
    dpi_changes: u32,
}

fn parse_hwnd(s: &str) -> Option<isize> {
    match s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        Some(hex) => isize::from_str_radix(hex, 16).ok(),
        None => s.parse().ok(),
    }
}

/// Spawns the stdin reader (DSG-6 protocol, host → surface): one thread
/// blocked in `stdin().lock().lines()`, forwarding each line to the frame
/// loop over an `mpsc` channel — reading a pipe is blocking, the Win32
/// message pump inside [`host::run_with_options`] is not, so this cannot run
/// on the main thread. The sender end is dropped (and the receiver starts
/// yielding `Err`, harmlessly ignored by [`drain_host_messages`]) when stdin
/// closes — e.g. the host process exited without closing this one first;
/// [`host::on_message`]'s own parent-watchdog timer (`docs/DESIGNER.md` §7)
/// is what actually ends the run in that case, not this channel.
fn spawn_stdin_reader(waker: host::UiWaker) -> mpsc::Receiver<String> {
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("stdin-protocol".to_string())
        .spawn(move || {
            let stdin = std::io::stdin();
            for line in stdin.lock().lines() {
                match line {
                    Ok(l) => {
                        if tx.send(l).is_err() {
                            return; // The frame loop (and its receiver) is gone.
                        }
                        // A frame drains it: the surface paints only when something happens (no polling).
                        waker.wake();
                    }
                    Err(_) => return, // Pipe closed/broken.
                }
            }
        })
        .expect("spawning the stdin-protocol reader thread");
    rx
}

/// One [`SurfaceMessage`] line to stdout, flushed immediately — the host is
/// waiting on it (`RustDesignSurfaceHost.Protocol.cs`'s own
/// `BeginOutputReadLine`), and a design surface's protocol traffic is rare
/// enough (one line per click/nudge/delete, never per frame) that buffering
/// would only add latency for no throughput benefit. Never panics: a broken
/// pipe (the host is gone) is silently swallowed, same as every other
/// best-effort I/O in this crate's own `edit.rs`/`edit_bridge`-style "stale
/// input, no-op" posture.
fn send(msg: &SurfaceMessage) {
    let line = protocol::encode_surface_message(msg);
    let mut out = std::io::stdout();
    let _ = writeln!(out, "{line}");
    let _ = out.flush();
}

/// While design mode is on, the compiled view must not see real
/// mouse/keyboard interaction (`DESIGNER.md` §6 scope item 2: "user input
/// does NOT reach widgets") — a copy of the frame with the pointer parked
/// off-canvas and no button/wheel/dismiss state achieves that with no change
/// to `kubuno_desktop_controls::host` at all: every leaf's own `interact`
/// (`crate::node::ButtonNode::interact` et al.) gates everything behind
/// `hot = bounds.contains(mouse)` / `mouse_down`, so a pointer that can never
/// be "hot" and a button that is never "down" can never start a press,
/// complete a click, or take keyboard focus — a `kubuno_desktop_ui::text::TextField`
/// only starts reading typed characters once IT has focus, which a click
/// alone grants. Design mode's own click/hover/keyboard handling
/// (`DesignController`) reads the REAL frame directly, never this one.
fn neutralize_frame(f: &Frame) -> Frame {
    Frame {
        mouse: (-1.0, -1.0),
        mouse_down: false,
        right_down: false,
        wheel: (0.0, 0.0),
        click_count: 0,
        dismiss: false,
        ..*f
    }
}

/// Checks whether `code` was pressed this frame, with or without Shift held
/// — consumes whichever matched ([`host::take_key`]), so a key held down
/// does not keep re-triggering every single frame. `(pressed, shift_held)`.
fn take_arrow(code: u16) -> (bool, bool) {
    if host::take_key(code, Modifiers::SHIFT) > 0 {
        (true, true)
    } else if host::take_key(code, Modifiers::NONE) > 0 {
        (true, false)
    } else {
        (false, false)
    }
}

/// This frame's design-mode key input, read from the host's own global key
/// queue (`host::take_key`) — the one place in this file that bridges
/// `kubuno_desktop_controls::host`'s global state into [`DesignController`]'s
/// host-agnostic [`DesignKeyInput`] (see that type's own doc for why
/// `kubuno-desktop-views` itself never reads the global state directly).
fn read_design_keys() -> DesignKeyInput {
    let (left, left_shift) = take_arrow(vk::LEFT);
    let (right, right_shift) = take_arrow(vk::RIGHT);
    let (up, up_shift) = take_arrow(vk::UP);
    let (down, down_shift) = take_arrow(vk::DOWN);
    DesignKeyInput {
        escape: host::take_key(vk::ESCAPE, Modifiers::NONE) > 0,
        delete: host::take_key(vk::DELETE, Modifiers::NONE) > 0,
        left,
        right,
        up,
        down,
        shift: left_shift || right_shift || up_shift || down_shift,
        select_all: host::take_key(vk::letter('A'), Modifiers::CTRL) > 0,
    }
}

/// Sends the edits of one keyboard gesture: a single op as `editRequest`, several (a multi-selection
/// nudged or deleted, or X and Y together) as ONE `editRequests` batch - one undo unit (DESIGNER.md §13).
fn send_ops(mut ops: Vec<EditOp>) {
    if ops.len() > 1 {
        let gesture = if ops.iter().any(|op| matches!(op, EditOp::RemoveElement { .. })) { Gesture::Delete } else { Gesture::Move };
        eprintln!("[embed] proto editRequests {ops:?} gesture={gesture:?}");
        send(&SurfaceMessage::EditRequests { ops, gesture });
    } else if let Some(op) = ops.pop() {
        eprintln!("[embed] proto editRequest {op:?}");
        send(&SurfaceMessage::EditRequest { op });
    }
}

/// What the preview currently shows (vskubuno docs/DESIGNER.md section 17): the state of the last
/// reload, the elements shown differently from their text, and the last `renderStatus` sent.
#[derive(Default)]
struct RenderStatus {
    state: Option<DesignRenderState>,
    issues: Vec<DesignIssue>,
    last_sent: Option<String>,
}

impl RenderStatus {
    /// Reloads `text` tolerantly (a view with errors still shows its valid part; a malformed text
    /// keeps the last good preview) and tells the host what the preview shows, when that changed.
    fn reload(&mut self, runtime: &mut Runtime, text: &str) {
        let r = runtime.reload_for_design(text);
        // A stale preview keeps the markers of the text it was built from.
        if r.state != DesignRenderState::Stale {
            self.issues = r.issues;
        }
        self.state = Some(r.state);
        let msg = SurfaceMessage::RenderStatus { state: r.state.into(), diagnostics: protocol::wire_diagnostics(text, &r.diagnostics) };
        let line = protocol::encode_surface_message(&msg);
        if self.last_sent.as_deref() != Some(line.as_str()) {
            eprintln!("[embed] proto renderStatus {:?} ({} diagnostics)", r.state, r.diagnostics.len());
            send(&msg);
            self.last_sent = Some(line);
        }
    }

    fn stale(&self) -> bool {
        self.state == Some(DesignRenderState::Stale)
    }
}

/// Paints, in `client`, why there is nothing to show: a view whose text has no element at all.
fn paint_empty_view(c: &dyn kubuno_desktop_controls::ControlCanvas, client: Rect, theme: &Theme) {
    let r = Rect::new(client.left + 16.0, client.top + 16.0, client.right - 16.0, client.bottom);
    let text = kubuno_desktop_views::messages::tr(
        "Nothing to preview: the view has no element yet (see the errors above the design surface).",
        "Rien à afficher : la vue n'a encore aucun élément (voir les erreurs au-dessus de la surface de conception).",
    );
    c.text(&text, &r, &c.formats().body, &theme.text_secondary, false);
}

/// Dims a stale preview (the last good one, shown while the text does not parse).
fn paint_stale_veil(c: &dyn kubuno_desktop_controls::ControlCanvas, client: Rect, theme: &Theme) {
    let mut veil = theme.window_background;
    veil.a = 0.55;
    c.fill_rect(&client, &veil);
}

/// The warning marker of each element the preview shows differently from its text (an attribute
/// ignored, children not shown): the element's bounds, and the badge inside their top-right corner.
/// One per element, with every issue of that element.
fn issue_markers<'i>(layout: &LayoutMap, issues: &'i [DesignIssue]) -> Vec<(Rect, Rect, Vec<&'i DesignIssue>)> {
    let mut out: Vec<(Rect, Rect, Vec<&'i DesignIssue>)> = Vec::new();
    for issue in issues.iter().filter(|i| !i.placeholder) {
        if let Some((_, _, list)) = out.iter_mut().find(|(_, _, l)| l[0].element_id == issue.element_id) {
            list.push(issue);
            continue;
        }
        let Some(entry) = layout.get(&issue.element_id) else { continue };
        let b = entry.bounds;
        let badge = Rect::new(b.right - 17.0, b.top + 1.0, b.right - 1.0, b.top + 17.0);
        out.push((b, badge, vec![issue]));
    }
    out
}

/// Paints the markers of [`issue_markers`], and the messages of the one under the pointer.
fn paint_issue_markers(c: &dyn kubuno_desktop_controls::ControlCanvas, theme: &Theme, layout: &LayoutMap, issues: &[DesignIssue], mouse: (f32, f32)) {
    let mut hovered: Option<(Rect, String)> = None;
    for (bounds, badge, list) in issue_markers(layout, issues) {
        // The element itself is outlined, so the marker is never taken for its neighbour's.
        for dash in design::dashed_outline(bounds, 3.0, 2.0, 1.0) {
            c.fill_rect(&dash, &theme.warning);
        }
        c.fill_rounded(&badge, 3.0, &theme.window_background);
        c.stroke_rounded(&badge, 3.0, &theme.warning);
        c.vector_icon("AlertTriangle", &Rect::new(badge.left + 2.0, badge.top + 2.0, badge.right - 2.0, badge.bottom - 2.0), 12.0, &theme.warning);
        if badge.contains(mouse.0, mouse.1) {
            let text = list.iter().map(|i| kubuno_desktop_views::messages::localize(&i.message)).collect::<Vec<_>>().join("\n");
            hovered = Some((badge, text));
        }
    }
    if let Some((badge, text)) = hovered {
        // Plain text: the backquotes around identifiers are for the Error List, not a tooltip.
        let text = text.replace('`', "");
        let format = &c.formats().caption;
        let lines = text.lines().count().max(1) as f32;
        let width = text.lines().map(|l| c.measure(l, format)).fold(0.0f32, f32::max) + 20.0;
        let tip = Rect::new(badge.left, badge.bottom + 4.0, badge.left + width, badge.bottom + 12.0 + 18.0 * lines);
        c.fill_rounded(&tip, 4.0, &theme.tooltip_background);
        c.text(&text, &Rect::new(tip.left + 8.0, tip.top + 4.0, tip.right - 8.0, tip.bottom - 4.0), &c.formats().caption, &theme.tooltip_foreground, false);
    }
}

/// Version of the `surfaceInfo` handshake below; the host refuses a surface that does not send it.
/// 2: `kubuno_desktop_ui` is linked statically (no DLL path or hash to check any more); version 1 surfaces
/// were linked against a `kubuno_desktop_ui-<hash>.dll` and are refused.
const SURFACE_INFO_VERSION: u32 = 2;

/// The handshake (vskubuno docs/DESIGNER.md section 15), the first line on stdout. The surface links
/// `kubuno_desktop_ui` statically, from the very rlibs the project's own build produced, so there is no DLL to
/// mismatch: the line only tells the host that this surface speaks the current protocol.
fn send_surface_info() {
    let line = format!("{{\"type\":\"surfaceInfo\",\"version\":{SURFACE_INFO_VERSION}}}");
    eprintln!("[embed] proto {line}");
    let mut out = std::io::stdout();
    let _ = writeln!(out, "{line}");
    let _ = out.flush();
}

pub fn main() -> ExitCode {
    // `--export-registry`: the registry this surface knows (the built-in elements and, for a design
    // build that links the project, its own controls), as `kubuno/registry` exports it — what
    // Visual Studio reads after a build for the Toolbox's project tab (EVT-7b). Nothing else runs.
    if env::args().any(|a| a == "--export-registry") {
        let mut out = std::io::stdout();
        let _ = writeln!(out, "{}", kubuno_desktop_views::registry::export::export_json());
        let _ = out.flush();
        return ExitCode::SUCCESS;
    }
    send_surface_info();
    // The designer: `d:` design-time attributes replace their run-time ones (`d:Visible="false"`, `d:Text="…"`).
    kubuno_desktop_views::design::set_design_time(true);
    // A design surface is not the running application: the paint-debug overlay (EVT-8) is a
    // run-time diagnostic and never shows here, whatever `KUBUNO_PAINT_DEBUG` Visual Studio's
    // Debug > Kubuno > Paint debug left in the environment this process inherited, and whatever
    // "Kubuno.PaintDebug" message it broadcasts to every Kubuno window later (its layout bounds
    // boxed every Label and TextField of the designed view).
    host::paint_debug::set_flags(host::paint_debug::PaintDebugFlags::OFF);
    host::on_message_value(host::paint_debug::message(), |_| Some(0));
    let mut parent = None;
    let mut path = None;
    // `--debug-probe`: paint the spike's diagnostic chrome (the probe line and
    // the Save/Menu demo buttons driving the tabOut/popup checks). Only the
    // DSG-7 spike (`spikes/HwndHostSpike`) passes it; the Visual Studio
    // designer surface shows the view alone.
    let mut debug_probe = false;
    let mut args = env::args().skip(1);
    while let Some(a) = args.next() {
        if a == "--debug-probe" {
            debug_probe = true;
        } else if a == "--parent" {
            match args.next().as_deref().and_then(parse_hwnd) {
                Some(h) => parent = Some(h),
                None => {
                    eprintln!("view_embed: --parent needs a window handle");
                    return ExitCode::FAILURE;
                }
            }
        } else {
            path = Some(a);
        }
    }

    let mut watcher = path.map(FileWatcher::new);
    let mut runtime = Runtime::new();
    // `current_text` mirrors whatever the surface last loaded (`setText`, or
    // the file watcher) — design mode's own nudge logic needs to re-parse it
    // on demand to read an element's CURRENT literal `X`/`Y` before offsetting
    // it (see `kubuno_desktop_views::design::DesignController::handle_keys`'s own
    // doc); nothing else in this file needs a second copy of the text.
    let mut current_text = String::new();
    if let Some(w) = watcher.as_mut() {
        if w.poll(&mut runtime) {
            current_text = std::fs::read_to_string(w.path()).unwrap_or_default();
        }
    }

    let mut vm = BagViewModel::default();
    let mut handlers: HandlerTable = handlers! {
        "save_clicked" => |vm, _v| { vm.set("Status", Value::Str("save_clicked fired".to_string())); },
        "offline_toggled" => |vm, v| { vm.set("Offline", v); },
    };

    let stdin_rx = spawn_stdin_reader(host::ui_waker());
    let mut design = DesignController::new();
    let mut layout_map = LayoutMap::new();
    let mut last_sent_selection = Selection::new();
    // A Layout toolbar / Format menu command from the host, resolved after this frame's paint (it
    // needs the fresh layout map), like the toolbox messages below.
    let mut pending_format: Option<FormatCommand> = None;
    // DSG-9 toolbox drag/drop (`kubuno_desktop_views::protocol::HostMessage::{DragEnter,
    // DragOver, Drop, DragLeave}`). `dragOver`/`drop` carry a position that
    // must be resolved against THIS frame's freshly-painted `layout_map`, not
    // the (possibly stale) one from whenever the stdin line actually arrived
    // — so, like the arrow-nudge `doc` parse below, they are only STORED here
    // and processed after this frame's paint pass.
    let mut toolbox = ToolboxController::new();
    let mut pending_drag_over: Option<(f32, f32)> = None;
    let mut pending_drop: Option<(f32, f32)> = None;
    // The design canvas (DESIGNER.md §12): the view's size/title, the canvas scroll position, an
    // in-progress resize of the view's frame, the size just written (shown until the new text
    // arrives, with a deadline), and a scrollbar thumb drag.
    let mut view_info = ViewInfo::of(&current_text);
    // What the preview shows of the current text (docs/DESIGNER.md section 17).
    let mut status = RenderStatus::default();
    let mut scroll = (0.0f32, 0.0f32);
    let mut frame_drag: Option<FrameResize> = None;
    let mut pending_size: Option<((f32, f32), u64)> = None;
    let mut thumb_drag: Option<ThumbDrag> = None;
    // Right-button releases (client px, screen px), recorded by the window procedure.
    let right_clicks: Rc<RefCell<Vec<RightClick>>> = Rc::new(RefCell::new(Vec::new()));
    // This surface's own window (for the keyboard context menu's screen position).
    let surface_hwnd = Rc::new(Cell::new(0isize));

    let probe = Rc::new(RefCell::new(Probe::default()));
    {
        let p = probe.clone();
        host::on_message(WM_SETFOCUS, move |_| {
            p.borrow_mut().focus_events += 1;
            eprintln!("[embed] WM_SETFOCUS");
            None
        });
        host::on_message(WM_KILLFOCUS, |a| {
            eprintln!("[embed] WM_KILLFOCUS (to {:#x})", a.wparam.0);
            None
        });
        for msg in [WM_KEYDOWN, WM_SYSKEYDOWN] {
            let p = probe.clone();
            host::on_message(msg, move |a| {
                let name = format!(
                    "vk {:#04x}{}",
                    a.wparam.0,
                    if msg == WM_SYSKEYDOWN { " (sys)" } else { "" }
                );
                eprintln!("[embed] key {name}");
                p.borrow_mut().last_key = name;
                None
            });
        }
        let p = probe.clone();
        host::on_message(WM_SIZE, move |a| {
            let (w, h) = (
                (a.lparam.0 & 0xFFFF) as u32,
                ((a.lparam.0 >> 16) & 0xFFFF) as u32,
            );
            p.borrow_mut().size_px = (w, h);
            eprintln!("[embed] WM_SIZE {w}x{h}");
            None
        });
        host::on_message(WM_LBUTTONDOWN, |_| {
            eprintln!("[embed] WM_LBUTTONDOWN");
            None
        });
        host::on_message(WM_LBUTTONUP, |_| {
            eprintln!("[embed] WM_LBUTTONUP");
            None
        });
        let clicks = right_clicks.clone();
        host::on_message(WM_RBUTTONUP, move |a| {
            // Signed 16-bit client coordinates (GET_X_LPARAM / GET_Y_LPARAM).
            let x = (a.lparam.0 & 0xFFFF) as u16 as i16 as i32;
            let y = ((a.lparam.0 >> 16) & 0xFFFF) as u16 as i16 as i32;
            let (sx, sy) = client_to_screen(a.hwnd.0 as isize, x, y);
            clicks.borrow_mut().push((x, y, sx, sy));
            // SAFETY: invalidating our own window just schedules the next frame.
            unsafe {
                let _ = win32::InvalidateRect(a.hwnd.0 as isize, std::ptr::null(), 0);
            }
            None
        });
        host::on_message(WM_MOUSEACTIVATE, |_| {
            eprintln!("[embed] WM_MOUSEACTIVATE");
            None
        });
        let p = probe.clone();
        host::on_message(WM_DPICHANGED_AFTERPARENT, move |_| {
            p.borrow_mut().dpi_changes += 1;
            eprintln!("[embed] WM_DPICHANGED_AFTERPARENT");
            None
        });
    }
    // Visual Studio Toolbox drags that land straight on this window (see `ole_drop`).
    let ole_shared = Rc::new(ole_drop::Shared::default());
    let ole_shared_for_ready = ole_shared.clone();
    let surface_hwnd_for_ready = surface_hwnd.clone();
    host::on_ready(move |hwnd| {
        surface_hwnd_for_ready.set(hwnd.0 as isize);
        ole_drop::register(hwnd.0 as isize, ole_shared_for_ready);
        // The handshake line a hosting process can wait for.
        eprintln!(
            "[embed] ready hwnd={:#x} parent={:?}",
            hwnd.0 as usize, parent
        );
    });

    // « Type Here » (MENUS.md §5): the slots of the menus shown open on the surface (last frame's),
    // and the in-place editor typing into one of them (or renaming a menu item, F2).
    let mut type_slots: Vec<kubuno_desktop_views::menus::TypeSlot> = Vec::new();
    let mut typing: Option<TypeEdit> = None;
    let menu_open = Rc::new(Cell::new(false));
    let was_down = Cell::new(false);
    // The designer's zoom request (`setZoom`): a factor, or 0 for « fit ».
    let zoom_request = Cell::new(1.0f32);
    // Frames that ran with no input and no host message, counted over 5-second windows: an idle
    // surface runs none, so a sustained count means something keeps asking for repaints.
    let idle_frames = Cell::new((0u64, 0u32));
    let mut ring = FocusRing::new();
    let save_id: FocusId = FocusId::of("save_btn");
    let menu_id: FocusId = FocusId::of("menu_btn");
    let mut opts = HostOptions::new("Kubuno view embed", 800, 600, Theme::light());
    opts.parent = parent;

    let result = host::run_with_options(opts, move |c, f: &Frame| {
        // Only the standalone file watcher polls (every 250 ms); under a host, the stdin reader wakes
        // the frame loop when a message arrives, so an idle surface does not repaint at all.
        if watcher.is_some() {
            host::request_repaint_after(250);
        }
        if let Some(w) = watcher.as_mut() {
            if w.poll(&mut runtime) {
                current_text = std::fs::read_to_string(w.path()).unwrap_or_default();
                view_info = ViewInfo::of(&current_text);
                pending_size = None;
            }
        }

        // DSG-6 protocol: drain whatever the host sent since the last frame.
        // Never more than a handful of lines even under a fast typist —
        // `RustDesignSurfaceHost` debounces its own `setText` pushes
        // (`DESIGNER.md` §2) — so draining the whole backlog every frame
        // costs nothing measurable.
        let mut incoming: Vec<HostMessage> = Vec::new();
        while let Ok(line) = stdin_rx.try_recv() {
            match protocol::parse_host_message(&line) {
                Some(msg) => incoming.push(msg),
                None => eprintln!("[embed] proto: ignoring unrecognised/malformed line: {line}"),
            }
        }
        // A Toolbox drag received by this window's own OLE drop target is the same traffic.
        incoming.extend(ole_shared.queue.borrow_mut().drain(..));
        let quiet = incoming.is_empty() && host::events().is_empty();
        let (since, count) = idle_frames.get();
        let now = host::now_ms();
        if now.saturating_sub(since) >= 5_000 {
            if count >= 20 {
                eprintln!("[embed] {count} frames with no input or message in 5 s: something keeps asking for repaints");
            }
            idle_frames.set((now, u32::from(quiet)));
        } else if quiet {
            idle_frames.set((since, count + 1));
        }
        for msg in incoming {
            match msg {
                HostMessage::SetText { text, base_dir } => {
                    eprintln!("[embed] proto setText ({} bytes)", text.len());
                    if let Some(dir) = base_dir {
                        runtime.set_base_dir(Some(std::path::PathBuf::from(dir)));
                    }
                    current_text = text;
                    status.reload(&mut runtime, &current_text);
                    view_info = ViewInfo::of(&current_text);
                    pending_size = None;
                }
                HostMessage::SetDesignMode { on } => {
                    eprintln!("[embed] proto setDesignMode on={on}");
                    design.set_enabled(on);
                }
                HostMessage::Select { id } => {
                    eprintln!("[embed] proto select {id:?}");
                    design.set_selected(id);
                }
                HostMessage::SelectMany { ids, primary } => {
                    eprintln!("[embed] proto selectMany {ids:?} primary={primary:?}");
                    design.set_selection(ids, primary);
                }
                HostMessage::Format { command } => {
                    eprintln!("[embed] proto format {command:?}");
                    pending_format = Some(command);
                }
                HostMessage::DragEnter { component } => {
                    eprintln!("[embed] proto dragEnter {component}");
                    toolbox.drag_enter(component);
                }
                HostMessage::DragOver { x, y } => {
                    // Resolved after this frame's paint — see this
                    // function's own comment on `pending_drag_over`.
                    pending_drag_over = Some((x, y));
                }
                HostMessage::Drop { x, y } => {
                    pending_drop = Some((x, y));
                }
                HostMessage::DragLeave => {
                    eprintln!("[embed] proto dragLeave");
                    toolbox.drag_leave();
                    pending_drag_over = None;
                    pending_drop = None;
                    send(&SurfaceMessage::DropTargetChanged { target: None });
                }
                HostMessage::ProjectComponents { components } => {
                    // EVT-7b: the project's controls this surface does not link become placeholders.
                    eprintln!("[embed] proto projectComponents ({} classes)", components.len());
                    kubuno_desktop_views::registry::set_declared(components);
                    if !current_text.is_empty() {
                        // Always answered: the host reads the banner again with its registry, now loaded.
                        status.last_sent = None;
                        status.reload(&mut runtime, &current_text);
                    }
                }
                HostMessage::SetDesignOptions { container_outlines } => {
                    eprintln!("[embed] proto setDesignOptions containerOutlines={container_outlines}");
                    design::set_container_outlines(container_outlines);
                }
                HostMessage::SetZoom { zoom } => {
                    eprintln!("[embed] proto setZoom {zoom}");
                    zoom_request.set(if zoom.is_finite() && zoom >= 0.0 { zoom } else { 1.0 });
                    host::request_repaint_after(0);
                }
                HostMessage::SetCanvasBackground { color } => {
                    // The IDE's theme: the canvas around the view follows a live switch.
                    if let Some(rgb) = kubuno_desktop_views::protocol::parse_rgb(&color) {
                        design::set_canvas_background(rgb);
                        host::request_repaint_after(0);
                    }
                }
                HostMessage::SetResources { culture, sets } => {
                    // The project's `.kbres` files and the design-time language (vskubuno docs/RESOURCES.md).
                    eprintln!("[embed] proto setResources ({} sets, culture {culture:?})", sets.len());
                    kubuno_desktop_views::protocol::apply_resources(&culture, &sets);
                    // The frame's title may be a `{Res …}`: read again in the new culture.
                    view_info = ViewInfo::of(&current_text);
                    host::request_repaint_after(0);
                }
            }
        }

        let theme = c.theme();
        // A rising edge of `Frame::mouse_down`. The host itself now
        // guarantees at least one frame of `mouse_down == true` for every
        // press, even a quick one whose release already arrived before this
        // frame was built (`kubuno_desktop_controls::host`'s own press-latching), so
        // this no longer needs a local latch on top of it.
        let pressed = f.mouse_down && !was_down.get();
        was_down.set(f.mouse_down);

        // DSG-7 "tabOut" protocol: a Tab/Shift+Tab that would run this tiny
        // demo ring past its last/first control is consumed HERE, before
        // `ring.begin_frame` gets to it, and turned into
        // `host::notify_tab_out` instead of wrapping around — see the module
        // doc for why this ring (not the compiled view's own) is what proves
        // the protocol out.
        // The in-place menu editor's Tab (into the new item's sub-menu) is its own, not the focus ring's.
        let typing_tab = typing.is_some() && host::take_key(vk::TAB, Modifiers::NONE) > 0;
        let tab_fwd = host::key_pressed(vk::TAB, Modifiers::NONE);
        let tab_back = host::key_pressed(vk::TAB, Modifiers::SHIFT);
        if tab_fwd || tab_back {
            let backward = tab_back;
            let at_edge = match ring.focused() {
                Some(id) if backward => id == save_id,
                Some(id) => id == menu_id,
                None => false,
            };
            if at_edge {
                let mods = if backward {
                    Modifiers::SHIFT
                } else {
                    Modifiers::NONE
                };
                if host::take_key(vk::TAB, mods) > 0 {
                    ring.blur();
                    host::notify_tab_out(backward);
                    eprintln!("[embed] tab-out backward={backward}");
                }
            }
        }
        ring.begin_frame(f);

        if debug_probe {
            // Probe line.
            let top = Rect::new(8.0, 4.0, f.size.0 - 96.0, 24.0);
            let pr = probe.borrow();
            let line = format!(
            "parent={} dpi={:.0} focused={} focus#={} key={} size={}x{} dpiChanges={} design={}",
            parent.map_or("none".to_string(), |h| format!("{h:#x}")),
            f.scale * 96.0,
            f.window_focused,
            pr.focus_events,
            if pr.last_key.is_empty() { "-" } else { &pr.last_key },
            pr.size_px.0,
            pr.size_px.1,
            pr.dpi_changes,
            design.enabled(),
        );
            drop(pr);
            c.text(
                &line,
                &top,
                &c.formats().caption,
                &theme.text_secondary,
                false,
            );

            // "Save" button: the FIRST control of the demo focus ring (Shift+Tab
            // off it is a tab-out backward) — purely a keyboard-protocol prop, its
            // click does nothing on its own.
            let save = Rect::new(f.size.0 - 176.0, 2.0, f.size.0 - 96.0, 26.0);
            let save_state = ring.register(save_id, save);
            let save_hot = save.contains(f.mouse.0, f.mouse.1);
            c.fill_rounded(
                &save,
                4.0,
                if save_hot {
                    &theme.accent_light
                } else {
                    &theme.surface_2
                },
            );
            if save_state.visible {
                c.stroke_rounded(
                    &Rect::new(
                        save.left - 1.0,
                        save.top - 1.0,
                        save.right + 1.0,
                        save.bottom + 1.0,
                    ),
                    5.0,
                    &theme.accent,
                );
            }
            c.text(
                "Save",
                &Rect::new(save.left + 10.0, save.top + 4.0, save.right, save.bottom),
                &c.formats().body,
                &theme.text_primary,
                false,
            );

            // "Menu" button: toggles a popup that overflows the child window; also
            // the LAST control of the demo focus ring (Tab off it tabs out
            // forward).
            let btn = Rect::new(f.size.0 - 88.0, 2.0, f.size.0 - 8.0, 26.0);
            let menu_state = ring.register(menu_id, btn);
            let hot = btn.contains(f.mouse.0, f.mouse.1);
            c.fill_rounded(
                &btn,
                4.0,
                if hot {
                    &theme.accent_light
                } else {
                    &theme.surface_2
                },
            );
            if menu_state.visible {
                c.stroke_rounded(
                    &Rect::new(
                        btn.left - 1.0,
                        btn.top - 1.0,
                        btn.right + 1.0,
                        btn.bottom + 1.0,
                    ),
                    5.0,
                    &theme.accent,
                );
            }
            c.text(
                "Menu ▾",
                &Rect::new(btn.left + 10.0, btn.top + 4.0, btn.right, btn.bottom),
                &c.formats().body,
                &theme.text_primary,
                false,
            );
            let menu = Rect::new(
                btn.right - 220.0,
                btn.bottom + 2.0,
                btn.right + 120.0,
                btn.bottom + 2.0 + f.size.1,
            );
            if pressed {
                if hot {
                    menu_open.set(!menu_open.get());
                } else if menu_open.get() {
                    if menu.contains(f.mouse.0, f.mouse.1) {
                        // The pointer reported in client DIP even where the popup
                        // overflows the child: the row is plain arithmetic.
                        let row = ((f.mouse.1 - menu.top - 4.0) / 28.0).floor() as i32 + 1;
                        eprintln!(
                            "[embed] popup item {row} clicked at ({:.0}, {:.0}) dip",
                            f.mouse.0, f.mouse.1
                        );
                    }
                    menu_open.set(false);
                }
            }
            if f.dismiss || host::key_pressed(vk::ESCAPE, Modifiers::default()) {
                menu_open.set(false);
            }
            if menu_open.get() {
                let hover = f.mouse;
                host::popup(menu, move |pc| {
                    let t = pc.theme();
                    let r = Rect::new(0.0, 0.0, menu.right - menu.left, menu.bottom - menu.top);
                    pc.fill_rounded(&r, 6.0, &t.card_background);
                    pc.stroke_rounded(&r, 6.0, &t.card_stroke);
                    for i in 0..8 {
                        let row = Rect::new(
                            4.0,
                            4.0 + i as f32 * 28.0,
                            r.right - 4.0,
                            30.0 + i as f32 * 28.0,
                        );
                        let abs = Rect::new(
                            menu.left + row.left,
                            menu.top + row.top,
                            menu.left + row.right,
                            menu.top + row.bottom,
                        );
                        if abs.contains(hover.0, hover.1) {
                            pc.fill_rounded(&row, 4.0, &t.accent_light);
                        }
                        let label = format!("Popup item {} (overflows the child)", i + 1);
                        pc.text(
                            &label,
                            &Rect::new(row.left + 8.0, row.top + 5.0, row.right, row.bottom),
                            &pc.formats().body,
                            &t.text_primary,
                            false,
                        );
                    }
                });
            }
        }

        // The spike's chrome occupies the top 32 DIP; without it the view
        // starts right at the top margin (design mode: the canvas starts at 0).
        let chrome_top = if debug_probe { 32.0 } else { 0.0 };
        let design_on = design.enabled();
        if !design_on {
            let body_top = f32::max(chrome_top, 16.0);
            let body = Rect::new(16.0, body_top, (f.size.0 - 16.0).max(16.0), (f.size.1 - 8.0).max(body_top));
            if runtime.has_view() {
                let _ = runtime.frame(c, f, &mut vm, &mut handlers, body);
                if status.stale() {
                    paint_stale_veil(c, body, theme);
                }
            } else if status.state == Some(DesignRenderState::Empty) && !current_text.trim().is_empty() {
                paint_empty_view(c, body, theme);
            }
        } else {
            layout_map.clear();
            let mouse = f.mouse;
            let now = host::now_ms();
            if pending_size.is_some_and(|(_, until)| now > until) {
                pending_size = None;
            }

            // ── The design canvas (DESIGNER.md §12): the view in a window frame at its design
            // size - live while a frame handle is dragged - on a scrollable neutral canvas.
            let (w, h) = match (frame_drag, pending_size) {
                (Some(drag), _) => drag.size_at(mouse.0, mouse.1),
                (None, Some((size, _))) => size,
                (None, None) => (view_info.size.width, view_info.size.height),
            };
            let (content_w, content_h) = FrameLayout::canvas_extent_titled(w, h, view_info.frame.title_height());
            // The designer's zoom: a factor, or « fit » (0): the largest one up to 100 % that shows the whole
            // window. The host renders at DPI × zoom, so the frame's DIP size here is already zoomed.
            if frame_drag.is_none() {
                let wanted = if zoom_request.get() > 0.0 {
                    zoom_request.get()
                } else {
                    let (uw, uh) = (f.size.0 * host::zoom(), (f.size.1 - chrome_top).max(1.0) * host::zoom());
                    (uw / content_w.max(1.0)).min(uh / content_h.max(1.0)).min(1.0)
                };
                host::set_zoom((wanted * 100.0).round() / 100.0);
            }
            let (mut view_w, mut view_h) = (f.size.0, (f.size.1 - chrome_top).max(0.0));
            let mut need_h = content_w > view_w;
            if need_h {
                view_h -= SCROLLBAR_SIZE;
            }
            let need_v = content_h > view_h;
            if need_v {
                view_w -= SCROLLBAR_SIZE;
                if !need_h && content_w > view_w {
                    need_h = true;
                    view_h -= SCROLLBAR_SIZE;
                }
            }
            if frame_drag.is_none() {
                let (wx, wy) = f.wheel_dip();
                if f.mods.shift {
                    scroll.0 += wx + wy;
                } else {
                    scroll.0 += wx;
                    scroll.1 += wy;
                }
            }
            scroll.0 = design::clamp_scroll(scroll.0, content_w, view_w);
            scroll.1 = design::clamp_scroll(scroll.1, content_h, view_h);
            let viewport = Rect::new(0.0, chrome_top, view_w, chrome_top + view_h);
            let v_track = Rect::new(view_w, chrome_top, view_w + SCROLLBAR_SIZE, chrome_top + view_h);
            let h_track = Rect::new(0.0, chrome_top + view_h, view_w, chrome_top + view_h + SCROLLBAR_SIZE);
            let v_thumb = if need_v { design::scrollbar_thumb(v_track, false, content_h, view_h, scroll.1) } else { None };
            let h_thumb = if need_h { design::scrollbar_thumb(h_track, true, content_w, view_w, scroll.0) } else { None };
            let in_scrollbars =
                (need_v && v_track.contains(mouse.0, mouse.1)) || (need_h && h_track.contains(mouse.0, mouse.1));

            // The window as it will run: the same frame, band and caption painter as the host's
            // (`kubuno_desktop_controls::window_chrome`), the view's title-bar controls laid out in the band.
            let frame = view_info.frame.layout(CANVAS_MARGIN - scroll.0, chrome_top + CANVAS_MARGIN - scroll.1, w, h);
            c.push_clip(&viewport);
            design::paint_canvas(c, viewport);
            design::paint_view_frame_styled(c, theme, &frame, &view_info.frame, design.selected() == Some(""));
            let design_chrome = view_info.frame.design_chrome(theme, &frame);
            // The band as the view filled it (its regions' widths): where a Toolbox drop over the title bar goes.
            let band_chrome = design_chrome.clone();
            kubuno_desktop_views::window::set_design_chrome(design_chrome);
            // The view is clipped to the window's corners, as the running window clips its page.
            let corner_radius = view_info.frame.corner_radius(theme);
            c.push_clip_rounded(&frame.outer, corner_radius);
            let mut design_glyphs: Vec<kubuno_desktop_views::virtual_regions::DesignGlyph> = Vec::new();
            if runtime.has_view() {
                let neutered = neutralize_frame(f);
                // What a ribbon shows in the designer follows the selection (the selected tab is active).
                kubuno_desktop_views::virtual_regions::set_design_selection(design.selection().ids());
                // A menu row dragged over another row opens that row's sub-menu (to drop into it).
                kubuno_desktop_views::menus::set_drag_hover(if design.is_dragging() { kubuno_desktop_views::menus::hover_at(mouse.0, mouse.1) } else { None });
                let _ = runtime.frame_with_design(c, &neutered, &mut vm, &mut handlers, frame.client, Some(&mut layout_map));
                design_glyphs = kubuno_desktop_views::virtual_regions::take_design_glyphs();
                type_slots = kubuno_desktop_views::menus::take_type_slots();
                if status.stale() {
                    // The last good preview, dimmed: the text being typed does not parse yet.
                    paint_stale_veil(c, frame.client, theme);
                }
            } else if status.state == Some(DesignRenderState::Empty) && !current_text.trim().is_empty() {
                // Nothing at all could be recovered: the empty frame at the view's size, and why.
                paint_empty_view(c, frame.client, theme);
            }
            // A ribbon docked at the top colours the title bar (the window's band continues its tab strip).
            match kubuno_desktop_views::virtual_regions::take_design_caption() {
                Some(colors) => kubuno_desktop_views::virtual_regions::paint_design_caption(c, &frame, &view_info.frame, colors),
                None => design::paint_view_caption(c, &frame, &view_info.frame),
            }
            toolbox.set_title_band(band_chrome.as_ref().map(|d| {
                kubuno_desktop_controls::window_chrome::layout(&d.style, d.bounds, d.has_icon, d.buttons, kubuno_desktop_views::window::declared_slots())
            }));
            kubuno_desktop_views::window::set_design_chrome(None);
            c.pop_clip_rounded();
            // The view selected: the title bar's smart tag (its tasks: a button in one of its regions, the header's
            // standard items), at the top-right corner of the window, outside it like Windows Forms' smart tags.
            if design.selected() == Some("") {
                if let Some(band) = band_chrome.as_ref() {
                    let b = band.bounds;
                    let tag = Rect::new(b.right - 16.0, b.top - 18.0, b.right - 2.0, b.top - 4.0);
                    kubuno_desktop_views::virtual_regions::paint_smart_tag(c, tag);
                    design_glyphs.push(kubuno_desktop_views::virtual_regions::DesignGlyph { rect: tag, element_id: String::new(), menu: "tasks" });
                }
            }

            // ── Pointer: scrollbars, then the frame's resize handles, then the view.
            let busy = frame_drag.is_some() || thumb_drag.is_some() || design.is_dragging();
            let hot_handle = if busy || in_scrollbars { None } else { frame.handle_at(mouse.0, mouse.1) };
            if let Some(handle) = frame_drag.map(|d| d.handle).or(hot_handle) {
                host::set_cursor(match handle {
                    FrameHandle::Right => Cursor::ResizeEW,
                    FrameHandle::Bottom => Cursor::ResizeNS,
                    FrameHandle::Corner => Cursor::ResizeNWSE,
                });
            }
            if !busy {
                design.update_hover(&layout_map, mouse.0, mouse.1);
            }

            let mut double_click: Option<String> = None;
            let mut glyph_menu: Option<SurfaceMessage> = None;
            let mut go_to_source: Option<protocol::WireDiagnostic> = None;
            let glyph_hit = if pressed && !in_scrollbars && hot_handle.is_none() { design_glyphs.iter().find(|g| g.rect.contains(mouse.0, mouse.1)).cloned() } else { None };
            // A click outside the in-place editor ends it: what was typed is kept (WinForms).
            if pressed && typing.as_ref().is_some_and(|t| !t.rect.contains(mouse.0, mouse.1)) {
                if let Some(op) = typing.take().and_then(|t| t.edit(&current_text)) {
                    eprintln!("[embed] proto editRequest(type here) {op:?}");
                    send(&SurfaceMessage::EditRequest { op });
                }
            }
            // A « Type Here » slot of a menu shown on the surface: the in-place editor opens in it.
            let slot_hit = if pressed && !in_scrollbars && hot_handle.is_none() && glyph_hit.is_none() {
                type_slots.iter().find(|s| s.rect.contains(mouse.0, mouse.1)).cloned()
            } else {
                None
            };
            // A warning marker (docs/DESIGNER.md section 17): selects its element, then shows the finding in the XML.
            let marker_hit = if pressed && !in_scrollbars && hot_handle.is_none() && !status.stale() {
                issue_markers(&layout_map, &status.issues).into_iter().find(|(_, badge, _)| badge.contains(mouse.0, mouse.1)).map(|(_, _, list)| list[0].clone())
            } else {
                None
            };
            if let Some(issue) = marker_hit {
                design.cancel_drag();
                design.set_selected(Some(issue.element_id.clone()));
                if let Some(range) = issue.range {
                    let mut d = protocol::wire_diagnostics(&current_text, &[kubuno_desktop_views::syntax::Diagnostic { range, line: 1, column: 1, message: issue.message.clone() }]);
                    go_to_source = d.pop();
                }
            } else if let Some(glyph) = glyph_hit {
                // A ribbon's « + » or smart tag: select its element and ask for its menu, under the glyph.
                design.cancel_drag();
                design.set_selected(Some(glyph.element_id.clone()));
                let (x, y) = (glyph.rect.left, glyph.rect.bottom);
                let (screen_x, screen_y) = client_to_screen(surface_hwnd.get(), (x * f.scale) as i32, (y * f.scale) as i32);
                glyph_menu = Some(SurfaceMessage::ContextMenu { x, y, screen_x, screen_y, element_id: Some(glyph.element_id), menu: Some(glyph.menu.to_string()) });
            } else if let Some(slot) = slot_hit {
                // The menu that owns the slot stays selected (and so open) while the item is typed.
                design.cancel_drag();
                design.set_selected(Some(slot.parent_id.clone()));
                typing = Some(TypeEdit::new(&slot));
                host::request_repaint_after(0);
            } else if pressed {
                if in_scrollbars {
                    let vertical = need_v && v_track.contains(mouse.0, mouse.1);
                    let (thumb, along, page, offset) = if vertical {
                        (v_thumb, mouse.1, view_h * 0.9, scroll.1)
                    } else {
                        (h_thumb, mouse.0, view_w * 0.9, scroll.0)
                    };
                    match thumb {
                        Some(t) if t.contains(mouse.0, mouse.1) => {
                            thumb_drag = Some(ThumbDrag { vertical, start_mouse: along, start_offset: offset });
                        }
                        Some(t) => {
                            let before = if vertical { along < t.top } else { along < t.left };
                            let delta = if before { -page } else { page };
                            if vertical {
                                scroll.1 += delta;
                            } else {
                                scroll.0 += delta;
                            }
                        }
                        None => {}
                    }
                } else if let Some(handle) = hot_handle {
                    frame_drag = Some(FrameResize { handle, start_mouse: mouse, start_size: (w, h) });
                    design.cancel_drag();
                    design.set_selected(Some(String::new()));
                } else if frame.client.contains(mouse.0, mouse.1)
                    || layout_map.hit_test(mouse.0, mouse.1).is_some_and(|e| !e.id.is_empty() && frame.outer.contains(mouse.0, mouse.1))
                {
                    let mods = PointerModifiers { ctrl: f.mods.ctrl, shift: f.mods.shift };
                    design.press_with(&layout_map, mouse.0, mouse.1, mods);
                    // A double-click: the element's default event handler (EVENTS.md §5.2), sent after
                    // `selectionChanged`. Not a move: the second press starts no drag.
                    if f.click_count >= 2 {
                        design.cancel_drag();
                        double_click = layout_map.hit_test(mouse.0, mouse.1).map(|e| e.id.clone()).or_else(|| Some(String::new()));
                    }
                } else if frame.outer.contains(mouse.0, mouse.1) {
                    // The title bar: the view itself.
                    design.cancel_drag();
                    design.set_selected(Some(String::new()));
                    if f.click_count >= 2 {
                        double_click = Some(String::new());
                    }
                } else {
                    // The canvas around the view: a marquee over the view's top-level elements
                    // (a plain click selects the view itself).
                    let mods = PointerModifiers { ctrl: f.mods.ctrl, shift: f.mods.shift };
                    design.begin_marquee(String::new(), mouse.0, mouse.1, mods);
                }
            }

            if let Some(drag) = thumb_drag {
                if f.mouse_down {
                    let (along, len, content, viewport_len) = if drag.vertical {
                        (mouse.1, v_track.bottom - v_track.top, content_h, view_h)
                    } else {
                        (mouse.0, h_track.right - h_track.left, content_w, view_w)
                    };
                    let offset =
                        design::scroll_for_thumb_drag(drag.start_offset, along - drag.start_mouse, len, content, viewport_len);
                    if drag.vertical {
                        scroll.1 = offset;
                    } else {
                        scroll.0 = offset;
                    }
                    host::request_repaint_after(16);
                } else {
                    thumb_drag = None;
                }
            }

            // A pointer that left the surface (`pointer_outside`) moves nothing: the drag keeps its last
            // position (a stray "away" position would otherwise drop the element at the far edge).
            if design.is_dragging() && !f.pointer_outside() {
                // Shift suppresses snapping (`DESIGNER.md` §4) — `f.mods` is
                // the REAL frame's modifier snapshot (the neutered copy fed
                // to `frame_with_design` above never reaches here).
                design.update_drag(&layout_map, mouse.0, mouse.1, f.mods.shift);
            }

            // Esc cancels a canvas resize (nothing was written yet).
            if frame_drag.is_some() && host::take_key(vk::ESCAPE, Modifiers::NONE) > 0 {
                frame_drag = None;
            }

            // F2 on a menu item: its text in place.
            if typing.is_none() && host::take_key(vk::F2, Modifiers::NONE) > 0 {
                if let Some(id) = design.selected().filter(|id| kubuno_desktop_views::menus::is_menu_row(id)).map(str::to_string) {
                    if let Some(entry) = layout_map.get(&id) {
                        let parse = kubuno_desktop_views::syntax::parse(&current_text);
                        let text = Document::cast(parse.syntax()).and_then(|d| d.resolve_id(&id)).and_then(|e| e.attribute("Text").and_then(|a| a.value())).unwrap_or_default();
                        typing = Some(TypeEdit { parent_id: String::new(), index: 0, horizontal: false, rename_of: Some(id), text, rect: entry.bounds });
                    }
                }
            }
            // The in-place editor of a menu takes the keyboard: text, Backspace, Escape; Enter (or Down)
            // goes on to the next slot, Tab into the new item's sub-menu (Enter does, on a menu bar).
            if let Some(mut t) = typing.take() {
                if t.rename_of.is_none() {
                    // The slot moves as items are added above it.
                    if let Some(s) = type_slots.iter().find(|s| s.parent_id == t.parent_id && s.index == t.index) {
                        t.rect = s.rect;
                    }
                }
                let mut keep = true;
                let mut next: Option<TypeEdit> = None;
                let mut typed_events = host::consume(|e| matches!(e, host::InputEvent::Key { down: true, .. } | host::InputEvent::Text(_)));
                if typing_tab {
                    typed_events.push(host::InputEvent::Key { vk: vk::TAB, down: true, repeat: false, mods: Modifiers::NONE });
                }
                for e in typed_events {
                    match e {
                        host::InputEvent::Text(s) => t.text.extend(s.chars().filter(|c| !c.is_control())),
                        host::InputEvent::Key { vk: k, .. } if k == vk::BACK => {
                            t.text.pop();
                        }
                        host::InputEvent::Key { vk: k, .. } if k == vk::ESCAPE => keep = false,
                        host::InputEvent::Key { vk: k, .. } if k == vk::ENTER || k == vk::TAB || (k == vk::DOWN && !t.horizontal) || (k == vk::RIGHT && t.horizontal) => {
                            let typed = !t.text.trim().is_empty();
                            if let Some(op) = t.edit(&current_text) {
                                eprintln!("[embed] proto editRequest(type here) {op:?}");
                                send(&SurfaceMessage::EditRequest { op });
                            }
                            if typed && t.rename_of.is_none() {
                                let id = t.new_id();
                                let into_sub = (k == vk::TAB && !t.horizontal) || (k == vk::ENTER && t.horizontal);
                                let r = t.rect;
                                let h = r.bottom - r.top;
                                next = Some(if into_sub {
                                    let rect = if t.horizontal { Rect::new(r.left, r.bottom + 10.0, r.left + 200.0, r.bottom + 40.0) } else { Rect::new(r.right - 2.0, r.top - 4.0, r.right + 198.0, r.top - 4.0 + h) };
                                    TypeEdit { parent_id: id.clone(), index: 0, horizontal: false, rename_of: None, text: String::new(), rect }
                                } else {
                                    let rect = if t.horizontal { Rect::new(r.right + 2.0, r.top, r.right + 2.0 + (r.right - r.left), r.bottom) } else { Rect::new(r.left, r.bottom, r.right, r.bottom + h) };
                                    TypeEdit { parent_id: t.parent_id.clone(), index: t.index + 1, horizontal: t.horizontal, rename_of: None, text: String::new(), rect }
                                });
                                // The new item is selected: its menu stays open, with its own slot.
                                design.set_selected(Some(if into_sub { id } else if t.horizontal { t.parent_id.clone() } else { id }));
                            }
                            keep = false;
                            break;
                        }
                        _ => {}
                    }
                }
                typing = if keep { Some(t) } else { next };
                host::request_repaint_after(500);
            }
            let keys = read_design_keys();
            // Re-parsed once per frame when ANY of the consumers below
            // actually needs it (an arrow nudge, a drag ending, or an active
            // toolbox drag) — `.kbview` files are hand-sized UI
            // descriptions, not megabyte documents (`kubuno_desktop_views::edit`'s own
            // module doc), so re-parsing here is not the frame's bottleneck;
            // still skipped entirely on an ordinary frame with none of these.
            let drag_ending = !f.mouse_down && design.is_dragging();
            let frame_drag_ending = !f.mouse_down && frame_drag.is_some();
            let need_doc = keys.left
                || keys.right
                || keys.up
                || keys.down
                || drag_ending
                || frame_drag_ending
                || pending_format.is_some()
                || toolbox.is_active();
            let doc = if need_doc {
                let p = kubuno_desktop_views::syntax::parse(&current_text);
                Document::cast(p.syntax())
            } else {
                None
            };

            send_ops(design.handle_keys(&layout_map, doc.as_ref(), keys));

            // A Layout toolbar / Format menu command (DESIGNER.md §13): ONE batch, one undo unit.
            if let Some(command) = pending_format.take() {
                let ops = doc.as_ref().map(|d| design::format_ops(&layout_map, d, design.selection(), command)).unwrap_or_default();
                if ops.is_empty() {
                    eprintln!("[embed] format {command:?}: nothing to change");
                } else {
                    eprintln!("[embed] proto editRequests {ops:?} gesture=Format");
                    send(&SurfaceMessage::EditRequests { ops, gesture: Gesture::Format });
                }
            }

            if drag_ending {
                match design.end_drag(doc.as_ref()) {
                    DragOutcome::None => {}
                    DragOutcome::Single(op) => {
                        eprintln!("[embed] proto editRequest {op:?}");
                        send(&SurfaceMessage::EditRequest { op });
                    }
                    DragOutcome::Batch { ops, gesture } => {
                        eprintln!("[embed] proto editRequests {ops:?} gesture={gesture:?}");
                        send(&SurfaceMessage::EditRequests { ops, gesture });
                    }
                }
            }

            // The end of a canvas resize: ONE batched edit (one undo unit) writing the new size.
            if frame_drag_ending {
                if let Some(drag) = frame_drag.take() {
                    let size = drag.size_at(mouse.0, mouse.1);
                    // The new size, plus the new place of the children its anchors moved (WinForms).
                    let ops = doc
                        .as_ref()
                        .map(|d| {
                            let mut ops = design::design_size_ops(d, size.0, size.1);
                            ops.extend(design::anchored_children_ops(&layout_map, d));
                            ops
                        })
                        .unwrap_or_default();
                    if !ops.is_empty() {
                        eprintln!("[embed] proto editRequests {ops:?} (design size)");
                        send(&SurfaceMessage::EditRequests { ops, gesture: Gesture::Resize });
                        // Shown until the new text arrives (setText clears it).
                        pending_size = Some((size, now + 3000));
                    }
                }
            } else if frame_drag.is_some() {
                host::request_repaint_after(16);
            }

            // Ctrl+C / Ctrl+X / Ctrl+V / Ctrl+D: carried out by the host (clipboard, language server).
            let command = [
                (vk::letter('C'), DesignCommand::Copy),
                (vk::letter('X'), DesignCommand::Cut),
                (vk::letter('V'), DesignCommand::Paste),
                (vk::letter('D'), DesignCommand::Duplicate),
            ]
            .into_iter()
            .find(|(key, _)| host::take_key(*key, Modifiers::CTRL) > 0)
            .map(|(_, command)| command);
            if let Some(name) = command {
                eprintln!("[embed] proto command {name:?}");
                send(&SurfaceMessage::Command { name, element_id: design.selected().map(str::to_string) });
            }

            // Right-click: select what is under the pointer (the view itself outside its client
            // area), then ask the host for the context menu - sent after `selectionChanged`.
            let mut context_menu = None;
            let last_right_click = right_clicks.borrow_mut().drain(..).next_back();
            if let Some((px, py, screen_x, screen_y)) = last_right_click {
                let (x, y) = (px as f32 / f.scale, py as f32 / f.scale);
                let in_bars = (need_v && v_track.contains(x, y)) || (need_h && h_track.contains(x, y));
                if !in_bars && frame_drag.is_none() {
                    let target = design::context_target(&layout_map, &frame, x, y);
                    design.cancel_drag();
                    // A right-click on an element of the multi-selection keeps it (it becomes the
                    // primary), like WinForms: the menu then applies to the whole selection.
                    design.select_for_context(target.clone().unwrap_or_default());
                    context_menu = Some(SurfaceMessage::ContextMenu { x, y, screen_x, screen_y, element_id: target, menu: None });
                }
            }
            // Shift+F10 / the context-menu key: the menu of the selection, at the selection.
            if host::take_key(vk::F10, Modifiers::SHIFT) > 0 || host::take_key(vk::APPS, Modifiers::NONE) > 0 {
                let target = design.selected().filter(|id| !id.is_empty()).map(str::to_string);
                let anchor = target.as_deref().and_then(|id| layout_map.get(id)).map(|e| e.bounds).unwrap_or(frame.title);
                let x = (anchor.left + 12.0).clamp(viewport.left, viewport.right);
                let y = (anchor.top + 12.0).clamp(viewport.top, viewport.bottom);
                let (screen_x, screen_y) =
                    client_to_screen(surface_hwnd.get(), (x * f.scale) as i32, (y * f.scale) as i32);
                context_menu = Some(SurfaceMessage::ContextMenu { x, y, screen_x, screen_y, element_id: target, menu: None });
            }

            // DSG-9 toolbox drag/drop — resolved against THIS frame's fresh
            // `layout_map`/`doc` (see `pending_drag_over`/`pending_drop`'s
            // own comment at their declaration).
            if toolbox.is_active() {
                if let Some((x, y)) = pending_drag_over.take() {
                    let target = doc
                        .as_ref()
                        .and_then(|d| toolbox.drag_over(&layout_map, d, x, y))
                        .cloned();
                    ole_shared.valid.set(target.as_ref().is_some_and(|t| t.valid));
                    send(&SurfaceMessage::DropTargetChanged {
                        target: target.map(Into::into),
                    });
                }
                if let Some((x, y)) = pending_drop.take() {
                    if let Some(d) = doc.as_ref() {
                        let _ = toolbox.drag_over(&layout_map, d, x, y); // refresh at the exact drop point
                    }
                    if let Some(op) = toolbox.drop() {
                        eprintln!("[embed] proto editRequest(drop) {op:?}");
                        send(&SurfaceMessage::EditRequest { op });
                    }
                    send(&SurfaceMessage::DropTargetChanged { target: None });
                }
            } else {
                pending_drag_over = None;
                pending_drop = None;
            }

            if *design.selection() != last_sent_selection {
                let selection = design.selection().clone();
                let bounds = selection.primary().and_then(|id| layout_map.get(id)).map(|e| e.bounds.into());
                send(&SurfaceMessage::SelectionChanged {
                    id: selection.primary().map(str::to_string),
                    bounds,
                    ids: selection.ids().to_vec(),
                });
                last_sent_selection = selection;
            }
            if let Some(element_id) = double_click.take() {
                eprintln!("[embed] proto doubleClick {element_id:?}");
                send(&SurfaceMessage::DoubleClick { element_id });
            }
            if let Some(menu) = context_menu {
                eprintln!("[embed] proto {menu:?}");
                send(&menu);
            }
            if let Some(menu) = glyph_menu {
                eprintln!("[embed] proto {menu:?}");
                send(&menu);
            }
            if let Some(diagnostic) = go_to_source.take() {
                eprintln!("[embed] proto goToSource {}:{}", diagnostic.line, diagnostic.column);
                send(&SurfaceMessage::GoToSource { diagnostic });
            }

            if !status.stale() {
                paint_issue_markers(c, theme, &layout_map, &status.issues, mouse);
            }
            design::paint_adorners(c, theme, &layout_map, design.selection(), design.hover());
            design::paint_frame_handles(c, &frame, frame_drag.map(|d| d.handle).or(hot_handle));
            for (i, preview) in design.drag_previews().into_iter().enumerate() {
                let guides = if i == 0 { design.drag_guides() } else { &[] };
                design::paint_drag_preview(c, theme, preview, guides, viewport);
            }
            if let Some(rect) = design.marquee_rect() {
                // What the release will select, then the rubber band itself.
                for hit in design.marquee_hits().iter().filter_map(|id| layout_map.get(id)) {
                    c.stroke_rounded_w(&design::selection_frame(hit.bounds), 0.0, &theme.accent_light, 1.0);
                }
                design::paint_marquee(c, theme, rect);
                host::request_repaint_after(16);
            }
            if let Some(marker) = design.reorder_marker(&layout_map) {
                design::paint_insertion_marker(c, theme, marker);
            }
            if let Some(t) = &typing {
                paint_type_edit(c, theme, t, now);
            }
            if let Some((zones, hot)) = toolbox.band_zones() {
                design::paint_band_drop_zones(c, theme, &zones, hot);
            }
            if let Some(target) = toolbox.target() {
                design::paint_drop_marker(c, theme, target);
            }
            if frame_drag.is_some() {
                design::paint_size_tooltip(c, mouse.0, mouse.1, (w, h));
            }
            c.pop_clip();
            let thumb_hot = |thumb: Option<Rect>| thumb.is_some_and(|t| t.contains(mouse.0, mouse.1));
            if need_v {
                let hot = thumb_drag.is_some_and(|d| d.vertical) || thumb_hot(v_thumb);
                design::paint_scrollbar(c, v_track, v_thumb, hot);
            }
            if need_h {
                let hot = thumb_drag.is_some_and(|d| !d.vertical) || thumb_hot(h_thumb);
                design::paint_scrollbar(c, h_track, h_thumb, hot);
            }
            if need_v && need_h {
                design::paint_canvas(c, Rect::new(view_w, chrome_top + view_h, f.size.0, f.size.1));
            }
        }

        ring.end_frame();
    });

    match result {
        Ok(()) => {
            eprintln!("[embed] message loop ended");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("view_embed: {e}");
            ExitCode::FAILURE
        }
    }
}

/// The OLE drop target of the design surface's own window: a drag from
/// Visual Studio's Toolbox (`vskubuno/docs/DESIGNER.md` section 11) arrives
/// here directly. The host process cannot receive it for us - found live:
/// OLE does not reach a drop target registered by `devenv.exe` on this
/// window's parent, because the window under the cursor belongs to THIS
/// process. Each callback only queues the very [`HostMessage`] the stdin
/// protocol already carries for a toolbox drag (DSG-9), so the frame loop
/// resolves it exactly like a host-relayed one; the answer to `DragOver`
/// (copy vs. "not allowed") is the validity the frame loop computed for the
/// previous position, one mouse move behind, which is imperceptible.
mod ole_drop {
    use std::cell::{Cell, RefCell};
    use std::collections::VecDeque;
    use std::ffi::c_void;
    use std::rc::Rc;

    use kubuno_desktop_views::protocol::HostMessage;

    use super::win32::{self, Guid, Point};

    const S_OK: i32 = 0;
    const E_NOINTERFACE: i32 = 0x8000_4002_u32 as i32;
    const E_POINTER: i32 = 0x8000_4003_u32 as i32;
    const DROPEFFECT_NONE: u32 = 0;
    const DROPEFFECT_COPY: u32 = 1;
    const DVASPECT_CONTENT: u32 = 1;
    const TYMED_HGLOBAL: u32 = 1;
    const IID_IUNKNOWN: Guid = Guid::new(0x0000_0000, 0x0000, 0x0000, [0xC0, 0, 0, 0, 0, 0, 0, 0x46]);
    const IID_IDROPTARGET: Guid = Guid::new(0x0000_0122, 0x0000, 0x0000, [0xC0, 0, 0, 0, 0, 0, 0, 0x46]);

    /// Toolbox messages waiting for the next frame, and whether the current target accepts the drop.
    pub struct Shared {
        pub queue: RefCell<VecDeque<HostMessage>>,
        pub valid: Cell<bool>,
    }

    impl Default for Shared {
        fn default() -> Self {
            Self { queue: RefCell::new(VecDeque::new()), valid: Cell::new(true) }
        }
    }

    /// `POINTL`, passed BY VALUE to `IDropTarget`'s methods (8 bytes: one register on x64).
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct PointL {
        x: i32,
        y: i32,
    }

    /// `FORMATETC`.
    #[repr(C)]
    struct FormatEtc {
        cf_format: u16,
        ptd: *mut c_void,
        dw_aspect: u32,
        lindex: i32,
        tymed: u32,
    }

    /// `IDataObject`'s vtable up to `GetData`, the only method called.
    #[repr(C)]
    struct DataObjectVtbl {
        query_interface: unsafe extern "system" fn(*mut c_void, *const Guid, *mut *mut c_void) -> i32,
        add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
        release: unsafe extern "system" fn(*mut c_void) -> u32,
        get_data: unsafe extern "system" fn(*mut c_void, *const FormatEtc, *mut win32::StgMedium) -> i32,
    }

    /// `IDropTarget`'s vtable (`IUnknown` + the four drop methods), in declaration order.
    #[repr(C)]
    struct DropTargetVtbl {
        query_interface: unsafe extern "system" fn(*mut DropTarget, *const Guid, *mut *mut c_void) -> i32,
        add_ref: unsafe extern "system" fn(*mut DropTarget) -> u32,
        release: unsafe extern "system" fn(*mut DropTarget) -> u32,
        drag_enter: unsafe extern "system" fn(*mut DropTarget, *mut c_void, u32, PointL, *mut u32) -> i32,
        drag_over: unsafe extern "system" fn(*mut DropTarget, u32, PointL, *mut u32) -> i32,
        drag_leave: unsafe extern "system" fn(*mut DropTarget) -> i32,
        drop: unsafe extern "system" fn(*mut DropTarget, *mut c_void, u32, PointL, *mut u32) -> i32,
    }

    static VTBL: DropTargetVtbl = DropTargetVtbl {
        query_interface: dt_query_interface,
        add_ref: dt_add_ref,
        release: dt_release,
        drag_enter: dt_drag_enter,
        drag_over: dt_drag_over,
        drag_leave: dt_drag_leave,
        drop: dt_drop,
    };

    /// A hand-written COM object implementing `IDropTarget`. Written against raw vtables instead of
    /// the `windows` crate on purpose: the design surface is compiled against the dependency graph
    /// of the project it previews (vskubuno docs/DESIGNER.md section 15), so it may only use crates
    /// and `windows` features that graph already has - the OLE/clipboard/memory features are not
    /// among them. OLE calls it on this window's own (STA) thread only, so `Rc`/`Cell` are fine.
    #[repr(C)]
    struct DropTarget {
        vtbl: *const DropTargetVtbl,
        refs: Cell<u32>,
        hwnd: isize,
        shared: Rc<Shared>,
        active: Cell<bool>,
    }

    /// Registers the drop target on `hwnd` (the surface's own window). Logged, never fatal.
    pub fn register(hwnd: isize, shared: Rc<Shared>) {
        let object = Box::into_raw(Box::new(DropTarget {
            vtbl: &VTBL,
            refs: Cell::new(1),
            hwnd,
            shared,
            active: Cell::new(false),
        }));
        // SAFETY: plain OLE calls on this (the window's own, STA) thread; OleInitialize on a thread the
        // host already CoInitialize'd as STA returns S_FALSE, which is success here. `object` is a
        // valid COM object; RegisterDragDrop takes its own reference, ours is released right after.
        unsafe {
            let hr = win32::OleInitialize(std::ptr::null_mut());
            if hr < 0 {
                eprintln!("[embed] OleInitialize failed ({hr:#x}) - Toolbox drops disabled");
            } else {
                let hr = win32::RegisterDragDrop(hwnd, object.cast());
                if hr == S_OK {
                    eprintln!("[embed] Toolbox drop target registered");
                } else {
                    eprintln!("[embed] RegisterDragDrop failed ({hr:#x}) - Toolbox drops disabled");
                }
            }
            dt_release(object);
        }
    }

    /// The Kubuno component name a Visual Studio Toolbox item carries (vskubuno's
    /// `ToolboxItemFormat`: UTF-8 bytes under the `Kubuno.Views.ToolboxItem` clipboard format).
    ///
    /// # Safety
    /// `data` must be null or a valid `IDataObject` pointer (OLE's `DragEnter` argument).
    unsafe fn component_of(data: *mut c_void) -> Option<String> {
        if data.is_null() {
            return None;
        }
        let name: Vec<u16> = "Kubuno.Views.ToolboxItem\0".encode_utf16().collect();
        let format = win32::RegisterClipboardFormatW(name.as_ptr());
        if format == 0 {
            return None;
        }
        let request = FormatEtc {
            cf_format: format as u16,
            ptd: std::ptr::null_mut(),
            dw_aspect: DVASPECT_CONTENT,
            lindex: -1,
            tymed: TYMED_HGLOBAL,
        };
        let mut medium = win32::StgMedium { tymed: 0, handle: std::ptr::null_mut(), unk_for_release: std::ptr::null_mut() };
        let vtbl = *(data as *mut *const DataObjectVtbl);
        if ((*vtbl).get_data)(data, &request, &mut medium) < 0 {
            return None;
        }
        let mut result = None;
        if medium.tymed == TYMED_HGLOBAL {
            let global = medium.handle;
            let size = win32::GlobalSize(global);
            let bytes = win32::GlobalLock(global) as *const u8;
            if !bytes.is_null() && size > 0 {
                let text = String::from_utf8_lossy(std::slice::from_raw_parts(bytes, size)).to_string();
                let text = text.trim_end_matches('\0').trim().to_string();
                if !text.is_empty() && text.chars().all(|c| c.is_alphanumeric() || "_-.:".contains(c)) {
                    result = Some(text);
                }
            }
            let _ = win32::GlobalUnlock(global);
        }
        win32::ReleaseStgMedium(&mut medium);
        result
    }

    impl DropTarget {
        /// `pt` (screen pixels) in this window's client DIPs - the unit DSG-9's dragOver/drop speak.
        fn to_dip(&self, pt: PointL) -> (f32, f32) {
            let mut p = Point { x: pt.x, y: pt.y };
            // SAFETY: plain Win32 calls on a valid window handle owned by this thread.
            let dpi = unsafe {
                let _ = win32::ScreenToClient(self.hwnd, &mut p);
                win32::GetDpiForWindow(self.hwnd)
            };
            let scale = if dpi == 0 { 1.0 } else { dpi as f32 / 96.0 };
            (p.x as f32 / scale, p.y as f32 / scale)
        }

        fn push(&self, msg: HostMessage) {
            self.shared.queue.borrow_mut().push_back(msg);
            // SAFETY: invalidating our own window just schedules the next frame.
            unsafe {
                let _ = win32::InvalidateRect(self.hwnd, std::ptr::null(), 0);
            }
        }

        fn effect(&self) -> u32 {
            if self.active.get() && self.shared.valid.get() {
                DROPEFFECT_COPY
            } else {
                DROPEFFECT_NONE
            }
        }
    }

    unsafe extern "system" fn dt_query_interface(this: *mut DropTarget, iid: *const Guid, out: *mut *mut c_void) -> i32 {
        if out.is_null() || iid.is_null() {
            return E_POINTER;
        }
        if *iid == IID_IUNKNOWN || *iid == IID_IDROPTARGET {
            dt_add_ref(this);
            *out = this.cast();
            S_OK
        } else {
            *out = std::ptr::null_mut();
            E_NOINTERFACE
        }
    }

    unsafe extern "system" fn dt_add_ref(this: *mut DropTarget) -> u32 {
        let refs = (*this).refs.get() + 1;
        (*this).refs.set(refs);
        refs
    }

    unsafe extern "system" fn dt_release(this: *mut DropTarget) -> u32 {
        let refs = (*this).refs.get().saturating_sub(1);
        (*this).refs.set(refs);
        if refs == 0 {
            drop(Box::from_raw(this));
        }
        refs
    }

    unsafe extern "system" fn dt_drag_enter(this: *mut DropTarget, data: *mut c_void, _keys: u32, pt: PointL, effect: *mut u32) -> i32 {
        let target = &*this;
        let component = component_of(data);
        target.active.set(component.is_some());
        if let Some(component) = component {
            eprintln!("[embed] ole dragEnter {component}");
            target.shared.valid.set(true);
            target.push(HostMessage::DragEnter { component });
            let (x, y) = target.to_dip(pt);
            target.push(HostMessage::DragOver { x, y });
        }
        if !effect.is_null() {
            *effect = target.effect();
        }
        S_OK
    }

    unsafe extern "system" fn dt_drag_over(this: *mut DropTarget, _keys: u32, pt: PointL, effect: *mut u32) -> i32 {
        let target = &*this;
        if target.active.get() {
            let (x, y) = target.to_dip(pt);
            target.push(HostMessage::DragOver { x, y });
        }
        if !effect.is_null() {
            *effect = target.effect();
        }
        S_OK
    }

    unsafe extern "system" fn dt_drag_leave(this: *mut DropTarget) -> i32 {
        let target = &*this;
        if target.active.replace(false) {
            target.push(HostMessage::DragLeave);
        }
        S_OK
    }

    unsafe extern "system" fn dt_drop(this: *mut DropTarget, _data: *mut c_void, _keys: u32, pt: PointL, effect: *mut u32) -> i32 {
        let target = &*this;
        let result = target.effect();
        if target.active.replace(false) {
            let (x, y) = target.to_dip(pt);
            eprintln!("[embed] ole drop at {x:.0},{y:.0}");
            target.push(HostMessage::Drop { x, y });
        }
        if !effect.is_null() {
            *effect = result;
        }
        S_OK
    }
}

/// The few Win32 functions this surface calls itself, declared here instead of through the `windows`
/// crate: the design surface is compiled against the project's own dependency graph (vskubuno
/// docs/DESIGNER.md section 15) and must not need a `windows` feature that graph lacks.
mod win32 {
    use std::ffi::c_void;

    #[repr(C)]
    pub struct Point {
        pub x: i32,
        pub y: i32,
    }

    #[repr(C)]
    #[derive(PartialEq, Eq)]
    pub struct Guid {
        data1: u32,
        data2: u16,
        data3: u16,
        data4: [u8; 8],
    }

    impl Guid {
        pub const fn new(data1: u32, data2: u16, data3: u16, data4: [u8; 8]) -> Self {
            Self { data1, data2, data3, data4 }
        }
    }

    /// `STGMEDIUM` (its union is one pointer-sized handle).
    #[repr(C)]
    pub struct StgMedium {
        pub tymed: u32,
        pub handle: *mut c_void,
        pub unk_for_release: *mut c_void,
    }

    #[link(name = "user32")]
    extern "system" {
        pub fn ClientToScreen(hwnd: isize, point: *mut Point) -> i32;
        pub fn ScreenToClient(hwnd: isize, point: *mut Point) -> i32;
        pub fn InvalidateRect(hwnd: isize, rect: *const c_void, erase: i32) -> i32;
        pub fn GetDpiForWindow(hwnd: isize) -> u32;
        pub fn RegisterClipboardFormatW(name: *const u16) -> u32;
    }

    #[link(name = "kernel32")]
    extern "system" {
        pub fn GlobalLock(memory: *mut c_void) -> *mut c_void;
        pub fn GlobalUnlock(memory: *mut c_void) -> i32;
        pub fn GlobalSize(memory: *mut c_void) -> usize;
    }

    #[link(name = "ole32")]
    extern "system" {
        pub fn OleInitialize(reserved: *mut c_void) -> i32;
        pub fn RegisterDragDrop(hwnd: isize, target: *mut c_void) -> i32;
        pub fn ReleaseStgMedium(medium: *mut StgMedium);
    }
}
