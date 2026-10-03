//! `<PrintDialog>` and `<PageSetupDialog>` (WinForms `PrintDialog`, `PageSetupDialog`): Windows' own
//! common dialogs (`PrintDlgEx`, `PageSetupDlg`) — the printer, copies, collation, page range and
//! print to file; the paper, tray, orientation and margins — bound to a document's settings.
//!
//! ```xml
//! <PrintDialog x:Name="print_dialog1" Document="print_document1" AllowSomePages="true"/>
//! ```
//!
//! `show_dialog()` returns [`DialogResult::OK`] when the user chose Print (or OK), after writing the
//! choices into the document ([`PrintDocument::set_printer_settings`] /
//! [`PrintDocument::set_default_page_settings`]). The dialogs are modal to the active window.

use std::cell::RefCell;
use std::path::PathBuf;
use std::ptr::null_mut;
use std::rc::Rc;

use kubuno_controls::buttons::DialogResult;
use kubuno_views::prelude::*;
use windows_sys::Win32::Foundation::{GlobalFree, HGLOBAL, POINT, RECT};
use windows_sys::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
use windows_sys::Win32::UI::Controls::Dialogs::{
    CommDlgExtendedError, GetSaveFileNameW, PageSetupDlgW, PrintDlgExW, DEVNAMES, OPENFILENAMEW, PAGESETUPDLGW, PRINTDLGEXW, PRINTPAGERANGE,
};

use crate::document::PrintDocument;
use crate::native::{self, DevMode};
use crate::settings::{Margins, PageSettings, PrintRange, PrinterSettings};
use crate::text;

// PrintDlgEx flags (commdlg.h).
const PD_SELECTION: u32 = 0x1;
const PD_PAGENUMS: u32 = 0x2;
const PD_NOSELECTION: u32 = 0x4;
const PD_NOPAGENUMS: u32 = 0x8;
const PD_COLLATE: u32 = 0x10;
const PD_PRINTTOFILE: u32 = 0x20;
const PD_USEDEVMODECOPIESANDCOLLATE: u32 = 0x40000;
const PD_DISABLEPRINTTOFILE: u32 = 0x80000;
const PD_NONETWORKBUTTON: u32 = 0x200000;
const PD_CURRENTPAGE: u32 = 0x400000;
const PD_NOCURRENTPAGE: u32 = 0x800000;
const START_PAGE_GENERAL: u32 = 0xFFFF_FFFF;
const PD_RESULT_PRINT: u32 = 1;
const PD_RESULT_APPLY: u32 = 2;
// PageSetupDlg flags.
const PSD_MINMARGINS: u32 = 0x1;
const PSD_MARGINS: u32 = 0x2;
const PSD_INTHOUSANDTHSOFINCHES: u32 = 0x4;
const PSD_DISABLEMARGINS: u32 = 0x10;
const PSD_DISABLEPRINTER: u32 = 0x20;
const PSD_DISABLEORIENTATION: u32 = 0x100;
const PSD_DISABLEPAPER: u32 = 0x200;
const PSD_NONETWORKBUTTON: u32 = 0x200000;
// GetSaveFileName flags.
const OFN_OVERWRITEPROMPT: u32 = 0x2;
const OFN_HIDEREADONLY: u32 = 0x4;
const OFN_PATHMUSTEXIST: u32 = 0x800;

/// Where a dialog reads and writes its settings: a document of the view (by name), a document of
/// code, or its own settings.
#[derive(Default)]
pub(crate) enum Target {
    #[default]
    Own,
    Code(Rc<RefCell<PrintDocument>>),
}

impl std::fmt::Debug for Target {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Target::Own => f.write_str("Own"),
            Target::Code(_) => f.write_str("Code(PrintDocument)"),
        }
    }
}

