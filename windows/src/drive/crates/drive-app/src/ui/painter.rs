#![allow(unused_imports)]
use windows::core::Result;
use windows::Win32::Graphics::Direct2D::Common::{D2D1_COLOR_F, D2D_RECT_F};
use windows::Win32::Graphics::Direct2D::{
    ID2D1Bitmap1, ID2D1DeviceContext, ID2D1SolidColorBrush, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
    D2D1_INTERPOLATION_MODE_LINEAR, D2D1_ROUNDED_RECT,
};
use windows::Win32::Graphics::DirectWrite::{
    IDWriteTextFormat, DWRITE_MEASURING_MODE_NATURAL, DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
    DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_LEADING,
};

use crate::graphics::Renderer;
use crate::services::storage::IconCache;
use crate::data::items::HomeModel;
use crate::view_models::shell_view_model::{Location, Tab, TabGroup};
use crate::styles::theme::Theme;
use super::*;

pub struct Painter<'a> {
    pub ctx: &'a ID2D1DeviceContext,
    pub renderer: &'a Renderer,
    pub theme: &'a Theme,
    brush: ID2D1SolidColorBrush,
    scale: std::cell::Cell<f32>,
}

impl<'a> Painter<'a> {
    pub fn new(renderer: &'a Renderer, theme: &'a Theme) -> Result<Self> {
        let brush = renderer.solid_brush(&theme.text_primary)?;
        Ok(Self {
            ctx: &renderer.d2d_context,
            renderer,
            theme,
            brush,
            scale: std::cell::Cell::new(1.0),
        })
    }

    pub(crate) fn set_color(&self, color: &D2D1_COLOR_F) -> &ID2D1SolidColorBrush {
        unsafe { self.brush.SetColor(color) };
        &self.brush
    }

    pub(crate) fn scale(&self) -> f32 {
        self.scale.get().max(0.01)
    }

    pub(crate) fn set_scale(&self, scale: f32) {
        self.scale.set(scale);
    }

    /// Snaps a DIP value to the physical pixel grid.
    pub(crate) fn px(&self, v: f32) -> f32 {
        let s = self.scale.get().max(0.01);
        (v * s).round() / s
    }

    /// Snaps a DIP value to physical pixel CENTERS (for 1px strokes).
    fn px_center(&self, v: f32) -> f32 {
        let s = self.scale.get().max(0.01);
        ((v * s).round() + 0.5) / s
    }

    fn snap(&self, rect: &Rect) -> Rect {
        Rect::new(self.px(rect.left), self.px(rect.top), self.px(rect.right), self.px(rect.bottom))
    }

    pub(crate) fn fill_rounded(&self, rect: &Rect, radius: f32, color: &D2D1_COLOR_F) {
        let snapped = self.snap(rect);
        let rr = D2D1_ROUNDED_RECT { rect: snapped.d2d(), radiusX: radius, radiusY: radius };
        unsafe { self.ctx.FillRoundedRectangle(&rr, self.set_color(color)) };
    }

    /// Fills a rect whose TOP corners only are rounded — the shape WinUI's
    /// `TopCornerRadiusFilterConverter` produces for a tab's TabContainer.
    pub(crate) fn fill_top_rounded(&self, rect: &Rect, radius: f32, color: &D2D1_COLOR_F) {
        use windows::core::Interface;
        use windows::Win32::Graphics::Direct2D::Common::{
            D2D1_FIGURE_BEGIN_FILLED, D2D1_FIGURE_END_CLOSED, D2D_SIZE_F,
        };
        use windows::Win32::Graphics::Direct2D::{
            D2D1_ARC_SEGMENT, D2D1_ARC_SIZE_SMALL, D2D1_SWEEP_DIRECTION_CLOCKWISE,
        };
        let r = self.snap(rect);
        let done = (|| -> windows::core::Result<()> {
            let factory = self
                .renderer
                .d2d_factory
                .cast::<windows::Win32::Graphics::Direct2D::ID2D1Factory>()?;
            unsafe {
                let geometry = factory.CreatePathGeometry()?;
                let sink = geometry.Open()?;
                let p = |x: f32, y: f32| windows_numerics::Vector2 { X: x, Y: y };
                let arc = |x: f32, y: f32| D2D1_ARC_SEGMENT {
                    point: p(x, y),
                    size: D2D_SIZE_F { width: radius, height: radius },
                    rotationAngle: 0.0,
                    sweepDirection: D2D1_SWEEP_DIRECTION_CLOCKWISE,
                    arcSize: D2D1_ARC_SIZE_SMALL,
                };
                sink.BeginFigure(p(r.left, r.bottom), D2D1_FIGURE_BEGIN_FILLED);
                sink.AddLine(p(r.left, r.top + radius));
                sink.AddArc(&arc(r.left + radius, r.top));
                sink.AddLine(p(r.right - radius, r.top));
                sink.AddArc(&arc(r.right, r.top + radius));
                sink.AddLine(p(r.right, r.bottom));
                sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                sink.Close()?;
                self.ctx.FillGeometry(&geometry, self.set_color(color), None);
            }
            Ok(())
        })()
        .is_ok();
        if !done {
            self.fill_rounded(rect, radius, color);
        }
    }

    /// Fills a rect whose ONLY top-right corner is rounded (the other three
    /// stay sharp) — the Close button hugs the window's rounded corner at
    /// top-right this way, without pulling away from the Maximize button to its left.
    pub(crate) fn fill_top_right_rounded(&self, rect: &Rect, radius: f32, color: &D2D1_COLOR_F) {
        use windows::core::Interface;
        use windows::Win32::Graphics::Direct2D::Common::{
            D2D1_FIGURE_BEGIN_FILLED, D2D1_FIGURE_END_CLOSED, D2D_SIZE_F,
        };
        use windows::Win32::Graphics::Direct2D::{
            D2D1_ARC_SEGMENT, D2D1_ARC_SIZE_SMALL, D2D1_SWEEP_DIRECTION_CLOCKWISE,
        };
        let r = self.snap(rect);
        let done = (|| -> windows::core::Result<()> {
            let factory = self
                .renderer
                .d2d_factory
                .cast::<windows::Win32::Graphics::Direct2D::ID2D1Factory>()?;
            unsafe {
                let geometry = factory.CreatePathGeometry()?;
                let sink = geometry.Open()?;
                let p = |x: f32, y: f32| windows_numerics::Vector2 { X: x, Y: y };
                let arc = D2D1_ARC_SEGMENT {
                    point: p(r.right, r.top + radius),
                    size: D2D_SIZE_F { width: radius, height: radius },
                    rotationAngle: 0.0,
                    sweepDirection: D2D1_SWEEP_DIRECTION_CLOCKWISE,
                    arcSize: D2D1_ARC_SIZE_SMALL,
                };
                // Sharp top-left corner, then arc only at top-right.
                sink.BeginFigure(p(r.left, r.top), D2D1_FIGURE_BEGIN_FILLED);
                sink.AddLine(p(r.right - radius, r.top));
                sink.AddArc(&arc);
                sink.AddLine(p(r.right, r.bottom));
                sink.AddLine(p(r.left, r.bottom));
                sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                sink.Close()?;
                self.ctx.FillGeometry(&geometry, self.set_color(color), None);
            }
            Ok(())
        })()
        .is_ok();
        if !done {
            self.fill_rounded(rect, 0.0, color);
        }
    }

