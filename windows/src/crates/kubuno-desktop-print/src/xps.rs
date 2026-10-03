//! Direct2D printing over an XPS print job: `IPrintDocumentPackageTargetFactory` creates the
//! spooler's job (optionally writing the printer's output into a file), `ID2D1PrintControl` turns each
//! page's Direct2D command list into an XPS page (vector: shapes, text as glyph runs with their fonts
//! embedded, bitmaps), and the job's print ticket carries the settings of the driver's `DEVMODE`
//! (paper, orientation, copies, collation, two-sided printing, colour, tray).
//!
//! The `windows` crate hides `IPrintDocumentPackageTargetFactory` and `ID2D1Device::CreatePrintControl`
//! behind features (`Win32_Storage_Xps_Printing`) that are not in `kubuno-desktop-ui`'s graph; enabling
//! them would rebuild `windows` and every crate built on it. They are declared here instead.

#![allow(non_snake_case)]

use std::ffi::c_void;
use std::path::Path;
use std::time::{Duration, Instant};

use windows::core::{Interface, HRESULT, HSTRING, PCWSTR};
use windows::Win32::Graphics::Direct2D::Common::D2D_SIZE_F;
use windows::Win32::Graphics::Direct2D::{
    ID2D1CommandList, ID2D1Device, ID2D1PrintControl, D2D1_COLOR_SPACE_SRGB, D2D1_PRINT_CONTROL_PROPERTIES, D2D1_PRINT_FONT_SUBSET_MODE_DEFAULT,
};
use windows::Win32::Graphics::Imaging::{CLSID_WICImagingFactory, IWICImagingFactory};
use windows::Win32::System::Com::StructuredStorage::CreateStreamOnHGlobal;
use windows::Win32::System::Com::{CoCreateInstance, IStream, CLSCTX_INPROC_SERVER, STGM_CREATE, STGM_SHARE_DENY_WRITE, STGM_WRITE, STREAM_SEEK_SET};
use windows::Win32::UI::Shell::SHCreateStreamOnFileEx;
use windows_core::{IUnknown, IUnknown_Vtbl, GUID};

use crate::native::{self, DevMode};
use crate::PrintError;

/// `IPrintDocumentPackageTargetFactory` (documenttarget.h).
#[windows_core::interface("d2959bf7-b31b-4a3d-9600-712eb1335ba4")]
unsafe trait IPrintDocumentPackageTargetFactory: IUnknown {
    fn CreateDocumentPackageTargetForPrintJob(&self, printer: PCWSTR, job: PCWSTR, output: *mut c_void, ticket: *mut c_void, target: *mut *mut c_void) -> HRESULT;
}

/// `IPrintDocumentPackageTarget` (documenttarget.h).
#[windows_core::interface("1b8efec4-3019-4c27-964e-367202156906")]
unsafe trait IPrintDocumentPackageTarget: IUnknown {
    fn GetPackageTargetTypes(&self, count: *mut u32, types: *mut *mut GUID) -> HRESULT;
    fn GetPackageTarget(&self, guid: *const GUID, riid: *const GUID, object: *mut *mut c_void) -> HRESULT;
    fn Cancel(&self) -> HRESULT;
}

/// `CLSID_PrintDocumentPackageTargetFactory`.
const CLSID_PRINT_DOCUMENT_PACKAGE_TARGET_FACTORY: GUID = GUID::from_u128(0x348ef17d_6c81_4982_92b4_ee188a43867a);

/// `ID2D1Device::CreatePrintControl` is the sixth entry of the device's vtable: IUnknown's three,
/// `ID2D1Resource::GetFactory`, `CreateDeviceContext`, then it (d2d1_1.h).
const CREATE_PRINT_CONTROL_SLOT: usize = 5;

type CreatePrintControlFn = unsafe extern "system" fn(*mut c_void, *mut c_void, *mut c_void, *const D2D1_PRINT_CONTROL_PROPERTIES, *mut *mut c_void) -> HRESULT;

