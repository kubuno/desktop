//! `PageCanvas` — the editable paginated document: the backdrop, the pages laid out side by side
//! like the web's page container, and on them the core's layout, the selection and the caret.
//!
//! The document, its selection, its history and its layout are the platform-neutral core's
//! (`kubuno_office_docs_core::editor::Editor`, a port of the web editor's engine and editing semantics);
//! this control is the Windows half: it measures with DirectWrite, paints with Direct2D, and turns
//! the input into the core's calls —
//!
//! * the pointer: click (caret), Shift+click (extend), double click (word), triple click
//!   (paragraph), drag (extending by the unit of the press) with auto-scroll near the edges
//!   (48 DIP, ≤ 30 DIP a step: `auto-scroll.ts`);
//! * the keys of `nav-keys.ts` and the browser: arrows (Ctrl: by word), ↑/↓ keeping the goal
//!   column, Home/End (Ctrl: document), PageUp/PageDown (one view height), Shift extends; Enter,
//!   Shift+Enter, Ctrl+Enter, Backspace/Delete (Ctrl: by word), Tab/Shift+Tab, Ctrl+A,
//!   Ctrl+Shift+V (paste as text);
//! * typing: `WM_CHAR` (dead keys and AltGr arrive composed; an IME's result too, its composition
//!   window placed at the caret);
//! * the clipboard (multi-format: `crate::platform::clipboard`).
//!
//! The ribbon's commands arrive as method calls from the window (`DocumentWindow`), and what the
//! ribbon shows of the selection leaves as `EditorStateChanged`. What the rulers and the status bar
//! need leaves as `ViewChanged`; what the rulers change comes back as `set_page_margins`,
//! `set_indents`, `set_tab_stops` — written to the document, undoable, a drag previewed live and
//! recorded once on release.

use std::rc::Rc;

use kubuno_desktop::prelude::*;
use kubuno_desktop::ui::graphics::{Brush, Color, DashStyle, Font, Pen, PointF, StringFormat};
use kubuno_desktop::ui::range::{ScrollBar, ScrollPart};
use kubuno_desktop::ui::{Canvas, Rect, Size, Widget, WidgetState};
use kubuno_desktop::views::component::{Component, Control, ControlCore, EventCx, Keys, PaintEventCx};
use kubuno_desktop::views::events::Event;
use kubuno_office_docs_core::edit::clipboard::{self as clip, Pasted};
use kubuno_office_docs_core::edit::commands::{self as cmd, EditState};
use kubuno_office_docs_core::edit::history::Selection;
use kubuno_office_docs_core::edit::step::EditError;
use kubuno_office_docs_core::editor::Motion;
use kubuno_office_docs_core::measure::{FixedMeasure, Measure};
use kubuno_office_docs_core::model::Node;

use crate::controls::ruler::{format_tab_stops, parse_tab_stops, TabKind, TabStop};
use crate::doc::fonts::Fonts;
use crate::doc::paint;
use crate::model::state::App;
use crate::platform::images::ImageCache;
use crate::platform::painter::Painter;
use crate::platform::{clipboard, ime};

/// One wheel notch, in DIP.
pub const WHEEL_STEP: f32 = 3.0 * 20.0;
/// One arrow-key or scroll-arrow step, in DIP.
pub const LINE_STEP: f32 = 40.0;
/// `auto-scroll.ts`: the edge band that scrolls while dragging.
const AUTO_EDGE: f32 = 48.0;
/// `pointer-select.ts`: multi-click delay and distance.
const MULTI_CLICK_MS: u64 = 500;
const MULTI_CLICK_SLOP: f32 = 4.0;
/// The caret: solid for 500 ms after a move, then blinking with a 1 s period (`_gdocs_blink 1s 0.5s`).
const BLINK_SOLID_MS: u64 = 500;
const BLINK_HALF_MS: u64 = 500;

/// The ground around the sheets and their edge: Word's grey in the light theme, the web's viewer
/// backdrop and the `Border` token in the dark one. The paper stays white in both, as on the web.
pub fn page_ground(t: &kubuno_desktop::ui::Theme) -> (windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F, windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F) {
    if t.mode == kubuno_desktop::ui::ThemeMode::Dark {
        (t.layer_background, t.card_stroke)
    } else {
        (paint::BACKDROP, paint::PAGE_EDGE)
    }
}

/// What the rulers and the status bar show of the page (raised as `ViewChanged`).
#[derive(kubuno_desktop::views::events::EventArgs, Debug, Clone, Default, PartialEq)]
pub struct PageViewEventArgs {
    pub zoom: f32,
    /// Where the active page's paper starts, in DIP from the canvas's left edge.
    pub origin_x: f32,
    /// Where the active page's paper starts, in DIP from the top of the visible page area.
    pub page_top: f32,
    pub page_width: f32,
    pub page_height: f32,
    pub margin_left: f32,
    pub margin_right: f32,
    pub margin_top: f32,
    pub margin_bottom: f32,
    /// The page of the caret, else the one most in view (1-based), and how many there are.
    pub current_page: usize,
    pub page_count: usize,
    /// The caret paragraph's indents (document pixels) and tab stops (`48:left,120:right`).
    pub indent_left: f32,
    pub indent_first_line: f32,
    pub indent_right: f32,
    pub tab_stops: String,
}

/// What the ribbon shows of the selection, and the document's edit state (raised as
/// `EditorStateChanged` when it differs).
#[derive(kubuno_desktop::views::events::EventArgs, Debug, Clone, Default, PartialEq)]
pub struct EditorStateEventArgs {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    pub subscript: bool,
    pub superscript: bool,
    /// Empty when mixed.
    pub font_family: String,
    pub font_size: String,
    pub align: String,
    /// `bulletList`, `orderedList`, `taskList` or empty.
    pub list: String,
    /// The style of the caret's block: `normal`, `h1`… (the gallery's values).
    pub style: String,
    pub line_height: f32,
    pub space_before: bool,
    pub space_after: bool,
    pub first_line: bool,
    pub hanging: bool,
    pub link: String,
    pub can_undo: bool,
    pub can_redo: bool,
    pub dirty: bool,
    pub has_selection: bool,
    pub in_table: bool,
    pub keep_next: bool,
    pub keep_lines: bool,
    pub page_break_before: bool,
    pub code_block: bool,
    pub small_caps: bool,
    /// The document's change counter (a save compares it).
    pub revision: u64,
    /// A message for the status bar (an image too large, a paste refused…), empty for none.
    pub message: String,
}

/// The dashed guide a ruler drag draws over the page, and its value tooltip.
#[derive(Debug, Clone, PartialEq)]
pub struct DragGuide {
    pub vertical: bool,
    pub position: f32,
    pub label: String,
    pub pointer: f32,
}

