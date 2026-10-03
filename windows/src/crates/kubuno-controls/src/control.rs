//! `Control` — the base every other control inherits, and the trait they all
//! implement.
//!
//! ## Why this file is the contract
//!
//! In WinForms, `Control` declares **52 settable properties** that every one of
//! the other 64 control types inherits; `ButtonBase` then adds 19 shared by
//! `Button`, `CheckBox` and `RadioButton` — and `Button` itself adds only
//! **two**. Re-implementing those 52 (or 19) per control would be both enormous
//! and wrong: they would drift apart. So the port mirrors the inheritance chain
//! with **composition + `Deref`**:
//!
//! ```ignore
//! pub struct ButtonBase { control: ControlBase, /* +19 */ }
//! pub struct Button     { base: ButtonBase,     /* +2  */ }
//! // Button derefs to ButtonBase, which derefs to ControlBase:
//! //   my_button.text        → ControlBase::text
//! //   my_button.flat_style  → ButtonBase::flat_style
//! ```
//!
//! Each level owns exactly the properties its .NET counterpart *declares*, so a
//! property lives in one place and every descendant gets it for free — the same
//! reuse the toolkit has, without inheritance in the language.

use drive_app_controls::{Canvas, Rect};
use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;

use crate::enums::*;
use crate::system::{edge_colors, edge_interior, Border3DSide, Border3DStyle, Visuals};

/// The properties `System.Windows.Forms.Control` itself declares.
///
/// Colours and the font are `Option`: in WinForms these are **ambient** — unset
/// means « take the parent's », which is a different fact from « take this exact
/// colour », and collapsing the two would break theming the moment a container
/// sets a colour.
// No `Debug`: `Rect` and `D2D1_COLOR_F` come from the drawing layer and do not
// implement it. A control is inspected through its properties, not printed.
#[derive(Clone)]
pub struct ControlBase {
    // ── Identity ─────────────────────────────────────────────────────────
    pub name: String,
    pub text: String,
    /// `Tag` — an arbitrary payload the host attaches. Kept as a string because
    /// the port has no boxed-object equivalent and a typed id is what callers
    /// actually use.
    pub tag:  Option<String>,

    // ── Geometry ─────────────────────────────────────────────────────────
    /// `Bounds`, relative to the parent's client area. `Location`/`Size`/
    /// `Left`/`Top`/`Width`/`Height` are views onto this one field, exactly as
    /// they are in the toolkit.
    pub bounds:       Rect,
    pub minimum_size: Size,
    pub maximum_size: Size,
    pub margin:       Padding,
    pub padding:      Padding,

    // ── Layout ───────────────────────────────────────────────────────────
    pub dock:           DockStyle,
    pub anchor:         AnchorStyles,
    pub auto_size:      bool,
    pub auto_size_mode: AutoSizeMode,

    // ── Appearance (ambient when `None`) ─────────────────────────────────
    pub back_color:            Option<D2D1_COLOR_F>,
    pub fore_color:            Option<D2D1_COLOR_F>,
    /// The font by role, resolved against `Canvas::formats()` at paint time —
    /// the port has no free-form font object, and every Kubuno surface draws
    /// with the shared formats.
    pub font:                  Option<FontRole>,
    /// `BackgroundImage` — declared by `Control`, so it lives here rather than
    /// on each control that re-declares it. Carried as an opaque handle the
    /// host resolves; the library owns no raster pipeline, so nothing paints it
    /// yet. Dropping the property would have been the worse answer: several
    /// controls re-declare it, and they must all agree on one storage.
    pub background_image:        Option<String>,
    pub background_image_layout: ImageLayout,

    // ── State ────────────────────────────────────────────────────────────
    pub enabled:    bool,
    pub visible:    bool,
    pub tab_index:  i32,
    pub tab_stop:   bool,
    pub allow_drop: bool,
    pub causes_validation: bool,
    pub use_wait_cursor:   bool,
    pub right_to_left:     RightToLeft,
    /// `ContextMenuStrip` — the menu this control opens on right-click.
    /// Declared by `Control`, so it belongs here rather than on each control
    /// that re-declares it. Held as an opaque key the host resolves against its
    /// own menu registry: a typed handle would point `control.rs` at the
    /// `toolstrip` module, and the foundation must not depend on a family.
    pub context_menu_strip: Option<String>,

