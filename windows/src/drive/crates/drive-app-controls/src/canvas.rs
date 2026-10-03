//! The drawing TRAIT shared by every control.
//!
//! Point of the exercise: move the DRAWING of controls out of the `drive-app`
//! crate so it lives WITH the control, in `drive-app-controls`. A control no
//! longer knows the concrete `Painter` (Direct2D, device context, brush…): it
//! paints through a `&dyn Canvas`, using only PRIMITIVES and a view struct in
//! primitives (Rect, colors, formats, already-resolved bitmaps).
//!
//! `Canvas` is the counterpart of the app's `Painter`: each method below maps
//! one-to-one to an inherent method of `Painter`, with the SAME signature. On
//! the app side, `impl Canvas for Painter` just FORWARDS (see
//! `drive-app/src/ui/painter.rs`). This is legal — foreign trait + local type —
//! and free: these are one-liners.
//!
//! # What is NOT in the trait (and why)
//!
//! Three `Painter` methods stay out of the trait because they are domain
//! COMPOSITES, not primitives, and they depend on types that live in
//! `drive-app` (so unknown here):
//!
//! * **`shell_icon`** — resolves an `ID2D1Bitmap1` from the app's
//!   `IconCache`. A control NEVER does this resolution itself. The app
//!   resolves the bitmap BEFORE drawing and passes it into the control's view
//!   struct as `Option<&ID2D1Bitmap1>`; the control renders it via
//!   [`Canvas::image`].
//! * **`draw_scrollbar`** — takes a `Scrollbar` (a `drive-app` type) and
//!   composes track + thumb + chevrons. Stays app-side, or moves to a
//!   callback.
//! * **`draw_edit_text`** — takes an `EditState` (a `drive-app` type) and
//!   composes selection + text + caret with clipping. Stays app-side.
//!
//! See the detailed contract at the bottom of this file.

use crate::geometry::Rect;
use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;
use windows::Win32::Graphics::Direct2D::ID2D1Bitmap1;
use windows::Win32::Graphics::DirectWrite::{IDWriteTextFormat, DWRITE_TEXT_ALIGNMENT};

/// The drawing surface a control receives to paint itself.
///
/// Implemented by the app's `Painter` (Direct2D). Signatures are IDENTICAL to
/// `Painter`'s inherent methods: the implementation forwards.
///
/// The trait is object-safe (`&dyn Canvas`): every method takes `&self`, no
/// generic parameter/return, no bare `Self`.
pub trait Canvas {
    // ------------------------------------------------------------------
    // Accessors — the ambient state a control needs to paint.
    // ------------------------------------------------------------------

    /// The current theme (resolved light/dark colors + system accent).
    fn theme(&self) -> &crate::Theme;

    /// The shared DirectWrite formats (body, titles, icon glyphs…).
    fn formats(&self) -> &crate::TextFormats;

    /// The current DPI scale factor (`dpi / 96`), already clamped to > 0.
    fn scale(&self) -> f32;

    // ------------------------------------------------------------------
    // Fill / stroke primitives.
    // ------------------------------------------------------------------

    /// Fills a rounded-corner rectangle, aligned to the pixel grid.
    fn fill_rounded(&self, rect: &Rect, radius: f32, color: &D2D1_COLOR_F);

    /// Fills a rectangle where ONLY the TOP corners are rounded — the shape of
    /// a tab's `TabContainer` (`TopCornerRadiusFilterConverter`).
    fn fill_top_rounded(&self, rect: &Rect, radius: f32, color: &D2D1_COLOR_F);

    /// Fills the triangle `a`→`b`→`c` with `color`, anti-aliased — used for the
    /// tooltip arrow. A stack of one-pixel bands rendered it as a hard, jagged
    /// staircase; worse, that staircase carries no shadow, so on the side the
    /// bubble's drop shadow is offset away from (the top) it read as bare and
    /// detached while the grounded sides looked clean. A single filled path is
    /// smooth on every side. The default does nothing; only a real renderer
    /// draws it, so a headless canvas simply omits the cosmetic arrow.
    fn fill_triangle(&self, _a: (f32, f32), _b: (f32, f32), _c: (f32, f32), _color: &D2D1_COLOR_F) {}

    /// Strokes an arc of the circle of `radius` around `centre`, `width` DIP
    /// thick with ROUND caps — the moving quarter of a spinner, the web's
    /// `border-t-*` on a `rounded-full` ring. Angles are radians, `0` pointing
    /// right and growing CLOCKWISE (y points down); `sweep` may be negative.
    ///
    /// A real renderer draws one anti-aliased path. This default, for a canvas
    /// that has none, rasterises the arc as overlapping round dots — legible,
    /// but visibly beaded, which is why the renderers override it.
    fn stroke_arc(
        &self,
        centre: (f32, f32),
        radius: f32,
        start: f32,
        sweep: f32,
        width: f32,
        color: &D2D1_COLOR_F,
    ) {
        let half = width / 2.0;
        let steps = ((sweep.abs() * radius) / (width * 0.5)).ceil().max(1.0) as usize;
        for i in 0..=steps {
            let a = start + sweep * (i as f32 / steps as f32);
            let (x, y) = (centre.0 + radius * a.cos(), centre.1 + radius * a.sin());
            self.fill_rounded(&Rect::new(x - half, y - half, x + half, y + half), half, color);
        }
    }