/// The paginated, editable document (see the module doc).
#[derive(kubuno_desktop::views::component::Component)]
#[kubuno(extends = Control, overrides(Control))]
#[category("Documents")]
#[toolbox(icon = "file-text")]
#[default_event("ViewChanged")]
pub struct PageCanvas {
    base: ControlCore,
    /// The band at the top of the canvas the horizontal ruler covers (DIP): the pages start below it.
    #[property(bindable)]
    #[category("Layout")]
    pub viewport_top: f32,
    /// Occurs when what the rulers and the status bar show changes.
    #[event]
    #[category("Action")]
    pub view_changed: Event<PageViewEventArgs>,
    /// Occurs when the selection's formatting or the document's edit state changes.
    #[event]
    #[category("Action")]
    pub editor_state_changed: Event<EditorStateEventArgs>,
    state: App,
    fonts: Option<(usize, Rc<Fonts>)>,
    images: ImageCache,
    guide: Option<DragGuide>,
    hscroll_hot: bool,
    hscroll_grab: Option<f32>,
    viewport: Rect,
    bounds: Rect,
    reported: Option<PageViewEventArgs>,
    reported_state: Option<EditorStateEventArgs>,
    // ── Editing ──
    has_focus: bool,
    /// When the caret last moved (the blink restarts there).
    caret_moved_ms: u64,
    /// The last press, for double/triple clicks: (time, x, y, count).
    last_click: (u64, f32, f32, u32),
    /// The pointer while a selection drag is in progress (view DIP), for auto-scroll.
    drag_at: Option<(f32, f32)>,
    /// Scroll so the caret shows at the next paint.
    reveal: bool,
    sys_caret: ime::SystemCaret,
    scale: f32,
    message: String,
    /// The format painter's capture while it is armed.
    painter: Option<cmd::CapturedFormat>,
    /// Affichage › Marques ¶ and the text boundaries.
    show_marks: bool,
}

impl Default for PageCanvas {
    fn default() -> Self {
        Self {
            base: ControlCore::default(),
            viewport_top: 0.0,
            view_changed: Event::default(),
            editor_state_changed: Event::default(),
            state: App::default(),
            fonts: None,
            images: ImageCache::default(),
            guide: None,
            hscroll_hot: false,
            hscroll_grab: None,
            viewport: Rect::default(),
            bounds: Rect::default(),
            reported: None,
            reported_state: None,
            has_focus: false,
            caret_moved_ms: 0,
            last_click: (0, 0.0, 0.0, 0),
            drag_at: None,
            reveal: false,
            sys_caret: ime::SystemCaret::default(),
            scale: 1.0,
            message: String::new(),
            painter: None,
            show_marks: false,
        }
    }
}

/// The page area's vertical scroll bar (in DIP) and its rail. `None` when the content fits.
pub fn scrollbar(viewport: Rect, state: &App) -> Option<(ScrollBar, Rect)> {
    let bar = ScrollBar::from_content(false, state.content_h, viewport.bottom - viewport.top, state.scroll)?.with_expanded(state.scroll_hot);
    let rail = bar.rail(&viewport);
    Some((bar, rail))
}

/// The scroll a thumb grabbed `grab` DIP below its top and dragged to `at` maps to.
pub fn scroll_from_thumb_along(bar: &ScrollBar, rail: &Rect, at: f32, grab: f32, max_scroll: f32, horizontal: bool) -> f32 {
    let thumb = bar.thumb_rect(*rail);
    let (start, rail_len, thumb_len) = if horizontal {
        (rail.left, rail.right - rail.left, thumb.right - thumb.left)
    } else {
        (rail.top, rail.bottom - rail.top, thumb.bottom - thumb.top)
    };
    let travel = rail_len - thumb_len;
    if travel <= 0.0 {
        return 0.0;
    }
    ((at - grab - start).clamp(0.0, travel)) / travel * max_scroll
}

/// How far the page area can scroll sideways (a page wider than the view).
pub fn max_scroll_x(viewport: Rect, state: &App) -> f32 {
    (state.content_w - (viewport.right - viewport.left)).max(0.0)
}

pub fn hscrollbar(viewport: Rect, state: &App, hot: bool) -> Option<(ScrollBar, Rect)> {
    let bar = ScrollBar::from_content(true, state.content_w, viewport.right - viewport.left, state.scroll_x)?.with_expanded(hot);
    let rail = bar.rail(&viewport);
    Some((bar, rail))
}

fn now() -> u64 {
    kubuno_desktop::controls::host::now_ms()
}

impl PageCanvas {
    /// Shows `state` (a document opened from a file, the server, or the sample).
    pub fn open(&mut self, state: App) {
        self.state = state;
        self.reported = None;
        self.reported_state = None;
        self.reveal = true;
        self.caret_moved_ms = now();
        self.invalidate();
    }

    pub fn state(&self) -> &App {
        &self.state
    }

    pub fn state_mut(&mut self) -> &mut App {
        &mut self.state
    }

    /// The measurer: DirectWrite once painted, the fixed test measurer before (designer, tests).
    fn measure(&self) -> Rc<dyn Measure> {
        match &self.fonts {
            Some((_, f)) => f.clone(),
            None => Rc::new(FixedMeasure),
        }
    }

    fn viewport_height(&self) -> f32 {
        (self.viewport.bottom - self.viewport.top).max(0.0)
    }

    /// Lays the document out and places the pages (cheap when nothing changed).
    fn ensure_layout(&mut self) {
        let m = self.measure();
        self.state.editor.relayout(&*m);
        let width = (self.viewport.right - self.viewport.left).max(1.0);
        self.state.place(width);
    }

    fn after_scroll(&mut self) {
        self.state.current_page = self.state.page_at_scroll(self.viewport_height());
        self.invalidate();
    }

    pub fn set_zoom(&mut self, zoom: f32) {
        let h = self.viewport_height();
        self.state.set_zoom(zoom, h);
        self.reveal = true;
        self.after_scroll();
    }

    /// Affichage › Une page.
    pub fn zoom_one_page(&mut self) {
        let s = self.state.editor.page_setup(0);
        let (vw, vh) = (self.viewport.right - self.viewport.left, self.viewport_height());
        self.set_zoom(((vw - 48.0) / s.width.max(1.0)).min((vh - 2.0 * crate::model::state::CANVAS_PAD_Y) / s.height.max(1.0)));
    }

    /// Affichage › Largeur de la page.
    pub fn zoom_page_width(&mut self) {
        let s = self.state.editor.page_setup(0);
        self.set_zoom((self.viewport.right - self.viewport.left - 48.0) / s.width.max(1.0));
    }

    pub fn set_drag_guide(&mut self, guide: Option<DragGuide>) {
        if self.guide != guide {
            self.guide = guide;
            self.invalidate();
        }
    }

    // ── Editing entry points ───────────────────────────────────────────────

    /// The edit is done: relayout, keep the caret visible, repaint, tell the ribbon.
    fn edited(&mut self) {
        // Tests and diagnostics: `KUBUNO_DOCS_DUMP=<file>` writes the document after every change.
        if let Some(path) = std::env::var_os("KUBUNO_DOCS_DUMP") {
            if let Ok(bytes) = self.state.editor.to_bytes() {
                let _ = std::fs::write(path, bytes);
            }
        }
        self.caret_moved_ms = now();
        self.reveal = true;
        self.ensure_layout();
        self.invalidate();
    }

    /// Runs an edit command on the document.
    pub fn run(&mut self, f: impl FnOnce(&mut EditState) -> Result<cmd::Change, EditError>) -> bool {
        let ok = match self.state.editor.apply(now(), f) {
            Ok(()) => true,
            Err(EditError::NotApplicable) => false,
            Err(e) => {
                kubuno_desktop::tracing::warn!("[documents] edit refused: {e}");
                false
            }
        };
        self.edited();
        ok
    }

    pub fn undo(&mut self) {
        self.state.editor.undo();
        self.edited();
    }

    pub fn redo(&mut self) {
        self.state.editor.redo();
        self.edited();
    }

