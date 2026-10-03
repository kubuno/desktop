//! Honouring the properties every control inherits (`vskubuno/docs/EVENTS.md` §16, "WinForms-rich
//! property sets"): `Enabled`, `Visible`, `BackColor`, `ForeColor`, `Font`, `RightToLeft`,
//! `BackgroundImage`, `BorderStyle`, `Cursor`, `UseWaitCursor`, `ToolTip`, `TabIndex`, `TabStop`,
//! `Margin`, `Padding`, `MinimumSize`, `MaximumSize`, the accessibility properties, `ContextMenu`,
//! `AllowDrop`, `GenerateMember`…
//!
//! Every element is wrapped in a `crate::design::DesignSlot`; the slot reads these properties once
//! at build time ([`CommonProps::read`], `None` when the element sets none of them — then nothing
//! changes, not even a branch per property) and applies them around its node's paint every frame:
//!
//! - **Visible** `false`: at run time the element is neither painted nor measured, takes no input,
//!   no focus and is absent from the accessibility tree. The designer still shows it (WinForms).
//! - **Enabled** `false`: the element and everything in it is drawn in its disabled look (the
//!   widgets read [`rest`]), receives no pointer input (the frame it sees has the pointer away), and
//!   leaves the Tab order; a focused descendant loses the focus.
//! - **Colours, font, right to left**: the subtree paints through a
//!   `kubuno_controls::styled::StyledCanvas` whose theme tokens (text, faces) and text formats are
//!   overridden — so the `kubuno_ui` widgets, which paint with the ambient theme and formats, follow
//!   without knowing. A control that paints its own face (a button, a text field) gets its face
//!   tokens replaced; any other gets its box filled.
//! - **Layout**: `MinimumSize`/`MaximumSize` clamp the box and the measure, `Margin` spaces the
//!   element in a flow (`<Stack>`), `Padding` insets a container's children and grows a leaf's
//!   measured size.
//! - **Frame services** ([`FrameServices`], collected while the tree paints and applied by the
//!   runtime once it has): the pointer shape over the deepest hovered element that asks for one,
//!   the tooltip of the deepest hovered element that has one, the accessibility tree, the mnemonics
//!   (`&Save`) and the context menus.

use std::cell::Cell;

use kubuno_controls::host::{self, Cursor, Frame};
use kubuno_controls::styled::{StyledCanvas, D2D1_COLOR_F};
use kubuno_controls::ControlCanvas;
use kubuno_ui::{Padding, Rect, Size, Theme, WidgetState};

use crate::binding::{PropSource, ViewModel};
use crate::props::{BuildError, Props};
use crate::registry::{ComponentMeta, LayoutKind};
use crate::style::{self, ColorValue, FontSpec};

// ── Ambient state (read by the widgets' nodes) ───────────────────────────────

thread_local! {
    /// How many disabled elements the element painting now is inside (0: enabled).
    static DISABLED: Cell<u32> = const { Cell::new(0) };
    /// The frame being painted is the designer's (Visible is ignored there, like in WinForms).
    static DESIGN: Cell<bool> = const { Cell::new(false) };
}

/// The resting [`WidgetState`] of a widget painted now: disabled inside a disabled element. What a
/// node starts its state from (instead of `WidgetState::REST`).
pub fn rest() -> WidgetState {
    WidgetState::REST.disabled(is_disabled())
}

/// Whether the element painting now is inside a disabled element (`Enabled="false"` on it or on a
/// container of it).
pub fn is_disabled() -> bool {
    DISABLED.with(Cell::get) > 0
}

/// Marks the frame about to be painted as the designer's (or not) — set by the runtime.
pub(crate) fn set_design_frame(design: bool) {
    DESIGN.with(|d| d.set(design));
}

pub(crate) fn design_frame() -> bool {
    DESIGN.with(Cell::get)
}

/// Keeps [`is_disabled`] true while it lives.
struct DisabledScope;

impl DisabledScope {
    fn enter() -> Self {
        DISABLED.with(|d| d.set(d.get() + 1));
        DisabledScope
    }
}

impl Drop for DisabledScope {
    fn drop(&mut self) {
        DISABLED.with(|d| d.set(d.get().saturating_sub(1)));
    }
}

// ── What an element sets ─────────────────────────────────────────────────────

/// How a `BackColor` reaches the element's pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Face {
    /// The control paints its own face (a button, a text field, a card): its face tokens change.
    Own,
    /// Anything else: its box is filled.
    Fill,
}

/// The elements that paint their own face (see [`Face`]).
const OWN_FACE: &[&str] = &[
    "Button", "IconButton", "TextField", "TextArea", "MaskedField", "SearchField", "Dropdown", "ComboBox", "NumericField", "DatePicker",
    "Card", "GroupBox", "Callout", "Badge", "ColorField", "GradientField",
];

/// A text property that may be bound, parsed each frame (the literal once).
#[derive(Debug, Clone)]
enum Parsed<T> {
    Literal(T),
    Bound(crate::binding::BindingSpec),
}

impl<T: Clone> Parsed<T> {
    fn read(props: &Props<'_>, name: &str, parse: impl Fn(&str) -> Option<T>) -> Result<Option<Self>, BuildError> {
        if !props.has(name) {
            return Ok(None);
        }
        Ok(match props.str(name, "")? {
            PropSource::Literal(s) => parse(&s).map(Parsed::Literal),
            PropSource::Bound { spec, .. } => Some(Parsed::Bound(spec)),
        })
    }

    fn resolve(&self, vm: &dyn ViewModel, parse: impl Fn(&str) -> Option<T>) -> Option<T> {
        match self {
            Parsed::Literal(v) => Some(v.clone()),
            Parsed::Bound(spec) => match crate::resources::get(vm, spec)? {
                crate::binding::Value::Str(s) => parse(&s),
                crate::binding::Value::Bool(b) => parse(if b { "true" } else { "false" }),
                crate::binding::Value::F32(v) => parse(&v.to_string()),
                crate::binding::Value::List(_) | crate::binding::Value::Object(_) => None,
            },
        }
    }
}

fn color(s: &str) -> Option<ColorValue> {
    style::parse_color(s).ok().flatten()
}

fn font(s: &str) -> Option<FontSpec> {
    style::parse_font(s).ok().flatten()
}

fn text(s: &str) -> Option<String> {
    Some(s.to_string())
}

/// The inherited properties an element sets (see the module doc), read once at build time.
pub struct CommonProps {
    visible: Option<PropSource<bool>>,
    /// `d:Visible="false"` in a designer process: the one way to hide a control on the design surface (a
    /// pane the data would hide at run time), since the designer otherwise shows every control.
    design_hidden: bool,
    /// Part of the view of a user control used by another view: the designer shows it as it would run (its
    /// `Visible` resolved), like a Windows Forms user control dropped on a form.
    in_user_control: bool,
    enabled: Option<PropSource<bool>>,
    back_color: Option<Parsed<ColorValue>>,
    fore_color: Option<Parsed<ColorValue>>,
    font: Option<Parsed<FontSpec>>,
    /// `Some(true)`/`Some(false)` for `Yes`/`No`; `None` inherits.
    right_to_left: Option<bool>,
    face: Face,
    /// `UseVisualStyleBackColor` as written (a button keeps its theme face when it is `true`).
    visual_style_back_color: Option<bool>,
    background_image: Option<(String, kubuno_controls::ImageLayout)>,
    border: Option<kubuno_controls::BorderStyle>,
    cursor: Option<Parsed<String>>,
    wait_cursor: Option<PropSource<bool>>,
    tooltip: Option<PropSource<String>>,
    tab_stop: Option<bool>,
    margin: Option<Padding>,
    /// `Control.Padding` (not an element's own `Padding`, which its node reads itself).
    padding: Option<Padding>,
    min_size: Option<(f32, f32)>,
    max_size: Option<(f32, f32)>,
    accessible_name: Option<PropSource<String>>,
    accessible_description: Option<PropSource<String>>,
    accessible_role: Option<String>,
    context_menu: Option<String>,
    allow_drop: bool,
    /// `Locked` (design time): the designer does not move or resize the element.
    locked: bool,
    container: bool,
}