    pub(crate) fn stroke_rounded(&self, rect: &Rect, radius: f32, color: &D2D1_COLOR_F) {
        // Exactly one device pixel, centered on pixel centers: crisp borders
        // at any DPI scale instead of a blurry 2px halo.
        let s = self.scale.get().max(0.01);
        let snapped = Rect::new(
            self.px_center(rect.left),
            self.px_center(rect.top),
            self.px_center(rect.right - 1.0 / s),
            self.px_center(rect.bottom - 1.0 / s),
        );
        let rr = D2D1_ROUNDED_RECT { rect: snapped.d2d(), radiusX: radius, radiusY: radius };
        unsafe { self.ctx.DrawRoundedRectangle(&rr, self.set_color(color), 1.0 / s, None) };
    }

    /// Rounded border at a given DIP thickness, drawn inward — the
    /// `ListViewItemPresenter`'s 2 DIP selection border
    /// (`GridViewItemSelectedBorderThickness`).
    pub(crate) fn stroke_rounded_w(&self, rect: &Rect, radius: f32, color: &D2D1_COLOR_F, width: f32) {
        let inset = width / 2.0;
        let r = Rect::new(rect.left + inset, rect.top + inset, rect.right - inset, rect.bottom - inset);
        let rr = D2D1_ROUNDED_RECT { rect: r.d2d(), radiusX: radius - inset, radiusY: radius - inset };
        unsafe { self.ctx.DrawRoundedRectangle(&rr, self.set_color(color), width, None) };
    }


    pub(crate) fn text_aligned(
        &self,
        text: &str,
        rect: &Rect,
        format: &IDWriteTextFormat,
        color: &D2D1_COLOR_F,
        alignment: windows::Win32::Graphics::DirectWrite::DWRITE_TEXT_ALIGNMENT,
    ) {
        unsafe {
            let _ = format.SetTextAlignment(alignment);
            let _ = format.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
            let wide: Vec<u16> = text.encode_utf16().collect();
            // Pixel-snapped layout rect: stable baselines, no half-pixel blur.
            let snapped = self.snap(rect);
            self.ctx.DrawText(
                &wide,
                format,
                &snapped.d2d(),
                self.set_color(color),
                windows::Win32::Graphics::Direct2D::D2D1_DRAW_TEXT_OPTIONS_CLIP,
                DWRITE_MEASURING_MODE_NATURAL,
            );
        }
    }

    /// Draws an image bitmap as a square icon centered in `rect`, snapped to
    /// the physical pixel grid at its native fetched size (no resampling).
    pub(crate) fn image(&self, bitmap: &ID2D1Bitmap1, rect: &Rect, size: f32) {
        self.image_alpha(bitmap, rect, size, 1.0);
    }

    pub(crate) fn image_alpha(&self, bitmap: &ID2D1Bitmap1, rect: &Rect, size: f32, alpha: f32) {
        let s = self.scale.get().max(0.01);
        let size_px = (size * s).round();
        let cx = (rect.left + rect.right) / 2.0;
        let cy = (rect.top + rect.bottom) / 2.0;
        let left = ((cx * s) - size_px / 2.0).round() / s;
        let top = ((cy * s) - size_px / 2.0).round() / s;
        let dest = Rect::new(left, top, left + size_px / s, top + size_px / s);
        unsafe {
            self.ctx.DrawBitmap(
                bitmap,
                Some(&dest.d2d()),
                alpha,
                windows::Win32::Graphics::Direct2D::D2D1_INTERPOLATION_MODE_HIGH_QUALITY_CUBIC,
                None,
                None,
            );
        }
    }

    /// Draws the raw BGRA pixels the shell handed us for one of its own menu
    /// entries. Menus are short-lived and hold a couple of dozen 16×16 icons,
    /// so the upload happens per paint rather than through the icon cache
    /// (which is keyed by path and lives on the main window's device).
    pub(crate) fn shell_bitmap(&self, bitmap: &drive_app_storage::ShellBitmap, rect: &Rect) {
        use windows::Win32::Graphics::Direct2D::Common::{
            D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_PIXEL_FORMAT,
        };
        use windows::Win32::Graphics::Direct2D::{
            D2D1_BITMAP_OPTIONS_NONE, D2D1_BITMAP_PROPERTIES1,
        };
        use windows::Win32::Graphics::Direct2D::Common::D2D_SIZE_U;
        use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;

        let props = D2D1_BITMAP_PROPERTIES1 {
            pixelFormat: D2D1_PIXEL_FORMAT {
                format: DXGI_FORMAT_B8G8R8A8_UNORM,
                alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
            },
            dpiX: 96.0,
            dpiY: 96.0,
            bitmapOptions: D2D1_BITMAP_OPTIONS_NONE,
            colorContext: std::mem::ManuallyDrop::new(None),
        };
        unsafe {
            let Ok(d2d) = self.ctx.CreateBitmap(
                D2D_SIZE_U { width: bitmap.width, height: bitmap.height },
                Some(bitmap.bgra.as_ptr() as *const _),
                bitmap.width * 4,
                &props,
            ) else {
                return;
            };
            self.image(&d2d, rect, 16.0);
        }
    }

    pub(crate) fn text(&self, text: &str, rect: &Rect, format: &IDWriteTextFormat, color: &D2D1_COLOR_F, centered: bool) {
        self.text_aligned(
            text,
            rect,
            format,
            color,
            if centered { DWRITE_TEXT_ALIGNMENT_CENTER } else { DWRITE_TEXT_ALIGNMENT_LEADING },
        );
    }

