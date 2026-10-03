//! `<PrintDocument>` (WinForms `PrintDocument`): the non-visual component a form prints with.
//!
//! ```xml
//! <PrintDocument x:Name="print_document1" DocumentName="Invoice" OnPrintPage="print_document1_print_page"/>
//! ```
//!
//! [`PrintDocument::print`] raises `BeginPrint`, then for each page `QueryPageSettings` and
//! `PrintPage` — whose handler draws the page on `e.graphics()` and sets `e.has_more_pages` — then
//! `EndPrint`, and sends the pages to the Windows spooler (Direct2D printing over an XPS job:
//! vector output). The same loop renders a preview ([`PrintDocument::render_preview`], what
//! `PrintPreviewControl` and `PrintPreviewDialog` show).
//!
//! Events go to the Rust subscribers of the fields (`doc.print_page.subscribe(…)`, or
//! [`PrintDocument::on_print_page`]) and to the element's `.kbview` handlers. A `.kbview` handler is
//! a method of the view: while one of the view's handlers runs, the view is borrowed, so a document
//! of the view printed from a handler (`self.print_document1.print()`) prints **when that handler
//! returns**, before the next frame (WinForms prints at once; the difference is invisible to the
//! user, but the handler cannot read the outcome — see [`PrintDocument::last_error`]).

use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use kubuno_desktop_ui::graphics::{Graphics, RectExt};
use kubuno_desktop_ui::Rect;
use kubuno_desktop_views::binding::{BindingFormat, Value};
use kubuno_desktop_views::events::{ElementRef, Event, EventArgs, Subscription};
use kubuno_desktop_views::format::ValueKind;
use kubuno_desktop_views::prelude::*;
use kubuno_desktop_views::scope::{BindingProvider, ComponentScope};

use crate::args::{PrintEventArgs, PrintPageEventArgs, QueryPageSettingsEventArgs};
use crate::engine::Surface;
use crate::native::{self, DevMode, PageMetrics};
use crate::settings::{hundredths_to_dip, Margins, PageSettings, PaperSize, PrintAction, PrintRange, PrinterSettings};
use crate::xps::SpoolJob;
use crate::{PreviewDocument, PreviewPage, PrintError};

/// How long [`PrintDocument::print`] waits for the spooler to finish writing a file (print to file).
const FILE_TIMEOUT: Duration = Duration::from_secs(120);

/// The XML properties as last applied to the settings objects (see [`PrintDocument::sync_properties`]).
#[derive(Debug, Clone, PartialEq)]
struct Applied {
    printer_name: String,
    landscape: bool,
    paper_size: String,
    margins: String,
    copies: u32,
    print_to_file: bool,
    print_file_name: String,
}

impl Default for Applied {
    fn default() -> Self {
        Self {
            printer_name: String::new(),
            landscape: false,
            paper_size: String::new(),
            margins: Margins::default().to_string(),
            copies: 1,
            print_to_file: false,
            print_file_name: String::new(),
        }
    }
}

/// What a document of a view was asked to do while its view was busy (see the module doc).
#[derive(Debug, Clone, PartialEq)]
enum Pending {
    Print,
    /// A preview window: its title and size.
    Preview(String, (f32, f32)),
}

