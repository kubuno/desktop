//! Icons drawn from image files, and any icon rasterized off screen.
//!
//! A control's icon is painted through `Canvas::vector_icon(name, rect, size, colour)`; `name` is
//! a glyph of the embedded vector set or, as [`kubuno_drive_desktop_app_controls::icon_source`] describes, an
//! image file followed or not by drawing options. The host painter draws the glyphs itself and
//! hands the rest to [`draw`], which renders the icon at the exact pixel size it covers on screen
//! (DPI-aware: an SVG is rendered as vectors at that size, a raster image is resampled with a
//! high-quality filter, an `.ico` gives the frame closest to that size) and keeps the result per
//! device.
//!
//! * **SVG** — Direct2D's own SVG renderer (`ID2D1DeviceContext5`, Windows 10 1703+). The icon's
//!   colour is the document's `currentColor`, so an SVG drawn with `currentColor` (Lucide's) follows
//!   the control's colour and the theme; any other colour is kept.
//! * **PNG, JPEG, BMP, GIF (first frame), ICO, TIFF, WebP** — Windows Imaging Component (WebP
//!   needs the system's WebP codec, present on Windows 10 1809+ and Windows 11).
//!
//! Options (`tint`, `size`, `scaling`, `mirror`): a tint recolours every pixel, its transparency
//! kept (a monochrome icon in another colour); `scaling` places a non-square image in its square
//! box; `mirror` flips it for a right-to-left layout. A tint names a colour (`#rrggbb[aa]`) or a
//! theme colour, which [`set_tint_resolver`] (installed by `kubuno_desktop_views`) resolves.
//!
//! [`rasterize`] is the same rendering into memory: the window's icon (task bar, Alt+Tab) and the
//! designer's previews use it.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::OnceLock;
use std::time::{Duration, Instant, SystemTime};

use kubuno_drive_desktop_app_controls::icon_source::{self, IconScaling, IconSpec};
use kubuno_drive_desktop_app_controls::Theme;
use windows::core::{w, Interface};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_PIXEL_FORMAT, D2D_RECT_F, D2D_SIZE_F, D2D_SIZE_U,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1CreateFactory, ID2D1Bitmap1, ID2D1DeviceContext, ID2D1DeviceContext5, ID2D1Factory, ID2D1Factory1, ID2D1RenderTarget,
    D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, D2D1_BITMAP_OPTIONS_NONE, D2D1_BITMAP_PROPERTIES1, D2D1_FACTORY_TYPE_SINGLE_THREADED,
    D2D1_FEATURE_LEVEL_DEFAULT, D2D1_INTERPOLATION_MODE_HIGH_QUALITY_CUBIC, D2D1_INTERPOLATION_MODE_LINEAR, D2D1_RENDER_TARGET_PROPERTIES,
    D2D1_RENDER_TARGET_TYPE_SOFTWARE, D2D1_RENDER_TARGET_USAGE_NONE, D2D1_SVG_ASPECT_ALIGN_NONE, D2D1_SVG_ASPECT_SCALING_MEET,
    D2D1_SVG_ATTRIBUTE_POD_TYPE_COLOR, D2D1_SVG_ATTRIBUTE_POD_TYPE_LENGTH, D2D1_SVG_ATTRIBUTE_POD_TYPE_PRESERVE_ASPECT_RATIO,
    D2D1_SVG_ATTRIBUTE_POD_TYPE_VIEWBOX, D2D1_SVG_LENGTH, D2D1_SVG_LENGTH_UNITS_NUMBER, D2D1_SVG_LENGTH_UNITS_PERCENTAGE,
    D2D1_SVG_PRESERVE_ASPECT_RATIO, D2D1_SVG_VIEWBOX, ID2D1SvgElement,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_WICPixelFormat32bppPBGRA, IWICImagingFactory, WICBitmapCacheOnLoad,
    WICBitmapDitherTypeNone, WICBitmapPaletteTypeCustom, WICDecodeMetadataCacheOnDemand,
};
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};