    /// Like `text`, but trimmed with an ELLIPSIS (« … ») at the right edge
    /// instead of cutting glyphs — the original's `TextTrimming.CharacterEllipsis`
    /// TextBlocks (sidebar, file names).
    pub(crate) fn text_ellipsis(
        &self,
        text: &str,
        rect: &Rect,
        format: &IDWriteTextFormat,
        color: &D2D1_COLOR_F,
    ) {
        use windows::Win32::Graphics::DirectWrite::{
            DWRITE_TRIMMING, DWRITE_TRIMMING_GRANULARITY_CHARACTER,
            DWRITE_WORD_WRAPPING_NO_WRAP,
        };
        unsafe {
            let _ = format.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_LEADING);
            let _ = format.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
            let wide: Vec<u16> = text.encode_utf16().collect();
            let snapped = self.snap(rect);
            let Ok(layout) = self.renderer.dwrite.CreateTextLayout(
                &wide,
                format,
                (snapped.right - snapped.left).max(1.0),
                (snapped.bottom - snapped.top).max(1.0),
            ) else {
                return;
            };
            let _ = layout.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP);
            let trimming = DWRITE_TRIMMING {
                granularity: DWRITE_TRIMMING_GRANULARITY_CHARACTER,
                delimiter: 0,
                delimiterCount: 0,
            };
            if let Ok(sign) = self.renderer.dwrite.CreateEllipsisTrimmingSign(&layout) {
                let _ = layout.SetTrimming(&trimming, &sign);
            }
            self.ctx.DrawTextLayout(
                windows_numerics::Vector2 { X: snapped.left, Y: snapped.top },
                &layout,
                self.set_color(color),
                windows::Win32::Graphics::Direct2D::D2D1_DRAW_TEXT_OPTIONS_CLIP,
            );
        }
    }

    /// Like `text_ellipsis`, but the text is horizontally CENTERED — the
    /// original's centered TextBlocks with `TextAlignment="Center"` +
    /// `TextTrimming="CharacterEllipsis"`. Alignment is set on the LAYOUT,
    /// not the shared format, so it doesn't bleed into other calls.
    pub(crate) fn text_ellipsis_center(
        &self,
        text: &str,
        rect: &Rect,
        format: &IDWriteTextFormat,
        color: &D2D1_COLOR_F,
    ) {
        use windows::Win32::Graphics::DirectWrite::{
            DWRITE_TRIMMING, DWRITE_TRIMMING_GRANULARITY_CHARACTER,
            DWRITE_WORD_WRAPPING_NO_WRAP,
        };
        unsafe {
            let _ = format.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_LEADING);
            let _ = format.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
            let wide: Vec<u16> = text.encode_utf16().collect();
            let snapped = self.snap(rect);
            let Ok(layout) = self.renderer.dwrite.CreateTextLayout(
                &wide,
                format,
                (snapped.right - snapped.left).max(1.0),
                (snapped.bottom - snapped.top).max(1.0),
            ) else {
                return;
            };
            let _ = layout.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP);
            // Horizontal centering on the layout alone: the format keeps its
            // alignment, only these glyphs are centered.
            let _ = layout.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER);
            let trimming = DWRITE_TRIMMING {
                granularity: DWRITE_TRIMMING_GRANULARITY_CHARACTER,
                delimiter: 0,
                delimiterCount: 0,
            };
            if let Ok(sign) = self.renderer.dwrite.CreateEllipsisTrimmingSign(&layout) {
                let _ = layout.SetTrimming(&trimming, &sign);
            }
            self.ctx.DrawTextLayout(
                windows_numerics::Vector2 { X: snapped.left, Y: snapped.top },
                &layout,
                self.set_color(color),
                windows::Win32::Graphics::Direct2D::D2D1_DRAW_TEXT_OPTIONS_CLIP,
            );
        }
    }

    /// The equivalent of a `TextBlock` with `TextWrapping="Wrap"` + `MaxLines` +
    /// `TextTrimming="CharacterEllipsis"`: the text wraps over at most
    /// `max_lines` lines and, if the rest doesn't fit, the last kept line
    /// ends with « … » (tile names in Grid mode, etc.).
    pub(crate) fn text_wrap_ellipsis(
        &self,
        text: &str,
        rect: &Rect,
        format: &IDWriteTextFormat,
        color: &D2D1_COLOR_F,
        max_lines: usize,
        centered: bool,
    ) {
        use windows::Win32::Graphics::DirectWrite::{
            IDWriteTextLayout, DWRITE_LINE_METRICS, DWRITE_PARAGRAPH_ALIGNMENT_NEAR,
        };
        let max_lines = max_lines.max(1);
        let snapped = self.snap(rect);
        let width = (snapped.right - snapped.left).max(1.0);
        let height = (snapped.bottom - snapped.top).max(1.0);
        let alignment =
            if centered { DWRITE_TEXT_ALIGNMENT_CENTER } else { DWRITE_TEXT_ALIGNMENT_LEADING };
        unsafe {
            // Trial layout: rect width, giant but FINITE height
            // (65536.0 — NEVER `f32::MAX`, which silently yields null
            // metrics) to count the lines of the full wrap.
            // Alignment lives on the LAYOUT: the shared format stays intact;
            // the text flows from the top, like the wrapped TextBlock.
            let make_layout = |t: &str| -> Option<IDWriteTextLayout> {
                let wide: Vec<u16> = t.encode_utf16().collect();
                let layout =
                    self.renderer.dwrite.CreateTextLayout(&wide, format, width, 65536.0).ok()?;
                let _ = layout.SetTextAlignment(alignment);
                let _ = layout.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_NEAR);
                Some(layout)
            };
            // Two calls to `GetLineMetrics`: the count, then the fill.
            let line_metrics = |layout: &IDWriteTextLayout| -> Vec<DWRITE_LINE_METRICS> {
                let mut count = 0u32;
                let _ = layout.GetLineMetrics(None, &mut count);
                let mut lines = vec![DWRITE_LINE_METRICS::default(); count.max(1) as usize];
                if layout.GetLineMetrics(Some(&mut lines), &mut count).is_err() {
                    lines.clear();
                }
                lines
            };
            let Some(layout) = make_layout(text) else {
                return;
            };
            let lines = line_metrics(&layout);
            let final_layout = if lines.len() <= max_lines {
                layout
            } else {
                // Too many lines: keep the text of the first `max_lines`
                // (lengths in UTF-16 units, converted at `char` boundaries),
                // then strip characters until « text + … » fits within
                // `max_lines` lines.
                let keep_utf16: usize =
                    lines.iter().take(max_lines).map(|l| l.length as usize).sum();
                let mut keep_bytes = 0usize;
                let mut units = 0usize;
                for (idx, ch) in text.char_indices() {
                    if units + ch.len_utf16() > keep_utf16 {
                        break;
                    }
                    units += ch.len_utf16();
                    keep_bytes = idx + ch.len_utf8();
                }
                let mut kept: String = text[..keep_bytes].trim_end().to_string();
                loop {
                    let candidate = format!("{kept}…");
                    if let Some(l) = make_layout(&candidate) {
                        if line_metrics(&l).len() <= max_lines {
                            break l;
                        }
                    }
                    if kept.pop().is_none() {
                        return;
                    }
                    // No bare whitespace right before the ellipsis.
                    while kept.ends_with(char::is_whitespace) {
                        kept.pop();
                    }
                }
            };
            // For drawing, bring the layout box back to the rect's real
            // height (the trial height was giant): `CLIP` then bounds the
            // glyphs to the requested rect.
            let _ = final_layout.SetMaxHeight(height);
            self.ctx.DrawTextLayout(
                windows_numerics::Vector2 { X: snapped.left, Y: snapped.top },
                &final_layout,
                self.set_color(color),
                windows::Win32::Graphics::Direct2D::D2D1_DRAW_TEXT_OPTIONS_CLIP,
            );
        }
    }

    /// The sidebar's resize handle, ported from the shell
    /// (`AppSidebar.tsx:356-382`) — the same element, so the same rules.
    ///
    /// It MATERIALISES ON APPROACH: at rest the gutter is empty (the line is
    /// `bg-transparent`, the grip `opacity-0`). Pointing at the rail fades in a
    /// 5 DIP `--color-border` line and a white pill (36×14, `rounded-full`,
    /// 1 px border) holding six `GripVertical` dots. While the pane is being
    /// dragged the line goes full `bg-primary` and the pill becomes
    /// `bg-primary-light text-primary border-primary/40`.
    ///
    /// (The reusable `@ui/ResizeHandle` is the always-visible variant, at
    /// `opacity-80`; the sidebar deliberately uses this discreet one.)
    pub(crate) fn draw_pane_resizer(&self, rail: &Rect, hot: bool, dragging: bool) {
        if !hot && !dragging {
            return;
        }
        const BAR_W: f32 = 5.0;
        const PILL_W: f32 = 14.0;
        const PILL_H: f32 = 36.0;
        const DOT_R: f32 = 1.1;

        let t = self.theme;
        let cx = (rail.left + rail.right) / 2.0;
        let cy = (rail.top + rail.bottom) / 2.0;

        // The line: `bg-border` on approach, full `bg-primary` while dragging.
        let bar = Rect::new(cx - BAR_W / 2.0, rail.top, cx + BAR_W / 2.0, rail.bottom);
        let bar_color = if dragging { t.accent } else { t.card_stroke };
        self.fill_rounded(&bar, BAR_W / 2.0, &bar_color);

        let pill =
            Rect::new(cx - PILL_W / 2.0, cy - PILL_H / 2.0, cx + PILL_W / 2.0, cy + PILL_H / 2.0);
        let (fill, stroke, ink) = if dragging {
            (t.accent_light, fade(&t.accent, 0.40), t.accent)
        } else {
            (t.layer_background, t.card_stroke, t.text_tertiary)
        };
        self.fill_rounded(&pill, PILL_W / 2.0, &fill);
        self.stroke_rounded(&pill, PILL_W / 2.0, &stroke);

        // `GripVertical`: two columns of three dots.
        for col in [-1.0f32, 1.0] {
            for row in [-1.0f32, 0.0, 1.0] {
                let dx = cx + col * 2.0;
                let dy = cy + row * 4.0;
                self.fill_rounded(
                    &Rect::new(dx - DOT_R, dy - DOT_R, dx + DOT_R, dy + DOT_R),
                    DOT_R,
                    &ink,
                );
            }
        }
    }

    /// The project's `Toggle`, painted from `@ui/toggleCanvas.ts` (`md`
    /// geometry) so every switch in the app matches the web's to the pixel:
    /// a 36×20 ROUNDED RECT (radius 6, not a pill), a 14×14 thumb with radius
    /// 4 inset by 3, and — the detail that was wrong here — a thumb that is
    /// ALWAYS WHITE over a FILLED track, off included.
    ///
    /// `origin` is the track's top-left corner. Returns the track rect so the
    /// caller can place its label after it.
    pub(crate) fn draw_toggle(&self, origin: (f32, f32), on: bool, enabled: bool) -> Rect {
        // One implementation, in the shared controls: the desktop shell draws
        // the very same switch, so the two can never drift apart.
        drive_app_controls::switch::draw(self, origin, on, enabled)
    }

    /// Draws the open tooltip (`ToolTipService.ToolTip`): a rounded pill
    /// placed under the pointer (`Placement=Mouse`), bounded to the window.
    ///
    /// `anchor` is the pointer position in DIP; `win` the window size
    /// in DIP, so the pill never overflows.
    pub(crate) fn draw_tooltip(&self, text: &str, anchor: (f32, f32), win: (f32, f32)) {
        let f = &self.renderer.formats;
        let t = self.theme;
        // `TOOLTIP_STYLE` verbatim: padding 6/10, 12px text on a 16px line
        // (→ 28 tall), radius 4, no border, capped at 280 wide.
        const PAD_X: f32 = 10.0;
        const PAD_Y: f32 = 6.0;
        const LINE: f32 = 16.0;
        const MAX_WIDTH: f32 = 280.0;
        let height = LINE + PAD_Y * 2.0;
        let tw = self.measure(text, &f.caption_strong).ceil();
        let width = (tw + PAD_X * 2.0).min(MAX_WIDTH);
        // Anchored to the POINTER: below it and left-aligned with it, flipping
        // above when the bottom edge is too close (`tooltipPlacement`).
        let mut left = anchor.0;
        let mut top = anchor.1 + 18.0;
        if left + width > win.0 - 2.0 {
            left = (win.0 - 2.0 - width).max(2.0);
        }
        if top + height > win.1 - 2.0 {
            top = (anchor.1 - height - 4.0).max(2.0);
        }
        let rect = Rect::new(left, top, left + width, top + height);
        self.draw_card_shadow(&rect, 4.0);
        self.fill_rounded(&rect, 4.0, &t.tooltip_background);
        let text_rect =
            Rect::new(rect.left + PAD_X, rect.top + PAD_Y, rect.right - PAD_X, rect.bottom - PAD_Y);
        self.text_ellipsis(text, &text_rect, &f.caption_strong, &t.tooltip_foreground);
    }

    /// The ghost of an item drag: target highlight + a count pill and a
    /// « Déplacer/Copier vers X » caption at the pointer (companion to the
    /// `Item_DragOver` highlight + `DragUIOverride`).
    pub(crate) fn draw_item_drag(&self, drag: &DragVisual, win: (f32, f32)) {
        let f = &self.renderer.formats;
        let t = self.theme;

        // Drop target highlight (border + accent veil, radius 4).
        if let Some(rect) = &drag.target_rect {
            let mut fill = t.accent;
            fill.a = 0.12;
            self.fill_rounded(rect, 4.0, &fill);
            self.stroke_rounded(rect, 4.0, &t.accent);
        }

        // Caption at the pointer (same tones as the tooltip).
        let pad_x = 9.0;
        let height = 26.0;
        let tw = self.measure(&drag.caption, &f.body).ceil();
        // Count pill to the left of the caption when dragging >1 item.
        let badge = if drag.count > 1 { 22.0 } else { 0.0 };
        let width = tw + pad_x * 2.0 + badge;
        let mut left = drag.cursor.0 + 16.0;
        let mut top = drag.cursor.1 + 18.0;
        if left + width > win.0 - 2.0 {
            left = (win.0 - 2.0 - width).max(2.0);
        }
        if top + height > win.1 - 2.0 {
            top = (drag.cursor.1 - height - 4.0).max(2.0);
        }
        let rect = Rect::new(left, top, left + width, top + height);
        self.draw_card_shadow(&rect, 4.0);
        self.fill_rounded(&rect, 4.0, &t.tooltip_background);
        if badge > 0.0 {
            let b = Rect::new(rect.left + 4.0, rect.top + 4.0, rect.left + 4.0 + 18.0, rect.bottom - 4.0);
            self.fill_rounded(&b, 9.0, &t.accent);
            self.text(&drag.count.to_string(), &b, &f.caption, &t.accent_foreground, true);
        }
        let text_rect = Rect::new(rect.left + pad_x + badge, rect.top + 5.0, rect.right - pad_x, rect.bottom);
        self.text(&drag.caption, &text_rect, &f.caption_strong, &t.tooltip_foreground, false);
    }

    /// Draws one of the original ThemedIcon vector geometries, centered.
    /// MONOCHROME ThemedIcon: ALL layers (`@base`, `@alt`, `@accent`,
    /// `@accentcontrast`) painted in the SAME `color`, respecting each
    /// layer's opacity — this is an AppBarButton's « Outline » rendering
    /// (the icon isn't tinted). Painting only the `@base` layer gave just a
    /// FRAGMENT (the scissor blades for Cut, the rectangle's back for
    /// Copy…): all layers must be composited.
    pub(crate) fn vector_icon(&self, name: &'static str, rect: &Rect, size: f32, color: &D2D1_COLOR_F) {
        use windows::core::Interface;
        let Ok(factory) = self.renderer.d2d_factory.cast::<windows::Win32::Graphics::Direct2D::ID2D1Factory>() else {
            return;
        };
        let mut icons = self.renderer.vector_icons.borrow_mut();
        let stroke_style = icons.stroke_style(&factory).cloned();
        let Some((layers, viewbox)) = icons.get_layers(&factory, name) else {
            return;
        };
        let k = size / viewbox;
        let left = self.px((rect.left + rect.right) / 2.0 - size / 2.0);
        let top = self.px((rect.top + rect.bottom) / 2.0 - size / 2.0);
        unsafe {
            for layer in layers {
                // The layer's group transform composed with the icon's scale and
                // placement, so the path data stays exactly as extracted.
                let [a, b, c2, d, e, f2] = layer.transform;
                let transform = windows_numerics::Matrix3x2 {
                    M11: a * k,
                    M12: b * k,
                    M21: c2 * k,
                    M22: d * k,
                    M31: e * k + left,
                    M32: f2 * k + top,
                };
                self.ctx.SetTransform(&transform);
                // A fixed brand colour (module logo) wins over the caller's.
                let c = match layer.color {
                    Some(brand) => brand,
                    None => D2D1_COLOR_F { a: color.a * layer.opacity, ..*color },
                };
                let brush = self.set_color(&c);
                match layer.stroke {
                    // Outlined (Lucide): width is in design units and scales
                    // with the geometry through the same matrix.
                    Some(width) => {
                        self.ctx.DrawGeometry(&layer.geometry, brush, width, stroke_style.as_ref())
                    }
                    None => self.ctx.FillGeometry(&layer.geometry, brush, None),
                }
            }
            self.ctx.SetTransform(&windows_numerics::Matrix3x2::identity());
        }
    }

    /// MULTI-LAYER ThemedIcon: each `ThemedIconLayer` is painted with its
    /// role color — foreground (`Base`/`Alt`), accent (`Accent`, blue) or
    /// contrast (`AccentContrast`, e.g. the white checkmark on the blue
    /// badge). This is the « treatment » that tints part of the icon.
    pub(crate) fn vector_icon_layered(
        &self,
        name: &'static str,
        rect: &Rect,
        size: f32,
        fg: &D2D1_COLOR_F,
        accent: &D2D1_COLOR_F,
    ) {
        use drive_app_controls::LayerRole;
        use windows::core::Interface;
        let Ok(factory) = self.renderer.d2d_factory.cast::<windows::Win32::Graphics::Direct2D::ID2D1Factory>() else {
            return;
        };
        // `AccentContrast` = the color that contrasts with the accent: white.
        let contrast = D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 1.0 };
        let mut icons = self.renderer.vector_icons.borrow_mut();
        let Some((layers, viewbox)) = icons.get_layers(&factory, name) else {
            return;
        };
        let k = size / viewbox;
        let left = self.px((rect.left + rect.right) / 2.0 - size / 2.0);
        let top = self.px((rect.top + rect.bottom) / 2.0 - size / 2.0);
        let transform = windows_numerics::Matrix3x2 { M11: k, M12: 0.0, M21: 0.0, M22: k, M31: left, M32: top };
        unsafe {
            self.ctx.SetTransform(&transform);
            for layer in layers {
                let base = match layer.role {
                    LayerRole::Base => *fg,
                    // `@alt` = the cutouts/hollows: INVERSE color at 40%
                    // (ThemedIconAltColor), not the solid foreground.
                    LayerRole::Alt => self.theme.icon_alt,
                    LayerRole::Accent => *accent,
                    LayerRole::AccentContrast => contrast,
                };
                let color = D2D1_COLOR_F { a: base.a * layer.opacity, ..base };
                self.ctx.FillGeometry(&layer.geometry, self.set_color(&color), None);
            }
            self.ctx.SetTransform(&windows_numerics::Matrix3x2::identity());
        }
    }

    /// Exact text width via DirectWrite (for caret/selection positioning).
    pub fn measure(&self, text: &str, format: &IDWriteTextFormat) -> f32 {
        let wide: Vec<u16> = text.encode_utf16().collect();
        unsafe {
            // A FINITE bound: with `f32::MAX`, DirectWrite « succeeds » but
            // yields null metrics depending on the call context.
            match self.renderer.dwrite.CreateTextLayout(&wide, format, 65536.0, 65536.0) {
                Ok(layout) => {
                    let mut metrics = Default::default();
                    match layout.GetMetrics(&mut metrics) {
                        Ok(()) => metrics.widthIncludingTrailingWhitespace,
                        Err(e) => {
                            tracing::warn!("measure GetMetrics failed: {e}");
                            0.0
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("measure CreateTextLayout failed: {e}");
                    0.0
                }
            }
        }
    }

    /// Height of the text wrapped at `width` DIP (for the Details pane's
    /// variable-height rows — the original's TextWrapping="Wrap").
    pub fn measure_height(&self, text: &str, format: &IDWriteTextFormat, width: f32) -> f32 {
        let wide: Vec<u16> = text.encode_utf16().collect();
        unsafe {
            self.renderer
                .dwrite
                .CreateTextLayout(&wide, format, width.max(1.0), 65536.0)
                .ok()
                .and_then(|layout| {
                    let mut metrics = Default::default();
                    layout.GetMetrics(&mut metrics).ok()?;
                    Some(metrics.height)
                })
                .unwrap_or(0.0)
        }
    }

    /// Looks up a cached shell icon; sizes are in DIP, cache keys in pixels.
    pub(crate) fn shell_icon<'b>(&self, icons: &'b IconCache, path: &str, size_dip: f32, scale: f32) -> Option<&'b ID2D1Bitmap1> {
        icons.get(path, (size_dip * scale).round() as i32)
    }

    #[allow(clippy::too_many_arguments)] // a paint entry point: the frame's inputs, passed flat
    pub fn draw(
        &self,
        layout: &Layout,
        state: &UiState,
        model: &HomeModel,
        icons: &IconCache,
        dpi: f32,
        ops: &[String],
        bg_image: Option<&ID2D1Bitmap1>,
    ) {
        let scale = dpi / 96.0;
        self.scale.set(scale);
        unsafe {
            self.ctx.SetAntialiasMode(D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
            self.ctx.SetTextAntialiasMode(
                windows::Win32::Graphics::Direct2D::D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE,
            );
        }
        let t = self.theme;

        // App.Theme.BackgroundBrush: the user's background tint over the
        // backdrop, then the optional background image (AppearancePage).
        let appearance = crate::services::settings::get();
        if let Some(c) = crate::services::settings::parse_color(&appearance.app_theme_background_color) {
            if c.a > 0.0 {
                let full = Rect::new(0.0, 0.0, layout.width, layout.height);
                self.fill_rounded(&full, 0.0, &c);
            }
        }
        if let Some(bitmap) = bg_image {
            self.draw_background_image(bitmap, layout, &appearance);
        }

        // A SINGLE translucent base under the whole area (excluding the
        // tab strip / address bar): this is `App.Theme.Sidebar.BackgroundBrush`'s
        // `LayerOnMicaBaseAltFillColorDefault`. In the original, this same
        // tint carries the sidebar, the gutter AND the underside of the
        // cards — there's no dark band between the sidebar and the
        // content. The cards (commands, files) sit LIGHTER on top
        // (`CardBackgroundFillColor…`), hence the relief.
        {
            let base = Rect::new(0.0, TAB_BAR_HEIGHT + TOOLBAR_HEIGHT, layout.width, layout.height);
            self.fill_rounded(&base, 0.0, &t.layer_fill);
        }

        // Content CARD (CornerRadius=8, separated from the other blocks).
        let panel = if let Some(other) = &layout.other_pane_rect {
            // Dual pane: one card spanning both panes.
            Rect::new(
                layout.content.left.min(other.left),
                layout.content.top,
                layout.content.right.max(other.right),
                layout.content.bottom,
            )
        } else {
            layout.content
        };
        // FLAT surface, no drop shadow: the web separates the content panel
        // from the page with its rounded white fill alone (`.rounded-xl` over
        // `--body-bg`). A shadow here read as a floating Fluent card.
        self.fill_rounded(&panel, drive_app_controls::themes::shape::radius::XL, &t.layer_background);

        // The toolbar draws FIRST: it fills the strip + active tab as one
        // union surface that the tab bar then annotates (title, buttons).
        self.draw_toolbar(layout, state);
        self.draw_tab_bar(layout, state, scale);

        // Clip scrolled content to the viewport.
        unsafe {
            self.ctx.PushAxisAlignedClip(&layout.content.d2d(), D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
        }
        match &state.active().location {
            Location::Home => self.draw_home(layout, state, model, icons, scale),
            Location::Dir(_) | Location::RecycleBin | Location::SearchResults { .. } => self.draw_files(layout, state, icons, scale),
            Location::Settings => self.draw_settings(layout, state),
        }
        unsafe {
            self.ctx.PopAxisAlignedClip();
        }

        // The `ScrollBar` sits ON the content, in the gutter.
        if let Some(bar) = &layout.scrollbar {
            let alpha = scrollbar_alpha(state);
            if alpha > 0.0 {
                drive_app_controls::scrollbar::draw(self, 
                    bar,
                    alpha,
                    state.hot == Some(Hot::ScrollThumb) || state.scroll_drag.is_some(),
                );
            }
        }

        self.draw_other_pane(layout, state, icons, scale);
        self.draw_info_pane(layout, state, icons, scale);
        self.draw_shelf_pane(layout, state, icons, scale);
        self.draw_cmdbar(layout, state);
        self.draw_statusbar(layout, state);
        // The sidebar draws AFTER the content and command bar: in docked
        // mode (Compact/Expanded) its column is disjoint from the content, so
        // the order doesn't matter; in Minimal mode, its FLOATING pane must
        // go ABOVE the content (otherwise the cards would cover it).
        self.draw_sidebar(layout, state, model);
        // The resize rails sit in the gutters BETWEEN panes, so they are drawn
        // after both sides — nothing may cover the grip.
        if let Some(rail) = &layout.sidebar_resizer {
            let hot = state.hot == Some(Hot::SidebarResizer);
            self.draw_pane_resizer(rail, hot, state.pane_dragging == Some(Hot::SidebarResizer));
        }
        if let Some(rail) = &layout.info_resizer {
            let hot = state.hot == Some(Hot::InfoPaneResizer);
            self.draw_pane_resizer(rail, hot, state.pane_dragging == Some(Hot::InfoPaneResizer));
        }
        self.draw_status_center(layout, state, ops);
        // Over the content, like a real flyout.
        self.draw_omnibar_suggestions(layout, state);
        // The `ContentDialog`, ON TOP of everything (modal).
        if let Some(dialog) = &state.dialog {
            self.draw_dialog(
                dialog,
                state.edit.as_ref().filter(|e| e.entry == EDIT_DIALOG),
                layout.width,
                layout.height,
                state.hot == Some(Hot::DialogPrimary),
                state.hot == Some(Hot::DialogClose),
                match state.hot {
                    Some(Hot::DialogItem(i)) => Some(i),
                    _ => None,
                },
            );
        }
        // The conflict dialog, modal over everything (after `dialog`).
        if let Some(conflict) = &state.conflict {
            self.draw_conflict_dialog(conflict, layout.width, layout.height, state.conflict_scroll, state.hot);
        }
        // The flyout is no longer drawn in-window: it lives in its own acrylic
        // popup window (controls::flyout_window), so it can extend past the app
        // and get DWM's real blur + shadow.
    }

    /// Background image with the original's Opacity / ImageFit / alignment
    /// settings (AppearancePage "Image de fond").
    fn draw_background_image(&self, bitmap: &ID2D1Bitmap1, layout: &Layout, s: &crate::services::settings::AppSettings) {
        use crate::services::settings::{ImageFit, ImageHorizontalAlignment, ImageVerticalAlignment};
        let size = unsafe { bitmap.GetSize() };
        if size.width <= 0.0 || size.height <= 0.0 {
            return;
        }
        let (cw, ch) = (layout.width, layout.height);
        let (dw, dh) = match s.app_theme_background_image_fit {
            ImageFit::None => (size.width, size.height),
            ImageFit::Fill => (cw, ch),
            ImageFit::Uniform => {
                let k = (cw / size.width).min(ch / size.height);
                (size.width * k, size.height * k)
            }
            ImageFit::UniformToFill => {
                let k = (cw / size.width).max(ch / size.height);
                (size.width * k, size.height * k)
            }
        };
        let x = match s.app_theme_background_image_horizontal_alignment {
            ImageHorizontalAlignment::Left => 0.0,
            ImageHorizontalAlignment::Center => (cw - dw) / 2.0,
            ImageHorizontalAlignment::Right => cw - dw,
        };
        let y = match s.app_theme_background_image_vertical_alignment {
            ImageVerticalAlignment::Top => 0.0,
            ImageVerticalAlignment::Center => (ch - dh) / 2.0,
            ImageVerticalAlignment::Bottom => ch - dh,
        };
        let dest = D2D_RECT_F { left: x, top: y, right: x + dw, bottom: y + dh };
        unsafe {
            self.ctx.PushAxisAlignedClip(
                &Rect::new(0.0, 0.0, cw, ch).d2d(),
                D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
            );
            self.ctx.DrawBitmap(
                bitmap,
                Some(&dest),
                s.app_theme_background_image_opacity.clamp(0.1, 1.0),
                D2D1_INTERPOLATION_MODE_LINEAR,
                None,
                None,
            );
            self.ctx.PopAxisAlignedClip();
        }
    }

    /// A blurred drop shadow, from the web's layered `box-shadow` tokens.
    ///
    /// Direct2D has no `box-shadow`, so each CSS layer is rebuilt as a stack of
    /// concentric rounded rects whose alphas add up. The ring COUNT is what
    /// matters: too few and the steps read as visible bands rather than a
    /// blur, which is exactly how the first attempt failed. Alpha per ring is
    /// the layer's opacity spread over the stack, and the rings grow to the
    /// layer's blur radius, so the accumulated profile ramps smoothly from the
    /// panel edge out to nothing.
    pub(crate) fn draw_layered_shadow(
        &self,
        rect: &Rect,
        radius: f32,
        layers: &[drive_app_controls::themes::shape::ShadowLayer],
        colour: (f32, f32, f32),
    ) {
        /// Enough steps that the banding disappears at any DPI.
        const RINGS: usize = 12;
        /// A CSS blur RADIUS is not how far the shadow reaches: a gaussian of
        /// radius r keeps most of its density within about 3/4 of r. Spreading
        /// the rings over the FULL radius turned a discreet shadow into a grey
        /// cloud; halving it went too far the other way.
        const REACH: f32 = 0.75;
        let (sr, sg, sb) = colour;
        for layer in layers {
            let per_ring = layer.opacity / RINGS as f32;
            let extent = layer.blur * REACH;
            let dy = layer.dy;
            // Outermost first: the inner rings then stack on top, so the alpha
            // piles up against the panel and thins out quickly — the profile a
            // real blur has. The QUADRATIC spacing is what packs the rings near
            // the edge instead of spreading them evenly.
            for i in (1..=RINGS).rev() {
                let t = i as f32 / RINGS as f32;
                // The layer's own spread inflates the shape first; the blur
                // then fans out from there.
                let spread = layer.spread + extent * t * t;
                let color = D2D1_COLOR_F { r: sr, g: sg, b: sb, a: per_ring };
                let shadow = Rect::new(
                    rect.left - spread,
                    rect.top - spread + dy,
                    rect.right + spread,
                    rect.bottom + spread + dy,
                );
                self.fill_rounded(&shadow, radius + spread, &color);
            }
        }
    }

    /// Soft ambient drop shadow (approximates the WinUI ThemeShadow declared
    /// on the shell content card, ModernShellPage.xaml:42).
    pub(crate) fn draw_card_shadow(&self, rect: &Rect, radius: f32) {
        for i in 1..=4 {
            let spread = i as f32 * 1.5;
            let shadow = Rect::new(
                rect.left - spread * 0.5,
                rect.top - spread * 0.25 + i as f32 * 0.75,
                rect.right + spread * 0.5,
                rect.bottom + spread,
            );
            let color = D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 0.05 };
            self.fill_rounded(&shadow, radius + spread * 0.5, &color);
        }
    }

    fn draw_sidebar(&self, layout: &Layout, state: &UiState, model: &HomeModel) {
        // The DRAWING lives in the `drive_app_controls::sidebar` control: we
        // build the view here (resolved icons) then delegate. The scrollbars
        // stay with the app (type `Scrollbar` + `UiState`, outside the Canvas contract).
        let view = self.build_sidebar_view(layout, state, model);
        drive_app_controls::sidebar::draw(self, &view);

        if let Some(bar) = &layout.sidebar_scrollbar {
            let alpha = sidebar_scrollbar_alpha(state);
            if alpha > 0.0 {
                drive_app_controls::scrollbar::draw(
                    self, bar, alpha,
                    state.hot == Some(Hot::SidebarScrollThumb) || state.sidebar_scroll_drag.is_some(),
                );
            }
        }
        if let Some(bar) = &layout.sidebar_hscrollbar {
            let alpha = sidebar_scrollbar_alpha(state);
            if alpha > 0.0 {
                drive_app_controls::scrollbar::draw(
                    self, bar, alpha,
                    state.hot == Some(Hot::SidebarHScrollThumb) || state.sidebar_hscroll_drag.is_some(),
                );
            }
        }
    }

    /// Builds the sidebar's drawing snapshot (primitives), resolved BEFORE
    /// drawing; the `'v` lifetime ties the view to `self` and `model`.
    fn build_sidebar_view<'v>(
        &'v self,
        layout: &Layout,
        state: &UiState,
        model: &'v HomeModel,
    ) -> drive_app_controls::sidebar::SidebarView<'v> {
        use drive_app_controls::sidebar::{RowIcon, SidebarRowView, SidebarView};

        let t = self.theme;
        let active_location = state.active().location.clone();
        let compact = layout.sidebar_mode == crate::ui::SidebarMode::Compact;

        let (sx0, sidebar_full) = match layout.sidebar_overlay {
            Some(o) => (o.left, o.right - o.left),
            None => (0.0, crate::ui::sidebar_width()),
        };
        let overlay = layout.sidebar_overlay.filter(|o| o.left > -sidebar_full + 1.0);
        let max_hscroll = (layout.sidebar_hextent - sidebar_full).max(0.0);
        let dx = if compact { 0.0 } else { -state.sidebar_hscroll.clamp(0.0, max_hscroll) };
        let clip = Rect::new(sx0, TAB_BAR_HEIGHT + TOOLBAR_HEIGHT, sx0 + sidebar_full, layout.height);

        let entries: Vec<SidebarEntry> = layout.sidebar_items.iter().map(|(_, e)| e.clone()).collect();
        let best = sidebar_selected_entry(&entries, model, &active_location);

        let settings = crate::services::settings::get();
        let tree_expanded = |path: &str| state.sidebar_expanded.contains(&path.to_lowercase());

        let mut rows = Vec::with_capacity(layout.sidebar_items.len());
        for (i, (rect, entry)) in layout.sidebar_items.iter().enumerate() {
            let hot = state.hot == Some(Hot::SidebarItem(i));
            let SidebarVisual { glyph, label, indent, is_section, selected } =
                sidebar_visual(entry, model, &active_location, best == Some(i));

            // The nav reads as ONE monochrome family, like Drive's: every row
            // is a flat mark in the secondary colour. The shell's per-folder
            // bitmaps (teal Downloads, red Music, yellow star…) made the pane
            // loud and fought the active-row pill. Only tags keep their colour,
            // since there the colour IS the data.
            let icon: RowIcon<'v> = match entry {
                SidebarEntry::Tag(idx) => {
                    let color = settings
                        .file_tags
                        .get(*idx)
                        .and_then(|tag| crate::services::settings::parse_color(&tag.color))
                        .unwrap_or(t.text_primary);
                    RowIcon::Vector { name: "FilledTag", color }
                }
                // Nav icons rest in the secondary colour (web: #5f6368) and
                // take the accent on the active row — `RowIcon::Vector` is
                // never re-tinted by the draw pass, so the state is resolved
                // here. Entries without a Material counterpart keep their
                // Segoe glyph.
                _ => match sidebar_vector_icon(entry, model) {
                    Some(name) => {
                        let color = if selected { t.accent } else { t.text_secondary };
                        RowIcon::Vector { name, color }
                    }
                    None => RowIcon::Glyph { text: glyph, color: t.text_secondary },
                },
            };

            let chevron = if is_section {
                Some(match entry {
                    SidebarEntry::SectionPinned => settings.is_pinned_section_expanded,
                    SidebarEntry::SectionLibraries => settings.is_library_section_expanded,
                    SidebarEntry::SectionDrives => settings.is_drive_section_expanded,
                    SidebarEntry::SectionNetwork => settings.is_network_section_expanded,
                    SidebarEntry::SectionTags => settings.is_file_tags_section_expanded,
                    _ => true,
                })
            } else {
                match entry {
                    SidebarEntry::Drive(idx) => Some(tree_expanded(&format!("{}:\\", model.drives[*idx].letter))),
                    SidebarEntry::Folder(path, _) => Some(tree_expanded(path)),
                    SidebarEntry::CloudDrive(idx) => Some(tree_expanded(&model.cloud_drives[*idx].sync_folder)),
                    _ => None,
                }
            };

            rows.push(SidebarRowView { rect: *rect, indent, label, is_section, selected, hot, chevron, icon });
        }

        SidebarView { clip, overlay, compact, dx, rows }
    }
    fn draw_settings(&self, layout: &Layout, state: &UiState) {
        let t = self.theme;
        let f = &self.renderer.formats;

        // The « Paramètres » title: `Margin="16,12,0,4"`, Subtitle style —
        // SettingsPage.xaml's SidebarView.Header.
        let title = Rect::new(
            layout.content.left + 16.0,
            layout.content.top + 12.0,
            layout.content.left + 216.0,
            layout.content.top + 40.0,
        );
        self.text(drive_localization::tr("Settings"), &title, &f.title, &t.text_primary, false);

        // Internal nav (Général, Apparence, …) with the original ThemedIcons.
        for (i, rect) in layout.settings_nav.iter().enumerate() {
            let selected = i == state.settings_section;
            let hot = state.hot == Some(Hot::SettingsNav(i));
            if selected || hot {
                self.fill_rounded(rect, 4.0, &t.control_fill_hover);
            }
            if selected {
                let pill = Rect::new(rect.left, rect.top + 8.0, rect.left + 3.0, rect.bottom - 8.0);
                self.fill_rounded(&pill, 1.5, &t.accent);
            }
            let icon = Rect::new(rect.left + 12.0, rect.top, rect.left + 32.0, rect.bottom);
            self.vector_icon(SETTINGS_SECTION_ICONS[i], &icon, 16.0, &t.text_primary);
            let label = Rect::new(rect.left + 42.0, rect.top, rect.right - 4.0, rect.bottom);
            self.text(drive_localization::tr(SETTINGS_SECTIONS[i]), &label, &f.body, &t.text_primary, false);
        }

        // The page title: `SubtitleTextBlockStyle` + `Padding="0,0,0,12"`
        // in the 12-padded ScrollViewer — it ALIGNS with « Paramètres »
        // (GeneralPage.xaml and friends), and scrolls with the content.
        let scroll = state.active().scroll;
        let page_left = layout.content.left + 300.0;
        let page_title = Rect::new(
            page_left,
            layout.content.top + 12.0 - scroll,
            layout.content.right - 24.0,
            layout.content.top + 40.0 - scroll,
        );
        self.text(drive_localization::tr(SETTINGS_SECTIONS[state.settings_section]), &page_title, &f.title, &t.text_primary, false);

        // Appearance: full AppearancePage port (theme, backdrop, background
        // colors/image, font, tab actions, address bar, toolbar, status bar).
        if state.settings_section == 1 {
            self.draw_settings_appearance(layout, state);
            return;
        }

        // Generic pages: SettingsCard/SettingsExpander rows + the
        // page-specific custom panels (ItemsHeader blocks).
        let Some(page) = &layout.settings_page else { return };
        self.draw_settings_page(page, state);
        use crate::views::settings::controls::SettingId;
        for (row, rect) in page.rows.iter().zip(&page.rects) {
            match row.id {
                SettingId::GenStartupPages => self.draw_startup_pages_panel(rect),
                SettingId::ActTopBar => self.draw_actions_top_bar(rect),
                SettingId::AbtLibrariesPanel => self.draw_libraries_panel(rect),
                _ => {}
            }
        }
    }
}

