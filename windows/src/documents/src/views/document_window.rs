//! Code-behind of Kubuno Documents' window (`document_window.kbview`) — Windows Forms' `Form1.cs`.
//!
//! The age canvas owns the open document (the core's editor) and its view state; the window opens
//! the document into it, binds what it reports (`ViewChanged`: the rulers, the status bar, the
//! zoom; `EditorStateChanged`: the ribbon's checked states, undo/redo, the title's « modifié »),
//! and turns the ribbon's commands, the context menu and the rulers into the canvas's edits.

use std::path::PathBuf;

use kubuno_desktop::prelude::*;
use kubuno_desktop::views::component::Control as _;
use kubuno_office_docs_core::model::Node;
use serde_json::{json, Value as Json};

use crate::controls::page_canvas::{DragGuide, EditorStateEventArgs, PageCanvas, PageViewEventArgs};
use crate::controls::ruler::{RulerGuideEventArgs, RulerIndentsEventArgs, RulerMarginsEventArgs, RulerTabStopsEventArgs};
use crate::model::state::App;
use crate::Resources;

/// How the window starts (the command line, read by `main`).
#[derive(Debug, Clone, Default)]
pub struct Options {
    /// A `content_json` file to open instead of the built-in sample.
    pub file: Option<PathBuf>,
    /// `--dark`: the dark theme.
    pub dark: bool,
    /// `--culture fr|en`: the UI language (else the system's).
    pub culture: Option<String>,
    /// `--zoom 50`: the zoom to open at, in percent.
    pub zoom: Option<f32>,
    /// `--doc <id>`: a document of the server, opened as the account the shell shows.
    pub doc: Option<String>,
    /// The title bar's waffle and avatar show the offline sample (the shell controls' design data) instead of
    /// the account's: `--sample`, or a Debug build under a debugger without `--live`.
    pub sample: bool,
}

impl Options {
    pub fn from_args() -> Self {
        let args: Vec<String> = std::env::args().collect();
        Self { sample: kubuno_desktop_header_data::sample_requested(&args), ..Self::parse(args.into_iter().skip(1)) }
    }

    pub fn parse(args: impl IntoIterator<Item = String>) -> Self {
        let mut options = Options::default();
        let mut args = args.into_iter();
        while let Some(a) = args.next() {
            match a.as_str() {
                "--dark" => options.dark = true,
                "--culture" => options.culture = args.next(),
                "--zoom" => options.zoom = args.next().and_then(|z| z.trim_end_matches('%').parse::<f32>().ok()),
                "--doc" => options.doc = args.next(),
                "--no-splash" | "--light" => {}
                _ if a.starts_with("--") => {}
                _ => options.file = Some(PathBuf::from(a)),
            }
        }
        options
    }

    /// The document to open: the file named on the command line, else the sample.
    pub fn open_document(&self) -> App {
        match &self.file {
            Some(path) => App::open_file(path).unwrap_or_else(|why| {
                kubuno_desktop::tracing::error!("[documents] {why}");
                App::default()
            }),
            None => App::default(),
        }
    }
}

