//! Painting part of a window in other colours, another font or with an image — what a control's
//! `BackColor`, `ForeColor`, `Font`, `RightToLeft`, `BackgroundImage` and `Image` need
//! (`vskubuno/docs/EVENTS.md` §16, "WinForms-rich property sets").
//!
//! The widgets of `kubuno_desktop_ui` paint with the ambient theme ([`Canvas::theme`]) and the shared text
//! formats ([`Canvas::formats`]); they never take a colour or a font from their caller. So a control
//! whose colours or font differ is painted through a [`StyledCanvas`]: the same surface, answering
//! with a theme whose tokens were overridden and a set of formats built for its font. Every drawing
//! call goes straight through to the real surface, so nothing else changes — clips, offsets, the
//! background stack and the pixel snapping stay the window's.
//!
//! Also here: the named system colours (`SystemColors.Control`…) a colour property may name, whether
//! Windows' high-contrast mode is on, and the loading and laying out of images.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use kubuno_drive_desktop_app_controls::{create_text_formats_styled, Canvas, Rect, Renderer, TextFormats, TextStyle, Theme};
/// The colour type every drawing call takes, re-exported for the crates above (which do not name
/// Direct2D themselves).
pub use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;
/// A decoded image (see [`load_image`]).
pub use windows::Win32::Graphics::Direct2D::ID2D1Bitmap1;
use windows::Win32::Graphics::DirectWrite::{IDWriteTextFormat, DWRITE_TEXT_ALIGNMENT, DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_TRAILING};

use crate::control::ControlCanvas;
use crate::enums::{ContentAlignment, ImageLayout};
use crate::system::Visuals;

/// A surface that paints like `inner`, except for the theme and the text formats it answers with,
/// and the decorations (underline, strike-out) it adds under the text it draws.
pub struct StyledCanvas<'a, C: ?Sized + Canvas = dyn ControlCanvas + 'a> {
    inner: &'a C,
    theme: Option<&'a Theme>,
    formats: Option<&'a TextFormats>,
    underline: bool,
    strikeout: bool,
}

impl<'a, C: ?Sized + Canvas> StyledCanvas<'a, C> {
    /// Paints through to `inner` (a [`ControlCanvas`], or a bare [`Canvas`] for a measure).
    pub fn new(inner: &'a C) -> Self {
        Self { inner, theme: None, formats: None, underline: false, strikeout: false }
    }

    /// Answers [`Canvas::theme`] with `theme`.
    pub fn with_theme(mut self, theme: &'a Theme) -> Self {
        self.theme = Some(theme);
        self
    }

    /// Answers [`Canvas::formats`] with `formats`.
    pub fn with_formats(mut self, formats: &'a TextFormats) -> Self {
        self.formats = Some(formats);
        self
    }

    /// Underlines and/or strikes out every text it draws (a font's `Underline`/`Strikeout` style,
    /// which a DirectWrite text format cannot carry).
    pub fn with_decorations(mut self, underline: bool, strikeout: bool) -> Self {
        self.underline = underline;
        self.strikeout = strikeout;
        self
    }

    fn decorate(&self, text: &str, rect: &Rect, format: &IDWriteTextFormat, color: &D2D1_COLOR_F, alignment: DWRITE_TEXT_ALIGNMENT) {
        if !self.underline && !self.strikeout || text.is_empty() {
            return;
        }
        let width = self.inner.measure(text, format).min(rect.right - rect.left).max(0.0);
        let left = if alignment == DWRITE_TEXT_ALIGNMENT_CENTER {
            (rect.left + rect.right - width) / 2.0
        } else if alignment == DWRITE_TEXT_ALIGNMENT_TRAILING {
            rect.right - width
        } else {
            rect.left
        };
        // SAFETY: a plain COM getter on a live text format.
        let size = unsafe { format.GetFontSize() }.max(1.0);
        let middle = (rect.top + rect.bottom) / 2.0;
        let thickness = (size / 14.0).max(1.0 / self.inner.scale().max(0.01));
        if self.underline {
            let y = middle + size * 0.42;
            self.inner.fill_rounded(&Rect::new(left, y, left + width, y + thickness), 0.0, color);
        }
        if self.strikeout {
            let y = middle + size * 0.02;
            self.inner.fill_rounded(&Rect::new(left, y, left + width, y + thickness), 0.0, color);
        }
    }
}