    pub fn select_all(&mut self) {
        self.state.editor.select_all();
        self.caret_moved_ms = now();
        self.invalidate();
    }

    fn owner_window() -> windows::Win32::Foundation::HWND {
        ime::main_window().unwrap_or_default()
    }

    pub fn copy(&mut self) -> bool {
        let Some((blocks, text)) = self.state.editor.selection_payload() else { return false };
        match clip::payloads(&blocks, &text) {
            Ok((private, html, plain)) => match clipboard::write(Self::owner_window(), &private, &html, &plain) {
                Ok(()) => true,
                Err(e) => {
                    kubuno_desktop::tracing::warn!("[documents] {e}");
                    false
                }
            },
            Err(e) => {
                kubuno_desktop::tracing::warn!("[documents] {e}");
                false
            }
        }
    }

    pub fn cut(&mut self) {
        if self.copy() {
            self.run(|s| cmd::delete_backward(s, false, None));
        }
    }

    /// Ctrl+V (`plain`: Ctrl+Shift+V, « Coller sans mise en forme »).
    pub fn paste(&mut self, plain: bool) {
        let c = clipboard::read(Self::owner_window());
        if !plain {
            if let (Some(bytes), None) = (&c.image, &c.private) {
                // An image alone (a screenshot, a copied picture): shrunk, re-encoded, inserted.
                if c.html.is_none() || c.text.as_deref().is_none_or(str::is_empty) {
                    self.insert_image_bytes(bytes);
                    return;
                }
            }
        }
        match clip::choose(c.private.as_deref(), c.html.as_deref(), c.text.as_deref(), plain) {
            Some(Pasted::Blocks(blocks)) => {
                let blocks = self.prepare_pasted(blocks);
                self.run(|s| cmd::paste_blocks(s, blocks));
            }
            Some(Pasted::Text(t)) => {
                self.run(|s| cmd::paste_text(s, &t));
            }
            None => {}
        }
    }

    /// Pasted blocks: images that are not `data:` URIs cannot be fetched here, and an oversized
    /// data image is re-encoded like an inserted one.
    fn prepare_pasted(&mut self, blocks: Vec<Node>) -> Vec<Node> {
        blocks
            .into_iter()
            .map(|b| {
                if b.node_type() == Some("image") {
                    let a = b.attrs();
                    if let Some(src) = a.get("src").and_then(|v| v.as_str()) {
                        if let Some((_, bytes)) = kubuno_office_docs_core::base64::data_uri(src) {
                            if bytes.len() > kubuno_office_docs_core::layout::images::MAX_ENCODED_BYTES {
                                if let Ok(p) = crate::platform::images::prepare_insert(&bytes) {
                                    return Node::element("image", Some(serde_json::json!({ "src": p.data_uri, "width": p.width, "height": p.height })), vec![]);
                                }
                            }
                        }
                    }
                }
                b
            })
            .collect()
    }

    /// Inserts an image from its bytes (paste, Insertion › Image): shrunk to fit the page's column.
    pub fn insert_image_bytes(&mut self, bytes: &[u8]) {
        match crate::platform::images::prepare_insert(bytes) {
            Ok(p) => {
                let col = self.state.editor.page_setup(0).col_w();
                let (w, h) = (p.width as f32, p.height as f32);
                let k = if w > col { col / w } else { 1.0 };
                let attrs = serde_json::json!({ "src": p.data_uri, "width": (w * k).round(), "height": (h * k).round() });
                self.run(|s| cmd::insert_blocks(s, vec![Node::element("image", Some(attrs), vec![])]));
            }
            Err(e) => {
                self.message = e;
                self.invalidate();
            }
        }
    }

    pub fn toggle_mark(&mut self, mark: &str) {
        let m = mark.to_string();
        self.run(move |s| cmd::toggle_mark(s, &m));
    }

    pub fn set_text_style(&mut self, key: &str, value: Option<String>) {
        let (k, v) = (key.to_string(), value.map(serde_json::Value::String));
        self.run(move |s| cmd::set_text_style(s, &k, v));
    }

    pub fn set_font_size(&mut self, size: &str) {
        let pt = size.trim().trim_end_matches("pt").trim().parse::<f32>().ok().filter(|v| *v > 0.0);
        if let Some(pt) = pt {
            let v = format!("{}pt", pt.clamp(cmd::MIN_FONT_PT, cmd::MAX_FONT_PT));
            self.set_text_style("fontSize", Some(v));
        }
    }

    pub fn grow_font(&mut self, delta: f32) {
        self.run(move |s| cmd::grow_font(s, delta));
    }

    pub fn change_case(&mut self, mode: &str) {
        let m = mode.to_string();
        self.run(move |s| cmd::change_case(s, &m));
    }

    pub fn set_highlight(&mut self, color: Option<&str>) {
        let c = color.map(str::to_string);
        self.run(move |s| cmd::set_highlight(s, c.as_deref()));
    }

    pub fn clear_formatting(&mut self) {
        self.run(cmd::clear_formatting);
    }

    pub fn set_align(&mut self, align: &str) {
        let a = align.to_string();
        self.run(move |s| cmd::set_align(s, &a));
    }

    pub fn toggle_list(&mut self, list: &str) {
        let l = list.to_string();
        self.run(move |s| cmd::toggle_list(s, &l));
    }

    pub fn indent(&mut self, delta: i32) {
        self.run(move |s| cmd::indent(s, delta));
    }

    pub fn set_paragraph_attr(&mut self, key: &str, value: serde_json::Value) {
        let k = key.to_string();
        self.run(move |s| cmd::set_paragraph_attrs(s, &[(&k, value)]));
    }

    /// Applies a style of the gallery (`normal`, `h1`…`h4`, `title`, `subtitle`, `quote`,
    /// `noSpacing`).
    pub fn apply_style(&mut self, id: &str) {
        let wanted = match id {
            "h1" => "heading1",
            "h2" => "heading2",
            "h3" => "heading3",
            "h4" => "heading4",
            other => other,
        };
        if let Some(style) = cmd::default_styles().into_iter().find(|s| s.id == wanted) {
            self.run(move |s| cmd::apply_named_style(s, &style));
        }
    }

    pub fn insert_page_break(&mut self) {
        self.run(cmd::insert_page_break);
    }

    pub fn insert_blocks(&mut self, blocks: Vec<Node>) {
        self.run(move |s| cmd::insert_blocks(s, blocks));
    }

    pub fn set_link(&mut self, href: Option<String>) {
        self.run(move |s| cmd::set_link(s, href.as_deref()));
    }

    /// Inserts text at the selection (a symbol).
    pub fn insert_text(&mut self, text: &str) {
        let t = text.to_string();
        self.run(move |s| cmd::insert_text(s, &t));
    }

    /// Small capitals on or off (`textStyle.smallCaps`).
    pub fn toggle_small_caps(&mut self) {
        let on = self.state.editor.selection_info().small_caps;
        self.set_text_style_value("smallCaps", if on { None } else { Some(serde_json::Value::Bool(true)) });
    }

    fn set_text_style_value(&mut self, key: &str, value: Option<serde_json::Value>) {
        let k = key.to_string();
        self.run(move |s| cmd::set_text_style(s, &k, value));
    }