/// Kubuno Documents' main window.
#[kubuno_desktop::view("document_window.kbview")]
pub struct DocumentWindow {
    #[control]
    page: Custom<PageCanvas>,
    #[bind]
    window_title: String,
    #[bind]
    show_ruler: bool,
    #[bind]
    ruler_inset: f32,
    // ── What the page canvas reports, bound to the rulers ──
    #[bind]
    zoom: f32,
    #[bind]
    page_origin: f32,
    #[bind]
    page_top: f32,
    #[bind]
    page_width: f32,
    #[bind]
    page_height: f32,
    #[bind]
    margin_left: f32,
    #[bind]
    margin_right: f32,
    #[bind]
    margin_top: f32,
    #[bind]
    margin_bottom: f32,
    #[bind]
    indent_left: f32,
    #[bind]
    indent_first_line: f32,
    #[bind]
    indent_right: f32,
    #[bind]
    tab_stops: String,
    #[bind]
    tab_type: String,
    // ── The status bar and the Backstage ──
    #[bind]
    page_text: String,
    #[bind]
    hidden_status: String,
    #[bind]
    has_hidden: bool,
    #[bind]
    zoom_percent: f32,
    #[bind]
    doc_title: String,
    #[bind]
    pages_text: String,
    #[bind]
    zoom_text: String,
    #[bind]
    hidden_text: String,
    // ── The ribbon's state, from the selection ──
    #[bind]
    font_families: Rows,
    #[bind]
    font_family: String,
    #[bind]
    font_size: String,
    #[bind]
    style_value: String,
    #[bind]
    bold: bool,
    #[bind]
    italic: bool,
    #[bind]
    underline: bool,
    #[bind]
    strike: bool,
    #[bind]
    subscript: bool,
    #[bind]
    superscript: bool,
    #[bind]
    format_painter: bool,
    #[bind]
    list_bullet: bool,
    #[bind]
    list_ordered: bool,
    #[bind]
    list_task: bool,
    #[bind]
    align_left: bool,
    #[bind]
    align_center: bool,
    #[bind]
    align_right: bool,
    #[bind]
    align_justify: bool,
    #[bind]
    ls1: bool,
    #[bind]
    ls1_15: bool,
    #[bind]
    ls1_5: bool,
    #[bind]
    ls2: bool,
    #[bind]
    ls2_5: bool,
    #[bind]
    ls3: bool,
    #[bind]
    space_before: bool,
    #[bind]
    space_after: bool,
    #[bind]
    first_line: bool,
    #[bind]
    hanging: bool,
    #[bind]
    keep_next: bool,
    #[bind]
    keep_lines: bool,
    #[bind]
    page_break_before: bool,
    #[bind]
    on_link: bool,
    #[bind]
    code_block: bool,
    #[bind]
    show_marks: bool,
    // ── The find bar ──
    #[bind]
    show_find: bool,
    #[bind]
    show_replace: bool,
    #[bind]
    find_text: String,
    #[bind]
    replace_text: String,
    #[bind]
    find_status: String,
    find_match_case: bool,
    #[bind]
    page_number_position: String,
    #[bind]
    orientation: String,
    #[bind]
    paper_size: String,
    #[bind]
    columns: String,
    #[bind]
    citation_style: String,
    #[bind]
    can_save: bool,
    #[bind]
    can_undo: bool,
    #[bind]
    can_redo: bool,
    /// The document to open at load (the canvas exists once the window is open).
    pending: Option<App>,
    /// The server document's session (`--doc`), kept in step by a background thread.
    live: Option<crate::api::live::Live>,
    /// The server document to open at load.
    open_doc: Option<String>,
    /// The last edit revision published to the session.
    last_revision: u64,
    #[bind]
    sync_status: String,
    #[bind]
    has_sync: bool,
    /// Where the document was opened from (a local file), for Ctrl+S.
    file: Option<PathBuf>,
    /// The zoom to open at (`--zoom`).
    open_zoom: Option<f32>,
    /// The title bar's header shows the offline sample (`Options::sample`).
    header_sample: bool,
    /// The last colours applied (the split buttons' main part reapplies them).
    last_color: String,
    last_highlight: String,
}

impl DocumentWindow {
    pub fn new(options: Options, document: App) -> Self {
        let mut window = Self {
            window_title: format!("{} — {}", document.title, Resources::app_title()),
            pending: Some(document),
            file: options.file.clone(),
            open_doc: options.doc.clone(),
            open_zoom: options.zoom,
            header_sample: options.sample,
            show_ruler: true,
            ruler_inset: crate::controls::ruler::RULER_SZ,
            zoom: 1.0,
            page_origin: f32::NAN,
            page_width: kubuno_office_docs_core::editor::PAGE_W,
            page_height: kubuno_office_docs_core::editor::PAGE_H,
            margin_left: kubuno_office_docs_core::editor::MARGIN,
            margin_right: kubuno_office_docs_core::editor::MARGIN,
            margin_top: kubuno_office_docs_core::editor::MARGIN,
            margin_bottom: kubuno_office_docs_core::editor::MARGIN,
            tab_type: "left".to_string(),
            zoom_percent: 100.0,
            font_family: "Arial".to_string(),
            font_size: "11".to_string(),
            style_value: "normal".to_string(),
            align_left: true,
            ls1_15: true,
            page_number_position: "none".to_string(),
            orientation: "portrait".to_string(),
            paper_size: "a4".to_string(),
            columns: "1".to_string(),
            citation_style: "APA".to_string(),
            last_color: "#ff0000".to_string(),
            last_highlight: "#fff475".to_string(),
            ..Self::default()
        };
        window.initialize_component();
        window
    }

    fn document_window_load(&mut self) {
        self.font_families = font_rows();
        if let Some(document) = self.pending.take() {
            self.doc_title = document.title.clone();
            self.hidden_text = Resources::info_nothing_hidden().to_string();
            let zoom = self.open_zoom.map(|z| z / 100.0);
            self.page.with(|p| {
                p.open(document);
                if let Some(z) = zoom {
                    p.set_zoom(z);
                }
                p.focus();
            });
        }
        if let Some(id) = self.open_doc.take() {
            self.start_live(id);
        }
        // The title bar's waffle and avatar: the account's apps, favourites and other accounts, through the
        // shell's broker (the sample: the controls' design data).
        let mut header = kubuno_desktop_header_data::FeedConfig::new("kubuno-documents");
        if !self.header_sample {
            header.proxy = kubuno_desktop_sync::get_proxy();
        }
        kubuno_desktop_header_data::start(kubuno_desktop_header_data::HeaderOptions::for_app(&["office-documents"]), header, self.header_sample, self.dispatcher());
    }