impl<C: ?Sized + Canvas> Canvas for StyledCanvas<'_, C> {
    fn theme(&self) -> &Theme {
        self.theme.unwrap_or_else(|| self.inner.theme())
    }

    fn formats(&self) -> &TextFormats {
        self.formats.unwrap_or_else(|| self.inner.formats())
    }

    fn scale(&self) -> f32 {
        self.inner.scale()
    }

    fn fill_rounded(&self, rect: &Rect, radius: f32, color: &D2D1_COLOR_F) {
        self.inner.fill_rounded(rect, radius, color);
    }

    fn fill_top_rounded(&self, rect: &Rect, radius: f32, color: &D2D1_COLOR_F) {
        self.inner.fill_top_rounded(rect, radius, color);
    }

    fn fill_triangle(&self, a: (f32, f32), b: (f32, f32), c: (f32, f32), color: &D2D1_COLOR_F) {
        self.inner.fill_triangle(a, b, c, color);
    }

    fn stroke_arc(&self, centre: (f32, f32), radius: f32, start: f32, sweep: f32, width: f32, color: &D2D1_COLOR_F) {
        self.inner.stroke_arc(centre, radius, start, sweep, width, color);
    }

    fn stroke_rounded(&self, rect: &Rect, radius: f32, color: &D2D1_COLOR_F) {
        self.inner.stroke_rounded(rect, radius, color);
    }

    fn stroke_rounded_w(&self, rect: &Rect, radius: f32, color: &D2D1_COLOR_F, width: f32) {
        self.inner.stroke_rounded_w(rect, radius, color, width);
    }

    fn text(&self, text: &str, rect: &Rect, format: &IDWriteTextFormat, color: &D2D1_COLOR_F, centered: bool) {
        self.inner.text(text, rect, format, color, centered);
        let alignment = if centered { DWRITE_TEXT_ALIGNMENT_CENTER } else { DWRITE_TEXT_ALIGNMENT(0) };
        self.decorate(text, rect, format, color, alignment);
    }

    fn text_aligned(&self, text: &str, rect: &Rect, format: &IDWriteTextFormat, color: &D2D1_COLOR_F, alignment: DWRITE_TEXT_ALIGNMENT) {
        self.inner.text_aligned(text, rect, format, color, alignment);
        self.decorate(text, rect, format, color, alignment);
    }

    fn text_ellipsis(&self, text: &str, rect: &Rect, format: &IDWriteTextFormat, color: &D2D1_COLOR_F) {
        self.inner.text_ellipsis(text, rect, format, color);
        self.decorate(text, rect, format, color, DWRITE_TEXT_ALIGNMENT(0));
    }

    fn text_ellipsis_center(&self, text: &str, rect: &Rect, format: &IDWriteTextFormat, color: &D2D1_COLOR_F) {
        self.inner.text_ellipsis_center(text, rect, format, color);
        self.decorate(text, rect, format, color, DWRITE_TEXT_ALIGNMENT_CENTER);
    }

    fn image(&self, bitmap: &ID2D1Bitmap1, rect: &Rect, size: f32) {
        self.inner.image(bitmap, rect, size);
    }

    fn image_alpha(&self, bitmap: &ID2D1Bitmap1, rect: &Rect, size: f32, alpha: f32) {
        self.inner.image_alpha(bitmap, rect, size, alpha);
    }

    fn vector_icon(&self, name: &'static str, rect: &Rect, size: f32, color: &D2D1_COLOR_F) {
        self.inner.vector_icon(name, rect, size, color);
    }

    fn vector_icon_layered(&self, name: &'static str, rect: &Rect, size: f32, fg: &D2D1_COLOR_F, accent: &D2D1_COLOR_F) {
        self.inner.vector_icon_layered(name, rect, size, fg, accent);
    }

    fn measure(&self, text: &str, format: &IDWriteTextFormat) -> f32 {
        self.inner.measure(text, format)
    }

    fn draw_card_shadow(&self, rect: &Rect, radius: f32) {
        self.inner.draw_card_shadow(rect, radius);
    }

    fn draw_shadow(&self, rect: &Rect, radius: f32, layers: &[kubuno_drive_desktop_app_controls::themes::shape::ShadowLayer], colour: (f32, f32, f32)) {
        self.inner.draw_shadow(rect, radius, layers, colour);
    }

    fn erase_rounded(&self, rect: &Rect, radius: f32) {
        self.inner.erase_rounded(rect, radius);
    }

    fn push_clip(&self, rect: &Rect) {
        self.inner.push_clip(rect);
    }

    fn push_clip_rounded(&self, rect: &Rect, radius: f32) {
        self.inner.push_clip_rounded(rect, radius);
    }

    fn pop_clip_rounded(&self) {
        self.inner.pop_clip_rounded();
    }

    fn pop_clip(&self) {
        self.inner.pop_clip();
    }

    fn push_offset(&self, dx: f32, dy: f32) {
        self.inner.push_offset(dx, dy);
    }

    fn pop_offset(&self) {
        self.inner.pop_offset();
    }

    fn begin_extent(&self) {
        self.inner.begin_extent();
    }

    fn end_extent(&self) -> Option<(f32, f32)> {
        self.inner.end_extent()
    }

    fn current_bg(&self) -> D2D1_COLOR_F {
        self.inner.current_bg()
    }

    fn push_bg(&self, colour: D2D1_COLOR_F) {
        self.inner.push_bg(colour);
    }

    fn pop_bg(&self) {
        self.inner.pop_bg();
    }

    fn graphics_renderer(&self) -> Option<&Renderer> {
        self.inner.graphics_renderer()
    }

    fn note_drawn(&self, rect: &Rect) {
        self.inner.note_drawn(rect);
    }
}