    /// The hanging indent toggle (`hanging`: first line −36 px, left at least 36 px).
    pub fn set_hanging(&mut self, on: bool, left: f32) {
        let patch = if on {
            vec![("indentFirstLine", serde_json::json!(-36)), ("indentLeft", serde_json::json!(left.max(36.0)))]
        } else {
            vec![("indentFirstLine", serde_json::Value::Null)]
        };
        self.run(move |s| cmd::set_paragraph_attrs(s, &patch));
    }

    /// A code block on or off for the selected paragraphs.
    pub fn toggle_code_block(&mut self, on: bool) {
        self.run(move |s| cmd::set_block_type(s, if on { "paragraph" } else { "codeBlock" }, None));
    }

    /// Reproduire la mise en forme: armed with the selection's look, applied by the next selection.
    pub fn toggle_format_painter(&mut self) {
        if self.painter.take().is_none() {
            self.painter = Some(cmd::capture_format(self.state.editor.state()));
        }
        self.invalidate();
    }

    pub fn format_painter_armed(&self) -> bool {
        self.painter.is_some()
    }

    fn apply_painter(&mut self) {
        let Some(cap) = self.painter.take() else { return };
        let word = self.state.editor.layout().map(|l| kubuno_office_docs_core::layout::caret::word_boundaries_at(l, self.state.editor.selection().head));
        self.run(move |s| cmd::apply_format(s, &cap, word));
    }

    /// Affichage › Marques ¶.
    pub fn set_show_marks(&mut self, on: bool) {
        self.show_marks = on;
        self.invalidate();
    }

    /// Révision › Statistiques: the word count in the status bar.
    pub fn show_word_count(&mut self) {
        let n = cmd::word_count(self.state.editor.doc());
        self.message = crate::Resources::status_words().replace("{words}", &n.to_string());
        self.invalidate();
    }

    /// The link under the selection (for the link dialog).
    pub fn current_link(&self) -> String {
        self.state.editor.selection_info().link.unwrap_or_default()
    }

    /// The selected text when it is on one line (what the find bar starts with).
    pub fn selected_line(&self) -> String {
        self.state.editor.selection_payload().map(|(_, t)| t).filter(|t| !t.contains('\n')).unwrap_or_default()
    }

    /// Finds the next (`backwards`: previous) match from the selection and selects it; returns
    /// its 1-based index and the number of matches.
    pub fn find(&mut self, query: &str, match_case: bool, backwards: bool) -> (usize, usize) {
        use kubuno_office_docs_core::edit::find::{find_all, FindOptions};
        let all = find_all(self.state.editor.doc(), query, FindOptions { match_case, whole_word: false });
        if all.is_empty() {
            return (0, 0);
        }
        let sel = self.state.editor.selection();
        let i = if backwards {
            all.iter().rposition(|m| m.0 < sel.from()).unwrap_or(all.len() - 1)
        } else {
            all.iter().position(|m| m.0 >= sel.to() || (m.0 > sel.from() && sel.is_empty())).unwrap_or(0)
        };
        let (f, t) = all[i];
        self.select_range(f, t);
        (i + 1, all.len())
    }

    /// Replaces the selected match (if the selection is one), then finds the next.
    pub fn replace(&mut self, query: &str, with: &str, match_case: bool) -> (usize, usize) {
        use kubuno_office_docs_core::edit::find::{find_all, FindOptions};
        let sel = self.state.editor.selection();
        let all = find_all(self.state.editor.doc(), query, FindOptions { match_case, whole_word: false });
        if all.iter().any(|m| *m == (sel.from(), sel.to())) {
            let (f, t, w) = (sel.from(), sel.to(), with.to_string());
            self.run(move |s| cmd::replace_text(s, f, t, &w));
            let after = self.state.editor.selection().to();
            self.state.editor.set_selection(Selection::caret(after));
        }
        self.find(query, match_case, false)
    }

    /// Replaces every match; returns how many.
    pub fn replace_all(&mut self, query: &str, with: &str, match_case: bool) -> usize {
        use kubuno_office_docs_core::edit::find::FindOptions;
        let mut count = 0;
        let (q, w) = (query.to_string(), with.to_string());
        self.run(|s| {
            let (c, n) = cmd::replace_all(s, &q, &w, FindOptions { match_case, whole_word: false })?;
            count = n;
            Ok(c)
        });
        count
    }

    /// Selects `from..to` and shows it.
    pub fn select_range(&mut self, from: usize, to: usize) {
        self.state.editor.set_selection(Selection::new(from, to));
        self.caret_moved_ms = now();
        self.reveal = true;
        self.invalidate();
    }

    // ── The rulers ─────────────────────────────────────────────────────────

    /// The page margins (document px) from the rulers; `commit` on release (recorded).
    pub fn set_page_margins(&mut self, left: f32, right: f32, top: f32, bottom: f32, commit: bool) {
        self.state.editor.set_margins(left, right, top, bottom, commit);
        self.edited();
    }

    /// The caret paragraph's indents (document px) from the ruler: previewed while dragging,
    /// recorded once on release.
    pub fn set_indents(&mut self, left: f32, first_line: f32, right: f32, commit: bool) {
        let r = if commit {
            self.state.editor.commit_transient(now(), move |s| cmd::set_indents(s, left, first_line, right))
        } else {
            self.state.editor.apply_transient(move |s| cmd::set_indents(s, left, first_line, right))
        };
        if let Err(e) = r {
            if e != EditError::NotApplicable {
                kubuno_desktop::tracing::warn!("[documents] indents: {e}");
            }
        }
        self.edited();
    }

