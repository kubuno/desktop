//! Icon sources: what an icon value names besides a glyph of the embedded vector set.
//!
//! Every control paints its icon through `Canvas::vector_icon(name, …)`, whose `name` is a
//! `&'static str`. Besides a glyph of the embedded set (`"Save"`), that name may be:
//!
//! * an image file — `C:\app\images\save.svg`, `images/logo.png` (SVG, PNG, JPEG, BMP, GIF, ICO,
//!   TIFF, WebP): the painters that know images (`kubuno_controls`' host painter) draw it in place
//!   of a glyph, at the size asked for;
//! * either of those followed by drawing options, after [`OPTIONS_SEPARATOR`]:
//!   `Save\u{1}tint=Accent;size=24x24;scaling=Fit;mirror` — an explicit tint, a size that
//!   overrides the control's, how a non-square image fills its box, and a horizontal flip (a
//!   right-to-left layout). [`compose`] writes that form, [`parse`] reads it back.
//!
//! [`intern`] turns such a runtime string into the `&'static str` the painters take (each distinct
//! value is kept once for the life of the process). Nothing here touches a file or a device: this
//! module only describes, the painters decode and draw.

use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};

/// Separates an icon source from its drawing options (a control character no file name or glyph
/// name contains).
pub const OPTIONS_SEPARATOR: char = '\u{1}';

/// The image file extensions an icon may name (lower case, without the dot).
pub const IMAGE_EXTENSIONS: &[&str] = &["svg", "png", "jpg", "jpeg", "bmp", "gif", "ico", "tif", "tiff", "webp"];

/// How an image that is not square fills the icon's box (`IconScaling`, WinForms'
/// `ToolStripItem.ImageScaling` / `PictureBoxSizeMode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum IconScaling {
    /// The whole image, as large as fits, its proportions kept (`Uniform`, `Zoom`).
    #[default]
    Fit,
    /// The whole box covered, its proportions kept, the overflow clipped (`UniformToFill`).
    Fill,
    /// The box exactly, its proportions not kept.
    Stretch,
    /// The image's own size (in DIP), centred, clipped to the box.
    None,
}

impl IconScaling {
    /// The value of an `IconScaling` attribute (`"Fit"`, `"Fill"`, `"Stretch"`, `"None"`; WinForms'
    /// `Zoom`/`StretchImage`/`CenterImage` and XAML's `Uniform`/`UniformToFill` are read too).
    pub fn parse(text: &str) -> Option<Self> {
        Some(match text.trim() {
            "Fit" | "Uniform" | "Zoom" | "SizeToFit" => IconScaling::Fit,
            "Fill" | "UniformToFill" => IconScaling::Fill,
            "Stretch" | "StretchImage" => IconScaling::Stretch,
            "None" | "CenterImage" | "Normal" => IconScaling::None,
            _ => return None,
        })
    }

    pub const fn name(self) -> &'static str {
        match self {
            IconScaling::Fit => "Fit",
            IconScaling::Fill => "Fill",
            IconScaling::Stretch => "Stretch",
            IconScaling::None => "None",
        }
    }

    /// Where an image of `image` (width, height) goes in a box of `bounds` (width, height), as
    /// `(x, y, width, height)` relative to the box's top-left corner — it may overflow the box
    /// ([`IconScaling::Fill`], [`IconScaling::None`]); the caller clips. Pure.
    pub fn place(self, image: (f32, f32), bounds: (f32, f32)) -> (f32, f32, f32, f32) {
        let (iw, ih) = (image.0.max(0.01), image.1.max(0.01));
        let (bw, bh) = (bounds.0.max(0.0), bounds.1.max(0.0));
        let (w, h) = match self {
            IconScaling::Stretch => (bw, bh),
            IconScaling::None => (iw, ih),
            IconScaling::Fit | IconScaling::Fill => {
                let (sx, sy) = (bw / iw, bh / ih);
                let k = if self == IconScaling::Fit { sx.min(sy) } else { sx.max(sy) };
                (iw * k, ih * k)
            }
        };
        ((bw - w) / 2.0, (bh - h) / 2.0, w, h)
    }
}