impl<C: ?Sized + ControlCanvas> ControlCanvas for StyledCanvas<'_, C> {
    fn visuals(&self) -> &Visuals {
        self.inner.visuals()
    }

    fn renderer(&self) -> Option<&Renderer> {
        self.inner.renderer()
    }

    fn draw_theme_part(&self, class: &str, part: i32, state: i32, rect: Rect, background: D2D1_COLOR_F) -> bool {
        self.inner.draw_theme_part(class, part, state, rect, background)
    }

    fn draw_bitmap(&self, bitmap: &ID2D1Bitmap1, dest: &Rect, alpha: f32) {
        self.inner.draw_bitmap(bitmap, dest, alpha);
    }
}

// ── Fonts ────────────────────────────────────────────────────────────────────

thread_local! {
    /// The text formats built for each style (they are device independent: kept for the thread).
    static FORMATS: RefCell<HashMap<StyleKey, Rc<TextFormats>>> = RefCell::new(HashMap::new());
}

#[derive(Clone, PartialEq, Eq, Hash)]
struct StyleKey {
    family: Option<String>,
    scale: u32,
    bold: bool,
    italic: bool,
    right_to_left: bool,
    app_font: Option<String>,
}

/// The shared text formats restyled by `style` (see [`TextStyle`]), built once per style and
/// thread; `None` when the surface has no DirectWrite factory to build them with (a test canvas).
pub fn styled_formats(c: &dyn ControlCanvas, style: &TextStyle) -> Option<Rc<TextFormats>> {
    if *style == TextStyle::default() {
        return None;
    }
    let renderer = c.renderer()?;
    formats_with(&renderer.dwrite, style)
}