    /// The caret paragraph's tab stops (`48:left,120:right`) from the ruler, with their kinds.
    pub fn set_tab_stops(&mut self, text: &str) {
        let stops: Vec<(f32, &'static str)> = parse_tab_stops(text).into_iter().map(|t| (t.pos, t.kind.key())).collect();
        self.run(move |s| cmd::set_tab_stops(s, &stops));
    }

    fn current_tab_stops(&self) -> Vec<TabStop> {
        self.state.editor.selection_info().tab_stops.into_iter().map(|(pos, kind)| TabStop { pos, kind: TabKind::parse(&kind) }).collect()
    }

    // ── Geometry: view ↔ pages ─────────────────────────────────────────────

    /// The view origin of the scrolled content (where content y = 0 is on screen).
    fn origin(&self) -> (f32, f32) {
        (self.viewport.left - self.state.scroll_x, self.viewport.top - self.state.scroll)
    }

    /// The page under (or nearest to) a view point, and the point in that page's content box
    /// (document px).
    fn hit_page(&self, x: f32, y: f32) -> Option<(usize, f32, f32)> {
        let (ox, oy) = self.origin();
        let zoom = self.state.zoom.max(0.01);
        let mut best: Option<(usize, f32)> = None;
        for (i, p) in self.state.places.iter().enumerate() {
            let (px0, py0) = (ox + p.x, oy + p.y);
            let dx = if x < px0 { px0 - x } else if x > px0 + p.w { x - px0 - p.w } else { 0.0 };
            let dy = if y < py0 { py0 - y } else if y > py0 + p.h { y - py0 - p.h } else { 0.0 };
            let d = dy * 4.0 + dx;
            if best.is_none_or(|(_, bd)| d < bd) {
                best = Some((i, d));
            }
        }
        let (i, _) = best?;
        let p = self.state.places[i];
        let s = self.state.setup_of_page(i);
        let lx = (x - ox - p.x) / zoom - s.left;
        let ly = ((y - oy - p.y) / zoom - s.top).clamp(0.0, s.content_h());
        Some((i, lx, ly))
    }

    /// The caret's box in view DIP: (x, y, height).
    fn caret_in_view(&self) -> Option<(f32, f32, f32)> {
        let m = self.measure();
        let (page, x, y, h, _) = self.state.editor.caret_on_page(&*m)?;
        let p = self.state.places.get(page)?;
        let s = self.state.setup_of_page(page);
        let (ox, oy) = self.origin();
        let z = self.state.zoom;
        Some((ox + p.x + (s.left + x) * z, oy + p.y + (s.top + y) * z, h * z))
    }

    /// Scrolls so the caret is in view (`scrollIntoView`).
    fn reveal_caret(&mut self) {
        let Some((x, y, h)) = self.caret_in_view() else { return };
        let vh = self.viewport_height();
        let pad = 24.0;
        if y < self.viewport.top + pad {
            self.state.scroll = (self.state.scroll - (self.viewport.top + pad - y)).max(0.0);
        } else if y + h > self.viewport.bottom - pad {
            self.state.scroll = (self.state.scroll + (y + h - (self.viewport.bottom - pad))).min(self.state.max_scroll(vh));
        }
        let vw = self.viewport.right - self.viewport.left;
        if max_scroll_x(self.viewport, &self.state) > 0.0 {
            if x < self.viewport.left + pad {
                self.state.scroll_x = (self.state.scroll_x - (self.viewport.left + pad - x)).max(0.0);
            } else if x > self.viewport.right - pad {
                self.state.scroll_x = (self.state.scroll_x + (x - (self.viewport.right - pad))).min((self.state.content_w - vw).max(0.0));
            }
        }
        self.state.current_page = self.state.page_at_scroll(vh);
    }

    // ── Painting ───────────────────────────────────────────────────────────

    fn view_args(&self) -> PageViewEventArgs {
        let pages = self.state.page_count.max(1);
        let vh = self.viewport_height();
        let m = self.measure();
        // The rulers follow the caret's page when it is visible (as on the web), else the page most
        // in view.
        let caret_page = self.state.editor.caret_on_page(&*m).map(|c| c.0).filter(|&i| {
            self.state.places.get(i).is_some_and(|p| p.y + p.h > self.state.scroll && p.y < self.state.scroll + vh)
        });
        let active = caret_page.unwrap_or_else(|| self.state.page_at_scroll(vh));
        let s = self.state.setup_of_page(active);
        let place = self.state.places.get(active).copied().unwrap_or(crate::model::state::PagePlace { x: 0.0, y: 0.0, w: s.width, h: s.height });
        let (ox, oy) = self.origin();
        let info = self.state.editor.selection_info();
        PageViewEventArgs {
            zoom: self.state.zoom,
            origin_x: ox + place.x - self.bounds.left,
            page_top: oy + place.y - self.viewport.top,
            page_width: s.width,
            page_height: s.height,
            margin_left: s.left,
            margin_right: s.right,
            margin_top: s.top,
            margin_bottom: s.bottom,
            current_page: active + 1,
            page_count: pages,
            indent_left: info.indent_left,
            indent_first_line: info.indent_first_line,
            indent_right: info.indent_right,
            tab_stops: format_tab_stops(&self.current_tab_stops()),
        }
    }

    fn editor_args(&self) -> EditorStateEventArgs {
        let e = &self.state.editor;
        let i = e.selection_info();
        let style = i.style_name.clone().unwrap_or_else(|| match (i.block.as_str(), i.level) {
            ("heading", l) => format!("h{l}"),
            _ => "normal".into(),
        });
        let style = match style.as_str() {
            "heading1" => "h1".to_string(),
            "heading2" => "h2".to_string(),
            "heading3" => "h3".to_string(),
            "heading4" => "h4".to_string(),
            s => s.to_string(),
        };
        EditorStateEventArgs {
            bold: i.bold,
            italic: i.italic,
            underline: i.underline,
            strike: i.strike,
            subscript: i.subscript,
            superscript: i.superscript,
            font_family: i.font_family.unwrap_or_default(),
            font_size: i.font_size.unwrap_or_default(),
            align: i.align,
            list: i.list.unwrap_or_default(),
            style,
            line_height: i.line_height,
            space_before: i.space_before > 0.0,
            space_after: i.space_after > 0.0,
            first_line: i.indent_first_line > 0.0,
            hanging: i.indent_first_line < 0.0,
            link: i.link.unwrap_or_default(),
            can_undo: e.can_undo(),
            can_redo: e.can_redo(),
            dirty: e.is_dirty(),
            has_selection: i.has_selection,
            in_table: i.in_table,
            keep_next: i.keep_next,
            keep_lines: i.keep_lines,
            page_break_before: i.page_break_before,
            code_block: i.block == "codeBlock",
            small_caps: i.small_caps,
            revision: e.revision,
            message: self.message.clone(),
        }
    }

    fn caret_visible(&self) -> bool {
        if !self.has_focus || !self.state.editor.selection().is_empty() {
            return false;
        }
        let t = now().saturating_sub(self.caret_moved_ms);
        t < BLINK_SOLID_MS || ((t - BLINK_SOLID_MS) / BLINK_HALF_MS) % 2 == 1
    }

    /// Milliseconds until the caret's next blink toggle.
    fn next_blink(&self) -> u32 {
        let t = now().saturating_sub(self.caret_moved_ms);
        if t < BLINK_SOLID_MS {
            return (BLINK_SOLID_MS - t) as u32;
        }
        (BLINK_HALF_MS - (t - BLINK_SOLID_MS) % BLINK_HALF_MS) as u32
    }

    fn paint_pages(&mut self, c: &dyn kubuno_desktop::controls::ControlCanvas) {
        let (backdrop, edge) = page_ground(c.theme());
        c.fill_rounded(&self.viewport, 0.0, &backdrop);
        let Some(renderer) = c.renderer() else { return };
        let Ok(painter) = Painter::new(renderer, c.theme()) else { return };
        painter.set_scale(c.scale());
        let Some((_, fonts)) = self.fonts.clone() else { return };
        let m: &dyn Measure = &*fonts;
        let sel_rects = self.state.editor.selection_on_pages(m);
        let caret = if self.caret_visible() { self.state.editor.caret_on_page(m) } else { None };
        let (ox, oy) = self.origin();
        c.push_clip(&self.viewport);
        let mut frame = paint::Frame { p: &painter, fonts: &fonts, images: &mut self.images, zoom: self.state.zoom, scale: c.scale(), edge, focused: self.has_focus, show_marks: self.show_marks };
        let vtop = self.viewport.top;
        let vbottom = self.viewport.bottom;
        for (i, page) in self.state.editor.pages().iter().enumerate() {
            let Some(place) = self.state.places.get(i) else { continue };
            let screen = crate::model::state::PagePlace { x: ox + place.x, y: oy + place.y, w: place.w, h: place.h };
            if screen.y > vbottom || screen.y + screen.h < vtop {
                continue;
            }
            let s = self.state.editor.page_setup(page.sec_idx);
            let rects: Vec<_> = sel_rects.iter().filter(|(p, _)| *p == i).map(|(_, r)| *r).collect();
            let car = caret.filter(|c| c.0 == i).map(|c| (c.1, c.2, c.3, c.4));
            paint::draw_page(&mut frame, page, &screen, (s.left, s.top), (s.width, s.height), &rects, car);
        }
        c.pop_clip();
    }

    fn paint_guide(&self, e: &PaintEventCx<'_>) {
        let Some(guide) = &self.guide else { return };
        let g = e.graphics;
        let v = self.viewport;
        let theme = g.theme_colors();
        let pen = Pen::new(Color::from(theme.accent), 1.0).with_dash(DashStyle::Dash);
        let (tip_back, tip_ink) = (Color::from(theme.tooltip_background), Color::from(theme.tooltip_foreground));
        let font = Font::default().sized(11.0);
        let tip = |text: &str, left: f32, top: f32| {
            if text.is_empty() {
                return;
            }
            let size = g.measure_string(text, &font, None, &StringFormat::generic_typographic());
            let r = Rect::new(left, top, left + size.width + 16.0, top + size.height + 6.0);
            g.fill_rounded_rectangle(Brush::solid(tip_back), r, 3.0);
            g.draw_string(text, &font, Brush::solid(tip_ink), r, &StringFormat::centered());
        };
        let args = self.view_args();
        if guide.vertical {
            let x = (self.bounds.left + guide.position).round() + 0.5;
            g.draw_line(&pen, PointF::new(x, v.top), PointF::new(x, v.bottom));
            let paper_w = (args.page_width * self.state.zoom).round();
            let left = self.bounds.left + args.origin_x + (guide.pointer - 44.0).clamp(0.0, (paper_w - 110.0).max(0.0));
            tip(&guide.label, left, v.top + crate::controls::ruler::RULER_OVERHANG + 4.0);
        } else {
            let y = (v.top + guide.position).round() + 0.5;
            g.draw_line(&pen, PointF::new(self.bounds.left, y), PointF::new(self.bounds.right, y));
            tip(&guide.label, self.bounds.left + 4.0, v.top + (guide.pointer - 12.0).max(0.0));
        }
    }

    /// The IME and the system caret follow ours.
    fn place_system_caret(&mut self) {
        if !self.has_focus {
            return;
        }
        let Some((x, y, h)) = self.caret_in_view() else { return };
        let Some(hwnd) = ime::focus_window() else { return };
        ime::place_composition(hwnd, x, y, h, self.scale);
        self.sys_caret.place(hwnd, x, y, h, self.scale);
    }

    /// Auto-scroll while a selection drag holds the pointer near the top or bottom edge.
    fn auto_scroll(&mut self) {
        let Some((x, y)) = self.drag_at else { return };
        if !self.state.editor.pressing() {
            self.drag_at = None;
            return;
        }
        let v = self.viewport;
        let dy = if y > v.bottom - AUTO_EDGE {
            ((y - (v.bottom - AUTO_EDGE)) / 2.0 + 5.0).min(30.0)
        } else if y < v.top + AUTO_EDGE {
            -(((v.top + AUTO_EDGE) - y) / 2.0 + 5.0).min(30.0)
        } else {
            0.0
        };
        if dy == 0.0 {
            return;
        }
        let before = self.state.scroll;
        self.state.scroll_by(dy, self.viewport_height());
        if self.state.scroll != before {
            self.drag_select(x, y);
            kubuno_desktop::controls::host::request_repaint_after(16);
        }
    }

    fn drag_select(&mut self, x: f32, y: f32) {
        if let Some((page, lx, ly)) = self.hit_page(x, y) {
            let (cx, cy) = self.state.editor.page_to_continuous(page, lx, ly);
            let m = self.measure();
            self.state.editor.drag_to(cx, cy, &*m);
            self.caret_moved_ms = now();
            self.invalidate();
        }
    }

    // ── Keys ───────────────────────────────────────────────────────────────

    fn motion(&mut self, motion: Motion, extend: bool) {
        let m = self.measure();
        self.state.editor.move_caret(motion, extend, &*m);
        self.caret_moved_ms = now();
        self.reveal = true;
        self.invalidate();
    }

    /// The keys the canvas handles itself (the ribbon's command shortcuts are taken before).
    fn key(&mut self, key: u16, mods: kubuno_desktop::controls::host::Modifiers) -> bool {
        use kubuno_desktop::controls::host::vk;
        let (ctrl, shift, alt) = (mods.ctrl, mods.shift, mods.alt);
        // AltGr is Ctrl+Alt: never a shortcut, its character arrives through WM_CHAR.
        if ctrl && alt {
            return false;
        }
        let vh = (self.viewport_height() / self.state.zoom.max(0.01)) as i32;
        match key {
            k if k == vk::LEFT => self.motion(if ctrl { Motion::WordLeft } else { Motion::Left }, shift),
            k if k == vk::RIGHT => self.motion(if ctrl { Motion::WordRight } else { Motion::Right }, shift),
            k if k == vk::UP => self.motion(Motion::Up, shift),
            k if k == vk::DOWN => self.motion(Motion::Down, shift),
            k if k == vk::HOME => self.motion(if ctrl { Motion::DocStart } else { Motion::LineStart }, shift),
            k if k == vk::END => self.motion(if ctrl { Motion::DocEnd } else { Motion::LineEnd }, shift),
            k if k == vk::PAGE_UP => self.motion(Motion::PageUp(vh), shift),
            k if k == vk::PAGE_DOWN => self.motion(Motion::PageDown(vh), shift),
            k if k == vk::BACK => {
                let (prev, _) = self.state.editor.word_targets();
                self.run(move |s| cmd::delete_backward(s, ctrl, prev));
            }
            k if k == vk::DELETE => {
                let (_, next) = self.state.editor.word_targets();
                self.run(move |s| cmd::delete_forward(s, ctrl, next));
            }
            k if k == vk::ENTER => {
                if ctrl {
                    self.insert_page_break();
                } else if shift {
                    self.run(cmd::insert_hard_break);
                } else {
                    self.run(cmd::enter);
                }
            }
            k if k == vk::TAB && !ctrl => self.tab(shift),
            k if k == vk::letter('A') && ctrl && !shift => self.select_all(),
            k if k == vk::letter('V') && ctrl && shift => self.paste(true),
            k if k == vk::ESCAPE => return false,
            _ => return false,
        }
        true
    }

    fn tab(&mut self, shift: bool) {
        // Tab may only move the caret to another cell (no change): run it through the editor so a
        // change is recorded and a move is not.
        let mut moved_to = None;
        let r = self.state.editor.apply(now(), |s| match cmd::tab(s, shift) {
            Ok(cmd::TabOutcome::Changed(c)) => Ok(c),
            Ok(cmd::TabOutcome::Moved) => {
                moved_to = Some(s.sel);
                Err(EditError::NotApplicable)
            }
            Ok(cmd::TabOutcome::Nothing) => Err(EditError::NotApplicable),
            Err(e) => Err(e),
        });
        if let Some(sel) = moved_to {
            self.state.editor.set_selection(sel);
        }
        let _ = r;
        self.edited();
    }
}

impl Control for PageCanvas {
    fn get_preferred_size(&self, _canvas: &dyn Canvas, _proposed: Size) -> Size {
        Size { width: 800.0, height: 600.0 }
    }