/// `<PrintDocument>` (see the module doc).
#[derive(Component)]
#[kubuno(extends = Component, overrides(Component))]
#[toolbox(icon = "printer", category = "Printing")]
#[default_event("PrintPage")]
#[default_property("DocumentName")]
pub struct PrintDocument {
    base: ComponentCore,
    /// The name of the document, shown in the printer's queue (and the default name of a file it is printed into).
    #[property]
    #[category("Behavior")]
    #[default_value("document")]
    pub document_name: String,
    /// Whether the page's Graphics starts at the top-left corner of the margins rather than of the paper.
    #[property]
    #[category("Behavior")]
    #[default_value(false)]
    pub origin_at_margins: bool,
    /// The printer to print on; empty for the default printer.
    #[property]
    #[category("Printing")]
    pub printer_name: String,
    /// Whether the pages are printed in landscape orientation.
    #[property]
    #[category("Printing")]
    #[default_value(false)]
    pub landscape: bool,
    /// The paper: Letter, Legal, A4, A3, A5, or a paper name of the printer; empty for the printer's default.
    #[property]
    #[category("Printing")]
    pub paper_size: String,
    /// The margins in hundredths of an inch: left, right, top, bottom.
    #[property]
    #[category("Printing")]
    #[default_value("100, 100, 100, 100")]
    pub margins: String,
    /// How many copies are printed.
    #[property]
    #[category("Printing")]
    #[default_value(1)]
    pub copies: u32,
    /// Whether the document is printed into a file (PrintFileName) rather than on paper.
    #[property]
    #[category("Printing")]
    #[default_value(false)]
    pub print_to_file: bool,
    /// The file the document is printed into when PrintToFile is set (a PDF with Microsoft Print to PDF).
    #[property]
    #[category("Printing")]
    pub print_file_name: String,
    /// Occurs when the printing starts, before the first page (cancelable).
    #[event]
    #[category("Action")]
    pub begin_print: Event<PrintEventArgs>,
    /// Occurs before each page is printed: its settings can be changed for that page only.
    #[event]
    #[category("Action")]
    pub query_page_settings: Event<QueryPageSettingsEventArgs>,
    /// Occurs for each page to print: draw it on e.graphics() and set e.has_more_pages.
    #[event]
    #[category("Action")]
    pub print_page: Event<PrintPageEventArgs>,
    /// Occurs when the last page was printed (or the printing was cancelled).
    #[event]
    #[category("Action")]
    pub end_print: Event<PrintEventArgs>,
    printer_settings: PrinterSettings,
    default_page_settings: PageSettings,
    applied: Applied,
    pending: Option<Pending>,
    last_error: Option<String>,
    /// Bumped by every property change that changes the pages (previews follow it).
    revision: u64,
    subscriptions: Vec<Subscription>,
}

impl Default for PrintDocument {
    fn default() -> Self {
        Self {
            base: ComponentCore::default(),
            document_name: "document".to_string(),
            origin_at_margins: false,
            printer_name: String::new(),
            landscape: false,
            paper_size: String::new(),
            margins: Margins::default().to_string(),
            copies: 1,
            print_to_file: false,
            print_file_name: String::new(),
            begin_print: Event::default(),
            query_page_settings: Event::default(),
            print_page: Event::default(),
            end_print: Event::default(),
            printer_settings: PrinterSettings::default(),
            default_page_settings: PageSettings::default(),
            applied: Applied::default(),
            pending: None,
            last_error: None,
            revision: 0,
            subscriptions: Vec::new(),
        }
    }
}

impl std::fmt::Debug for PrintDocument {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PrintDocument").field("document_name", &self.document_name).field("printer_settings", &self.printer_settings).finish()
    }
}

impl PrintDocument {
    pub fn new() -> Self {
        Self::default()
    }

    /// A document named `name` (`DocumentName`).
    pub fn named(name: impl Into<String>) -> Self {
        Self { document_name: name.into(), ..Self::default() }
    }

    // ── Settings ─────────────────────────────────────────────────────────────────────────────────

    /// The printer and the job's options (`PrinterSettings`).
    pub fn printer_settings(&mut self) -> &PrinterSettings {
        self.sync_properties();
        &self.printer_settings
    }

    /// The printer and the job's options, to change them (`PrinterSettings`).
    pub fn printer_settings_mut(&mut self) -> &mut PrinterSettings {
        self.sync_properties();
        self.revision += 1;
        &mut self.printer_settings
    }

    /// The settings every page starts from (`DefaultPageSettings`).
    pub fn default_page_settings(&mut self) -> &PageSettings {
        self.sync_properties();
        &self.default_page_settings
    }

    /// The settings every page starts from, to change them (`DefaultPageSettings`).
    pub fn default_page_settings_mut(&mut self) -> &mut PageSettings {
        self.sync_properties();
        self.revision += 1;
        &mut self.default_page_settings
    }

    /// Replaces the printer settings (what a Print dialog returned).
    pub fn set_printer_settings(&mut self, settings: PrinterSettings) {
        self.sync_properties();
        self.printer_settings = settings;
        self.revision += 1;
    }

    /// Replaces the page settings (what a Page Setup dialog returned).
    pub fn set_default_page_settings(&mut self, settings: PageSettings) {
        self.sync_properties();
        self.default_page_settings = settings;
        self.revision += 1;
    }