/// [`styled_formats`] without a surface: built with the process's shared DirectWrite factory (the formats
/// are device independent), for measuring text where only a bare [`Canvas`] is at hand.
pub fn measure_formats(style: &TextStyle) -> Option<Rc<TextFormats>> {
    if *style == TextStyle::default() {
        return None;
    }
    // SAFETY: creating the shared factory has no precondition.
    let dwrite: windows::Win32::Graphics::DirectWrite::IDWriteFactory =
        unsafe { windows::Win32::Graphics::DirectWrite::DWriteCreateFactory(windows::Win32::Graphics::DirectWrite::DWRITE_FACTORY_TYPE_SHARED) }.ok()?;
    formats_with(&dwrite, style)
}

fn formats_with(dwrite: &windows::Win32::Graphics::DirectWrite::IDWriteFactory, style: &TextStyle) -> Option<Rc<TextFormats>> {
    let app_font = crate::host::current_font_override();
    let key = StyleKey {
        family: style.family.clone(),
        scale: style.scale.to_bits(),
        bold: style.bold,
        italic: style.italic,
        right_to_left: style.right_to_left,
        app_font: app_font.clone(),
    };
    if let Some(found) = FORMATS.with(|f| f.borrow().get(&key).cloned()) {
        return Some(found);
    }
    match create_text_formats_styled(dwrite, app_font.as_deref(), style) {
        Ok(formats) => {
            let formats = Rc::new(formats);
            FORMATS.with(|f| f.borrow_mut().insert(key, formats.clone()));
            Some(formats)
        }
        Err(error) => {
            tracing::warn!("text formats for {:?} could not be created: {error}", style.family);
            None
        }
    }
}

// ── System colours and high contrast ──────────────────────────────────────────

/// The .NET `SystemColors` names a colour property accepts, each with its Win32 `COLOR_*` index.
pub const SYSTEM_COLORS: &[(&str, i32)] = &[
    ("ActiveBorder", 10),
    ("ActiveCaption", 2),
    ("ActiveCaptionText", 9),
    ("AppWorkspace", 12),
    ("ButtonFace", 15),
    ("ButtonHighlight", 20),
    ("ButtonShadow", 16),
    ("Control", 15),
    ("ControlDark", 16),
    ("ControlDarkDark", 21),
    ("ControlLight", 22),
    ("ControlLightLight", 20),
    ("ControlText", 18),
    ("Desktop", 1),
    ("GradientActiveCaption", 27),
    ("GradientInactiveCaption", 28),
    ("GrayText", 17),
    ("Highlight", 13),
    ("HighlightText", 14),
    ("HotTrack", 26),
    ("InactiveBorder", 11),
    ("InactiveCaption", 3),
    ("InactiveCaptionText", 19),
    ("Info", 24),
    ("InfoText", 23),
    ("Menu", 4),
    ("MenuBar", 30),
    ("MenuHighlight", 29),
    ("MenuText", 7),
    ("ScrollBar", 0),
    ("Window", 5),
    ("WindowFrame", 6),
    ("WindowText", 8),
];

/// The current value of the system colour `name` (a [`SYSTEM_COLORS`] name, any case).
pub fn system_color(name: &str) -> Option<D2D1_COLOR_F> {
    let (_, index) = SYSTEM_COLORS.iter().find(|(n, _)| n.eq_ignore_ascii_case(name))?;
    // SAFETY: `GetSysColor` takes any index and returns 0 for an unknown one.
    let colorref = unsafe { windows::Win32::Graphics::Gdi::GetSysColor(windows::Win32::Graphics::Gdi::SYS_COLOR_INDEX(*index)) };
    Some(crate::system::colorref_to_d2d(colorref))
}

