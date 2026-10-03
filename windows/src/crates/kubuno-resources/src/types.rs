//! The typed values the generated accessors return.

use kubuno_resources_model::Kind;
use std::sync::Arc;

/// Resource bytes: embedded in the program (`'static`) or read at run time (shared).
#[derive(Debug, Clone)]
pub enum Bytes {
    Static(&'static [u8]),
    Shared(Arc<[u8]>),
}

impl std::ops::Deref for Bytes {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        match self {
            Bytes::Static(b) => b,
            Bytes::Shared(b) => b,
        }
    }
}

impl Bytes {
    /// A number naming this content while it is alive (its address and length): what image caches
    /// key a decoded bitmap by, without hashing the bytes every frame.
    pub fn content_id(&self) -> u64 {
        let ptr = self.as_ptr() as u64;
        ptr ^ ((self.len() as u64) << 40) ^ 0x6B62_7265_7300_0000
    }
}

/// A resolved resource value (what a dynamic lookup returns).
#[derive(Debug, Clone)]
pub enum ResolvedValue {
    Text { kind: Kind, text: String },
    Bytes { kind: Kind, format: String, bytes: Bytes },
}

impl ResolvedValue {
    pub fn kind(&self) -> Kind {
        match self {
            ResolvedValue::Text { kind, .. } | ResolvedValue::Bytes { kind, .. } => *kind,
        }
    }

    /// The text of a text value (`String`, `Color`, `Font`), or a text `File`'s bytes as UTF-8.
    pub fn text(&self) -> Option<String> {
        match self {
            ResolvedValue::Text { text, .. } => Some(text.clone()),
            ResolvedValue::Bytes { kind: Kind::File, bytes, .. } => std::str::from_utf8(bytes).ok().map(str::to_string),
            ResolvedValue::Bytes { .. } => None,
        }
    }
}

/// An image resource (`Image` entry): the bytes of an SVG, PNG, JPEG, BMP, GIF, ICO, TIFF or WebP
/// file, in the culture current when it was read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Image {
    set: &'static str,
    name: &'static str,
    bytes: &'static [u8],
    format: &'static str,
}

impl Image {
    #[doc(hidden)]
    pub const fn new(set: &'static str, name: &'static str, bytes: &'static [u8], format: &'static str) -> Self {
        Self { set, name, bytes, format }
    }

    /// The file's bytes.
    pub fn bytes(&self) -> &'static [u8] {
        self.bytes
    }

    /// The format: the file extension, lower case (`png`, `svg`…).
    pub fn format(&self) -> &'static str {
        self.format
    }

    /// The resource name.
    pub fn name(&self) -> &'static str {
        self.name
    }

    /// The URI an image property takes (`Image`, `BackgroundImage`, `Icon`): `kbres:<set>/<name>`.
    /// The picture is looked up when it is painted, in the culture current then.
    pub fn uri(&self) -> String {
        crate::uri(self.set, self.name)
    }

    /// Width and height in pixels, read from the file header (PNG, GIF, BMP, ICO's largest image,
    /// JPEG); `None` for other formats.
    pub fn size(&self) -> Option<(u32, u32)> {
        image_size(self.bytes)
    }
}

/// An icon resource (`Icon` entry): usually a multi-size `.ico` file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Icon(Image);

impl Icon {
    #[doc(hidden)]
    pub const fn new(set: &'static str, name: &'static str, bytes: &'static [u8], format: &'static str) -> Self {
        Self(Image::new(set, name, bytes, format))
    }

    /// The icon as an image (its bytes, format and URI).
    pub fn image(&self) -> Image {
        self.0
    }

    pub fn bytes(&self) -> &'static [u8] {
        self.0.bytes
    }

    pub fn uri(&self) -> String {
        self.0.uri()
    }

    /// The sizes an `.ico` file holds (`[(16, 16), (32, 32), (256, 256)]`); one size for another
    /// format when it can be read.
    pub fn sizes(&self) -> Vec<(u32, u32)> {
        ico_sizes(self.0.bytes).unwrap_or_else(|| self.0.size().into_iter().collect())
    }
}

/// A sound resource (`Audio` entry).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Audio {
    bytes: &'static [u8],
    format: &'static str,
}

impl Audio {
    #[doc(hidden)]
    pub const fn new(bytes: &'static [u8], format: &'static str) -> Self {
        Self { bytes, format }
    }