    /// `Cursor` — the pointer shape over this control. An opaque name the host
    /// resolves (`"IDC_ARROW"`, `"IDC_IBEAM"`…): the library owns no cursor
    /// handle, and a typed handle would drag Win32 into the foundation.
    pub cursor: Option<String>,
    /// `ImeMode` — carried so it round-trips; the port drives no IME.
    pub ime_mode: ImeMode,
    /// `Capture` — whether this control has the mouse capture. Runtime state the
    /// host sets; no control reads it while painting.
    pub capture: bool,

    // ── Accessibility ────────────────────────────────────────────────────
    pub accessible_name:        Option<String>,
    pub accessible_description: Option<String>,
    /// `AccessibleRole` — what the control reports itself as. `Default` means
    /// « whatever this control would say anyway ».
    pub accessible_role: AccessibleRole,
    /// `AccessibleDefaultActionDescription` — the verb assistive technology
    /// announces for the control's primary action ("Press", "Toggle"…).
    pub accessible_default_action_description: Option<String>,
    /// `IsAccessible` — whether the control is exposed to assistive technology
    /// at all.
    pub is_accessible: bool,
}

// ── Properties `Control` declares that this port deliberately does NOT model ──
//
// Named here rather than omitted in silence, because a reader comparing against
// the catalogue is owed the reason:
//
// * `CanFocus`, `CanSelect`, `Focused`, `IsHandleCreated`, `RecreatingHandle`,
//   `IsMirrored`, `TopLevelControl`, `Parent` — all derived from a live window
//   tree. A control here is a value with no handle and no parent pointer; the
//   host owns that graph and can answer them.
// * `Region`, `WindowTarget` — Win32 objects. Modelling them as opaque handles
//   would let a caller set something the library can never honour.
// * `DataBindings`, `DataContext` — the binding stack is not part of this wave;
//   `ListControl`'s data-binding surface is documented as deferred the same way.
// * `ClientSize` — a view, not storage: see [`ControlBase::client_size`].

/// Which shared text format a control paints with. WinForms carries a `Font`
/// object; Kubuno carries a role that resolves against the theme's formats, so
/// a control can never hard-code a family or a size.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FontRole {
    Caption,
    CaptionStrong,
    #[default]
    Body,
    BodyStrong,
    Heading,
    Title,
}

impl Default for ControlBase {
    /// The toolkit's documented defaults — `TabStop = true`,
    /// `CausesValidation = true`, `Anchor = Top | Left`, `Dock = None`,
    /// `AutoSize = false`, `Visible = true`, `Enabled = true`.
    fn default() -> Self {
        Self {
            name: String::new(),
            text: String::new(),
            tag: None,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            minimum_size: Size::EMPTY,
            maximum_size: Size::EMPTY,
            margin: Padding::all(3.0),   // WinForms' DefaultMargin
            padding: Padding::ZERO,
            dock: DockStyle::default(),
            anchor: AnchorStyles::default(),
            auto_size: false,
            auto_size_mode: AutoSizeMode::default(),
            back_color: None,
            fore_color: None,
            font: None,
            background_image: None,
            background_image_layout: ImageLayout::default(),
            enabled: true,
            visible: true,
            tab_index: 0,
            tab_stop: true,
            allow_drop: false,
            causes_validation: true,
            use_wait_cursor: false,
            right_to_left: RightToLeft::default(),
            context_menu_strip: None,
            cursor: None,
            ime_mode: ImeMode::default(),
            capture: false,
            accessible_name: None,
            accessible_description: None,
            accessible_role: AccessibleRole::default(),
            accessible_default_action_description: None,
            is_accessible: true,
        }
    }
}

impl ControlBase {
    pub fn new() -> Self {
        Self::default()
    }

    // The `Location`/`Size`/`Left`… views onto `bounds`, so callers can use the
    // vocabulary they know from the designer.

    pub fn left(&self) -> f32 {
        self.bounds.left
    }

    pub fn top(&self) -> f32 {
        self.bounds.top
    }

    pub fn width(&self) -> f32 {
        self.bounds.right - self.bounds.left
    }

