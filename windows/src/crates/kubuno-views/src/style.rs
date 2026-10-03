//! The values of the WinForms-rich property set (`vskubuno/docs/EVENTS.md` §16): colours (Kubuno
//! theme tokens first, then free colours), fonts, boxes (`Margin`/`Padding`), sizes, cursors, and the
//! WCAG contrast check the designer warns with.
//!
//! ## Colours
//!
//! The colour policy is **theme tokens by default, free colours allowed**. A colour attribute holds:
//!
//! - nothing: the ambient colour (the parent's, else the theme's);
//! - a Kubuno theme token (`Primary`, `Surface`, `Danger`…, [`THEME_TOKENS`]): it follows the light,
//!   dark and high-contrast themes by itself;
//! - a free colour: `#RGB`, `#RRGGBB`, `#RRGGBBAA` (CSS order, alpha last), a web colour name
//!   (`CornflowerBlue`, [`WEB_COLORS`]) or a Windows system colour (`ControlText`, see
//!   `kubuno_controls::styled::SYSTEM_COLORS`). A free colour does not follow the theme, so it can
//!   break the contrast in one of them: [`contrast_warnings`] says when.
//!
//! The grammars match the Visual Studio side (its colour editor and converters).

use kubuno_controls::ControlCanvas;
use kubuno_ui::Theme;
use kubuno_controls::styled::D2D1_COLOR_F;

/// A colour as four 8-bit channels (`#RRGGBBAA`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rgba(pub u8, pub u8, pub u8, pub u8);

impl Rgba {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self(r, g, b, 255)
    }

    pub fn d2d(self) -> D2D1_COLOR_F {
        D2D1_COLOR_F { r: f32::from(self.0) / 255.0, g: f32::from(self.1) / 255.0, b: f32::from(self.2) / 255.0, a: f32::from(self.3) / 255.0 }
    }

    pub fn from_d2d(c: D2D1_COLOR_F) -> Self {
        let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        Self(q(c.r), q(c.g), q(c.b), q(c.a))
    }

    /// `#RRGGBB`, or `#RRGGBBAA` when not opaque.
    pub fn hex(self) -> String {
        if self.3 == 255 {
            format!("#{:02X}{:02X}{:02X}", self.0, self.1, self.2)
        } else {
            format!("#{:02X}{:02X}{:02X}{:02X}", self.0, self.1, self.2, self.3)
        }
    }
}

/// One Kubuno theme token a colour property may name.
#[derive(Debug, Clone, Copy)]
pub struct ThemeToken {
    pub name: &'static str,
    pub doc: &'static str,
    pub doc_fr: &'static str,
    /// Its value in a theme.
    pub get: fn(&Theme) -> D2D1_COLOR_F,
    /// The system colour it becomes under Windows' high contrast.
    pub high_contrast: &'static str,
}

impl ThemeToken {
    /// The token's value in the light theme.
    pub fn light(&self) -> Rgba {
        Rgba::from_d2d((self.get)(&light_theme()))
    }

    /// The token's value in the dark theme.
    pub fn dark(&self) -> Rgba {
        Rgba::from_d2d((self.get)(&dark_theme()))
    }
}

fn light_theme() -> Theme {
    thread_local!(static LIGHT: Theme = Theme::light());
    LIGHT.with(Theme::clone)
}

fn dark_theme() -> Theme {
    thread_local!(static DARK: Theme = Theme::dark());
    DARK.with(Theme::clone)
}

macro_rules! token {
    ($name:literal, $field:ident, $hc:literal, $doc:literal, $fr:literal) => {
        ThemeToken { name: $name, doc: $doc, doc_fr: $fr, get: |t: &Theme| t.$field, high_contrast: $hc }
    };
}