fn create_print_control(device: &ID2D1Device, wic: &IWICImagingFactory, target: &IPrintDocumentPackageTarget, raster_dpi: f32) -> windows::core::Result<ID2D1PrintControl> {
    let props = D2D1_PRINT_CONTROL_PROPERTIES { fontSubset: D2D1_PRINT_FONT_SUBSET_MODE_DEFAULT, rasterDPI: raster_dpi, colorSpace: D2D1_COLOR_SPACE_SRGB };
    let this = device.as_raw();
    // SAFETY: `this` is a live `ID2D1Device`; its vtable is a table of function pointers whose entry
    // `CREATE_PRINT_CONTROL_SLOT` is `CreatePrintControl` with the signature above (d2d1_1.h). The
    // arguments are live interface pointers borrowed for the call and a valid out-pointer, whose
    // reference is taken over by `from_raw`.
    unsafe {
        let vtable = *(this as *const *const usize);
        let f: CreatePrintControlFn = std::mem::transmute::<usize, CreatePrintControlFn>(*vtable.add(CREATE_PRINT_CONTROL_SLOT));
        let mut out: *mut c_void = std::ptr::null_mut();
        f(this, wic.as_raw(), target.as_raw(), &props, &mut out).ok()?;
        if out.is_null() {
            return Err(windows::core::Error::from_hresult(windows::Win32::Foundation::E_POINTER));
        }
        Ok(ID2D1PrintControl::from_raw(out))
    }
}

/// A print ticket (XML, in a memory stream at its start) for `devmode` on `printer`.
/// `page_scope`: a page's own ticket (orientation, paper) rather than the job's.
pub(crate) fn ticket_stream(printer: &str, devmode: &DevMode, page_scope: bool) -> Result<IStream, PrintError> {
    use windows_sys::Win32::Graphics::Printing::PrintTicket::{PTCloseProvider, PTConvertDevModeToPrintTicket, PTOpenProvider, HPTPROVIDER};
    let _driver = native::driver_lock();
    let name = native::wide(printer);
    let mut provider: HPTPROVIDER = std::ptr::null_mut();
    // SAFETY: a NUL-terminated printer name and a valid out-parameter; closed below.
    let hr = unsafe { PTOpenProvider(name.as_ptr(), 1, &mut provider) };
    if hr < 0 || provider.is_null() {
        return Err(PrintError::Driver(format!("no print ticket provider for {printer} (0x{hr:08X})")));
    }
    let result = (|| {
        // SAFETY: a new stream over a block it owns.
        let stream = unsafe { CreateStreamOnHGlobal(Default::default(), true) }.map_err(PrintError::com("CreateStreamOnHGlobal"))?;
        let scope = if page_scope { 0 } else { 2 }; // kPTPageScope, kPTJobScope
        // SAFETY: an open provider, the DEVMODE's bytes (the W form: prntvpt's metadata names the A
        // struct, the function reads the size from the buffer), and a live stream it writes to.
        let hr = unsafe { PTConvertDevModeToPrintTicket(provider, devmode.bytes().len() as u32, devmode.as_ptr().cast(), scope, stream.as_raw()) };
        if hr < 0 {
            return Err(PrintError::Driver(format!("the settings of {printer} could not be converted to a print ticket (0x{hr:08X})")));
        }
        // SAFETY: a plain seek on the stream just written.
        unsafe { stream.Seek(0, STREAM_SEEK_SET, None) }.map_err(PrintError::com("IStream::Seek"))?;
        Ok(stream)
    })();
    // SAFETY: the provider opened above.
    unsafe { PTCloseProvider(provider) };
    result
}

/// One print job being written: pages are added as they are drawn, then the job is closed.
pub(crate) struct SpoolJob {
    control: ID2D1PrintControl,
    target: IPrintDocumentPackageTarget,
    printer: String,
    job_name: String,
    /// The file the printer's output goes to (print to file), kept open until the job is done.
    output: Option<(IStream, std::path::PathBuf)>,
    /// The spooler's job ids with this name before the job started (to find ours).
    before: Vec<u32>,
    pages: usize,
}