    pub fn height(&self) -> f32 {
        self.bounds.bottom - self.bounds.top
    }

    pub fn size(&self) -> Size {
        Size::new(self.width(), self.height())
    }

    /// `ClientSize` — the area inside the non-client frame.
    ///
    /// A view, never storage, exactly as in the toolkit: `ClientSize` and
    /// `Size` are two readings of one rectangle, and keeping both as fields is
    /// how they drift. These controls are drawn, not windowed, so they have no
    /// non-client frame of their own and the two coincide; a container that
    /// *does* inset (a bordered `Panel`, a `GroupBox` with its caption band)
    /// overrides this with its own reading.
    pub fn client_size(&self) -> Size {
        self.size()
    }

    /// Moves the control without resizing it — `Location = value`.
    pub fn set_location(&mut self, x: f32, y: f32) {
        let (w, h) = (self.width(), self.height());
        self.bounds = Rect::new(x, y, x + w, y + h);
    }

    /// Resizes in place — `Size = value`, clamped by `MinimumSize`/`MaximumSize`
    /// exactly as the toolkit does (an empty constraint means « no bound »).
    pub fn set_size(&mut self, size: Size) {
        let s = self.clamp(size);
        self.bounds = Rect::new(
            self.bounds.left,
            self.bounds.top,
            self.bounds.left + s.width,
            self.bounds.top + s.height,
        );
    }

    pub fn set_bounds(&mut self, r: Rect) {
        self.bounds = r;
        self.set_size(Size::new(r.right - r.left, r.bottom - r.top));
    }

    /// Applies `MinimumSize` / `MaximumSize`. `Size::EMPTY` means unset on both,
    /// which is why neither is treated as a literal zero bound.
    pub fn clamp(&self, mut s: Size) -> Size {
        if self.minimum_size.width > 0.0 {
            s.width = s.width.max(self.minimum_size.width);
        }
        if self.minimum_size.height > 0.0 {
            s.height = s.height.max(self.minimum_size.height);
        }
        if self.maximum_size.width > 0.0 {
            s.width = s.width.min(self.maximum_size.width);
        }
        if self.maximum_size.height > 0.0 {
            s.height = s.height.min(self.maximum_size.height);
        }
        s
    }

    /// `ClientRectangle` deflated by `Padding` — where a control lays out its
    /// content and its children.
    pub fn display_rect(&self) -> Rect {
        let b = self.bounds;
        Rect::new(
            b.left + self.padding.left,
            b.top + self.padding.top,
            (b.right - self.padding.right).max(b.left + self.padding.left),
            (b.bottom - self.padding.bottom).max(b.top + self.padding.top),
        )
    }

    /// The effective foreground: the control's own, else the theme's primary
    /// text — the ambient resolution WinForms performs against the parent.
    pub fn resolved_fore(&self, c: &dyn Canvas) -> D2D1_COLOR_F {
        self.fore_color.unwrap_or(c.theme().text_primary)
    }
}

/// What the pointer and the focus are doing to a control at the moment it
/// paints.
///
/// A control is a value: it cannot observe the mouse itself, and threading a
/// pointer position through every one of them would make ten families pay for
/// what a few need. So the host — which *does* know — passes this in.
///
/// Without it, a real part of the toolkit is unreachable rather than merely
/// unimplemented: `FlatAppearance.MouseOverBackColor` and `MouseDownBackColor`
/// have nowhere to apply, `LinkBehavior::HoverUnderline` can never underline,
/// and `FlatStyle::Popup` and `FlatStyle::System` cannot be told apart from
/// `Standard` — because they differ *only* under the mouse.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ControlState {
    /// The pointer is over the control (WinForms' « hot »).
    pub hot:     bool,
    /// The pointer is down on the control.
    pub pressed: bool,
    /// The control holds the keyboard focus.
    pub focused: bool,
    /// This is the form's `AcceptButton` — the one Enter activates, which the
    /// toolkit paints with a distinct border.
    pub default: bool,
}