/// Runs `f` on the dialog's document: the code one, else the view's component named `name`.
fn with_document<R>(target: &Target, name: &str, f: impl FnOnce(&mut PrintDocument) -> R) -> Option<R> {
    match target {
        Target::Code(doc) => match doc.try_borrow_mut() {
            Ok(mut d) => Some(f(&mut d)),
            Err(_) => None,
        },
        Target::Own => {
            let name = name.trim();
            if name.is_empty() {
                return None;
            }
            kubuno_views::scope::current()?.with::<PrintDocument, _>(name, f)
        }
    }
}

// ── DEVNAMES ─────────────────────────────────────────────────────────────────────────────────────

/// A `DEVNAMES` block naming `printer` (a common dialog's initial printer), freed by the caller.
fn devnames_for(printer: &str) -> HGLOBAL {
    let header = std::mem::size_of::<DEVNAMES>() / 2;
    let device: Vec<u16> = native::wide(printer);
    // driver "" | device | port ""
    let mut chars = vec![0u16; header];
    let driver_at = chars.len();
    chars.push(0);
    let device_at = chars.len();
    chars.extend_from_slice(&device);
    let port_at = chars.len();
    chars.push(0);
    // SAFETY: a new block of the right size, filled while locked.
    unsafe {
        let h = GlobalAlloc(GMEM_MOVEABLE, chars.len() * 2);
        if h.is_null() {
            return null_mut();
        }
        let p = GlobalLock(h).cast::<u16>();
        if p.is_null() {
            GlobalFree(h);
            return null_mut();
        }
        std::ptr::copy_nonoverlapping(chars.as_ptr(), p, chars.len());
        let names = p.cast::<DEVNAMES>();
        (*names).wDriverOffset = driver_at as u16;
        (*names).wDeviceOffset = device_at as u16;
        (*names).wOutputOffset = port_at as u16;
        (*names).wDefault = 0;
        GlobalUnlock(h);
        h
    }
}

/// The printer a `DEVNAMES` block names.
fn devnames_printer(h: HGLOBAL) -> Option<String> {
    if h.is_null() {
        return None;
    }
    // SAFETY: a block the dialog filled: a DEVNAMES header and NUL-terminated strings at its offsets.
    unsafe {
        let p = GlobalLock(h).cast::<u16>();
        if p.is_null() {
            return None;
        }
        let names = &*p.cast::<DEVNAMES>();
        let name = native::from_wide_ptr(p.add(names.wDeviceOffset as usize));
        GlobalUnlock(h);
        Some(name).filter(|n| !n.is_empty())
    }
}

/// Frees the blocks a dialog returned.
fn free(blocks: &[HGLOBAL]) {
    for h in blocks {
        if !h.is_null() {
            // SAFETY: blocks allocated for (or by) the dialog, freed once.
            unsafe { GlobalFree(*h) };
        }
    }
}

// ── PrintDialog ──────────────────────────────────────────────────────────────────────────────────

/// `<PrintDialog>` (see the module doc).
#[derive(Component)]
#[kubuno(extends = Component)]
#[toolbox(icon = "printer-check", category = "Printing")]
#[default_property("Document")]
pub struct PrintDialog {
    base: ComponentCore,
    /// The PrintDocument whose printer settings the dialog shows and changes.
    #[property]
    #[category("Data")]
    #[editor("reference:PrintDocument")]
    pub document: String,
    /// Whether the Current Page option is offered.
    #[property]
    #[category("Behavior")]
    #[default_value(false)]
    pub allow_current_page: bool,
    /// Whether the Print to file check box is enabled.
    #[property]
    #[category("Behavior")]
    #[default_value(true)]
    pub allow_print_to_file: bool,
    /// Whether the Selection option is offered.
    #[property]
    #[category("Behavior")]
    #[default_value(false)]
    pub allow_selection: bool,
    /// Whether the Pages option (a page range) is offered.
    #[property]
    #[category("Behavior")]
    #[default_value(false)]
    pub allow_some_pages: bool,
    /// Whether the Print to file check box is checked.
    #[property]
    #[category("Behavior")]
    #[default_value(false)]
    pub print_to_file: bool,
    /// Whether the Network button is shown.
    #[property]
    #[category("Behavior")]
    #[default_value(true)]
    pub show_network: bool,
    printer_settings: PrinterSettings,
    target: Target,
}

