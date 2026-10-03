//! Printing, the Windows Forms way (`vskubuno/docs/PRINTING.md`): [`PrintDocument`],
//! [`PrintPreviewDialog`], [`PrintDialog`], [`PageSetupDialog`], [`PrintPreviewControl`] and the
//! settings of `System.Drawing.Printing`.
//!
//! ```no_run
//! use kubuno::prelude::*;
//! use kubuno::printing::*;
//! use kubuno::ui::graphics::{Color, Font, FontStyle, StringFormat};
//!
//! # fn f(owner: &Form) {
//! let doc = PrintDocument::new().document_name("Report");
//! let mut line = 0;
//! doc.on_begin_print(move |_, _| {});
//! doc.on_print_page(move |_, e| {
//!     line += 1;
//!     let g = e.graphics();
//!     g.draw_string(&format!("Page {line}"), &Font::new("Segoe UI", 14.0, FontStyle::REGULAR), Color::BLACK, e.margin_bounds, &StringFormat::generic_default());
//!     e.has_more_pages = line < 3;
//! });
//! PrintPreviewDialog::new().document(&doc).show_dialog(owner);
//! # }
//! ```
//!
//! The types here are cheap, clonable **handles**, like the controls of [`crate::forms`]: a component
//! of a `.kbview` (the fields `#[kubuno::view]` generates for `<PrintDocument x:Name="…">`…), or a
//! component created in code. The component classes themselves are in [`components`]
//! (`kubuno_print`).
//!
//! A document of a view printed — or previewed — from one of the view's handlers prints **when that
//! handler returns** (its own `PrintPage` handler is a method of the view, which the running handler
//! borrows); see `kubuno_print::document`.

use std::cell::RefCell;
use std::rc::Rc;

use kubuno_views::events::{ElementRef, Event};

use crate::forms::{AsControl, AsForm, Control, DialogResult};

pub use kubuno_print::{
    dip_to_hundredths, hundredths_to_dip, Duplex, Margins, PageSettings, PaperSize, PaperSource, PrintAction, PrintError, PrintEventArgs, PrintPageEventArgs, PrintRange,
    PrinterResolution, PrinterSettings, QueryPageSettingsEventArgs,
};

/// The component classes (`<PrintDocument>`… of a `.kbview`), for code that works with the view
/// runtime directly (`runtime.with_component::<components::PrintDocument, _>(…)`).
pub mod components {
    pub use kubuno_print::{PageSetupDialog, PreviewPagesButton, PrintDialog, PrintDocument, PrintPreviewControl, PrintPreviewDialog};
}

/// `kubuno_controls`' `DialogResult` (what `kubuno_print` returns) as the forms' one.
fn result(r: kubuno_print::DialogResult) -> DialogResult {
    use kubuno_print::DialogResult as R;
    match r {
        R::None => DialogResult::None,
        R::OK => DialogResult::Ok,
        R::Cancel => DialogResult::Cancel,
        R::Abort => DialogResult::Abort,
        R::Retry => DialogResult::Retry,
        R::Ignore => DialogResult::Ignore,
        R::Yes => DialogResult::Yes,
        R::No => DialogResult::No,
        R::TryAgain => DialogResult::TryAgain,
        R::Continue => DialogResult::Continue,
    }
}

/// Runs `f` on the view component a linked handle stands for: the one of its form's open window,
/// else of the view whose frame is running. `None` when it cannot be reached now (no window yet,
/// or the component is busy).
fn with_view_component<T: kubuno_views::component::Component, R>(control: &Control, f: impl FnOnce(&mut T) -> R) -> Option<R> {
    let name = control.get_name();
    let scope = control.form().and_then(|form| form.shared.scope.borrow().clone()).or_else(kubuno_views::scope::current)?;
    scope.with::<T, R>(&name, f)
}

/// A component created in code, or one of a view.
#[derive(Clone)]
enum Backing<T> {
    Own(Rc<RefCell<T>>),
    View,
}