    fn on_paint(&mut self, e: &mut PaintEventCx<'_>) {
        let b = e.clip_rectangle;
        self.bounds = b;
        let top = (b.top + self.viewport_top.max(0.0)).min(b.bottom);
        self.viewport = Rect::new(b.left, top, b.right, b.bottom);
        let c = e.canvas();
        self.scale = c.scale();
        // The DirectWrite measurer, rebuilt with the factory it was made from.
        if let Some(renderer) = c.renderer() {
            let key = windows::core::Interface::as_raw(&renderer.dwrite) as usize;
            if self.fonts.as_ref().is_none_or(|(k, _)| *k != key) {
                self.fonts = Some((key, Rc::new(Fonts::new(renderer.dwrite.clone()))));
                // Measured with another measurer before: lay out again.
                self.state.editor.mark_measure_changed();
            }
        }
        self.ensure_layout();
        self.auto_scroll();
        if self.reveal {
            self.reveal = false;
            self.reveal_caret();
        }
        let vh = self.viewport_height();
        self.state.scroll = self.state.scroll.clamp(0.0, self.state.max_scroll(vh));
        self.state.scroll_x = self.state.scroll_x.clamp(0.0, max_scroll_x(self.viewport, &self.state));
        c.fill_rounded(&Rect::new(b.left, b.top, b.right, top), 0.0, &page_ground(c.theme()).0);
        self.paint_pages(c);
        if let Some((bar, rail)) = scrollbar(self.viewport, &self.state) {
            let st = if self.state.scroll_hot { WidgetState::REST.hot(true) } else { WidgetState::REST };
            bar.paint(c, rail, st);
        }
        if let Some((bar, rail)) = hscrollbar(self.viewport, &self.state, self.hscroll_hot) {
            bar.paint(c, rail, if self.hscroll_hot { WidgetState::REST.hot(true) } else { WidgetState::REST });
        }
        self.paint_guide(e);
        self.place_system_caret();
        if self.has_focus && self.state.editor.selection().is_empty() {
            kubuno_desktop::controls::host::request_repaint_after(self.next_blink().max(16));
        }
        if !self.design_mode() {
            let args = self.view_args();
            if self.reported.as_ref() != Some(&args) {
                self.reported = Some(args.clone());
                self.raise_view_changed(args);
                kubuno_desktop::controls::host::request_repaint_after(0);
            }
            let st = self.editor_args();
            if self.reported_state.as_ref() != Some(&st) {
                self.reported_state = Some(st.clone());
                self.raise_editor_state_changed(st);
                kubuno_desktop::controls::host::request_repaint_after(0);
            }
        }
        e.raise(self, "OnPaint");
    }