impl Default for PrintDialog {
    fn default() -> Self {
        Self {
            base: ComponentCore::default(),
            document: String::new(),
            allow_current_page: false,
            allow_print_to_file: true,
            allow_selection: false,
            allow_some_pages: false,
            print_to_file: false,
            show_network: true,
            printer_settings: PrinterSettings::default(),
            target: Target::Own,
        }
    }
}

impl std::fmt::Debug for PrintDialog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PrintDialog").field("document", &self.document).field("target", &self.target).finish()
    }
}

impl PrintDialog {
    pub fn new() -> Self {
        Self::default()
    }

    /// Binds the dialog to a document of code (`printDialog.Document = doc`).
    pub fn with_document(mut self, document: Rc<RefCell<PrintDocument>>) -> Self {
        self.target = Target::Code(document);
        self
    }

    /// The settings the dialog shows when it has no document (`PrinterSettings`), and what it chose.
    pub fn printer_settings(&self) -> &PrinterSettings {
        &self.printer_settings
    }

    pub fn set_printer_settings(&mut self, settings: PrinterSettings) {
        self.printer_settings = settings;
    }

    /// Shows the dialog (`ShowDialog()`): `OK` when the user chose Print or Apply.
    pub fn show_dialog(&mut self) -> DialogResult {
        let mut settings = with_document(&self.target, &self.document, |d| d.printer_settings().clone()).unwrap_or_else(|| self.printer_settings.clone());
        if self.print_to_file {
            settings.print_to_file = true;
        }
        let Some(chosen) = self.run(&settings) else { return DialogResult::Cancel };
        self.print_to_file = chosen.print_to_file;
        if with_document(&self.target, &self.document, |d| d.set_printer_settings(chosen.clone())).is_none() {
            self.printer_settings = chosen;
        }
        DialogResult::OK
    }

