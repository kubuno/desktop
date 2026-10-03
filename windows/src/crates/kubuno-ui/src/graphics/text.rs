//! Text: [`Font`], [`StringFormat`] (alignment, wrapping, trimming, direction) and the rendering
//! hints.

use kubuno_controls::control::FontRole;

/// The style bits of a font (`FontStyle`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct FontStyle(pub u8);

impl FontStyle {
    pub const REGULAR: Self = Self(0);
    pub const BOLD: Self = Self(1);
    pub const ITALIC: Self = Self(2);
    pub const UNDERLINE: Self = Self(4);
    pub const STRIKEOUT: Self = Self(8);

    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
}

impl std::ops::BitOr for FontStyle {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

/// A font (`System.Drawing.Font`): a family and a size, or one of the theme's shared text roles
/// ([`Font::role`], the default: the body text of the app's own font, which follows the app font
/// setting).
#[derive(Debug, Clone, PartialEq)]
pub struct Font {
    /// The family (`"Segoe UI"`); `None` = the role's (the app font).
    pub family: Option<String>,
    /// The em size in DIP (1/96 inch); `None` = the role's.
    pub size: Option<f32>,
    pub style: FontStyle,
    /// The shared text format this font starts from.
    pub role: FontRole,
}

impl Default for Font {
    fn default() -> Self {
        Self::role(FontRole::Body)
    }
}

impl Font {
    /// `new Font(family, emSize, style)`, the size in POINTS like WinForms' default unit.
    pub fn new(family: &str, size_pt: f32, style: FontStyle) -> Self {
        Self { family: Some(family.to_string()), size: Some(size_pt.max(0.1) * 96.0 / 72.0), style, role: FontRole::Body }
    }

    /// A family and a size in DIP (`GraphicsUnit.Pixel` at 96 DPI).
    pub fn with_dip(family: &str, size_dip: f32, style: FontStyle) -> Self {
        Self { family: Some(family.to_string()), size: Some(size_dip.max(0.1)), style, role: FontRole::Body }
    }

    /// One of the theme's shared text formats (caption, body, heading…) in the app's font.
    pub fn role(role: FontRole) -> Self {
        Self { family: None, size: None, style: FontStyle::REGULAR, role }
    }

    /// The same font in another size (DIP).
    pub fn sized(mut self, size_dip: f32) -> Self {
        self.size = Some(size_dip.max(0.1));
        self
    }

    /// The same font with `style` added.
    pub fn styled(mut self, style: FontStyle) -> Self {
        self.style = self.style | style;
        self
    }

    pub fn bold(self) -> Self {
        self.styled(FontStyle::BOLD)
    }

    pub fn italic(self) -> Self {
        self.styled(FontStyle::ITALIC)
    }

    /// Whether it is exactly a shared text format (nothing overridden), which is drawn with the
    /// theme's own format object.
    pub fn is_plain_role(&self) -> bool {
        self.family.is_none() && self.size.is_none() && self.style == FontStyle::REGULAR
    }

    /// The nominal em size of the role, in DIP (the Kubuno type ramp), for sizing without a text
    /// engine.
    pub fn nominal_size(&self) -> f32 {
        self.size.unwrap_or(match self.role {
            FontRole::Caption | FontRole::CaptionStrong => 12.0,
            FontRole::Body | FontRole::BodyStrong => 14.0,
            FontRole::Heading => 18.0,
            FontRole::Title => 24.0,
        })
    }

    /// The height of a line in DIP (`Font.Height`), nominally 1.25 em.
    pub fn height(&self) -> f32 {
        self.nominal_size() * 1.25
    }
}

/// Where text sits along an axis (`StringAlignment`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum StringAlignment {
    /// Left (or right in right-to-left text), top.
    #[default]
    Near,
    Center,
    Far,
}

/// How text that does not fit is cut (`StringTrimming`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum StringTrimming {
    /// Cut at the edge of the layout box, no mark.
    None,
    /// At the last character that fits.
    #[default]
    Character,
    /// At the last word that fits.
    Word,
    /// At a character, with « … ».
    EllipsisCharacter,
    /// At a word, with « … ».
    EllipsisWord,
    /// « … » in the middle (a path: `C:\…\file.txt`) — drawn as `EllipsisCharacter` by DirectWrite
    /// with the last path segment kept.
    EllipsisPath,
}

/// Layout switches (`StringFormatFlags`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub struct StringFormatFlags(pub u16);

impl StringFormatFlags {
    pub const NONE: Self = Self(0);
    /// Right-to-left reading order.
    pub const DIRECTION_RIGHT_TO_LEFT: Self = Self(0x0001);
    /// Never wraps (one line per paragraph).
    pub const NO_WRAP: Self = Self(0x1000);
    /// Text may paint outside the layout box.
    pub const NO_CLIP: Self = Self(0x4000);
    /// Only whole lines are shown.
    pub const LINE_LIMIT: Self = Self(0x2000);
    /// Trailing spaces count when measuring.
    pub const MEASURE_TRAILING_SPACES: Self = Self(0x0800);

    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
}

impl std::ops::BitOr for StringFormatFlags {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

/// How a string is laid out in its box (`StringFormat`).
#[derive(Debug, Clone, Copy, PartialEq, Default, Hash, Eq)]
pub struct StringFormat {
    /// Horizontal alignment.
    pub alignment: StringAlignment,
    /// Vertical alignment (`LineAlignment`).
    pub line_alignment: StringAlignment,
    pub trimming: StringTrimming,
    pub flags: StringFormatFlags,
}

impl StringFormat {
    /// `StringFormat.GenericDefault`: top-left, wrapping, character trimming.
    pub const fn generic_default() -> Self {
        Self { alignment: StringAlignment::Near, line_alignment: StringAlignment::Near, trimming: StringTrimming::Character, flags: StringFormatFlags::NONE }
    }