/// The Kubuno theme tokens, in the order the colour editor lists them.
pub const THEME_TOKENS: &[ThemeToken] = &[
    token!("Primary", accent, "Highlight", "The main accent colour (primary buttons, links, selection).", "Couleur d'accent principale (boutons principaux, liens, sélection)."),
    token!("PrimaryHover", accent_hover, "Highlight", "The accent colour under the mouse.", "Couleur d'accent sous la souris."),
    token!("PrimaryLight", accent_light, "Window", "A pale tint of the accent colour, for backgrounds.", "Teinte pâle de la couleur d'accent, pour les fonds."),
    token!("OnPrimary", accent_foreground, "HighlightText", "Text and icons shown on the accent colour.", "Texte et icônes affichés sur la couleur d'accent."),
    token!("Background", window_background, "Window", "The window background.", "Fond de la fenêtre."),
    token!("Surface", layer_background, "Window", "The main content surface.", "Surface principale du contenu."),
    token!("Surface1", card_background, "Window", "A raised surface (cards, secondary buttons).", "Surface surélevée (cartes, boutons secondaires)."),
    token!("Surface2", surface_2, "Window", "A darker surface step (hover of light buttons).", "Surface un cran plus foncée (survol des boutons clairs)."),
    token!("Surface3", surface_3, "Window", "The darkest surface step (pressed light buttons).", "Surface la plus foncée (boutons clairs enfoncés)."),
    token!("TextPrimary", text_primary, "WindowText", "The main text colour.", "Couleur principale du texte."),
    token!("TextSecondary", text_secondary, "WindowText", "Secondary text (descriptions, hints).", "Texte secondaire (descriptions, indications)."),
    token!("TextTertiary", text_tertiary, "WindowText", "Subtle text (column headers, captions).", "Texte discret (en-têtes de colonnes, légendes)."),
    token!("Border", card_stroke, "WindowText", "The usual border colour.", "Couleur habituelle des bordures."),
    token!("BorderStrong", border_strong, "WindowText", "A stronger border (under the mouse).", "Bordure plus marquée (sous la souris)."),
    token!("Divider", divider, "WindowText", "Separator lines.", "Lignes de séparation."),
    token!("Danger", danger, "WindowText", "Errors and destructive actions.", "Erreurs et actions destructrices."),
    token!("DangerLight", danger_light, "Window", "A pale background for errors.", "Fond pâle pour les erreurs."),
    token!("Success", success, "WindowText", "Success and confirmation.", "Réussite et confirmation."),
    token!("SuccessLight", success_light, "Window", "A pale background for successes.", "Fond pâle pour les réussites."),
    token!("Warning", warning, "WindowText", "Warnings.", "Avertissements."),
    token!("WarningLight", warning_light, "Window", "A pale background for warnings.", "Fond pâle pour les avertissements."),
    token!("Caution", caution, "WindowText", "Warning text, readable on light backgrounds.", "Texte d'avertissement, lisible sur fond clair."),
    token!("LinkVisited", link_visited, "HotTrack", "A link that was already followed.", "Lien déjà visité."),
    token!("Selection", list_selected, "Highlight", "The background of a selected row.", "Fond d'une ligne sélectionnée."),
    token!("Hover", row_hover, "Window", "The background of a row under the mouse.", "Fond d'une ligne sous la souris."),
    token!("ListSelected", list_selected, "Highlight", "The background of a selected item of a list (the same colour as Selection).", "Fond d'un élément sélectionné d'une liste (même couleur que Selection)."),
    token!("ControlFillHover", control_fill_hover, "Window", "The background of a subtle control (an icon button, a menu row) under the mouse.", "Fond d'un contrôle discret (bouton icône, ligne de menu) sous la souris."),
    token!("TitleBarBackground", titlebar_background, "ActiveCaption", "The background of the window's title bar.", "Fond de la barre de titre de la fenêtre."),
    token!("TooltipBackground", tooltip_background, "Info", "The background of tooltips.", "Fond des info-bulles."),
    token!("TooltipForeground", tooltip_foreground, "InfoText", "The text of tooltips.", "Texte des info-bulles."),
];

/// The theme token named `name` (exact spelling).
pub fn theme_token(name: &str) -> Option<&'static ThemeToken> {
    THEME_TOKENS.iter().find(|t| t.name == name)
}

