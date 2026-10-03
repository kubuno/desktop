//! The flat Win32 printing APIs (winspool, gdi32), through `windows-sys`: the installed printers,
//! the driver's settings (`DEVMODE`), what a printer can do (`DeviceCapabilities`), the page's
//! physical metrics, and the spooler's jobs.

use std::ffi::c_void;
use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::{GlobalFree, HGLOBAL};
use windows_sys::Win32::Graphics::Gdi::{CreateICW, DeleteDC, GetDeviceCaps, DEVMODEW};
use windows_sys::Win32::Graphics::Printing::{
    ClosePrinter, DocumentPropertiesW, EnumJobsW, EnumPrintersW, GetDefaultPrinterW, OpenPrinterW, JOB_INFO_1W, PRINTER_HANDLE, PRINTER_INFO_4W,
};
use windows_sys::Win32::Storage::Xps::DeviceCapabilitiesW;
use windows_sys::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE};

use crate::settings::{tenths_mm_to_hundredths, hundredths_to_tenths_mm, Duplex, PageSettings, PaperSize, PaperSource, PrinterResolution, PrinterSettings};
use crate::PrintError;

// `DeviceCapabilities` indices (wingdi.h), as plain numbers.
pub(crate) const DC_PAPERS: u16 = 2;
pub(crate) const DC_PAPERSIZE: u16 = 3;
pub(crate) const DC_BINS: u16 = 6;
pub(crate) const DC_DUPLEX: u16 = 7;
pub(crate) const DC_BINNAMES: u16 = 12;
pub(crate) const DC_ENUMRESOLUTIONS: u16 = 13;
pub(crate) const DC_PAPERNAMES: u16 = 16;
pub(crate) const DC_COPIES: u16 = 18;
pub(crate) const DC_COLORDEVICE: u16 = 32;

// `DEVMODE.dmFields` bits.
const DM_ORIENTATION: u32 = 0x1;
const DM_PAPERSIZE: u32 = 0x2;
const DM_PAPERLENGTH: u32 = 0x4;
const DM_PAPERWIDTH: u32 = 0x8;
const DM_COPIES: u32 = 0x100;
const DM_DEFAULTSOURCE: u32 = 0x200;
const DM_PRINTQUALITY: u32 = 0x400;
const DM_COLOR: u32 = 0x800;
const DM_DUPLEX: u32 = 0x1000;
const DM_YRESOLUTION: u32 = 0x2000;
const DM_COLLATE: u32 = 0x8000;
// `DocumentProperties` modes.
const DM_MODE_OUT_BUFFER: u32 = 2;
const DM_MODE_IN_BUFFER: u32 = 8;
// `GetDeviceCaps` indices.
const HORZRES: i32 = 8;
const VERTRES: i32 = 10;
const LOGPIXELSX: i32 = 88;
const LOGPIXELSY: i32 = 90;
const PHYSICALWIDTH: i32 = 110;
const PHYSICALHEIGHT: i32 = 111;
const PHYSICALOFFSETX: i32 = 112;
const PHYSICALOFFSETY: i32 = 113;

const PRINTER_ENUM_LOCAL: u32 = 2;
const PRINTER_ENUM_CONNECTIONS: u32 = 4;

/// `s` as a NUL-terminated UTF-16 string.
pub(crate) fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// The UTF-16 string at `p` (NUL-terminated), or empty.
///
/// # Safety
/// `p` is null or points to a NUL-terminated UTF-16 string.
pub(crate) unsafe fn from_wide_ptr(p: *const u16) -> String {
    if p.is_null() {
        return String::new();
    }
    let mut len = 0;
    // SAFETY: the caller guarantees a NUL-terminated string.
    unsafe {
        while *p.add(len) != 0 {
            len += 1;
        }
        String::from_utf16_lossy(std::slice::from_raw_parts(p, len))
    }
}

/// A fixed-size UTF-16 field (`WCHAR name[N]`), up to its first NUL.
fn from_wide_field(field: &[u16]) -> String {
    let end = field.iter().position(|c| *c == 0).unwrap_or(field.len());
    String::from_utf16_lossy(&field[..end])
}

// ── Printers ─────────────────────────────────────────────────────────────────────────────────────