/// An icon value taken apart: its source and its drawing options.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct IconSpec<'a> {
    /// A glyph name of the embedded set, or an image file.
    pub source: &'a str,
    /// An explicit tint (`#rrggbb`, `#rrggbbaa`, or a theme colour name the painter resolves):
    /// every pixel of the icon takes that colour, its transparency kept.
    pub tint: Option<&'a str>,
    /// A size in DIP (width, height) that replaces the one the control asks for.
    pub size: Option<(f32, f32)>,
    pub scaling: IconScaling,
    /// Flipped horizontally (a right-to-left layout).
    pub mirror: bool,
}

impl IconSpec<'_> {
    /// The source names an image file rather than a glyph.
    pub fn is_image(&self) -> bool {
        is_image_path(self.source)
    }

    /// Any drawing option is set.
    pub fn has_options(&self) -> bool {
        self.tint.is_some() || self.size.is_some() || self.scaling != IconScaling::Fit || self.mirror
    }
}

/// Reads `value` (a source, optionally followed by [`OPTIONS_SEPARATOR`] and its options). An
/// unknown or malformed option is ignored. Pure.
pub fn parse(value: &str) -> IconSpec<'_> {
    let Some((source, options)) = value.split_once(OPTIONS_SEPARATOR) else {
        return IconSpec { source: value, ..IconSpec::default() };
    };
    let mut spec = IconSpec { source, ..IconSpec::default() };
    for option in options.split(';').map(str::trim).filter(|o| !o.is_empty()) {
        let (key, val) = option.split_once('=').map_or((option, ""), |(k, v)| (k.trim(), v.trim()));
        match key {
            "tint" if !val.is_empty() => spec.tint = Some(val),
            "size" => spec.size = parse_size(val),
            "scaling" => spec.scaling = IconScaling::parse(val).unwrap_or_default(),
            "mirror" => spec.mirror = true,
            _ => {}
        }
    }
    spec
}

/// `24`, `24x16` or `24, 16` → (width, height) in DIP; `None` for anything else or a size ≤ 0.
fn parse_size(text: &str) -> Option<(f32, f32)> {
    let mut parts = text.split(['x', ',']).map(str::trim);
    let w: f32 = parts.next()?.parse().ok()?;
    let h: f32 = match parts.next() {
        Some(h) => h.parse().ok()?,
        None => w,
    };
    (parts.next().is_none() && w > 0.0 && h > 0.0 && w.is_finite() && h.is_finite()).then_some((w, h))
}

/// The value [`parse`] reads back as `spec` (just its source when it has no option).
pub fn compose(spec: &IconSpec<'_>) -> String {
    if !spec.has_options() {
        return spec.source.to_string();
    }
    let mut options = Vec::new();
    if let Some(t) = spec.tint {
        options.push(format!("tint={t}"));
    }
    if let Some((w, h)) = spec.size {
        options.push(format!("size={w}x{h}"));
    }
    if spec.scaling != IconScaling::Fit {
        options.push(format!("scaling={}", spec.scaling.name()));
    }
    if spec.mirror {
        options.push("mirror".to_string());
    }
    format!("{}{OPTIONS_SEPARATOR}{}", spec.source, options.join(";"))
}

/// The prefix of an image held by a resource file of the project (`kbres:<set>/<name>`, what a
/// `{Res key}` value becomes at run time): its bytes come from the resources runtime.
pub const RESOURCE_PREFIX: &str = "kbres:";

/// Whether `source` names an image: a file by its extension (case-insensitive), or a resource
/// (`kbres:…`).
pub fn is_image_path(source: &str) -> bool {
    is_resource(source) || image_extension(source).is_some()
}

/// Whether `source` is an image of a resource file (`kbres:…`).
pub fn is_resource(source: &str) -> bool {
    source.starts_with(RESOURCE_PREFIX)
}

/// The lower-case extension of `source` when it is one of [`IMAGE_EXTENSIONS`].
pub fn image_extension(source: &str) -> Option<&'static str> {
    let file = source.rsplit(['/', '\\']).next()?;
    let (_, ext) = file.rsplit_once('.')?;
    IMAGE_EXTENSIONS.iter().copied().find(|e| e.eq_ignore_ascii_case(ext))
}