    /// `PrintDlgEx` over `settings`; the chosen settings, or `None` when cancelled.
    fn run(&self, settings: &PrinterSettings) -> Option<PrinterSettings> {
        let printer = settings.resolved_printer_name().ok();
        let devmode = printer.as_deref().and_then(|p| DevMode::for_printer(p, settings.devmode.as_ref()).ok());
        let mut ranges = [PRINTPAGERANGE { nFromPage: settings.from_page.max(1), nToPage: settings.to_page.max(settings.from_page.max(1)) }];
        let mut flags = PD_USEDEVMODECOPIESANDCOLLATE;
        if !self.allow_selection {
            flags |= PD_NOSELECTION;
        }
        if !self.allow_some_pages {
            flags |= PD_NOPAGENUMS;
        }
        if !self.allow_current_page {
            flags |= PD_NOCURRENTPAGE;
        }
        if !self.allow_print_to_file {
            flags |= PD_DISABLEPRINTTOFILE;
        }
        if settings.print_to_file {
            flags |= PD_PRINTTOFILE;
        }
        if !self.show_network {
            flags |= PD_NONETWORKBUTTON;
        }
        match settings.print_range {
            PrintRange::SomePages if self.allow_some_pages => flags |= PD_PAGENUMS,
            PrintRange::Selection if self.allow_selection => flags |= PD_SELECTION,
            PrintRange::CurrentPage if self.allow_current_page => flags |= PD_CURRENTPAGE,
            _ => {}
        }
        if settings.collate {
            flags |= PD_COLLATE;
        }
        let h_devmode = devmode.as_ref().map_or(null_mut(), DevMode::to_hglobal);
        let h_devnames = printer.as_deref().map_or(null_mut(), devnames_for);
        // SAFETY: a zeroed struct is its documented empty state.
        let mut pd: PRINTDLGEXW = unsafe { std::mem::zeroed() };
        pd.lStructSize = std::mem::size_of::<PRINTDLGEXW>() as u32;
        pd.hwndOwner = native::owner_window();
        pd.hDevMode = h_devmode;
        pd.hDevNames = h_devnames;
        pd.Flags = flags;
        pd.nPageRanges = if self.allow_some_pages { 1 } else { 0 };
        pd.nMaxPageRanges = 1;
        pd.lpPageRanges = ranges.as_mut_ptr();
        pd.nMinPage = settings.minimum_page.max(1);
        pd.nMaxPage = settings.maximum_page.max(settings.minimum_page.max(1));
        pd.nCopies = settings.copies.max(1) as u32;
        pd.nStartPage = START_PAGE_GENERAL;
        // SAFETY: a filled PRINTDLGEXW whose pointers (the range array, the blocks) outlive the call.
        let hr = unsafe { PrintDlgExW(&mut pd) };
        let result = if hr < 0 {
            // SAFETY: no arguments.
            let code = unsafe { CommDlgExtendedError() };
            tracing::error!(target: "kubuno_print", "PrintDlgEx failed: 0x{hr:08X} (CommDlgExtendedError {code})");
            None
        } else if pd.dwResultAction == PD_RESULT_PRINT || pd.dwResultAction == PD_RESULT_APPLY {
            let mut chosen = settings.clone();
            if let Some(name) = devnames_printer(pd.hDevNames) {
                chosen.printer_name = name;
            }
            if let Some(dm) = DevMode::from_hglobal(pd.hDevMode) {
                dm.read_printer(&mut chosen);
                chosen.devmode = Some(dm);
            }
            chosen.print_to_file = pd.Flags & PD_PRINTTOFILE != 0;
            chosen.collate = pd.Flags & PD_COLLATE != 0 || chosen.collate;
            chosen.print_range = if pd.Flags & PD_PAGENUMS != 0 {
                PrintRange::SomePages
            } else if pd.Flags & PD_SELECTION != 0 {
                PrintRange::Selection
            } else if pd.Flags & PD_CURRENTPAGE != 0 {
                PrintRange::CurrentPage
            } else {
                PrintRange::AllPages
            };
            if chosen.print_range == PrintRange::SomePages && pd.nPageRanges > 0 {
                chosen.from_page = ranges[0].nFromPage;
                chosen.to_page = ranges[0].nToPage;
            }
            Some(chosen)
        } else {
            None
        };
        free(&[pd.hDevMode, pd.hDevNames]);
        if pd.hDevMode != h_devmode {
            free(&[h_devmode]);
        }
        if pd.hDevNames != h_devnames {
            free(&[h_devnames]);
        }
        result
    }
}

// ── PageSetupDialog ──────────────────────────────────────────────────────────────────────────────

/// `<PageSetupDialog>` (see the module doc).
#[derive(Component)]
#[kubuno(extends = Component)]
#[toolbox(icon = "file-sliders", category = "Printing")]
#[default_property("Document")]
pub struct PageSetupDialog {
    base: ComponentCore,
    /// The PrintDocument whose page settings the dialog shows and changes.
    #[property]
    #[category("Data")]
    #[editor("reference:PrintDocument")]
    pub document: String,
    /// Whether the margins can be changed.
    #[property]
    #[category("Behavior")]
    #[default_value(true)]
    pub allow_margins: bool,
    /// Whether the orientation can be changed.
    #[property]
    #[category("Behavior")]
    #[default_value(true)]
    pub allow_orientation: bool,
    /// Whether the paper size and tray can be changed.
    #[property]
    #[category("Behavior")]
    #[default_value(true)]
    pub allow_paper: bool,
    /// Whether the Printer button is enabled.
    #[property]
    #[category("Behavior")]
    #[default_value(true)]
    pub allow_printer: bool,
    /// The smallest margins the user may enter, in hundredths of an inch (left, right, top, bottom).
    #[property]
    #[category("Behavior")]
    #[default_value("0, 0, 0, 0")]
    pub min_margins: String,
    /// Whether the Network button is shown.
    #[property]
    #[category("Behavior")]
    #[default_value(true)]
    pub show_network: bool,
    page_settings: PageSettings,
    printer_settings: PrinterSettings,
    target: Target,
}

