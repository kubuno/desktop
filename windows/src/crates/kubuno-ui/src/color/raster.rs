//! Per-pixel rasters for the colour picker: the hue ring (`conic-gradient` on a
//! `rounded-full` box) and the saturation/value area in its three shapes.
//!
//! The web draws the SV area pixel by pixel into a `<canvas>` (`SvArea` in
//! `ColorPicker.tsx`, supersampled ×3 and scaled down by CSS) and lets the
//! browser rasterise the conic ring. Both are reproduced here the same way: a
//! Direct2D bitmap is filled pixel by pixel at the surface's own resolution,
//! each pixel averaging a 3×3 grid of samples — which is also what gives the
//! shapes their anti-aliased edges. The bitmaps are cached per device, size
//! and hue, so a frame that changes nothing rebuilds nothing.
//!
//! Without a Direct2D device (a headless canvas) nothing is drawn and the
//! caller falls back to its strip-based approximation.

use std::cell::RefCell;
use std::collections::HashMap;

use drive_app_controls::{Canvas, Rect};
use windows::Win32::Graphics::Direct2D::Common::{D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_PIXEL_FORMAT, D2D_SIZE_U};
use windows::Win32::Graphics::Direct2D::{ID2D1Bitmap1, D2D1_BITMAP_OPTIONS_NONE, D2D1_BITMAP_PROPERTIES1};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;

use super::hsv_to_rgb;
use super::picker::SvShape;
use crate::graphics::{Graphics, Image};

/// Samples per pixel along each axis — the web's `SS = 3`.
const SS: usize = 3;

/// What a raster shows.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Raster {
    /// The hue ring: an ellipse filling the box, minus an inner ellipse inset
    /// by `ring` on every side, coloured by the angle from the box's centre
    /// (0° up, clockwise), as `conic-gradient(#f00 0deg, … #f00 360deg)` is.
    Ring { ring: f32 },
    /// The saturation/value area at hue `h`, in `shape`. `radius` rounds the
    /// square's corners (`borderRadius: 2`).
    Sv { h: f64, shape: SvShape, radius: f32 },
}

type Key = (usize, u8, u64, u32, u32, u32, u32, u32);

thread_local! {
    static CACHE: RefCell<HashMap<Key, ID2D1Bitmap1>> = RefCell::new(HashMap::new());
}

/// The most bitmaps kept at once; past it the cache starts over.
const CACHE_MAX: usize = 24;

/// Draws `raster` filling `rect` (DIP). Returns `false` when the canvas has
/// no Direct2D device, so the caller can fall back to its own painting.
pub fn draw(c: &dyn Canvas, rect: Rect, raster: Raster) -> bool {
    let Some(renderer) = c.graphics_renderer() else { return false };
    let scale = c.scale().max(0.1);
    let (w, h) = (rect.right - rect.left, rect.bottom - rect.top);
    if w <= 0.0 || h <= 0.0 {
        return true;
    }
    // Snap the bitmap's origin to the device pixel grid so it is drawn 1:1,
    // and remember the sub-pixel offset of the true shape inside it.
    let (lx, ty) = (rect.left * scale, rect.top * scale);
    let (x0, y0) = (lx.floor(), ty.floor());
    let (fx, fy) = (lx - x0, ty - y0);
    let wpx = (w * scale + fx).ceil().max(1.0) as u32;
    let hpx = (h * scale + fy).ceil().max(1.0) as u32;
    let device = windows::core::Interface::as_raw(&renderer.d2d_context) as usize;
    let (kind, a, b) = match raster {
        Raster::Ring { ring } => (0u8, ring.to_bits() as u64, 0u32),
        Raster::Sv { h, shape, radius } => (1 + shape as u8, h.to_bits(), radius.to_bits()),
    };
    let q = |v: f32| (v * 8.0).round() as u32;
    let key: Key = (device, kind, a, b, wpx, hpx, q(fx), q(fy) ^ (w.to_bits() ^ h.to_bits()));
    let cached = CACHE.with(|m| m.borrow().get(&key).cloned());
    let bitmap = match cached {
        Some(b) => b,
        None => {
            let pixels = rasterise(raster, w, h, scale, fx, fy, wpx, hpx);
            let Some(b) = make_bitmap(renderer, &pixels, wpx, hpx, scale) else { return false };
            CACHE.with(|m| {
                let mut m = m.borrow_mut();
                if m.len() >= CACHE_MAX {
                    m.clear();
                }
                m.insert(key, b.clone());
            });
            b
        }
    };
    let dest = Rect::new(x0 / scale, y0 / scale, (x0 + wpx as f32) / scale, (y0 + hpx as f32) / scale);
    Graphics::new(c).draw_image(&Image::from_bitmap(bitmap), dest);
    true
}