    // ── A document of the server ────────────────────────────────────────────

    /// Opens document `id` of the server: a background session joins it, keeps the crash journal and
    /// saves (vskubuno docs/DOCUMENTS-EDITING.md §D); its events come back on the UI thread.
    fn start_live(&mut self, id: String) {
        let Some(dispatcher) = self.dispatcher() else {
            kubuno_desktop::tracing::warn!("[documents] the window has no dispatcher: the server document cannot open");
            return;
        };
        self.sync_status = Resources::sync_opening().to_string();
        self.has_sync = true;
        let post = dispatcher.clone();
        let sink: crate::api::live::Sink = std::sync::Arc::new(move |event| {
            drop(post.begin_invoke(move |w: &mut DocumentWindow| w.on_live(event)));
        });
        let doc = id.clone();
        self.live = Some(crate::api::live::Live::open(id, sink, move || crate::api::remote::connect(&doc)));
    }

    fn document_bytes(&mut self) -> Option<Vec<u8>> {
        self.page.with(|p| p.state_mut().editor.to_bytes().ok()).flatten()
    }

    /// Loads `bytes` into the canvas, titled `title`.
    fn load_bytes(&mut self, bytes: &[u8], title: &str) -> bool {
        match kubuno_office_docs_core::editor::Editor::open(bytes) {
            Ok(editor) => {
                self.doc_title = title.to_string();
                self.page.with(|p| {
                    p.open(App::new(editor, title.to_string()));
                    p.focus();
                });
                true
            }
            Err(e) => {
                self.sync_status = Resources::open_failed().replace("{why}", &e.to_string());
                false
            }
        }
    }

    fn on_live(&mut self, event: crate::api::live::Event) {
        use crate::api::live::Event;
        use crate::api::session::{Protection, Resolution, State};
        match event {
            Event::Opened { title, content, recovered, protection, offline } => {
                self.load_bytes(&content, &title);
                if let Some(copy) = recovered {
                    let restore = super::confirm_dialog::ConfirmDialog::ask(self, Resources::recover_title(), Resources::recover_text(), Resources::recover_yes(), Resources::recover_no());
                    if restore && self.load_bytes(&copy, &title) {
                        // The restored copy is unsaved work: published to the session, saved by it.
                        if let Some(live) = &self.live {
                            live.recovered(true);
                            live.edited(copy, false);
                        }
                    } else if let Some(live) = &self.live {
                        live.recovered(false);
                    }
                }
                self.sync_status = if offline {
                    Resources::sync_offline().to_string()
                } else if protection == Protection::DigestOnly {
                    Resources::sync_digest_only().to_string()
                } else {
                    Resources::sync_saved().to_string()
                };
            }
            Event::Failed(why) => {
                self.sync_status = Resources::open_failed().replace("{why}", &why);
            }
            Event::Status(s) => {
                let mut text = match s.state {
                    State::Clean if !s.unsaved => Resources::sync_saved().to_string(),
                    State::Saving => Resources::sync_saving().to_string(),
                    State::Failed => Resources::sync_failed().to_string(),
                    State::TooLarge => Resources::sync_too_large().to_string(),
                    State::Conflict => Resources::sync_conflict().to_string(),
                    _ => Resources::sync_unsaved().to_string(),
                };
                if s.protection == Protection::DigestOnly && matches!(s.state, State::Clean | State::Dirty) {
                    text = format!("{text} · {}", Resources::sync_digest_only());
                }
                if !s.editors.is_empty() {
                    text = format!("{text} · {}", Resources::sync_editors().replace("{names}", &s.editors.join(", ")));
                }
                self.sync_status = text;
                if s.state == State::Clean && !s.unsaved {
                    self.page.with(|p| {
                        p.state_mut().editor.mark_saved();
                        p.invalidate();
                    });
                }
            }
            Event::Conflict => {
                let mine = super::confirm_dialog::ConfirmDialog::ask(self, Resources::conflict_title(), Resources::conflict_text(), Resources::conflict_mine(), Resources::conflict_theirs());
                if let Some(live) = &self.live {
                    live.resolve(if mine { Resolution::KeepMine } else { Resolution::TakeTheirs });
                }
            }
            Event::Reload(content) => {
                let title = self.doc_title.clone();
                self.load_bytes(&content, &title);
            }
            Event::Closed => {}
        }
    }

    /// The window closes: the session saves what it can, keeps the rest in the journal and leaves.
    fn document_window_closing(&mut self) {
        if let Some(live) = self.live.take() {
            let bytes = self.document_bytes();
            if !live.close(bytes) {
                kubuno_desktop::tracing::warn!("[documents] the server session did not finish closing in time; the local copy is kept");
            }
        }
    }

