//! `PrinterSettings`, `PageSettings` and their value types (WinForms `System.Drawing.Printing`).
//!
//! Units follow Windows Forms: paper sizes, margins and printable areas are in **hundredths of an
//! inch**. The page [`Graphics`](kubuno_ui::graphics::Graphics) a `PrintPage` handler draws on is in
//! DIP (1/96 inch, the unit of every Kubuno surface), and so are the bounds of
//! [`PrintPageEventArgs`](crate::PrintPageEventArgs): [`hundredths_to_dip`] converts.

use std::fmt;

use kubuno_ui::Rect;

use crate::native;
use crate::PrintError;

/// Hundredths of an inch → DIP (1/96 inch).
pub fn hundredths_to_dip(v: f32) -> f32 {
    v * 0.96
}

/// DIP (1/96 inch) → hundredths of an inch.
pub fn dip_to_hundredths(v: f32) -> f32 {
    v / 0.96
}

/// Tenths of a millimetre (what the spooler measures paper in) → hundredths of an inch.
pub(crate) fn tenths_mm_to_hundredths(v: i32) -> i32 {
    ((v as f64) * 100.0 / 254.0).round() as i32
}

/// Hundredths of an inch → tenths of a millimetre.
pub(crate) fn hundredths_to_tenths_mm(v: i32) -> i32 {
    ((v as f64) * 254.0 / 100.0).round() as i32
}

// ── Margins ──────────────────────────────────────────────────────────────────────────────────────

/// The margins of a page, in hundredths of an inch (WinForms `Margins`; 100 = one inch each by
/// default).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Margins {
    pub left: i32,
    pub right: i32,
    pub top: i32,
    pub bottom: i32,
}

impl Default for Margins {
    fn default() -> Self {
        Self::all(100)
    }
}

impl Margins {
    pub const fn new(left: i32, right: i32, top: i32, bottom: i32) -> Self {
        Self { left, right, top, bottom }
    }

    pub const fn all(v: i32) -> Self {
        Self::new(v, v, v, v)
    }

    /// Reads `"left, right, top, bottom"` (the `.kbview` spelling, WinForms' `MarginsConverter`),
    /// or a single number for all four. `None` for anything else, or a negative margin.
    pub fn parse(text: &str) -> Option<Self> {
        let parts: Vec<i32> = text.split([',', ';']).map(|p| p.trim().parse::<i32>()).collect::<Result<_, _>>().ok()?;
        let m = match parts.as_slice() {
            [v] => Self::all(*v),
            [l, r, t, b] => Self::new(*l, *r, *t, *b),
            _ => return None,
        };
        (m.left >= 0 && m.right >= 0 && m.top >= 0 && m.bottom >= 0).then_some(m)
    }
}

impl fmt::Display for Margins {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}, {}, {}, {}", self.left, self.right, self.top, self.bottom)
    }
}

// ── Paper ────────────────────────────────────────────────────────────────────────────────────────

/// A paper size (WinForms `PaperSize`): its name, its portrait size in hundredths of an inch, and
/// the spooler's `DMPAPER_*` number (`raw_kind`; 0 = a custom size).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PaperSize {
    pub paper_name: String,
    pub width: i32,
    pub height: i32,
    pub raw_kind: i32,
}

impl PaperSize {
    /// A custom size (WinForms `new PaperSize(name, width, height)`).
    pub fn custom(name: impl Into<String>, width: i32, height: i32) -> Self {
        Self { paper_name: name.into(), width, height, raw_kind: 0 }
    }

    /// US Letter, 8.5 × 11 in (`DMPAPER_LETTER`).
    pub fn letter() -> Self {
        Self { paper_name: "Letter".into(), width: 850, height: 1100, raw_kind: 1 }
    }

    /// US Legal, 8.5 × 14 in (`DMPAPER_LEGAL`).
    pub fn legal() -> Self {
        Self { paper_name: "Legal".into(), width: 850, height: 1400, raw_kind: 5 }
    }

    /// A4, 210 × 297 mm (`DMPAPER_A4`).
    pub fn a4() -> Self {
        Self { paper_name: "A4".into(), width: 827, height: 1169, raw_kind: 9 }
    }

    /// A3, 297 × 420 mm (`DMPAPER_A3`).
    pub fn a3() -> Self {
        Self { paper_name: "A3".into(), width: 1169, height: 1654, raw_kind: 8 }
    }

    /// A5, 148 × 210 mm (`DMPAPER_A5`).
    pub fn a5() -> Self {
        Self { paper_name: "A5".into(), width: 583, height: 827, raw_kind: 11 }
    }

    /// The standard sizes known without a printer (what a `PaperSize="…"` attribute names).
    pub fn standard() -> Vec<Self> {
        vec![Self::letter(), Self::legal(), Self::a4(), Self::a3(), Self::a5()]
    }

    /// The standard size named `name` (case-insensitive: `"a4"`, `"Letter"`).
    pub fn by_name(name: &str) -> Option<Self> {
        let n = name.trim();
        Self::standard().into_iter().find(|p| p.paper_name.eq_ignore_ascii_case(n))
    }
}