/// The names of the properties [`CommonProps`] reads (an element writing none of them gets none).
const COMMON_NAMES: &[&str] = &[
    "Visible", "Enabled", "BackColor", "ForeColor", "Font", "RightToLeft", "UseVisualStyleBackColor", "BackgroundImage", "BorderStyle", "Cursor",
    "UseWaitCursor", "ToolTip", "TabStop", "Margin", "Padding", "MinimumSize", "MaximumSize", "AccessibleName", "AccessibleDescription",
    "AccessibleRole", "ContextMenu", "AllowDrop", "AutoSize", "Locked",
];

/// Whether `element` writes `AutoSize="true"`: its container then sizes it to its content and its
/// `Width`/`Height` only act as a floor (`AutoSizeMode="GrowOnly"`, the default) or not at all
/// (`GrowAndShrink`).
pub fn auto_sized(element: &crate::ast::Element) -> bool {
    element.attribute("AutoSize").and_then(|a| a.value()).is_some_and(|v| v.trim() == "true")
}

impl CommonProps {
    /// What `element` sets of the inherited property set; `None` when it sets none (the element
    /// then paints exactly as without this module). `base_dir` resolves relative image paths.
    pub fn read(props: &Props<'_>, meta: &'static ComponentMeta, base_dir: Option<&std::path::Path>) -> Result<Option<Self>, BuildError> {
        let element = props.element();
        if !COMMON_NAMES.iter().any(|n| element.attribute(n).is_some() && meta.own_property(n).is_none()) {
            return Ok(None);
        }
        // An element's own property of the same name (a `Stack`'s `Padding`, a `Timer`'s
        // `Enabled`) is its node's business, not this module's.
        let inherited = |name: &str| meta.own_property(name).is_none() && element.attribute(name).is_some();
        let bool_of = |name: &str, default: bool| -> Result<Option<PropSource<bool>>, BuildError> {
            if inherited(name) {
                props.bool(name, default).map(Some)
            } else {
                Ok(None)
            }
        };
        let literal = |name: &str| -> Option<String> { if inherited(name) { element.attribute(name)?.value() } else { None } };
        let literal_bool = |name: &str| literal(name).map(|v| v.trim() == "true");
        let resolve_path = |path: String| -> String {
            // `{Res banner}` → `kbres:banner`, looked up when painted (vskubuno docs/RESOURCES.md).
            if let Some(uri) = crate::resources::image_uri(&path) {
                return uri;
            }
            match base_dir {
                Some(dir) if std::path::Path::new(&path).is_relative() => dir.join(&path).to_string_lossy().into_owned(),
                _ => path,
            }
        };
        let layout = match literal("BackgroundImageLayout").as_deref() {
            Some("None") => kubuno_controls::ImageLayout::None,
            Some("Center") => kubuno_controls::ImageLayout::Center,
            Some("Stretch") => kubuno_controls::ImageLayout::Stretch,
            Some("Zoom") => kubuno_controls::ImageLayout::Zoom,
            _ => kubuno_controls::ImageLayout::Tile,
        };
        let border = match literal("BorderStyle").as_deref() {
            Some("FixedSingle") => Some(kubuno_controls::BorderStyle::FixedSingle),
            Some("Fixed3D") => Some(kubuno_controls::BorderStyle::Fixed3D),
            _ => None,
        };
        let str_of = |name: &str| -> Result<Option<PropSource<String>>, BuildError> {
            if inherited(name) {
                props.str(name, "").map(Some)
            } else {
                Ok(None)
            }
        };
        let parsed = |name: &str, f: fn(&str) -> Option<ColorValue>| -> Result<Option<Parsed<ColorValue>>, BuildError> {
            if inherited(name) {
                Parsed::read(props, name, f)
            } else {
                Ok(None)
            }
        };
        Ok(Some(Self {
            visible: bool_of("Visible", true)?,
            design_hidden: crate::design::design_time()
                && element.attribute("d:Visible").and_then(|a| a.value()).is_some_and(|v| v.trim() == "false"),
            in_user_control: crate::node::custom::inside_user_control(),
            enabled: bool_of("Enabled", true)?,
            back_color: parsed("BackColor", color)?,
            fore_color: parsed("ForeColor", color)?,
            font: if inherited("Font") { Parsed::read(props, "Font", font)? } else { None },
            right_to_left: match literal("RightToLeft").as_deref() {
                Some("Yes") => Some(true),
                Some("No") => Some(false),
                _ => None,
            },
            face: if OWN_FACE.contains(&meta.name) || meta.is_a("ButtonBase") && meta.name == "Button" { Face::Own } else { Face::Fill },
            visual_style_back_color: literal_bool("UseVisualStyleBackColor"),
            background_image: literal("BackgroundImage").filter(|p| !p.trim().is_empty()).map(|p| (resolve_path(p), layout)),
            border,
            cursor: if inherited("Cursor") { Parsed::read(props, "Cursor", text)? } else { None },
            wait_cursor: bool_of("UseWaitCursor", false)?,
            tooltip: str_of("ToolTip")?,
            tab_stop: literal_bool("TabStop"),
            margin: literal("Margin").and_then(|v| style::parse_padding(&v).ok()),
            padding: literal("Padding").and_then(|v| style::parse_padding(&v).ok()),
            min_size: {
                let min = literal("MinimumSize").and_then(|v| style::parse_size(&v).ok());
                // `AutoSize` with `GrowOnly`: the written `Width`/`Height` stay the smallest size.
                let grow_only = literal("AutoSizeMode").is_none_or(|m| m.trim() != "GrowAndShrink");
                if literal_bool("AutoSize") == Some(true) && grow_only {
                    let num = |name: &str| element.attribute(name).and_then(|a| a.value()).and_then(|v| v.trim().parse::<f32>().ok()).unwrap_or(0.0);
                    let (w, h) = min.unwrap_or((0.0, 0.0));
                    Some((w.max(num("Width")), h.max(num("Height"))))
                } else {
                    min
                }
            },
            max_size: literal("MaximumSize").and_then(|v| style::parse_size(&v).ok()),
            accessible_name: str_of("AccessibleName")?,
            accessible_description: str_of("AccessibleDescription")?,
            accessible_role: literal("AccessibleRole").filter(|r| r != "Default"),
            context_menu: literal("ContextMenu").map(|m| crate::menus::reference_name(&m)).filter(|m| !m.trim().is_empty()),
            allow_drop: literal_bool("AllowDrop").unwrap_or(false),
            locked: literal_bool("Locked").unwrap_or(false),
            container: meta.children != crate::registry::ChildrenModel::None,
        }))
    }