    /// Strokes a rounded border exactly one physical pixel wide, centered on
    /// pixel centers (crisp border at any DPI).
    fn stroke_rounded(&self, rect: &Rect, radius: f32, color: &D2D1_COLOR_F);

    /// Strokes a rounded border `width` DIP thick, inward — the 2 DIP
    /// selection border of the `ListViewItemPresenter`.
    fn stroke_rounded_w(&self, rect: &Rect, radius: f32, color: &D2D1_COLOR_F, width: f32);

    // ------------------------------------------------------------------
    // Text primitives.
    // ------------------------------------------------------------------

    /// Draws text in `rect`, left-aligned or horizontally centered, always
    /// vertically centered.
    fn text(&self, text: &str, rect: &Rect, format: &IDWriteTextFormat, color: &D2D1_COLOR_F, centered: bool);

    /// Like [`Canvas::text`], but with an explicit DirectWrite alignment.
    fn text_aligned(
        &self,
        text: &str,
        rect: &Rect,
        format: &IDWriteTextFormat,
        color: &D2D1_COLOR_F,
        alignment: DWRITE_TEXT_ALIGNMENT,
    );

    /// Left-aligned text, truncated with an ELLIPSIS (« … ») at the right
    /// edge — the `TextTrimming.CharacterEllipsis` of the original TextBlocks.
    fn text_ellipsis(&self, text: &str, rect: &Rect, format: &IDWriteTextFormat, color: &D2D1_COLOR_F);

    /// Like [`Canvas::text_ellipsis`], but the text is horizontally CENTERED
    /// (the alignment lives on the layout, not the shared format, so it does
    /// not leak into other calls).
    fn text_ellipsis_center(&self, text: &str, rect: &Rect, format: &IDWriteTextFormat, color: &D2D1_COLOR_F);

    // ------------------------------------------------------------------
    // Image primitives (bitmaps ALREADY resolved — see `shell_icon` contract).
    // ------------------------------------------------------------------

    /// Draws a bitmap as a square icon centered in `rect`, aligned to the
    /// pixel grid at its native `size` DIP (no resampling).
    fn image(&self, bitmap: &ID2D1Bitmap1, rect: &Rect, size: f32);

    /// Like [`Canvas::image`], with an `alpha` opacity (0..=1).
    fn image_alpha(&self, bitmap: &ID2D1Bitmap1, rect: &Rect, size: f32, alpha: f32);

    // ------------------------------------------------------------------
    // Vector icon primitives (ThemedIcon geometries from the original).
    // ------------------------------------------------------------------

