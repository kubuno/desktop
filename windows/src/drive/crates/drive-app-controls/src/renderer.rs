//! Rendering pipeline (low-level mirror of `Files.App.Controls`): D3D11 device
//! → DXGI composition swapchain → Direct2D context, presented through
//! DirectComposition so transparent pixels let the DWM's Mica backdrop show
//! through behind the window.

use windows::core::{Interface, Result, HSTRING};
use windows::Win32::Foundation::{E_FAIL, HWND};
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_WICPixelFormat32bppPBGRA, IWICImagingFactory,
    WICBitmapDitherTypeNone, WICBitmapPaletteTypeCustom, WICDecodeMetadataCacheOnDemand,
};
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};
use windows::Win32::UI::Shell::SHCreateMemStream;
use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::Direct3D::{D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP};
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::DirectComposition::*;
use windows::Win32::Graphics::DirectWrite::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Dxgi::*;

/// `Clone` (COM references): a subtree painted with another font gets its own set
/// ([`create_text_formats_styled`]).
#[derive(Clone)]
pub struct TextFormats {
    /// Caption with word wrap + centering, for grid tile names.
    pub caption_wrap: IDWriteTextFormat,
    /// Body with word wrap (Details pane values — paths…).
    pub body_wrap: IDWriteTextFormat,
    pub body: IDWriteTextFormat,
    /// Meta size (`--kb-text-meta`) — `AppBarButton` label ("New" in the
    /// command bar), Text family like the control (not the Small variant).
    pub body_small: IDWriteTextFormat,
    pub body_strong: IDWriteTextFormat,
    /// `--kb-text-meta`: metadata, captions.
    pub caption: IDWriteTextFormat,
    /// Meta size, medium — active tab titles.
    pub caption_strong: IDWriteTextFormat,
    /// `--kb-text-micro`: badges, counters.
    pub micro: IDWriteTextFormat,
    pub subtitle: IDWriteTextFormat,
    /// Medium — `--kb-text-heading`: window/dialog titles, section headers.
    pub heading: IDWriteTextFormat,
    /// Semibold heading — a heading the web marks `font-semibold`.
    pub heading_strong: IDWriteTextFormat,
    /// `--kb-text-title`: the object name heading a panel.
    pub title: IDWriteTextFormat,
    /// `--kb-text-page`: a page title (`h1`).
    pub page: IDWriteTextFormat,
    /// `--kb-text-page` inside the administration console (`.kb-admin`).
    pub page_admin: IDWriteTextFormat,
    pub icon_tiny: IDWriteTextFormat,
    /// Caption buttons (min/max/close): 10px like WinUI's `CaptionButton`.
    pub icon_caption: IDWriteTextFormat,
    pub icon_small: IDWriteTextFormat,
    /// Navigation buttons: the original NavigationToolbar uses FontSize=14.
    pub icon_nav: IDWriteTextFormat,
    pub icon: IDWriteTextFormat,
    pub icon_large: IDWriteTextFormat,
}

/// Bitmaps decoded from the original Files image assets.
pub struct Images {
    pub folder: ID2D1Bitmap1,
    pub home: ID2D1Bitmap1,
    pub star: ID2D1Bitmap1,
    pub cloud: ID2D1Bitmap1,
    pub tags: ID2D1Bitmap1,
}

pub struct Renderer {
    pub d2d_context: ID2D1DeviceContext,
    pub d2d_factory: ID2D1Factory1,
    pub dwrite: IDWriteFactory,
    pub formats: TextFormats,
    pub images: Images,
    /// Original ThemedIcon geometries, built lazily.
    pub vector_icons: std::cell::RefCell<crate::VectorIcons>,
    swapchain: IDXGISwapChain1,
    /// Kept so the visual tree can be committed again after the swap chain is
    /// resized.
    dcomp_device: IDCompositionDevice,
    _dcomp_target: Option<IDCompositionTarget>,
    _dcomp_visual: Option<IDCompositionVisual>,
    dpi: f32,
}

impl Renderer {
    pub fn new(hwnd: HWND, width_px: u32, height_px: u32, dpi: f32, font_override: Option<&str>) -> Result<Self> {
        Self::build(Some(hwnd), width_px, height_px, dpi, font_override)
    }