    pub fn bytes(&self) -> &'static [u8] {
        self.bytes
    }

    pub fn format(&self) -> &'static str {
        self.format
    }

    /// Plays a `.wav` sound asynchronously (Windows' `PlaySound` from memory, like
    /// `System.Media.SoundPlayer`); false for another format or when it cannot be played.
    pub fn play(&self) -> bool {
        if self.format != "wav" || self.bytes.is_empty() {
            return false;
        }
        // Declared by hand (winmm's `PlaySoundW`): the `windows` feature for it would change the
        // `windows` build every UI crate shares.
        #[link(name = "winmm")]
        extern "system" {
            fn PlaySoundW(sound: *const u16, module: *mut core::ffi::c_void, flags: u32) -> i32;
        }
        const SND_ASYNC: u32 = 0x0001;
        const SND_NODEFAULT: u32 = 0x0002;
        const SND_MEMORY: u32 = 0x0004;
        // SAFETY: with SND_MEMORY the "sound name" is a pointer to the whole WAV image, which is
        // 'static, so it outlives the asynchronous playback.
        unsafe { PlaySoundW(self.bytes.as_ptr().cast(), std::ptr::null_mut(), SND_MEMORY | SND_ASYNC | SND_NODEFAULT) != 0 }
    }
}

/// A colour resource (`Color` entry).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    /// Parses `#RRGGBB`, `#AARRGGBB`, `#RGB` or `r, g, b[, a]`; transparent black when invalid.
    pub fn parse(s: &str) -> Self {
        let (r, g, b, a) = kubuno_resources_model::format::parse_color(s).unwrap_or((0, 0, 0, 0));
        Self { r, g, b, a }
    }

    /// `#RRGGBB` (or `#AARRGGBB` when not opaque) — the form view properties take.
    pub fn to_hex(&self) -> String {
        if self.a == 255 {
            format!("#{:02X}{:02X}{:02X}", self.r, self.g, self.b)
        } else {
            format!("#{:02X}{:02X}{:02X}{:02X}", self.a, self.r, self.g, self.b)
        }
    }
}

/// A font resource (`Font` entry): the text a view's `Font` property takes
/// (`Segoe UI, 14pt, style=Bold`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FontSpec(pub &'static str);

impl FontSpec {
    pub fn as_str(&self) -> &'static str {
        self.0
    }
}

impl std::fmt::Display for FontSpec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}

fn be32(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

fn le32(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

fn le16(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?))
}

/// The sizes of an ICO file's images.
pub fn ico_sizes(b: &[u8]) -> Option<Vec<(u32, u32)>> {
    if le16(b, 0)? != 0 || le16(b, 2)? != 1 {
        return None;
    }
    let count = le16(b, 4)? as usize;
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let at = 6 + i * 16;
        let w = *b.get(at)? as u32;
        let h = *b.get(at + 1)? as u32;
        out.push((if w == 0 { 256 } else { w }, if h == 0 { 256 } else { h }));
    }
    out.sort_unstable();
    out.dedup();
    Some(out)
}

/// The pixel size of an image file, from its header.
pub fn image_size(b: &[u8]) -> Option<(u32, u32)> {
    if b.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Some((be32(b, 16)?, be32(b, 20)?));
    }
    if b.starts_with(b"GIF8") {
        return Some((le16(b, 6)? as u32, le16(b, 8)? as u32));
    }
    if b.starts_with(b"BM") {
        return Some((le32(b, 18)?, (le32(b, 22)? as i32).unsigned_abs()));
    }
    if let Some(sizes) = ico_sizes(b) {
        return sizes.last().copied();
    }
    if b.starts_with(&[0xFF, 0xD8]) {
        let mut i = 2;
        while i + 9 < b.len() {
            if b[i] != 0xFF {
                return None;
            }
            let marker = b[i + 1];
            let len = u16::from_be_bytes([b[i + 2], b[i + 3]]) as usize;
            if matches!(marker, 0xC0..=0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF) {
                let h = u16::from_be_bytes([b[i + 5], b[i + 6]]) as u32;
                let w = u16::from_be_bytes([b[i + 7], b[i + 8]]) as u32;
                return Some((w, h));
            }
            i += 2 + len;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_headers() {
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        png.extend_from_slice(&40u32.to_be_bytes());
        png.extend_from_slice(&20u32.to_be_bytes());
        assert_eq!(image_size(&png), Some((40, 20)));
        let mut ico = vec![0, 0, 1, 0, 2, 0];
        ico.extend_from_slice(&[16, 16, 0, 0, 1, 0, 32, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        ico.extend_from_slice(&[0, 0, 0, 0, 1, 0, 32, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        assert_eq!(ico_sizes(&ico), Some(vec![(16, 16), (256, 256)]));
        assert_eq!(image_size(&ico), Some((256, 256)));
        assert_eq!(Color::parse("#3366FF").to_hex(), "#3366FF");
        assert_eq!(Color::parse("#803366FF").to_hex(), "#803366FF");
    }
}
