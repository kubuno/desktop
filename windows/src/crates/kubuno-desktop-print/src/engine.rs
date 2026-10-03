//! The page surface: a Direct2D device of its own (a detached [`Renderer`], no window), on which
//! each page is recorded into an `ID2D1CommandList` through a [`Graphics`] — the very `Graphics` of
//! EVT-8, drawing through the host's [`Painter`] so the canvas primitives and the `kubuno_desktop_ui` widgets
//! (a control printed with `draw_to_bitmap`) work on paper too. A recorded page is then either handed
//! to the printer ([`crate::xps::SpoolJob`], vector output) or rasterised at the size a preview shows
//! it ([`Surface::rasterize`]).
//!
//! Page coordinates are DIP (1/96 inch) from the physical page's top-left corner, on a light theme
//! (paper is white, whatever the application's theme).

use kubuno_drive_desktop_app_controls::{Renderer, Theme};
use kubuno_desktop_controls::host::Painter;
use kubuno_desktop_controls::{ThemeRenderer, Visuals};
use kubuno_desktop_ui::graphics::Graphics;
use windows::core::Interface;
use windows::Win32::Graphics::Direct2D::Common::{D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_COMPOSITE_MODE_SOURCE_OVER, D2D1_PIXEL_FORMAT, D2D_SIZE_U};
use windows::Win32::Graphics::Direct2D::{
    ID2D1Bitmap1, ID2D1CommandList, ID2D1Device, ID2D1Image, D2D1_BITMAP_OPTIONS_CANNOT_DRAW, D2D1_BITMAP_OPTIONS_CPU_READ, D2D1_BITMAP_OPTIONS_NONE,
    D2D1_BITMAP_OPTIONS_TARGET, D2D1_BITMAP_PROPERTIES1, D2D1_INTERPOLATION_MODE_HIGH_QUALITY_CUBIC, D2D1_MAP_OPTIONS_READ,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows_numerics::Matrix3x2;

use crate::PrintError;

const WHITE: D2D1_COLOR_F = D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 1.0 };

/// A page rasterised for a preview: premultiplied BGRA, `width` × `height` pixels, rows of
/// `width * 4` bytes.
#[derive(Clone)]
pub struct PageBitmap {
    pub width: u32,
    pub height: u32,
    pub pixels: std::rc::Rc<Vec<u8>>,
}

impl std::fmt::Debug for PageBitmap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "PageBitmap({}x{})", self.width, self.height)
    }
}

/// The Direct2D device pages are recorded on (see the module doc).
pub struct Surface {
    renderer: Renderer,
    visuals: Visuals,
    parts: ThemeRenderer,
    theme: Theme,
}

impl Surface {
    /// A surface of its own (a hardware Direct2D device, WARP when there is none).
    pub fn new() -> Result<Self, PrintError> {
        let renderer = Renderer::new_detached(1, 1, 96.0, None).map_err(PrintError::com("Direct2D device"))?;
        let visuals = Visuals::read(&renderer.dwrite, 96.0).map_err(PrintError::com("system visuals"))?;
        Ok(Self { renderer, visuals, parts: ThemeRenderer::new(), theme: Theme::light() })
    }

    /// The Direct2D device (what a print control is created on).
    pub(crate) fn device(&self) -> Result<ID2D1Device, PrintError> {
        // SAFETY: a plain getter on the live context.
        unsafe { self.renderer.d2d_context.GetDevice() }.map_err(PrintError::com("ID2D1DeviceContext::GetDevice"))
    }