    /// A number that changes whenever what the document prints may have changed (a preview
    /// compares it to know when to render again).
    pub fn revision(&self) -> u64 {
        let props = self.props();
        self.revision + if props == self.applied { 0 } else { 1 }
    }

    /// Asks the previews of this document to render again (the data it prints changed).
    pub fn invalidate(&mut self) {
        self.revision += 1;
    }

    /// Why the last print failed (`None` after a success): what a handler reads after a print
    /// deferred to the end of the handler.
    pub fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }

    fn props(&self) -> Applied {
        Applied {
            printer_name: self.printer_name.clone(),
            landscape: self.landscape,
            paper_size: self.paper_size.clone(),
            margins: self.margins.clone(),
            copies: self.copies,
            print_to_file: self.print_to_file,
            print_file_name: self.print_file_name.clone(),
        }
    }

    /// Applies the XML properties that changed since last time to the settings objects (code that
    /// changed the settings directly keeps its values as long as the XML does not change).
    fn sync_properties(&mut self) {
        let now = self.props();
        if now == self.applied {
            return;
        }
        let was = std::mem::replace(&mut self.applied, now.clone());
        if now.printer_name != was.printer_name {
            self.printer_settings.printer_name = now.printer_name.trim().to_string();
            self.printer_settings.devmode = None;
        }
        if now.landscape != was.landscape {
            self.default_page_settings.landscape = now.landscape;
        }
        if now.paper_size != was.paper_size {
            let name = now.paper_size.trim();
            self.default_page_settings.paper_size = if name.is_empty() {
                None
            } else {
                PaperSize::by_name(name).or_else(|| self.printer_settings.paper_sizes().into_iter().find(|p| p.paper_name.eq_ignore_ascii_case(name))).or_else(|| {
                    tracing::warn!(target: "kubuno_desktop_print", paper = name, "unknown paper size: the printer's default is used");
                    None
                })
            };
        }
        if now.margins != was.margins {
            match Margins::parse(&now.margins) {
                Some(m) => self.default_page_settings.margins = m,
                None => tracing::warn!(target: "kubuno_desktop_print", margins = %now.margins, "Margins is \"left, right, top, bottom\" in hundredths of an inch"),
            }
        }
        if now.copies != was.copies {
            self.printer_settings.copies = now.copies.clamp(1, u16::MAX as u32) as u16;
        }
        if now.print_to_file != was.print_to_file {
            self.printer_settings.print_to_file = now.print_to_file;
        }
        if now.print_file_name != was.print_file_name {
            self.printer_settings.print_file_name = now.print_file_name.clone();
        }
        self.revision += 1;
    }

    // ── Events ───────────────────────────────────────────────────────────────────────────────────

    /// Subscribes `f` to `PrintPage` for the document's lifetime (`PrintPage += …`).
    pub fn on_print_page(&mut self, f: impl FnMut(&ElementRef<'_>, &mut PrintPageEventArgs) + 'static) -> &mut Self {
        let s = self.print_page.subscribe(f);
        self.subscriptions.push(s);
        self.revision += 1;
        self
    }

    /// Subscribes `f` to `BeginPrint` for the document's lifetime.
    pub fn on_begin_print(&mut self, f: impl FnMut(&ElementRef<'_>, &mut PrintEventArgs) + 'static) -> &mut Self {
        let s = self.begin_print.subscribe(f);
        self.subscriptions.push(s);
        self
    }

    /// Subscribes `f` to `QueryPageSettings` for the document's lifetime.
    pub fn on_query_page_settings(&mut self, f: impl FnMut(&ElementRef<'_>, &mut QueryPageSettingsEventArgs) + 'static) -> &mut Self {
        let s = self.query_page_settings.subscribe(f);
        self.subscriptions.push(s);
        self.revision += 1;
        self
    }

    /// Subscribes `f` to `EndPrint` for the document's lifetime.
    pub fn on_end_print(&mut self, f: impl FnMut(&ElementRef<'_>, &mut PrintEventArgs) + 'static) -> &mut Self {
        let s = self.end_print.subscribe(f);
        self.subscriptions.push(s);
        self
    }

    /// Raises an event: the Rust subscribers, then the element's `.kbview` handler (when the view's
    /// handlers can run now).
    fn raise<A: EventArgs>(&self, attr: &'static str, event: &Event<A>, args: &mut A) {
        let name = self.site().map(|s| s.name.clone());
        let sender = ElementRef { name: name.as_deref(), element: "PrintDocument", id: "", bounds: Rect::default(), focus_id: None, attributes: &[] };
        event.raise(&sender, args);
        if let Some(name) = name.filter(|n| !n.is_empty()) {
            kubuno_desktop_views::scope::raise_now(&name, attr, args);
        }
    }

    /// Whether this is a component of a running view whose `.kbview` handlers cannot run now (its
    /// view model is borrowed by a handler): the work is then deferred to the end of the handler.
    fn view_is_busy(&self) -> bool {
        view_is_busy(self.site())
    }

    // ── Printing ─────────────────────────────────────────────────────────────────────────────────

    /// Prints the document (`Print()`): see the module doc. Returns how many pages went to the
    /// printer — 0 when the print was deferred to the end of the running handler.
    pub fn print(&mut self) -> Result<usize, PrintError> {
        if self.view_is_busy() {
            self.pending = Some(Pending::Print);
            kubuno_desktop_controls::host::request_repaint_after(1);
            return Ok(0);
        }
        self.print_now()
    }

    /// Prints at once, whatever the view is doing (its `.kbview` handlers are not run while it is
    /// busy: only the Rust subscribers draw).
    pub fn print_now(&mut self) -> Result<usize, PrintError> {
        let result = self.print_job();
        match &result {
            Ok(_) => self.last_error = None,
            Err(e) => {
                tracing::error!(target: "kubuno_desktop_print", document = %self.document_name, "printing failed: {e}");
                self.last_error = Some(e.to_string());
            }
        }
        result
    }

    fn print_job(&mut self) -> Result<usize, PrintError> {
        self.sync_properties();
        let mut printer = self.printer_settings.clone();
        let name = printer.resolved_printer_name()?;
        let output = if printer.print_to_file {
            let file = printer.print_file_name.trim().to_string();
            if file.is_empty() {
                match crate::dialogs::ask_output_file(&self.document_name, &name) {
                    Some(path) => Some(path),
                    None => return Ok(0),
                }
            } else {
                Some(PathBuf::from(file))
            }
        } else {
            None
        };
        printer.printer_name = name.clone();
        let surface = Surface::new()?;
        let action = if output.is_some() { PrintAction::PrintToFile } else { PrintAction::PrintToPrinter };
        let mut sink = SpoolSink { surface: &surface, printer: name, job_name: self.document_name.clone(), output, job: None, range: (printer.print_range, printer.from_page, printer.to_page), sent: 0 };
        self.run(&mut sink, action, &printer)
    }

    /// Shows the document in a print preview window titled `title` (`size` in DIP), modal to the
    /// application's window — what `PrintPreviewDialog.ShowDialog()` does. Deferred to the end of the
    /// running handler like [`Self::print`] (`DialogResult::None` then).
    pub fn show_preview(&mut self, title: &str, size: (f32, f32)) -> kubuno_desktop_controls::buttons::DialogResult {
        if self.view_is_busy() {
            self.pending = Some(Pending::Preview(title.to_string(), size));
            kubuno_desktop_controls::host::request_repaint_after(1);
            return kubuno_desktop_controls::buttons::DialogResult::None;
        }
        let preview = self.render_preview();
        crate::preview_dialog::run_preview_window(title, size, preview, &mut || self.print_now())
    }

    /// Renders every page for a preview (`PreviewPrintController`): the same events as a print.
    pub fn render_preview(&mut self) -> Result<PreviewDocument, PrintError> {
        let surface = Rc::new(Surface::new()?);
        self.render_preview_on(surface)
    }

    pub(crate) fn render_preview_on(&mut self, surface: Rc<Surface>) -> Result<PreviewDocument, PrintError> {
        self.sync_properties();
        let mut printer = self.printer_settings.clone();
        // A preview needs no printer: without one, the pages keep their nominal size.
        if let Ok(name) = printer.resolved_printer_name() {
            printer.printer_name = name;
        }
        let revision = self.revision();
        let mut sink = PreviewSink { surface: &surface, pages: Vec::new() };
        self.run(&mut sink, PrintAction::PrintToPreview, &printer)?;
        let pages = sink.pages;
        Ok(PreviewDocument { surface, pages, revision })
    }

    /// Runs the events over pages drawn on recording `Graphics` (tests, measuring): what each page
    /// drew, and the pages' settings.
    pub fn render_recorded(&mut self) -> Result<Vec<(PageSettings, kubuno_desktop_ui::graphics::DisplayList)>, PrintError> {
        self.sync_properties();
        let printer = self.printer_settings.clone();
        let mut sink = RecordSink { pages: Vec::new() };
        self.run(&mut sink, PrintAction::PrintToPreview, &printer)?;
        Ok(sink.pages)
    }

    /// The print loop (WinForms `PrintController.Print`): BeginPrint, then QueryPageSettings and
    /// PrintPage per page while `has_more_pages`, then EndPrint.
    fn run(&mut self, sink: &mut dyn PageSink, action: PrintAction, printer: &PrinterSettings) -> Result<usize, PrintError> {
        let mut begin = PrintEventArgs { cancel: false, print_action: action };
        self.raise("OnBeginPrint", &self.begin_print, &mut begin);
        if begin.cancel {
            return Ok(0);
        }
        let mut resolver = Resolver::new(printer, sink.needs_printer());
        let result = (|| {
            let job_page = resolver.resolve(&self.default_page_settings, printer);
            sink.begin(&job_page)?;
            let mut page_number = 1u32;
            let mut cancelled = false;
            loop {
                let mut query = QueryPageSettingsEventArgs { cancel: false, print_action: action, page_settings: self.default_page_settings.clone(), page_number };
                self.raise("OnQueryPageSettings", &self.query_page_settings, &mut query);
                if query.cancel {
                    cancelled = true;
                    break;
                }
                let page = resolver.resolve(&query.page_settings, printer);
                let differs = page.devmode.is_some() && page.devmode != job_page.devmode;
                let (w, h) = page.size;
                let margins = page.settings.margins;
                let margin_bounds = Rect::new(
                    hundredths_to_dip(margins.left as f32),
                    hundredths_to_dip(margins.top as f32),
                    (w - hundredths_to_dip(margins.right as f32)).max(hundredths_to_dip(margins.left as f32)),
                    (h - hundredths_to_dip(margins.bottom as f32)).max(hundredths_to_dip(margins.top as f32)),
                );
                let origin_at_margins = self.origin_at_margins;
                let mut outcome = (false, false);
                {
                    let this = &*self;
                    let settings = page.settings.clone();
                    let mut draw = |g: &Graphics<'_>| {
                        if origin_at_margins {
                            g.translate_transform(margin_bounds.left, margin_bounds.top);
                        }
                        outcome = PrintPageEventArgs::lend(g, Rect::from_xywh(0.0, 0.0, w, h), margin_bounds, settings.clone(), page_number, |e| {
                            this.raise("OnPrintPage", &this.print_page, e);
                            (e.has_more_pages, e.cancel)
                        });
                    };
                    sink.page(page_number, &page, differs, &mut draw)?;
                }
                let (more, cancel) = outcome;
                if cancel {
                    cancelled = true;
                    break;
                }
                if !more {
                    break;
                }
                page_number += 1;
                if page_number > 100_000 {
                    return Err(PrintError::Driver("more than 100000 pages: PrintPage never cleared has_more_pages".into()));
                }
            }
            sink.end(cancelled)
        })();
        let mut end = PrintEventArgs { cancel: false, print_action: action };
        self.raise("OnEndPrint", &self.end_print, &mut end);
        result
    }
}

impl Component for PrintDocument {
    fn as_binding_provider(&self) -> Option<&dyn BindingProvider> {
        Some(self)
    }
    fn as_binding_provider_mut(&mut self) -> Option<&mut dyn BindingProvider> {
        Some(self)
    }
}

/// Answers no path: the document is a provider only to be called once per frame
/// ([`BindingProvider::binding_sync`]) with the view's handlers at hand — when a print deferred
/// by a busy view runs.
impl BindingProvider for PrintDocument {
    fn binding_get(&self, _path: &str, _want: ValueKind, _format: &BindingFormat, _scope: &ComponentScope) -> Option<Value> {
        None
    }

    fn binding_set(&mut self, _path: &str, _value: Value, _format: &BindingFormat, _scope: &ComponentScope) -> bool {
        false
    }

    fn binding_sync(&mut self, _scope: &ComponentScope) -> bool {
        if self.pending.is_none() || !kubuno_desktop_views::scope::can_raise_now() {
            return false;
        }
        match self.pending.take() {
            Some(Pending::Print) => {
                let _ = self.print_now();
            }
            Some(Pending::Preview(title, size)) => {
                self.show_preview(&title, size);
            }
            None => {}
        }
        true
    }
}

// ── Resolving a page against the printer ─────────────────────────────────────────────────────────

/// A page's settings resolved against the printer.
pub(crate) struct ResolvedPage {
    pub settings: PageSettings,
    /// The physical page, DIP.
    pub size: (f32, f32),
    /// Device pixels per DIP.
    pub device_scale: f32,
    /// The driver's settings for this page (none without a printer).
    pub devmode: Option<DevMode>,
}

struct Resolver {
    printer: Option<String>,
    base: Option<DevMode>,
    sizes: Vec<PaperSize>,
    cache: Vec<(PageSettings, ResolvedPage)>,
}

impl Resolver {
    fn new(printer: &PrinterSettings, needs_printer: bool) -> Self {
        let _ = needs_printer;
        let name = printer.resolved_printer_name().ok();
        let base = name.as_deref().and_then(|n| DevMode::for_printer(n, printer.devmode.as_ref()).map_err(|e| tracing::warn!(target: "kubuno_desktop_print", "{e}")).ok());
        let sizes = name.as_deref().map(native::paper_sizes).unwrap_or_default();
        Self { printer: name, base, sizes, cache: Vec::new() }
    }

    fn resolve(&mut self, page: &PageSettings, printer: &PrinterSettings) -> ResolvedPage {
        if let Some((_, r)) = self.cache.iter().find(|(p, _)| p == page) {
            return ResolvedPage { settings: r.settings.clone(), size: r.size, device_scale: r.device_scale, devmode: r.devmode.clone() };
        }
        let mut settings = page.clone();
        let mut devmode = None;
        let mut metrics = None;
        if let (Some(name), Some(base)) = (&self.printer, &self.base) {
            match base.with_settings(name, page, printer) {
                Ok(dm) => {
                    if settings.paper_size.is_none() {
                        let mut read = PageSettings::default();
                        dm.read_page(&mut read, &self.sizes);
                        settings.paper_size = read.paper_size;
                    }
                    metrics = native::page_metrics(name, &dm).map_err(|e| tracing::warn!(target: "kubuno_desktop_print", "{e}")).ok();
                    devmode = Some(dm);
                }
                Err(e) => tracing::warn!(target: "kubuno_desktop_print", "{e}"),
            }
        }
        let m = metrics.unwrap_or_else(|| {
            let (w, h) = settings.size();
            PageMetrics::nominal(w, h)
        });
        settings.hard_margin_x = m.offset_x / m.dpi_x * 100.0;
        settings.hard_margin_y = m.offset_y / m.dpi_y * 100.0;
        settings.printable_area = Rect::from_xywh(settings.hard_margin_x, settings.hard_margin_y, m.printable_width / m.dpi_x * 100.0, m.printable_height / m.dpi_y * 100.0);
        let (wh, hh) = m.page_hundredths();
        let resolved = ResolvedPage { settings, size: (hundredths_to_dip(wh), hundredths_to_dip(hh)), device_scale: m.dpi_x / 96.0, devmode };
        self.cache.push((page.clone(), ResolvedPage { settings: resolved.settings.clone(), size: resolved.size, device_scale: resolved.device_scale, devmode: resolved.devmode.clone() }));
        resolved
    }
}

// ── Where the pages go ───────────────────────────────────────────────────────────────────────────

/// A print controller's side of the loop (WinForms `PrintController`): the spooler, a preview, a
/// recording.
pub(crate) trait PageSink {
    /// Whether the pages need a printer (a preview does not).
    fn needs_printer(&self) -> bool {
        false
    }
    fn begin(&mut self, job: &ResolvedPage) -> Result<(), PrintError>;
    /// One page: `draw` paints it on the page's `Graphics`. `differs`: its settings differ from the
    /// job's (it needs a ticket of its own).
    fn page(&mut self, number: u32, page: &ResolvedPage, differs: bool, draw: &mut dyn FnMut(&Graphics<'_>)) -> Result<(), PrintError>;
    /// The job ends: returns the pages delivered.
    fn end(&mut self, cancelled: bool) -> Result<usize, PrintError>;
}

struct SpoolSink<'a> {
    surface: &'a Surface,
    printer: String,
    job_name: String,
    output: Option<PathBuf>,
    job: Option<SpoolJob>,
    range: (PrintRange, u32, u32),
    sent: usize,
}

impl PageSink for SpoolSink<'_> {
    fn needs_printer(&self) -> bool {
        true
    }

    fn begin(&mut self, job: &ResolvedPage) -> Result<(), PrintError> {
        let devmode = match &job.devmode {
            Some(d) => d.clone(),
            None => DevMode::for_printer(&self.printer, None)?,
        };
        let device = self.surface.device()?;
        let dpi = (job.device_scale * 96.0).clamp(150.0, 600.0);
        self.job = Some(SpoolJob::start(&device, &self.printer, &self.job_name, &devmode, self.output.as_deref(), dpi)?);
        Ok(())
    }

    fn page(&mut self, number: u32, page: &ResolvedPage, differs: bool, draw: &mut dyn FnMut(&Graphics<'_>)) -> Result<(), PrintError> {
        let list = self.surface.record(page.size, page.device_scale, draw)?;
        let (range, from, to) = self.range;
        if range == PrintRange::SomePages && (number < from.max(1) || (to > 0 && number > to)) {
            return Ok(());
        }
        let ticket = match (&page.devmode, differs) {
            (Some(dm), true) => Some(crate::xps::ticket_stream(&self.printer, dm, true)?),
            _ => None,
        };
        let Some(job) = self.job.as_mut() else { return Err(PrintError::Spooler("the job was not started".into())) };
        job.add_page(&list, page.size, ticket.as_ref())?;
        self.sent += 1;
        Ok(())
    }

    fn end(&mut self, cancelled: bool) -> Result<usize, PrintError> {
        let Some(job) = self.job.take() else { return Ok(0) };
        if cancelled || self.sent == 0 {
            job.cancel();
            return Ok(0);
        }
        job.finish(FILE_TIMEOUT)
    }
}

impl Drop for SpoolSink<'_> {
    fn drop(&mut self) {
        // A job left open by an error is cancelled, never half-printed.
        if let Some(job) = self.job.take() {
            job.cancel();
        }
    }
}

struct PreviewSink<'a> {
    surface: &'a Surface,
    pages: Vec<PreviewPage>,
}

impl PageSink for PreviewSink<'_> {
    fn begin(&mut self, _job: &ResolvedPage) -> Result<(), PrintError> {
        Ok(())
    }

    fn page(&mut self, _number: u32, page: &ResolvedPage, _differs: bool, draw: &mut dyn FnMut(&Graphics<'_>)) -> Result<(), PrintError> {
        let list = self.surface.record(page.size, page.device_scale.min(4.0), draw)?;
        self.pages.push(PreviewPage { list, size: page.size, settings: page.settings.clone() });
        Ok(())
    }

    fn end(&mut self, _cancelled: bool) -> Result<usize, PrintError> {
        Ok(self.pages.len())
    }
}

struct RecordSink {
    pages: Vec<(PageSettings, kubuno_desktop_ui::graphics::DisplayList)>,
}

impl PageSink for RecordSink {
    fn begin(&mut self, _job: &ResolvedPage) -> Result<(), PrintError> {
        Ok(())
    }

    fn page(&mut self, _number: u32, page: &ResolvedPage, _differs: bool, draw: &mut dyn FnMut(&Graphics<'_>)) -> Result<(), PrintError> {
        let g = Graphics::recorder();
        draw(&g);
        self.pages.push((page.settings.clone(), g.take_recording().unwrap_or_default()));
        Ok(())
    }

    fn end(&mut self, _cancelled: bool) -> Result<usize, PrintError> {
        Ok(self.pages.len())
    }
}

/// Whether a component sited at `site` belongs to a running view whose `.kbview` handlers cannot run
/// now (its view model is borrowed by a handler): its work is then deferred to the end of the
/// handler ([`BindingProvider::binding_sync`]).
pub(crate) fn view_is_busy(site: Option<&Site>) -> bool {
    let Some(site) = site.filter(|s| !s.design_mode && !s.name.is_empty()) else { return false };
    let Some(scope) = kubuno_desktop_views::scope::current() else { return false };
    scope.get(&site.name).is_some() && !kubuno_desktop_views::scope::can_raise_now()
}
