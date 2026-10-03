//! `ToolStrip` and its two independent hierarchies.
//!
//! The ToolStrip family is the sharpest example of the library's central rule
//! (« do not re-implement what a base carries »), because it has **two** parallel
//! inheritance chains, and each one is a base carrying almost everything:
//!
//! * **The control chain** — `ToolStrip` is a `Control` (via `ScrollableControl`),
//!   and `MenuStrip`, `StatusStrip`, `ToolStripDropDown` → `ToolStripDropDownMenu`
//!   → `ContextMenuStrip` are `ToolStrip`s that mostly just change four defaults
//!   (`LayoutStyle`, `GripStyle`, `Dock`, `CanOverflow`/`Stretch`). It is mirrored
//!   here with composition + `Deref`, exactly like `ButtonBase`.
//!
//! * **The item chain** — the things *inside* a strip are **not** `Control`s.
//!   `ToolStripItem` is a `Component`, so it never appears in the reflection
//!   catalogue's control tree; its surface comes from the docs. `ToolStripItem`
//!   declares the whole shared item surface (`Text`, `Image`, `DisplayStyle`,
//!   the three alignments, `Overflow`, …) and the leaves add only their own:
//!   `ToolStripButton` adds `Checked`, `ToolStripMenuItem` adds `ShortcutKeys`,
//!   `ToolStripStatusLabel` adds `Spring`. This chain is mirrored the same way.
//!
//! ## Geometry stays pure
//!
//! As with `layout::layout`, everything a test can check without a window is a
//! free function: [`measure_item_content`] sizes one item from its
//! `DisplayStyle`/`TextImageRelation`, and [`layout_items`] lays a measured list
//! along the strip — honouring `Alignment`, the grip reserve, overflow into the
//! overflow button, and `StatusStrip`'s `Spring` sizing. The canvas-bound
//! `preferred_size`/`paint` only supply the one thing a test cannot: the text
//! width DirectWrite reports.
//!
//! ## What it paints with
//!
//! Every colour, metric and font below comes from [`crate::system`] — the
//! surface answers for them through [`ControlCanvas::visuals`]. Nothing here
//! reads the Kubuno theme or the embedded face: the oracle for this family is
//! the reference sheet painted by the real `System.Windows.Forms`, and a strip
//! drawn in another palette answers a different question however good it looks.
//! Concretely: the strip face is `Control` (`MenuBar` for a menu bar), item ink
//! is `ControlText`/`MenuText`, a disabled item is `GrayText`, a selected one is
//! `HighlightText` over `Highlight`, and the two decorations that are neither
//! text nor face — the separator and the move handle — are the toolkit's
//! **two-tone** pair of `ControlDark` and `ControlLightLight`, drawn through
//! [`ControlCanvas::draw_edge`] wherever the shape is a real 3-D edge.

use kubuno_drive_desktop_app_controls::{Canvas, Rect};
use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;
use windows::Win32::Graphics::DirectWrite::{
    DWriteCreateFactory, IDWriteFactory, IDWriteTextFormat, DWRITE_FACTORY_TYPE_SHARED,
    DWRITE_TEXT_ALIGNMENT, DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_LEADING,
    DWRITE_TEXT_ALIGNMENT_TRAILING,
};

use crate::control::{Control, ControlBase, ControlCanvas, FontRole};
use crate::enums::{BorderStyle, ContentAlignment, DockStyle, Padding, RightToLeft, Size};
use crate::system::{
    edge_interior, Border3DSide, Border3DStyle as EdgeStyle, SystemColors, SystemFonts,
    SystemMetrics,
};

// ─────────────────────────────────────────────────────────────────────────────
// Enumerations owned by this family
//
// These live here, not in `enums.rs`, because they are ToolStrip-specific: no
// other control references them. Members and discriminants are the toolkit's.
// ─────────────────────────────────────────────────────────────────────────────

/// How a `ToolStrip` arranges its items (`ToolStripLayoutStyle`). The declared
/// default is [`StackWithOverflow`](Self::StackWithOverflow), which resolves at
/// runtime to horizontal or vertical stacking from the strip's orientation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ToolStripLayoutStyle {
    Flow,
    #[default]
    StackWithOverflow,
    HorizontalStackWithOverflow,
    VerticalStackWithOverflow,
    Table,
}

/// The two axes a stack can run along. Not a WinForms type — it is what
/// [`ToolStripLayoutStyle::resolve`] turns the orientation-dependent
/// `StackWithOverflow` into, so [`layout_items`] never has to look at a dock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StripAxis {
    Horizontal,
    Vertical,
}

impl ToolStripLayoutStyle {
    /// Resolves to `(axis, spring_enabled)`. `StackWithOverflow` picks its axis
    /// from whether the strip is docked to a horizontal edge, mirroring how the
    /// toolkit derives `Orientation`. `Table` is treated as a single springing
    /// row (what a `StatusStrip` uses it for) and `Flow` as a vertical column
    /// (what a drop-down menu uses it for) — the two uses this port needs; other
    /// grid/wrapping behaviours of `Table`/`Flow` are not yet honoured.
    pub fn resolve(self, horizontal_dock: bool) -> (StripAxis, bool) {
        match self {
            Self::StackWithOverflow if horizontal_dock => (StripAxis::Horizontal, false),
            Self::StackWithOverflow => (StripAxis::Vertical, false),
            Self::HorizontalStackWithOverflow => (StripAxis::Horizontal, false),
            Self::VerticalStackWithOverflow => (StripAxis::Vertical, false),
            Self::Table => (StripAxis::Horizontal, true),
            Self::Flow => (StripAxis::Vertical, false),
        }
    }
}

/// Whether the move handle is drawn (`ToolStripGripStyle`). `ToolStrip` shows it;
/// `MenuStrip`/`StatusStrip` hide it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ToolStripGripStyle {
    Hidden,
    #[default]
    Visible,
}

/// Which renderer a strip uses (`ToolStripRenderMode`). The declared default is
/// [`ManagerRenderMode`](Self::ManagerRenderMode) for every strip type; the
/// *effective* renderer (professional vs system) differs, but that is a painting
/// concern outside the property surface and is not modelled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ToolStripRenderMode {
    Custom,
    System,
    Professional,
    #[default]
    ManagerRenderMode,
}

/// The direction a drop-down opens (`ToolStripDropDownDirection`). `Default`
/// asks the parent to choose, which is the property's own default value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ToolStripDropDownDirection {
    AboveLeft,
    AboveRight,
    BelowLeft,
    BelowRight,
    #[default]
    Default,
    Left,
    Right,
}

/// Text orientation on a strip (`ToolStripTextDirection`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ToolStripTextDirection {
    Inherit,
    #[default]
    Horizontal,
    Vertical90,
    Vertical270,
}

/// What an item paints (`ToolStripItemDisplayStyle`). Default is
/// [`ImageAndText`](Self::ImageAndText); hosts default to `None` (the hosted
/// control is the content).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ToolStripItemDisplayStyle {
    None = 0,
    Text = 1,
    Image = 2,
    #[default]
    ImageAndText = 3,
}

impl ToolStripItemDisplayStyle {
    pub const fn shows_text(self) -> bool {
        matches!(self, Self::Text | Self::ImageAndText)
    }
    pub const fn shows_image(self) -> bool {
        matches!(self, Self::Image | Self::ImageAndText)
    }
}

/// Which end of the strip an item aligns to (`ToolStripItemAlignment`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ToolStripItemAlignment {
    #[default]
    Left = 0,
    Right = 1,
}

/// Whether an item may move into the overflow menu (`ToolStripItemOverflow`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ToolStripItemOverflow {
    #[default]
    AsNeeded = 0,
    Always = 1,
    Never = 2,
}

/// How text and image are positioned relative to each other (`TextImageRelation`).
/// Default is [`ImageBeforeText`](Self::ImageBeforeText).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextImageRelation {
    Overlay = 0,
    #[default]
    ImageBeforeText = 1,
    TextBeforeImage = 2,
    ImageAboveText = 3,
    TextAboveImage = 4,
}

/// Whether an item's image is resized to the strip's `ImageScalingSize`
/// (`ToolStripItemImageScaling`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ToolStripItemImageScaling {
    None,
    #[default]
    SizeToFit,
}

/// Which sides of a `ToolStripStatusLabel` draw a border
/// (`ToolStripStatusLabelBorderSides`). A bitflag set; default `None`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToolStripStatusLabelBorderSides(pub u8);

impl ToolStripStatusLabelBorderSides {
    pub const NONE: Self = Self(0);
    pub const LEFT: Self = Self(1);
    pub const TOP: Self = Self(2);
    pub const RIGHT: Self = Self(4);
    pub const BOTTOM: Self = Self(8);
    pub const ALL: Self = Self(1 | 2 | 4 | 8);

    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
}

impl Default for ToolStripStatusLabelBorderSides {
    /// `ToolStripStatusLabelBorderSides.None` — the property's default.
    fn default() -> Self {
        Self::NONE
    }
}

/// The 3-D border look of a status label (`Border3DStyle`).
///
/// A twin of [`crate::system::Border3DStyle`], which is the *painting*
/// primitive's enum: this one is the property `ToolStripStatusLabel` declares,
/// with the default the toolkit gives it (`Flat`, where the system enum defaults
/// to `Raised`), and it is mapped onto the primitive at paint time. Every member
/// is honoured — `Flat` comes out as the single `ControlDark` ring it is, and the
/// bevels as the real two-tone `DrawEdge`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Border3DStyle {
    Adjust,
    Bump,
    Etched,
    #[default]
    Flat,
    Raised,
    RaisedInner,
    RaisedOuter,
    Sunken,
    SunkenInner,
    SunkenOuter,
}

/// How a link label underlines (`LinkBehavior`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LinkBehavior {
    #[default]
    SystemDefault,
    AlwaysUnderline,
    HoverUnderline,
    NeverUnderline,
}

/// How a `ToolStripProgressBar` renders progress (`ProgressBarStyle`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ProgressBarStyle {
    #[default]
    Blocks,
    Continuous,
    Marquee,
}

/// The editing behaviour of a hosted combo box (`ComboBoxStyle`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ComboBoxStyle {
    Simple,
    #[default]
    DropDown,
    DropDownList,
}

// ─────────────────────────────────────────────────────────────────────────────
// Keyboard shortcut — the payload of `ToolStripMenuItem.ShortcutKeys`
// ─────────────────────────────────────────────────────────────────────────────

/// A `ToolStripMenuItem` shortcut. WinForms stores a `Keys` value (a large flags
/// enum); the port keeps the three modifiers plus the key's display name, which
/// is all a menu needs to show « Ctrl+S » and measure the room it takes. An
/// absent shortcut is `None` on the item, matching `Keys.None`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Shortcut {
    pub ctrl:  bool,
    pub alt:   bool,
    pub shift: bool,
    /// The key's display name, e.g. `"S"`, `"F4"`, `"Del"`.
    pub key:   String,
}

impl Shortcut {
    pub fn new(ctrl: bool, alt: bool, shift: bool, key: impl Into<String>) -> Self {
        Self { ctrl, alt, shift, key: key.into() }
    }