/// The printers of this user: local ones and network connections.
pub(crate) fn installed_printers() -> Vec<String> {
    let flags = PRINTER_ENUM_LOCAL | PRINTER_ENUM_CONNECTIONS;
    let mut needed = 0u32;
    let mut count = 0u32;
    // SAFETY: a size query (null buffer, 0 bytes); the out-parameters are valid.
    unsafe { EnumPrintersW(flags, null(), 4, null_mut(), 0, &mut needed, &mut count) };
    if needed == 0 {
        return Vec::new();
    }
    // `u64`s: the buffer holds pointer-aligned structs.
    let mut buffer = vec![0u64; (needed as usize).div_ceil(8)];
    // SAFETY: the buffer has `needed` bytes, as asked.
    let ok = unsafe { EnumPrintersW(flags, null(), 4, buffer.as_mut_ptr().cast(), needed, &mut needed, &mut count) };
    if ok == 0 {
        tracing::warn!(target: "kubuno_print", "EnumPrinters failed: {}", std::io::Error::last_os_error());
        return Vec::new();
    }
    let infos = buffer.as_ptr().cast::<PRINTER_INFO_4W>();
    (0..count as usize)
        // SAFETY: the spooler wrote `count` PRINTER_INFO_4W at the start of the buffer, their strings after.
        .map(|i| unsafe { from_wide_ptr((*infos.add(i)).pPrinterName) })
        .filter(|n| !n.is_empty())
        .collect()
}

/// The user's default printer.
pub(crate) fn default_printer() -> Option<String> {
    let mut len = 0u32;
    // SAFETY: a size query.
    unsafe { GetDefaultPrinterW(null_mut(), &mut len) };
    if len == 0 {
        return None;
    }
    let mut buffer = vec![0u16; len as usize];
    // SAFETY: `len` characters, as asked.
    let ok = unsafe { GetDefaultPrinterW(buffer.as_mut_ptr(), &mut len) };
    (ok != 0).then(|| from_wide_field(&buffer)).filter(|n| !n.is_empty())
}

/// An open printer (closed when dropped).
pub(crate) struct Printer(PRINTER_HANDLE);

impl Printer {
    pub(crate) fn open(name: &str) -> Result<Self, PrintError> {
        let w = wide(name);
        let mut handle = PRINTER_HANDLE { Value: null_mut() };
        // SAFETY: a NUL-terminated name, a valid out-parameter, no defaults.
        let ok = unsafe { OpenPrinterW(w.as_ptr(), &mut handle, null()) };
        if ok == 0 || handle.Value.is_null() {
            return Err(PrintError::InvalidPrinter(name.to_string()));
        }
        Ok(Self(handle))
    }

    /// The spooler's jobs of this printer: `(id, document name)`.
    pub(crate) fn jobs(&self) -> Vec<(u32, String)> {
        let mut needed = 0u32;
        let mut count = 0u32;
        // SAFETY: a size query.
        unsafe { EnumJobsW(self.0, 0, 1024, 1, null_mut(), 0, &mut needed, &mut count) };
        if needed == 0 {
            return Vec::new();
        }
        let mut buffer = vec![0u64; (needed as usize).div_ceil(8)];
        // SAFETY: the buffer has `needed` bytes.
        let ok = unsafe { EnumJobsW(self.0, 0, 1024, 1, buffer.as_mut_ptr().cast(), needed, &mut needed, &mut count) };
        if ok == 0 {
            return Vec::new();
        }
        let infos = buffer.as_ptr().cast::<JOB_INFO_1W>();
        (0..count as usize)
            // SAFETY: `count` JOB_INFO_1W at the start of the buffer.
            .map(|i| unsafe { ((*infos.add(i)).JobId, from_wide_ptr((*infos.add(i)).pDocument)) })
            .collect()
    }
}

impl Drop for Printer {
    fn drop(&mut self) {
        // SAFETY: a handle `OpenPrinterW` returned, closed once.
        unsafe { ClosePrinter(self.0) };
    }
}

// ── DEVMODE ──────────────────────────────────────────────────────────────────────────────────────

/// A driver's `DEVMODE` (its public part and the driver's private bytes after it), kept aligned.
#[derive(Clone, PartialEq)]
pub(crate) struct DevMode {
    words: Vec<u64>,
    len: usize,
}