    /// Records a page of `size` DIP: `draw` paints it on a [`Graphics`] (DIP from the page's
    /// top-left corner); `device_scale`: device pixels per DIP (the canvas primitives snap to them).
    pub fn record(&self, _size: (f32, f32), device_scale: f32, draw: &mut dyn FnMut(&Graphics<'_>)) -> Result<ID2D1CommandList, PrintError> {
        let ctx = &self.renderer.d2d_context;
        // SAFETY: Direct2D calls on the surface's own context, on this thread only; the command list
        // is the context's target between `BeginDraw` and `EndDraw` and closed after it.
        unsafe {
            let list = ctx.CreateCommandList().map_err(PrintError::com("CreateCommandList"))?;
            let previous = ctx.GetTarget().ok();
            ctx.SetTarget(&list);
            ctx.SetDpi(96.0, 96.0);
            ctx.BeginDraw();
            ctx.SetTransform(&Matrix3x2::identity());
            let result = (|| {
                let painter = Painter::new(&self.renderer, &self.theme, &self.visuals, &self.parts).map_err(PrintError::com("Painter"))?;
                painter.set_scale(device_scale.max(1.0));
                painter.set_ground(WHITE);
                let g = Graphics::new(&painter);
                draw(&g);
                Ok::<(), PrintError>(())
            })();
            let end = ctx.EndDraw(None, None);
            ctx.SetTarget(previous.as_ref());
            result?;
            end.map_err(PrintError::com("EndDraw"))?;
            list.Close().map_err(PrintError::com("ID2D1CommandList::Close"))?;
            Ok(list)
        }
    }

    /// Draws a recorded page of `size` DIP into a `width` × `height` pixel bitmap on white paper and
    /// reads it back (a preview's page, uploaded to the window's own device).
    pub fn rasterize(&self, list: &ID2D1CommandList, size: (f32, f32), width: u32, height: u32) -> Result<PageBitmap, PrintError> {
        let (width, height) = (width.clamp(1, 8192), height.clamp(1, 8192));
        let ctx = &self.renderer.d2d_context;
        let format = D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED };
        // DIP → pixels: the page's width spans the bitmap's.
        let dpi_x = 96.0 * width as f32 / size.0.max(1.0);
        let dpi_y = 96.0 * height as f32 / size.1.max(1.0);
        // SAFETY: Direct2D calls on the surface's own context; the target bitmap is the context's
        // target for the draw only; the mapped memory is read within `Map`/`Unmap`, `pitch` bytes per
        // row as Direct2D reports.
        unsafe {
            let target_props = D2D1_BITMAP_PROPERTIES1 { pixelFormat: format, dpiX: dpi_x, dpiY: dpi_y, bitmapOptions: D2D1_BITMAP_OPTIONS_TARGET, ..Default::default() };
            let target: ID2D1Bitmap1 = ctx.CreateBitmap(D2D_SIZE_U { width, height }, None, 0, &target_props).map_err(PrintError::com("CreateBitmap (target)"))?;
            let previous = ctx.GetTarget().ok();
            ctx.SetTarget(&target);
            ctx.SetDpi(dpi_x, dpi_y);
            ctx.BeginDraw();
            ctx.SetTransform(&Matrix3x2::identity());
            ctx.Clear(Some(&WHITE));
            let image: ID2D1Image = list.cast().map_err(PrintError::com("ID2D1CommandList as ID2D1Image"))?;
            ctx.DrawImage(&image, None, None, D2D1_INTERPOLATION_MODE_HIGH_QUALITY_CUBIC, D2D1_COMPOSITE_MODE_SOURCE_OVER);
            let end = ctx.EndDraw(None, None);
            ctx.SetTarget(previous.as_ref());
            ctx.SetDpi(96.0, 96.0);
            end.map_err(PrintError::com("EndDraw (preview)"))?;
            let read_props = D2D1_BITMAP_PROPERTIES1 {
                pixelFormat: format,
                dpiX: dpi_x,
                dpiY: dpi_y,
                bitmapOptions: D2D1_BITMAP_OPTIONS_CPU_READ | D2D1_BITMAP_OPTIONS_CANNOT_DRAW,
                ..Default::default()
            };
            let readable: ID2D1Bitmap1 = ctx.CreateBitmap(D2D_SIZE_U { width, height }, None, 0, &read_props).map_err(PrintError::com("CreateBitmap (read back)"))?;
            readable.CopyFromBitmap(None, &target, None).map_err(PrintError::com("CopyFromBitmap"))?;
            let mapped = readable.Map(D2D1_MAP_OPTIONS_READ).map_err(PrintError::com("ID2D1Bitmap1::Map"))?;
            let row = width as usize * 4;
            let mut pixels = vec![0u8; row * height as usize];
            for y in 0..height as usize {
                let src = mapped.bits.add(y * mapped.pitch as usize);
                std::ptr::copy_nonoverlapping(src, pixels.as_mut_ptr().add(y * row), row);
            }
            let _ = readable.Unmap();
            Ok(PageBitmap { width, height, pixels: std::rc::Rc::new(pixels) })
        }
    }
}

/// Uploads `page` into a bitmap of `renderer`'s device (a preview drawn in a window).
pub(crate) fn upload(renderer: &Renderer, page: &PageBitmap) -> Option<ID2D1Bitmap1> {
    let props = D2D1_BITMAP_PROPERTIES1 {
        pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED },
        dpiX: 96.0,
        dpiY: 96.0,
        bitmapOptions: D2D1_BITMAP_OPTIONS_NONE,
        ..Default::default()
    };
    // SAFETY: `pixels` holds `height` rows of `width * 4` bytes, the pitch given.
    unsafe {
        renderer
            .d2d_context
            .CreateBitmap(D2D_SIZE_U { width: page.width, height: page.height }, Some(page.pixels.as_ptr().cast()), page.width * 4, &props)
            .map_err(|e| tracing::warn!(target: "kubuno_desktop_print", "a preview page could not be uploaded: {e}"))
            .ok()
    }
}