/// An icon in memory: `width` × `height` pixels, premultiplied BGRA, top-down rows.
#[derive(Debug, Clone, PartialEq)]
pub struct Raster {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

impl Raster {
    /// The pixels with straight (not premultiplied) alpha — what a Windows icon or a WPF
    /// `Bgra32` bitmap takes.
    pub fn straight_alpha(&self) -> Vec<u8> {
        let mut out = self.pixels.clone();
        for px in out.as_chunks_mut::<4>().0 {
            let a = px[3];
            if a != 0 && a != 255 {
                for c in &mut px[..3] {
                    *c = ((u32::from(*c) * 255 + u32::from(a) / 2) / u32::from(a)).min(255) as u8;
                }
            }
        }
        out
    }
}

// ── Tint ───────────────────────────────────────────────────────────────────────────────────────

static TINT_RESOLVER: OnceLock<fn(&str, &Theme) -> Option<D2D1_COLOR_F>> = OnceLock::new();

/// Installs how a tint that names a theme colour (`Accent`, `TextSecondary`…) is resolved against a
/// theme — `kubuno_desktop_views` installs its colour tokens. The first call wins.
pub fn set_tint_resolver(resolver: fn(&str, &Theme) -> Option<D2D1_COLOR_F>) {
    let _ = TINT_RESOLVER.set(resolver);
}

/// The colour of tint `text` in `theme`: `#rrggbb[aa]`, else a theme colour through the resolver.
pub fn tint_color(text: &str, theme: Option<&Theme>) -> Option<D2D1_COLOR_F> {
    if let Some([r, g, b, a]) = icon_source::parse_hex_color(text) {
        return Some(D2D1_COLOR_F { r, g, b, a });
    }
    let theme = theme?;
    TINT_RESOLVER.get().and_then(|resolve| resolve(text, theme))
}

// ── Decoded sources ────────────────────────────────────────────────────────────────────────────

/// An image file read once: the SVG text, or the raster frames converted to premultiplied BGRA
/// (every frame of an `.ico`, the first of anything else), largest last.
enum Source {
    Svg(Vec<u8>),
    Raster(Vec<Frame>),
}

/// A decoded frame: premultiplied BGRA rows.
struct Frame {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

struct Cached {
    source: Option<Rc<Source>>,
    modified: Option<SystemTime>,
    checked: Instant,
}

/// How often a file already read is checked for a change (the designer shows an edited image).
const RECHECK: Duration = Duration::from_secs(2);

/// (device, icon value, pixel width, pixel height, colour as RGBA).
type BitmapKey = (usize, usize, u32, u32, u32);
/// Resolves a resource image (`kbres:…`) to its bytes.
type ResourceLoader = fn(&str) -> Option<Vec<u8>>;

thread_local! {
    static SOURCES: RefCell<HashMap<String, Cached>> = RefCell::new(HashMap::new());
    /// Device bitmaps: (device, icon value, pixel size, colour) → the rendered icon.
    static BITMAPS: RefCell<HashMap<BitmapKey, Option<ID2D1Bitmap1>>> = RefCell::new(HashMap::new());
    /// The software factory, the WIC factory and the glyph geometries of [`rasterize`].
    static OFFSCREEN: RefCell<Option<Offscreen>> = const { RefCell::new(None) };
}

struct Offscreen {
    factory: ID2D1Factory1,
    wic: IWICImagingFactory,
    glyphs: kubuno_drive_desktop_app_controls::VectorIcons,
}

fn with_offscreen<R>(f: impl FnOnce(&mut Offscreen) -> Option<R>) -> Option<R> {
    OFFSCREEN.with(|slot| {
        let mut slot = slot.try_borrow_mut().ok()?;
        if slot.is_none() {
            // SAFETY: plain factory creation; COM is initialized on any thread that paints or asks
            // for an icon (the host's UI thread, the language server's request thread).
            let made = unsafe {
                let factory: windows::core::Result<ID2D1Factory1> = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None);
                let wic: windows::core::Result<IWICImagingFactory> = CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER);
                factory.and_then(|factory| wic.map(|wic| Offscreen { factory, wic, glyphs: Default::default() }))
            };
            match made {
                Ok(o) => *slot = Some(o),
                Err(error) => {
                    tracing::warn!("icon rendering is not available: {error}");
                    return None;
                }
            }
        }
        f(slot.as_mut()?)
    })
}