    fn is_input_key(&self, key: Keys) -> bool {
        use kubuno_desktop::controls::host::vk;
        [vk::PAGE_DOWN, vk::PAGE_UP, vk::DOWN, vk::UP, vk::LEFT, vk::RIGHT, vk::HOME, vk::END, vk::TAB, vk::ENTER, vk::BACK, vk::DELETE, vk::SPACE, vk::ESCAPE].contains(&key.key.0)
    }

    fn on_got_focus(&mut self, e: &mut EventCx<'_, kubuno_desktop::views::events::EmptyEventArgs>) {
        self.has_focus = true;
        self.caret_moved_ms = now();
        self.invalidate();
        e.raise(&*self, "OnGotFocus");
    }

    fn on_lost_focus(&mut self, e: &mut EventCx<'_, kubuno_desktop::views::events::EmptyEventArgs>) {
        self.has_focus = false;
        self.sys_caret.destroy();
        self.invalidate();
        e.raise(&*self, "OnLostFocus");
    }

    fn on_mouse_wheel(&mut self, e: &mut EventCx<'_, MouseEventArgs>) {
        let notches = e.args().delta;
        let h = self.viewport_height();
        if e.args().mods.ctrl {
            let factor = if notches < 0.0 { 1.1 } else { 1.0 / 1.1 };
            let zoom = self.state.zoom * factor;
            self.state.set_zoom(zoom, h);
        } else if e.args().mods.shift {
            self.state.scroll_x = (self.state.scroll_x + notches * WHEEL_STEP).clamp(0.0, max_scroll_x(self.viewport, &self.state));
        } else {
            self.state.scroll_by(notches * WHEEL_STEP, h);
        }
        self.after_scroll();
        e.raise(&*self, "OnMouseWheel");
    }

    fn on_mouse_down(&mut self, e: &mut EventCx<'_, MouseEventArgs>) {
        let (x, y) = (e.args().x + self.bounds.left, e.args().y + self.bounds.top);
        let button = e.args().button;
        let shift = e.args().mods.shift;
        self.focus();
        self.has_focus = true;
        let h = self.viewport_height();
        if let Some((bar, rail)) = scrollbar(self.viewport, &self.state).filter(|(_, rail)| rail.contains(x, y)) {
            match bar.part_at(rail, x, y) {
                Some(ScrollPart::Thumb) => self.state.scroll_grab = Some(y - bar.thumb_rect(rail).top),
                Some(ScrollPart::PageLow) => self.state.scroll_by(-h, h),
                Some(ScrollPart::PageHigh) => self.state.scroll_by(h, h),
                Some(ScrollPart::ArrowLow) => self.state.scroll_by(-LINE_STEP, h),
                Some(ScrollPart::ArrowHigh) => self.state.scroll_by(LINE_STEP, h),
                None => {}
            }
            self.after_scroll();
        } else if let Some((bar, rail)) = hscrollbar(self.viewport, &self.state, true).filter(|(_, rail)| rail.contains(x, y)) {
            let page_x = self.viewport.right - self.viewport.left;
            let max = max_scroll_x(self.viewport, &self.state);
            match bar.part_at(rail, x, y) {
                Some(ScrollPart::Thumb) => self.hscroll_grab = Some(x - bar.thumb_rect(rail).left),
                Some(ScrollPart::PageLow) => self.state.scroll_x = (self.state.scroll_x - page_x).clamp(0.0, max),
                Some(ScrollPart::PageHigh) => self.state.scroll_x = (self.state.scroll_x + page_x).clamp(0.0, max),
                Some(ScrollPart::ArrowLow) => self.state.scroll_x = (self.state.scroll_x - LINE_STEP).clamp(0.0, max),
                Some(ScrollPart::ArrowHigh) => self.state.scroll_x = (self.state.scroll_x + LINE_STEP).clamp(0.0, max),
                None => {}
            }
            self.after_scroll();
        } else if self.viewport.contains(x, y) {
            if let Some((page, lx, ly)) = self.hit_page(x, y) {
                let (cx, cy) = self.state.editor.page_to_continuous(page, lx, ly);
                let m = self.measure();
                if button == MouseButton::Left {
                    // Click counting (`clickCount`): 500 ms, 4 DIP.
                    let t = now();
                    let (lt, lx0, ly0, n) = self.last_click;
                    let near = (x - lx0).abs() <= MULTI_CLICK_SLOP && (y - ly0).abs() <= MULTI_CLICK_SLOP;
                    let count = if near && t.saturating_sub(lt) <= MULTI_CLICK_MS { n + 1 } else { 1 };
                    self.last_click = (t, x, y, count);
                    self.state.editor.press(cx, cy, count.min(3), shift && count == 1, &*m);
                    self.drag_at = Some((x, y));
                    self.message.clear();
                } else if button == MouseButton::Right {
                    // A right click outside the selection moves the caret; inside, it keeps it (web).
                    let layout_pos = self.state.editor.layout().map(|l| kubuno_office_docs_core::layout::caret::coords_to_pos(l, cx, cy, &*m));
                    let sel = self.state.editor.selection();
                    if let Some(pos) = layout_pos {
                        if sel.is_empty() || pos < sel.from() || pos > sel.to() {
                            self.state.editor.set_selection(Selection::caret(pos));
                        }
                    }
                }
                self.caret_moved_ms = now();
                self.invalidate();
            }
        }
        e.raise(&*self, "OnMouseDown");
    }