    /// A renderer whose swap chain the CALLER attaches to its own composition
    /// tree (Windows.UI.Composition), so no DirectComposition target is created
    /// here: a window can only have one, and the caller's tree is what carries
    /// the backdrop and the rounded clip.
    pub fn new_detached(
        width_px: u32,
        height_px: u32,
        dpi: f32,
        font_override: Option<&str>,
    ) -> Result<Self> {
        Self::build(None, width_px, height_px, dpi, font_override)
    }

    /// The swap chain, for a caller that composes it itself.
    pub fn swapchain(&self) -> &IDXGISwapChain1 {
        &self.swapchain
    }

    fn build(
        hwnd: Option<HWND>,
        width_px: u32,
        height_px: u32,
        dpi: f32,
        font_override: Option<&str>,
    ) -> Result<Self> {
        unsafe {
            let d3d_device = create_d3d_device()?;
            let dxgi_device: IDXGIDevice = d3d_device.cast()?;

            let adapter = dxgi_device.GetAdapter()?;
            let factory: IDXGIFactory2 = adapter.GetParent()?;

            let desc = DXGI_SWAP_CHAIN_DESC1 {
                Width: width_px.max(1),
                Height: height_px.max(1),
                Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
                BufferCount: 2,
                SwapEffect: DXGI_SWAP_EFFECT_FLIP_DISCARD,
                AlphaMode: DXGI_ALPHA_MODE_PREMULTIPLIED,
                Scaling: DXGI_SCALING_STRETCH,
                ..Default::default()
            };
            let swapchain = factory.CreateSwapChainForComposition(&d3d_device, &desc, None)?;

            let d2d_factory: ID2D1Factory1 = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
            let d2d_device = d2d_factory.CreateDevice(&dxgi_device)?;
            let d2d_context = d2d_device.CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE)?;
            bind_target(&d2d_context, &swapchain, dpi)?;

            let dcomp_device: IDCompositionDevice = DCompositionCreateDevice(&dxgi_device)?;
            // Only when this renderer owns the window's composition. A detached
            // one leaves the tree to its caller.
            let (dcomp_target, dcomp_visual) = match hwnd {
                Some(hwnd) => {
                    let target = dcomp_device.CreateTargetForHwnd(hwnd, true)?;
                    let visual = dcomp_device.CreateVisual()?;
                    visual.SetContent(&swapchain)?;
                    target.SetRoot(&visual)?;
                    dcomp_device.Commit()?;
                    (Some(target), Some(visual))
                }
                None => (None, None),
            };

            let dwrite: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
            let formats = create_text_formats(&dwrite, font_override)?;
            let images = load_images(&d2d_context)?;

            Ok(Self {
                d2d_context,
                d2d_factory,
                dwrite,
                formats,
                images,
                vector_icons: std::cell::RefCell::new(Default::default()),
                swapchain,
                dcomp_device,
                _dcomp_target: dcomp_target,
                _dcomp_visual: dcomp_visual,
                dpi,
            })
        }
    }

    pub fn resize(&mut self, width_px: u32, height_px: u32, dpi: f32) -> Result<()> {
        self.dpi = dpi;
        unsafe {
            self.d2d_context.SetTarget(None);
            self.swapchain.ResizeBuffers(
                2,
                width_px.max(1),
                height_px.max(1),
                DXGI_FORMAT_B8G8R8A8_UNORM,
                DXGI_SWAP_CHAIN_FLAG(0),
            )?;
            bind_target(&self.d2d_context, &self.swapchain, dpi)?;
            // Commit the tree again so the composition picks up the resized
            // buffer rather than the one it last saw.
            self.dcomp_device.Commit()?;
        }
        Ok(())
    }

    pub fn present(&self) -> Result<()> {
        unsafe { self.swapchain.Present(1, DXGI_PRESENT(0)).ok() }
    }

    /// Exact text width via DirectWrite (like `Painter::measure`, but
    /// available outside a draw pass — used to size breadcrumb segments
    /// tightly before `Layout::compute`).
    pub fn measure_width(&self, text: &str, format: &IDWriteTextFormat) -> f32 {
        let wide: Vec<u16> = text.encode_utf16().collect();
        unsafe {
            match self.dwrite.CreateTextLayout(&wide, format, 65536.0, 65536.0) {
                Ok(layout) => {
                    let mut m = Default::default();
                    if layout.GetMetrics(&mut m).is_ok() {
                        m.widthIncludingTrailingWhitespace
                    } else {
                        0.0
                    }
                }
                Err(_) => 0.0,
            }
        }
    }

    pub fn solid_brush(&self, color: &D2D1_COLOR_F) -> Result<ID2D1SolidColorBrush> {
        unsafe { self.d2d_context.CreateSolidColorBrush(color, None) }
    }

    /// Rebuilds the text formats after `AppThemeFontFamily` changed.
    pub fn rebuild_text_formats(&mut self, font_override: Option<&str>) -> Result<()> {
        self.formats = create_text_formats(&self.dwrite, font_override)?;
        Ok(())
    }

    /// Loads a user image file (AppThemeBackgroundImageSource) through WIC.
    pub fn load_image_file(&self, path: &str) -> Result<ID2D1Bitmap1> {
        unsafe {
            let wic: IWICImagingFactory =
                CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?;
            let decoder = wic.CreateDecoderFromFilename(
                &HSTRING::from(path),
                None,
                windows::Win32::Foundation::GENERIC_READ,
                WICDecodeMetadataCacheOnDemand,
            )?;
            let frame = decoder.GetFrame(0)?;
            let converter = wic.CreateFormatConverter()?;
            converter.Initialize(
                &frame,
                &GUID_WICPixelFormat32bppPBGRA,
                WICBitmapDitherTypeNone,
                None,
                0.0,
                WICBitmapPaletteTypeCustom,
            )?;
            self.d2d_context.CreateBitmapFromWicBitmap(&converter, None)
        }
    }

    /// Installed font family names (AppearanceViewModel's font options).
    pub fn system_font_families(&self) -> Vec<String> {
        let mut names = Vec::new();
        unsafe {
            let mut collection = None;
            if self.dwrite.GetSystemFontCollection(&mut collection, false).is_err() {
                return names;
            }
            let Some(collection) = collection else { return names };
            for i in 0..collection.GetFontFamilyCount() {
                let Ok(family) = collection.GetFontFamily(i) else { continue };
                let Ok(family_names) = family.GetFamilyNames() else { continue };
                let mut index = 0u32;
                let mut exists = windows::core::BOOL::default();
                let _ = family_names.FindLocaleName(windows::core::w!("en-us"), &mut index, &mut exists);
                if !exists.as_bool() {
                    index = 0;
                }
                let Ok(len) = family_names.GetStringLength(index) else { continue };
                let mut buf = vec![0u16; len as usize + 1];
                if family_names.GetString(index, &mut buf).is_ok() {
                    buf.pop();
                    names.push(String::from_utf16_lossy(&buf));
                }
            }
        }
        names.sort_unstable_by_key(|a| a.to_lowercase());
        names.dedup();
        names
    }
}