/// The paint surface a control actually receives: a [`Canvas`] that also knows
/// the **system** visuals.
///
/// ## Why a second trait rather than a parameter
///
/// The controls in this crate reproduce the WinForms surface, so they paint in
/// the system's own colours, metrics and UI font — see [`crate::system`]. That
/// is ambient state: *every* control needs it, in *every* paint. Threading it
/// through as an extra argument would widen `paint` for all ten families and
/// every helper under them, and it would have to be widened again the next time
/// something ambient appears.
///
/// [`Canvas`] itself cannot carry it: it lives in `drive-app-controls`, which
/// knows nothing about WinForms and is shared with the Drive app. So the
/// library narrows the surface *here*, in the one crate that has the
/// requirement.
///
/// `ControlCanvas: Canvas`, so everything a family already calls — `c.theme()`,
/// `c.formats()`, `c.fill_rounded(…)` — keeps working unchanged, and a
/// `&dyn ControlCanvas` still coerces to a `&dyn Canvas` for the helpers that
/// only measure.
pub trait ControlCanvas: Canvas {
    /// The system colours, metrics and fonts, at this surface's DPI.
    fn visuals(&self) -> &Visuals;

    /// The Direct2D / DirectWrite renderer behind this surface, when it has
    /// one — for an app that paints part of its window with its own raw
    /// DirectWrite code (a word processor's pages) in the same `BeginDraw` as
    /// the controls. `None` for a surface with no renderer to share.
    fn renderer(&self) -> Option<&drive_app_controls::Renderer> {
        None
    }

    /// A **square** filled rectangle.
    ///
    /// The design system this library grew out of rounds everything, so the
    /// only fill it offered was `fill_rounded`. A WinForms control has no
    /// rounded corners at all: this is the shape it actually paints, named so a
    /// family never has to remember to pass a zero radius.
    fn fill_rect(&self, rect: &Rect, color: &D2D1_COLOR_F) {
        self.fill_rounded(rect, 0.0, color);
    }

    /// A **square** one-physical-pixel outline — `BorderStyle::FixedSingle`.
    fn stroke_rect(&self, rect: &Rect, color: &D2D1_COLOR_F) {
        self.stroke_rounded(rect, 0.0, color);
    }

    /// A 3-D edge — Win32's `DrawEdge`, i.e. `ControlPaint.DrawBorder3D`.
    ///
    /// A WinForms border is **two concentric one-pixel rings**, each with a
    /// light side (top + left) and a dark side (bottom + right); the two-tone
    /// bevel is what makes a control read as raised or sunken. A stroke cannot
    /// express it, which is why this is a primitive rather than something a
    /// family assembles for itself — ten families assembling it would produce
    /// ten slightly different bevels.
    ///
    /// Returns the **interior**: `rect` deflated by the rings actually drawn,
    /// which is where the control's content goes. That is what `DrawEdge`
    /// reports under `BF_ADJUST`, and it saves every caller from re-deriving the
    /// thickness.
    ///
    /// Each ring is one *device* pixel, so the bevel stays hairline-thin at any
    /// DPI — the only legitimate use of `Canvas::scale`, and it divides by it
    /// rather than multiplying.
    fn draw_edge(&self, rect: &Rect, style: Border3DStyle, sides: Border3DSide) -> Rect {
        let edge = edge_colors(&self.visuals().colors, style);
        let scale = self.scale();
        let t = 1.0 / scale;
        let rings = [(edge.outer_light, edge.outer_dark), (edge.inner_light, edge.inner_dark)];
        for (depth, (light, dark)) in rings.into_iter().enumerate() {
            if light.is_none() && dark.is_none() {
                continue;
            }
            let inset = depth as f32 * t;
            let r = Rect::new(
                rect.left + inset,
                rect.top + inset,
                rect.right - inset,
                rect.bottom - inset,
            );
            // The light side goes down first and the dark side over it, so the
            // two corners the sides share come out dark — the asymmetry a real
            // bevel has, and the reason a raised box does not look like a frame.
            if let Some(c) = light {
                if sides.has(Border3DSide::TOP) {
                    self.fill_rect(&Rect::new(r.left, r.top, r.right, r.top + t), &c);
                }
                if sides.has(Border3DSide::LEFT) {
                    self.fill_rect(&Rect::new(r.left, r.top, r.left + t, r.bottom), &c);
                }
            }
            if let Some(c) = dark {
                if sides.has(Border3DSide::BOTTOM) {
                    self.fill_rect(&Rect::new(r.left, r.bottom - t, r.right, r.bottom), &c);
                }
                if sides.has(Border3DSide::RIGHT) {
                    self.fill_rect(&Rect::new(r.right - t, r.top, r.right, r.bottom), &c);
                }
            }
        }
        edge_interior(rect, edge.rings(), scale)
    }