    /// The default display string, in the toolkit's modifier order
    /// (Ctrl, Alt, Shift) joined with `+`. `ShortcutKeyDisplayString` overrides
    /// this, which is why it is a separate method and not baked into the item.
    pub fn display_string(&self) -> String {
        let mut parts: Vec<&str> = Vec::new();
        if self.ctrl {
            parts.push("Ctrl");
        }
        if self.alt {
            parts.push("Alt");
        }
        if self.shift {
            parts.push("Shift");
        }
        let mut s = parts.join("+");
        if !self.key.is_empty() {
            if !s.is_empty() {
                s.push('+');
            }
            s.push_str(&self.key);
        }
        s
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Pure measurement of a single item's content
// ─────────────────────────────────────────────────────────────────────────────

/// The gap between an image and text when an item shows both, and the internal
/// padding a text item reserves on each side. Kept as DIP constants — the device
/// context is already set to the window's DPI — because the toolkit's own values
/// are not exposed by reflection; they only affect spacing, never which member of
/// an enum is honoured.
pub const IMAGE_TEXT_GAP: f32 = 4.0;
/// Horizontal padding reserved around an item's text, per side.
pub const TEXT_PADDING_H: f32 = 6.0;

/// The content size of one item, given its already-measured `text` and `image`
/// sizes — the pure core of `GetPreferredSize`. Honours `DisplayStyle` (which of
/// text/image show at all) and, when both show, `TextImageRelation` (side by side
/// vs stacked vs overlaid). `Overlay` returns the larger of the two on each axis.
///
/// `text` should already be `Size::EMPTY` when there is no text to draw, and
/// likewise `image`; the caller decides that from `DisplayStyle` and whether the
/// item actually has an image. `gap` is the image/text separation.
pub fn measure_item_content(
    display: ToolStripItemDisplayStyle,
    relation: TextImageRelation,
    text: Size,
    image: Size,
    gap: f32,
) -> Size {
    let text_used = display.shows_text() && (text.width > 0.0 || text.height > 0.0);
    let image_used = display.shows_image() && (image.width > 0.0 || image.height > 0.0);

    match (text_used, image_used) {
        (false, false) => Size::EMPTY,
        (true, false) => text,
        (false, true) => image,
        (true, true) => match relation {
            // Side by side: widths add (plus the gap), height is the taller.
            TextImageRelation::ImageBeforeText | TextImageRelation::TextBeforeImage => {
                Size::new(image.width + gap + text.width, image.height.max(text.height))
            }
            // Stacked: heights add (plus the gap), width is the wider.
            TextImageRelation::ImageAboveText | TextImageRelation::TextAboveImage => {
                Size::new(image.width.max(text.width), image.height + gap + text.height)
            }
            // Overlaid: the bounding box of the two.
            TextImageRelation::Overlay => {
                Size::new(image.width.max(text.width), image.height.max(text.height))
            }
        },
    }
}

/// The content width of a drop-down menu item: a left gutter for the check mark
/// and image, the text, then a right column for the shortcut and the submenu
/// arrow. Pure, so the « shortcut widens the item » rule is testable without a
/// canvas — the caller passes the DirectWrite widths of the text and the
/// shortcut string.
pub fn measure_menu_item(
    text_width: f32,
    shortcut_width: f32,
    check_gutter: f32,
    arrow_column: f32,
    gap: f32,
) -> Size {
    let mut w = check_gutter + text_width + arrow_column;
    if shortcut_width > 0.0 {
        w += gap + shortcut_width;
    }
    Size::new(w, 0.0)
}

// ─────────────────────────────────────────────────────────────────────────────
// Pure strip layout: place measured items along the strip
// ─────────────────────────────────────────────────────────────────────────────

/// One item's input to [`layout_items`]: everything the placement needs without
/// borrowing a whole item. `size` is the item's measured outer size (content plus
/// its own margin); the engine treats the main axis only.
#[derive(Debug, Clone, Copy)]
pub struct StripItemInput {
    pub size:      Size,
    pub alignment: ToolStripItemAlignment,
    pub overflow:  ToolStripItemOverflow,
    pub spring:    bool,
    pub visible:   bool,
}

impl Default for StripItemInput {
    fn default() -> Self {
        Self {
            size: Size::EMPTY,
            alignment: ToolStripItemAlignment::Left,
            overflow: ToolStripItemOverflow::AsNeeded,
            spring: false,
            visible: true,
        }
    }
}

/// The result of laying items along a strip.
// No `Debug`: it carries `Rect`s, and the drawing layer's `Rect` is not `Debug`.
#[derive(Clone)]
pub struct StripLayout {
    /// Each item's rectangle inside the strip. An empty rect means the item is
    /// hidden or was pushed into the overflow menu (see `on_overflow`).
    pub rects:           Vec<Rect>,
    /// `true` where the matching item did not fit and moved to the overflow menu.
    pub on_overflow:     Vec<bool>,
    /// The overflow button's rectangle, present only when at least one item
    /// overflowed.
    pub overflow_button: Option<Rect>,
}

/// Lays measured `items` along `strip`, honouring:
/// * the `grip` reserve at the leading edge,
/// * `Alignment` (left items pack from the lead, right items from the trailing
///   end, both keeping collection order),
/// * overflow — when `can_overflow` and the content is too long, trailing
///   `AsNeeded` left-aligned items move into the overflow menu (reserving
///   `overflow_size` for its button); `Always` items always overflow and `Never`
///   items never do,
/// * `spring` — when `spring_enabled`, the leftover main-axis space is shared
///   equally among the springing items, the indivisible remainder going one unit
///   at a time to the earliest of them (what a `StatusStrip` does).
///
/// Pure: no canvas, so it is fully unit-tested.
pub fn layout_items(
    axis: StripAxis,
    strip: Rect,
    grip: f32,
    overflow_size: f32,
    can_overflow: bool,
    spring_enabled: bool,
    items: &[StripItemInput],
) -> StripLayout {
    let horizontal = axis == StripAxis::Horizontal;
    let axis_len = if horizontal { strip.right - strip.left } else { strip.bottom - strip.top };
    let n = items.len();

    let main = |s: Size| if horizontal { s.width } else { s.height };
    let mut len: Vec<f32> = items.iter().map(|it| main(it.size)).collect();

    // ── Overflow classification ─────────────────────────────────────────────
    // Start with `Always` items overflowed, then, while the on-strip run is too
    // long, push the last movable item out until it fits.
    let mut overflow = vec![false; n];
    for (i, it) in items.iter().enumerate() {
        if it.visible && it.overflow == ToolStripItemOverflow::Always {
            overflow[i] = true;
        }
    }

    let on_strip_len = |overflow: &[bool]| -> f32 {
        items
            .iter()
            .enumerate()
            .filter(|(i, it)| it.visible && !overflow[*i])
            .map(|(i, _)| len[i])
            .sum()
    };

    if can_overflow {
        loop {
            let any_over = overflow.iter().enumerate().any(|(i, &o)| o && items[i].visible);
            let reserved = if any_over { overflow_size } else { 0.0 };
            let cap = (axis_len - grip - reserved).max(0.0);
            if on_strip_len(&overflow) <= cap {
                break;
            }
            // Push the last left-aligned, on-strip, `AsNeeded` item to overflow.
            let victim = (0..n).rev().find(|&i| {
                items[i].visible
                    && !overflow[i]
                    && items[i].overflow == ToolStripItemOverflow::AsNeeded
                    && items[i].alignment == ToolStripItemAlignment::Left
            });
            match victim {
                Some(i) => overflow[i] = true,
                None => break, // nothing movable left; the rest simply clips
            }
        }
    }

    let overflow_shown = can_overflow && overflow.iter().enumerate().any(|(i, &o)| o && items[i].visible);
    let reserved_end = if overflow_shown { overflow_size } else { 0.0 };

    // ── Spring distribution ─────────────────────────────────────────────────
    if spring_enabled {
        let springs: Vec<usize> = (0..n)
            .filter(|&i| items[i].visible && !overflow[i] && items[i].spring)
            .collect();
        if !springs.is_empty() {
            let used = on_strip_len(&overflow);
            let leftover = axis_len - grip - reserved_end - used;
            if leftover > 0.0 {
                let k = springs.len() as f32;
                // Whole-unit share plus a remainder handed out one unit at a
                // time, so the total is preserved even when it does not divide.
                let base = (leftover / k).floor();
                let remainder = (leftover - base * k).round() as usize;
                for (rank, &i) in springs.iter().enumerate() {
                    len[i] += base + if rank < remainder { 1.0 } else { 0.0 };
                }
            }
        }
    }

    // ── Placement ────────────────────────────────────────────────────────────
    // Left group packs forward from the grip; right group is positioned so its
    // first item sits just left of its total width, preserving collection order.
    let right_total: f32 = (0..n)
        .filter(|&i| {
            items[i].visible && !overflow[i] && items[i].alignment == ToolStripItemAlignment::Right
        })
        .map(|i| len[i])
        .sum();

    let mut left_cursor = grip;
    let mut right_cursor = (axis_len - reserved_end - right_total).max(grip);

    let mut starts = vec![0.0_f32; n];
    for i in 0..n {
        if !items[i].visible || overflow[i] {
            continue;
        }
        match items[i].alignment {
            ToolStripItemAlignment::Left => {
                starts[i] = left_cursor;
                left_cursor += len[i];
            }
            ToolStripItemAlignment::Right => {
                starts[i] = right_cursor;
                right_cursor += len[i];
            }
        }
    }

    // ── Build rectangles ─────────────────────────────────────────────────────
    let mut rects = vec![Rect::new(0.0, 0.0, 0.0, 0.0); n];
    for i in 0..n {
        if !items[i].visible || overflow[i] {
            continue;
        }
        rects[i] = if horizontal {
            Rect::new(strip.left + starts[i], strip.top, strip.left + starts[i] + len[i], strip.bottom)
        } else {
            Rect::new(strip.left, strip.top + starts[i], strip.right, strip.top + starts[i] + len[i])
        };
    }

    let overflow_button = if overflow_shown {
        Some(if horizontal {
            Rect::new(strip.right - overflow_size, strip.top, strip.right, strip.bottom)
        } else {
            Rect::new(strip.left, strip.bottom - overflow_size, strip.right, strip.bottom)
        })
    } else {
        None
    };

    StripLayout { rects, on_overflow: overflow, overflow_button }
}

// ─────────────────────────────────────────────────────────────────────────────
// The item hierarchy — `ToolStripItem` and its leaves
// ─────────────────────────────────────────────────────────────────────────────

/// The surface `System.Windows.Forms.ToolStripItem` declares — every item's
/// shared block, the counterpart of `ControlBase` for the (non-`Control`) item
/// tree. Colours and font are ambient (`None` = take the owner's), exactly as on
/// a control.
#[derive(Debug, Clone)]
pub struct ToolStripItem {
    pub name: String,
    pub text: String,
    /// `Image` — the port has no image object, so an image is modelled by the key
    /// the host would resolve it with; `None` means « no image ». Presence is
    /// what layout and paint need.
    pub image: Option<String>,
    pub tag:   Option<String>,

    pub display_style:        ToolStripItemDisplayStyle,
    pub text_align:           ContentAlignment,
    pub image_align:          ContentAlignment,
    pub text_image_relation:  TextImageRelation,
    pub image_scaling:        ToolStripItemImageScaling,

    pub alignment: ToolStripItemAlignment,
    pub overflow:  ToolStripItemOverflow,
    pub dock:      DockStyle,
    pub auto_size: bool,
    pub margin:    Padding,
    pub padding:   Padding,

    pub enabled:      bool,
    /// `Visible`/`Available` — the item's own visibility (distinct from whether
    /// it currently fits, which is an overflow decision made at layout time).
    pub visible:      bool,
    pub auto_tooltip: bool,
    pub tool_tip_text: String,
    pub right_to_left: RightToLeft,

    pub back_color: Option<D2D1_COLOR_F>,
    pub fore_color: Option<D2D1_COLOR_F>,
    pub font:       Option<FontRole>,
}

impl Default for ToolStripItem {
    /// The toolkit's declared item defaults: `DisplayStyle = ImageAndText`,
    /// both alignments `MiddleCenter`, `TextImageRelation = ImageBeforeText`,
    /// `Alignment = Left`, `Overflow = AsNeeded`, `AutoSize = true`,
    /// `AutoToolTip = false`, `ImageScaling = SizeToFit`, and the base
    /// `DefaultMargin` of `(0, 1, 0, 2)`.
    fn default() -> Self {
        Self {
            name: String::new(),
            text: String::new(),
            image: None,
            tag: None,
            display_style: ToolStripItemDisplayStyle::default(),
            text_align: ContentAlignment::MiddleCenter,
            image_align: ContentAlignment::MiddleCenter,
            text_image_relation: TextImageRelation::default(),
            image_scaling: ToolStripItemImageScaling::default(),
            alignment: ToolStripItemAlignment::default(),
            overflow: ToolStripItemOverflow::default(),
            dock: DockStyle::None,
            auto_size: true,
            margin: Padding::new(0.0, 1.0, 0.0, 2.0),
            padding: Padding::ZERO,
            enabled: true,
            visible: true,
            auto_tooltip: false,
            tool_tip_text: String::new(),
            right_to_left: RightToLeft::default(),
            back_color: None,
            fore_color: None,
            font: None,
        }
    }
}

impl ToolStripItem {
    pub fn new() -> Self {
        Self::default()
    }

    /// A text item's convenience constructor.
    pub fn with_text(text: impl Into<String>) -> Self {
        Self { text: text.into(), ..Self::default() }
    }
}

/// `ToolStripButton` — a clickable, optionally-toggling item. Over its base it
/// declares only the check state (and overrides `AutoToolTip` to default `true`,
/// applied in `Default`).
#[derive(Debug, Clone)]
pub struct ToolStripButton {
    pub item:           ToolStripItem,
    pub checked:        bool,
    pub check_on_click: bool,
    pub check_state:    crate::enums::CheckState,
}

impl Default for ToolStripButton {
    fn default() -> Self {
        Self {
            item: ToolStripItem { auto_tooltip: true, ..ToolStripItem::default() },
            checked: false,
            check_on_click: false,
            check_state: crate::enums::CheckState::Unchecked,
        }
    }
}

impl ToolStripButton {
    pub fn new(text: impl Into<String>) -> Self {
        let mut b = Self::default();
        b.item.text = text.into();
        b
    }
}

impl std::ops::Deref for ToolStripButton {
    type Target = ToolStripItem;
    fn deref(&self) -> &ToolStripItem {
        &self.item
    }
}
impl std::ops::DerefMut for ToolStripButton {
    fn deref_mut(&mut self) -> &mut ToolStripItem {
        &mut self.item
    }
}

/// `ToolStripLabel` — non-interactive text, optionally a hyperlink. Adds the link
/// properties. A link is drawn in `HotTrack`, the system's own hyperlink colour;
/// `LinkVisited` and `LinkBehavior` are still not honoured — the canvas offers no
/// underline, so the three underlining behaviours cannot be told apart, and the
/// visited colour has no `SystemColors` counterpart to take.
// Every field's WinForms default is its Rust default (`IsLink = false`,
// `LinkVisited = false`, `LinkBehavior = SystemDefault`), so the derive states
// that faithfully — a hand-written impl would only be a second place to drift.
#[derive(Debug, Clone, Default)]
pub struct ToolStripLabel {
    pub item:          ToolStripItem,
    pub is_link:       bool,
    pub link_visited:  bool,
    pub link_behavior: LinkBehavior,
}

impl ToolStripLabel {
    pub fn new(text: impl Into<String>) -> Self {
        Self { item: ToolStripItem::with_text(text), ..Self::default() }
    }
}

impl std::ops::Deref for ToolStripLabel {
    type Target = ToolStripItem;
    fn deref(&self) -> &ToolStripItem {
        &self.item
    }
}
impl std::ops::DerefMut for ToolStripLabel {
    fn deref_mut(&mut self) -> &mut ToolStripItem {
        &mut self.item
    }
}

/// `ToolStripStatusLabel` — a `StatusStrip`'s label. Adds `Spring` (share the
/// leftover width), a border-sides flag and a 3-D border style, and overrides the
/// base `DefaultMargin` to `(0, 3, 0, 2)`.
#[derive(Debug, Clone)]
pub struct ToolStripStatusLabel {
    pub item:         ToolStripItem,
    pub spring:       bool,
    pub border_sides: ToolStripStatusLabelBorderSides,
    pub border_style: Border3DStyle,
}

impl Default for ToolStripStatusLabel {
    fn default() -> Self {
        Self {
            item: ToolStripItem { margin: Padding::new(0.0, 3.0, 0.0, 2.0), ..ToolStripItem::default() },
            spring: false,
            border_sides: ToolStripStatusLabelBorderSides::default(),
            border_style: Border3DStyle::default(),
        }
    }
}

impl ToolStripStatusLabel {
    pub fn new(text: impl Into<String>) -> Self {
        let mut l = Self::default();
        l.item.text = text.into();
        l
    }
}

impl std::ops::Deref for ToolStripStatusLabel {
    type Target = ToolStripItem;
    fn deref(&self) -> &ToolStripItem {
        &self.item
    }
}
impl std::ops::DerefMut for ToolStripStatusLabel {
    fn deref_mut(&mut self) -> &mut ToolStripItem {
        &mut self.item
    }
}

/// `ToolStripSeparator` — a divider. Declares no property of its own; its whole
/// behaviour is a fixed thickness and a hairline.
#[derive(Debug, Clone, Default)]
pub struct ToolStripSeparator {
    pub item: ToolStripItem,
}

impl std::ops::Deref for ToolStripSeparator {
    type Target = ToolStripItem;
    fn deref(&self) -> &ToolStripItem {
        &self.item
    }
}
impl std::ops::DerefMut for ToolStripSeparator {
    fn deref_mut(&mut self) -> &mut ToolStripItem {
        &mut self.item
    }
}

/// `ToolStripDropDownItem` — the base of every item that opens a drop-down. It
/// declares `DropDownDirection` and owns the child items; `ToolStripMenuItem`
/// builds on it. Modelled as a struct so the menu item can compose it, mirroring
/// the .NET base/leaf split.
#[derive(Debug, Clone, Default)]
pub struct ToolStripDropDownItem {
    pub item:                ToolStripItem,
    pub drop_down_direction: ToolStripDropDownDirection,
    /// `DropDownItems` — the child menu; a leaf is included by boxing so the
    /// (recursive) menu tree stays a plain owned value.
    pub drop_down_items:     Vec<StripItem>,
}

impl std::ops::Deref for ToolStripDropDownItem {
    type Target = ToolStripItem;
    fn deref(&self) -> &ToolStripItem {
        &self.item
    }
}
impl std::ops::DerefMut for ToolStripDropDownItem {
    fn deref_mut(&mut self) -> &mut ToolStripItem {
        &mut self.item
    }
}

/// `ToolStripMenuItem` — a menu entry. Over `ToolStripDropDownItem` it declares
/// the shortcut trio and the check state.
#[derive(Debug, Clone)]
pub struct ToolStripMenuItem {
    pub base:                       ToolStripDropDownItem,
    pub shortcut_keys:              Option<Shortcut>,
    pub shortcut_key_display_string: Option<String>,
    pub show_shortcut_keys:         bool,
    pub checked:                    bool,
    pub check_on_click:             bool,
    pub check_state:                crate::enums::CheckState,
}

impl Default for ToolStripMenuItem {
    /// `ShowShortcutKeys = true`, no shortcut, unchecked — the toolkit's defaults.
    fn default() -> Self {
        Self {
            base: ToolStripDropDownItem::default(),
            shortcut_keys: None,
            shortcut_key_display_string: None,
            show_shortcut_keys: true,
            checked: false,
            check_on_click: false,
            check_state: crate::enums::CheckState::Unchecked,
        }
    }
}

impl ToolStripMenuItem {
    pub fn new(text: impl Into<String>) -> Self {
        let mut m = Self::default();
        m.base.item.text = text.into();
        m
    }

    /// The shortcut text a menu draws on the right — the explicit
    /// `ShortcutKeyDisplayString` if set, otherwise the shortcut's own string,
    /// and nothing at all when `ShowShortcutKeys` is off or there is no shortcut.
    pub fn shortcut_text(&self) -> String {
        if !self.show_shortcut_keys {
            return String::new();
        }
        if let Some(s) = &self.shortcut_key_display_string {
            return s.clone();
        }
        match &self.shortcut_keys {
            Some(sc) => sc.display_string(),
            None => String::new(),
        }
    }

    pub fn has_drop_down_items(&self) -> bool {
        !self.base.drop_down_items.is_empty()
    }
}

impl std::ops::Deref for ToolStripMenuItem {
    type Target = ToolStripDropDownItem;
    fn deref(&self) -> &ToolStripDropDownItem {
        &self.base
    }
}
impl std::ops::DerefMut for ToolStripMenuItem {
    fn deref_mut(&mut self) -> &mut ToolStripDropDownItem {
        &mut self.base
    }
}

/// `ToolStripControlHost` — the base that hosts a real control inside a strip.
/// Declares `ControlAlign` and overrides `DisplayStyle` to `None` (the hosted
/// control is the content). The hosted control itself is out of scope for the
/// item tree; the concrete hosts below model the properties they surface.
#[derive(Debug, Clone)]
pub struct ToolStripControlHost {
    pub item:          ToolStripItem,
    pub control_align: ContentAlignment,
}

impl Default for ToolStripControlHost {
    fn default() -> Self {
        Self {
            item: ToolStripItem {
                display_style: ToolStripItemDisplayStyle::None,
                ..ToolStripItem::default()
            },
            control_align: ContentAlignment::MiddleCenter,
        }
    }
}

impl std::ops::Deref for ToolStripControlHost {
    type Target = ToolStripItem;
    fn deref(&self) -> &ToolStripItem {
        &self.item
    }
}
impl std::ops::DerefMut for ToolStripControlHost {
    fn deref_mut(&mut self) -> &mut ToolStripItem {
        &mut self.item
    }
}

/// `ToolStripComboBox` — a combo box hosted in a strip. Models the combo surface
/// it declares (`DropDownStyle`, `Items`, `SelectedIndex`, …); the full
/// auto-complete and data-binding surface is not yet honoured.
#[derive(Debug, Clone)]
pub struct ToolStripComboBox {
    pub host:                ToolStripControlHost,
    pub drop_down_style:     ComboBoxStyle,
    pub items:               Vec<String>,
    pub selected_index:      i32,
    pub max_drop_down_items: i32,
    pub sorted:              bool,
    pub max_length:          i32,
}

impl Default for ToolStripComboBox {
    fn default() -> Self {
        Self {
            host: ToolStripControlHost::default(),
            drop_down_style: ComboBoxStyle::default(),
            items: Vec::new(),
            selected_index: -1,
            max_drop_down_items: 8,
            sorted: false,
            max_length: 0,
        }
    }
}

impl ToolStripComboBox {
    /// The text currently shown in the closed combo — the selected item, or the
    /// host's own text when nothing is selected.
    pub fn display_text(&self) -> &str {
        if self.selected_index >= 0 && (self.selected_index as usize) < self.items.len() {
            &self.items[self.selected_index as usize]
        } else {
            &self.host.item.text
        }
    }
}

impl std::ops::Deref for ToolStripComboBox {
    type Target = ToolStripControlHost;
    fn deref(&self) -> &ToolStripControlHost {
        &self.host
    }
}
impl std::ops::DerefMut for ToolStripComboBox {
    fn deref_mut(&mut self) -> &mut ToolStripControlHost {
        &mut self.host
    }
}

/// `ToolStripTextBox` — a text box hosted in a strip. Models the edit surface it
/// declares; the full `TextBoxBase` surface stays with the `text` family.
#[derive(Debug, Clone)]
pub struct ToolStripTextBox {
    pub host:         ToolStripControlHost,
    pub border_style: BorderStyle,
    pub read_only:    bool,
    pub max_length:   i32,
}

impl Default for ToolStripTextBox {
    fn default() -> Self {
        Self {
            host: ToolStripControlHost::default(),
            border_style: BorderStyle::Fixed3D,
            read_only: false,
            max_length: 32767,
        }
    }
}

impl std::ops::Deref for ToolStripTextBox {
    type Target = ToolStripControlHost;
    fn deref(&self) -> &ToolStripControlHost {
        &self.host
    }
}
impl std::ops::DerefMut for ToolStripTextBox {
    fn deref_mut(&mut self) -> &mut ToolStripControlHost {
        &mut self.host
    }
}

/// `ToolStripProgressBar` — a progress bar hosted in a strip. Its `Value` obeys
/// the same range trap as `ProgressBar`/`NumericUpDown`: it is clamped into
/// `[Minimum, Maximum]`, so callers set the range before the value.
#[derive(Debug, Clone)]
pub struct ToolStripProgressBar {
    pub host:                   ToolStripControlHost,
    pub minimum:                i32,
    pub maximum:                i32,
    value:                      i32,
    pub step:                   i32,
    pub style:                  ProgressBarStyle,
    pub marquee_animation_speed: i32,
    pub right_to_left_layout:   bool,
}

impl Default for ToolStripProgressBar {
    /// `Minimum = 0`, `Maximum = 100`, `Value = 0`, `Step = 10` — as declared.
    fn default() -> Self {
        Self {
            host: ToolStripControlHost::default(),
            minimum: 0,
            maximum: 100,
            value: 0,
            step: 10,
            style: ProgressBarStyle::default(),
            marquee_animation_speed: 100,
            right_to_left_layout: false,
        }
    }
}

impl ToolStripProgressBar {
    pub fn value(&self) -> i32 {
        self.value
    }

    /// Sets `Value`, clamped into `[Minimum, Maximum]` exactly as the toolkit
    /// does (it throws outside the range; the port clamps, which is the same
    /// observable result for a valid UI).
    pub fn set_value(&mut self, v: i32) {
        self.value = v.clamp(self.minimum, self.maximum);
    }

    /// The fill fraction 0..=1 for painting.
    fn fraction(&self) -> f32 {
        let span = (self.maximum - self.minimum) as f32;
        if span <= 0.0 {
            0.0
        } else {
            ((self.value - self.minimum) as f32 / span).clamp(0.0, 1.0)
        }
    }
}

impl std::ops::Deref for ToolStripProgressBar {
    type Target = ToolStripControlHost;
    fn deref(&self) -> &ToolStripControlHost {
        &self.host
    }
}
impl std::ops::DerefMut for ToolStripProgressBar {
    fn deref_mut(&mut self) -> &mut ToolStripControlHost {
        &mut self.host
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// `StripItem` — one owned value for the heterogeneous item collection
// ─────────────────────────────────────────────────────────────────────────────

/// A single entry in a strip's `Items` (or a menu's `DropDownItems`). WinForms
/// keeps a `ToolStripItemCollection` of the polymorphic base; the port keeps an
/// enum so the collection is a plain owned value, laid out and painted by
/// matching on the kind.
#[derive(Debug, Clone)]
pub enum StripItem {
    Button(ToolStripButton),
    Label(ToolStripLabel),
    StatusLabel(ToolStripStatusLabel),
    Separator(ToolStripSeparator),
    MenuItem(ToolStripMenuItem),
    ComboBox(ToolStripComboBox),
    TextBox(ToolStripTextBox),
    ProgressBar(ToolStripProgressBar),
}

/// The default host width for a combo/text/progress item before auto-sizing —
/// the toolkit's `DefaultSize.Width` for those hosts.
const HOST_DEFAULT_WIDTH: f32 = 100.0;
/// Fixed thickness of a separator across the strip's main axis.
const SEPARATOR_THICKNESS: f32 = 6.0;

impl StripItem {
    /// The shared item block, whatever the kind — the one accessor every generic
    /// operation (visibility, alignment, tooltip) goes through.
    pub fn item(&self) -> &ToolStripItem {
        match self {
            Self::Button(b) => &b.item,
            Self::Label(l) => &l.item,
            Self::StatusLabel(s) => &s.item,
            Self::Separator(s) => &s.item,
            Self::MenuItem(m) => &m.base.item,
            Self::ComboBox(c) => &c.host.item,
            Self::TextBox(t) => &t.host.item,
            Self::ProgressBar(p) => &p.host.item,
        }
    }

    fn spring(&self) -> bool {
        matches!(self, Self::StatusLabel(s) if s.spring)
    }

    /// Measures this item into a [`StripItemInput`] for [`layout_items`]. The
    /// [`Measurer`] is needed only for the text width; the shape of the result
    /// comes from the pure [`measure_item_content`].
    ///
    /// Everything here is in DIP. The device context is already set to the
    /// window's DPI, so a DIP constant reaches the screen at the right physical
    /// size on its own — multiplying by `Canvas::scale` would apply the factor a
    /// second time and, because it inflates the measured widths, would change
    /// *behaviour*: oversized items stop fitting and fall into the overflow menu.
    fn measure(&self, m: &Measurer, image_side: f32, row_height: f32) -> StripItemInput {
        let it = self.item();

        let text_size = |txt: &str| -> Size {
            if txt.is_empty() {
                Size::EMPTY
            } else {
                Size::new(m.width(txt, it.font) + 2.0 * TEXT_PADDING_H, row_height)
            }
        };
        let image_size = if it.image.is_some() { Size::new(image_side, image_side) } else { Size::EMPTY };

        let content = match self {
            Self::Separator(_) => Size::new(SEPARATOR_THICKNESS, row_height),
            // The hosts size to their own default width; the combo's text does
            // not widen it (WinForms sizes a hosted control, not its content).
            Self::ComboBox(_) | Self::TextBox(_) | Self::ProgressBar(_) => {
                Size::new(HOST_DEFAULT_WIDTH, row_height)
            }
            Self::MenuItem(m) => {
                // A top-level menu item measures like a text item; a real
                // drop-down would add the check gutter and shortcut column.
                let t = text_size(&m.base.item.text);
                measure_item_content(it.display_style, it.text_image_relation, t, image_size, IMAGE_TEXT_GAP)
            }
            _ => {
                let t = text_size(&it.text);
                measure_item_content(it.display_style, it.text_image_relation, t, image_size, IMAGE_TEXT_GAP)
            }
        };

        // Fold the item's own margin into the outer size the engine packs.
        let outer = Size::new(
            content.width + it.margin.horizontal(),
            (content.height + it.margin.vertical()).max(row_height),
        );

        StripItemInput {
            size: outer,
            alignment: it.alignment,
            overflow: it.overflow,
            spring: self.spring(),
            visible: it.visible,
        }
    }

    /// Paints this item inside its laid-out `rect`.
    ///
    /// The surface is a [`ControlCanvas`], not a bare [`Canvas`]: an item paints
    /// in the SYSTEM's colours and UI font like every control in this library,
    /// and `visuals()` is where they live.
    fn paint(&self, c: &dyn ControlCanvas, rect: Rect, p: ItemPaint) {
        match self {
            Self::Button(b) => paint_button(c, rect, b, p),
            Self::Label(l) => paint_label(c, rect, l, p),
            Self::StatusLabel(s) => paint_status_label(c, rect, s, p),
            Self::Separator(_) => paint_separator(c, rect, p),
            Self::MenuItem(m) => paint_menu_item(c, rect, m, p),
            Self::ComboBox(cb) => paint_combo(c, rect, cb),
            Self::TextBox(t) => paint_textbox(c, rect, t),
            Self::ProgressBar(pb) => paint_progress(c, rect, pb),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// The control hierarchy — `ToolStrip` and its descendants
// ─────────────────────────────────────────────────────────────────────────────

/// The surface `System.Windows.Forms.ToolStrip` declares on top of a control,
/// plus its `Items`. `MenuStrip`, `StatusStrip` and the drop-downs compose this.
///
/// # Three declared properties that deliberately have no field here
///
/// * **`AutoScroll`** — the toolkit re-declares it only to hide it from the
///   designer: a strip *overflows*, it does not scroll. The state itself belongs
///   to the composed `ScrollableControl`/`ControlBase` base and is reached
///   through `Deref`, so restating it here would create a second copy that could
///   drift from the one the layout actually reads.
/// * **`Controls`** — the `Control` collection, which is NOT the same thing as
///   [`ToolStrip::items`]. A strip's children are `ToolStripItem`s (Components,
///   not Controls); a real control only ever enters a strip wrapped in a
///   [`ToolStripControlHost`], which is what puts it in `Controls`. Modelling a
///   second collection would invent a relationship the toolkit does not have, so
///   the hosted-control path is the one this port exposes.
/// * **`ImageList`** — not modelled, because the port has no `ImageList` type at
///   all: images are carried per item as a key the host resolves
///   ([`ToolStripItem::image`]), and `ImageIndex`/`ImageKey` lookups have nothing
///   to resolve against. `TabControl` has the same gap for the same reason, and
///   both close together the day an `ImageList` equivalent lands.
#[derive(Clone)]
pub struct ToolStrip {
    control: ControlBase,

    /// `AllowClickThrough` — whether a click that activates the owning form is
    /// also delivered to the item under the pointer, instead of being swallowed
    /// by the activation. **Not yet honoured**: the port has no focus or
    /// activation model, so nothing consumes it; it is kept as declared state so
    /// a host that grows one does not have to re-add it.
    pub allow_click_through: bool,
    pub allow_item_reorder:  bool,
    pub allow_merge:         bool,
    pub can_overflow:        bool,
    pub grip_style:          ToolStripGripStyle,
    pub grip_margin:         Padding,
    pub layout_style:        ToolStripLayoutStyle,
    pub render_mode:         ToolStripRenderMode,
    pub show_item_tool_tips: bool,
    pub stretch:             bool,
    pub image_scaling_size:  Size,
    pub text_direction:      ToolStripTextDirection,
    pub default_drop_down_direction: ToolStripDropDownDirection,

    pub items: Vec<StripItem>,
}

impl Default for ToolStrip {
    /// The toolkit's `ToolStrip` defaults: `GripStyle = Visible`, `Dock = Top`,
    /// `LayoutStyle = StackWithOverflow`, `CanOverflow = true`, `Stretch = false`,
    /// `AutoSize = true`, `TabStop = false`, `ImageScalingSize = 16×16`.
    fn default() -> Self {
        let mut control = ControlBase::new();
        control.dock = DockStyle::Top;
        control.auto_size = true;
        control.tab_stop = false;
        Self {
            control,
            allow_click_through: false,
            allow_item_reorder: false,
            allow_merge: true,
            can_overflow: true,
            grip_style: ToolStripGripStyle::Visible,
            grip_margin: Padding::all(2.0),
            layout_style: ToolStripLayoutStyle::StackWithOverflow,
            render_mode: ToolStripRenderMode::ManagerRenderMode,
            show_item_tool_tips: true,
            stretch: false,
            image_scaling_size: Size::new(16.0, 16.0),
            text_direction: ToolStripTextDirection::Horizontal,
            default_drop_down_direction: ToolStripDropDownDirection::Default,
            items: Vec::new(),
        }
    }
}

impl ToolStrip {
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether this strip is oriented horizontally, from its dock — what
    /// `StackWithOverflow` needs to pick an axis.
    fn horizontal(&self) -> bool {
        !matches!(self.control.dock, DockStyle::Left | DockStyle::Right)
    }

    /// The main-axis space the move handle takes, in DIP — zero when hidden.
    fn grip_reserve(&self) -> f32 {
        if self.grip_style == ToolStripGripStyle::Visible {
            GRIP_WIDTH + self.grip_margin.horizontal()
        } else {
            0.0
        }
    }

    fn row_height(&self) -> f32 {
        // The taller of the image slot and a text line, which is all the canvas
        // exposes (there is no font-height accessor), plus the item's vertical
        // margins. `ImageScalingSize` is already a DIP size, so nothing is scaled.
        self.image_scaling_size.height.max(GLYPH_HEIGHT) + 4.0
    }

    fn measured_inputs(&self, m: &Measurer) -> Vec<StripItemInput> {
        let image_side = self.image_scaling_size.width;
        let row = self.row_height();
        self.items.iter().map(|it| it.measure(m, image_side, row)).collect()
    }

    /// Lays the strip's items into `bounds`, returning the geometry a paint pass
    /// walks. Shared by every strip type.
    fn layout_in(&self, m: &Measurer, bounds: Rect) -> StripLayout {
        let (axis, spring) = self.layout_style.resolve(self.horizontal());
        let inputs = self.measured_inputs(m);
        layout_items(
            axis,
            bounds,
            self.grip_reserve(),
            OVERFLOW_WIDTH,
            self.can_overflow,
            spring,
            &inputs,
        )
    }

    /// `GetPreferredSize` from a surface that offers only a bare [`Canvas`] —
    /// what [`Control::preferred_size`] receives. It reads the system UI font
    /// for itself (see [`system_ui_font`]) so an item is sized against the very
    /// face it will be painted in.
    fn preferred_on(&self, c: &dyn Canvas) -> Size {
        let fonts = system_ui_font(c);
        self.preferred(&Measurer { canvas: c, fonts: fonts.as_ref() })
    }

    /// The content size the strip would like along the main axis, plus the row
    /// height across it — `GetPreferredSize`, honouring `Padding`.
    fn preferred(&self, m: &Measurer) -> Size {
        let inputs = self.measured_inputs(m);
        let pad = self.control.padding;
        if self.horizontal() {
            let sum: f32 = inputs.iter().filter(|i| i.visible).map(|i| i.size.width).sum();
            Size::new(
                self.grip_reserve() + sum + pad.horizontal(),
                self.row_height() + pad.vertical(),
            )
        } else {
            let h: f32 = inputs.iter().filter(|i| i.visible).map(|i| i.size.height).sum();
            Size::new(self.row_height() + pad.horizontal(), self.grip_reserve() + h + pad.vertical())
        }
    }

    /// Paints the strip with one item drawn **selected** — `HighlightText` over
    /// `Highlight`, which is what the toolkit does to the item under the
    /// pointer.
    ///
    /// A `ToolStripItem` is a `Component`, not a `Control`: it cannot observe
    /// the mouse, and [`crate::ControlState`] says only that the *strip* is hot,
    /// never which of its items is. So the host — which does know, because it
    /// hit-tested the layout — names the item here. [`Control::paint`] paints
    /// with no hot item, which is what a resting strip looks like.
    pub fn paint_items(
        &self,
        c: &dyn ControlCanvas,
        bounds: Rect,
        kind: StripKind,
        hot: Option<usize>,
    ) {
        self.paint_strip(c, bounds, kind, false, hot);
    }

    /// Paints background, grip and every on-strip item; `sizing_grip` draws the
    /// `StatusStrip` corner dots.
    fn paint_strip(
        &self,
        c: &dyn ControlCanvas,
        bounds: Rect,
        kind: StripKind,
        sizing_grip: bool,
        hot: Option<usize>,
    ) {
        let v = c.visuals();
        let (face, ink) = kind.palette(&v.colors);
        // `BackColor` is ambient on a control: unset means « the system face for
        // this kind of strip », which is the whole point of the two palettes.
        let face = self.control.back_color.unwrap_or(face);
        c.fill_rect(&bounds, &face);
        if matches!(kind, StripKind::DropDown { .. }) {
            // A drop-down is a popup window, and the toolkit frames it: the two
            // raised rings every classic menu has around its items.
            c.draw_edge(&bounds, EdgeStyle::Raised, Border3DSide::ALL);
        }

        if self.grip_style == ToolStripGripStyle::Visible {
            paint_grip(c, bounds, self.grip_margin);
        }

        let m = Measurer { canvas: c, fonts: Some(&v.fonts) };
        let layout = self.layout_in(&m, bounds);
        let (axis, _) = self.layout_style.resolve(self.horizontal());
        let item_paint = ItemPaint { ink, axis, gutter: kind.gutter(&v.metrics), hot: false };
        for (i, it) in self.items.iter().enumerate() {
            if !layout.on_overflow[i] {
                let r = layout.rects[i];
                if r.right > r.left {
                    let p = ItemPaint { hot: hot == Some(i), ..item_paint };
                    it.paint(c, inset_margin(r, it.item().margin), p);
                }
            }
        }
        if let Some(btn) = layout.overflow_button {
            paint_overflow_button(c, btn, &ink);
        }
        if sizing_grip {
            paint_sizing_grip(c, bounds);
        }
    }
}

/// `MenuStrip` — a horizontal menu bar. Over `ToolStrip` it hides the grip,
/// turns off overflow, stretches, and turns off item tooltips.
#[derive(Clone)]
pub struct MenuStrip {
    pub base: ToolStrip,
    /// `MdiWindowListItem` — the item MDI window entries append to. Modelled as an
    /// index into `Items`; MDI child tracking itself is not in scope.
    pub mdi_window_list_item: Option<usize>,
}

impl Default for MenuStrip {
    /// A menu bar is a `ToolStrip` with five different defaults: no grip, no
    /// overflow, no item tooltips, stretched across its container, and docked to
    /// the top.
    fn default() -> Self {
        let base = ToolStrip {
            grip_style: ToolStripGripStyle::Hidden,
            can_overflow: false,
            show_item_tool_tips: false,
            stretch: true,
            control: ControlBase { dock: DockStyle::Top, ..ControlBase::default() },
            ..ToolStrip::default()
        };
        Self { base, mdi_window_list_item: None }
    }
}

impl MenuStrip {
    pub fn new() -> Self {
        Self::default()
    }
}

impl std::ops::Deref for MenuStrip {
    type Target = ToolStrip;
    fn deref(&self) -> &ToolStrip {
        &self.base
    }
}
impl std::ops::DerefMut for MenuStrip {
    fn deref_mut(&mut self) -> &mut ToolStrip {
        &mut self.base
    }
}

/// `StatusStrip` — the bottom status bar. Over `ToolStrip` it docks to the
/// bottom, hides the grip, uses `Table` layout (springing), disables overflow,
/// and shows a sizing grip.
#[derive(Clone)]
pub struct StatusStrip {
    pub base:        ToolStrip,
    pub sizing_grip: bool,
}

impl Default for StatusStrip {
    fn default() -> Self {
        let mut base = ToolStrip::default();
        base.control.dock = DockStyle::Bottom;
        base.grip_style = ToolStripGripStyle::Hidden;
        base.layout_style = ToolStripLayoutStyle::Table;
        base.can_overflow = false;
        base.show_item_tool_tips = false;
        base.stretch = true;
        Self { base, sizing_grip: true }
    }
}

impl StatusStrip {
    pub fn new() -> Self {
        Self::default()
    }
}

impl std::ops::Deref for StatusStrip {
    type Target = ToolStrip;
    fn deref(&self) -> &ToolStrip {
        &self.base
    }
}
impl std::ops::DerefMut for StatusStrip {
    fn deref_mut(&mut self) -> &mut ToolStrip {
        &mut self.base
    }
}

/// `ToolStripDropDown` — a free-floating strip (not docked, initially hidden).
/// Over `ToolStrip` it declares auto-close, opacity and the drop shadow.
#[derive(Clone)]
pub struct ToolStripDropDown {
    pub base:               ToolStrip,
    pub auto_close:         bool,
    pub drop_shadow_enabled: bool,
    pub opacity:            f64,
}

impl Default for ToolStripDropDown {
    fn default() -> Self {
        let mut base = ToolStrip::default();
        base.control.dock = DockStyle::None;
        base.control.visible = false; // `Visible` defaults to false
        base.grip_style = ToolStripGripStyle::Hidden;
        Self { base, auto_close: true, drop_shadow_enabled: true, opacity: 1.0 }
    }
}

impl std::ops::Deref for ToolStripDropDown {
    type Target = ToolStrip;
    fn deref(&self) -> &ToolStrip {
        &self.base
    }
}
impl std::ops::DerefMut for ToolStripDropDown {
    fn deref_mut(&mut self) -> &mut ToolStrip {
        &mut self.base
    }
}

/// `ToolStripDropDownMenu` — a drop-down laid out as a vertical `Flow` of menu
/// items, with margins for a check column and an image column.
#[derive(Clone)]
pub struct ToolStripDropDownMenu {
    pub base:              ToolStripDropDown,
    pub show_check_margin: bool,
    pub show_image_margin: bool,
}

impl Default for ToolStripDropDownMenu {
    fn default() -> Self {
        let mut base = ToolStripDropDown::default();
        base.base.layout_style = ToolStripLayoutStyle::Flow;
        Self { base, show_check_margin: false, show_image_margin: true }
    }
}

impl ToolStripDropDownMenu {
    /// The painting surface this menu is: a drop-down whose left gutter is one
    /// `SM_CXMENUCHECK` column per margin it shows. `ShowCheckMargin` and
    /// `ShowImageMargin` are independent in the toolkit, and both can be on.
    fn kind(&self) -> StripKind {
        StripKind::DropDown {
            margins: u32::from(self.show_check_margin) + u32::from(self.show_image_margin),
        }
    }
}

impl std::ops::Deref for ToolStripDropDownMenu {
    type Target = ToolStripDropDown;
    fn deref(&self) -> &ToolStripDropDown {
        &self.base
    }
}
impl std::ops::DerefMut for ToolStripDropDownMenu {
    fn deref_mut(&mut self) -> &mut ToolStripDropDown {
        &mut self.base
    }
}

/// `ContextMenuStrip` — a `ToolStripDropDownMenu` shown on right-click. Declares
/// no property of its own; it exists to be a distinct type the host raises.
#[derive(Clone, Default)]
pub struct ContextMenuStrip {
    pub base: ToolStripDropDownMenu,
}

impl std::ops::Deref for ContextMenuStrip {
    type Target = ToolStripDropDownMenu;
    fn deref(&self) -> &ToolStripDropDownMenu {
        &self.base
    }
}
impl std::ops::DerefMut for ContextMenuStrip {
    fn deref_mut(&mut self) -> &mut ToolStripDropDownMenu {
        &mut self.base
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// `Control` implementations
// ─────────────────────────────────────────────────────────────────────────────

impl Control for ToolStrip {
    fn control(&self) -> &ControlBase {
        &self.control
    }
    fn control_mut(&mut self) -> &mut ControlBase {
        &mut self.control
    }
    fn preferred_size(&self, c: &dyn Canvas) -> Size {
        self.preferred_on(c)
    }
    fn paint(&self, c: &dyn ControlCanvas, bounds: Rect) {
        self.paint_strip(c, bounds, StripKind::Tool, false, None);
    }
    fn type_name(&self) -> &'static str {
        "ToolStrip"
    }
}

impl Control for MenuStrip {
    fn control(&self) -> &ControlBase {
        &self.base.control
    }
    fn control_mut(&mut self) -> &mut ControlBase {
        &mut self.base.control
    }
    fn preferred_size(&self, c: &dyn Canvas) -> Size {
        self.base.preferred_on(c)
    }
    fn paint(&self, c: &dyn ControlCanvas, bounds: Rect) {
        self.base.paint_strip(c, bounds, StripKind::Menu, false, None);
    }
    fn type_name(&self) -> &'static str {
        "MenuStrip"
    }
}

impl Control for StatusStrip {
    fn control(&self) -> &ControlBase {
        &self.base.control
    }
    fn control_mut(&mut self) -> &mut ControlBase {
        &mut self.base.control
    }
    fn preferred_size(&self, c: &dyn Canvas) -> Size {
        self.base.preferred_on(c)
    }
    fn paint(&self, c: &dyn ControlCanvas, bounds: Rect) {
        self.base.paint_strip(c, bounds, StripKind::Status, self.sizing_grip, None);
    }
    fn type_name(&self) -> &'static str {
        "StatusStrip"
    }
}

impl Control for ToolStripDropDown {
    fn control(&self) -> &ControlBase {
        &self.base.control
    }
    fn control_mut(&mut self) -> &mut ControlBase {
        &mut self.base.control
    }
    fn preferred_size(&self, c: &dyn Canvas) -> Size {
        self.base.preferred_on(c)
    }
    fn paint(&self, c: &dyn ControlCanvas, bounds: Rect) {
        // A bare drop-down hosts arbitrary items, not menu entries: it gets the
        // menu palette and the frame, but no check/image margin.
        self.base.paint_strip(c, bounds, StripKind::DropDown { margins: 0 }, false, None);
    }
    fn type_name(&self) -> &'static str {
        "ToolStripDropDown"
    }
}

impl Control for ToolStripDropDownMenu {
    fn control(&self) -> &ControlBase {
        &self.base.base.control
    }
    fn control_mut(&mut self) -> &mut ControlBase {
        &mut self.base.base.control
    }
    fn preferred_size(&self, c: &dyn Canvas) -> Size {
        self.base.base.preferred_on(c)
    }
    fn paint(&self, c: &dyn ControlCanvas, bounds: Rect) {
        self.base.base.paint_strip(c, bounds, self.kind(), false, None);
    }
    fn type_name(&self) -> &'static str {
        "ToolStripDropDownMenu"
    }
}

impl Control for ContextMenuStrip {
    fn control(&self) -> &ControlBase {
        &self.base.base.base.control
    }
    fn control_mut(&mut self) -> &mut ControlBase {
        &mut self.base.base.base.control
    }
    fn preferred_size(&self, c: &dyn Canvas) -> Size {
        self.base.base.base.preferred_on(c)
    }
    fn paint(&self, c: &dyn ControlCanvas, bounds: Rect) {
        self.base.base.base.paint_strip(c, bounds, self.base.kind(), false, None);
    }
    fn type_name(&self) -> &'static str {
        "ContextMenuStrip"
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Painting helpers
//
// Every metric below is a plain DIP constant or a `SystemMetrics` field, used
// as-is. The renderer calls `SetDpi` on the device context, so the Direct2D
// coordinate space IS the DIP space: a rect at y = 10.0 lands at 10 DIP, which
// is 17.5 physical pixels at 175 %, with no arithmetic here — and `SystemMetrics`
// is already converted to DIP on the way in. Multiplying by `Canvas::scale` would
// apply the factor twice. `scale` is only ever right for picking a
// *physical-pixel* thickness, where one divides by it, which is what a hairline
// two-tone edge is made of.
//
// There is no corner radius anywhere in this file, and there is not meant to be:
// a WinForms strip is square, and the one place a bevel appears it is Win32's
// `DrawEdge`, reached through `ControlCanvas::draw_edge`.
// ─────────────────────────────────────────────────────────────────────────────

const GRIP_WIDTH: f32 = 7.0;
const OVERFLOW_WIDTH: f32 = 16.0;
const GLYPH_HEIGHT: f32 = 15.0;
/// The side of a chevron glyph drawn through `Canvas::vector_icon`.
const CHEVRON_SIZE: f32 = 12.0;
/// The side of a grip dot, in **physical pixels** — the toolkit's move handle
/// and sizing grip are both built from this one 2×2 square, drawn twice in two
/// tones. See [`paint_grip_dot`] for why this one decoration is measured in
/// pixels rather than DIP.
const GRIP_DOT_PX: f32 = 2.0;
/// The gap from one grip dot to the next, in **physical pixels**.
const GRIP_PITCH_PX: f32 = 4.0;

/// Which of the toolkit's surfaces a strip is, and therefore which system
/// palette it paints with.
///
/// Not a WinForms type: in the toolkit this is the **renderer**'s job
/// (`OnRenderToolStripBackground` and `OnRenderMenuStripBackground` are
/// different methods, reading different colours). The port has no renderer
/// object, so a strip names its surface and the one shared paint pass reads the
/// palette from the system.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StripKind {
    /// A tool bar: the `Control` face under `ControlText` ink.
    Tool,
    /// A menu bar: the `MenuBar` face under `MenuText` ink — the toolkit reads
    /// `COLOR_MENUBAR` here, which is a different colour from `COLOR_BTNFACE` on
    /// a machine that themes its menus.
    Menu,
    /// A status bar: the `Control` face, plus the sizing grip when the strip
    /// asks for one.
    Status,
    /// A floating drop-down: the menu palette, the raised frame a popup has, and
    /// a left gutter of `margins` check-mark columns — the check margin and the
    /// image margin, each `SM_CXMENUCHECK` wide.
    DropDown { margins: u32 },
}

impl StripKind {
    /// `(face, ink)` — the strip's background and its resting text colour.
    fn palette(self, colors: &SystemColors) -> (D2D1_COLOR_F, D2D1_COLOR_F) {
        match self {
            // `COLOR_MENU` — the drop-down's own background — is not among the
            // colours `SystemColors` reads, so a popup takes the menu BAR colour.
            // The two are the same grey on a default Windows 11; on a machine
            // where they differ, this is the seam.
            Self::Menu | Self::DropDown { .. } => (colors.menu_bar, colors.menu_text),
            Self::Tool | Self::Status => (colors.control, colors.control_text),
        }
    }

    /// The left gutter a drop-down reserves before its items' text.
    fn gutter(self, metrics: &SystemMetrics) -> f32 {
        match self {
            Self::DropDown { margins } => margins as f32 * metrics.menu_check_width,
            _ => 0.0,
        }
    }
}

/// What one item is painted with: the strip's own ink, the axis it was laid
/// along, the drop-down gutter, and whether the pointer is on it.
///
/// The strip's *face* is deliberately absent: an item paints no background of
/// its own unless it is selected or carries a `BackColor`, so the band the strip
/// already filled shows through — which is what makes a resting tool bar one
/// flat surface rather than a row of cells.
#[derive(Debug, Clone, Copy)]
struct ItemPaint {
    /// The resting ink for this strip's palette (`ControlText` or `MenuText`).
    ink:    D2D1_COLOR_F,
    axis:   StripAxis,
    gutter: f32,
    /// `ToolStripItem.Selected` — the item under the pointer, which the toolkit
    /// paints in `HighlightText` over `Highlight`.
    hot:    bool,
}

/// What an item measures its text with: the surface that can measure, and the
/// system UI font to measure in.
///
/// The font is an `Option` because the two entry points differ. `paint` gets a
/// [`ControlCanvas`] and takes the font straight from `visuals()`, which is the
/// font it will then draw with. `Control::preferred_size` gets a bare [`Canvas`]
/// and has to read the system font for itself ([`system_ui_font`]); if that read
/// fails there is no honest width to report, so an item measures as if it had no
/// text rather than measuring in some other face.
struct Measurer<'a> {
    canvas: &'a dyn Canvas,
    fonts:  Option<&'a SystemFonts>,
}

impl Measurer<'_> {
    /// The width DirectWrite reports for `text` in the item's font.
    fn width(&self, text: &str, role: Option<FontRole>) -> f32 {
        match self.fonts {
            Some(f) => self.canvas.measure(text, font_for(f, role)),
            None => 0.0,
        }
    }
}

/// The system UI font, read for a surface that offers only a bare [`Canvas`].
///
/// [`Control::preferred_size`] takes a `&dyn Canvas`, which cannot answer for
/// the system's visuals — but measuring a strip in any other face would size
/// every item against a font it is not painted in, and the items would then be
/// laid out to the wrong widths (the very failure the DIP regression test
/// pins, in a different disguise). So the font is read from the system here, at
/// the surface's own DPI, exactly as [`crate::system::Visuals`] reads it.
/// Nothing is cached and no state is kept; widening `preferred_size` to a
/// [`ControlCanvas`] would remove the need for this function entirely.
fn system_ui_font(c: &dyn Canvas) -> Option<SystemFonts> {
    // The SHARED factory is DirectWrite's own process-wide singleton, so this is
    // a lookup rather than a construction.
    let dwrite: IDWriteFactory = unsafe { DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED) }.ok()?;
    SystemFonts::read(&dwrite, c.scale() * 96.0).ok()
}

/// The DirectWrite format a `FontRole` resolves to **in the system's UI font**.
///
/// WinForms carries a real `Font` per item; the port carries a role, and the
/// system publishes one family in three faces. The strong roles take the bold
/// face and everything else the message font — so a control still never names a
/// family or a size, and the family it does not name is the one the toolkit
/// measured its own reference sheets with.
fn font_for(fonts: &SystemFonts, role: Option<FontRole>) -> &IDWriteTextFormat {
    match role {
        Some(
            FontRole::CaptionStrong
            | FontRole::BodyStrong
            | FontRole::Heading
            | FontRole::Title,
        ) => &fonts.message_bold,
        _ => &fonts.message,
    }
}

/// Deflates a laid-out rect by the item's margin, giving the content box the item
/// actually draws in. The margin is already in DIP, like every `Padding`.
fn inset_margin(r: Rect, m: Padding) -> Rect {
    Rect::new(
        r.left + m.left,
        r.top + m.top,
        (r.right - m.right).max(r.left + m.left),
        (r.bottom - m.bottom).max(r.top + m.top),
    )
}

/// `r` narrowed by `pad` on both sides, never inverted — the breathing room a
/// left- or right-aligned label has inside its cell, and the same
/// [`TEXT_PADDING_H`] the measurement reserved for it.
fn inset_h(r: Rect, pad: f32) -> Rect {
    let left = r.left + pad;
    Rect::new(left, r.top, (r.right - pad).max(left), r.bottom)
}

/// The family's `Border3DStyle` (the *property*) as the system's (the *paint
/// primitive*). Two enums with the same members, deliberately: see the property
/// enum's own documentation.
fn edge_style(style: Border3DStyle) -> EdgeStyle {
    match style {
        Border3DStyle::Adjust => EdgeStyle::Adjust,
        Border3DStyle::Bump => EdgeStyle::Bump,
        Border3DStyle::Etched => EdgeStyle::Etched,
        Border3DStyle::Flat => EdgeStyle::Flat,
        Border3DStyle::Raised => EdgeStyle::Raised,
        Border3DStyle::RaisedInner => EdgeStyle::RaisedInner,
        Border3DStyle::RaisedOuter => EdgeStyle::RaisedOuter,
        Border3DStyle::Sunken => EdgeStyle::Sunken,
        Border3DStyle::SunkenInner => EdgeStyle::SunkenInner,
        Border3DStyle::SunkenOuter => EdgeStyle::SunkenOuter,
    }
}

/// `ToolStripStatusLabelBorderSides` as `Border3DSide`. The two bit sets are the
/// same four bits in the same order (`Left = 1`, `Top = 2`, `Right = 4`,
/// `Bottom = 8`), but they are converted member by member rather than by casting
/// the payload — a cast would keep compiling if either enum ever renumbered.
fn edge_sides(sides: ToolStripStatusLabelBorderSides) -> Border3DSide {
    let mut out = 0u8;
    for (from, to) in [
        (ToolStripStatusLabelBorderSides::LEFT, Border3DSide::LEFT),
        (ToolStripStatusLabelBorderSides::TOP, Border3DSide::TOP),
        (ToolStripStatusLabelBorderSides::RIGHT, Border3DSide::RIGHT),
        (ToolStripStatusLabelBorderSides::BOTTOM, Border3DSide::BOTTOM),
    ] {
        if sides.contains(from) {
            out |= to.0;
        }
    }
    Border3DSide(out)
}

/// One handle dot: a 2×2 highlight square with a 2×2 shadow square one pixel up
/// and to the left of it.
///
/// That pair is the whole trick of the toolkit's grip — the dot reads as
/// engraved because it has a shaded side and a lit one, `ControlDark` above
/// `ControlLightLight`. A single tinted square (which is what a themed palette
/// invites) reads as a grey speck instead, at any size. The shadow goes down
/// **second** so it is the half that survives the overlap, which is the way
/// round the reference sheet shows.
///
/// This is the one decoration in the file measured in **physical pixels**: the
/// toolkit draws these dots into a pixel `Graphics` and they come out 2 px
/// whatever the scale — a grip that grew with the DPI would be a fat dotted
/// ribbon at 175 %, which is not what the reference shows. Dividing by
/// `Canvas::scale` is the legitimate direction, the same one `draw_edge` uses
/// for a hairline.
fn paint_grip_dot(c: &dyn ControlCanvas, x: f32, y: f32) {
    let colors = &c.visuals().colors;
    let t = 1.0 / c.scale();
    let side = GRIP_DOT_PX * t;
    c.fill_rect(
        &Rect::new(x + t, y + t, x + t + side, y + t + side),
        &colors.control_light_light,
    );
    c.fill_rect(&Rect::new(x, y, x + side, y + side), &colors.control_dark);
}

/// The move handle: a dotted column down the strip's leading edge.
fn paint_grip(c: &dyn ControlCanvas, bounds: Rect, margin: Padding) {
    let pitch = GRIP_PITCH_PX / c.scale();
    let x = bounds.left + margin.left + 2.0;
    let mut y = bounds.top + 4.0;
    while y < bounds.bottom - 4.0 {
        paint_grip_dot(c, x, y);
        y += pitch;
    }
}

/// `ToolStripSeparator` — one **etched** line across the strip's cross axis.
///
/// The toolkit draws it with `ControlPaint.DrawBorder3D(…, Border3DStyle.Etched,
/// side)`, whose lit half is `ControlDark` with `ControlLightLight` one pixel
/// after it: the two-tone pair, never a single tinted hairline. Asking
/// `draw_edge` for the one side is exactly that, and it is one physical pixel
/// per tone at any DPI.
fn paint_separator(c: &dyn ControlCanvas, rect: Rect, p: ItemPaint) {
    let t = 1.0 / c.scale();
    match p.axis {
        StripAxis::Horizontal => {
            let cx = (rect.left + rect.right) * 0.5;
            let line = Rect::new(cx, rect.top + 3.0, cx + 2.0 * t, rect.bottom - 3.0);
            c.draw_edge(&line, EdgeStyle::Etched, Border3DSide::LEFT);
        }
        StripAxis::Vertical => {
            let cy = (rect.top + rect.bottom) * 0.5;
            let line = Rect::new(rect.left + 3.0, cy, rect.right - 3.0, cy + 2.0 * t);
            c.draw_edge(&line, EdgeStyle::Etched, Border3DSide::TOP);
        }
    }
}

/// The face under an item: the selection when the pointer is on it, else its own
/// `BackColor` when it has one, else nothing at all — the strip's face shows
/// through, which is what makes a resting tool bar one flat band.
fn paint_item_face(c: &dyn ControlCanvas, rect: &Rect, item: &ToolStripItem, p: ItemPaint) {
    if p.hot && item.enabled {
        c.fill_rect(rect, &c.visuals().colors.highlight);
    } else if let Some(back) = item.back_color {
        c.fill_rect(rect, &back);
    }
}

/// The ink an item's text takes: `GrayText` when disabled, `HighlightText` over
/// a selection, its own `ForeColor` when set, else the strip's palette ink.
fn item_ink(c: &dyn ControlCanvas, item: &ToolStripItem, p: ItemPaint) -> D2D1_COLOR_F {
    let colors = &c.visuals().colors;
    if !item.enabled {
        return colors.gray_text;
    }
    if p.hot {
        return colors.highlight_text;
    }
    item.fore_color.unwrap_or(p.ink)
}

/// Draws `text` in `rect` under the item's `TextAlign`, in the system UI font.
///
/// Only the horizontal half of `TextAlign` is honoured: every primitive here
/// centres vertically, which is what an item does in a strip row — the toolkit's
/// own `ToolStripItem` ignores the vertical half for the same reason unless the
/// item is taller than its content.
fn draw_text(c: &dyn ControlCanvas, rect: Rect, text: &str, item: &ToolStripItem, ink: D2D1_COLOR_F) {
    if text.is_empty() {
        return;
    }
    let (h, _) = item.text_align.fractions();
    let (align, r) = if h == 0.0 {
        (DWRITE_TEXT_ALIGNMENT_LEADING, inset_h(rect, TEXT_PADDING_H))
    } else if h == 1.0 {
        (DWRITE_TEXT_ALIGNMENT_TRAILING, inset_h(rect, TEXT_PADDING_H))
    } else {
        (DWRITE_TEXT_ALIGNMENT_CENTER, rect)
    };
    draw_aligned(c, r, text, item.font, ink, align);
}

/// Text in the system UI font at an explicit alignment — the one call every
/// helper above funnels into, so the font resolution happens in one place.
fn draw_aligned(
    c: &dyn ControlCanvas,
    rect: Rect,
    text: &str,
    role: Option<FontRole>,
    ink: D2D1_COLOR_F,
    align: DWRITE_TEXT_ALIGNMENT,
) {
    c.text_aligned(text, &rect, font_for(&c.visuals().fonts, role), &ink, align);
}

/// A pressed cell — what the toolkit paints under a **checked** button and a
/// checked menu item: a single sunken ring over a face one shade darker than the
/// strip's.
///
/// The classic renderer dithers `ControlLightLight` over `Control` there; the
/// canvas has no pattern brush, so the flat `ControlLight` between the two is
/// the honest single-colour stand-in, and the sunken bevel is the part that
/// actually says « toggled ».
fn paint_pressed_cell(c: &dyn ControlCanvas, rect: &Rect) {
    c.fill_rect(rect, &c.visuals().colors.control_light);
    c.draw_edge(rect, EdgeStyle::SunkenOuter, Border3DSide::ALL);
}

fn paint_button(c: &dyn ControlCanvas, rect: Rect, b: &ToolStripButton, p: ItemPaint) {
    let checked = b.checked || b.check_state == crate::enums::CheckState::Checked;
    if checked && !p.hot {
        paint_pressed_cell(c, &rect);
    } else {
        paint_item_face(c, &rect, &b.item, p);
        if checked {
            // Hot AND checked: the selection is the face, the bevel still says
            // the button is down.
            c.draw_edge(&rect, EdgeStyle::SunkenOuter, Border3DSide::ALL);
        }
    }
    draw_text(c, rect, &b.item.text, &b.item, item_ink(c, &b.item, p));
}

fn paint_label(c: &dyn ControlCanvas, rect: Rect, l: &ToolStripLabel, p: ItemPaint) {
    paint_item_face(c, &rect, &l.item, p);
    // A link takes the system's hyperlink colour (`COLOR_HOTLIGHT`), which is
    // what `LinkLabel` reads. `LinkBehavior` is still not honoured: the canvas
    // has no underline, so `AlwaysUnderline` and `NeverUnderline` look alike.
    let ink = if l.is_link && l.item.enabled && !p.hot {
        l.item.fore_color.unwrap_or(c.visuals().colors.hot_track)
    } else {
        item_ink(c, &l.item, p)
    };
    draw_text(c, rect, &l.item.text, &l.item, ink);
}

fn paint_status_label(c: &dyn ControlCanvas, rect: Rect, s: &ToolStripStatusLabel, p: ItemPaint) {
    paint_item_face(c, &rect, &s.item, p);
    draw_text(c, rect, &s.item.text, &s.item, item_ink(c, &s.item, p));
    if s.border_sides != ToolStripStatusLabelBorderSides::NONE {
        c.draw_edge(&rect, edge_style(s.border_style), edge_sides(s.border_sides));
    }
}

/// A menu entry. On a menu **bar** it is centred text; in a drop-down it is the
/// toolkit's three columns — the check/image gutter, the text, then the shortcut
/// and the submenu arrow.
fn paint_menu_item(c: &dyn ControlCanvas, rect: Rect, m: &ToolStripMenuItem, p: ItemPaint) {
    let item = &m.base.item;
    let checked = m.checked || m.check_state == crate::enums::CheckState::Checked;
    if checked && !p.hot {
        paint_pressed_cell(c, &rect);
    } else {
        paint_item_face(c, &rect, item, p);
    }
    let ink = item_ink(c, item, p);

    if p.gutter <= 0.0 {
        draw_text(c, rect, &item.text, item, ink);
        return;
    }

    // The check mark sits in the FIRST margin column, whether or not an image
    // margin follows it: `SM_CXMENUCHECK` × `SM_CYMENUCHECK` is the cell Windows
    // sizes a menu check for, and the glyph is geometry, never a « ✓ » character.
    let metrics = &c.visuals().metrics;
    if checked {
        let cell = Rect::new(
            rect.left,
            rect.top,
            rect.left + metrics.menu_check_width,
            rect.bottom,
        );
        c.vector_icon(
            "Check",
            &cell,
            metrics.menu_check_width.min(metrics.menu_check_height),
            &ink,
        );
    }

    // A submenu arrow takes a trailing column of its own; the shortcut is
    // right-aligned in what is left.
    let arrow = if m.has_drop_down_items() { CHEVRON_SIZE } else { 0.0 };
    let body_left = (rect.left + p.gutter).min(rect.right);
    let body = Rect::new(body_left, rect.top, (rect.right - arrow).max(body_left), rect.bottom);
    draw_aligned(c, body, &item.text, item.font, ink, DWRITE_TEXT_ALIGNMENT_LEADING);

    let shortcut = m.shortcut_text();
    if !shortcut.is_empty() {
        draw_aligned(
            c,
            inset_h(body, TEXT_PADDING_H),
            &shortcut,
            item.font,
            ink,
            DWRITE_TEXT_ALIGNMENT_TRAILING,
        );
    }
    if arrow > 0.0 {
        let cell = Rect::new(body.right, rect.top, rect.right, rect.bottom);
        c.vector_icon("ChevronRight", &cell, CHEVRON_SIZE, &ink);
    }
}

/// A hosted `ComboBox`: the sunken white well the toolkit gives a field, its
/// text in `WindowText`, and the drop-down button at the trailing edge.
fn paint_combo(c: &dyn ControlCanvas, rect: Rect, cb: &ToolStripComboBox) {
    let colors = &c.visuals().colors;
    let item = &cb.host.item;
    let inner = c.draw_edge(&rect, EdgeStyle::Sunken, Border3DSide::ALL);
    c.fill_rect(&inner, &colors.window);

    let gutter_left = (inner.right - OVERFLOW_WIDTH).max(inner.left);
    let gutter = Rect::new(gutter_left, inner.top, inner.right, inner.bottom);
    let text_rect = Rect::new(inner.left + TEXT_PADDING_H, inner.top, gutter_left, inner.bottom);
    let ink = if item.enabled { colors.window_text } else { colors.gray_text };
    draw_aligned(c, text_rect, cb.display_text(), item.font, ink, DWRITE_TEXT_ALIGNMENT_LEADING);
    // The drop-down chevron is a VECTOR icon, never a text character: the shared
    // face has no arrow glyph, so a « ▼ » would render as a tofu box.
    c.vector_icon("ChevronDown", &gutter, CHEVRON_SIZE, &colors.control_text);
}

/// A hosted `TextBox`, framed by whatever `BorderStyle` it carries — `Fixed3D`
/// is the sunken well, `FixedSingle` the flat `WindowFrame` line, `None`
/// nothing at all.
fn paint_textbox(c: &dyn ControlCanvas, rect: Rect, t: &ToolStripTextBox) {
    let colors = &c.visuals().colors;
    let item = &t.host.item;
    let inner = match t.border_style {
        BorderStyle::Fixed3D => c.draw_edge(&rect, EdgeStyle::Sunken, Border3DSide::ALL),
        BorderStyle::FixedSingle => {
            c.stroke_rect(&rect, &colors.window_frame);
            edge_interior(&rect, 1, c.scale())
        }
        BorderStyle::None => rect,
    };
    c.fill_rect(&inner, &colors.window);
    let ink = if item.enabled { colors.window_text } else { colors.gray_text };
    let text_rect = inset_h(inner, TEXT_PADDING_H);
    draw_aligned(c, text_rect, &item.text, item.font, ink, DWRITE_TEXT_ALIGNMENT_LEADING);
}

/// A hosted `ProgressBar`: a sunken well filled from the leading edge in
/// `Highlight`, which is the colour a non-themed progress bar has always used.
/// `Blocks` and `Marquee` are still drawn as one continuous fill — the segmented
/// chunks and the animation both need state this family does not carry.
fn paint_progress(c: &dyn ControlCanvas, rect: Rect, pb: &ToolStripProgressBar) {
    let colors = &c.visuals().colors;
    let track = Rect::new(rect.left, rect.top + 2.0, rect.right, rect.bottom - 2.0);
    let inner = c.draw_edge(&track, EdgeStyle::Sunken, Border3DSide::ALL);
    c.fill_rect(&inner, &colors.control);
    let frac = pb.fraction();
    if frac > 0.0 {
        let w = (inner.right - inner.left) * frac;
        c.fill_rect(&Rect::new(inner.left, inner.top, inner.left + w, inner.bottom), &colors.highlight);
    }
}

/// The overflow button — the same downward chevron the toolkit shows to mean
/// « the rest of the items are in here ».
fn paint_overflow_button(c: &dyn ControlCanvas, rect: Rect, ink: &D2D1_COLOR_F) {
    c.vector_icon("ChevronDown", &rect, CHEVRON_SIZE, ink);
}

/// The `StatusStrip` sizing grip: three short diagonal rows of two-tone dots in
/// the bottom-right corner.
///
/// Hand-drawn rather than a glyph — the icon set has no sizing-grip shape — but
/// out of the same shadow/highlight dot the move handle is made of, so the two
/// grips are one decoration at two sizes instead of two inventions.
fn paint_sizing_grip(c: &dyn ControlCanvas, bounds: Rect) {
    let pitch = GRIP_PITCH_PX / c.scale();
    for row in 0..3 {
        for col in 0..=row {
            let x = bounds.right - pitch - col as f32 * pitch;
            let y = bounds.bottom - pitch - row as f32 * pitch;
            paint_grip_dot(c, x, y);
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests — pure geometry and declared defaults, no window needed
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::enums::CheckState;

    const STRIP: Rect = Rect { left: 0.0, top: 0.0, right: 200.0, bottom: 24.0 };

    fn input(width: f32) -> StripItemInput {
        StripItemInput { size: Size::new(width, 24.0), ..StripItemInput::default() }
    }

    // ── Declared defaults ────────────────────────────────────────────────────

    #[test]
    fn strip_defaults_differ_per_type_as_the_toolkit_declares() {
        let ts = ToolStrip::default();
        assert_eq!(ts.grip_style, ToolStripGripStyle::Visible);
        assert_eq!(ts.control().dock, DockStyle::Top);
        assert_eq!(ts.layout_style, ToolStripLayoutStyle::StackWithOverflow);
        assert!(ts.can_overflow && !ts.stretch);
        assert!(ts.control().auto_size && !ts.control().tab_stop);
        assert!(ts.allow_merge, "AllowMerge defaults to true");
        assert!(!ts.allow_click_through && !ts.allow_item_reorder, "both default to false");

        let ms = MenuStrip::default();
        assert_eq!(ms.grip_style, ToolStripGripStyle::Hidden);
        assert_eq!(ms.control().dock, DockStyle::Top);
        assert!(!ms.can_overflow && ms.stretch && !ms.show_item_tool_tips);

        let ss = StatusStrip::default();
        assert_eq!(ss.grip_style, ToolStripGripStyle::Hidden);
        assert_eq!(ss.control().dock, DockStyle::Bottom);
        assert_eq!(ss.layout_style, ToolStripLayoutStyle::Table);
        assert!(!ss.can_overflow && ss.stretch && ss.sizing_grip);

        let dd = ToolStripDropDown::default();
        assert_eq!(dd.control().dock, DockStyle::None);
        assert!(!dd.control().visible, "a drop-down starts hidden");
        assert!(dd.auto_close && dd.drop_shadow_enabled);

        let menu = ToolStripDropDownMenu::default();
        assert_eq!(menu.base.base.layout_style, ToolStripLayoutStyle::Flow);
        assert!(menu.show_image_margin && !menu.show_check_margin);
    }

    #[test]
    fn item_defaults_match_the_docs() {
        let it = ToolStripItem::default();
        assert_eq!(it.display_style, ToolStripItemDisplayStyle::ImageAndText);
        assert_eq!(it.text_align, ContentAlignment::MiddleCenter);
        assert_eq!(it.text_image_relation, TextImageRelation::ImageBeforeText);
        assert_eq!(it.alignment, ToolStripItemAlignment::Left);
        assert_eq!(it.overflow, ToolStripItemOverflow::AsNeeded);
        assert!(it.auto_size && it.enabled && it.visible && !it.auto_tooltip);
        assert_eq!(it.margin, Padding::new(0.0, 1.0, 0.0, 2.0));

        // The leaves add only their own, with their own defaults.
        let b = ToolStripButton::default();
        assert!(!b.checked && !b.check_on_click && b.check_state == CheckState::Unchecked);
        assert!(b.item.auto_tooltip, "ToolStripButton overrides AutoToolTip to true");

        let m = ToolStripMenuItem::default();
        assert!(m.show_shortcut_keys && m.shortcut_keys.is_none() && !m.checked);

        let sl = ToolStripStatusLabel::default();
        assert!(!sl.spring);
        assert_eq!(sl.border_sides, ToolStripStatusLabelBorderSides::NONE);
        assert_eq!(sl.item.margin, Padding::new(0.0, 3.0, 0.0, 2.0));

        let pb = ToolStripProgressBar::default();
        assert_eq!((pb.minimum, pb.maximum, pb.value(), pb.step), (0, 100, 0, 10));
    }

    // ── Item measurement per DisplayStyle ────────────────────────────────────

    #[test]
    fn measurement_follows_display_style_and_relation() {
        let text = Size::new(40.0, 16.0);
        let image = Size::new(16.0, 16.0);

        // Text only.
        let s = measure_item_content(ToolStripItemDisplayStyle::Text, TextImageRelation::ImageBeforeText, text, image, 4.0);
        assert_eq!(s, text);

        // Image only.
        let s = measure_item_content(ToolStripItemDisplayStyle::Image, TextImageRelation::ImageBeforeText, text, image, 4.0);
        assert_eq!(s, image);

        // Both, side by side: widths add + gap, height is the taller.
        let s = measure_item_content(ToolStripItemDisplayStyle::ImageAndText, TextImageRelation::ImageBeforeText, text, image, 4.0);
        assert_eq!(s, Size::new(16.0 + 4.0 + 40.0, 16.0));

        // Both, stacked: heights add + gap, width is the wider.
        let s = measure_item_content(ToolStripItemDisplayStyle::ImageAndText, TextImageRelation::TextAboveImage, text, image, 4.0);
        assert_eq!(s, Size::new(40.0, 16.0 + 4.0 + 16.0));

        // Both, overlaid: the bounding box.
        let s = measure_item_content(ToolStripItemDisplayStyle::ImageAndText, TextImageRelation::Overlay, text, image, 4.0);
        assert_eq!(s, Size::new(40.0, 16.0));

        // DisplayStyle::None shows nothing.
        let s = measure_item_content(ToolStripItemDisplayStyle::None, TextImageRelation::ImageBeforeText, text, image, 4.0);
        assert_eq!(s, Size::EMPTY);
    }

    // ── Horizontal stack with alignment ──────────────────────────────────────

    #[test]
    fn horizontal_stack_packs_left_then_right() {
        let mut right = input(30.0);
        right.alignment = ToolStripItemAlignment::Right;
        let items = [input(50.0), input(20.0), right];
        let out = layout_items(StripAxis::Horizontal, STRIP, 0.0, 16.0, false, false, &items);

        // Left group packs from the leading edge.
        assert_eq!((out.rects[0].left, out.rects[0].right), (0.0, 50.0));
        assert_eq!((out.rects[1].left, out.rects[1].right), (50.0, 70.0));
        // Right item sits against the trailing edge.
        assert_eq!((out.rects[2].left, out.rects[2].right), (170.0, 200.0));
        assert!(out.overflow_button.is_none());
    }

    #[test]
    fn a_visible_grip_shifts_the_first_item_right() {
        let out = layout_items(StripAxis::Horizontal, STRIP, 11.0, 16.0, false, false, &[input(50.0)]);
        assert_eq!(out.rects[0].left, 11.0, "the grip reserve pushes items off the lead edge");
    }

    // ── The metrics are DIP, never multiplied by the DPI factor ──────────────

    /// The regression that the demo caught at 175 %: the device context is
    /// already set to the window's DPI, so multiplying a metric by
    /// `Canvas::scale` applied the factor twice. It inflated the measured items
    /// by 1.75× and pushed the last one into the overflow menu — a *behaviour*
    /// change, not just a cosmetic one. These two metrics need no canvas at all,
    /// which is precisely the proof that no DPI arithmetic is left in them.
    #[test]
    fn strip_metrics_are_plain_dip_and_need_no_canvas() {
        let ts = ToolStrip::default();
        // GRIP_WIDTH (7) + GripMargin.Horizontal (2 + 2), as DIP.
        assert_eq!(ts.grip_reserve(), 11.0);
        // max(ImageScalingSize.Height 16, GLYPH_HEIGHT 15) + 4.
        assert_eq!(ts.row_height(), 20.0);

        // A hidden grip reserves nothing at all.
        assert_eq!(MenuStrip::default().grip_reserve(), 0.0);
    }

    /// The demo strip's real proportions: grip + « Enregistrer » + separator +
    /// « Étiquette » + combo + « Activé » inside its group box. At true DIP they
    /// fit with room to spare, so nothing may overflow — the 1.75× inflation was
    /// the only reason « Activé » ever moved into the overflow button.
    #[test]
    fn the_demo_toolstrip_fits_without_overflowing_at_dip_sizes() {
        let items = [input(82.0), input(6.0), input(72.0), input(100.0), input(52.0)];
        let strip = Rect::new(0.0, 0.0, 465.0, 24.0);
        let out = layout_items(StripAxis::Horizontal, strip, 11.0, OVERFLOW_WIDTH, true, false, &items);
        assert_eq!(out.on_overflow, vec![false; 5], "every item fits at DIP sizes");
        assert!(out.overflow_button.is_none(), "so no overflow button is shown");
        // The same content inflated by 1.75 no longer fits — the old bug.
        let inflated: Vec<StripItemInput> = items.iter().map(|i| input(i.size.width * 1.75)).collect();
        let bad = layout_items(StripAxis::Horizontal, strip, 11.0 * 1.75, OVERFLOW_WIDTH, true, false, &inflated);
        assert!(bad.on_overflow.iter().any(|&o| o), "double-scaling forced an overflow");
    }

    // ── Spring width distribution ────────────────────────────────────────────

    #[test]
    fn spring_shares_leftover_evenly() {
        let mut a = input(10.0);
        a.spring = true;
        let mut b = input(10.0);
        b.spring = true;
        // Width 100, used 20, leftover 80, two springs → +40 each → 50 wide.
        let out = layout_items(StripAxis::Horizontal, Rect::new(0.0, 0.0, 100.0, 24.0), 0.0, 16.0, false, true, &[a, b]);
        assert_eq!(out.rects[0].right - out.rects[0].left, 50.0);
        assert_eq!(out.rects[1].right - out.rects[1].left, 50.0);
        // They tile without a gap.
        assert_eq!(out.rects[1].left, out.rects[0].right);
    }

    #[test]
    fn spring_hands_the_indivisible_remainder_to_the_earliest() {
        let mut items = [input(10.0), input(10.0), input(10.0)];
        for it in &mut items {
            it.spring = true;
        }
        // Width 110, used 30, leftover 80, three springs: 80/3 = 26 rem 2, so the
        // first two get 26+1 = 27 and the last gets 26 (10 base + share).
        let out = layout_items(StripAxis::Horizontal, Rect::new(0.0, 0.0, 110.0, 24.0), 0.0, 16.0, false, true, &items);
        let widths: Vec<f32> = out.rects.iter().map(|r| r.right - r.left).collect();
        assert_eq!(widths, vec![10.0 + 27.0, 10.0 + 27.0, 10.0 + 26.0]);
        assert_eq!(widths.iter().sum::<f32>(), 110.0, "the whole width is used");
    }

    // ── Overflow selection ───────────────────────────────────────────────────

    #[test]
    fn overflow_pushes_trailing_items_when_too_narrow() {
        // Four 60-wide items in a 150-wide strip; the overflow button costs 16.
        let narrow = Rect::new(0.0, 0.0, 150.0, 24.0);
        let items = [input(60.0), input(60.0), input(60.0), input(60.0)];
        let out = layout_items(StripAxis::Horizontal, narrow, 0.0, 16.0, true, false, &items);
        assert!(out.overflow_button.is_some());
        // 3×60 = 180 > 150-16 = 134, so the last two overflow, leaving 2×60 = 120 ≤ 134.
        assert_eq!(out.on_overflow, vec![false, false, true, true]);
    }

    #[test]
    fn overflow_never_keeps_an_item_on_strip_and_always_removes_one() {
        let mut never = input(60.0);
        never.overflow = ToolStripItemOverflow::Never;
        let mut always = input(20.0);
        always.overflow = ToolStripItemOverflow::Always;
        let items = [input(60.0), never, input(60.0), always];
        let out = layout_items(StripAxis::Horizontal, STRIP, 0.0, 16.0, true, false, &items);
        assert!(out.on_overflow[3], "Always overflows regardless of room");
        assert!(!out.on_overflow[1], "Never stays on strip even under pressure");
    }

    #[test]
    fn no_overflow_button_when_the_strip_cannot_overflow() {
        let items = [input(120.0), input(120.0)];
        let out = layout_items(StripAxis::Horizontal, STRIP, 0.0, 16.0, false, false, &items);
        assert!(out.overflow_button.is_none());
        assert_eq!(out.on_overflow, vec![false, false]);
    }

    // ── Menu-item shortcut text ──────────────────────────────────────────────

    #[test]
    fn shortcut_display_string_uses_toolkit_modifier_order() {
        assert_eq!(Shortcut::new(true, false, false, "S").display_string(), "Ctrl+S");
        assert_eq!(Shortcut::new(true, true, true, "Del").display_string(), "Ctrl+Alt+Shift+Del");
        assert_eq!(Shortcut::new(false, false, false, "F4").display_string(), "F4");
    }

    #[test]
    fn shortcut_text_respects_show_flag_and_override() {
        let mut m = ToolStripMenuItem::new("Enregistrer");
        m.shortcut_keys = Some(Shortcut::new(true, false, false, "S"));
        assert_eq!(m.shortcut_text(), "Ctrl+S");

        m.shortcut_key_display_string = Some("Ctrl+Enr".into());
        assert_eq!(m.shortcut_text(), "Ctrl+Enr", "the display-string override wins");

        m.show_shortcut_keys = false;
        assert_eq!(m.shortcut_text(), "", "hidden when ShowShortcutKeys is off");
    }

    #[test]
    fn a_shortcut_widens_the_menu_item() {
        let plain = measure_menu_item(80.0, 0.0, 20.0, 12.0, 8.0);
        let with_sc = measure_menu_item(80.0, 40.0, 20.0, 12.0, 8.0);
        assert!(with_sc.width > plain.width);
        // The extra is the gap plus the shortcut width.
        assert_eq!(with_sc.width - plain.width, 8.0 + 40.0);
    }

    // ── Painting decisions a test can check without a window ─────────────────

    /// The whole point of carrying a [`StripKind`]: a menu bar is painted from
    /// the MENU colours and a tool bar from the CONTROL ones. Reading
    /// `COLOR_BTNFACE` for both compiles, and looks right on a machine where the
    /// two happen to be the same grey — which is most of them, and is exactly
    /// why this is asserted rather than eyeballed.
    #[test]
    fn a_menu_bar_paints_from_the_menu_palette_and_a_tool_bar_from_the_control_one() {
        // Read the real set, then force the four colours apart: they are equal
        // on a default Windows 11, so a swap would otherwise be invisible.
        let mut colors = crate::system::SystemColors::read();
        let g = |v: f32| D2D1_COLOR_F { r: v, g: v, b: v, a: 1.0 };
        colors.control = g(0.1);
        colors.control_text = g(0.2);
        colors.menu_bar = g(0.3);
        colors.menu_text = g(0.4);

        assert_eq!(StripKind::Tool.palette(&colors), (colors.control, colors.control_text));
        assert_eq!(StripKind::Status.palette(&colors), (colors.control, colors.control_text));
        assert_eq!(StripKind::Menu.palette(&colors), (colors.menu_bar, colors.menu_text));
        // A popup is menu-coloured too — `COLOR_MENU` itself is not among the
        // colours `SystemColors` reads, so `MenuBar` stands in for it.
        assert_eq!(
            StripKind::DropDown { margins: 1 }.palette(&colors),
            (colors.menu_bar, colors.menu_text)
        );
    }

    /// The drop-down gutter is `SM_CXMENUCHECK` per margin shown — never a
    /// hand-picked number, and never scaled.
    #[test]
    fn a_drop_down_reserves_one_check_column_per_margin() {
        let mut metrics = crate::system::SystemMetrics::read(96.0);
        metrics.menu_check_width = 13.0;

        assert_eq!(StripKind::Tool.gutter(&metrics), 0.0);
        assert_eq!(StripKind::Menu.gutter(&metrics), 0.0, "a menu BAR item has no gutter");
        assert_eq!(StripKind::DropDown { margins: 0 }.gutter(&metrics), 0.0);
        assert_eq!(StripKind::DropDown { margins: 1 }.gutter(&metrics), 13.0);
        assert_eq!(StripKind::DropDown { margins: 2 }.gutter(&metrics), 26.0);

        // A menu showing both margins is the two-column case.
        let mut menu = ToolStripDropDownMenu::default();
        assert_eq!(menu.kind(), StripKind::DropDown { margins: 1 }, "image margin only");
        menu.show_check_margin = true;
        assert_eq!(menu.kind(), StripKind::DropDown { margins: 2 });
        menu.show_image_margin = false;
        assert_eq!(menu.kind(), StripKind::DropDown { margins: 1 });
    }

    /// The status label's border property and the paint primitive are two enums
    /// with the same members; the mapping must not lose one.
    #[test]
    fn the_status_label_border_maps_onto_the_system_edge() {
        use crate::system::Border3DStyle as Edge;
        assert_eq!(edge_style(Border3DStyle::Flat), Edge::Flat);
        assert_eq!(edge_style(Border3DStyle::Sunken), Edge::Sunken);
        assert_eq!(edge_style(Border3DStyle::Etched), Edge::Etched);
        assert_eq!(edge_style(Border3DStyle::RaisedOuter), Edge::RaisedOuter);
        // The declared default is `Flat`, which is one `ControlDark` ring — the
        // divider a status label shows, now drawn by `DrawEdge` like every other
        // border in the library.
        assert_eq!(edge_style(ToolStripStatusLabel::default().border_style), Edge::Flat);
    }

    #[test]
    fn the_border_sides_map_side_for_side() {
        let sides = |b: ToolStripStatusLabelBorderSides| edge_sides(b);
        assert_eq!(sides(ToolStripStatusLabelBorderSides::NONE), Border3DSide(0));
        assert_eq!(sides(ToolStripStatusLabelBorderSides::ALL), Border3DSide::ALL);
        let lr = ToolStripStatusLabelBorderSides(
            ToolStripStatusLabelBorderSides::LEFT.0 | ToolStripStatusLabelBorderSides::RIGHT.0,
        );
        let mapped = sides(lr);
        assert!(mapped.has(Border3DSide::LEFT) && mapped.has(Border3DSide::RIGHT));
        assert!(!mapped.has(Border3DSide::TOP) && !mapped.has(Border3DSide::BOTTOM));
    }

    // ── Value clamping trap ──────────────────────────────────────────────────

    #[test]
    fn progress_value_clamps_into_its_range() {
        let mut pb = ToolStripProgressBar::default();
        pb.set_value(150);
        assert_eq!(pb.value(), 100, "clamped to Maximum");
        pb.set_value(-5);
        assert_eq!(pb.value(), 0, "clamped to Minimum");
        assert!((pb.fraction() - 0.0).abs() < f32::EPSILON);
    }
}