fn create_d3d_device() -> Result<ID3D11Device> {
    unsafe {
        let mut device: Option<ID3D11Device> = None;
        let flags = D3D11_CREATE_DEVICE_BGRA_SUPPORT;
        let result = D3D11CreateDevice(
            None,
            D3D_DRIVER_TYPE_HARDWARE,
            Default::default(),
            flags,
            None,
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            None,
        );
        if result.is_err() {
            // Fall back to WARP (software) — keeps the app usable in VMs.
            D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_WARP,
                Default::default(),
                flags,
                None,
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                None,
            )?;
        }
        Ok(device.expect("D3D11CreateDevice succeeded but returned no device"))
    }
}

fn bind_target(ctx: &ID2D1DeviceContext, swapchain: &IDXGISwapChain1, dpi: f32) -> Result<()> {
    unsafe {
        let surface: IDXGISurface = swapchain.GetBuffer(0)?;
        let props = D2D1_BITMAP_PROPERTIES1 {
            pixelFormat: D2D1_PIXEL_FORMAT {
                format: DXGI_FORMAT_B8G8R8A8_UNORM,
                alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
            },
            dpiX: dpi,
            dpiY: dpi,
            bitmapOptions: D2D1_BITMAP_OPTIONS_TARGET | D2D1_BITMAP_OPTIONS_CANNOT_DRAW,
            colorContext: std::mem::ManuallyDrop::new(None),
        };
        let bitmap = ctx.CreateBitmapFromDxgiSurface(&surface, Some(&props))?;
        ctx.SetTarget(&bitmap);
        ctx.SetDpi(dpi, dpi);
    }
    Ok(())
}