    /// Whether the element is shown this frame (always in the designer, unless `d:Visible="false"` hides it there).
    pub fn visible(&self, vm: &dyn ViewModel) -> bool {
        if design_frame() && !self.in_user_control {
            return !self.design_hidden;
        }
        self.visible.as_ref().is_none_or(|v| v.resolve(vm))
    }

    /// Whether the element itself is enabled (not counting its containers).
    pub fn enabled(&self, vm: &dyn ViewModel) -> bool {
        self.enabled.as_ref().is_none_or(|v| v.resolve(vm))
    }

    /// The element's `TabStop` (`None` when not written).
    pub fn tab_stop(&self) -> Option<bool> {
        self.tab_stop
    }

    /// The name of the `<ContextMenu>` the element opens on a right click.
    pub fn context_menu(&self) -> Option<&str> {
        self.context_menu.as_deref()
    }

    /// Whether the designer keeps the element where it is (`Locked`).
    pub fn locked(&self) -> bool {
        self.locked
    }

    /// Whether files may be dropped on the element.
    pub fn allow_drop(&self) -> bool {
        self.allow_drop
    }

    /// The element's tooltip this frame (empty: none).
    pub fn tooltip(&self, vm: &dyn ViewModel) -> Option<String> {
        self.tooltip.as_ref().map(|t| t.resolve(vm)).filter(|t| !t.trim().is_empty())
    }

    /// The element's pointer shape this frame, `None` for its own.
    pub fn cursor(&self, vm: &dyn ViewModel) -> Option<Cursor> {
        if self.wait_cursor.as_ref().is_some_and(|w| w.resolve(vm)) {
            return Some(Cursor::Wait);
        }
        self.cursor.as_ref().and_then(|c| c.resolve(vm, text)).and_then(|c| style::cursor(&c))
    }

    /// The accessible name, description and role the element declares.
    pub fn accessibility(&self, vm: &dyn ViewModel) -> (Option<String>, Option<String>, Option<&str>) {
        (
            self.accessible_name.as_ref().map(|n| n.resolve(vm)).filter(|n| !n.is_empty()),
            self.accessible_description.as_ref().map(|d| d.resolve(vm)).filter(|d| !d.is_empty()),
            self.accessible_role.as_deref(),
        )
    }

    /// `size` clamped by `MinimumSize`/`MaximumSize` (0 = no limit on that axis).
    pub fn clamp(&self, size: Size) -> Size {
        let (mut w, mut h) = (size.width, size.height);
        if let Some((mw, mh)) = self.min_size {
            if mw > 0.0 {
                w = w.max(mw);
            }
            if mh > 0.0 {
                h = h.max(mh);
            }
        }
        if let Some((xw, xh)) = self.max_size {
            if xw > 0.0 {
                w = w.min(xw);
            }
            if xh > 0.0 {
                h = h.min(xh);
            }
        }
        Size::new(w, h)
    }

    /// The element's `Margin` and `Padding` (the paint debug overlay shows them).
    pub fn margin_padding(&self) -> (Option<Padding>, Option<Padding>) {
        (self.margin, self.padding)
    }

    /// The element's box inside the `bounds` its container gave it: `Margin` removed in a flow,
    /// then clamped by the size limits (keeping the top-left corner, like WinForms).
    pub fn place(&self, bounds: Rect, parent_layout: LayoutKind) -> Rect {
        let mut b = bounds;
        if parent_layout == LayoutKind::Flow {
            if let Some(m) = self.margin {
                b = Rect::new(b.left + m.left, b.top + m.top, (b.right - m.right).max(b.left + m.left), (b.bottom - m.bottom).max(b.top + m.top));
            }
        }
        let size = self.clamp(Size::new(b.right - b.left, b.bottom - b.top));
        Rect::new(b.left, b.top, b.left + size.width, b.top + size.height)
    }

    /// The element's measured size, adjusted: a leaf's `Padding` added, the limits applied, and in a
    /// flow its `Margin` around it.
    pub fn measure(&self, inner: Size, parent_layout: LayoutKind) -> Size {
        let mut s = inner;
        if !self.container {
            if let Some(p) = self.padding {
                s = Size::new(s.width + p.left + p.right, s.height + p.top + p.bottom);
            }
        }
        let s = self.clamp(s);
        match (parent_layout, self.margin) {
            (LayoutKind::Flow, Some(m)) => Size::new(s.width + m.left + m.right, s.height + m.top + m.bottom),
            _ => s,
        }
    }

    /// Runs `measure` on `canvas` with the element's `Font` (and reading order) applied, as its paint
    /// applies them: a size taken from the text (an `AutoSize` label in a `Stack`) fits the text painted.
    pub fn measuring<R>(&self, canvas: &dyn kubuno_ui::Canvas, vm: &dyn ViewModel, measure: impl FnOnce(&dyn kubuno_ui::Canvas) -> R) -> R {
        let spec = self.font.as_ref().and_then(|f| f.resolve(vm, font));
        let formats = match (&spec, self.right_to_left) {
            (None, None | Some(false)) => None,
            _ => kubuno_controls::styled::measure_formats(&spec.clone().unwrap_or_default().text_style(self.right_to_left.unwrap_or(false))),
        };
        match formats.as_deref() {
            Some(f) => measure(&StyledCanvas::new(canvas).with_formats(f)),
            None => measure(canvas),
        }
    }

    /// Where the node paints inside the element's box: a container's children area is inset by
    /// `Padding` (a leaf keeps its whole box; its padding grew its size instead).
    pub fn content(&self, bounds: Rect) -> Rect {
        match (self.container, self.padding) {
            (true, Some(p)) => Rect::new(
                bounds.left + p.left,
                bounds.top + p.top,
                (bounds.right - p.right).max(bounds.left + p.left),
                (bounds.bottom - p.bottom).max(bounds.top + p.top),
            ),
            _ => bounds,
        }
    }

    /// Whether the element paints a box of its own through these properties (`BackColor`,
    /// `BackgroundImage`, a `BorderStyle` other than `None`): such a container is visible without
    /// the designer's dashed outline.
    pub fn paints_box(&self) -> bool {
        self.back_color.is_some()
            || self.background_image.is_some()
            || self.border.is_some_and(|b| b != kubuno_controls::BorderStyle::None)
    }

