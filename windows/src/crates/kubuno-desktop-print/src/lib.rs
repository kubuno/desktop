//! # `kubuno-desktop-print` — the Windows Forms printing stack for Kubuno desktop applications
//!
//! `vskubuno/docs/PRINTING.md`. The components of `System.Drawing.Printing` and of the Windows Forms
//! printing dialogs, as non-visual components of a `.kbview` (the designer's component tray) and as
//! plain Rust objects:
//!
//! - [`PrintDocument`]: `BeginPrint`, `QueryPageSettings`, `PrintPage` (the page's EVT-8
//!   [`Graphics`](kubuno_desktop_ui::graphics::Graphics), its bounds, `has_more_pages`, `cancel`),
//!   `EndPrint`; [`PrintDocument::print`].
//! - [`PrinterSettings`] / [`PageSettings`]: the printer (installed printers, the default one),
//!   copies, collation, two-sided printing, page range, printing to a file; paper, tray,
//!   orientation, margins, colour — and what the printer can do, asked of the spooler.
//! - [`PrintPreviewControl`] (a control) and [`PrintPreviewDialog`] (a window with its tool bar):
//!   the pages rendered through the same `PrintPage` handler.
//! - [`PrintDialog`] and [`PageSetupDialog`]: Windows' own common dialogs (`PrintDlgEx`,
//!   `PageSetupDlg`), bound to the document's settings.
//!
//! **How pages reach the printer.** Each page is recorded into a Direct2D command list on a device
//! of the job's own (the `Graphics` of EVT-8 over the host's `Painter`), and `ID2D1PrintControl`
//! writes it into an XPS print job of the Windows spooler, whose print ticket carries the settings
//! (`DEVMODE` → print ticket). The output is vector (text stays text, fonts embedded), the path Windows
//! recommends for Direct2D applications; with "Microsoft Print to PDF" and a file name
//! (`PrinterSettings::print_to_file`), a PDF is written without any dialog. A preview draws the same
//! command lists into bitmaps.
//!
//! An application links the components' registrations (static constructors) with the `kubuno-desktop`
//! crate (`kubuno_desktop::printing`), or with `extern crate kubuno_desktop_print as _;`.

pub mod args;
pub mod dialogs;
pub mod document;
pub mod engine;
mod native;
pub mod preview;
pub mod preview_dialog;
pub mod settings;
mod text;
mod xps;

use std::cell::RefCell;
use std::rc::Rc;

pub use args::{PrintEventArgs, PrintPageEventArgs, QueryPageSettingsEventArgs};
pub use dialogs::{PageSetupDialog, PrintDialog};
pub use document::PrintDocument;
pub use engine::{PageBitmap, Surface};
pub use kubuno_desktop_controls::buttons::DialogResult;
pub use preview::{PreviewLayout, PrintPreviewControl};
pub use preview_dialog::{PreviewPagesButton, PrintPreviewDialog};
pub use settings::{dip_to_hundredths, hundredths_to_dip, Duplex, Margins, PageSettings, PaperSize, PaperSource, PrintAction, PrintRange, PrinterResolution, PrinterSettings};

use kubuno_drive_desktop_app_controls::Theme;
use windows::Win32::Graphics::Direct2D::ID2D1CommandList;

/// What can go wrong printing. Never a panic: every Win32/COM failure is one of these.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PrintError {
    /// No printer is installed (and none was named).
    #[error("no printer is installed")]
    NoPrinter,
    /// The named printer is not installed or cannot be opened (WinForms' `InvalidPrinterException`).
    #[error("the printer \"{0}\" is not installed or cannot be opened")]
    InvalidPrinter(String),
    /// The printer's driver refused the settings.
    #[error("printer driver: {0}")]
    Driver(String),
    /// The spooler refused the job.
    #[error("print spooler: {0}")]
    Spooler(String),
    /// The output file could not be written.
    #[error("output file: {0}")]
    Output(String),
    /// A Direct2D / COM call failed.
    #[error("{what}: {message}")]
    Com { what: &'static str, message: String },
    /// The component is busy (a handler of its own event reached it again).
    #[error("the {0} is busy (reached again from one of its own events)")]
    Busy(&'static str),
}

impl PrintError {
    /// Maps a `windows` error of the call `what`.
    pub(crate) fn com(what: &'static str) -> impl Fn(windows::core::Error) -> PrintError {
        move |e| PrintError::Com { what, message: e.message().to_string() }
    }
}

/// One page rendered for a preview: its Direct2D command list (on the preview's [`Surface`]), its size
/// in DIP and its settings.
#[derive(Clone)]
pub struct PreviewPage {
    pub(crate) list: ID2D1CommandList,
    pub size: (f32, f32),
    pub settings: PageSettings,
}

impl std::fmt::Debug for PreviewPage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PreviewPage").field("size", &self.size).field("landscape", &self.settings.landscape).finish()
    }
}

/// The pages of a document rendered for a preview ([`PrintDocument::render_preview`]).
#[derive(Clone)]
pub struct PreviewDocument {
    pub(crate) surface: Rc<Surface>,
    pub pages: Vec<PreviewPage>,
    /// The document's [`PrintDocument::revision`] the pages were rendered at.
    pub revision: u64,
}

impl std::fmt::Debug for PreviewDocument {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PreviewDocument").field("pages", &self.pages).field("revision", &self.revision).finish()
    }
}

impl PreviewDocument {
    /// Rasterises page `index` at `width` × `height` pixels.
    pub fn rasterize(&self, index: usize, width: u32, height: u32) -> Result<PageBitmap, PrintError> {
        let page = self.pages.get(index).ok_or(PrintError::Driver(format!("no page {}", index + 1)))?;
        self.surface.rasterize(&page.list, page.size, width, height)
    }
}

thread_local! {
    static THEME: RefCell<Option<Theme>> = const { RefCell::new(None) };
}

/// The theme of the print dialogs Kubuno draws (the preview dialog): the application's
/// (`kubuno_desktop::Application::set_theme` forwards it), light by default.
pub fn set_dialog_theme(theme: Theme) {
    THEME.with(|t| *t.borrow_mut() = Some(theme));
}

pub(crate) fn dialog_theme() -> Theme {
    THEME.with(|t| t.borrow().clone()).unwrap_or_else(Theme::light)
}