fn modified(path: &str) -> Option<SystemTime> {
    if icon_source::is_resource(path) {
        return None;
    }
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

static RESOURCE_LOADER: OnceLock<ResourceLoader> = OnceLock::new();

/// Installs where the bytes of a resource image (`kbres:<set>/<name>`, a `{Res key}` value at run
/// time) come from — the resources runtime installs it. The first call wins. Call [`release`] when
/// the resources change (another culture), so they are read again.
pub fn set_resource_loader(loader: ResourceLoader) {
    let _ = RESOURCE_LOADER.set(loader);
}

/// The decoded image at `path` — a file, or a resource (`kbres:…`) — cached; a file is read again
/// when it changed on disk.
fn source(path: &str, wic: &IWICImagingFactory) -> Option<Rc<Source>> {
    let now = Instant::now();
    let fresh = SOURCES.with(|m| {
        let mut m = m.borrow_mut();
        let entry = m.get_mut(path)?;
        if icon_source::is_resource(path) || now.duration_since(entry.checked) < RECHECK {
            return Some(entry.source.clone());
        }
        entry.checked = now;
        (modified(path) == entry.modified).then(|| entry.source.clone())
    });
    if let Some(found) = fresh {
        return found;
    }
    let bytes = if icon_source::is_resource(path) {
        RESOURCE_LOADER.get().and_then(|load| load(path)).ok_or_else(|| "no resources runtime knows it".to_string())
    } else {
        std::fs::read(path).map_err(|e| e.to_string())
    };
    let decoded = match bytes.and_then(|b| decode_bytes(&b, wic).map_err(|e| e.to_string())) {
        Ok(s) => Some(Rc::new(s)),
        Err(error) => {
            tracing::warn!("icon image {path} could not be read: {error}");
            None
        }
    };
    SOURCES.with(|m| m.borrow_mut().insert(path.to_string(), Cached { source: decoded.clone(), modified: modified(path), checked: now }));
    decoded
}

/// Whether `bytes` are an SVG document (text starting with `<`, with an `<svg` element early on).
pub fn is_svg(bytes: &[u8]) -> bool {
    let head = &bytes[..bytes.len().min(4096)];
    let text = String::from_utf8_lossy(head);
    let t = text.trim_start_matches('\u{feff}').trim_start();
    t.starts_with('<') && t.contains("<svg")
}

/// Decodes an image held in memory: an SVG document, or anything WIC reads (every frame of an
/// icon file, the first of anything else — a GIF's first frame, a TIFF's first page) as
/// premultiplied BGRA pixels.
///
/// The decoding runs on a short-lived worker thread of the multithreaded apartment while the
/// caller waits: some codecs (the WebP one, installed as a Store extension) make cross-apartment
/// COM calls, and from the UI thread those pump window messages in the middle of a paint — a
/// `WM_SIZE` then resizes the swap chain while it is being drawn on (`DXGI_ERROR_INVALID_CALL`).
/// A plain thread join does not pump.
fn decode_bytes(bytes: &[u8], _wic: &IWICImagingFactory) -> windows::core::Result<Source> {
    if is_svg(bytes) {
        return Ok(Source::Svg(bytes.to_vec()));
    }
    let decoded = std::thread::scope(|scope| scope.spawn(|| decode_raster(bytes)).join());
    match decoded {
        Ok(result) => result.map(Source::Raster),
        Err(_) => Err(windows::core::Error::new(windows::core::HRESULT(0x8000_FFFF_u32 as i32), "the image decoder failed")),
    }
}

/// [`decode_bytes`]' worker: the frames, smallest first.
fn decode_raster(bytes: &[u8]) -> windows::core::Result<Vec<Frame>> {
    use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED};
    // SAFETY: COM for this worker thread only, released before it ends; plain WIC calls on objects
    // created here, every one released before `CoUninitialize`; `bytes` outlives the stream.
    unsafe {
        let initialized = CoInitializeEx(None, COINIT_MULTITHREADED).is_ok();
        let result = (|| {
            let wic: IWICImagingFactory = CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?;
            let stream = wic.CreateStream()?;
            stream.InitializeFromMemory(bytes)?;
            let decoder = wic.CreateDecoderFromStream(&stream, std::ptr::null(), WICDecodeMetadataCacheOnDemand)?;
            let count = decoder.GetFrameCount()?.max(1);
            let is_icon = decoder.GetContainerFormat().is_ok_and(|f| f == windows::Win32::Graphics::Imaging::GUID_ContainerFormatIco);
            let frames = if is_icon { count } else { 1 };
            let mut out = Vec::new();
            for i in 0..frames {
                let frame = decoder.GetFrame(i)?;
                let converter = wic.CreateFormatConverter()?;
                converter.Initialize(&frame, &GUID_WICPixelFormat32bppPBGRA, WICBitmapDitherTypeNone, None, 0.0, WICBitmapPaletteTypeCustom)?;
                let (mut w, mut h) = (0, 0);
                converter.GetSize(&mut w, &mut h)?;
                let mut pixels = vec![0u8; w as usize * h as usize * 4];
                converter.CopyPixels(std::ptr::null(), w * 4, &mut pixels)?;
                out.push(Frame { width: w, height: h, pixels });
            }
            out.sort_by_key(|f| u64::from(f.width) * u64::from(f.height));
            Ok(out)
        })();
        if initialized {
            CoUninitialize();
        }
        result
    }
}