    // ── The page canvas ─────────────────────────────────────────────────────

    fn page_view_changed(&mut self, e: &PageViewEventArgs) {
        self.zoom = e.zoom;
        self.page_origin = e.origin_x;
        self.page_top = e.page_top;
        self.page_width = e.page_width;
        self.page_height = e.page_height;
        self.margin_left = e.margin_left;
        self.margin_right = e.margin_right;
        self.margin_top = e.margin_top;
        self.margin_bottom = e.margin_bottom;
        self.indent_left = e.indent_left;
        self.indent_first_line = e.indent_first_line;
        self.indent_right = e.indent_right;
        self.tab_stops = e.tab_stops.clone();
        let percent = (e.zoom * 100.0).round();
        self.zoom_percent = percent;
        self.zoom_text = format!("{percent} %");
        self.pages_text = e.page_count.max(1).to_string();
        self.page_text = Resources::status_page().replace("{current}", &e.current_page.to_string()).replace("{pages}", &e.page_count.max(1).to_string());
    }

    fn page_editor_state_changed(&mut self, e: &EditorStateEventArgs) {
        self.bold = e.bold;
        self.italic = e.italic;
        self.underline = e.underline;
        self.strike = e.strike;
        self.subscript = e.subscript;
        self.superscript = e.superscript;
        self.font_family = e.font_family.clone();
        self.font_size = e.font_size.clone();
        self.style_value = e.style.clone();
        self.list_bullet = e.list == "bulletList";
        self.list_ordered = e.list == "orderedList";
        self.list_task = e.list == "taskList";
        self.align_left = e.align == "left";
        self.align_center = e.align == "center";
        self.align_right = e.align == "right";
        self.align_justify = e.align == "justify";
        let ls = |v: f32| (e.line_height - v).abs() < 0.001;
        (self.ls1, self.ls1_15, self.ls1_5, self.ls2, self.ls2_5, self.ls3) = (ls(1.0), ls(1.15), ls(1.5), ls(2.0), ls(2.5), ls(3.0));
        self.space_before = e.space_before;
        self.space_after = e.space_after;
        self.first_line = e.first_line;
        self.hanging = e.hanging;
        self.keep_next = e.keep_next;
        self.keep_lines = e.keep_lines;
        self.page_break_before = e.page_break_before;
        self.code_block = e.code_block;
        self.on_link = !e.link.is_empty();
        self.can_undo = e.can_undo;
        self.can_redo = e.can_redo;
        self.can_save = e.dirty;
        if e.dirty && e.revision != self.last_revision && self.live.is_some() {
            self.last_revision = e.revision;
            if let Some(bytes) = self.document_bytes() {
                if let Some(live) = &self.live {
                    live.edited(bytes, false);
                }
            }
        }
        let title = if e.dirty { Resources::title_modified().replace("{title}", &self.doc_title) } else { self.doc_title.clone() };
        self.window_title = format!("{title} — {}", Resources::app_title());
        self.has_hidden = !e.message.is_empty();
        self.hidden_status = e.message.clone();
    }

    // ── File ────────────────────────────────────────────────────────────────

    fn save_execute(&mut self) {
        // A local file is written back in place (envelope and sibling keys kept). A document opened
        // from the server saves through its session (vskubuno docs/DOCUMENTS-EDITING.md §D).
        if self.live.is_some() {
            if let Some(bytes) = self.document_bytes() {
                if let Some(live) = &self.live {
                    live.save_now(bytes);
                }
            }
            return;
        }
        let Some(path) = self.file.clone() else { return };
        let result: Result<(), String> = match self.page.with(|p| p.state_mut().editor.to_bytes()) {
            Some(Ok(bytes)) => std::fs::write(&path, bytes).map_err(|e| e.to_string()),
            Some(Err(e)) => Err(e.to_string()),
            None => Err("no document".to_string()),
        };
        match result {
            Ok(()) => {
                self.page.with(|p| {
                    p.state_mut().editor.mark_saved();
                    p.invalidate();
                });
            }
            Err(why) => {
                kubuno_desktop::tracing::error!("[documents] save {}: {why}", path.display());
                self.hidden_status = Resources::msg_save_failed().replace("{why}", &why);
                self.has_hidden = true;
            }
        }
    }

    fn bs_close_click(&mut self) {
        self.close();
    }

    // ── Clipboard and history ───────────────────────────────────────────────

    fn undo_execute(&mut self) {
        self.page.with(|p| p.undo());
    }

    fn redo_execute(&mut self) {
        self.page.with(|p| p.redo());
    }

    fn cut_execute(&mut self) {
        self.page.with(|p| p.cut());
    }

    fn copy_execute(&mut self) {
        self.page.with(|p| {
            p.copy();
        });
    }

