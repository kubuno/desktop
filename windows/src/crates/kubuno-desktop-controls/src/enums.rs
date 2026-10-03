//! The shared WinForms enumerations, reproduced with their exact members and
//! default values.
//!
//! These are extracted from the shipping `System.Windows.Forms` surface (see
//! `tools/winforms-ref/`), not from prose: a member that exists here exists
//! there, with the same meaning and the same discriminant order. Nothing is
//! invented, and nothing that the toolkit offers is quietly dropped — a control
//! that cannot yet honour a member must say so, never silently treat it as
//! another.
//!
//! An enumeration lives here as soon as **more than one family** declares a
//! property of that type. A single copy is not tidiness for its own sake: four
//! copies of `HorizontalAlignment` were found across the families, and the two
//! copies of `LeftRightAlignment` had already drifted apart on their default
//! (see that type). One definition is what makes such a divergence impossible
//! rather than merely unlikely.

use windows::Win32::Graphics::DirectWrite::{
    DWRITE_TEXT_ALIGNMENT, DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_LEADING,
    DWRITE_TEXT_ALIGNMENT_TRAILING,
};

/// Where a control docks against its parent's edges (`DockStyle`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DockStyle {
    #[default]
    None,
    Top,
    Bottom,
    Left,
    Right,
    Fill,
}

/// Which parent edges a control keeps a fixed distance from (`AnchorStyles`).
/// A bitflag set: the WinForms default is `Top | Left`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnchorStyles(pub u8);

impl AnchorStyles {
    pub const NONE: Self = Self(0);
    pub const TOP: Self = Self(1);
    pub const BOTTOM: Self = Self(2);
    pub const LEFT: Self = Self(4);
    pub const RIGHT: Self = Self(8);

    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

impl Default for AnchorStyles {
    /// `Top | Left` — the WinForms default.
    fn default() -> Self {
        Self(1 | 4)
    }
}

/// How a control paints its border (`BorderStyle`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BorderStyle {
    None,
    FixedSingle,
    /// The WinForms default for `TextBox`, `Panel` uses `None`.
    #[default]
    Fixed3D,
}

/// How a button-like control paints (`FlatStyle`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FlatStyle {
    Flat,
    Popup,
    #[default]
    Standard,
    System,
}

/// Where content sits inside a control's box (`ContentAlignment`). The nine
/// members carry the toolkit's own discriminants so a round-trip is exact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ContentAlignment {
    TopLeft = 1,
    TopCenter = 2,
    TopRight = 4,
    MiddleLeft = 16,
    #[default]
    MiddleCenter = 32,
    MiddleRight = 64,
    BottomLeft = 256,
    BottomCenter = 512,
    BottomRight = 1024,
}

impl ContentAlignment {
    /// `(horizontal, vertical)` as 0.0/0.5/1.0 fractions — what a painter needs.
    pub const fn fractions(self) -> (f32, f32) {
        let h = match self {
            Self::TopLeft | Self::MiddleLeft | Self::BottomLeft => 0.0,
            Self::TopCenter | Self::MiddleCenter | Self::BottomCenter => 0.5,
            Self::TopRight | Self::MiddleRight | Self::BottomRight => 1.0,
        };
        let v = match self {
            Self::TopLeft | Self::TopCenter | Self::TopRight => 0.0,
            Self::MiddleLeft | Self::MiddleCenter | Self::MiddleRight => 0.5,
            Self::BottomLeft | Self::BottomCenter | Self::BottomRight => 1.0,
        };
        (h, v)
    }
}

/// Horizontal-only alignment (`HorizontalAlignment`).
///
/// Distinct from [`ContentAlignment`], which is the nine-cell box alignment: the
/// toolkit uses this one wherever only the horizontal axis is adjustable —
/// `TextBox.TextAlign` and a `ListView` column's `TextAlign`, among others. It
/// lives here rather than in one family because more than one family needs it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HorizontalAlignment {
    #[default]
    Left = 0,
    Right = 1,
    Center = 2,
}