impl Default for PageSetupDialog {
    fn default() -> Self {
        Self {
            base: ComponentCore::default(),
            document: String::new(),
            allow_margins: true,
            allow_orientation: true,
            allow_paper: true,
            allow_printer: true,
            min_margins: "0, 0, 0, 0".to_string(),
            show_network: true,
            page_settings: PageSettings::default(),
            printer_settings: PrinterSettings::default(),
            target: Target::Own,
        }
    }
}

impl std::fmt::Debug for PageSetupDialog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PageSetupDialog").field("document", &self.document).field("target", &self.target).finish()
    }
}

impl PageSetupDialog {
    pub fn new() -> Self {
        Self::default()
    }

    /// Binds the dialog to a document of code.
    pub fn with_document(mut self, document: Rc<RefCell<PrintDocument>>) -> Self {
        self.target = Target::Code(document);
        self
    }

    /// The page settings the dialog shows when it has no document (`PageSettings`), and what it chose.
    pub fn page_settings(&self) -> &PageSettings {
        &self.page_settings
    }

    pub fn set_page_settings(&mut self, settings: PageSettings) {
        self.page_settings = settings;
    }

    /// Shows the dialog (`ShowDialog()`): `OK` when the user chose OK.
    pub fn show_dialog(&mut self) -> DialogResult {
        let (page, printer) = with_document(&self.target, &self.document, |d| (d.default_page_settings().clone(), d.printer_settings().clone()))
            .unwrap_or_else(|| (self.page_settings.clone(), self.printer_settings.clone()));
        let Some((page, printer)) = self.run(&page, &printer) else { return DialogResult::Cancel };
        let applied = with_document(&self.target, &self.document, |d| {
            d.set_default_page_settings(page.clone());
            d.set_printer_settings(printer.clone());
        });
        if applied.is_none() {
            self.page_settings = page;
            self.printer_settings = printer;
        }
        DialogResult::OK
    }

    fn run(&self, page: &PageSettings, printer: &PrinterSettings) -> Option<(PageSettings, PrinterSettings)> {
        let name = printer.resolved_printer_name().ok();
        let devmode = name.as_deref().and_then(|n| {
            let base = DevMode::for_printer(n, printer.devmode.as_ref()).ok()?;
            base.with_settings(n, page, printer).ok()
        });
        let mut flags = PSD_MARGINS | PSD_INTHOUSANDTHSOFINCHES;
        let min = Margins::parse(&self.min_margins).unwrap_or(Margins::all(0));
        if min != Margins::all(0) {
            flags |= PSD_MINMARGINS;
        }
        if !self.allow_margins {
            flags |= PSD_DISABLEMARGINS;
        }
        if !self.allow_orientation {
            flags |= PSD_DISABLEORIENTATION;
        }
        if !self.allow_paper {
            flags |= PSD_DISABLEPAPER;
        }
        if !self.allow_printer {
            flags |= PSD_DISABLEPRINTER;
        }
        if !self.show_network {
            flags |= PSD_NONETWORKBUTTON;
        }
        let h_devmode = devmode.as_ref().map_or(null_mut(), DevMode::to_hglobal);
        let h_devnames = name.as_deref().map_or(null_mut(), devnames_for);
        let m = page.margins;
        // Hundredths → thousandths of an inch.
        let rect = |m: Margins| RECT { left: m.left * 10, top: m.top * 10, right: m.right * 10, bottom: m.bottom * 10 };
        // SAFETY: a zeroed struct is its documented empty state.
        let mut psd: PAGESETUPDLGW = unsafe { std::mem::zeroed() };
        psd.lStructSize = std::mem::size_of::<PAGESETUPDLGW>() as u32;
        psd.hwndOwner = native::owner_window();
        psd.hDevMode = h_devmode;
        psd.hDevNames = h_devnames;
        psd.Flags = flags;
        psd.ptPaperSize = POINT { x: 0, y: 0 };
        psd.rtMinMargin = rect(min);
        psd.rtMargin = rect(m);
        // SAFETY: a filled PAGESETUPDLGW whose blocks outlive the call.
        let ok = unsafe { PageSetupDlgW(&mut psd) } != 0;
        let result = if ok {
            let mut chosen_page = page.clone();
            let mut chosen_printer = printer.clone();
            if let Some(n) = devnames_printer(psd.hDevNames) {
                chosen_printer.printer_name = n;
            }
            let sizes = native::paper_sizes(&chosen_printer.printer_name);
            if let Some(dm) = DevMode::from_hglobal(psd.hDevMode) {
                dm.read_page(&mut chosen_page, &sizes);
                dm.read_printer(&mut chosen_printer);
                chosen_printer.devmode = Some(dm);
            }
            let r = psd.rtMargin;
            // Thousandths → hundredths of an inch.
            chosen_page.margins = Margins::new((r.left + 5) / 10, (r.right + 5) / 10, (r.top + 5) / 10, (r.bottom + 5) / 10);
            Some((chosen_page, chosen_printer))
        } else {
            // SAFETY: no arguments.
            let code = unsafe { CommDlgExtendedError() };
            if code != 0 {
                tracing::error!(target: "kubuno_print", "PageSetupDlg failed (CommDlgExtendedError {code})");
            }
            None
        };
        free(&[psd.hDevMode, psd.hDevNames]);
        if psd.hDevMode != h_devmode {
            free(&[h_devmode]);
        }
        if psd.hDevNames != h_devnames {
            free(&[h_devnames]);
        }
        result
    }
}