/// Whether Windows' high-contrast mode is on — theme tokens then resolve to the system colours,
/// like every Windows application's.
pub fn high_contrast() -> bool {
    use windows::Win32::UI::Accessibility::{HCF_HIGHCONTRASTON, HIGHCONTRASTW};
    use windows::Win32::UI::WindowsAndMessaging::{SystemParametersInfoW, SPI_GETHIGHCONTRAST, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS};
    let mut hc = HIGHCONTRASTW { cbSize: std::mem::size_of::<HIGHCONTRASTW>() as u32, ..Default::default() };
    // SAFETY: `hc` is a correctly sized, writable HIGHCONTRASTW.
    let ok = unsafe {
        SystemParametersInfoW(SPI_GETHIGHCONTRAST, hc.cbSize, Some(&mut hc as *mut _ as *mut core::ffi::c_void), SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0))
    };
    ok.is_ok() && (hc.dwFlags & HCF_HIGHCONTRASTON) == HCF_HIGHCONTRASTON
}

// ── Images ────────────────────────────────────────────────────────────────────

thread_local! {
    /// Decoded bitmaps by path, for the device context they belong to (a new device reloads them).
    static IMAGES: RefCell<HashMap<(usize, String), Option<ID2D1Bitmap1>>> = RefCell::new(HashMap::new());
}

/// Resolves an image URI of an application resource (`kbres:<set>/<name>`, vskubuno docs/RESOURCES.md):
/// calls `with(content id, bytes, format)` and returns true when the URI names an image. Installed by
/// the view layer (`kubuno_desktop_views::resources::install`), which knows the resources and the culture.
pub type ResourceImageResolver = fn(uri: &str, with: &mut dyn FnMut(u64, &[u8], &str)) -> bool;

static RESOURCE_IMAGES: std::sync::RwLock<Option<ResourceImageResolver>> = std::sync::RwLock::new(None);

/// Installs the resolver of `kbres:` image URIs (see [`ResourceImageResolver`]).
pub fn set_resource_image_resolver(resolver: ResourceImageResolver) {
    if let Ok(mut r) = RESOURCE_IMAGES.write() {
        *r = Some(resolver);
    }
}

/// A `kbres:` image: decoded once per content (the content id changes with the culture, so a
/// localised picture follows a culture switch).
fn load_resource_image(c: &dyn ControlCanvas, uri: &str) -> Option<ID2D1Bitmap1> {
    let resolver = RESOURCE_IMAGES.read().ok().and_then(|r| *r)?;
    let mut found = None;
    let known = resolver(uri, &mut |id, bytes, _format| {
        found = if crate::icon_image::is_svg(bytes) { load_svg_bytes(c, id, bytes) } else { load_image_bytes(c, id, bytes) };
    });
    if !known {
        tracing::debug!("image resource {uri} not found");
    }
    found
}

/// An SVG held in memory, rendered once per `key` at its intrinsic size for the surface's device.
fn load_svg_bytes(c: &dyn ControlCanvas, key: u64, bytes: &[u8]) -> Option<ID2D1Bitmap1> {
    let renderer = c.renderer()?;
    let device = windows::core::Interface::as_raw(&renderer.d2d_context) as usize;
    let cache_key = (device, format!("svg:{key:x}"));
    if let Some(found) = IMAGES.with(|m| m.borrow().get(&cache_key).cloned()) {
        return found;
    }
    let bitmap = match crate::icon_image::decode_svg_bytes(renderer, bytes) {
        Ok(b) => Some(b),
        Err(error) => {
            tracing::warn!("an SVG of {} bytes could not be rendered: {error}", bytes.len());
            None
        }
    };
    IMAGES.with(|m| m.borrow_mut().insert(cache_key, bitmap.clone()));
    bitmap
}