    fn on_mouse_move(&mut self, e: &mut EventCx<'_, MouseEventArgs>) {
        let (x, y) = (e.args().x + self.bounds.left, e.args().y + self.bounds.top);
        if e.args().button != MouseButton::Left {
            self.state.scroll_grab = None;
            self.hscroll_grab = None;
            if self.state.editor.pressing() {
                self.state.editor.release();
                self.drag_at = None;
            }
        }
        if self.state.editor.pressing() && e.args().button == MouseButton::Left {
            self.drag_at = Some((x, y));
            self.drag_select(x, y);
            kubuno_desktop::controls::host::request_repaint_after(16);
        }
        let hbar = hscrollbar(self.viewport, &self.state, self.hscroll_hot);
        let hhot = self.hscroll_grab.is_some() || hbar.as_ref().is_some_and(|(_, rail)| rail.inflate(0.0, 4.0).contains(x, y));
        if hhot != self.hscroll_hot {
            self.hscroll_hot = hhot;
            self.invalidate();
        }
        if let (Some(grab), Some((bar, rail))) = (self.hscroll_grab, hbar) {
            let max = max_scroll_x(self.viewport, &self.state);
            self.state.scroll_x = scroll_from_thumb_along(&bar, &rail, x, grab, max, true);
            self.after_scroll();
        }
        let bar = scrollbar(self.viewport, &self.state);
        let hot = self.state.scroll_grab.is_some() || bar.as_ref().is_some_and(|(_, rail)| rail.inflate(4.0, 0.0).contains(x, y));
        if hot != self.state.scroll_hot {
            self.state.scroll_hot = hot;
            self.invalidate();
        }
        if let (Some(grab), Some((bar, rail))) = (self.state.scroll_grab, bar) {
            let max = self.state.max_scroll(self.viewport_height());
            self.state.scroll = scroll_from_thumb_along(&bar, &rail, y, grab, max, false);
            self.after_scroll();
        }
        e.raise(&*self, "OnMouseMove");
    }

    fn on_mouse_up(&mut self, e: &mut EventCx<'_, MouseEventArgs>) {
        self.state.scroll_grab = None;
        self.hscroll_grab = None;
        let was_selecting = self.state.editor.pressing();
        self.state.editor.release();
        // The format painter applies to the selection just made (a click: the word under it).
        if was_selecting && self.painter.is_some() {
            self.apply_painter();
        }
        self.drag_at = None;
        e.raise(&*self, "OnMouseUp");
    }

    fn on_mouse_leave(&mut self, e: &mut EventCx<'_, kubuno_desktop::views::events::EmptyEventArgs>) {
        if self.state.scroll_grab.is_none() && std::mem::take(&mut self.state.scroll_hot) {
            self.invalidate();
        }
        if self.hscroll_grab.is_none() && std::mem::take(&mut self.hscroll_hot) {
            self.invalidate();
        }
        e.raise(&*self, "OnMouseLeave");
    }

    fn cursor_at(&self, x: f32, y: f32) -> Option<kubuno_desktop::controls::host::Cursor> {
        let (x, y) = (x + self.bounds.left, y + self.bounds.top);
        let over_bar = scrollbar(self.viewport, &self.state).is_some_and(|(_, r)| r.contains(x, y));
        (self.viewport.contains(x, y) && !over_bar).then_some(kubuno_desktop::controls::host::Cursor::IBeam)
    }

    fn on_key_down(&mut self, e: &mut EventCx<'_, KeyEventArgs>) {
        let (key, mods) = (e.args().key.0, e.args().mods);
        if self.key(key, mods) {
            e.args_mut().handled = true;
            e.args_mut().suppress_key_press = true;
        }
        e.raise(&*self, "OnKeyDown");
    }

    fn on_key_press(&mut self, e: &mut EventCx<'_, KeyPressEventArgs>) {
        let c = e.args().key_char;
        if !c.is_control() {
            let text = c.to_string();
            self.run(move |s| cmd::insert_text(s, &text));
            e.args_mut().handled = true;
        }
        e.raise(&*self, "OnKeyPress");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn canvas_with(json: &str) -> PageCanvas {
        let mut c = PageCanvas::default();
        let editor = kubuno_office_docs_core::editor::Editor::open(json.as_bytes()).expect("opens");
        c.open(App::new(editor, "t".into()));
        c.viewport = Rect::new(0.0, 20.0, 1200.0, 900.0);
        c.ensure_layout();
        c
    }

    const DOC: &str = r#"{"content":[{"content":[{"text":"hello world","type":"text"}],"type":"paragraph"}],"type":"doc"}"#;

    #[test]
    fn typing_and_keys_edit_the_document() {
        let mut c = canvas_with(DOC);
        c.state.editor.set_selection(Selection::caret(6));
        c.run(|s| cmd::insert_text(s, ","));
        let mods = kubuno_desktop::controls::host::Modifiers::NONE;
        assert!(c.key(kubuno_desktop::controls::host::vk::ENTER, mods));
        let text = kubuno_office_docs_core::edit::clipboard::plain_text(c.state.editor.doc(), 0, 100);
        assert_eq!(text, "hello,\n world");
        assert!(c.key(kubuno_desktop::controls::host::vk::BACK, mods));
        let text = kubuno_office_docs_core::edit::clipboard::plain_text(c.state.editor.doc(), 0, 100);
        assert_eq!(text, "hello, world");
        c.undo();
        let text = kubuno_office_docs_core::edit::clipboard::plain_text(c.state.editor.doc(), 0, 100);
        assert_eq!(text, "hello world");
    }

    #[test]
    fn a_click_on_the_page_places_the_caret() {
        let mut c = canvas_with(DOC);
        let p = c.state.places[0];
        let s = c.state.setup_of_page(0);
        let (ox, oy) = c.origin();
        // Far right of the first line: the end of the text.
        let x = ox + p.x + (s.left + 500.0) * c.state.zoom;
        let y = oy + p.y + (s.top + 5.0) * c.state.zoom;
        let (page, lx, ly) = c.hit_page(x, y).expect("a page");
        let (cx, cy) = c.state.editor.page_to_continuous(page, lx, ly);
        c.state.editor.press(cx, cy, 1, false, &FixedMeasure);
        assert_eq!(c.state.editor.selection(), Selection::caret(12));
    }

    #[test]
    fn the_ruler_writes_the_document_and_undoes_in_one_step() {
        let mut c = canvas_with(DOC);
        c.set_indents(30.0, 0.0, 0.0, false);
        c.set_indents(60.0, 12.0, 0.0, true);
        let a = c.state.editor.doc().children()[0].attrs();
        assert_eq!(a.get("indentLeft").and_then(|v| v.as_i64()), Some(60));
        c.set_tab_stops("96:center,48:left");
        assert_eq!(format_tab_stops(&c.current_tab_stops()), "48:left,96:center");
        c.undo();
        c.undo();
        assert!(c.state.editor.doc().children()[0].attr("indentLeft").is_none());
    }
}