/// `#rrggbb` or `#rrggbbaa` → `(r, g, b, a)` in 0..=1; `None` for anything else.
pub fn parse_hex_color(text: &str) -> Option<[f32; 4]> {
    let hex = text.trim().strip_prefix('#')?;
    if !(hex.len() == 6 || hex.len() == 8) || !hex.is_ascii() {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok().map(|b| f32::from(b) / 255.0);
    Some([byte(0)?, byte(2)?, byte(4)?, if hex.len() == 8 { byte(6)? } else { 1.0 }])
}

/// `value` as a `&'static str`: the same text, kept once for the life of the process (the painters
/// take icon names as `&'static str`). Each distinct value is stored once.
pub fn intern(value: &str) -> &'static str {
    static INTERNED: OnceLock<Mutex<HashSet<&'static str>>> = OnceLock::new();
    let set = INTERNED.get_or_init(|| Mutex::new(HashSet::new()));
    let mut set = match set.lock() {
        Ok(guard) => guard,
        // A panic while holding the lock leaves the set intact (inserts are atomic).
        Err(poisoned) => poisoned.into_inner(),
    };
    if let Some(found) = set.get(value) {
        return found;
    }
    let leaked: &'static str = Box::leak(value.to_string().into_boxed_str());
    set.insert(leaked);
    leaked
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_source_has_no_option() {
        let s = parse("Save");
        assert_eq!(s.source, "Save");
        assert!(!s.has_options());
        assert_eq!(compose(&s), "Save");
    }

    #[test]
    fn options_round_trip() {
        let spec = IconSpec { source: "C:/a/b.svg", tint: Some("Accent"), size: Some((24.0, 16.0)), scaling: IconScaling::Fill, mirror: true };
        let text = compose(&spec);
        assert!(text.starts_with("C:/a/b.svg\u{1}"));
        assert_eq!(parse(&text), spec);
    }

    #[test]
    fn malformed_options_are_ignored() {
        let s = parse("Save\u{1}size=abc;scaling=Weird;unknown=1;tint=");
        assert_eq!(s, IconSpec { source: "Save", ..IconSpec::default() });
        assert_eq!(parse("Save\u{1}size=20").size, Some((20.0, 20.0)));
        assert_eq!(parse("Save\u{1}size=20, 12").size, Some((20.0, 12.0)));
        assert_eq!(parse("Save\u{1}size=0").size, None);
    }

    #[test]
    fn image_files_are_recognised_by_extension() {
        for f in ["a.svg", "dir/b.PNG", "c:\\x\\c.jpeg", "d.jpg", "e.bmp", "f.gif", "g.ico", "h.tif", "i.tiff", "j.webp"] {
            assert!(is_image_path(f), "{f}");
        }
        for f in ["Save", "ChevronDown", "readme.txt", "noext", "dir.png/file"] {
            assert!(!is_image_path(f), "{f}");
        }
        assert!(is_image_path("kbres:images/logo"));
        assert!(is_resource("kbres:logo") && !is_resource("logo.png"));
        assert_eq!(image_extension("x/Y.SvG"), Some("svg"));
    }

    #[test]
    fn scaling_places_the_image_in_its_box() {
        // A 2:1 image in a square box.
        assert_eq!(IconScaling::Fit.place((40.0, 20.0), (20.0, 20.0)), (0.0, 5.0, 20.0, 10.0));
        assert_eq!(IconScaling::Fill.place((40.0, 20.0), (20.0, 20.0)), (-10.0, 0.0, 40.0, 20.0));
        assert_eq!(IconScaling::Stretch.place((40.0, 20.0), (20.0, 20.0)), (0.0, 0.0, 20.0, 20.0));
        assert_eq!(IconScaling::None.place((8.0, 8.0), (20.0, 20.0)), (6.0, 6.0, 8.0, 8.0));
        assert_eq!(IconScaling::parse("UniformToFill"), Some(IconScaling::Fill));
        assert_eq!(IconScaling::parse("Zoom"), Some(IconScaling::Fit));
        assert_eq!(IconScaling::parse("x"), None);
    }

    #[test]
    fn hex_colours() {
        assert_eq!(parse_hex_color("#ff0000"), Some([1.0, 0.0, 0.0, 1.0]));
        assert_eq!(parse_hex_color("#00000080").map(|c| (c[3] * 255.0).round()), Some(128.0));
        assert_eq!(parse_hex_color("Accent"), None);
        assert_eq!(parse_hex_color("#12345"), None);
    }

    #[test]
    fn interning_keeps_one_copy() {
        let a = intern("some/icon.svg");
        let b = intern(&String::from("some/icon.svg"));
        assert!(std::ptr::eq(a, b));
    }
}