/// The app's `Painter` IS a [`Canvas`] surface for the controls: each
/// method forwards to `Painter`'s same-named inherent method. Foreign trait
/// (`drive_app_controls`) + local type (`Painter`): legal, and zero-cost
/// (one-liners). See `drive-app-controls/src/canvas.rs` for the contract
/// and the three composites left out of the trait (shell_icon,
/// draw_scrollbar, draw_edit_text).
impl<'a> drive_app_controls::Canvas for Painter<'a> {
    fn theme(&self) -> &drive_app_controls::Theme {
        self.theme
    }
    fn formats(&self) -> &drive_app_controls::TextFormats {
        &self.renderer.formats
    }
    fn scale(&self) -> f32 {
        // Resolves to the INHERENT `Painter::scale` method (inherent
        // methods take priority over a trait's), no recursion.
        Painter::scale(self)
    }

    fn fill_rounded(&self, rect: &Rect, radius: f32, color: &D2D1_COLOR_F) {
        Painter::fill_rounded(self, rect, radius, color)
    }
    fn fill_top_rounded(&self, rect: &Rect, radius: f32, color: &D2D1_COLOR_F) {
        Painter::fill_top_rounded(self, rect, radius, color)
    }
    fn stroke_rounded(&self, rect: &Rect, radius: f32, color: &D2D1_COLOR_F) {
        Painter::stroke_rounded(self, rect, radius, color)
    }
    fn stroke_rounded_w(&self, rect: &Rect, radius: f32, color: &D2D1_COLOR_F, width: f32) {
        Painter::stroke_rounded_w(self, rect, radius, color, width)
    }