impl std::fmt::Debug for DevMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "DevMode({} bytes, {:?})", self.len, from_wide_field(&self.dm().dmDeviceName))
    }
}

impl DevMode {
    fn zeroed(len: usize) -> Self {
        let len = len.max(std::mem::size_of::<DEVMODEW>());
        Self { words: vec![0u64; len.div_ceil(8)], len }
    }

    /// A copy of `bytes` (a `DEVMODE` read from a global memory block).
    pub(crate) fn from_bytes(bytes: &[u8]) -> Self {
        let mut dm = Self::zeroed(bytes.len());
        dm.bytes_mut()[..bytes.len()].copy_from_slice(bytes);
        dm.len = bytes.len().max(std::mem::size_of::<DEVMODEW>());
        dm
    }

    pub(crate) fn bytes(&self) -> &[u8] {
        // SAFETY: `words` holds at least `len` bytes (`zeroed`), plain data.
        unsafe { std::slice::from_raw_parts(self.words.as_ptr().cast::<u8>(), self.len) }
    }

    fn bytes_mut(&mut self) -> &mut [u8] {
        // SAFETY: as `bytes`, exclusively borrowed.
        unsafe { std::slice::from_raw_parts_mut(self.words.as_mut_ptr().cast::<u8>(), self.len) }
    }

    pub(crate) fn as_ptr(&self) -> *const DEVMODEW {
        self.words.as_ptr().cast()
    }

    fn dm(&self) -> &DEVMODEW {
        // SAFETY: at least `size_of::<DEVMODEW>()` bytes, 8-aligned (`u64` storage), plain data.
        unsafe { &*self.words.as_ptr().cast::<DEVMODEW>() }
    }

    fn dm_mut(&mut self) -> &mut DEVMODEW {
        // SAFETY: as `dm`, exclusively borrowed.
        unsafe { &mut *self.words.as_mut_ptr().cast::<DEVMODEW>() }
    }

    /// The printer's current settings, merged with `base` when given (the driver validates them).
    pub(crate) fn for_printer(name: &str, base: Option<&DevMode>) -> Result<Self, PrintError> {
        let _driver = driver_lock();
        let printer = Printer::open(name)?;
        let w = wide(name);
        // SAFETY: a size query (fMode 0) on an open printer.
        let size = unsafe { DocumentPropertiesW(null_mut(), printer.0, w.as_ptr(), null_mut(), null(), 0) };
        if size <= 0 {
            return Err(PrintError::Driver(format!("DocumentProperties({name}) failed")));
        }
        let mut out = Self::zeroed(size as usize);
        // A DEVMODE of another printer (a name changed after the dialog) would confuse the driver.
        let base = base.filter(|b| from_wide_field(&b.dm().dmDeviceName).eq_ignore_ascii_case(name.get(..name.len().min(31)).unwrap_or(name)));
        let (mode, input) = match base {
            Some(b) => (DM_MODE_OUT_BUFFER | DM_MODE_IN_BUFFER, b.as_ptr()),
            None => (DM_MODE_OUT_BUFFER, null()),
        };
        // SAFETY: `out` has the size the driver asked for; `input` is null or a DEVMODE of this printer.
        let r = unsafe { DocumentPropertiesW(null_mut(), printer.0, w.as_ptr(), out.words.as_mut_ptr().cast(), input, mode) };
        if r < 0 {
            return Err(PrintError::Driver(format!("DocumentProperties({name}) failed")));
        }
        Ok(out)
    }