    /// Paints the element through `paint` with its colours, font, background, border and enabled
    /// state applied (see the module doc). `enabled` is the element's own `Enabled` this frame.
    pub fn paint_styled(&self, cx: &mut crate::node::PaintCx<'_>, bounds: Rect, enabled: bool, paint: impl FnOnce(&mut crate::node::PaintCx<'_>, Rect)) {
        let vm = &*cx.vm;
        let canvas = cx.canvas;
        let back = self.back_color.as_ref().and_then(|c| c.resolve(vm, color));
        let back = back.filter(|_| !(self.face == Face::Own && self.visual_style_back_color == Some(true)));
        let fore = self.fore_color.as_ref().and_then(|c| c.resolve(vm, color));
        let font = self.font.as_ref().and_then(|f| f.resolve(vm, font));

        let theme = (back.is_some() || fore.is_some()).then(|| {
            let mut theme = canvas.theme().clone();
            if let Some(b) = &back {
                apply_back_value(&mut theme, b, b.resolve(canvas), self.face);
            }
            if let Some(f) = &fore {
                apply_fore(&mut theme, f.resolve(canvas));
            }
            theme
        });
        let rtl = self.right_to_left.unwrap_or(false);
        let formats = match (&font, self.right_to_left) {
            (None, None | Some(false)) => None,
            _ => {
                let spec = font.clone().unwrap_or_default();
                kubuno_controls::styled::styled_formats(canvas, &spec.text_style(rtl))
            }
        };
        let (underline, strikeout) = font.as_ref().map(|f| (f.underline, f.strikeout)).unwrap_or((false, false));

        // The box: filled background, then the background image.
        if let (Some(b), Face::Fill) = (&back, self.face) {
            canvas.fill_rounded(&bounds, 0.0, &b.resolve(canvas));
        }
        if let Some((path, layout)) = &self.background_image {
            if let Some(bitmap) = kubuno_controls::styled::load_image(canvas, path) {
                kubuno_controls::styled::draw_image(canvas, &bitmap, bounds, *layout);
            }
        }

        let _disabled = (!enabled).then(DisabledScope::enter);
        let masked;
        let frame: &Frame = if enabled || design_frame() {
            cx.frame
        } else {
            masked = away(cx.frame);
            &masked
        };
        let pushed_bg = back.as_ref().map(|b| b.resolve(canvas));
        if let Some(bg) = pushed_bg {
            canvas.push_bg(bg);
        }
        let content = self.content(bounds);
        if theme.is_some() || formats.is_some() || underline || strikeout {
            let mut styled = StyledCanvas::new(canvas).with_decorations(underline, strikeout);
            if let Some(t) = theme.as_ref() {
                styled = styled.with_theme(t);
            }
            if let Some(f) = formats.as_deref() {
                styled = styled.with_formats(f);
            }
            let mut inner = cx.with_surface(&styled, frame);
            paint(&mut inner, content);
        } else if !std::ptr::eq(frame, cx.frame) {
            let mut inner = cx.with_surface(canvas, frame);
            paint(&mut inner, content);
        } else {
            paint(cx, content);
        }
        if pushed_bg.is_some() {
            canvas.pop_bg();
        }
        if let Some(border) = self.border {
            paint_border(canvas, bounds, border);
        }
    }
}

/// A frame where the pointer is away and no button is down — what a disabled element sees, and the
/// view under an open context menu.
pub(crate) fn away_frame(frame: &Frame) -> Frame {
    away(frame)
}

fn away(frame: &Frame) -> Frame {
    Frame {
        mouse: (host::POINTER_AWAY, host::POINTER_AWAY),
        mouse_down: false,
        right_down: false,
        middle_down: false,
        wheel: (0.0, 0.0),
        click_count: 0,
        ..*frame
    }
}

/// A colour a little darker (or lighter, for a dark colour): the hover/pressed steps of a face.
fn shade(c: D2D1_COLOR_F, amount: f32) -> D2D1_COLOR_F {
    let light = 0.2126 * c.r + 0.7152 * c.g + 0.0722 * c.b > 0.5;
    let k = if light { 1.0 - amount } else { 1.0 + amount };
    D2D1_COLOR_F { r: (c.r * k).clamp(0.0, 1.0), g: (c.g * k).clamp(0.0, 1.0), b: (c.b * k).clamp(0.0, 1.0), a: c.a }
}

/// The theme tokens a `BackColor` replaces: the surfaces, and for a control that paints its own
/// face, the face colours of its variants (with their hover and pressed steps).
/// The theme the children of an element with `BackColor="value"` (resolved: `c`) paint with.
///
/// A theme surface (`Background`, `Surface`…) on a container is a ground the design system's own
/// surfaces compose with: a card or a white panel on it keeps its own colour, and only the ground
/// itself changes. A free colour is the Windows Forms ambient `BackColor`: the children take it.
fn apply_back_value(theme: &mut Theme, value: &ColorValue, c: D2D1_COLOR_F, face: Face) {
    match value {
        ColorValue::Token(_) if face == Face::Fill => theme.window_background = c,
        _ => apply_back(theme, c, face),
    }
}

fn apply_back(theme: &mut Theme, c: D2D1_COLOR_F, face: Face) {
    theme.window_background = c;
    theme.layer_background = c;
    theme.card_background = c;
    theme.toolbar_background = c;
    if face == Face::Own {
        theme.accent = c;
        theme.accent_hover = shade(c, 0.12);
        theme.surface_2 = shade(c, 0.06);
        theme.surface_3 = shade(c, 0.12);
        theme.control_fill_hover = shade(c, 0.06);
        theme.control_fill_pressed = shade(c, 0.12);
        theme.card_preview_background = c;
    }
}

/// The theme tokens a `ForeColor` replaces: every text colour, including the text on the accent.
fn apply_fore(theme: &mut Theme, c: D2D1_COLOR_F) {
    theme.text_primary = c;
    theme.text_secondary = c;
    theme.accent_foreground = c;
    theme.text_nav_active = c;
}

/// `BorderStyle`: a one-pixel line (`FixedSingle`) or a sunken edge (`Fixed3D`) around `bounds`.
fn paint_border(c: &dyn ControlCanvas, bounds: Rect, border: kubuno_controls::BorderStyle) {
    match border {
        kubuno_controls::BorderStyle::FixedSingle => c.stroke_rect(&bounds, &c.theme().border_strong),
        kubuno_controls::BorderStyle::Fixed3D => {
            c.draw_edge(&bounds, kubuno_controls::Border3DStyle::Sunken, kubuno_controls::Border3DSide::ALL);
        }
        kubuno_controls::BorderStyle::None => {}
    }
}

// ── Frame services ───────────────────────────────────────────────────────────

/// What a mnemonic (`&Save`) does when Alt and its letter are pressed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MnemonicAction {
    /// Activate the element (a button's click, a check box's toggle).
    Activate,
    /// Move the focus to the next control in the Tab order (a label's).
    FocusNext,
}

/// One mnemonic painted this frame.
#[derive(Debug, Clone)]
pub struct Mnemonic {
    pub key: char,
    /// The stable id of the element.
    pub element: String,
    pub action: MnemonicAction,
}

/// What the elements asked of the window this frame (see the module doc), collected while the tree
/// paints and applied by the runtime afterwards.
#[derive(Default)]
pub struct FrameServices {
    /// How deep the element painting now is.
    pub(crate) depth: u32,
    /// The pointer shape of the deepest hovered element that asks for one.
    pub(crate) cursor: Option<(u32, Cursor)>,
    /// The tooltip of the deepest hovered element that has one: its text and client box.
    pub(crate) tooltip: Option<(u32, String, Rect, String)>,
    /// The accessibility tree, in paint order.
    pub(crate) access: Vec<host::access::AccessNode>,
    /// The accessibility nodes of the elements being painted, outermost first: the parent of the
    /// next node is the last one (a user control's elements under the user control, a Repeater's
    /// items under the Repeater), whatever their ids say.
    pub(crate) access_parents: Vec<u64>,
    /// The mnemonics painted, in paint order.
    pub(crate) mnemonics: Vec<Mnemonic>,
    /// The elements with a context menu: stable id, client box, menu name.
    pub(crate) context_menus: Vec<(String, Rect, String)>,
    /// The elements accepting dropped files: stable id, client box.
    pub(crate) drop_targets: Vec<(String, Rect)>,
    /// Elements whose mnemonic must activate them this frame (set by the runtime before painting).
    pub(crate) activate: Vec<String>,
    /// Each node of the accessibility tree: its id, the element's stable id and focus id.
    pub(crate) access_ids: Vec<(u64, String, Option<kubuno_ui::FocusId>)>,
    /// A text field used Enter as a submit (see [`submit`]).
    pub(crate) submitted: bool,
    /// The elements with bound properties (DATA-2 error glyphs): stable id, content box, client box,
    /// the paths they are bound to.
    pub(crate) bound: Vec<(String, Rect, Rect, std::rc::Rc<[String]>)>,
    /// The context menus declared in the nested views painted (a user control's own `<ContextMenu>`): the window
    /// opens them like the page's, their handlers run on the user control.
    pub(crate) local_menus: Vec<LocalMenu>,
    /// The menu bars painted (`crate::menus`): their top-level items, for the runtime that moves between
    /// their menus (hover, Left/Right, Alt, F10).
    pub(crate) menu_bars: Vec<crate::menus::BarInfo>,
    /// The focusable controls of the window's title band (`TitleBar.Region` children and the standard header items),
    /// in paint order: F6 (and Alt alone, when the view has no menu bar) moves the focus between them and the page.
    pub(crate) title_band_focus: Vec<kubuno_ui::FocusId>,
}