    fn paste_execute(&mut self) {
        self.page.with(|p| p.paste(false));
    }

    fn paste_plain_execute(&mut self) {
        self.page.with(|p| p.paste(true));
    }

    fn select_all_execute(&mut self) {
        self.page.with(|p| p.select_all());
    }

    fn fmtpainter_execute(&mut self) {
        self.page.with(|p| p.toggle_format_painter());
    }

    fn clear_execute(&mut self) {
        self.page.with(|p| p.clear_formatting());
    }

    // ── Font ────────────────────────────────────────────────────────────────

    fn bold_execute(&mut self) {
        self.page.with(|p| p.toggle_mark("bold"));
    }

    fn italic_execute(&mut self) {
        self.page.with(|p| p.toggle_mark("italic"));
    }

    fn underline_execute(&mut self) {
        self.page.with(|p| p.toggle_mark("underline"));
    }

    fn strike_execute(&mut self) {
        self.page.with(|p| p.toggle_mark("strike"));
    }

    fn subscript_execute(&mut self) {
        self.page.with(|p| p.toggle_mark("subscript"));
    }

    fn superscript_execute(&mut self) {
        self.page.with(|p| p.toggle_mark("superscript"));
    }

    fn font_family_changed(&mut self, e: &TextChangedEventArgs) {
        let f = e.new.trim().to_string();
        if !f.is_empty() {
            self.page.with(|p| p.set_text_style("fontFamily", Some(f)));
        }
    }

    fn font_size_changed(&mut self, e: &TextChangedEventArgs) {
        let s = e.new.clone();
        self.page.with(|p| p.set_font_size(&s));
    }

    fn grow_execute(&mut self) {
        self.page.with(|p| p.grow_font(1.0));
    }

    fn shrink_execute(&mut self) {
        self.page.with(|p| p.grow_font(-1.0));
    }

    fn case_upper_execute(&mut self) {
        self.page.with(|p| p.change_case("upper"));
    }

    fn case_lower_execute(&mut self) {
        self.page.with(|p| p.change_case("lower"));
    }

    fn case_title_execute(&mut self) {
        self.page.with(|p| p.change_case("title"));
    }

    fn case_sentence_execute(&mut self) {
        self.page.with(|p| p.change_case("sentence"));
    }

    fn case_toggle_execute(&mut self) {
        self.page.with(|p| p.change_case("toggle"));
    }

    fn case_smallcaps_execute(&mut self) {
        self.page.with(|p| p.toggle_small_caps());
    }

    fn apply_color(&mut self, color: Option<&str>) {
        if let Some(c) = color {
            self.last_color = c.to_string();
        }
        let c = color.map(str::to_string);
        self.page.with(|p| p.set_text_style("color", c));
    }

    fn color_execute(&mut self) {
        let c = self.last_color.clone();
        self.apply_color(Some(&c));
    }

    fn color_auto_execute(&mut self) {
        self.apply_color(None);
    }

    fn color_red_execute(&mut self) {
        self.apply_color(Some("#ff0000"));
    }

    fn color_blue_execute(&mut self) {
        self.apply_color(Some("#0070c0"));
    }

    fn apply_highlight(&mut self, color: Option<&str>) {
        if let Some(c) = color {
            self.last_highlight = c.to_string();
        }
        let c = color.map(str::to_string);
        self.page.with(|p| p.set_highlight(c.as_deref()));
    }

    fn highlight_execute(&mut self) {
        let c = self.last_highlight.clone();
        self.apply_highlight(Some(&c));
    }

    fn hl_yellow_execute(&mut self) {
        self.apply_highlight(Some("#fff475"));
    }

    fn hl_green_execute(&mut self) {
        self.apply_highlight(Some("#ccff90"));
    }

    fn hl_cyan_execute(&mut self) {
        self.apply_highlight(Some("#a7ffeb"));
    }

    fn hl_none_execute(&mut self) {
        self.apply_highlight(None);
    }

    // ── Paragraph ───────────────────────────────────────────────────────────

    fn ul_execute(&mut self) {
        self.page.with(|p| p.toggle_list("bulletList"));
    }

    fn ol_execute(&mut self) {
        self.page.with(|p| p.toggle_list("orderedList"));
    }

    fn task_execute(&mut self) {
        self.page.with(|p| p.toggle_list("taskList"));
    }

    fn al_left_execute(&mut self) {
        self.page.with(|p| p.set_align("left"));
    }

    fn al_center_execute(&mut self) {
        self.page.with(|p| p.set_align("center"));
    }

    fn al_right_execute(&mut self) {
        self.page.with(|p| p.set_align("right"));
    }

    fn al_justify_execute(&mut self) {
        self.page.with(|p| p.set_align("justify"));
    }

    fn ind_dec_execute(&mut self) {
        self.page.with(|p| p.indent(-1));
    }