/// An SVG document held in memory as a bitmap of `renderer`'s device at its intrinsic size (its
/// `viewBox`, else `width`/`height`) — what an image property (`Image`, `BackgroundImage`) shows
/// for an SVG resource. `currentColor` is the theme's text colour.
pub fn decode_svg_bytes(renderer: &kubuno_drive_desktop_app_controls::Renderer, bytes: &[u8]) -> windows::core::Result<ID2D1Bitmap1> {
    let fail = || windows::core::Error::new(windows::core::HRESULT(0x8007_000D_u32 as i32), "not an SVG document Direct2D can render");
    let (w, h) = svg_intrinsic_size(bytes).ok_or_else(fail)?;
    let source = Source::Svg(bytes.to_vec());
    let ink = kubuno_drive_desktop_app_controls::Theme::light().text_primary;
    let raster = with_offscreen(|o| render_source(o, Some(&source), &IconSpec::default(), w.ceil() as u32, h.ceil() as u32, ink, None)).ok_or_else(fail)?;
    upload(&renderer.d2d_context, &raster).ok_or_else(fail)
}

/// The frame to draw in a box of `want` pixels: the smallest at least that large, else the largest.
fn best_frame(frames: &[Frame], want: u32) -> Option<&Frame> {
    frames.iter().find(|f| f.width.max(f.height) >= want).or_else(|| frames.last())
}

/// The intrinsic size of image file `path`, in pixels (an SVG's `viewBox` or `width`/`height`);
/// `None` when it cannot be read.
pub fn image_size(path: &str) -> Option<(f32, f32)> {
    with_offscreen(|o| match &*source(path, &o.wic)? {
        Source::Svg(bytes) => svg_intrinsic_size(bytes),
        Source::Raster(frames) => frames.last().map(|f| (f.width as f32, f.height as f32)),
    })
}

/// Whether image `path` is an icon file with several sizes (an `.ico`, by its content: a resource has
/// no extension).
pub fn has_several_sizes(path: &str) -> bool {
    with_offscreen(|o| match &*source(path, &o.wic)? {
        Source::Raster(frames) => Some(frames.len() > 1),
        Source::Svg(_) => Some(false),
    })
    .unwrap_or(false)
}

/// An SVG's `viewBox` size, else its `width`/`height` (24 × 24 when it states neither). Pure.
pub fn svg_intrinsic_size(bytes: &[u8]) -> Option<(f32, f32)> {
    let text = std::str::from_utf8(bytes).ok()?;
    let start = text.find("<svg")?;
    let tag = &text[start..start + text[start..].find('>')?];
    let attr = |name: &str| -> Option<&str> {
        let mut rest = tag;
        loop {
            let i = rest.find(name)?;
            let before = rest[..i].chars().last();
            rest = &rest[i + name.len()..];
            let trimmed = rest.trim_start();
            if before.is_some_and(char::is_whitespace) && trimmed.starts_with('=') {
                let value = trimmed[1..].trim_start();
                let quote = value.chars().next()?;
                let value = &value[1..];
                return Some(&value[..value.find(quote)?]);
            }
        }
    };
    let number = |v: &str| -> Option<f32> {
        let v = v.trim();
        if v.ends_with('%') {
            return None;
        }
        v.trim_end_matches("px").trim().parse::<f32>().ok().filter(|n| *n > 0.0)
    };
    if let Some(vb) = attr("viewBox") {
        let n: Vec<f32> = vb.split([' ', ',']).filter(|s| !s.is_empty()).filter_map(|s| s.parse().ok()).collect();
        if n.len() == 4 && n[2] > 0.0 && n[3] > 0.0 {
            return Some((n[2], n[3]));
        }
    }
    Some((attr("width").and_then(number).unwrap_or(24.0), attr("height").and_then(number).unwrap_or(24.0)))
}

// ── Rendering ──────────────────────────────────────────────────────────────────────────────────