    /// This DEVMODE with `page` and `printer` applied, validated by the driver of `name`.
    pub(crate) fn with_settings(&self, name: &str, page: &PageSettings, printer: &PrinterSettings) -> Result<Self, PrintError> {
        let mut dm = self.clone();
        {
            let d = dm.dm_mut();
            let mut fields = d.dmFields;
            // SAFETY: the printer arm of the union (a printer's DEVMODE).
            let p = unsafe { &mut d.Anonymous1.Anonymous1 };
            p.dmOrientation = if page.landscape { 2 } else { 1 };
            fields |= DM_ORIENTATION;
            if let Some(paper) = &page.paper_size {
                if paper.raw_kind > 0 {
                    p.dmPaperSize = paper.raw_kind as i16;
                    fields |= DM_PAPERSIZE;
                    fields &= !(DM_PAPERLENGTH | DM_PAPERWIDTH);
                } else {
                    p.dmPaperSize = 256; // DMPAPER_USER
                    p.dmPaperWidth = hundredths_to_tenths_mm(paper.width).clamp(1, i16::MAX as i32) as i16;
                    p.dmPaperLength = hundredths_to_tenths_mm(paper.height).clamp(1, i16::MAX as i32) as i16;
                    fields |= DM_PAPERSIZE | DM_PAPERLENGTH | DM_PAPERWIDTH;
                }
            }
            if let Some(source) = &page.paper_source {
                p.dmDefaultSource = source.raw_kind as i16;
                fields |= DM_DEFAULTSOURCE;
            }
            if let Some(res) = page.printer_resolution {
                p.dmPrintQuality = res.x as i16;
                fields |= DM_PRINTQUALITY;
                if res.y > 0 {
                    d.dmYResolution = res.y as i16;
                    fields |= DM_YRESOLUTION;
                }
            }
            p.dmCopies = printer.copies.max(1) as i16;
            fields |= DM_COPIES;
            d.dmCollate = if printer.collate { 1 } else { 0 };
            fields |= DM_COLLATE;
            if let Some(color) = page.color {
                d.dmColor = if color { 2 } else { 1 };
                fields |= DM_COLOR;
            }
            match printer.duplex {
                Duplex::Default => {}
                Duplex::Simplex => {
                    d.dmDuplex = 1;
                    fields |= DM_DUPLEX;
                }
                Duplex::Vertical => {
                    d.dmDuplex = 2;
                    fields |= DM_DUPLEX;
                }
                Duplex::Horizontal => {
                    d.dmDuplex = 3;
                    fields |= DM_DUPLEX;
                }
            }
            d.dmFields = fields;
        }
        Self::for_printer(name, Some(&dm))
    }

    /// Reads orientation, paper, tray, colour and resolution into `page` (`sizes`: the printer's
    /// papers, to name the paper).
    pub(crate) fn read_page(&self, page: &mut PageSettings, sizes: &[PaperSize]) {
        let d = self.dm();
        // SAFETY: the printer arm of the union.
        let p = unsafe { d.Anonymous1.Anonymous1 };
        if d.dmFields & DM_ORIENTATION != 0 {
            page.landscape = p.dmOrientation == 2;
        }
        if d.dmFields & DM_PAPERSIZE != 0 {
            let kind = p.dmPaperSize as i32;
            page.paper_size = sizes.iter().find(|s| s.raw_kind == kind).cloned().or_else(|| {
                (d.dmFields & (DM_PAPERLENGTH | DM_PAPERWIDTH) == DM_PAPERLENGTH | DM_PAPERWIDTH)
                    .then(|| PaperSize::custom("Custom", tenths_mm_to_hundredths(p.dmPaperWidth as i32), tenths_mm_to_hundredths(p.dmPaperLength as i32)))
            });
        }
        if d.dmFields & DM_COLOR != 0 {
            page.color = Some(d.dmColor == 2);
        }
        if d.dmFields & DM_DEFAULTSOURCE != 0 {
            page.paper_source = Some(PaperSource { source_name: String::new(), raw_kind: p.dmDefaultSource as i32 });
        }
        if d.dmFields & DM_PRINTQUALITY != 0 {
            let y = if d.dmFields & DM_YRESOLUTION != 0 { d.dmYResolution as i32 } else { p.dmPrintQuality as i32 };
            page.printer_resolution = Some(PrinterResolution { x: p.dmPrintQuality as i32, y });
        }
    }

    /// Reads copies, collation and two-sided printing into `printer`.
    pub(crate) fn read_printer(&self, printer: &mut PrinterSettings) {
        let d = self.dm();
        // SAFETY: the printer arm of the union.
        let p = unsafe { d.Anonymous1.Anonymous1 };
        if d.dmFields & DM_COPIES != 0 {
            printer.copies = p.dmCopies.max(1) as u16;
        }
        if d.dmFields & DM_COLLATE != 0 {
            printer.collate = d.dmCollate == 1;
        }
        if d.dmFields & DM_DUPLEX != 0 {
            printer.duplex = match d.dmDuplex {
                2 => Duplex::Vertical,
                3 => Duplex::Horizontal,
                _ => Duplex::Simplex,
            };
        }
    }

