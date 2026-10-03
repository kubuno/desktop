//! [`Image`]: a picture to draw — a file decoded on first use for the surface's device, or a bitmap
//! the caller already holds.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use drive_app_controls::Renderer;
use windows::Win32::Graphics::Direct2D::ID2D1Bitmap1;

use super::types::SizeF;

#[derive(Clone)]
enum Source {
    File(Rc<str>),
    Bitmap(ID2D1Bitmap1),
}

/// A picture (`System.Drawing.Image`): cheap to clone, decoded lazily.
#[derive(Clone)]
pub struct Image {
    source: Source,
}

impl std::fmt::Debug for Image {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.source {
            Source::File(p) => write!(f, "Image::from_file({p:?})"),
            Source::Bitmap(_) => f.write_str("Image::from_bitmap(..)"),
        }
    }
}

impl PartialEq for Image {
    fn eq(&self, other: &Self) -> bool {
        match (&self.source, &other.source) {
            (Source::File(a), Source::File(b)) => a == b,
            (Source::Bitmap(a), Source::Bitmap(b)) => a == b,
            _ => false,
        }
    }
}

/// Decoded files by (device, path): a device context owns its bitmaps, a new device reloads.
type Decoded = HashMap<(usize, Rc<str>), Option<ID2D1Bitmap1>>;

thread_local! {
    static DECODED: RefCell<Decoded> = RefCell::new(HashMap::new());
}

impl Image {
    /// A picture file (PNG, JPEG, BMP, GIF, TIFF, ICO — anything WIC decodes), `Image.FromFile`.
    /// Decoded the first time it is drawn; a file that cannot be read draws nothing (logged once).
    pub fn from_file(path: impl AsRef<str>) -> Self {
        Self { source: Source::File(Rc::from(path.as_ref())) }
    }

    /// A bitmap already decoded for the surface's device.
    pub fn from_bitmap(bitmap: ID2D1Bitmap1) -> Self {
        Self { source: Source::Bitmap(bitmap) }
    }

    /// The file it comes from, if any.
    pub fn path(&self) -> Option<&str> {
        match &self.source {
            Source::File(p) => Some(p),
            Source::Bitmap(_) => None,
        }
    }

    /// The bitmap for `renderer`'s device, decoding the file on first use.
    pub fn bitmap(&self, renderer: &Renderer) -> Option<ID2D1Bitmap1> {
        match &self.source {
            Source::Bitmap(b) => Some(b.clone()),
            Source::File(path) => {
                let device = windows::core::Interface::as_raw(&renderer.d2d_context) as usize;
                let key = (device, path.clone());
                if let Some(found) = DECODED.with(|m| m.borrow().get(&key).cloned()) {
                    return found;
                }
                let bitmap = match renderer.load_image_file(path) {
                    Ok(b) => Some(b),
                    Err(error) => {
                        tracing::warn!("image {path} could not be loaded: {error}");
                        None
                    }
                };
                DECODED.with(|m| m.borrow_mut().insert(key, bitmap.clone()));
                bitmap
            }
        }
    }

    /// The size in DIP, once decoded for `renderer` (`Image.Size`).
    pub fn size(&self, renderer: &Renderer) -> Option<SizeF> {
        let b = self.bitmap(renderer)?;
        // SAFETY: a plain COM getter on a live bitmap.
        let s = unsafe { b.GetSize() };
        Some(SizeF::new(s.width, s.height))
    }
}