    /// MONOCHROME ThemedIcon: all layers painted in the same `color`
    /// (respecting each layer's opacity) — the "Outline" rendering of an
    /// AppBarButton.
    ///
    /// `name` is the geometry's name (static literal: icons are code
    /// constants, as in the original).
    fn vector_icon(&self, name: &'static str, rect: &Rect, size: f32, color: &D2D1_COLOR_F);

    /// MULTI-LAYER ThemedIcon: each layer is painted according to its role —
    /// foreground (`fg`), accent (`accent`), or contrast (white) — the
    /// "treatment" that tints part of the icon.
    fn vector_icon_layered(
        &self,
        name: &'static str,
        rect: &Rect,
        size: f32,
        fg: &D2D1_COLOR_F,
        accent: &D2D1_COLOR_F,
    );

    // ------------------------------------------------------------------
    // Measurement & shadow.
    // ------------------------------------------------------------------

    /// Exact text width via DirectWrite (caret/selection positioning, tight
    /// sizing).
    fn measure(&self, text: &str, format: &IDWriteTextFormat) -> f32;

    /// Soft drop shadow under a card (approximates WinUI's `ThemeShadow`).
    fn draw_card_shadow(&self, rect: &Rect, radius: f32);

    /// An arbitrary layered CSS `box-shadow`, as the web writes it — one call per
    /// surface that publishes its own recipe (see
    /// [`crate::themes::shape::SHADOW_WAFFLE`]). The colour is a parameter
    /// because not every Kubuno shadow is [`crate::themes::shape::SHADOW_GREY`].
    ///
    /// The shadow spills OUTSIDE `rect`, so a surface drawn in its own popup
    /// window needs that window grown to leave room, or the shadow is clipped
    /// away at the window edge.
    ///
    /// It is built from stacked FILLED rounded rects, so it also darkens the
    /// area under `rect`. That is invisible when an opaque surface is painted on
    /// top, but a surface that stays transparent for a compositor backdrop must
    /// erase it with [`Canvas::erase_rounded`] first.
    fn draw_shadow(
        &self,
        rect: &Rect,
        radius: f32,
        layers: &[crate::themes::shape::ShadowLayer],
        colour: (f32, f32, f32),
    );

    /// Replaces everything inside `rect` with TRANSPARENT, corners included —
    /// an erase, not a blend, so it removes what is already drawn there.
    ///
    /// Exists for the case above: a panel hosted over a blurred backdrop draws
    /// its own shadow, then erases its own interior so the blur shows instead of
    /// the stacked shadow fills.
    fn erase_rounded(&self, rect: &Rect, radius: f32);

    // ------------------------------------------------------------------
    // Clipping (axis-aligned clip) — for an editor's scrolling text or a
    // list's scrollable area. Pushed/popped in pairs.
    // ------------------------------------------------------------------

    /// Restricts the following drawing to `rect` (`PushAxisAlignedClip`).
    fn push_clip(&self, rect: &Rect);

    /// Clips to a ROUNDED rect — what a circular avatar needs, since an
    /// axis-aligned clip would leave a square photo square. Paired with
    /// [`Canvas::pop_clip_rounded`], which is a different pop: D2D keeps
    /// axis-aligned clips and layers on separate stacks.
    fn push_clip_rounded(&self, rect: &Rect, radius: f32);

    fn pop_clip_rounded(&self);

    /// Pops the last clip (`PopAxisAlignedClip`).
    fn pop_clip(&self);

    // ------------------------------------------------------------------
    // Offset & extent — what a scrolling container needs: it paints content
    // laid out in the content's own coordinates, shifted by the scroll, and
    // learns how far that content reached so it knows when to show a bar.
    // Defaults are no-ops, so a Canvas that does not implement them still
    // compiles (and simply never scrolls).
    // ------------------------------------------------------------------

    /// Translates everything drawn until the matching [`Canvas::pop_offset`]
    /// by `(dx, dy)` DIP. Nests: offsets add up. Clips pushed inside the scope
    /// are in the translated space too.
    fn push_offset(&self, _dx: f32, _dy: f32) {}

    /// Pops the last [`Canvas::push_offset`].
    fn pop_offset(&self) {}

    /// Starts recording how far the following drawing reaches, in the
    /// coordinates in force at this call. Only what survives the clips pushed
    /// AFTER this call counts: a nested scroller's hidden content does not
    /// make its parent scroll. Nests.
    fn begin_extent(&self) {}

    /// Ends the last [`Canvas::begin_extent`]: the right-most and bottom-most
    /// coordinates drawn since, or `None` when nothing was drawn (or the
    /// Canvas does not track extents).
    fn end_extent(&self) -> Option<(f32, f32)> {
        None
    }

    // ------------------------------------------------------------------
    // Background colour stack — carries « the fill of the parent surface »
    // through the paint tree, so a widget can default its own background to
    // its container's without either knowing about the other.
    //
    // Rule: a widget's own background is `current_bg()` unless a variant asked
    // for something specific. A container that paints a distinct surface
    // (a Card body, a Popover, a page area inside Tabs) wraps its children in
    // `push_bg(colour)` / `pop_bg()`, so a control that lands inside it reads
    // the card's colour, not the window's. Default is `theme.window_background`
    // — the outermost surface a caller ever sees.
    // ------------------------------------------------------------------

    /// The parent surface's fill, as most recently pushed. The default returns
    /// the theme's window background — a Canvas that has not yet implemented
    /// the stack still speaks the rule, from the outermost surface down.
    fn current_bg(&self) -> D2D1_COLOR_F {
        self.theme().window_background
    }

    /// Announces a new parent surface. Children painted between this call and
    /// the matching [`Canvas::pop_bg`] see `colour` as their default background.
    /// Default is a no-op, again to keep an out-of-date Canvas compilable.
    fn push_bg(&self, _colour: D2D1_COLOR_F) {}

    /// Pops the last [`Canvas::push_bg`].
    fn pop_bg(&self) {}

    // ------------------------------------------------------------------
    // Device access — for a drawing layer built ABOVE the primitives (the
    // WinForms-`Graphics`-like surface of `kubuno_ui::graphics`: paths,
    // gradients, transforms, dashed pens), which draws with Direct2D itself
    // in the same `BeginDraw` as the primitives. Defaults keep every existing
    // Canvas compiling: without a renderer, that layer falls back to the
    // primitives above.
    // ------------------------------------------------------------------

    /// The Direct2D / DirectWrite renderer this surface draws with, when it
    /// has one and lets a higher drawing layer use its device context
    /// directly. `None` (the default) for a surface that does not.
    fn graphics_renderer(&self) -> Option<&crate::Renderer> {
        None
    }

    /// Records that `rect` (in the current coordinates) was drawn by such a
    /// layer, so the surface's extent tracking ([`Canvas::begin_extent`])
    /// counts it like its own primitives. Default: nothing to record.
    fn note_drawn(&self, _rect: &Rect) {}
}