    /// A copy in a movable global memory block (what the common dialogs take), freed by the caller.
    pub(crate) fn to_hglobal(&self) -> HGLOBAL {
        let bytes = self.bytes();
        // SAFETY: a new block of the right size, locked, filled and unlocked.
        unsafe {
            let h = GlobalAlloc(GMEM_MOVEABLE, bytes.len());
            if h.is_null() {
                return null_mut();
            }
            let p = GlobalLock(h).cast::<u8>();
            if p.is_null() {
                GlobalFree(h);
                return null_mut();
            }
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), p, bytes.len());
            GlobalUnlock(h);
            h
        }
    }

    /// A copy of the DEVMODE in the global block `h` (a dialog's result).
    pub(crate) fn from_hglobal(h: HGLOBAL) -> Option<Self> {
        if h.is_null() {
            return None;
        }
        // SAFETY: a block the dialog allocated, locked while copied.
        unsafe {
            let size = GlobalSize(h);
            let p = GlobalLock(h).cast::<u8>();
            if p.is_null() || size < std::mem::size_of::<DEVMODEW>() {
                if !p.is_null() {
                    GlobalUnlock(h);
                }
                return None;
            }
            let dm = Self::from_bytes(std::slice::from_raw_parts(p, size));
            GlobalUnlock(h);
            Some(dm)
        }
    }
}

// ── Capabilities ─────────────────────────────────────────────────────────────────────────────────

/// `DeviceCapabilities(name, capability)` as a number (-1 when unknown).
pub(crate) fn capability(name: &str, capability: u16) -> i32 {
    let _driver = driver_lock();
    let w = wide(name);
    // SAFETY: a query without an output buffer.
    unsafe { DeviceCapabilitiesW(w.as_ptr(), null(), capability, null_mut(), null()) }
}

/// `DeviceCapabilities` filling an array of `count` items of `item` bytes each.
fn capability_array(name: &str, cap: u16, item: usize) -> Vec<u8> {
    let count = capability(name, cap);
    if count <= 0 {
        return Vec::new();
    }
    let _driver = driver_lock();
    let w = wide(name);
    let mut buffer = vec![0u64; (count as usize * item).div_ceil(8) + 1];
    // SAFETY: room for `count` items of `item` bytes, as the query said.
    let got = unsafe { DeviceCapabilitiesW(w.as_ptr(), null(), cap, buffer.as_mut_ptr().cast(), null()) };
    if got <= 0 {
        return Vec::new();
    }
    // SAFETY: plain bytes of the buffer, `got` items written.
    unsafe { std::slice::from_raw_parts(buffer.as_ptr().cast::<u8>(), got as usize * item) }.to_vec()
}

fn u16s(bytes: &[u8]) -> Vec<u16> {
    bytes.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect()
}

fn i32s(bytes: &[u8]) -> Vec<i32> {
    bytes.as_chunks::<4>().0.iter().map(|c| i32::from_le_bytes(*c)).collect()
}

/// The paper sizes of the printer (`DC_PAPERS`, `DC_PAPERNAMES`, `DC_PAPERSIZE`).
pub(crate) fn paper_sizes(name: &str) -> Vec<PaperSize> {
    let kinds = u16s(&capability_array(name, DC_PAPERS, 2));
    let names = u16s(&capability_array(name, DC_PAPERNAMES, 64 * 2));
    let sizes = i32s(&capability_array(name, DC_PAPERSIZE, 8));
    kinds
        .iter()
        .enumerate()
        .filter_map(|(i, kind)| {
            let (w, h) = (*sizes.get(i * 2)?, *sizes.get(i * 2 + 1)?);
            let label = names.get(i * 64..(i + 1) * 64).map(from_wide_field).unwrap_or_default();
            Some(PaperSize { paper_name: label, width: tenths_mm_to_hundredths(w), height: tenths_mm_to_hundredths(h), raw_kind: *kind as i32 })
        })
        .collect()
}