/// The .NET web colours (`KnownColor` web names), `Transparent` included.
pub const WEB_COLORS: &[(&str, u32)] = &[
    ("Transparent", 0x00FF_FFFF), ("AliceBlue", 0xF0F8FF), ("AntiqueWhite", 0xFAEBD7), ("Aqua", 0x00FFFF), ("Aquamarine", 0x7FFFD4),
    ("Azure", 0xF0FFFF), ("Beige", 0xF5F5DC), ("Bisque", 0xFFE4C4), ("Black", 0x000000), ("BlanchedAlmond", 0xFFEBCD),
    ("Blue", 0x0000FF), ("BlueViolet", 0x8A2BE2), ("Brown", 0xA52A2A), ("BurlyWood", 0xDEB887), ("CadetBlue", 0x5F9EA0),
    ("Chartreuse", 0x7FFF00), ("Chocolate", 0xD2691E), ("Coral", 0xFF7F50), ("CornflowerBlue", 0x6495ED), ("Cornsilk", 0xFFF8DC),
    ("Crimson", 0xDC143C), ("Cyan", 0x00FFFF), ("DarkBlue", 0x00008B), ("DarkCyan", 0x008B8B), ("DarkGoldenrod", 0xB8860B),
    ("DarkGray", 0xA9A9A9), ("DarkGreen", 0x006400), ("DarkKhaki", 0xBDB76B), ("DarkMagenta", 0x8B008B), ("DarkOliveGreen", 0x556B2F),
    ("DarkOrange", 0xFF8C00), ("DarkOrchid", 0x9932CC), ("DarkRed", 0x8B0000), ("DarkSalmon", 0xE9967A), ("DarkSeaGreen", 0x8FBC8B),
    ("DarkSlateBlue", 0x483D8B), ("DarkSlateGray", 0x2F4F4F), ("DarkTurquoise", 0x00CED1), ("DarkViolet", 0x9400D3), ("DeepPink", 0xFF1493),
    ("DeepSkyBlue", 0x00BFFF), ("DimGray", 0x696969), ("DodgerBlue", 0x1E90FF), ("Firebrick", 0xB22222), ("FloralWhite", 0xFFFAF0),
    ("ForestGreen", 0x228B22), ("Fuchsia", 0xFF00FF), ("Gainsboro", 0xDCDCDC), ("GhostWhite", 0xF8F8FF), ("Gold", 0xFFD700),
    ("Goldenrod", 0xDAA520), ("Gray", 0x808080), ("Green", 0x008000), ("GreenYellow", 0xADFF2F), ("Honeydew", 0xF0FFF0),
    ("HotPink", 0xFF69B4), ("IndianRed", 0xCD5C5C), ("Indigo", 0x4B0082), ("Ivory", 0xFFFFF0), ("Khaki", 0xF0E68C),
    ("Lavender", 0xE6E6FA), ("LavenderBlush", 0xFFF0F5), ("LawnGreen", 0x7CFC00), ("LemonChiffon", 0xFFFACD), ("LightBlue", 0xADD8E6),
    ("LightCoral", 0xF08080), ("LightCyan", 0xE0FFFF), ("LightGoldenrodYellow", 0xFAFAD2), ("LightGray", 0xD3D3D3), ("LightGreen", 0x90EE90),
    ("LightPink", 0xFFB6C1), ("LightSalmon", 0xFFA07A), ("LightSeaGreen", 0x20B2AA), ("LightSkyBlue", 0x87CEFA), ("LightSlateGray", 0x778899),
    ("LightSteelBlue", 0xB0C4DE), ("LightYellow", 0xFFFFE0), ("Lime", 0x00FF00), ("LimeGreen", 0x32CD32), ("Linen", 0xFAF0E6),
    ("Magenta", 0xFF00FF), ("Maroon", 0x800000), ("MediumAquamarine", 0x66CDAA), ("MediumBlue", 0x0000CD), ("MediumOrchid", 0xBA55D3),
    ("MediumPurple", 0x9370DB), ("MediumSeaGreen", 0x3CB371), ("MediumSlateBlue", 0x7B68EE), ("MediumSpringGreen", 0x00FA9A), ("MediumTurquoise", 0x48D1CC),
    ("MediumVioletRed", 0xC71585), ("MidnightBlue", 0x191970), ("MintCream", 0xF5FFFA), ("MistyRose", 0xFFE4E1), ("Moccasin", 0xFFE4B5),
    ("NavajoWhite", 0xFFDEAD), ("Navy", 0x000080), ("OldLace", 0xFDF5E6), ("Olive", 0x808000), ("OliveDrab", 0x6B8E23),
    ("Orange", 0xFFA500), ("OrangeRed", 0xFF4500), ("Orchid", 0xDA70D6), ("PaleGoldenrod", 0xEEE8AA), ("PaleGreen", 0x98FB98),
    ("PaleTurquoise", 0xAFEEEE), ("PaleVioletRed", 0xDB7093), ("PapayaWhip", 0xFFEFD5), ("PeachPuff", 0xFFDAB9), ("Peru", 0xCD853F),
    ("Pink", 0xFFC0CB), ("Plum", 0xDDA0DD), ("PowderBlue", 0xB0E0E6), ("Purple", 0x800080), ("Red", 0xFF0000),
    ("RosyBrown", 0xBC8F8F), ("RoyalBlue", 0x4169E1), ("SaddleBrown", 0x8B4513), ("Salmon", 0xFA8072), ("SandyBrown", 0xF4A460),
    ("SeaGreen", 0x2E8B57), ("SeaShell", 0xFFF5EE), ("Sienna", 0xA0522D), ("Silver", 0xC0C0C0), ("SkyBlue", 0x87CEEB),
    ("SlateBlue", 0x6A5ACD), ("SlateGray", 0x708090), ("Snow", 0xFFFAFA), ("SpringGreen", 0x00FF7F), ("SteelBlue", 0x4682B4),
    ("Tan", 0xD2B48C), ("Teal", 0x008080), ("Thistle", 0xD8BFD8), ("Tomato", 0xFF6347), ("Turquoise", 0x40E0D0),
    ("Violet", 0xEE82EE), ("Wheat", 0xF5DEB3), ("White", 0xFFFFFF), ("WhiteSmoke", 0xF5F5F5), ("Yellow", 0xFFFF00),
    ("YellowGreen", 0x9ACD32),
];