/// A context menu of a nested view (see [`FrameServices::local_menus`]).
#[derive(Clone)]
pub(crate) struct LocalMenu {
    /// Its key in [`FrameServices::context_menus`] (unique per user control instance).
    pub(crate) key: String,
    /// Its `x:Name` in its view (what the user control's own code opens it by).
    pub(crate) name: String,
    pub(crate) spec: std::rc::Rc<crate::window::MenuSpec>,
    /// Where its handlers run.
    pub(crate) scope: Option<std::rc::Rc<dyn crate::events::router::DispatchScope>>,
}

impl FrameServices {
    pub fn new() -> Self {
        Self::default()
    }

    /// Records the pointer shape of a hovered element at the current depth.
    pub(crate) fn offer_cursor(&mut self, cursor: Cursor) {
        if self.cursor.is_none_or(|(d, _)| self.depth >= d) {
            self.cursor = Some((self.depth, cursor));
        }
    }

    /// Records the tooltip of a hovered element at the current depth.
    pub(crate) fn offer_tooltip(&mut self, text: String, client: Rect, element: &str) {
        if self.tooltip.as_ref().is_none_or(|(d, ..)| self.depth >= *d) {
            self.tooltip = Some((self.depth, text, client, element.to_string()));
        }
    }

    /// Whether the element `id` is to be activated by its mnemonic this frame (taken once).
    pub(crate) fn take_activation(&mut self, id: &str) -> bool {
        match self.activate.iter().position(|a| a == id) {
            Some(i) => {
                self.activate.remove(i);
                true
            }
            None => false,
        }
    }
}

/// `bounds` (content coordinates) in client coordinates.
pub(crate) fn to_client(bounds: Rect) -> Rect {
    let (dx, dy) = host::content_offset();
    Rect::new(bounds.left + dx, bounds.top + dy, bounds.right + dx, bounds.bottom + dy)
}