impl fmt::Display for PaperSize {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({:.2} x {:.2} in)", self.paper_name, self.width as f32 / 100.0, self.height as f32 / 100.0)
    }
}

/// A paper tray (WinForms `PaperSource`): its name and the spooler's `DMBIN_*` number.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PaperSource {
    pub source_name: String,
    pub raw_kind: i32,
}

/// A print resolution (WinForms `PrinterResolution`), in dots per inch. Negative values are the
/// driver's quality levels (`DMRES_HIGH` = -4 … `DMRES_DRAFT` = -1), as in the `DEVMODE`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PrinterResolution {
    pub x: i32,
    pub y: i32,
}

// ── Enums ────────────────────────────────────────────────────────────────────────────────────────

/// Which pages the user chose to print (WinForms `PrintRange`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum PrintRange {
    #[default]
    AllPages,
    /// `from_page` to `to_page` (1-based, inclusive). Pages outside are rendered (the handler's state
    /// advances) but not sent to the printer — WinForms leaves that to the handler.
    SomePages,
    /// The selection: what it is, the handler knows (`e.page_settings.printer_settings.print_range`).
    Selection,
    /// The page the user is on: the handler decides which (like WinForms).
    CurrentPage,
}

/// Two-sided printing (WinForms `Duplex`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum Duplex {
    /// The printer's setting.
    #[default]
    Default,
    Simplex,
    Vertical,
    Horizontal,
}

/// What a print job is doing (WinForms `PrintAction`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum PrintAction {
    /// Printing on a printer.
    #[default]
    PrintToPrinter,
    /// Printing into a file (`PrinterSettings::print_to_file`).
    PrintToFile,
    /// Rendering pages for a preview.
    PrintToPreview,
}

// ── PageSettings ─────────────────────────────────────────────────────────────────────────────────

/// How a page is printed (WinForms `PageSettings`): orientation, paper, tray, margins, colour,
/// resolution. `None` fields mean "the printer's default". [`PrintDocument`](crate::PrintDocument)
/// hands each page a copy resolved against the printer: its `paper_size` set, and the printer's
/// `hard_margin_x`/`hard_margin_y` and `printable_area` filled in.
#[derive(Debug, Clone, PartialEq)]
pub struct PageSettings {
    pub landscape: bool,
    pub margins: Margins,
    pub paper_size: Option<PaperSize>,
    pub paper_source: Option<PaperSource>,
    /// `Some(true)` colour, `Some(false)` monochrome, `None` the printer's default.
    pub color: Option<bool>,
    pub printer_resolution: Option<PrinterResolution>,
    /// The left edge the printer cannot print on, hundredths of an inch (read-only: filled when
    /// the settings are resolved against a printer).
    pub hard_margin_x: f32,
    /// The top edge the printer cannot print on, hundredths of an inch.
    pub hard_margin_y: f32,
    /// The area the printer can print on, hundredths of an inch, in the page's orientation.
    pub printable_area: Rect,
}

impl Default for PageSettings {
    fn default() -> Self {
        Self {
            landscape: false,
            margins: Margins::default(),
            paper_size: None,
            paper_source: None,
            color: None,
            printer_resolution: None,
            hard_margin_x: 0.0,
            hard_margin_y: 0.0,
            printable_area: Rect::new(0.0, 0.0, 0.0, 0.0),
        }
    }
}

impl PageSettings {
    pub fn new() -> Self {
        Self::default()
    }

    /// The paper size used: the chosen one, else US Letter until resolved against a printer.
    pub fn paper(&self) -> PaperSize {
        self.paper_size.clone().unwrap_or_else(PaperSize::letter)
    }

    /// The page's size in hundredths of an inch, in its orientation (`Bounds.Size`).
    pub fn size(&self) -> (f32, f32) {
        let p = self.paper();
        let (w, h) = (p.width as f32, p.height as f32);
        if self.landscape {
            (h, w)
        } else {
            (w, h)
        }
    }

    /// The page in hundredths of an inch (WinForms `PageSettings.Bounds`).
    pub fn bounds(&self) -> Rect {
        let (w, h) = self.size();
        Rect::new(0.0, 0.0, w, h)
    }

    /// The page inside its margins, hundredths of an inch (`MarginBounds`).
    pub fn margin_bounds(&self) -> Rect {
        let (w, h) = self.size();
        let m = self.margins;
        Rect::new(m.left as f32, m.top as f32, (w - m.right as f32).max(m.left as f32), (h - m.bottom as f32).max(m.top as f32))
    }
}

// ── PrinterSettings ──────────────────────────────────────────────────────────────────────────────