/// The typical value of each Windows system colour (Windows 11 light, default settings) — what the
/// contrast check assumes, since the real ones depend on the machine.
const SYSTEM_DEFAULTS: &[(&str, u32)] = &[
    ("ActiveBorder", 0xB4B4B4), ("ActiveCaption", 0x99B4D1), ("ActiveCaptionText", 0x000000), ("AppWorkspace", 0xABABAB),
    ("ButtonFace", 0xF0F0F0), ("ButtonHighlight", 0xFFFFFF), ("ButtonShadow", 0xA0A0A0), ("Control", 0xF0F0F0),
    ("ControlDark", 0xA0A0A0), ("ControlDarkDark", 0x696969), ("ControlLight", 0xE3E3E3), ("ControlLightLight", 0xFFFFFF),
    ("ControlText", 0x000000), ("Desktop", 0x000000), ("GradientActiveCaption", 0xB9D1EA), ("GradientInactiveCaption", 0xD7E4F2),
    ("GrayText", 0x6D6D6D), ("Highlight", 0x0078D7), ("HighlightText", 0xFFFFFF), ("HotTrack", 0x0066CC),
    ("InactiveBorder", 0xF4F7FC), ("InactiveCaption", 0xBFCDDB), ("InactiveCaptionText", 0x000000), ("Info", 0xFFFFE1),
    ("InfoText", 0x000000), ("Menu", 0xF0F0F0), ("MenuBar", 0xF0F0F0), ("MenuHighlight", 0x3399FF), ("MenuText", 0x000000),
    ("ScrollBar", 0xC8C8C8), ("Window", 0xFFFFFF), ("WindowFrame", 0x646464), ("WindowText", 0x000000),
];

/// A parsed colour attribute.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ColorValue {
    Token(&'static str),
    Rgba(Rgba),
    /// A web colour, by its canonical name.
    Web(&'static str, Rgba),
    /// A Windows system colour, by its canonical name.
    System(&'static str),
}

impl ColorValue {
    /// A free colour: it does not follow the theme (the contrast check applies to it).
    pub fn is_free(&self) -> bool {
        !matches!(self, ColorValue::Token(_))
    }

    /// Its value painted on `c`: a token from the canvas' theme (the system colour it maps to under
    /// high contrast), a system colour from Windows.
    pub fn resolve(&self, c: &dyn ControlCanvas) -> D2D1_COLOR_F {
        self.resolve_with(c.theme(), high_contrast())
    }

    /// Its value in `theme` (`high_contrast`: tokens take their system colour).
    pub fn resolve_with(&self, theme: &Theme, high_contrast: bool) -> D2D1_COLOR_F {
        match self {
            ColorValue::Token(name) => match theme_token(name) {
                Some(t) if high_contrast => system_rgba(t.high_contrast).unwrap_or_else(|| (t.get)(theme)),
                Some(t) => (t.get)(theme),
                None => theme.text_primary,
            },
            ColorValue::Rgba(c) | ColorValue::Web(_, c) => c.d2d(),
            ColorValue::System(name) => system_rgba(name).unwrap_or(theme.text_primary),
        }
    }

    /// The attribute text that parses back to it ([`parse_color`]): the token, web or system colour
    /// name, else `#RRGGBB(AA)`.
    pub fn to_attribute(&self) -> String {
        match self {
            ColorValue::Token(name) | ColorValue::Web(name, _) | ColorValue::System(name) => (*name).to_string(),
            ColorValue::Rgba(c) => c.hex(),
        }
    }

    /// Its value in the light theme, as a drawing colour (a system colour as Windows has it now).
    pub fn to_color(&self) -> kubuno_ui::graphics::Color {
        kubuno_ui::graphics::Color::from(self.resolve_with(&light_theme(), false))
    }

    /// Its value for the contrast check in a theme: the typical value of a system colour.
    pub fn analysis_value(&self, theme: &Theme) -> Rgba {
        match self {
            ColorValue::System(name) => system_default(name).unwrap_or(Rgba::rgb(0, 0, 0)),
            other => Rgba::from_d2d(other.resolve_with(theme, false)),
        }
    }
}

fn system_rgba(name: &str) -> Option<D2D1_COLOR_F> {
    kubuno_controls::styled::system_color(name)
}

fn system_default(name: &str) -> Option<Rgba> {
    SYSTEM_DEFAULTS.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, v)| rgb_u32(*v))
}