/// Renders icon `value` (a glyph name or an image file, with its options) into `width` × `height`
/// pixels: its `scaling` places a non-square image, `color` is a glyph's colour and an SVG's
/// `currentColor`, a `tint` option recolours every pixel (resolved in `theme`), `mirror` flips it.
/// `None` when the source cannot be read or the system cannot render it.
pub fn rasterize(value: &str, width: u32, height: u32, color: D2D1_COLOR_F, theme: Option<&Theme>) -> Option<Raster> {
    let spec = icon_source::parse(value);
    let (width, height) = (width.clamp(1, 1024), height.clamp(1, 1024));
    let mut raster = with_offscreen(|o| render(o, &spec, width, height, color, theme))?;
    if let Some(t) = spec.tint.and_then(|t| tint_color(t, theme)) {
        recolor(&mut raster.pixels, t);
    }
    if spec.mirror {
        mirror(&mut raster);
    }
    Some(raster)
}

fn render(o: &mut Offscreen, spec: &IconSpec<'_>, width: u32, height: u32, color: D2D1_COLOR_F, theme: Option<&Theme>) -> Option<Raster> {
    let source = if spec.is_image() { Some(source(spec.source, &o.wic)?) } else { None };
    render_source(o, source.as_deref(), spec, width, height, color, theme)
}

/// Renders `source` (a decoded image; `None` for the glyph `spec` names) into `width` × `height` pixels.
fn render_source(o: &mut Offscreen, source: Option<&Source>, spec: &IconSpec<'_>, width: u32, height: u32, color: D2D1_COLOR_F, theme: Option<&Theme>) -> Option<Raster> {
    // SAFETY: plain Direct2D/WIC calls on objects created here; the render target draws into
    // `bitmap`, whose pixels are copied out after `EndDraw`.
    unsafe {
        let bitmap = o.wic.CreateBitmap(width, height, &GUID_WICPixelFormat32bppPBGRA, WICBitmapCacheOnLoad).ok()?;
        let props = D2D1_RENDER_TARGET_PROPERTIES {
            r#type: D2D1_RENDER_TARGET_TYPE_SOFTWARE,
            pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED },
            dpiX: 96.0,
            dpiY: 96.0,
            usage: D2D1_RENDER_TARGET_USAGE_NONE,
            minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
        };
        let rt: ID2D1RenderTarget = o.factory.CreateWicBitmapRenderTarget(&bitmap, &props).ok()?;
        let ctx: ID2D1DeviceContext = rt.cast().ok()?;
        ctx.BeginDraw();
        ctx.Clear(Some(&D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 0.0 }));
        let (w, h) = (width as f32, height as f32);
        let drawn = match source {
            None => draw_glyph(o, &ctx, spec, w, h, color, theme),
            Some(Source::Svg(bytes)) => draw_svg(&ctx, bytes, spec.scaling, w, h, color),
            Some(Source::Raster(frames)) => draw_raster(&ctx, frames, spec.scaling, w, h),
        };
        let ended = ctx.EndDraw(None, None);
        if !drawn || ended.is_err() {
            return None;
        }
        let mut pixels = vec![0u8; (width * height * 4) as usize];
        bitmap.CopyPixels(std::ptr::null(), width * 4, &mut pixels).ok()?;
        Some(Raster { width, height, pixels })
    }
}

/// A glyph of the embedded set, as large as fits in the box.
unsafe fn draw_glyph(o: &mut Offscreen, ctx: &ID2D1DeviceContext, spec: &IconSpec<'_>, w: f32, h: f32, color: D2D1_COLOR_F, theme: Option<&Theme>) -> bool {
    let Some(name) = kubuno_drive_desktop_app_controls::icon_name(spec.source) else { return false };
    let Ok(factory) = o.factory.cast::<ID2D1Factory>() else { return false };
    let stroke = o.glyphs.stroke_style(&factory).cloned();
    let Some((layers, viewbox)) = o.glyphs.get_layers(&factory, name) else { return false };
    let (x, y, dw, _) = spec.scaling.place((viewbox, viewbox), (w, h));
    let k = dw / viewbox;
    let accent = theme.map_or(color, |t| t.accent);
    let Ok(brush) = ctx.CreateSolidColorBrush(&color, None) else { return false };
    for layer in layers {
        let [a, b, c, d, e, f] = layer.transform;
        ctx.SetTransform(&windows_numerics::Matrix3x2 { M11: a * k, M12: b * k, M21: c * k, M22: d * k, M31: e * k + x, M32: f * k + y });
        use kubuno_drive_desktop_app_controls::LayerRole;
        let base = layer.color.unwrap_or(match layer.role {
            LayerRole::Accent => accent,
            LayerRole::AccentContrast => D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 1.0 },
            _ => color,
        });
        brush.SetColor(&D2D1_COLOR_F { a: base.a * layer.opacity, ..base });
        match layer.stroke {
            Some(width) => ctx.DrawGeometry(&layer.geometry, &brush, width, stroke.as_ref()),
            None => ctx.FillGeometry(&layer.geometry, &brush, None),
        }
    }
    ctx.SetTransform(&windows_numerics::Matrix3x2::identity());
    true
}