/// The paper trays of the printer (`DC_BINS`, `DC_BINNAMES`).
pub(crate) fn paper_sources(name: &str) -> Vec<PaperSource> {
    let kinds = u16s(&capability_array(name, DC_BINS, 2));
    let names = u16s(&capability_array(name, DC_BINNAMES, 24 * 2));
    kinds
        .iter()
        .enumerate()
        .map(|(i, kind)| PaperSource { source_name: names.get(i * 24..(i + 1) * 24).map(from_wide_field).unwrap_or_default(), raw_kind: *kind as i32 })
        .collect()
}

/// The resolutions of the printer (`DC_ENUMRESOLUTIONS`).
pub(crate) fn resolutions(name: &str) -> Vec<PrinterResolution> {
    i32s(&capability_array(name, DC_ENUMRESOLUTIONS, 8)).as_chunks::<2>().0.iter().map(|c| PrinterResolution { x: c[0], y: c[1] }).collect()
}

// ── Page metrics ─────────────────────────────────────────────────────────────────────────────────

/// A page as the printer sees it, in device pixels (`GetDeviceCaps` on an information context).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PageMetrics {
    pub dpi_x: f32,
    pub dpi_y: f32,
    pub physical_width: f32,
    pub physical_height: f32,
    pub offset_x: f32,
    pub offset_y: f32,
    pub printable_width: f32,
    pub printable_height: f32,
}

impl PageMetrics {
    /// A page of `width` × `height` hundredths of an inch without hard margins (at 600 dpi), for
    /// a printer that could not be asked.
    pub(crate) fn nominal(width: f32, height: f32) -> Self {
        let k = 6.0;
        Self { dpi_x: 600.0, dpi_y: 600.0, physical_width: width * k, physical_height: height * k, offset_x: 0.0, offset_y: 0.0, printable_width: width * k, printable_height: height * k }
    }

    /// The physical page in hundredths of an inch.
    pub(crate) fn page_hundredths(&self) -> (f32, f32) {
        (self.physical_width / self.dpi_x * 100.0, self.physical_height / self.dpi_y * 100.0)
    }
}

/// The metrics of a page printed with `devmode` on `name`.
pub(crate) fn page_metrics(name: &str, devmode: &DevMode) -> Result<PageMetrics, PrintError> {
    let _driver = driver_lock();
    let w = wide(name);
    // SAFETY: an information context for the printer with its DEVMODE; deleted below.
    let dc = unsafe { CreateICW(null(), w.as_ptr(), null(), devmode.as_ptr()) };
    if dc.is_null() {
        return Err(PrintError::InvalidPrinter(name.to_string()));
    }
    // SAFETY: plain queries on a live DC.
    let caps = |i: i32| unsafe { GetDeviceCaps(dc, i) } as f32;
    let m = PageMetrics {
        dpi_x: caps(LOGPIXELSX).max(1.0),
        dpi_y: caps(LOGPIXELSY).max(1.0),
        physical_width: caps(PHYSICALWIDTH),
        physical_height: caps(PHYSICALHEIGHT),
        offset_x: caps(PHYSICALOFFSETX),
        offset_y: caps(PHYSICALOFFSETY),
        printable_width: caps(HORZRES),
        printable_height: caps(VERTRES),
    };
    // SAFETY: the DC created above.
    unsafe { DeleteDC(dc) };
    if m.physical_width <= 0.0 || m.physical_height <= 0.0 {
        return Err(PrintError::Driver(format!("{name} reports an empty page")));
    }
    Ok(m)
}

/// The handle of the window that owns a native dialog: the active window of the thread.
pub(crate) fn owner_window() -> *mut c_void {
    // SAFETY: no arguments.
    unsafe { windows_sys::Win32::UI::Input::KeyboardAndMouse::GetActiveWindow() }
}

/// Whether the user's UI language is French (the texts of the preview dialog).
pub(crate) fn ui_is_french() -> bool {
    // SAFETY: no arguments.
    let lang = unsafe { windows_sys::Win32::Globalization::GetUserDefaultUILanguage() };
    lang & 0x3ff == 0x0c
}

/// Printer drivers are not all safe to call from several threads at once (found with the test
/// harness: two threads asking "Microsoft Print to PDF" for its settings at the same time crash the
/// process). Every call into a driver takes this process-wide lock.
pub(crate) fn driver_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}