// ── Print to file ────────────────────────────────────────────────────────────────────────────────

/// Asks where to print a document into (print to file without a file name): a Save dialog offering
/// the printer's own format (PDF for "Microsoft Print to PDF", XPS for the XPS writer, `.prn`
/// otherwise). `None` when cancelled.
pub(crate) fn ask_output_file(document: &str, printer: &str) -> Option<PathBuf> {
    let lower = printer.to_lowercase();
    let (filter, ext) = if lower.contains("pdf") {
        ("PDF (*.pdf)\0*.pdf\0", "pdf")
    } else if lower.contains("xps") {
        ("XPS (*.xps; *.oxps)\0*.xps;*.oxps\0", "xps")
    } else {
        ("Printer files (*.prn)\0*.prn\0", "prn")
    };
    let filter: Vec<u16> = format!("{filter}{}\0*.*\0\0", text::all_files()).encode_utf16().collect();
    let ext_w = native::wide(ext);
    let title = native::wide(&text::print_to_file_title());
    let mut file = vec![0u16; 1024];
    let initial: Vec<u16> = format!("{document}.{ext}").encode_utf16().take(1000).collect();
    file[..initial.len()].copy_from_slice(&initial);
    // SAFETY: a zeroed struct is its documented empty state.
    let mut ofn: OPENFILENAMEW = unsafe { std::mem::zeroed() };
    ofn.lStructSize = std::mem::size_of::<OPENFILENAMEW>() as u32;
    ofn.hwndOwner = native::owner_window();
    ofn.lpstrFilter = filter.as_ptr();
    ofn.nFilterIndex = 1;
    ofn.lpstrFile = file.as_mut_ptr();
    ofn.nMaxFile = file.len() as u32;
    ofn.lpstrDefExt = ext_w.as_ptr();
    ofn.lpstrTitle = title.as_ptr();
    ofn.Flags = OFN_OVERWRITEPROMPT | OFN_HIDEREADONLY | OFN_PATHMUSTEXIST;
    // SAFETY: a filled OPENFILENAMEW whose buffers outlive the call.
    let ok = unsafe { GetSaveFileNameW(&mut ofn) } != 0;
    if !ok {
        return None;
    }
    let end = file.iter().position(|c| *c == 0).unwrap_or(file.len());
    Some(PathBuf::from(String::from_utf16_lossy(&file[..end])))
}