    /// A **themed** part — the real `uxtheme.dll` rendering of a control's
    /// chrome — drawn into `rect`. Returns `false` when it could not be, and
    /// **that is the interesting half of the contract**.
    ///
    /// ## Why a boolean rather than a fallible draw
    ///
    /// The library has two correct renderings of the same control, not one with
    /// an approximation: with visual styles **on** — how the reference sheets
    /// were made — a `Fixed3D` field is a flat `#ABADB3` line from the theme,
    /// and with them **off** it is the classic two-ring `DrawEdge` well
    /// [`ControlCanvas::draw_edge`] paints from `GetSysColor`. Neither is a
    /// degraded version of the other, and the second is the *only* thing that
    /// exists on a themed-off machine.
    ///
    /// So a family writes both, as one paint with two branches:
    ///
    /// ```ignore
    /// if !c.draw_theme_part(theme::class::EDIT, EP_EDITBORDER_NOSCROLL, state, bounds, ground) {
    ///     c.draw_edge(&bounds, Border3DStyle::Sunken, Border3DSide::ALL);
    /// }
    /// ```
    ///
    /// `false` covers every reason at once — visual styles off, an unmanifested
    /// process, a class or part this theme does not define, a degenerate
    /// rectangle, a device failure — because a caller's answer to all of them is
    /// the same and distinguishing them would only invite a family to handle
    /// one and forget the rest.
    ///
    /// `background` is the colour that surrounds the part on screen: the ground
    /// the control has put, or is about to put, behind it. It is what the part
    /// is composited onto, so a rounded button corner blends into the form's
    /// face instead of into black. Getting it wrong shows as a dark fringe on
    /// the corners, not as a missing part.
    ///
    /// The DEFAULT is `false` — a surface that knows nothing about themes (a
    /// measuring canvas, a test double) makes every control classic rather than
    /// unpainted. Only the host's `Painter` overrides it, because it is the only
    /// one holding a Direct2D device and the window's part cache.
    fn draw_theme_part(
        &self,
        _class: &str,
        _part: i32,
        _state: i32,
        _rect: Rect,
        _background: D2D1_COLOR_F,
    ) -> bool {
        false
    }

    /// Draws `bitmap` stretched into `dest` (in the current coordinates), `alpha` opaque — a
    /// control's `BackgroundImage`/`Image` (see [`crate::styled::draw_image`], which lays it out).
    /// The default draws nothing: only a surface with a Direct2D device can.
    fn draw_bitmap(&self, _bitmap: &windows::Win32::Graphics::Direct2D::ID2D1Bitmap1, _dest: &Rect, _alpha: f32) {}
}

/// Lets a `Box<dyn Control>` be cloned.
///
/// A container owns its children as `Vec<Box<dyn Control>>`, so a container can
/// only be `Clone` if a boxed control is. Trait objects cannot require `Clone`
/// directly (it is not object-safe), so the capability is expressed here and
/// **blanket-implemented** for every control that is itself `Clone` — no control
/// has to write it by hand.
pub trait ControlClone {
    fn clone_box(&self) -> Box<dyn Control>;
}

impl<T: 'static + Control + Clone> ControlClone for T {
    fn clone_box(&self) -> Box<dyn Control> {
        Box::new(self.clone())
    }
}

impl Clone for Box<dyn Control> {
    fn clone(&self) -> Self {
        self.clone_box()
    }
}

/// What every control implements.
///
/// Deliberately small: the property surface lives in the `*Base` structs, and
/// this trait is only what the layout engine and the host loop need — measure,
/// paint, hit-test. A control that adds behaviour adds it as inherent methods,
/// not here, so the trait stays object-safe and cheap to implement.
///
/// The `ControlClone` supertrait means **every control must derive `Clone`**.
/// That is not a tax: `ControlBase` is `Clone`, and it is what lets a container
/// holding boxed children be `Clone` like every other control.
pub trait Control: ControlClone {
    /// The inherited property block. Every control has one, at the bottom of
    /// its composition chain.
    fn control(&self) -> &ControlBase;
    fn control_mut(&mut self) -> &mut ControlBase;

