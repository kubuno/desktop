//! Images through WIC: decoding the document's `data:` images into Direct2D bitmaps (cached), and
//! preparing an image for insertion — decoded, shrunk to the core's insert plan
//! (`kubuno_docs_core::layout::images::plan_insert`: 1600 px long edge, PNG when the source has
//! transparency, JPEG otherwise) and re-encoded as a `data:` URI, refused past the per-image
//! budget (the whole document must stay under the server's 2 MiB PATCH limit).
//!
//! An image that is not a `data:` URI (a URL) is not fetched: it paints as the web paints an image
//! that has not loaded — a light grey box.

use std::cell::RefCell;
use std::collections::HashMap;

use kubuno_docs_core::base64;
use kubuno_docs_core::layout::images::{fits_insert_budget, plan_insert, Encoding};
use windows::core::{Interface, GUID};
use windows::Win32::Graphics::Direct2D::{ID2D1Bitmap1, ID2D1DeviceContext};
use windows::Win32::Graphics::Imaging::*;
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, IStream, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, STATSTG, STREAM_SEEK_SET, STATFLAG_NONAME};
use windows::Win32::UI::Shell::SHCreateMemStream;

fn wic() -> Option<IWICImagingFactory> {
    unsafe {
        // COM is normally already initialised on the UI thread; a second call is harmless.
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).ok()
    }
}

/// A decoded image: its first frame, its size and whether its pixel format carries alpha.
pub struct Decoded {
    frame: IWICBitmapFrameDecode,
    pub width: u32,
    pub height: u32,
    pub has_alpha: bool,
}

/// Decodes image bytes (PNG, JPEG, GIF, BMP, TIFF, ICO, WebP when the codec is installed).
pub fn decode(bytes: &[u8]) -> Option<Decoded> {
    let wic = wic()?;
    unsafe {
        let stream: IStream = SHCreateMemStream(Some(bytes))?;
        let decoder = wic.CreateDecoderFromStream(&stream, std::ptr::null(), WICDecodeMetadataCacheOnDemand).ok()?;
        let frame = decoder.GetFrame(0).ok()?;
        let (mut w, mut h) = (0u32, 0u32);
        frame.GetSize(&mut w, &mut h).ok()?;
        let fmt = frame.GetPixelFormat().ok()?;
        let alpha_formats: [GUID; 6] = [
            GUID_WICPixelFormat32bppBGRA,
            GUID_WICPixelFormat32bppRGBA,
            GUID_WICPixelFormat32bppPBGRA,
            GUID_WICPixelFormat32bppPRGBA,
            GUID_WICPixelFormat64bppRGBA,
            GUID_WICPixelFormat64bppBGRA,
        ];
        let container = decoder.GetContainerFormat().ok();
        // A palette PNG/GIF may carry transparency too: keep those lossless.
        let has_alpha = alpha_formats.contains(&fmt)
            || container == Some(GUID_ContainerFormatGif)
            || (container == Some(GUID_ContainerFormatPng) && fmt != GUID_WICPixelFormat24bppBGR && fmt != GUID_WICPixelFormat24bppRGB);
        Some(Decoded { frame, width: w, height: h, has_alpha })
    }
}

fn converted(wic: &IWICImagingFactory, source: &IWICBitmapSource, format: &GUID) -> Option<IWICFormatConverter> {
    unsafe {
        let c = wic.CreateFormatConverter().ok()?;
        c.Initialize(source, format, WICBitmapDitherTypeNone, None, 0.0, WICBitmapPaletteTypeCustom).ok()?;
        Some(c)
    }
}

/// A Direct2D bitmap of `bytes` for `ctx`.
pub fn to_bitmap(ctx: &ID2D1DeviceContext, bytes: &[u8]) -> Option<ID2D1Bitmap1> {
    let d = decode(bytes)?;
    let wic = wic()?;
    let src: IWICBitmapSource = d.frame.cast().ok()?;
    let conv = converted(&wic, &src, &GUID_WICPixelFormat32bppPBGRA)?;
    unsafe { ctx.CreateBitmapFromWicBitmap(&conv, None).ok() }
}

/// What a decoded image becomes once prepared for insertion.
pub struct Prepared {
    pub data_uri: String,
    pub width: u32,
    pub height: u32,
}