impl SpoolJob {
    /// Starts a job named `job_name` on `printer` with the job ticket of `devmode`, printed into
    /// `output` when given. `device`: the Direct2D device the pages are recorded on.
    pub(crate) fn start(device: &ID2D1Device, printer: &str, job_name: &str, devmode: &DevMode, output: Option<&Path>, raster_dpi: f32) -> Result<Self, PrintError> {
        let ticket = ticket_stream(printer, devmode, false)?;
        let before = native::Printer::open(printer).map(|p| p.jobs().into_iter().filter(|(_, n)| n == job_name).map(|(id, _)| id).collect()).unwrap_or_default();
        // SAFETY: COM calls on this thread's apartment (the UI thread has one: the host's OLE
        // initialisation); every pointer passed is live for the call.
        unsafe {
            let factory: IPrintDocumentPackageTargetFactory =
                CoCreateInstance(&CLSID_PRINT_DOCUMENT_PACKAGE_TARGET_FACTORY, None, CLSCTX_INPROC_SERVER).map_err(PrintError::com("PrintDocumentPackageTargetFactory"))?;
            let output = match output {
                Some(path) => {
                    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
                        std::fs::create_dir_all(dir).map_err(|e| PrintError::Output(format!("{}: {e}", path.display())))?;
                    }
                    let stream = SHCreateStreamOnFileEx(&HSTRING::from(path.as_os_str()), (STGM_CREATE | STGM_WRITE | STGM_SHARE_DENY_WRITE).0, 0, true, None)
                        .map_err(|e| PrintError::Output(format!("{}: {e}", path.display())))?;
                    Some((stream, path.to_path_buf()))
                }
                None => None,
            };
            let printer_w = HSTRING::from(printer);
            let job_w = HSTRING::from(job_name);
            let mut target: *mut c_void = std::ptr::null_mut();
            factory
                .CreateDocumentPackageTargetForPrintJob(
                    PCWSTR(printer_w.as_ptr()),
                    PCWSTR(job_w.as_ptr()),
                    output.as_ref().map_or(std::ptr::null_mut(), |(s, _)| s.as_raw()),
                    ticket.as_raw(),
                    &mut target,
                )
                .ok()
                .map_err(|e| PrintError::Spooler(format!("{printer}: {e}")))?;
            if target.is_null() {
                return Err(PrintError::Spooler(format!("{printer}: no print job")));
            }
            let target = IPrintDocumentPackageTarget::from_raw(target);
            let wic: IWICImagingFactory = CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).map_err(PrintError::com("WICImagingFactory"))?;
            let control = create_print_control(device, &wic, &target, raster_dpi).map_err(PrintError::com("ID2D1Device::CreatePrintControl"))?;
            Ok(Self { control, target, printer: printer.to_string(), job_name: job_name.to_string(), output, before, pages: 0 })
        }
    }

    /// Adds a page (`size` in DIP) with its own ticket when its settings differ from the job's.
    pub(crate) fn add_page(&mut self, list: &ID2D1CommandList, size: (f32, f32), page_ticket: Option<&IStream>) -> Result<(), PrintError> {
        // SAFETY: a closed command list of the print control's device, and a live stream or none.
        unsafe { self.control.AddPage(list, D2D_SIZE_F { width: size.0, height: size.1 }, page_ticket, None, None) }.map_err(PrintError::com("ID2D1PrintControl::AddPage"))?;
        self.pages += 1;
        Ok(())
    }

    /// Cancels the job (nothing is printed).
    pub(crate) fn cancel(self) {
        // SAFETY: plain calls on live objects; errors are irrelevant (the job goes away either way).
        unsafe {
            let _ = self.target.Cancel();
            let _ = self.control.Close();
        }
        if let Some((_, path)) = &self.output {
            let _ = std::fs::remove_file(path);
        }
    }

    /// Ends the job: the pages go to the spooler. When printing into a file, waits (up to `timeout`)
    /// until the spooler has written it — the file is complete when this returns.
    pub(crate) fn finish(self, timeout: Duration) -> Result<usize, PrintError> {
        // SAFETY: a plain call on the live print control.
        unsafe { self.control.Close() }.map_err(PrintError::com("ID2D1PrintControl::Close"))?;
        let pages = self.pages;
        let Some((stream, path)) = self.output else { return Ok(pages) };
        drop(self.control);
        drop(self.target);
        wait_for_job(&self.printer, &self.job_name, &self.before, timeout);
        // SAFETY: flushes what the stream buffered; the file is closed when it is released below.
        let _ = unsafe { stream.Commit(windows::Win32::System::Com::STGC_DEFAULT) };
        drop(stream);
        match std::fs::metadata(&path) {
            Ok(m) if m.len() > 0 => Ok(pages),
            _ => Err(PrintError::Output(format!("{}: the printer wrote nothing", path.display()))),
        }
    }
}

/// Waits until the spooler no longer has the job named `job_name` that was not there before
/// (`before`), or `timeout`. A job that never shows up (already done) ends the wait at once.
fn wait_for_job(printer: &str, job_name: &str, before: &[u32], timeout: Duration) {
    let Ok(handle) = native::Printer::open(printer) else { return };
    let start = Instant::now();
    let mut seen = false;
    loop {
        let ours = handle.jobs().into_iter().any(|(id, n)| n == job_name && !before.contains(&id));
        if ours {
            seen = true;
        } else if seen || start.elapsed() > Duration::from_millis(1500) {
            return;
        }
        if start.elapsed() > timeout {
            tracing::warn!(target: "kubuno_desktop_print", printer, job_name, "the print job did not complete in {:?}", timeout);
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}