/// The image file at `path`, decoded once for the surface's device (`None` when the surface has no
/// device, or the file cannot be read or decoded — logged once).
pub fn load_image(c: &dyn ControlCanvas, path: &str) -> Option<ID2D1Bitmap1> {
    if path.starts_with("kbres:") {
        return load_resource_image(c, path);
    }
    let renderer = c.renderer()?;
    let device = windows::core::Interface::as_raw(&renderer.d2d_context) as usize;
    let key = (device, path.to_string());
    if let Some(found) = IMAGES.with(|m| m.borrow().get(&key).cloned()) {
        return found;
    }
    let bitmap = match renderer.load_image_file(path) {
        Ok(b) => Some(b),
        Err(error) => {
            tracing::warn!("image {path} could not be loaded: {error}");
            None
        }
    };
    IMAGES.with(|m| m.borrow_mut().insert(key, bitmap.clone()));
    bitmap
}

/// An image held in memory (the bytes of a PNG, JPEG, GIF, BMP, TIFF or ICO file), decoded once per
/// `key` for the surface's device — `key` names the content (a hash, a stamp): the same key never
/// decodes twice. `None` when the surface has no device or the bytes are not an image (logged once).
pub fn load_image_bytes(c: &dyn ControlCanvas, key: u64, bytes: &[u8]) -> Option<ID2D1Bitmap1> {
    let renderer = c.renderer()?;
    let device = windows::core::Interface::as_raw(&renderer.d2d_context) as usize;
    let cache_key = (device, format!("mem:{key:x}"));
    if let Some(found) = IMAGES.with(|m| m.borrow().get(&cache_key).cloned()) {
        return found;
    }
    let bitmap = match decode_bytes(renderer, bytes) {
        Ok(b) => Some(b),
        Err(error) => {
            tracing::warn!("an image of {} bytes could not be decoded: {error}", bytes.len());
            None
        }
    };
    IMAGES.with(|m| m.borrow_mut().insert(cache_key, bitmap.clone()));
    bitmap
}

fn decode_bytes(renderer: &Renderer, bytes: &[u8]) -> windows::core::Result<ID2D1Bitmap1> {
    use windows::Win32::Graphics::Imaging::*;
    use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};
    // SAFETY: plain WIC calls on objects created here; `bytes` outlives the stream, which is only
    // read while the frame is converted into a device bitmap below.
    unsafe {
        let wic: IWICImagingFactory = CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?;
        let stream = wic.CreateStream()?;
        stream.InitializeFromMemory(bytes)?;
        let decoder = wic.CreateDecoderFromStream(&stream, std::ptr::null(), WICDecodeMetadataCacheOnDemand)?;
        let frame = decoder.GetFrame(0)?;
        let converter = wic.CreateFormatConverter()?;
        converter.Initialize(&frame, &GUID_WICPixelFormat32bppPBGRA, WICBitmapDitherTypeNone, None, 0.0, WICBitmapPaletteTypeCustom)?;
        renderer.d2d_context.CreateBitmapFromWicBitmap(&converter, None)
    }
}

/// Releases the decoded images of this thread (see [`load_image`]) — what the host does when its
/// window loop ends, while the device and COM are still there: left to the thread-local destructors,
/// they were released during the process's exit, after the Direct2D and WIC libraries, and the process
/// ended with an error code.
pub fn release_images() {
    IMAGES.with(|m| m.borrow_mut().clear());
}

/// The size of `bitmap`, in DIP.
pub fn image_size(bitmap: &ID2D1Bitmap1) -> (f32, f32) {
    // SAFETY: a plain COM getter on a live bitmap.
    let s = unsafe { bitmap.GetSize() };
    (s.width, s.height)
}