fn make_bitmap(renderer: &drive_app_controls::Renderer, pixels: &[u8], w: u32, h: u32, scale: f32) -> Option<ID2D1Bitmap1> {
    let props = D2D1_BITMAP_PROPERTIES1 {
        pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED },
        dpiX: 96.0 * scale,
        dpiY: 96.0 * scale,
        bitmapOptions: D2D1_BITMAP_OPTIONS_NONE,
        ..Default::default()
    };
    // SAFETY: `pixels` holds `w * h` BGRA pixels, `4 * w` bytes per row, and
    // outlives the call, which copies them.
    unsafe {
        renderer
            .d2d_context
            .CreateBitmap(D2D_SIZE_U { width: w, height: h }, Some(pixels.as_ptr() as *const _), w * 4, &props)
            .ok()
    }
}

/// The premultiplied BGRA pixels of `raster` for a `w × h` DIP box whose
/// origin sits `(fx, fy)` device pixels into a `wpx × hpx` bitmap.
#[allow(clippy::too_many_arguments)]
pub fn rasterise(raster: Raster, w: f32, h: f32, scale: f32, fx: f32, fy: f32, wpx: u32, hpx: u32) -> Vec<u8> {
    let mut out = vec![0u8; (wpx * hpx * 4) as usize];
    let (w, h) = (w as f64, h as f64);
    let s = scale as f64;
    let n = (SS * SS) as f64;
    for py in 0..hpx {
        for px in 0..wpx {
            let (mut r, mut g, mut b, mut cov) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
            for sy in 0..SS {
                for sx in 0..SS {
                    // The sample, in DIP relative to the box's own origin.
                    let x = (px as f64 + (sx as f64 + 0.5) / SS as f64 - fx as f64) / s;
                    let y = (py as f64 + (sy as f64 + 0.5) / SS as f64 - fy as f64) / s;
                    if let Some(rgb) = sample(raster, x, y, w, h) {
                        r += rgb.r;
                        g += rgb.g;
                        b += rgb.b;
                        cov += 1.0;
                    }
                }
            }
            if cov == 0.0 {
                continue;
            }
            // Premultiplied: each channel is its mean over the WHOLE pixel
            // (uncovered samples count as transparent black).
            let o = ((py * wpx + px) * 4) as usize;
            let ch = |v: f64| (v / n).round().clamp(0.0, 255.0) as u8;
            out[o] = ch(b);
            out[o + 1] = ch(g);
            out[o + 2] = ch(r);
            out[o + 3] = ((cov / n) * 255.0).round().clamp(0.0, 255.0) as u8;
        }
    }
    out
}

/// The colour at `(x, y)` DIP inside a `w × h` box, or `None` outside the
/// shape.
pub fn sample(raster: Raster, x: f64, y: f64, w: f64, h: f64) -> Option<super::Rgb> {
    if x < 0.0 || y < 0.0 || x >= w || y >= h {
        return None;
    }
    match raster {
        Raster::Ring { ring } => {
            let ring = ring as f64;
            let (cx, cy) = (w / 2.0, h / 2.0);
            let (dx, dy) = (x - cx, y - cy);
            let outer = (dx / cx).powi(2) + (dy / cy).powi(2);
            let (ix, iy) = (cx - ring, cy - ring);
            let inner = if ix > 0.0 && iy > 0.0 { (dx / ix).powi(2) + (dy / iy).powi(2) } else { 2.0 };
            if outer > 1.0 || inner <= 1.0 {
                return None;
            }
            let a = (dx.atan2(-dy).to_degrees() + 360.0) % 360.0;
            Some(hsv_to_rgb(a, 1.0, 1.0))
        }
        Raster::Sv { h: hue, shape, radius } => {
            let (ss, vv) = match shape {
                SvShape::Square => {
                    let r = radius as f64;
                    if r > 0.0 {
                        // Outside a rounded corner.
                        let cx = x.clamp(r, w - r);
                        let cy = y.clamp(r, h - r);
                        if (x - cx).powi(2) + (y - cy).powi(2) > r * r {
                            return None;
                        }
                    }
                    (x / w, 1.0 - y / h)
                }
                SvShape::Circle => {
                    let rr = w / 2.0;
                    if ((x - rr).powi(2) + (y - rr).powi(2)).sqrt() > rr {
                        return None;
                    }
                    (x / w, 1.0 - y / h)
                }
                SvShape::Triangle => {
                    let t = super::picker::triangle(w as f32);
                    let (ww, wh, wb) = super::picker::bary(x, y, t.white, t.hue, t.black);
                    if ww < 0.0 || wh < 0.0 || wb < 0.0 {
                        return None;
                    }
                    let vv = 1.0 - wb;
                    let ss = if ww + wh > 0.0 { wh / (ww + wh) } else { 0.0 };
                    (ss, vv)
                }
            };
            Some(hsv_to_rgb(hue, ss.clamp(0.0, 1.0), vv.clamp(0.0, 1.0)))
        }
    }
}