macro_rules! handle {
    ($(#[$doc:meta])* $name:ident, $class:ident) => {
        $(#[$doc])*
        #[derive(Clone)]
        pub struct $name {
            control: Control,
            own: Rc<RefCell<kubuno_print::$class>>,
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl std::fmt::Debug for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}({:?})", stringify!($name), self.control.get_name())
            }
        }

        impl AsControl for $name {
            fn as_control(&self) -> &Control {
                &self.control
            }
        }

        impl $name {
            /// The element's name in the `.kbview`.
            pub const ELEMENT: &'static str = stringify!($class);

            /// A component created in code.
            pub fn new() -> Self {
                Self { control: Control::new(stringify!($class)), own: Rc::new(RefCell::new(kubuno_print::$class::default())) }
            }

            fn backing(&self) -> Backing<kubuno_print::$class> {
                if self.control.0.from_view.get() && !self.control.get_name().is_empty() {
                    Backing::View
                } else {
                    Backing::Own(self.own.clone())
                }
            }

            /// Runs `f` on the component (`None`: a component of a view not reachable now).
            pub fn with<R>(&self, f: impl FnOnce(&mut kubuno_print::$class) -> R) -> Option<R> {
                match self.backing() {
                    Backing::Own(own) => match own.try_borrow_mut() {
                        Ok(mut c) => Some(f(&mut c)),
                        Err(_) => {
                            tracing::warn!(concat!(stringify!($name), " is busy (reached from one of its own events)"));
                            None
                        }
                    },
                    Backing::View => {
                        let out = with_view_component::<kubuno_print::$class, R>(&self.control, f);
                        if out.is_none() {
                            tracing::warn!(name = %self.control.get_name(), concat!(stringify!($name), " of the view cannot be reached now (its window is not open, or it is busy)"));
                        }
                        out
                    }
                }
            }
        }
    };
}

handle!(
    /// A document to print (WinForms `PrintDocument`): its settings, its events (`BeginPrint`,
    /// `QueryPageSettings`, `PrintPage`, `EndPrint`) and [`PrintDocument::print`].
    PrintDocument,
    PrintDocument
);

impl PrintDocument {
    /// The document's name in the printer's queue (`DocumentName`), builder form.
    pub fn document_name(self, name: impl Into<String>) -> Self {
        self.set_document_name(name);
        self
    }

    pub fn set_document_name(&self, name: impl Into<String>) {
        let name = name.into();
        self.with(|d| d.document_name = name);
    }

    pub fn get_document_name(&self) -> String {
        self.with(|d| d.document_name.clone()).unwrap_or_default()
    }

    /// Whether the page's `Graphics` starts at the margins (`OriginAtMargins`).
    pub fn set_origin_at_margins(&self, on: bool) {
        self.with(|d| d.origin_at_margins = on);
    }

    /// The printer and the job's options (a copy: change it and [`Self::set_printer_settings`]).
    pub fn printer_settings(&self) -> PrinterSettings {
        self.with(|d| d.printer_settings().clone()).unwrap_or_default()
    }

    pub fn set_printer_settings(&self, settings: PrinterSettings) {
        self.with(|d| d.set_printer_settings(settings));
    }

    /// The settings every page starts from (a copy: change it and
    /// [`Self::set_default_page_settings`]).
    pub fn default_page_settings(&self) -> PageSettings {
        self.with(|d| d.default_page_settings().clone()).unwrap_or_default()
    }

    pub fn set_default_page_settings(&self, settings: PageSettings) {
        self.with(|d| d.set_default_page_settings(settings));
    }

    /// Prints the document (`Print()`): the pages sent to the printer, 0 when the print waits for
    /// the running handler to return (see the module doc; read [`Self::last_error`] afterwards).
    pub fn print(&self) -> Result<usize, PrintError> {
        self.with(|d| d.print()).unwrap_or(Err(PrintError::Busy("PrintDocument")))
    }

    /// Why the last print failed.
    pub fn last_error(&self) -> Option<String> {
        self.with(|d| d.last_error().map(str::to_string)).flatten()
    }

    /// Asks the previews to render the pages again (the data printed changed).
    pub fn invalidate(&self) {
        self.with(|d| d.invalidate());
    }