    fn ind_inc_execute(&mut self) {
        self.page.with(|p| p.indent(1));
    }

    fn line_height(&mut self, v: f64) {
        self.page.with(|p| p.set_paragraph_attr("lineHeight", json!(v)));
    }

    fn ls1_execute(&mut self) {
        self.line_height(1.0);
    }

    fn ls1_15_execute(&mut self) {
        self.line_height(1.15);
    }

    fn ls1_5_execute(&mut self) {
        self.line_height(1.5);
    }

    fn ls2_execute(&mut self) {
        self.line_height(2.0);
    }

    fn ls2_5_execute(&mut self) {
        self.line_height(2.5);
    }

    fn ls3_execute(&mut self) {
        self.line_height(3.0);
    }

    fn sp_before_execute(&mut self) {
        let v = if self.space_before { 0 } else { 12 };
        self.page.with(|p| p.set_paragraph_attr("spaceBefore", json!(v)));
    }

    fn sp_after_execute(&mut self) {
        let v = if self.space_after { 0 } else { 12 };
        self.page.with(|p| p.set_paragraph_attr("spaceAfter", json!(v)));
    }

    fn firstline_execute(&mut self) {
        let v = if self.first_line { 0 } else { 36 };
        self.page.with(|p| p.set_paragraph_attr("indentFirstLine", json!(v)));
    }

    fn hanging_execute(&mut self) {
        let on = self.hanging;
        let left = self.indent_left;
        self.page.with(|p| p.set_hanging(!on, left));
    }

    fn toggle_flag(&mut self, key: &str, on: bool) {
        let v = if on { Json::Null } else { Json::Bool(true) };
        let k = key.to_string();
        self.page.with(|p| p.set_paragraph_attr(&k, v));
    }

    fn flow_keepnext_execute(&mut self) {
        let on = self.keep_next;
        self.toggle_flag("keepNext", on);
    }

    fn flow_keeplines_execute(&mut self) {
        let on = self.keep_lines;
        self.toggle_flag("keepLines", on);
    }

    fn flow_pagebreak_execute(&mut self) {
        let on = self.page_break_before;
        self.toggle_flag("pageBreakBefore", on);
    }

    fn style_gallery_item_click(&mut self, e: &TextChangedEventArgs) {
        let id = e.new.clone();
        self.page.with(|p| p.apply_style(&id));
    }

    fn code_execute(&mut self) {
        let on = self.code_block;
        self.page.with(|p| p.toggle_code_block(on));
    }

    // ── Insert ──────────────────────────────────────────────────────────────

    fn pb_execute(&mut self) {
        self.page.with(|p| p.insert_page_break());
    }

    fn hr_execute(&mut self) {
        self.page.with(|p| p.insert_blocks(vec![Node::of_type("horizontalRule")]));
    }

    fn table_insert_execute(&mut self) {
        self.page.with(|p| p.insert_blocks(vec![table_node(3, 3)]));
    }

    fn img_execute(&mut self) {
        let Some(path) = crate::platform::file_dialog::open_image() else { return };
        match std::fs::read(&path) {
            Ok(bytes) => {
                self.page.with(|p| p.insert_image_bytes(&bytes));
            }
            Err(e) => kubuno_desktop::tracing::warn!("[documents] {}: {e}", path.display()),
        }
    }

    fn insert_symbol(&mut self, s: &str) {
        let t = s.to_string();
        self.page.with(|p| p.insert_text(&t));
    }

    fn sym_emdash_execute(&mut self) {
        self.insert_symbol("—");
    }
    fn sym_endash_execute(&mut self) {
        self.insert_symbol("–");
    }
    fn sym_ellipsis_execute(&mut self) {
        self.insert_symbol("…");
    }
    fn sym_nbsp_execute(&mut self) {
        self.insert_symbol("\u{00A0}");
    }
    fn sym_guillemets_execute(&mut self) {
        self.insert_symbol("«\u{00A0}\u{00A0}»");
    }
    fn sym_euro_execute(&mut self) {
        self.insert_symbol("€");
    }
    fn sym_tm_execute(&mut self) {
        self.insert_symbol("™");
    }
    fn sym_copyright_execute(&mut self) {
        self.insert_symbol("©");
    }
    fn sym_registered_execute(&mut self) {
        self.insert_symbol("®");
    }
    fn sym_section_execute(&mut self) {
        self.insert_symbol("§");
    }
    fn sym_bullet_execute(&mut self) {
        self.insert_symbol("•");
    }
    fn sym_degree_execute(&mut self) {
        self.insert_symbol("°");
    }
    fn sym_times_execute(&mut self) {
        self.insert_symbol("×");
    }
    fn sym_divide_execute(&mut self) {
        self.insert_symbol("÷");
    }
    fn sym_plusminus_execute(&mut self) {
        self.insert_symbol("±");
    }
    fn sym_arrow_execute(&mut self) {
        self.insert_symbol("→");
    }
    fn sym_notequal_execute(&mut self) {
        self.insert_symbol("≠");
    }
    fn sym_half_execute(&mut self) {
        self.insert_symbol("½");
    }
    fn sym_approx_execute(&mut self) {
        self.insert_symbol("≈");
    }
    fn sym_infinity_execute(&mut self) {
        self.insert_symbol("∞");
    }
    fn sym_sqrt_execute(&mut self) {
        self.insert_symbol("√");
    }
    fn sym_sum_execute(&mut self) {
        self.insert_symbol("∑");
    }
    fn sym_pi_execute(&mut self) {
        self.insert_symbol("π");
    }
    fn sym_delta_execute(&mut self) {
        self.insert_symbol("Δ");
    }