    fn text(&self, text: &str, rect: &Rect, format: &IDWriteTextFormat, color: &D2D1_COLOR_F, centered: bool) {
        Painter::text(self, text, rect, format, color, centered)
    }
    fn text_aligned(
        &self,
        text: &str,
        rect: &Rect,
        format: &IDWriteTextFormat,
        color: &D2D1_COLOR_F,
        alignment: windows::Win32::Graphics::DirectWrite::DWRITE_TEXT_ALIGNMENT,
    ) {
        Painter::text_aligned(self, text, rect, format, color, alignment)
    }
    fn text_ellipsis(&self, text: &str, rect: &Rect, format: &IDWriteTextFormat, color: &D2D1_COLOR_F) {
        Painter::text_ellipsis(self, text, rect, format, color)
    }
    fn text_ellipsis_center(&self, text: &str, rect: &Rect, format: &IDWriteTextFormat, color: &D2D1_COLOR_F) {
        Painter::text_ellipsis_center(self, text, rect, format, color)
    }

    fn image(&self, bitmap: &ID2D1Bitmap1, rect: &Rect, size: f32) {
        Painter::image(self, bitmap, rect, size)
    }
    fn image_alpha(&self, bitmap: &ID2D1Bitmap1, rect: &Rect, size: f32, alpha: f32) {
        Painter::image_alpha(self, bitmap, rect, size, alpha)
    }