impl HorizontalAlignment {
    /// The matching DirectWrite alignment, so the three families that use this
    /// (TextBox, ListView columns, UpDownBase) share one mapping instead of
    /// each writing their own.
    pub const fn dwrite(self) -> DWRITE_TEXT_ALIGNMENT {
        match self {
            Self::Left => DWRITE_TEXT_ALIGNMENT_LEADING,
            Self::Right => DWRITE_TEXT_ALIGNMENT_TRAILING,
            Self::Center => DWRITE_TEXT_ALIGNMENT_CENTER,
        }
    }
}

/// Which side something sits on (`LeftRightAlignment`).
///
/// The toolkit's two-value alignment, distinct from [`HorizontalAlignment`]
/// because there is no « centre ».
///
/// **It deliberately implements no `Default`.** The catalogue shows the two
/// properties that use it disagree: `UpDownBase.UpDownAlign` defaults to
/// `Right`, `DateTimePicker.DropDownAlign` to `Left`. The default therefore
/// belongs to the *property*, not to the type — a `#[default]` here would look
/// harmless and would silently flip one of them. Each owner states its own in
/// its `Default` impl.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeftRightAlignment {
    Left = 0,
    Right = 1,
}

/// The Input Method Editor mode a control asks for (`ImeMode`).
///
/// Carried because `Control` declares it; the port drives no IME, so the host
/// is what would act on it. Dropping it would have been the wrong answer — a
/// caller porting a form must be able to round-trip the property.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ImeMode {
    #[default]
    Inherit = -1,
    NoControl = 0,
    On = 1,
    Off = 2,
    Disable = 3,
    Hiragana = 4,
    Katakana = 5,
    KatakanaHalf = 6,
    AlphaFull = 7,
    Alpha = 8,
    HangulFull = 9,
    Hangul = 10,
    Close = 11,
    OnHalf = 12,
}

/// What a control reports itself as to assistive technology (`AccessibleRole`).
///
/// The full .NET set, in its own discriminant order, so a value round-trips
/// exactly. `Default` means « the role the control would report anyway », which
/// is a different fact from any explicit role and must not be collapsed into
/// `None`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AccessibleRole {
    #[default]
    Default = -1,
    None = 0,
    TitleBar = 1,
    MenuBar = 2,
    ScrollBar = 3,
    Grip = 4,
    Sound = 5,
    Cursor = 6,
    Caret = 7,
    Alert = 8,
    Window = 9,
    Client = 10,
    MenuPopup = 11,
    MenuItem = 12,
    ToolTip = 13,
    Application = 14,
    Document = 15,
    Pane = 16,
    Chart = 17,
    Dialog = 18,
    Border = 19,
    Grouping = 20,
    Separator = 21,
    ToolBar = 22,
    StatusBar = 23,
    Table = 24,
    ColumnHeader = 25,
    RowHeader = 26,
    Column = 27,
    Row = 28,
    Cell = 29,
    Link = 30,
    HelpBalloon = 31,
    Character = 32,
    List = 33,
    ListItem = 34,
    Outline = 35,
    OutlineItem = 36,
    PageTab = 37,
    PropertyPage = 38,
    Indicator = 39,
    Graphic = 40,
    StaticText = 41,
    Text = 42,
    PushButton = 43,
    CheckButton = 44,
    RadioButton = 45,
    ComboBox = 46,
    DropList = 47,
    ProgressBar = 48,
    Dial = 49,
    HotkeyField = 50,
    Slider = 51,
    SpinButton = 52,
    Diagram = 53,
    Animation = 54,
    Equation = 55,
    ButtonDropDown = 56,
    ButtonMenu = 57,
    ButtonDropDownGrid = 58,
    WhiteSpace = 59,
    PageTabList = 60,
    Clock = 61,
    SplitButton = 62,
    IpAddress = 63,
    OutlineButton = 64,
}

/// A three-state check (`CheckState`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CheckState {
    #[default]
    Unchecked,
    Checked,
    Indeterminate,
}