    fn link_execute(&mut self) {
        let current = self.page.with(|p| p.current_link()).unwrap_or_default();
        if let Some(url) = crate::views::link_dialog::LinkDialog::ask(self, &current) {
            self.page.with(|p| p.set_link((!url.is_empty()).then_some(url)));
        }
    }

    fn link_rm_execute(&mut self) {
        self.page.with(|p| p.set_link(None));
    }

    fn open_find(&mut self, replace: bool) {
        let selected = self.page.with(|p| p.selected_line()).unwrap_or_default();
        if !selected.is_empty() {
            self.find_text = selected;
        }
        self.show_find = true;
        self.show_replace = replace;
        self.find_status.clear();
    }

    fn find_execute(&mut self) {
        self.open_find(false);
    }

    fn replace_execute(&mut self) {
        self.open_find(true);
    }

    fn show_found(&mut self, (index, total): (usize, usize)) {
        self.find_status = if total == 0 { Resources::find_none().to_string() } else { Resources::find_count().replace("{index}", &index.to_string()).replace("{total}", &total.to_string()) };
    }

    fn find_next_click(&mut self) {
        let (q, c) = (self.find_text.clone(), self.find_match_case);
        if q.is_empty() {
            return;
        }
        let r = self.page.with(|p| p.find(&q, c, false)).unwrap_or((0, 0));
        self.show_found(r);
    }

    fn find_prev_click(&mut self) {
        let (q, c) = (self.find_text.clone(), self.find_match_case);
        if q.is_empty() {
            return;
        }
        let r = self.page.with(|p| p.find(&q, c, true)).unwrap_or((0, 0));
        self.show_found(r);
    }

    fn find_text_changed(&mut self) {
        self.find_status.clear();
    }

    fn find_key_down(&mut self, e: &mut KeyEventArgs) {
        use kubuno_desktop::controls::host::vk;
        if e.key.0 == vk::ENTER {
            if e.mods.shift { self.find_prev_click() } else { self.find_next_click() }
            e.handled = true;
        } else if e.key.0 == vk::ESCAPE {
            self.find_close_click();
            e.handled = true;
        }
    }

    fn find_case_changed(&mut self, e: &CheckedChangedEventArgs) {
        self.find_match_case = e.new;
        self.find_status.clear();
    }

    fn find_replace_click(&mut self) {
        let (q, w, c) = (self.find_text.clone(), self.replace_text.clone(), self.find_match_case);
        if q.is_empty() {
            return;
        }
        let r = self.page.with(|p| p.replace(&q, &w, c)).unwrap_or((0, 0));
        self.show_found(r);
    }

    fn find_replace_all_click(&mut self) {
        let (q, w, c) = (self.find_text.clone(), self.replace_text.clone(), self.find_match_case);
        if q.is_empty() {
            return;
        }
        let n = self.page.with(|p| p.replace_all(&q, &w, c)).unwrap_or(0);
        self.find_status = Resources::find_replaced().replace("{count}", &n.to_string());
    }

    fn find_close_click(&mut self) {
        self.show_find = false;
        self.page.with(|p| {
            p.focus();
        });
    }

    fn wordcount_execute(&mut self) {
        self.page.with(|p| p.show_word_count());
    }

    // ── Layout ──────────────────────────────────────────────────────────────

    fn margins(&mut self, top: f32, right: f32, bottom: f32, left: f32) {
        self.page.with(|p| p.set_page_margins(left, right, top, bottom, true));
    }

    fn mg_normal_execute(&mut self) {
        self.margins(96.0, 96.0, 96.0, 96.0);
    }

    fn mg_narrow_execute(&mut self) {
        self.margins(48.0, 48.0, 48.0, 48.0);
    }

    fn mg_moderate_execute(&mut self) {
        self.margins(96.0, 72.0, 96.0, 72.0);
    }

    fn mg_wide_execute(&mut self) {
        self.margins(96.0, 192.0, 96.0, 192.0);
    }