    /// `StringFormat.GenericTypographic`: no trimming, trailing-space-exact, no clipping.
    pub const fn generic_typographic() -> Self {
        Self {
            alignment: StringAlignment::Near,
            line_alignment: StringAlignment::Near,
            trimming: StringTrimming::None,
            flags: StringFormatFlags(StringFormatFlags::NO_CLIP.0 | StringFormatFlags::LINE_LIMIT.0),
        }
    }

    /// Centred both ways (the common "label in a box").
    pub const fn centered() -> Self {
        Self { alignment: StringAlignment::Center, line_alignment: StringAlignment::Center, trimming: StringTrimming::Character, flags: StringFormatFlags::NONE }
    }

    /// One line, vertically centred, ellipsis at the end — a list row's label.
    pub const fn single_line_ellipsis() -> Self {
        Self { alignment: StringAlignment::Near, line_alignment: StringAlignment::Center, trimming: StringTrimming::EllipsisCharacter, flags: StringFormatFlags::NO_WRAP }
    }

    pub const fn with_alignment(mut self, alignment: StringAlignment) -> Self {
        self.alignment = alignment;
        self
    }

    pub const fn with_line_alignment(mut self, alignment: StringAlignment) -> Self {
        self.line_alignment = alignment;
        self
    }

    pub const fn with_trimming(mut self, trimming: StringTrimming) -> Self {
        self.trimming = trimming;
        self
    }

    pub const fn with_flags(mut self, flags: StringFormatFlags) -> Self {
        self.flags = StringFormatFlags(self.flags.0 | flags.0);
        self
    }

    pub const fn wraps(&self) -> bool {
        !self.flags.contains(StringFormatFlags::NO_WRAP)
    }
}

/// Edge antialiasing of shapes (`SmoothingMode`). Kubuno's default antialiases (WinForms' default
/// does not: the modern look needs it).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum SmoothingMode {
    /// Antialiased (Kubuno's default).
    #[default]
    Default,
    HighSpeed,
    HighQuality,
    /// Aliased: crisp, jagged edges.
    None,
    AntiAlias,
}

impl SmoothingMode {
    pub fn antialiased(self) -> bool {
        !matches!(self, SmoothingMode::HighSpeed | SmoothingMode::None)
    }
}

/// How glyphs are rasterised (`TextRenderingHint`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum TextRenderingHint {
    /// The surface's setting (ClearType when the system uses it).
    #[default]
    SystemDefault,
    SingleBitPerPixelGridFit,
    SingleBitPerPixel,
    AntiAliasGridFit,
    /// Greyscale antialiasing (what a transparent or rotated surface needs).
    AntiAlias,
    ClearTypeGridFit,
}

/// How images are resampled (`InterpolationMode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum InterpolationMode {
    #[default]
    Default,
    Low,
    High,
    Bilinear,
    Bicubic,
    NearestNeighbor,
    HighQualityBilinear,
    HighQualityBicubic,
}

/// Whether drawing blends over what is there or replaces it (`CompositingMode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum CompositingMode {
    #[default]
    SourceOver,
    SourceCopy,
}

/// A rough, engine-free text measure: 0.55 em per character, 1.25 em per line (wrapping at
/// `max_width`). What a surface with no text engine (a test recorder) answers, deterministic.
pub fn approximate_measure(text: &str, font: &Font, max_width: Option<f32>) -> (f32, f32) {
    let em = font.nominal_size();
    let char_w = em * 0.55;
    let line_h = font.height();
    let mut width: f32 = 0.0;
    let mut lines = 0usize;
    for paragraph in text.split('\n') {
        let w = paragraph.chars().count() as f32 * char_w;
        match max_width {
            Some(max) if max > 0.0 && w > max => {
                lines += (w / max).ceil() as usize;
                width = width.max(max);
            }
            _ => {
                lines += 1;
                width = width.max(w);
            }
        }
    }
    (width, lines.max(1) as f32 * line_h)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fonts_are_points_by_default_and_roles_otherwise() {
        let f = Font::new("Segoe UI", 12.0, FontStyle::BOLD | FontStyle::ITALIC);
        assert_eq!(f.size, Some(16.0));
        assert!(f.style.contains(FontStyle::BOLD) && f.style.contains(FontStyle::ITALIC));
        assert!(Font::default().is_plain_role());
        assert!(!Font::default().bold().is_plain_role());
        assert_eq!(Font::role(FontRole::Heading).nominal_size(), 18.0);
    }

    #[test]
    fn string_formats() {
        assert!(StringFormat::generic_default().wraps());
        let one = StringFormat::single_line_ellipsis();
        assert!(!one.wraps() && one.trimming == StringTrimming::EllipsisCharacter);
        let rtl = StringFormat::centered().with_flags(StringFormatFlags::DIRECTION_RIGHT_TO_LEFT);
        assert!(rtl.flags.contains(StringFormatFlags::DIRECTION_RIGHT_TO_LEFT));
        assert!(!SmoothingMode::None.antialiased() && SmoothingMode::Default.antialiased());
    }

    #[test]
    fn the_approximate_measure_wraps() {
        let f = Font::default().sized(10.0);
        let (w, h) = approximate_measure("abcd", &f, None);
        assert!((w - 22.0).abs() < 1e-4 && (h - 12.5).abs() < 1e-4);
        let (_, h2) = approximate_measure("abcd", &f, Some(11.0));
        assert!((h2 - 25.0).abs() < 1e-4);
        assert_eq!(approximate_measure("a\nb", &f, None).1, 25.0);
    }
}