fn rgb_u32(v: u32) -> Rgba {
    Rgba::rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

/// Whether Windows' high contrast is on (read once per second at most: it is asked every frame).
pub fn high_contrast() -> bool {
    thread_local!(static CACHE: std::cell::Cell<(u64, bool)> = const { std::cell::Cell::new((u64::MAX, false)) });
    let now = kubuno_controls::host::now_ms() / 1000;
    CACHE.with(|c| {
        let (at, value) = c.get();
        if at == now {
            return value;
        }
        let value = kubuno_controls::styled::high_contrast();
        c.set((now, value));
        value
    })
}

/// Parses a colour attribute (see the module doc). `Ok(None)` for an empty value (the ambient
/// colour); `Err` explains the expected forms.
pub fn parse_color(text: &str) -> Result<Option<ColorValue>, String> {
    let t = text.trim();
    if t.is_empty() {
        return Ok(None);
    }
    if let Some(hex) = t.strip_prefix('#') {
        return parse_hex(hex).map(|c| Some(ColorValue::Rgba(c))).ok_or_else(|| format!("`{t}` is not a colour: write #RRGGBB or #RRGGBBAA"));
    }
    if let Some(token) = theme_token(t) {
        return Ok(Some(ColorValue::Token(token.name)));
    }
    if let Some((name, v)) = WEB_COLORS.iter().find(|(n, _)| n.eq_ignore_ascii_case(t)) {
        let c = if *name == "Transparent" { Rgba(255, 255, 255, 0) } else { rgb_u32(*v) };
        return Ok(Some(ColorValue::Web(name, c)));
    }
    if let Some((name, _)) = kubuno_controls::styled::SYSTEM_COLORS.iter().find(|(n, _)| n.eq_ignore_ascii_case(t)) {
        return Ok(Some(ColorValue::System(name)));
    }
    Err(format!("`{t}` is not a colour: write a theme colour such as Primary or Surface, #RRGGBB, a web colour name or a system colour name"))
}

fn parse_hex(hex: &str) -> Option<Rgba> {
    if !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let n = |s: &str| u8::from_str_radix(s, 16).ok();
    match hex.len() {
        3 => {
            let d: Vec<u8> = hex.chars().map(|c| c.to_digit(16).map(|v| (v * 17) as u8)).collect::<Option<_>>()?;
            Some(Rgba::rgb(d[0], d[1], d[2]))
        }
        6 => Some(Rgba::rgb(n(&hex[0..2])?, n(&hex[2..4])?, n(&hex[4..6])?)),
        8 => Some(Rgba(n(&hex[0..2])?, n(&hex[2..4])?, n(&hex[4..6])?, n(&hex[6..8])?)),
        _ => None,
    }
}

// ── Contrast (WCAG 2) ─────────────────────────────────────────────────────────

/// The relative luminance of an opaque colour (WCAG 2).
pub fn luminance(c: Rgba) -> f64 {
    let lin = |v: u8| {
        let s = f64::from(v) / 255.0;
        if s <= 0.039_28 {
            s / 12.92
        } else {
            ((s + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * lin(c.0) + 0.7152 * lin(c.1) + 0.0722 * lin(c.2)
}

/// `top` painted over the opaque `under` (its alpha composited).
pub fn over(top: Rgba, under: Rgba) -> Rgba {
    let a = f64::from(top.3) / 255.0;
    let mix = |t: u8, u: u8| (f64::from(t) * a + f64::from(u) * (1.0 - a)).round() as u8;
    Rgba::rgb(mix(top.0, under.0), mix(top.1, under.1), mix(top.2, under.2))
}

/// The WCAG contrast ratio of `fg` over `bg` (1 to 21); translucent colours are composited first
/// (`bg` over the theme's window background, `fg` over the result).
pub fn contrast_ratio(fg: Rgba, bg: Rgba) -> f64 {
    let (a, b) = (luminance(fg), luminance(bg));
    let (hi, lo) = if a > b { (a, b) } else { (b, a) };
    (hi + 0.05) / (lo + 0.05)
}

/// The minimum ratio WCAG AA asks of text: 4.5, or 3 for large text (24 px, or 18.66 px bold).
pub fn required_ratio(size_px: f32, bold: bool) -> f64 {
    if size_px >= 24.0 || (bold && size_px >= 18.66) {
        3.0
    } else {
        4.5
    }
}

/// One theme a contrast finding concerns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeKind {
    Light,
    Dark,
}

impl ThemeKind {
    pub fn name(self) -> &'static str {
        match self {
            ThemeKind::Light => "light",
            ThemeKind::Dark => "dark",
        }
    }

    fn theme(self) -> Theme {
        match self {
            ThemeKind::Light => light_theme(),
            ThemeKind::Dark => dark_theme(),
        }
    }
}

/// A text colour that does not contrast enough with its background in one theme.
#[derive(Debug, Clone, PartialEq)]
pub struct ContrastFinding {
    pub theme: ThemeKind,
    pub ratio: f64,
    pub required: f64,
    pub foreground: Rgba,
    pub background: Rgba,
}

/// Checks the text colour `fore` (ambient: the theme's text) over `back` (ambient: the theme's
/// window background) in the light and dark themes, for text of `size_px` (bold or not). Only
/// when at least one of them is a free colour: tokens are designed to contrast with each other.
pub fn contrast_warnings(fore: Option<&ColorValue>, back: Option<&ColorValue>, size_px: f32, bold: bool) -> Vec<ContrastFinding> {
    if !fore.is_some_and(ColorValue::is_free) && !back.is_some_and(ColorValue::is_free) {
        return Vec::new();
    }
    let required = required_ratio(size_px, bold);
    let mut out = Vec::new();
    for kind in [ThemeKind::Light, ThemeKind::Dark] {
        let theme = kind.theme();
        let ground = Rgba::from_d2d(theme.window_background);
        let bg = over(back.map(|b| b.analysis_value(&theme)).unwrap_or(ground), ground);
        let fg = over(fore.map(|f| f.analysis_value(&theme)).unwrap_or_else(|| Rgba::from_d2d(theme.text_primary)), bg);
        let ratio = contrast_ratio(fg, bg);
        if ratio + 1e-9 < required {
            out.push(ContrastFinding { theme: kind, ratio, required, foreground: fg, background: bg });
        }
    }
    out
}

// ── Fonts ─────────────────────────────────────────────────────────────────────

/// A parsed `Font` attribute — the WinForms text form `"Segoe UI, 12pt, style=Bold, Italic"`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FontSpec {
    pub family: Option<String>,
    /// In DIP (`12pt` = 16 DIP).
    pub size_px: Option<f32>,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strikeout: bool,
}

/// The size of the body text every font size is relative to (the shared `body` format).
pub const BODY_SIZE_PX: f32 = 14.0;

/// The ambient font as the designer shows it.
pub const DEFAULT_FONT: &str = "Segoe UI Variable Text, 10.5pt";

impl FontSpec {
    /// The restyling the text formats get (the size as a factor of the body size).
    pub fn text_style(&self, right_to_left: bool) -> drive_app_controls::TextStyle {
        drive_app_controls::TextStyle {
            family: self.family.clone(),
            scale: self.size_px.map(|s| s / BODY_SIZE_PX).unwrap_or(1.0),
            bold: self.bold,
            italic: self.italic,
            right_to_left,
        }
    }

    /// The WinForms text form.
    pub fn to_text(&self) -> String {
        let mut parts = vec![self.family.clone().unwrap_or_else(|| "Segoe UI Variable Text".to_string())];
        let pt = self.size_px.unwrap_or(BODY_SIZE_PX) * 72.0 / 96.0;
        parts.push(format!("{}pt", trim_number(pt)));
        let styles: Vec<&str> = [(self.bold, "Bold"), (self.italic, "Italic"), (self.underline, "Underline"), (self.strikeout, "Strikeout")]
            .into_iter()
            .filter_map(|(on, name)| on.then_some(name))
            .collect();
        if !styles.is_empty() {
            parts.push(format!("style={}", styles.join(", ")));
        }
        parts.join(", ")
    }
}

fn trim_number(v: f32) -> String {
    let s = format!("{v:.2}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// Parses a `Font` attribute. `Ok(None)` for an empty value (the ambient font).
pub fn parse_font(text: &str) -> Result<Option<FontSpec>, String> {
    let t = text.trim();
    if t.is_empty() {
        return Ok(None);
    }
    let mut spec = FontSpec::default();
    let mut in_style = false;
    for (i, part) in t.split(',').map(str::trim).enumerate() {
        if part.is_empty() {
            continue;
        }
        let lower = part.to_ascii_lowercase();
        let style_word = if let Some(rest) = lower.strip_prefix("style=") {
            in_style = true;
            Some(rest.trim().to_string())
        } else if in_style {
            Some(lower.clone())
        } else {
            None
        };
        if let Some(word) = style_word {
            match word.as_str() {
                "bold" => spec.bold = true,
                "italic" => spec.italic = true,
                "underline" => spec.underline = true,
                "strikeout" => spec.strikeout = true,
                "regular" => {}
                other => return Err(format!("`{other}` is not a font style: use Bold, Italic, Underline or Strikeout")),
            }
            continue;
        }
        let size = |unit: &str, factor: f32| lower.strip_suffix(unit).and_then(|n| n.trim().parse::<f32>().ok()).map(|n| n * factor);
        if let Some(px) = size("pt", 96.0 / 72.0).or_else(|| size("px", 1.0)) {
            if !(px.is_finite() && px > 0.0 && px < 1000.0) {
                return Err(format!("`{part}` is not a font size"));
            }
            spec.size_px = Some(px);
            continue;
        }
        if i == 0 {
            spec.family = Some(part.to_string());
            continue;
        }
        return Err(format!("`{part}` is not part of a font: write a family, a size in pt, then style=Bold, Italic…"));
    }
    Ok(Some(spec))
}

// ── Boxes and sizes ───────────────────────────────────────────────────────────

/// Parses a `Margin`/`Padding`: `"left, top, right, bottom"`, or one number for all four sides.
pub fn parse_padding(text: &str) -> Result<kubuno_ui::Padding, String> {
    let t = text.trim();
    if t.is_empty() {
        return Ok(kubuno_ui::Padding::ZERO);
    }
    let values: Vec<f32> = t
        .split(',')
        .map(|p| p.trim().parse::<f32>().map_err(|_| format!("`{t}` is not a spacing: write left, top, right, bottom in pixels (or one number)")))
        .collect::<Result<_, _>>()?;
    match values.as_slice() {
        [a] => Ok(kubuno_ui::Padding::all(*a)),
        [l, t, r, b] => Ok(kubuno_ui::Padding { left: *l, top: *t, right: *r, bottom: *b }),
        _ => Err(format!("`{t}` is not a spacing: write left, top, right, bottom in pixels (or one number)")),
    }
}

/// Parses a `MinimumSize`/`MaximumSize`: `"width, height"` (`0` = no limit on that axis).
pub fn parse_size(text: &str) -> Result<(f32, f32), String> {
    let t = text.trim();
    if t.is_empty() {
        return Ok((0.0, 0.0));
    }
    let values: Vec<f32> = t
        .split(',')
        .map(|p| p.trim().parse::<f32>().map_err(|_| format!("`{t}` is not a size: write width, height in pixels")))
        .collect::<Result<_, _>>()?;
    match values.as_slice() {
        [w, h] if *w >= 0.0 && *h >= 0.0 => Ok((*w, *h)),
        _ => Err(format!("`{t}` is not a size: write width, height in pixels")),
    }
}

/// The host cursor of a `Cursor` value (`Default` → `None`: the control's own).
pub fn cursor(name: &str) -> Option<kubuno_controls::host::Cursor> {
    use kubuno_controls::host::Cursor;
    Some(match name.trim() {
        "Arrow" => Cursor::Arrow,
        "IBeam" => Cursor::IBeam,
        "Hand" => Cursor::Hand,
        "Wait" | "AppStarting" => Cursor::Wait,
        "No" => Cursor::NotAllowed,
        "SizeAll" | "Cross" => Cursor::Move,
        "SizeNS" | "UpArrow" => Cursor::ResizeNS,
        "SizeWE" => Cursor::ResizeEW,
        "SizeNWSE" => Cursor::ResizeNWSE,
        "SizeNESW" => Cursor::ResizeNESW,
        "Help" => Cursor::Arrow,
        _ => return None,
    })
}

/// The `Cursor` values, in the order the designer lists them.
pub const CURSORS: &[&str] =
    &["Default", "Arrow", "IBeam", "Hand", "Wait", "No", "SizeAll", "SizeNS", "SizeWE", "SizeNWSE", "SizeNESW", "Cross", "Help", "AppStarting", "UpArrow"];

/// The theme tokens as the JSON the Visual Studio colour editor's table is checked against
/// (`tests/.../Fixtures/theme-tokens.json`).
pub fn theme_tokens_json() -> serde_json::Value {
    serde_json::Value::Array(
        THEME_TOKENS
            .iter()
            .map(|t| {
                serde_json::json!({
                    "name": t.name,
                    "light": t.light().hex(),
                    "dark": t.dark().hex(),
                    "high_contrast": t.high_contrast,
                    "doc": t.doc,
                    "doc_fr": t.doc_fr,
                })
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_parse_in_every_form() {
        assert_eq!(parse_color(""), Ok(None));
        assert_eq!(parse_color("Primary"), Ok(Some(ColorValue::Token("Primary"))));
        assert_eq!(parse_color("#1A73E8"), Ok(Some(ColorValue::Rgba(Rgba::rgb(0x1A, 0x73, 0xE8)))));
        assert_eq!(parse_color("#fff"), Ok(Some(ColorValue::Rgba(Rgba::rgb(255, 255, 255)))));
        assert_eq!(parse_color("#11223380"), Ok(Some(ColorValue::Rgba(Rgba(0x11, 0x22, 0x33, 0x80)))));
        assert_eq!(parse_color("cornflowerblue"), Ok(Some(ColorValue::Web("CornflowerBlue", Rgba::rgb(0x64, 0x95, 0xED)))));
        assert_eq!(parse_color("ControlText"), Ok(Some(ColorValue::System("ControlText"))));
        assert!(parse_color("#12").is_err());
        assert!(parse_color("Blurple").is_err());
        assert!(!ColorValue::Token("Primary").is_free() && ColorValue::System("Window").is_free());
    }

    #[test]
    fn tokens_follow_the_theme_and_have_their_high_contrast_colour() {
        let primary = theme_token("Primary").unwrap();
        assert_eq!(primary.light().hex(), "#1A73E8");
        assert_eq!(primary.dark().hex(), "#8AB4F8");
        assert_eq!(theme_token("Divider").unwrap().dark().hex(), "#5F63688C");
        assert_eq!(theme_token("TooltipBackground").unwrap().light().hex(), "#3C4043F2");
        for t in THEME_TOKENS {
            assert!(kubuno_controls::styled::SYSTEM_COLORS.iter().any(|(n, _)| *n == t.high_contrast), "{}", t.name);
            assert!(parse_color(t.name) == Ok(Some(ColorValue::Token(t.name))));
        }
        let value = ColorValue::Token("Primary");
        assert_eq!(Rgba::from_d2d(value.resolve_with(&Theme::dark(), false)).hex(), "#8AB4F8");
    }

    #[test]
    fn contrast_ratios_are_the_wcag_ones() {
        let black = Rgba::rgb(0, 0, 0);
        let white = Rgba::rgb(255, 255, 255);
        assert!((contrast_ratio(black, white) - 21.0).abs() < 1e-6);
        assert!((contrast_ratio(Rgba::rgb(0x77, 0x77, 0x77), white) - 4.48).abs() < 0.01);
        assert_eq!(over(Rgba(0, 0, 0, 128), white), Rgba::rgb(127, 127, 127));
        assert_eq!(required_ratio(14.0, false), 4.5);
        assert_eq!(required_ratio(24.0, false), 3.0);
        assert_eq!(required_ratio(19.0, true), 3.0);
    }

    #[test]
    fn a_free_colour_that_breaks_contrast_in_one_theme_is_reported_for_that_theme() {
        // Dark grey text: fine on the light background, unreadable on the dark one.
        let fore = parse_color("#333333").unwrap();
        let findings = contrast_warnings(fore.as_ref(), None, 14.0, false);
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert_eq!(findings[0].theme, ThemeKind::Dark);
        // Tokens only: no warning.
        let token = parse_color("TextSecondary").unwrap();
        assert!(contrast_warnings(token.as_ref(), None, 14.0, false).is_empty());
        // Black on white everywhere: fine.
        let (f, b) = (parse_color("Black").unwrap(), parse_color("White").unwrap());
        assert!(contrast_warnings(f.as_ref(), b.as_ref(), 14.0, false).is_empty());
        // A pale free background under the theme's text: fine in light, broken in dark.
        let back = parse_color("#FFFFCC").unwrap();
        let findings = contrast_warnings(None, back.as_ref(), 14.0, false);
        assert_eq!(findings.iter().map(|f| f.theme).collect::<Vec<_>>(), vec![ThemeKind::Dark]);
    }

    #[test]
    fn fonts_round_trip_the_winforms_text() {
        let f = parse_font("Segoe UI, 12pt, style=Bold, Italic").unwrap().unwrap();
        assert_eq!(f.family.as_deref(), Some("Segoe UI"));
        assert_eq!(f.size_px, Some(16.0));
        assert!(f.bold && f.italic && !f.underline);
        assert_eq!(f.to_text(), "Segoe UI, 12pt, style=Bold, Italic");
        assert_eq!(parse_font("Consolas, 20px").unwrap().unwrap().size_px, Some(20.0));
        assert_eq!(parse_font("").unwrap(), None);
        assert!(parse_font("Segoe UI, 12pt, style=Wobbly").is_err());
        assert!(parse_font("Segoe UI, huge").is_err());
        let style = f.text_style(true);
        assert!((style.scale - 16.0 / 14.0).abs() < 1e-6 && style.bold && style.right_to_left);
        assert_eq!(parse_font(DEFAULT_FONT).unwrap().unwrap().size_px, Some(14.0));
    }

    #[test]
    fn boxes_and_sizes_parse_like_winforms() {
        assert_eq!(parse_padding("3"), Ok(kubuno_ui::Padding::all(3.0)));
        assert_eq!(parse_padding("1, 2, 3, 4"), Ok(kubuno_ui::Padding { left: 1.0, top: 2.0, right: 3.0, bottom: 4.0 }));
        assert!(parse_padding("1, 2").is_err());
        assert_eq!(parse_size("100, 30"), Ok((100.0, 30.0)));
        assert_eq!(parse_size(""), Ok((0.0, 0.0)));
        assert!(parse_size("100").is_err());
        assert!(cursor("Hand").is_some() && cursor("Default").is_none());
        assert!(CURSORS.iter().all(|c| *c == "Default" || cursor(c).is_some()));
    }

    /// Regenerates the Visual Studio colour editor's fixture: `KUBUNO_THEME_TOKENS_FIXTURE=<path> cargo
    /// test -p kubuno-views --lib write_theme_tokens_fixture -- --ignored`.
    #[test]
    #[ignore]
    fn write_theme_tokens_fixture() {
        let Some(path) = std::env::var_os("KUBUNO_THEME_TOKENS_FIXTURE") else { return };
        let json = serde_json::to_string_pretty(&theme_tokens_json()).expect("the tokens serialize");
        std::fs::write(path, json + "\n").expect("the fixture is written");
    }
}