/// The id of an element in the accessibility tree: its stable path hashed (never 0, the window's).
pub fn access_id(stable_id: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in stable_id.bytes().chain(std::iter::once(0xFF)) {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h.max(1)
}

/// The role an element reports when its `AccessibleRole` is `Default`, from its class.
pub fn default_role(meta: &ComponentMeta) -> host::access::AccessRole {
    use host::access::AccessRole as R;
    match meta.name {
        "Button" | "IconButton" | "ToolbarItem" => R::Button,
        "CheckBox" | "CheckedListBox" => if meta.name == "CheckBox" { R::CheckBox } else { R::List },
        "RadioButton" => R::RadioButton,
        "Switch" => R::Switch,
        "TextField" | "MaskedField" | "SearchField" | "NumericField" => if meta.name == "NumericField" { R::SpinButton } else { R::TextInput },
        "TextArea" => R::MultilineTextInput,
        "Label" | "Badge" => R::Label,
        "LinkLabel" | "BreadcrumbItem" => R::Link,
        "ListBox" | "ListView" => R::List,
        "ComboBox" | "Dropdown" | "DatePicker" => R::ComboBox,
        "Slider" => R::Slider,
        "ProgressBar" | "Spinner" => R::ProgressIndicator,
        "Tabs" => R::TabList,
        "TabItem" => R::Tab,
        "DataTable" => R::Table,
        "TreeView" => R::Tree,
        "Toolbar" => R::Toolbar,
        "Icon" => R::Image,
        "Separator" => R::Separator,
        "Callout" => R::Alert,
        "MenuBar" => R::MenuBar,
        "DropDownButton" | "SplitButton" => R::Button,
        _ if meta.children != crate::registry::ChildrenModel::None => R::Group,
        _ => R::Unknown,
    }
}

/// The role of a WinForms `AccessibleRole` value.
pub fn role_of(name: &str) -> Option<host::access::AccessRole> {
    use host::access::AccessRole as R;
    Some(match name {
        "PushButton" | "ButtonDropDown" | "ButtonMenu" | "SplitButton" | "OutlineButton" => R::Button,
        "CheckButton" => R::CheckBox,
        "RadioButton" => R::RadioButton,
        "Text" => R::TextInput,
        "StaticText" => R::Label,
        "Link" => R::Link,
        "List" => R::List,
        "ListItem" => R::ListItem,
        "ComboBox" | "DropList" => R::ComboBox,
        "Slider" | "Dial" => R::Slider,
        "ProgressBar" => R::ProgressIndicator,
        "SpinButton" => R::SpinButton,
        "PageTab" => R::Tab,
        "PageTabList" => R::TabList,
        "PropertyPage" => R::TabPanel,
        "Table" => R::Table,
        "Row" => R::Row,
        "Cell" => R::Cell,
        "ColumnHeader" | "RowHeader" | "Column" => R::ColumnHeader,
        "Outline" => R::Tree,
        "OutlineItem" => R::TreeItem,
        "ToolBar" => R::Toolbar,
        "StatusBar" => R::StatusBar,
        "Graphic" | "Animation" | "Chart" | "Diagram" | "Equation" => R::Image,
        "Separator" => R::Separator,
        "Alert" | "HelpBalloon" => R::Alert,
        "Dialog" => R::Dialog,
        "MenuPopup" => R::Menu,
        "MenuItem" => R::MenuItem,
        "MenuBar" => R::MenuBar,
        "ScrollBar" => R::ScrollBar,
        "ToolTip" => R::Tooltip,
        "Document" => R::Document,
        "Window" | "Client" | "Application" => R::Window,
        "Pane" | "Border" => R::Pane,
        "Grouping" => R::Group,
        "None" | "WhiteSpace" | "Grip" | "Sound" | "Cursor" | "Caret" | "TitleBar" | "Indicator" | "Character" | "HotkeyField" | "Clock"
        | "IpAddress" | "ButtonDropDownGrid" => R::Unknown,
        _ => return None,
    })
}

/// A text attribute's display form and its mnemonic (`&Save` → `("Save", Some(('s', 0)))`):
/// `&&` is a literal ampersand; the letter after a single `&` is the shortcut (its index in the
/// displayed text, in characters).
pub fn mnemonic(text: &str) -> (String, Option<(char, usize)>) {
    let mut out = String::with_capacity(text.len());
    let mut key = None;
    let mut chars = text.chars().peekable();
    let mut index = 0;
    while let Some(c) = chars.next() {
        if c == '&' {
            match chars.next() {
                Some('&') => {
                    out.push('&');
                    index += 1;
                }
                Some(next) => {
                    if key.is_none() && !next.is_whitespace() {
                        key = Some((next.to_lowercase().next().unwrap_or(next), index));
                    }
                    out.push(next);
                    index += 1;
                }
                None => out.push('&'),
            }
        } else {
            out.push(c);
            index += 1;
        }
    }
    (out, key)
}

/// The text a control shows for `text` and whether it underlines a letter: with `UseMnemonic`
/// (`use_mnemonic`), the ampersands are removed; the shortcut letter is underlined only while Alt
/// is held (the Windows "keyboard cues"). Registers the mnemonic with the frame's services.
pub(crate) fn mnemonic_text(
    text: &str,
    use_mnemonic: bool,
    frame: &Frame,
    services: Option<&mut FrameServices>,
    element: Option<&str>,
    action: MnemonicAction,
) -> (String, Option<usize>) {
    if !use_mnemonic || !text.contains('&') {
        return (text.to_string(), None);
    }
    let (display, key) = mnemonic(text);
    let Some((key, index)) = key else { return (display, None) };
    if let (Some(services), Some(element)) = (services, element) {
        services.mnemonics.push(Mnemonic { key, element: element.to_string(), action });
    }
    // Underlined while Alt is held (the Windows keyboard cues), and always in the designer.
    (display, (frame.mods.alt || design_frame()).then_some(index))
}

// ── Level properties the nodes read (`ButtonBase`, `LabelBase`, `TextBoxBase`) ──────────────────

/// A `ContentAlignment` value.
pub fn content_alignment(name: &str) -> Option<kubuno_controls::ContentAlignment> {
    use kubuno_controls::ContentAlignment as A;
    Some(match name.trim() {
        "TopLeft" => A::TopLeft,
        "TopCenter" => A::TopCenter,
        "TopRight" => A::TopRight,
        "MiddleLeft" => A::MiddleLeft,
        "MiddleCenter" => A::MiddleCenter,
        "MiddleRight" => A::MiddleRight,
        "BottomLeft" => A::BottomLeft,
        "BottomCenter" => A::BottomCenter,
        "BottomRight" => A::BottomRight,
        _ => return None,
    })
}

pub(crate) fn literal_attr(props: &Props<'_>, name: &str) -> Option<String> {
    props.element().attribute(name).and_then(|a| a.value()).map(|v| v.trim().to_string()).filter(|v| !v.is_empty() && !crate::binding::is_binding_expr(v))
}

pub(crate) fn resolve_path(path: String, base_dir: Option<&std::path::Path>) -> String {
    match base_dir {
        Some(dir) if std::path::Path::new(&path).is_relative() => dir.join(&path).to_string_lossy().into_owned(),
        _ => path,
    }
}

/// What a button node reads of `ButtonBase`: `TextAlign`, `Image`, `ImageAlign`,
/// `TextImageRelation`, `UseMnemonic`.
#[derive(Debug, Clone, PartialEq)]
pub struct ButtonBaseProps {
    pub text_align: kubuno_controls::ContentAlignment,
    pub image: Option<String>,
    pub image_align: kubuno_controls::ContentAlignment,
    pub relation: kubuno_controls::buttons::TextImageRelation,
    pub use_mnemonic: bool,
    /// `IconSize`: the icon's own size, in DIP.
    pub icon_size: Option<f32>,
    /// `IconSpacing`: the gap between the icon (or the image) and the text, in DIP.
    pub icon_spacing: Option<f32>,
    /// `ImageAlign` or `TextImageRelation` is written: the icon is laid out like an image.
    pub icon_aligned: bool,
    /// `TextImageRelation` is written (else an icon goes before the text).
    pub relation_written: bool,
}

impl Default for ButtonBaseProps {
    fn default() -> Self {
        Self {
            text_align: kubuno_controls::ContentAlignment::MiddleCenter,
            image: None,
            image_align: kubuno_controls::ContentAlignment::MiddleCenter,
            relation: kubuno_controls::buttons::TextImageRelation::Overlay,
            use_mnemonic: true,
            icon_size: None,
            icon_spacing: None,
            icon_aligned: false,
            relation_written: false,
        }
    }
}

impl ButtonBaseProps {
    /// The `ButtonBase` properties the element writes (literal values), the defaults for the rest.
    pub fn read(props: &Props<'_>, base_dir: Option<&std::path::Path>) -> Self {
        use kubuno_controls::buttons::TextImageRelation as R;
        let d = Self::default();
        Self {
            text_align: literal_attr(props, "TextAlign").and_then(|v| content_alignment(&v)).unwrap_or(d.text_align),
            // A file relative to the view, or a resource (`{Res key}`, read as `kbres:…` by `Props::str`).
            // `{Res key}` is a binding-shaped value (`Props::str` gives a `Bound` source for it): its `kbres:` URI is
            // built here, and resolved in the current culture when painted.
            image: props.element().attribute("Image").and_then(|a| a.value()).map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
                .and_then(|p| crate::resources::image_uri(&p).or_else(|| (!p.starts_with('{')).then(|| resolve_path(p.clone(), base_dir)))),
            image_align: literal_attr(props, "ImageAlign").and_then(|v| content_alignment(&v)).unwrap_or(d.image_align),
            relation: match literal_attr(props, "TextImageRelation").as_deref() {
                Some("ImageAboveText") => R::ImageAboveText,
                Some("TextAboveImage") => R::TextAboveImage,
                Some("ImageBeforeText") => R::ImageBeforeText,
                Some("TextBeforeImage") => R::TextBeforeImage,
                _ => R::Overlay,
            },
            use_mnemonic: literal_attr(props, "UseMnemonic").is_none_or(|v| v != "false"),
            icon_size: literal_attr(props, "IconSize").and_then(|v| crate::icon::parse_icon_size(&v).ok().flatten()).map(|(w, h)| w.min(h)),
            icon_spacing: literal_attr(props, "IconSpacing").and_then(|v| v.trim().parse::<f32>().ok()).filter(|v| v.is_finite() && *v >= 0.0),
            icon_aligned: literal_attr(props, "ImageAlign").is_some() || literal_attr(props, "TextImageRelation").is_some(),
            relation_written: literal_attr(props, "TextImageRelation").is_some(),
        }
    }
}

/// What a text field node reads of `TextBoxBase` (`ReadOnly` may be bound).
pub struct TextBoxProps {
    pub read_only: Option<PropSource<bool>>,
    pub max_length: Option<i32>,
    pub accepts_tab: bool,
    pub password_char: Option<char>,
    pub casing: kubuno_controls::text::CharacterCasing,
    pub hide_selection: bool,
    pub text_align: Option<kubuno_controls::HorizontalAlignment>,
    /// `AcceptsReturn` (a text area: `false` makes Enter the view's default button's).
    pub accepts_return: bool,
    /// `WordWrap` (a text area).
    pub word_wrap: bool,
}

impl TextBoxProps {
    /// The `TextBoxBase` properties the element writes, the defaults for the rest.
    pub fn read(props: &Props<'_>) -> Result<Self, BuildError> {
        use kubuno_controls::text::CharacterCasing as C;
        Ok(Self {
            read_only: if props.has("ReadOnly") { Some(props.bool("ReadOnly", false)?) } else { None },
            max_length: literal_attr(props, "MaxLength").and_then(|v| v.parse::<f32>().ok()).map(|v| v.max(0.0) as i32),
            accepts_tab: literal_attr(props, "AcceptsTab").is_some_and(|v| v == "true"),
            password_char: literal_attr(props, "PasswordChar").and_then(|v| v.chars().next()),
            casing: match literal_attr(props, "CharacterCasing").as_deref() {
                Some("Upper") => C::Upper,
                Some("Lower") => C::Lower,
                _ => C::Normal,
            },
            hide_selection: literal_attr(props, "HideSelection").is_none_or(|v| v != "false"),
            text_align: match literal_attr(props, "TextAlign").as_deref() {
                Some("Right") => Some(kubuno_controls::HorizontalAlignment::Right),
                Some("Center") => Some(kubuno_controls::HorizontalAlignment::Center),
                Some("Left") => Some(kubuno_controls::HorizontalAlignment::Left),
                _ => None,
            },
            accepts_return: literal_attr(props, "AcceptsReturn").is_none_or(|v| v != "false"),
            word_wrap: literal_attr(props, "WordWrap").is_none_or(|v| v != "false"),
        })
    }

    /// Applies the `TextBoxBase` ones to any text box model (a masked one included).
    pub fn apply_base(&self, tb: &mut kubuno_controls::text::TextBoxBase, vm: &dyn ViewModel) {
        if let Some(r) = &self.read_only {
            tb.read_only = r.resolve(vm);
        }
        if let Some(m) = self.max_length {
            tb.max_length = m;
        }
        tb.accepts_tab = self.accepts_tab;
        tb.hide_selection = self.hide_selection;
    }

    /// Applies them to a text box model before it is edited and painted this frame.
    pub fn apply(&self, tb: &mut kubuno_controls::text::TextBox, vm: &dyn ViewModel) {
        self.apply_base(tb, vm);
        if self.password_char.is_some() {
            tb.password_char = self.password_char;
        }
        tb.character_casing = self.casing;
        if let Some(a) = self.text_align {
            tb.text_align = a;
        }
    }

    /// The focus options of the field: it keeps Tab when it accepts tabs.
    pub fn focus_opts(&self, multiline: bool) -> kubuno_ui::FocusOpts {
        kubuno_ui::FocusOpts { wants_tab: self.accepts_tab && multiline, ..kubuno_ui::FocusOpts::TEXT }
    }
}

/// Records that a text field used Enter as a submit this frame (a single-line field, or a text
/// area with `AcceptsReturn = false`): the view's `AcceptButton` then gets it.
pub(crate) fn submit(services: Option<&mut FrameServices>) {
    if let Some(s) = services {
        s.submitted = true;
    }
}

// ── AutoScroll ──────────────────────────────────────────────────────────────────────────────────

/// Whether `element` scrolls its content (`AutoScroll="true"` on a scrollable control or the view).
pub fn auto_scrolls(element: &crate::ast::Element, meta: &ComponentMeta, is_root: bool) -> bool {
    (is_root || meta.is_a("ScrollableControl"))
        && meta.own_property("AutoScroll").is_none()
        && element.attribute("AutoScroll").and_then(|a| a.value()).is_some_and(|v| v.trim() == "true")
}

/// `AutoScroll`: the element's content is laid out at least at its measured size and scrolled
/// inside the element's box, with the Kubuno scroll bars (the designer shows it unscrolled).
pub struct AutoScrollNode {
    area: kubuno_ui::containers::ScrollArea,
    inner: Box<dyn crate::node::ViewNode>,
}

impl AutoScrollNode {
    pub fn new(inner: Box<dyn crate::node::ViewNode>) -> Self {
        Self { area: kubuno_ui::containers::ScrollArea::new(), inner }
    }

    /// The box the content is laid out in: `bounds`, grown to the content's own size.
    pub fn content_bounds(bounds: Rect, content: Size) -> Rect {
        Rect::new(bounds.left, bounds.top, bounds.right.max(bounds.left + content.width), bounds.bottom.max(bounds.top + content.height))
    }
}

impl crate::node::ViewNode for AutoScrollNode {
    fn measure(&self, c: &dyn kubuno_ui::Canvas, vm: &dyn ViewModel) -> Size {
        self.inner.measure(c, vm)
    }

    fn measure_for_width(&self, c: &dyn kubuno_ui::Canvas, vm: &dyn ViewModel, width: f32) -> Size {
        self.inner.measure_for_width(c, vm, width)
    }

    fn intrinsic_width(&self, c: &dyn kubuno_ui::Canvas, vm: &dyn ViewModel) -> Option<f32> {
        self.inner.intrinsic_width(c, vm)
    }

    fn paint(&mut self, cx: &mut crate::node::PaintCx<'_>, bounds: Rect) {
        if design_frame() {
            self.inner.paint(cx, bounds);
            return;
        }
        let content = Self::content_bounds(bounds, self.inner.measure(cx.canvas, cx.vm));
        // Same pattern as `ScrollAreaNode::paint`: the area pushes the clip/offset on the canvas
        // and hands back the translated frame; the content paints through the original canvas.
        let outer_canvas = cx.canvas;
        let canvas: &dyn kubuno_ui::Canvas = outer_canvas;
        let outer_frame = cx.frame;
        let inner_node = &mut self.inner;
        let mut inner = cx.reborrow();
        self.area.frame(canvas, bounds, outer_frame, move |_c, f| {
            let mut content_cx = inner.with_surface(outer_canvas, f);
            inner_node.paint(&mut content_cx, content);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{AstNode, Document};

    fn common(src: &str) -> Option<CommonProps> {
        let parse = crate::syntax::parse(src);
        let element = Document::cast(parse.syntax()).and_then(|d| d.root_element()).unwrap();
        let meta = crate::registry::lookup(&element.name().unwrap()).unwrap();
        CommonProps::read(&Props::new(&element, meta), meta, Some(std::path::Path::new("C:/views"))).unwrap()
    }

    #[test]
    fn an_element_without_inherited_properties_gets_nothing() {
        assert!(common(r#"<Button Text="Ok" X="4"/>"#).is_none());
        assert!(common(r#"<Button Text="Ok" Enabled="false"/>"#).is_some());
    }

    #[test]
    fn visible_and_enabled_resolve_their_bindings() {
        let c = common(r#"<Button Visible="{Binding Shown}" Enabled="false"/>"#).unwrap();
        let mut vm = crate::binding::MapViewModel::new();
        vm.set("Shown", crate::binding::Value::Bool(false));
        assert!(!c.visible(&vm));
        vm.set("Shown", crate::binding::Value::Bool(true));
        assert!(c.visible(&vm));
        assert!(!c.enabled(&vm));
        set_design_frame(true);
        vm.set("Shown", crate::binding::Value::Bool(false));
        assert!(c.visible(&vm), "the designer shows an invisible control");
        set_design_frame(false);
    }

    #[test]
    fn a_label_is_measured_in_its_own_font() {
        let width = |label: &str| {
            let view = crate::compile::compile(&format!(r#"<Stack Direction="LeftToRight">{label}</Stack>"#)).unwrap();
            let canvas = kubuno_ui::graphics::testing::RecordingCanvas::new();
            view.root.measure(&canvas, &crate::binding::MapViewModel::new()).width
        };
        let plain = width(r#"<Label Text="Administrateurs" AutoSize="true"/>"#);
        let large = width(r#"<Label Text="Administrateurs" AutoSize="true" Font="Segoe UI, 20pt, style=Bold"/>"#);
        assert!(large > plain * 1.5, "an AutoSize label in a Stack fits the font it paints in ({large} vs {plain})");
    }

    #[test]
    fn size_limits_margin_and_padding_shape_the_box() {
        let c = common(r#"<Button MinimumSize="100, 0" MaximumSize="0, 30" Margin="4, 2, 4, 2" Padding="10"/>"#).unwrap();
        assert_eq!(c.clamp(Size::new(50.0, 50.0)), Size::new(100.0, 30.0));
        // In a flow the margin is outside the box; elsewhere it is ignored (WinForms).
        let placed = c.place(Rect::new(0.0, 0.0, 200.0, 40.0), LayoutKind::Flow);
        assert_eq!((placed.left, placed.top, placed.right, placed.bottom), (4.0, 2.0, 196.0, 32.0));
        let placed = c.place(Rect::new(0.0, 0.0, 200.0, 40.0), LayoutKind::DockAnchor);
        assert_eq!((placed.left, placed.top, placed.right, placed.bottom), (0.0, 0.0, 200.0, 30.0));
        // A leaf's padding grows its measure; the margin surrounds it in a flow.
        assert_eq!(c.measure(Size::new(60.0, 20.0), LayoutKind::Flow), Size::new(108.0, 34.0));
        assert_eq!(c.measure(Size::new(60.0, 20.0), LayoutKind::None), Size::new(100.0, 30.0));
        assert_eq!(c.content(Rect::new(0.0, 0.0, 100.0, 30.0)).left, 0.0, "a leaf keeps its box");
        let panel = common(r#"<Panel Padding="8"/>"#);
        assert!(panel.is_none(), "a Panel's own Padding is its node's");
        let stack = common(r#"<ScrollArea Padding="6, 6, 6, 6"/>"#).unwrap();
        assert_eq!(stack.content(Rect::new(0.0, 0.0, 100.0, 100.0)).left, 6.0, "a container's children are inset");
    }

    #[test]
    fn colours_fonts_and_images_are_parsed_once() {
        let c = common(r##"<Label BackColor="#FF8800" ForeColor="TextSecondary" Font="Consolas, 12pt, style=Bold" BackgroundImage="img/logo.png" BackgroundImageLayout="Zoom" BorderStyle="FixedSingle"/>"##).unwrap();
        let vm = crate::binding::MapViewModel::new();
        assert!(matches!(c.back_color.as_ref().and_then(|b| b.resolve(&vm, color)), Some(ColorValue::Rgba(_))));
        assert_eq!(c.fore_color.as_ref().and_then(|f| f.resolve(&vm, color)), Some(ColorValue::Token("TextSecondary")));
        assert_eq!(c.font.as_ref().and_then(|f| f.resolve(&vm, font)).and_then(|f| f.size_px), Some(16.0));
        let (path, layout) = c.background_image.clone().unwrap();
        assert!(path.replace('\\', "/").ends_with("C:/views/img/logo.png"), "{path}");
        assert_eq!(layout, kubuno_controls::ImageLayout::Zoom);
        assert_eq!(c.border, Some(kubuno_controls::BorderStyle::FixedSingle));
        assert_eq!(c.face, Face::Fill);
        assert_eq!(common(r#"<Button BackColor="Danger"/>"#).unwrap().face, Face::Own);
    }

    #[test]
    fn a_back_colour_replaces_the_faces_of_a_button_and_the_surfaces_of_the_rest() {
        let red = D2D1_COLOR_F { r: 1.0, g: 0.0, b: 0.0, a: 1.0 };
        let mut t = Theme::light();
        apply_back(&mut t, red, Face::Own);
        assert_eq!((t.accent.r, t.card_background.r, t.window_background.r), (1.0, 1.0, 1.0));
        let mut t = Theme::light();
        apply_back(&mut t, red, Face::Fill);
        assert_eq!(t.card_background.r, 1.0);
        assert_ne!(t.accent.r, 1.0, "a filled element keeps the accent");
        // A theme surface keeps the design system's own surfaces (a white card on a grey page).
        let mut t = Theme::light();
        let white = t.layer_background;
        apply_back_value(&mut t, &ColorValue::Token("Background"), red, Face::Fill);
        assert_eq!((t.window_background.r, t.layer_background), (1.0, white));
        let mut t = Theme::light();
        apply_back_value(&mut t, &ColorValue::Rgba(crate::style::Rgba::rgb(255, 0, 0)), red, Face::Fill);
        assert_eq!(t.layer_background.r, 1.0, "a free colour is ambient");
        let mut t = Theme::light();
        apply_fore(&mut t, red);
        assert_eq!((t.text_primary.r, t.accent_foreground.r), (1.0, 1.0));
    }

    #[test]
    fn cursor_tooltip_and_accessibility_are_read() {
        let c = common(r#"<Button Cursor="Hand" ToolTip="Saves the file" AccessibleName="Save" AccessibleRole="PushButton" TabStop="false"/>"#).unwrap();
        let vm = crate::binding::MapViewModel::new();
        assert_eq!(c.cursor(&vm), Some(Cursor::Hand));
        assert_eq!(c.tooltip(&vm).as_deref(), Some("Saves the file"));
        assert_eq!(c.accessibility(&vm), (Some("Save".to_string()), None, Some("PushButton")));
        assert_eq!(c.tab_stop(), Some(false));
        let w = common(r#"<Button Cursor="Hand" UseWaitCursor="true"/>"#).unwrap();
        assert_eq!(w.cursor(&vm), Some(Cursor::Wait), "the busy pointer wins");
        assert_eq!(role_of("PushButton"), Some(host::access::AccessRole::Button));
        assert!(crate::registry::common::ACCESSIBLE_ROLES.iter().all(|r| *r == "Default" || role_of(r).is_some()), "every role maps");
    }

    #[test]
    fn mnemonics_are_read_like_winforms() {
        assert_eq!(mnemonic("&Save"), ("Save".to_string(), Some(('s', 0))));
        assert_eq!(mnemonic("Save &as"), ("Save as".to_string(), Some(('a', 5))));
        assert_eq!(mnemonic("Fish && Chips"), ("Fish & Chips".to_string(), None));
        assert_eq!(mnemonic("Trailing&"), ("Trailing&".to_string(), None));
        assert_eq!(mnemonic("A & B"), ("A  B".to_string(), None), "a space is no shortcut");
    }

    #[test]
    fn disabled_scopes_nest_and_color_the_rest_state() {
        assert!(!rest().disabled);
        {
            let _a = DisabledScope::enter();
            let _b = DisabledScope::enter();
            assert!(rest().disabled);
        }
        assert!(!is_disabled());
        let f = away(&Frame {
            size: (10.0, 10.0),
            mouse: (5.0, 5.0),
            mouse_down: true,
            right_down: true,
            middle_down: false,
            dismiss: false,
            scale: 1.0,
            client_origin: (0.0, 0.0),
            work_area: (0.0, 0.0, 10.0, 10.0),
            chrome_top: 0.0,
            mods: host::Modifiers::NONE,
            wheel: (0.0, 1.0),
            click_count: 1,
            window_focused: true,
        });
        assert!(!f.mouse_down && !f.right_down && f.mouse.0 < -1000.0 && f.wheel == (0.0, 0.0));
    }

    #[test]
    fn access_ids_are_stable_and_never_the_window() {
        assert_eq!(access_id("0.1"), access_id("0.1"));
        assert_ne!(access_id("0.1"), access_id("0.2"));
        assert_ne!(access_id(""), 0);
    }
}