    // ── View ────────────────────────────────────────────────────────────────

    fn ruler_execute(&mut self) {
        self.ruler_inset = if self.show_ruler { crate::controls::ruler::RULER_SZ } else { 0.0 };
    }

    fn marks_execute(&mut self) {
        let on = self.show_marks;
        self.page.with(|p| p.set_show_marks(on));
    }

    fn zoom_100_execute(&mut self) {
        self.page.with(|p| p.set_zoom(1.0));
    }

    fn zoom_one_page_execute(&mut self) {
        self.page.with(|p| p.zoom_one_page());
    }

    fn zoom_page_width_execute(&mut self) {
        self.page.with(|p| p.zoom_page_width());
    }

    // ── The status bar ──────────────────────────────────────────────────────

    fn zoom_slider_value_changed(&mut self, e: &NumericValueChangedEventArgs) {
        let zoom = e.new / 100.0;
        self.page.with(|p| p.set_zoom(zoom));
    }

    // ── The rulers ──────────────────────────────────────────────────────────

    fn horizontal_ruler_margins_changed(&mut self, e: &RulerMarginsEventArgs) {
        let (top, bottom) = (self.margin_top, self.margin_bottom);
        let (s, end, c) = (e.start, e.end, e.commit);
        self.page.with(|p| p.set_page_margins(s, end, top, bottom, c));
    }

    fn vertical_ruler_margins_changed(&mut self, e: &RulerMarginsEventArgs) {
        let (left, right) = (self.margin_left, self.margin_right);
        let (s, end, c) = (e.start, e.end, e.commit);
        self.page.with(|p| p.set_page_margins(left, right, s, end, c));
    }

    fn horizontal_ruler_indents_changed(&mut self, e: &RulerIndentsEventArgs) {
        let (l, f, r, c) = (e.left, e.first_line, e.right, e.commit);
        self.page.with(|p| p.set_indents(l, f, r, c));
    }

    fn horizontal_ruler_tab_stops_changed(&mut self, e: &RulerTabStopsEventArgs) {
        self.tab_stops = e.tab_stops.clone();
        let t = e.tab_stops.clone();
        self.page.with(|p| p.set_tab_stops(&t));
    }

    fn horizontal_ruler_drag_guide_changed(&mut self, e: &RulerGuideEventArgs) {
        self.show_guide(e);
    }

    fn vertical_ruler_drag_guide_changed(&mut self, e: &RulerGuideEventArgs) {
        self.show_guide(e);
    }

    fn show_guide(&mut self, e: &RulerGuideEventArgs) {
        let guide = e.visible.then(|| DragGuide { vertical: e.vertical, position: e.position, label: e.label.clone(), pointer: e.pointer });
        self.page.with(|p| p.set_drag_guide(guide));
    }

    fn ruler_corner_tab_type_changed(&mut self, e: &TextChangedEventArgs) {
        self.tab_type = e.new.clone();
    }
}

/// `makeTableNode(rows, cols)` (`DocumentEditorPage.tsx:1608`): empty cells holding one paragraph.
pub fn table_node(rows: usize, cols: usize) -> Node {
    let cell = || Node::element("tableCell", None, vec![Node::of_type("paragraph")]);
    let row = || Node::element("tableRow", None, (0..cols).map(|_| cell()).collect());
    Node::element("table", None, (0..rows).map(|_| row()).collect())
}

/// The installed font families, for the ribbon's font list (`Text` rows).
fn font_rows() -> Rows {
    Rows::from(kubuno_desktop::ui::editors::system_font_families().iter().map(|f| Row::new().with("Text", Value::Str(f.clone()))).collect::<Vec<_>>())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_command_line_names_a_file_and_the_switches() {
        let o = Options::parse(["--no-splash", "--dark", "--culture", "en", r"C:\docs\rapport.json"].map(String::from));
        assert!(o.dark);
        assert_eq!(o.culture.as_deref(), Some("en"));
        assert_eq!(o.file, Some(PathBuf::from(r"C:\docs\rapport.json")));
        assert_eq!(Options::parse([]).file, None);
        let o = Options::parse(["--doc", "3f2a", "--zoom", "50%"].map(String::from));
        assert_eq!((o.doc.as_deref(), o.zoom, o.file), (Some("3f2a"), Some(50.0), None));
    }

    #[test]
    fn a_missing_file_opens_the_sample() {
        let o = Options::parse(["Z:/nowhere/missing.json".to_string()]);
        assert_eq!(o.open_document().title, App::default().title);
    }

    #[test]
    fn a_table_has_the_shape_the_web_inserts() {
        let t = table_node(2, 3);
        assert_eq!(t.children().len(), 2);
        assert_eq!(t.children()[0].children().len(), 3);
        assert_eq!(t.children()[0].children()[0].children()[0].node_type(), Some("paragraph"));
    }
}