/// Where `size` goes in `bounds` for `layout` (`BackgroundImageLayout`): the rectangles to draw, one
/// per tile for [`ImageLayout::Tile`]. Pure.
pub fn layout_image(size: (f32, f32), bounds: Rect, layout: ImageLayout) -> Vec<Rect> {
    let (w, h) = (size.0.max(1.0), size.1.max(1.0));
    let (bw, bh) = (bounds.right - bounds.left, bounds.bottom - bounds.top);
    match layout {
        ImageLayout::None => vec![Rect::new(bounds.left, bounds.top, bounds.left + w, bounds.top + h)],
        ImageLayout::Center => {
            let (x, y) = (bounds.left + (bw - w) / 2.0, bounds.top + (bh - h) / 2.0);
            vec![Rect::new(x, y, x + w, y + h)]
        }
        ImageLayout::Stretch => vec![bounds],
        ImageLayout::Zoom => {
            let k = (bw / w).min(bh / h).max(0.0);
            let (zw, zh) = (w * k, h * k);
            let (x, y) = (bounds.left + (bw - zw) / 2.0, bounds.top + (bh - zh) / 2.0);
            vec![Rect::new(x, y, x + zw, y + zh)]
        }
        ImageLayout::Tile => {
            let mut out = Vec::new();
            let mut y = bounds.top;
            // Bounded: a degenerate tiny image over a huge area must not produce millions of draws.
            while y < bounds.bottom && out.len() < 4096 {
                let mut x = bounds.left;
                while x < bounds.right && out.len() < 4096 {
                    out.push(Rect::new(x, y, x + w, y + h));
                    x += w;
                }
                y += h;
            }
            out
        }
    }
}

/// Where an image of `size` sits in `bounds` for `align` (a button's `ImageAlign`). Pure.
pub fn align_image(size: (f32, f32), bounds: Rect, align: ContentAlignment) -> Rect {
    let (fx, fy) = align.fractions();
    let x = bounds.left + (bounds.right - bounds.left - size.0) * fx;
    let y = bounds.top + (bounds.bottom - bounds.top - size.1) * fy;
    Rect::new(x, y, x + size.0, y + size.1)
}

/// Draws `bitmap` into `bounds` laid out by `layout`, clipped to `bounds`.
pub fn draw_image(c: &dyn ControlCanvas, bitmap: &ID2D1Bitmap1, bounds: Rect, layout: ImageLayout) {
    c.push_clip(&bounds);
    for dest in layout_image(image_size(bitmap), bounds, layout) {
        c.draw_bitmap(bitmap, &dest, 1.0);
    }
    c.pop_clip();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_layouts_place_the_image_like_winforms() {
        let b = Rect::new(0.0, 0.0, 100.0, 50.0);
        let r = layout_image((20.0, 10.0), b, ImageLayout::Center);
        assert_eq!((r[0].left, r[0].top, r[0].right, r[0].bottom), (40.0, 20.0, 60.0, 30.0));
        let r = layout_image((20.0, 10.0), b, ImageLayout::Zoom);
        assert_eq!((r[0].left, r[0].top, r[0].right, r[0].bottom), (0.0, 0.0, 100.0, 50.0));
        let r = layout_image((40.0, 10.0), b, ImageLayout::Zoom);
        assert_eq!((r[0].left, r[0].right), (0.0, 100.0));
        assert_eq!((r[0].top, r[0].bottom), (12.5, 37.5));
        assert_eq!(layout_image((20.0, 10.0), b, ImageLayout::Tile).len(), 25);
        let r = layout_image((20.0, 10.0), b, ImageLayout::None);
        assert_eq!((r[0].right, r[0].bottom), (20.0, 10.0));
        assert_eq!(layout_image((20.0, 10.0), b, ImageLayout::Stretch)[0].right, 100.0);
    }

    #[test]
    fn an_image_is_aligned_in_its_box() {
        let r = align_image((10.0, 10.0), Rect::new(0.0, 0.0, 100.0, 40.0), ContentAlignment::MiddleRight);
        assert_eq!((r.left, r.top), (90.0, 15.0));
        let r = align_image((10.0, 10.0), Rect::new(0.0, 0.0, 100.0, 40.0), ContentAlignment::TopLeft);
        assert_eq!((r.left, r.top), (0.0, 0.0));
    }

    #[test]
    fn system_colour_names_are_the_dotnet_ones() {
        assert!(SYSTEM_COLORS.iter().any(|(n, _)| *n == "ControlText"));
        assert!(system_color("windowtext").is_some());
        assert!(system_color("NotAColour").is_none());
    }
}