/// An SVG document, rendered as vectors at the box's size: `currentColor` is `color`.
unsafe fn draw_svg(ctx: &ID2D1DeviceContext, bytes: &[u8], scaling: IconScaling, w: f32, h: f32, color: D2D1_COLOR_F) -> bool {
    let Ok(ctx5) = ctx.cast::<ID2D1DeviceContext5>() else {
        tracing::warn!("SVG icons need Windows 10 1703 or later");
        return false;
    };
    let Some(stream) = windows::Win32::UI::Shell::SHCreateMemStream(Some(bytes)) else { return false };
    let (iw, ih) = svg_intrinsic_size(bytes).unwrap_or((24.0, 24.0));
    let (x, y, dw, dh) = scaling.place((iw, ih), (w, h));
    let Ok(doc) = ctx5.CreateSvgDocument(&stream, D2D_SIZE_F { width: dw.max(1.0), height: dh.max(1.0) }) else {
        tracing::warn!("an SVG icon could not be parsed");
        return false;
    };
    let Ok(root) = doc.GetRoot() else { return false };
    // The document fills its viewport exactly (the placement above already kept or not its
    // proportions): its viewBox spans the intrinsic size, its own width and height are 100 %.
    let set = |root: &ID2D1SvgElement, name: windows::core::PCWSTR, ty, value: *const std::ffi::c_void, size: usize| {
        let _ = root.SetAttributeValue2(name, ty, value, size as u32);
    };
    let mut viewbox = D2D1_SVG_VIEWBOX::default();
    let has_viewbox = root
        .GetAttributeValue2(w!("viewBox"), D2D1_SVG_ATTRIBUTE_POD_TYPE_VIEWBOX, &mut viewbox as *mut _ as *mut _, std::mem::size_of::<D2D1_SVG_VIEWBOX>() as u32)
        .is_ok()
        && viewbox.width > 0.0
        && viewbox.height > 0.0;
    if !has_viewbox {
        let vb = D2D1_SVG_VIEWBOX { x: 0.0, y: 0.0, width: iw, height: ih };
        set(&root, w!("viewBox"), D2D1_SVG_ATTRIBUTE_POD_TYPE_VIEWBOX, &vb as *const _ as *const _, std::mem::size_of::<D2D1_SVG_VIEWBOX>());
    }
    let full = D2D1_SVG_LENGTH { value: 100.0, units: D2D1_SVG_LENGTH_UNITS_PERCENTAGE };
    set(&root, w!("width"), D2D1_SVG_ATTRIBUTE_POD_TYPE_LENGTH, &full as *const _ as *const _, std::mem::size_of::<D2D1_SVG_LENGTH>());
    set(&root, w!("height"), D2D1_SVG_ATTRIBUTE_POD_TYPE_LENGTH, &full as *const _ as *const _, std::mem::size_of::<D2D1_SVG_LENGTH>());
    let aspect = D2D1_SVG_PRESERVE_ASPECT_RATIO { defer: false.into(), align: D2D1_SVG_ASPECT_ALIGN_NONE, meetOrSlice: D2D1_SVG_ASPECT_SCALING_MEET };
    set(&root, w!("preserveAspectRatio"), D2D1_SVG_ATTRIBUTE_POD_TYPE_PRESERVE_ASPECT_RATIO, &aspect as *const _ as *const _, std::mem::size_of::<D2D1_SVG_PRESERVE_ASPECT_RATIO>());
    set(&root, w!("color"), D2D1_SVG_ATTRIBUTE_POD_TYPE_COLOR, &color as *const _ as *const _, std::mem::size_of::<D2D1_COLOR_F>());
    let _ = D2D1_SVG_LENGTH_UNITS_NUMBER; // Lengths in user units are kept as written.
    ctx.PushAxisAlignedClip(&D2D_RECT_F { left: 0.0, top: 0.0, right: w, bottom: h }, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
    ctx.SetTransform(&windows_numerics::Matrix3x2 { M11: 1.0, M12: 0.0, M21: 0.0, M22: 1.0, M31: x, M32: y });
    ctx5.DrawSvgDocument(&doc);
    ctx.SetTransform(&windows_numerics::Matrix3x2::identity());
    ctx.PopAxisAlignedClip();
    true
}

/// A raster image (its best frame), resampled into the box with a high-quality filter.
unsafe fn draw_raster(ctx: &ID2D1DeviceContext, frames: &[Frame], scaling: IconScaling, w: f32, h: f32) -> bool {
    let Some(frame) = best_frame(frames, w.max(h).ceil() as u32) else { return false };
    let (fw, fh) = (&frame.width, &frame.height);
    let Some(bitmap) = upload_pixels(ctx, frame.width, frame.height, &frame.pixels) else { return false };
    let (x, y, dw, dh) = scaling.place((*fw as f32, *fh as f32), (w, h));
    // Down: a high-quality cubic filter; up: linear (a cubic filter fades a tiny image to nothing).
    let mode = if dw < *fw as f32 || dh < *fh as f32 { D2D1_INTERPOLATION_MODE_HIGH_QUALITY_CUBIC } else { D2D1_INTERPOLATION_MODE_LINEAR };
    ctx.PushAxisAlignedClip(&D2D_RECT_F { left: 0.0, top: 0.0, right: w, bottom: h }, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
    ctx.DrawBitmap(&bitmap, Some(&D2D_RECT_F { left: x, top: y, right: x + dw, bottom: y + dh }), 1.0, mode, None, None);
    ctx.PopAxisAlignedClip();
    true
}

/// Every pixel takes `tint`, its coverage kept (premultiplied BGRA). Pure.
pub fn recolor(pixels: &mut [u8], tint: D2D1_COLOR_F) {
    let channel = |c: f32| c.clamp(0.0, 1.0);
    for px in pixels.as_chunks_mut::<4>().0 {
        let a = f32::from(px[3]) / 255.0 * channel(tint.a);
        px[0] = (channel(tint.b) * a * 255.0).round() as u8;
        px[1] = (channel(tint.g) * a * 255.0).round() as u8;
        px[2] = (channel(tint.r) * a * 255.0).round() as u8;
        px[3] = (a * 255.0).round() as u8;
    }
}

/// Flips `raster` horizontally. Pure.
pub fn mirror(raster: &mut Raster) {
    let row = raster.width as usize * 4;
    for line in raster.pixels.chunks_exact_mut(row) {
        line.as_chunks_mut::<4>().0.reverse();
    }
}

// ── On screen ──────────────────────────────────────────────────────────────────────────────────

/// Draws icon `name` (an image file, or a glyph with options — see the module doc) `size` DIP
/// square — or its own `size` option — centred in `rect` on device context `ctx` at DPI `scale`,
/// in `color` (the context's transform places it). Returns the DIP rectangle drawn, `None`
/// when nothing could be (the file is missing or unreadable).
#[allow(clippy::too_many_arguments)]
pub fn draw(
    ctx: &ID2D1DeviceContext,
    name: &'static str,
    rect: (f32, f32, f32, f32),
    size: f32,
    scale: f32,
    color: D2D1_COLOR_F,
    theme: &Theme,
) -> Option<(f32, f32, f32, f32)> {
    let spec = icon_source::parse(name);
    let (bw, bh) = spec.size.unwrap_or((size, size));
    let s = scale.max(0.01);
    let (pw, ph) = ((bw * s).round().max(1.0) as u32, (bh * s).round().max(1.0) as u32);
    let cx = (rect.0 + rect.2) / 2.0;
    let cy = (rect.1 + rect.3) / 2.0;
    // On the pixel grid, so the rendered pixels map one to one.
    let left = ((cx * s) - pw as f32 / 2.0).round() / s;
    let top = ((cy * s) - ph as f32 / 2.0).round() / s;
    let dest = (left, top, left + pw as f32 / s, top + ph as f32 / s);
    let device = ctx.as_raw() as usize;
    let rgba = |c: D2D1_COLOR_F| {
        let b = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u32;
        (b(c.r) << 24) | (b(c.g) << 16) | (b(c.b) << 8) | b(c.a)
    };
    // A tint replaces the colour: one bitmap whatever the colour asked for.
    let key_color = if spec.tint.is_some() { 0 } else { rgba(color) };
    let key = (device, name.as_ptr() as usize, pw, ph, key_color);
    let bitmap = BITMAPS.with(|m| m.borrow().get(&key).cloned()).unwrap_or_else(|| {
        let made = rasterize(name, pw, ph, color, Some(theme)).and_then(|r| upload(ctx, &r));
        BITMAPS.with(|m| m.borrow_mut().insert(key, made.clone()));
        made
    })?;
    // A glyph or an SVG is already rendered in `color`; a raster image takes its transparency.
    let raster = spec.is_image() && icon_source::image_extension(spec.source) != Some("svg") && spec.tint.is_none();
    let opacity = if raster { color.a.clamp(0.0, 1.0) } else { 1.0 };
    // SAFETY: a plain draw call on a live device context and a bitmap made on it.
    unsafe {
        ctx.DrawBitmap(&bitmap, Some(&D2D_RECT_F { left: dest.0, top: dest.1, right: dest.2, bottom: dest.3 }), opacity, D2D1_INTERPOLATION_MODE_HIGH_QUALITY_CUBIC, None, None);
    }
    Some(dest)
}

/// `raster` as a bitmap of device context `ctx`.
fn upload(ctx: &ID2D1DeviceContext, raster: &Raster) -> Option<ID2D1Bitmap1> {
    upload_pixels(ctx, raster.width, raster.height, &raster.pixels)
}

/// Premultiplied BGRA rows of `width` x `height` pixels as a bitmap of device context `ctx`.
fn upload_pixels(ctx: &ID2D1DeviceContext, width: u32, height: u32, pixels: &[u8]) -> Option<ID2D1Bitmap1> {
    if pixels.len() < width as usize * height as usize * 4 {
        return None;
    }
    let props = D2D1_BITMAP_PROPERTIES1 {
        pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED },
        dpiX: 96.0,
        dpiY: 96.0,
        bitmapOptions: D2D1_BITMAP_OPTIONS_NONE,
        colorContext: std::mem::ManuallyDrop::new(None),
    };
    // SAFETY: the pixel buffer is `width * height * 4` bytes with that pitch, read during the call.
    unsafe { ctx.CreateBitmap(D2D_SIZE_U { width, height }, Some(pixels.as_ptr().cast()), width * 4, &props).ok() }
}

/// Releases this thread's decoded icon images and their device bitmaps (the host does it when its
/// window loop ends, while the device and COM are still there).
pub fn release() {
    BITMAPS.with(|m| m.borrow_mut().clear());
    SOURCES.with(|m| m.borrow_mut().clear());
    OFFSCREEN.with(|o| {
        if let Ok(mut o) = o.try_borrow_mut() {
            *o = None;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn svg_sizes_come_from_the_view_box_or_the_dimensions() {
        assert_eq!(svg_intrinsic_size(br#"<svg xmlns="x" viewBox="0 0 48 32" width="10"/>"#), Some((48.0, 32.0)));
        assert_eq!(svg_intrinsic_size(br#"<?xml version="1.0"?><svg width="16px" height='20'></svg>"#), Some((16.0, 20.0)));
        assert_eq!(svg_intrinsic_size(br#"<svg width="100%"></svg>"#), Some((24.0, 24.0)));
        assert_eq!(svg_intrinsic_size(b"not svg"), None);
    }

    #[test]
    fn a_tint_keeps_the_coverage() {
        let mut px = vec![0, 0, 0, 255, 10, 10, 10, 128, 0, 0, 0, 0];
        recolor(&mut px, D2D1_COLOR_F { r: 1.0, g: 0.0, b: 0.0, a: 1.0 });
        assert_eq!(px, vec![0, 0, 255, 255, 0, 0, 128, 128, 0, 0, 0, 0]);
    }

    #[test]
    fn mirror_flips_each_row() {
        let mut r = Raster { width: 2, height: 1, pixels: vec![1, 1, 1, 1, 2, 2, 2, 2] };
        mirror(&mut r);
        assert_eq!(r.pixels, vec![2, 2, 2, 2, 1, 1, 1, 1]);
    }

    #[test]
    fn straight_alpha_undoes_the_premultiplication() {
        let r = Raster { width: 1, height: 1, pixels: vec![64, 0, 128, 128] };
        assert_eq!(r.straight_alpha(), vec![128, 0, 255, 128]);
    }
}