    /// `PrintPage += f`: `f` draws a page on `e.graphics()` and sets `e.has_more_pages`.
    pub fn on_print_page(&self, f: impl FnMut(&ElementRef<'_>, &mut PrintPageEventArgs) + 'static) {
        self.with(|d| {
            d.on_print_page(f);
        });
    }

    /// `BeginPrint += f` (reset the handler's state here: a preview then a print run the pages twice).
    pub fn on_begin_print(&self, f: impl FnMut(&ElementRef<'_>, &mut PrintEventArgs) + 'static) {
        self.with(|d| {
            d.on_begin_print(f);
        });
    }

    /// `QueryPageSettings += f`: change a page's settings before it is printed.
    pub fn on_query_page_settings(&self, f: impl FnMut(&ElementRef<'_>, &mut QueryPageSettingsEventArgs) + 'static) {
        self.with(|d| {
            d.on_query_page_settings(f);
        });
    }

    /// `EndPrint += f`.
    pub fn on_end_print(&self, f: impl FnMut(&ElementRef<'_>, &mut PrintEventArgs) + 'static) {
        self.with(|d| {
            d.on_end_print(f);
        });
    }

    /// The `PrintPage` event, to subscribe with a [`kubuno_views::events::Subscription`] of your own.
    pub fn print_page(&self) -> Event<PrintPageEventArgs> {
        self.with(|d| d.print_page.clone()).unwrap_or_default()
    }

    /// The document of code behind this handle, or the view's name of it: how a dialog reaches it.
    fn source(&self) -> DocSource {
        match self.backing() {
            Backing::Own(own) => DocSource::Code(own),
            Backing::View => DocSource::View(self.control.get_name()),
        }
    }
}

enum DocSource {
    Code(Rc<RefCell<kubuno_print::PrintDocument>>),
    View(String),
}

handle!(
    /// The print preview window (WinForms `PrintPreviewDialog`).
    PrintPreviewDialog,
    PrintPreviewDialog
);

impl PrintPreviewDialog {
    /// The document to preview (`Document = doc`), builder form.
    pub fn document(self, document: &PrintDocument) -> Self {
        self.set_document(document);
        self
    }

    pub fn set_document(&self, document: &PrintDocument) {
        let source = document.source();
        self.with(|d| match source {
            DocSource::Code(doc) => *d = std::mem::take(d).with_document(doc),
            DocSource::View(name) => d.document = name,
        });
    }

    /// The window's title (`Text`).
    pub fn set_text(&self, text: impl Into<String>) {
        let text = text.into();
        self.with(|d| d.text = text);
    }

    /// Renders the pages and shows the preview, modal to `owner` (`ShowDialog(owner)`). A preview of a
    /// document of the view opens when the running handler returns (`DialogResult::None` then).
    pub fn show_dialog(&self, owner: &dyn AsForm) -> DialogResult {
        let _ = owner;
        self.with(|d| result(d.show_dialog())).unwrap_or(DialogResult::None)
    }
}

handle!(
    /// Windows' Print dialog (WinForms `PrintDialog`): printer, copies, collation, page range, print
    /// to file, written into its document's printer settings.
    PrintDialog,
    PrintDialog
);

impl PrintDialog {
    /// The document whose settings the dialog changes (`Document = doc`), builder form.
    pub fn document(self, document: &PrintDocument) -> Self {
        self.set_document(document);
        self
    }

    pub fn set_document(&self, document: &PrintDocument) {
        let source = document.source();
        self.with(|d| match source {
            DocSource::Code(doc) => *d = std::mem::take(d).with_document(doc),
            DocSource::View(name) => d.document = name,
        });
    }

    /// Offers a page range (`AllowSomePages`).
    pub fn set_allow_some_pages(&self, on: bool) {
        self.with(|d| d.allow_some_pages = on);
    }

    /// Offers the Selection option (`AllowSelection`).
    pub fn set_allow_selection(&self, on: bool) {
        self.with(|d| d.allow_selection = on);
    }

    /// Offers the Current Page option (`AllowCurrentPage`).
    pub fn set_allow_current_page(&self, on: bool) {
        self.with(|d| d.allow_current_page = on);
    }

    /// Shows the dialog (`ShowDialog(owner)`): `Ok` when the user chose Print.
    pub fn show_dialog(&self, owner: &dyn AsForm) -> DialogResult {
        let _ = owner;
        self.with(|d| result(d.show_dialog())).unwrap_or(DialogResult::None)
    }
}

handle!(
    /// Windows' Page Setup dialog (WinForms `PageSetupDialog`): paper, tray, orientation, margins,
    /// written into its document's default page settings.
    PageSetupDialog,
    PageSetupDialog
);

impl PageSetupDialog {
    /// The document whose page settings the dialog changes (`Document = doc`), builder form.
    pub fn document(self, document: &PrintDocument) -> Self {
        self.set_document(document);
        self
    }

    pub fn set_document(&self, document: &PrintDocument) {
        let source = document.source();
        self.with(|d| match source {
            DocSource::Code(doc) => *d = std::mem::take(d).with_document(doc),
            DocSource::View(name) => d.document = name,
        });
    }

    /// Shows the dialog (`ShowDialog(owner)`): `Ok` when the user chose OK.
    pub fn show_dialog(&self, owner: &dyn AsForm) -> DialogResult {
        let _ = owner;
        self.with(|d| result(d.show_dialog())).unwrap_or(DialogResult::None)
    }
}

/// The preview of a document inside a form (WinForms `PrintPreviewControl`): a control of the view
/// (`<PrintPreviewControl x:Name="preview" Document="print_document1"/>`), driven from code.
#[derive(Clone, Debug, Default)]
pub struct PrintPreviewControl {
    control: Control,
}

impl AsControl for PrintPreviewControl {
    fn as_control(&self) -> &Control {
        &self.control
    }
}

impl std::ops::Deref for PrintPreviewControl {
    type Target = Control;
    fn deref(&self) -> &Control {
        &self.control
    }
}

impl PrintPreviewControl {
    /// The element's name in the `.kbview`.
    pub const ELEMENT: &'static str = "PrintPreviewControl";

    /// A control created in code (add it to a form; its document is a view's, by name:
    /// `property("Document", "print_document1")`).
    pub fn new() -> Self {
        Self { control: Control::new(Self::ELEMENT) }
    }

    fn with<R>(&self, f: impl FnOnce(&mut kubuno_print::PrintPreviewControl) -> R) -> Option<R> {
        with_view_component::<kubuno_print::PrintPreviewControl, R>(&self.control, f)
    }

    /// Shows the pages of `document` (`Document = doc`).
    pub fn set_document(&self, document: &PrintDocument) {
        match document.source() {
            DocSource::View(name) => self.control.set_property("Document", name),
            DocSource::Code(doc) => {
                self.with(|c| c.set_document(doc));
            }
        }
    }

    /// Renders the pages again (`InvalidatePreview()`).
    pub fn invalidate_preview(&self) {
        self.with(|c| c.invalidate_preview());
    }

    /// The zoom (1.0 = actual size); turns `AutoZoom` off.
    pub fn set_zoom(&self, zoom: f32) {
        self.control.set_property("AutoZoom", false);
        self.control.set_property("Zoom", zoom);
    }

    /// Whether the pages are fitted to the control (`AutoZoom`).
    pub fn set_auto_zoom(&self, on: bool) {
        self.control.set_property("AutoZoom", on);
    }

    /// Shows `columns` × `rows` pages.
    pub fn set_layout(&self, columns: u32, rows: u32) {
        self.control.set_property("Columns", columns);
        self.control.set_property("Rows", rows);
    }

    /// The first page shown, from 0 (`StartPage`).
    pub fn set_start_page(&self, page: u32) {
        self.with(|c| c.set_start_page(page));
    }

    /// How many pages the document has (0 until they are rendered).
    pub fn page_count(&self) -> usize {
        self.with(|c| c.page_count()).unwrap_or(0)
    }
}