/// A printer and how to print on it (WinForms `PrinterSettings`): which printer, copies, collation,
/// two-sided printing, the page range the user chose, printing to a file — and what the printer can
/// do (`paper_sizes`, `paper_sources`, `printer_resolutions`, `can_duplex`, `supports_color`,
/// `maximum_copies`), asked of the spooler.
#[derive(Debug, Clone, PartialEq)]
pub struct PrinterSettings {
    /// The printer; empty = the default printer (resolved when printing).
    pub printer_name: String,
    pub copies: u16,
    pub collate: bool,
    pub duplex: Duplex,
    pub print_range: PrintRange,
    /// First and last page of [`PrintRange::SomePages`] (1-based).
    pub from_page: u32,
    pub to_page: u32,
    /// The page numbers the Print dialog offers.
    pub minimum_page: u32,
    pub maximum_page: u32,
    /// Print into a file instead of on paper: `print_file_name`, or a file chosen in a Save dialog
    /// when it is empty. With "Microsoft Print to PDF" the file is a PDF, with "Microsoft XPS
    /// Document Writer" an XPS document; other printers write their own language (PCL, PostScript…).
    pub print_to_file: bool,
    pub print_file_name: String,
    /// The driver's settings the Print / Page Setup dialogs returned (a `DEVMODE`), kept so the
    /// options of the driver's own property pages survive.
    pub(crate) devmode: Option<native::DevMode>,
}

impl Default for PrinterSettings {
    fn default() -> Self {
        Self {
            printer_name: String::new(),
            copies: 1,
            collate: false,
            duplex: Duplex::Default,
            print_range: PrintRange::AllPages,
            from_page: 0,
            to_page: 0,
            minimum_page: 0,
            maximum_page: 9999,
            print_to_file: false,
            print_file_name: String::new(),
            devmode: None,
        }
    }
}

impl PrinterSettings {
    pub fn new() -> Self {
        Self::default()
    }

    /// Settings for the printer named `name`.
    pub fn for_printer(name: impl Into<String>) -> Self {
        Self { printer_name: name.into(), ..Self::default() }
    }

    /// The printers installed for this user, local and network connections
    /// (`PrinterSettings.InstalledPrinters`).
    pub fn installed_printers() -> Vec<String> {
        native::installed_printers()
    }

    /// The user's default printer, if any.
    pub fn default_printer_name() -> Option<String> {
        native::default_printer()
    }

    /// The printer these settings print on: `printer_name`, else the default printer.
    pub fn resolved_printer_name(&self) -> Result<String, PrintError> {
        let name = self.printer_name.trim();
        if !name.is_empty() {
            return Ok(name.to_string());
        }
        native::default_printer().ok_or(PrintError::NoPrinter)
    }

    /// Whether `printer_name` (or the default printer) is installed (`IsValid`).
    pub fn is_valid(&self) -> bool {
        self.resolved_printer_name().is_ok_and(|name| native::installed_printers().iter().any(|p| p.eq_ignore_ascii_case(&name)))
    }

    /// Whether these settings print on the default printer (`IsDefaultPrinter`).
    pub fn is_default_printer(&self) -> bool {
        self.printer_name.trim().is_empty() || native::default_printer().is_some_and(|d| d.eq_ignore_ascii_case(self.printer_name.trim()))
    }

    /// The paper sizes the printer offers (`PaperSizes`).
    pub fn paper_sizes(&self) -> Vec<PaperSize> {
        self.resolved_printer_name().map(|n| native::paper_sizes(&n)).unwrap_or_default()
    }

    /// The paper trays of the printer (`PaperSources`).
    pub fn paper_sources(&self) -> Vec<PaperSource> {
        self.resolved_printer_name().map(|n| native::paper_sources(&n)).unwrap_or_default()
    }

    /// The resolutions the printer offers (`PrinterResolutions`).
    pub fn printer_resolutions(&self) -> Vec<PrinterResolution> {
        self.resolved_printer_name().map(|n| native::resolutions(&n)).unwrap_or_default()
    }

    /// Whether the printer prints on both sides (`CanDuplex`).
    pub fn can_duplex(&self) -> bool {
        self.resolved_printer_name().is_ok_and(|n| native::capability(&n, native::DC_DUPLEX) == 1)
    }

    /// Whether the printer prints in colour (`SupportsColor`).
    pub fn supports_color(&self) -> bool {
        self.resolved_printer_name().is_ok_and(|n| native::capability(&n, native::DC_COLORDEVICE) == 1)
    }

    /// The most copies the driver makes by itself (`MaximumCopies`).
    pub fn maximum_copies(&self) -> u32 {
        self.resolved_printer_name().map(|n| native::capability(&n, native::DC_COPIES).max(1) as u32).unwrap_or(1)
    }

    /// The printer's default page settings (`DefaultPageSettings`): its paper, orientation and
    /// colour, with one-inch margins.
    pub fn default_page_settings(&self) -> PageSettings {
        let mut page = PageSettings::default();
        if let Ok(name) = self.resolved_printer_name() {
            if let Ok(dm) = native::DevMode::for_printer(&name, self.devmode.as_ref()) {
                dm.read_page(&mut page, &native::paper_sizes(&name));
            }
        }
        page
    }
}