    /// The size the control would like, given the ambient font and its content
    /// — `GetPreferredSize`. Honours `Padding`; the caller applies `Margin`.
    fn preferred_size(&self, c: &dyn Canvas) -> Size;

    /// Paints the control into `bounds` (already positioned by the layout), in
    /// its resting state.
    ///
    /// The surface is a [`ControlCanvas`], not a bare [`Canvas`]: a control
    /// paints in the SYSTEM's colours, metrics and font, and reaching them
    /// through the surface is what keeps that out of every signature below.
    fn paint(&self, c: &dyn ControlCanvas, bounds: Rect);

    /// Paints honouring the pointer/focus state the host observed.
    ///
    /// Provided, and by default it simply ignores the state and calls
    /// [`Control::paint`] — so the nine families with nothing to show under the
    /// mouse implement nothing, and a family that *does* (buttons, links)
    /// overrides this one and has its own `paint` delegate here with
    /// `ControlState::default()`. That keeps the trait object-safe, leaves every
    /// existing call site working, and never widens `paint` for one family's
    /// benefit.
    fn paint_with_state(&self, c: &dyn ControlCanvas, bounds: Rect, _state: ControlState) {
        self.paint(c, bounds);
    }

    /// Whether the point is inside the control's interactive area. The default
    /// is the whole box, which is what most controls want.
    fn hit_test(&self, x: f32, y: f32) -> bool {
        let b = self.control().bounds;
        self.control().visible && self.control().enabled && b.contains(x, y)
    }

    /// The control's name, for debugging and for the demo forms' captions.
    fn type_name(&self) -> &'static str;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_defaults_match_the_toolkit() {
        let c = ControlBase::new();
        assert!(c.enabled && c.visible && c.tab_stop && c.causes_validation);
        assert!(!c.auto_size && !c.allow_drop);
        assert_eq!(c.dock, DockStyle::None);
        assert_eq!(c.margin, Padding::all(3.0));
        assert_eq!(c.padding, Padding::ZERO);
    }

    #[test]
    fn location_and_size_are_views_onto_bounds() {
        let mut c = ControlBase::new();
        c.set_bounds(Rect::new(10.0, 20.0, 110.0, 60.0));
        assert_eq!((c.left(), c.top(), c.width(), c.height()), (10.0, 20.0, 100.0, 40.0));
        c.set_location(0.0, 0.0);
        assert_eq!((c.width(), c.height()), (100.0, 40.0), "moving must not resize");
    }

    #[test]
    fn an_empty_constraint_is_not_a_zero_bound() {
        let mut c = ControlBase::new();
        // Both constraints unset: the size passes through untouched.
        assert_eq!(c.clamp(Size::new(50.0, 20.0)), Size::new(50.0, 20.0));
        c.minimum_size = Size::new(80.0, 0.0);
        assert_eq!(c.clamp(Size::new(50.0, 20.0)), Size::new(80.0, 20.0));
        c.maximum_size = Size::new(60.0, 0.0);
        assert_eq!(c.clamp(Size::new(500.0, 20.0)).width, 60.0);
    }

    #[test]
    fn the_display_rect_is_deflated_by_padding() {
        let mut c = ControlBase::new();
        c.set_bounds(Rect::new(0.0, 0.0, 100.0, 50.0));
        c.padding = Padding::new(5.0, 4.0, 3.0, 2.0);
        let d = c.display_rect();
        assert_eq!((d.left, d.top, d.right, d.bottom), (5.0, 4.0, 97.0, 48.0));
    }

    /// Padding larger than the box must not produce an inverted rectangle.
    #[test]
    fn the_display_rect_never_inverts() {
        let mut c = ControlBase::new();
        c.set_bounds(Rect::new(0.0, 0.0, 10.0, 10.0));
        c.padding = Padding::all(30.0);
        let d = c.display_rect();
        assert!(d.right >= d.left && d.bottom >= d.top);
    }
}