/// Decodes, shrinks and re-encodes an image for insertion. `Err` with a message the status bar
/// shows when the bytes are not an image or the result is still over the per-image budget.
pub fn prepare_insert(bytes: &[u8]) -> Result<Prepared, String> {
    let d = decode(bytes).ok_or_else(|| "image illisible".to_string())?;
    let plan = plan_insert(d.width, d.height, d.has_alpha);
    let wic = wic().ok_or_else(|| "WIC indisponible".to_string())?;
    unsafe {
        let src: IWICBitmapSource = d.frame.cast().map_err(|e| e.to_string())?;
        let scaled: IWICBitmapSource = if plan.resized {
            let s = wic.CreateBitmapScaler().map_err(|e| e.to_string())?;
            s.Initialize(&src, plan.width, plan.height, WICBitmapInterpolationModeHighQualityCubic).map_err(|e| e.to_string())?;
            s.cast().map_err(|e| e.to_string())?
        } else {
            src
        };
        let (container, mut pixel, mime) = match plan.encoding {
            Encoding::Png => (GUID_ContainerFormatPng, GUID_WICPixelFormat32bppBGRA, "image/png"),
            Encoding::Jpeg { .. } => (GUID_ContainerFormatJpeg, GUID_WICPixelFormat24bppBGR, "image/jpeg"),
        };
        let conv = converted(&wic, &scaled, &pixel).ok_or_else(|| "conversion impossible".to_string())?;
        let stream: IStream = SHCreateMemStream(None).ok_or_else(|| "mémoire insuffisante".to_string())?;
        let encoder = wic.CreateEncoder(&container, std::ptr::null()).map_err(|e| e.to_string())?;
        encoder.Initialize(&stream, WICBitmapEncoderNoCache).map_err(|e| e.to_string())?;
        let mut frame: Option<IWICBitmapFrameEncode> = None;
        let mut props: Option<windows::Win32::System::Com::StructuredStorage::IPropertyBag2> = None;
        encoder.CreateNewFrame(&mut frame, &mut props).map_err(|e| e.to_string())?;
        let frame = frame.ok_or_else(|| "encodeur sans image".to_string())?;
        frame.Initialize(props.as_ref()).map_err(|e| e.to_string())?;
        frame.SetSize(plan.width, plan.height).map_err(|e| e.to_string())?;
        frame.SetPixelFormat(&mut pixel).map_err(|e| e.to_string())?;
        let conv_src: IWICBitmapSource = conv.cast().map_err(|e| e.to_string())?;
        frame.WriteSource(&conv_src, std::ptr::null()).map_err(|e| e.to_string())?;
        frame.Commit().map_err(|e| e.to_string())?;
        encoder.Commit().map_err(|e| e.to_string())?;
        // Read the encoded bytes back out of the memory stream.
        let mut stat = STATSTG::default();
        stream.Stat(&mut stat, STATFLAG_NONAME).map_err(|e| e.to_string())?;
        let size = stat.cbSize as usize;
        stream.Seek(0, STREAM_SEEK_SET, None).map_err(|e| e.to_string())?;
        let mut out = vec![0u8; size];
        let mut read = 0u32;
        stream.Read(out.as_mut_ptr().cast(), size as u32, Some(&mut read)).ok().map_err(|e| e.to_string())?;
        out.truncate(read as usize);
        if !fits_insert_budget(out.len()) {
            return Err(format!("image trop lourde ({} Ko après réduction)", out.len() / 1024));
        }
        Ok(Prepared { data_uri: format!("data:{mime};base64,{}", base64::encode(&out)), width: plan.width, height: plan.height })
    }
}

/// The document's images as Direct2D bitmaps, decoded once per source and device context.
#[derive(Default)]
pub struct ImageCache {
    ctx: usize,
    map: RefCell<HashMap<u64, Option<ID2D1Bitmap1>>>,
}

fn key(src: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    src.hash(&mut h);
    h.finish()
}

impl ImageCache {
    /// The bitmap of `src` (`None` for a URL or undecodable bytes: the caller paints a placeholder).
    pub fn bitmap(&mut self, ctx: &ID2D1DeviceContext, src: &str) -> Option<ID2D1Bitmap1> {
        let id = ctx.as_raw() as usize;
        if id != self.ctx {
            self.ctx = id;
            self.map.borrow_mut().clear();
        }
        let k = key(src);
        if let Some(found) = self.map.borrow().get(&k) {
            return found.clone();
        }
        let bitmap = base64::data_uri(src).and_then(|(_, bytes)| to_bitmap(ctx, &bytes));
        self.map.borrow_mut().insert(k, bitmap.clone());
        bitmap
    }
}