/// Decodes a PNG (embedded from the original `Files.App/Assets`) into a
/// Direct2D bitmap through WIC.
fn load_png(ctx: &ID2D1DeviceContext, wic: &IWICImagingFactory, bytes: &'static [u8]) -> Result<ID2D1Bitmap1> {
    unsafe {
        let stream = SHCreateMemStream(Some(bytes)).ok_or_else(|| windows::core::Error::from(E_FAIL))?;
        let decoder = wic.CreateDecoderFromStream(&stream, std::ptr::null(), WICDecodeMetadataCacheOnDemand)?;
        let frame = decoder.GetFrame(0)?;
        let converter = wic.CreateFormatConverter()?;
        converter.Initialize(
            &frame,
            &GUID_WICPixelFormat32bppPBGRA,
            WICBitmapDitherTypeNone,
            None,
            0.0,
            WICBitmapPaletteTypeCustom,
        )?;
        ctx.CreateBitmapFromWicBitmap(&converter, None)
    }
}

fn load_images(ctx: &ID2D1DeviceContext) -> Result<Images> {
    let wic: IWICImagingFactory =
        unsafe { CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)? };
    Ok(Images {
        folder: load_png(ctx, &wic, include_bytes!("../assets/FolderIcon.png"))?,
        home: load_png(ctx, &wic, include_bytes!("../assets/fluent-icons/Home.png"))?,
        star: load_png(ctx, &wic, include_bytes!("../assets/fluent-icons/Star.png"))?,
        cloud: load_png(ctx, &wic, include_bytes!("../assets/fluent-icons/CloudDrive.png"))?,
        tags: load_png(ctx, &wic, include_bytes!("../assets/fluent-icons/FileTags.png"))?,
    })
}

/// How the text of a part of a window differs from the application's own — a control's `Font`
/// (family, size, bold, italic) and `RightToLeft`. The default changes nothing.
#[derive(Debug, Clone, PartialEq)]
pub struct TextStyle {
    /// Replaces the UI family (not the icon font).
    pub family: Option<String>,
    /// Multiplies every text size (not the icon sizes); `1.0` keeps them.
    pub scale: f32,
    pub bold: bool,
    pub italic: bool,
    /// Lays the text out right to left (Arabic, Hebrew), leading edge on the right.
    pub right_to_left: bool,
}

impl Default for TextStyle {
    fn default() -> Self {
        Self { family: None, scale: 1.0, bold: false, italic: false, right_to_left: false }
    }
}

fn create_text_formats(dwrite: &IDWriteFactory, font_override: Option<&str>) -> Result<TextFormats> {
    create_text_formats_styled(dwrite, font_override, &TextStyle::default())
}