/// Whether a check/radio paints as a glyph or as a toggle button (`Appearance`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Appearance {
    #[default]
    Normal,
    Button,
}

/// Which scrollbars a control offers (`ScrollBars`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ScrollBars {
    #[default]
    None,
    Horizontal,
    Vertical,
    Both,
}

/// Reading order (`RightToLeft`). `Inherit` takes the parent's value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RightToLeft {
    No,
    Yes,
    #[default]
    Inherit,
}

/// Which directions an auto-sizing control may grow (`AutoSizeMode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AutoSizeMode {
    #[default]
    GrowOnly,
    GrowAndShrink,
}

/// How a background image is laid out (`ImageLayout`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ImageLayout {
    None,
    #[default]
    Tile,
    Center,
    Stretch,
    Zoom,
}

/// The four-sided box used for both `Margin` and `Padding` (`Padding`).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Padding {
    pub left:   f32,
    pub top:    f32,
    pub right:  f32,
    pub bottom: f32,
}

impl Padding {
    pub const ZERO: Self = Self { left: 0.0, top: 0.0, right: 0.0, bottom: 0.0 };

    /// The same amount on all four sides — `new Padding(all)`.
    pub const fn all(v: f32) -> Self {
        Self { left: v, top: v, right: v, bottom: v }
    }

    pub const fn new(left: f32, top: f32, right: f32, bottom: f32) -> Self {
        Self { left, top, right, bottom }
    }

    /// `Padding.Horizontal` — left + right.
    pub const fn horizontal(self) -> f32 {
        self.left + self.right
    }

    /// `Padding.Vertical` — top + bottom.
    pub const fn vertical(self) -> f32 {
        self.top + self.bottom
    }
}

/// A width/height pair (`Size`). `Size::EMPTY` is WinForms' `Size.Empty`, which
/// is what `MinimumSize`/`MaximumSize` use to mean « no constraint ».
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Size {
    pub width:  f32,
    pub height: f32,
}

impl Size {
    pub const EMPTY: Self = Self { width: 0.0, height: 0.0 };

    pub const fn new(width: f32, height: f32) -> Self {
        Self { width, height }
    }

    pub const fn is_empty(self) -> bool {
        self.width == 0.0 && self.height == 0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_anchor_default_is_top_left() {
        let a = AnchorStyles::default();
        assert!(a.contains(AnchorStyles::TOP));
        assert!(a.contains(AnchorStyles::LEFT));
        assert!(!a.contains(AnchorStyles::RIGHT));
        assert!(!a.contains(AnchorStyles::BOTTOM));
    }

    #[test]
    fn alignment_fractions_cover_the_nine_cells() {
        assert_eq!(ContentAlignment::TopLeft.fractions(), (0.0, 0.0));
        assert_eq!(ContentAlignment::MiddleCenter.fractions(), (0.5, 0.5));
        assert_eq!(ContentAlignment::BottomRight.fractions(), (1.0, 1.0));
    }

    #[test]
    fn padding_sums_its_sides() {
        let p = Padding::new(1.0, 2.0, 3.0, 4.0);
        assert_eq!(p.horizontal(), 4.0);
        assert_eq!(p.vertical(), 6.0);
        assert_eq!(Padding::all(3.0).horizontal(), 6.0);
    }

    /// The documented WinForms defaults, which the port must not drift from.
    #[test]
    fn defaults_match_the_toolkit() {
        assert_eq!(DockStyle::default(), DockStyle::None);
        assert_eq!(FlatStyle::default(), FlatStyle::Standard);
        assert_eq!(CheckState::default(), CheckState::Unchecked);
        assert_eq!(Appearance::default(), Appearance::Normal);
        assert_eq!(RightToLeft::default(), RightToLeft::Inherit);
        assert_eq!(ImageLayout::default(), ImageLayout::Tile);
        assert_eq!(AutoSizeMode::default(), AutoSizeMode::GrowOnly);
    }
}