    fn vector_icon(&self, name: &'static str, rect: &Rect, size: f32, color: &D2D1_COLOR_F) {
        Painter::vector_icon(self, name, rect, size, color)
    }
    fn vector_icon_layered(
        &self,
        name: &'static str,
        rect: &Rect,
        size: f32,
        fg: &D2D1_COLOR_F,
        accent: &D2D1_COLOR_F,
    ) {
        Painter::vector_icon_layered(self, name, rect, size, fg, accent)
    }

    fn measure(&self, text: &str, format: &IDWriteTextFormat) -> f32 {
        Painter::measure(self, text, format)
    }
    fn draw_card_shadow(&self, rect: &Rect, radius: f32) {
        Painter::draw_card_shadow(self, rect, radius)
    }
    fn draw_shadow(
        &self,
        rect: &Rect,
        radius: f32,
        layers: &[drive_app_controls::themes::shape::ShadowLayer],
        colour: (f32, f32, f32),
    ) {
        Painter::draw_layered_shadow(self, rect, radius, layers, colour)
    }
    fn erase_rounded(&self, rect: &Rect, radius: f32) {
        let rr = windows::Win32::Graphics::Direct2D::D2D1_ROUNDED_RECT {
            rect: self.snap(rect).d2d(),
            radiusX: radius,
            radiusY: radius,
        };
        let clear = D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 0.0 };
        unsafe {
            // COPY replaces the destination instead of blending into it: the
            // only way a fill can erase what is already drawn.
            self.ctx.SetPrimitiveBlend(
                windows::Win32::Graphics::Direct2D::D2D1_PRIMITIVE_BLEND_COPY,
            );
            self.ctx.FillRoundedRectangle(&rr, self.set_color(&clear));
            self.ctx.SetPrimitiveBlend(
                windows::Win32::Graphics::Direct2D::D2D1_PRIMITIVE_BLEND_SOURCE_OVER,
            );
        }
    }
    fn push_clip(&self, rect: &Rect) {
        unsafe {
            self.ctx.PushAxisAlignedClip(&rect.d2d(), D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
        }
    }
    fn push_clip_rounded(&self, rect: &Rect, radius: f32) {
        use windows::core::Interface;
        unsafe {
            let Ok(factory) = self
                .renderer
                .d2d_factory
                .cast::<windows::Win32::Graphics::Direct2D::ID2D1Factory>()
            else {
                self.push_clip(rect);
                return;
            };
            let rr = windows::Win32::Graphics::Direct2D::D2D1_ROUNDED_RECT {
                rect: rect.d2d(),
                radiusX: radius,
                radiusY: radius,
            };
            let Ok(mask) = factory.CreateRoundedRectangleGeometry(&rr) else {
                self.push_clip(rect);
                return;
            };
            let params = windows::Win32::Graphics::Direct2D::D2D1_LAYER_PARAMETERS1 {
                contentBounds: windows::Win32::Graphics::Direct2D::Common::D2D_RECT_F {
                    left: f32::NEG_INFINITY,
                    top: f32::NEG_INFINITY,
                    right: f32::INFINITY,
                    bottom: f32::INFINITY,
                },
                geometricMask: std::mem::ManuallyDrop::new(Some(mask.into())),
                maskAntialiasMode: D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
                maskTransform: windows_numerics::Matrix3x2::identity(),
                opacity: 1.0,
                opacityBrush: std::mem::ManuallyDrop::new(None),
                layerOptions: windows::Win32::Graphics::Direct2D::D2D1_LAYER_OPTIONS1_NONE,
            };
            self.ctx.PushLayer(&params, None);
        }
    }

    fn pop_clip_rounded(&self) {
        unsafe {
            self.ctx.PopLayer();
        }
    }

    fn pop_clip(&self) {
        unsafe {
            self.ctx.PopAxisAlignedClip();
        }
    }
}

/// The SIDEBAR's ScrollBar opacity (same law as `scrollbar_alpha`).
pub fn sidebar_scrollbar_alpha(state: &UiState) -> f32 {
    use crate::user_controls::scrollbar::{FADE_AFTER_MS, FADE_MS};
    if state.sidebar_scrollbar_expanded || state.sidebar_scroll_drag.is_some() {
        return 1.0;
    }
    let Some(since) = state.sidebar_scrolled_at else { return 0.0 };
    let ms = since.elapsed().as_secs_f32() * 1000.0;
    if ms <= FADE_AFTER_MS {
        1.0
    } else {
        (1.0 - (ms - FADE_AFTER_MS) / FADE_MS).clamp(0.0, 1.0)
    }
}