/// The shared text formats (see [`Renderer::formats`]) restyled by `style` — what a subtree with its
/// own font paints with. `font_override` is the application's own family choice, as for the
/// renderer's formats.
pub fn create_text_formats_styled(dwrite: &IDWriteFactory, font_override: Option<&str>, style: &TextStyle) -> Result<TextFormats> {
    // AppThemeFontFamily: a custom family replaces every Segoe UI Variable
    // optical variant (the original swaps ContentControlThemeFontFamily). The
    // font choice comes from the app (settings) and is injected here — the
    // controls crate doesn't know about `services::settings`.
    let custom: Option<String> = style
        .family
        .as_deref()
        .or(font_override)
        .filter(|f| !f.trim().is_empty())
        .map(str::to_owned);
    let make = |family: &str, weight: DWRITE_FONT_WEIGHT, size: f32| -> Result<IDWriteTextFormat> {
        // A user's chosen font replaces the UI family; otherwise the family
        // passed here — the OS's own "Segoe UI Variable" optical variants —
        // resolves from the SYSTEM collection, so every desktop app reads in the
        // operating system's default typeface (Windows falls back to Segoe UI
        // where the variable family is absent). Icon fonts (Segoe Fluent Icons)
        // always resolve system-wide too.
        let text_font = family.starts_with("Segoe UI Variable");
        let family = match (&custom, text_font) {
            (Some(f), true) => f.as_str(),
            _ => family,
        };
        let (size, weight, font_style) = if text_font {
            let weight = if style.bold && weight.0 < DWRITE_FONT_WEIGHT_BOLD.0 { DWRITE_FONT_WEIGHT_BOLD } else { weight };
            let font_style = if style.italic { DWRITE_FONT_STYLE_ITALIC } else { DWRITE_FONT_STYLE_NORMAL };
            ((size * style.scale).max(1.0), weight, font_style)
        } else {
            (size, weight, DWRITE_FONT_STYLE_NORMAL)
        };
        unsafe {
            let format = dwrite.CreateTextFormat(
                &HSTRING::from(family),
                None,
                weight,
                font_style,
                DWRITE_FONT_STRETCH_NORMAL,
                size,
                &HSTRING::from("fr-FR"),
            )?;
            format.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;
            if style.right_to_left && text_font {
                format.SetReadingDirection(DWRITE_READING_DIRECTION_RIGHT_TO_LEFT)?;
            }
            Ok(format)
        }
    };

    let caption_wrap = {
        let format = make("Segoe UI Variable Small", DWRITE_FONT_WEIGHT_NORMAL, ts::META)?;
        unsafe {
            format.SetWordWrapping(windows::Win32::Graphics::DirectWrite::DWRITE_WORD_WRAPPING_EMERGENCY_BREAK)?;
        }
        format
    };
    let body_wrap = {
        let format = make("Segoe UI Variable Text", DWRITE_FONT_WEIGHT_NORMAL, ts::BODY)?;
        unsafe {
            format.SetWordWrapping(windows::Win32::Graphics::DirectWrite::DWRITE_WORD_WRAPPING_EMERGENCY_BREAK)?;
        }
        format
    };

    // The web allows SIX text sizes and no others (`--kb-text-*`): micro 10.5,
    // meta 11.5, body 13.5, heading 15.5, title 21.5, page 22.5.
    // `MEDIUM` is this system's heaviest step — there is no bold.
    use crate::themes::shape::text as ts;
    const MEDIUM: DWRITE_FONT_WEIGHT = DWRITE_FONT_WEIGHT_MEDIUM;
    Ok(TextFormats {
        caption_wrap,
        body_wrap,
        body: make("Segoe UI Variable Text", DWRITE_FONT_WEIGHT_NORMAL, ts::BODY)?,
        body_small: make("Segoe UI Variable Text", DWRITE_FONT_WEIGHT_NORMAL, ts::META)?,
        body_strong: make("Segoe UI Variable Text", MEDIUM, ts::BODY)?,
        caption: make("Segoe UI Variable Small", DWRITE_FONT_WEIGHT_NORMAL, ts::META)?,
        caption_strong: make("Segoe UI Variable Small", MEDIUM, ts::META)?,
        micro: make("Segoe UI Variable Small", DWRITE_FONT_WEIGHT_NORMAL, ts::MICRO)?,
        subtitle: make("Segoe UI Variable Text", MEDIUM, ts::BODY)?,
        heading: make("Segoe UI Variable Text", MEDIUM, ts::HEADING)?,
        // `text-base font-semibold`: the semi-bold (600) weight of the system UI
        // font. Used for the waffle's « Vos favoris ».
        heading_strong: make("Segoe UI Variable Text", DWRITE_FONT_WEIGHT_SEMI_BOLD, ts::HEADING)?,
        // Titles and page titles are NOT bold in this system; the size carries
        // the hierarchy, not the weight.
        title: make("Segoe UI Variable Display", DWRITE_FONT_WEIGHT_NORMAL, ts::TITLE)?,
        page: make("Segoe UI Variable Display", DWRITE_FONT_WEIGHT_NORMAL, ts::PAGE)?,
        page_admin: make("Segoe UI Variable Display", DWRITE_FONT_WEIGHT_NORMAL, ts::PAGE_ADMIN)?,
        // The chevrons of a ScrollBar's `RepeatButton`: 8 px.
        icon_tiny: make("Segoe Fluent Icons", DWRITE_FONT_WEIGHT_NORMAL, 8.0)?,
        icon_caption: make("Segoe Fluent Icons", DWRITE_FONT_WEIGHT_NORMAL, 10.0)?,
        icon_small: make("Segoe Fluent Icons", DWRITE_FONT_WEIGHT_NORMAL, 12.0)?,
        icon_nav: make("Segoe Fluent Icons", DWRITE_FONT_WEIGHT_NORMAL, 14.0)?,
        icon: make("Segoe Fluent Icons", DWRITE_FONT_WEIGHT_NORMAL, 16.0)?,
        icon_large: make("Segoe Fluent Icons", DWRITE_FONT_WEIGHT_NORMAL, 28.0)?,
    })
}
