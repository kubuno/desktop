//! Component family `media` — pictures: `<Avatar>` (a person's picture, or their initials on a
//! tint, with a presence dot — the web's avatars) and `<PictureBox>` (WinForms `PictureBox`).
//! Compiled with the `family-media` feature (on by default through `all-families`).
//!
//! An image comes from a file (`Image`, relative to the view, bindable) or from memory
//! (`ImageData`, bound to the bytes of a PNG, JPEG, GIF, BMP… file handed as a `Shared<Vec<u8>>` /
//! `Value::Object` — a picture downloaded by the application). Decoded once per content
//! (`kubuno_desktop_controls::styled::load_image` / `load_image_bytes`).

#[allow(unused_imports)] // Used by the `component!` invocations below.
use crate::registry::macros::component;
use crate::registry::ComponentMeta;

use crate::binding::{BindingSpec, PropSource, Value, ViewModel};
use crate::node::{PaintCx, ViewNode};
use crate::props::{BuildError, Props};

use kubuno_desktop_controls::{styled, ImageLayout};
use kubuno_desktop_ui::{Canvas, Rect, Size};
use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;
use windows::Win32::Graphics::Direct2D::ID2D1Bitmap1;

/// `<SplashArtwork>`, the artwork of a splash screen (`kubuno_desktop_ui::splash`), in its own file.
#[path = "splash.rs"]
pub mod splash;

/// The image of `path` (resolved against the view's folder) or of the bytes `data` resolves to.
pub(crate) struct ImageSource {
    path: PropSource<String>,
    data: Option<BindingSpec>,
    base_dir: Option<std::path::PathBuf>,
}

impl ImageSource {
    fn read(props: &Props<'_>, base_dir: Option<std::path::PathBuf>) -> Result<Self, BuildError> {
        Ok(Self { path: props.str("Image", "")?, data: props.str("ImageData", "")?.binding().cloned(), base_dir })
    }

    /// The bitmap to show this frame, if any.
    fn bitmap(&self, c: &dyn kubuno_desktop_controls::ControlCanvas, vm: &dyn ViewModel) -> Option<ID2D1Bitmap1> {
        if let Some(spec) = &self.data {
            if let Some(Value::Object(o)) = vm.get(&spec.path) {
                if let Some(bytes) = o.downcast_ref::<Vec<u8>>() {
                    if !bytes.is_empty() {
                        let key = (o.identity() as u64) ^ ((bytes.len() as u64) << 40);
                        return styled::load_image_bytes(c, key, bytes);
                    }
                }
            }
        }
        let path = self.path.resolve(vm);
        if path.trim().is_empty() {
            return None;
        }
        let full = match &self.base_dir {
            // A resource (`kbres:` URI, vskubuno docs/RESOURCES.md) is not a path.
            Some(dir) if std::path::Path::new(&path).is_relative() && !path.starts_with(kubuno_desktop_resources::URI_SCHEME) => dir.join(&path).to_string_lossy().into_owned(),
            _ => path,
        };
        styled::load_image(c, &full)
    }
}

/// Draws `bitmap` covering `dest` (cropped, never distorted: the web's `object-cover`).
fn draw_cover(c: &dyn kubuno_desktop_controls::ControlCanvas, bitmap: &ID2D1Bitmap1, dest: Rect) {
    let (w, h) = styled::image_size(bitmap);
    if w <= 0.0 || h <= 0.0 {
        return;
    }
    let (dw, dh) = (dest.right - dest.left, dest.bottom - dest.top);
    let scale = (dw / w).max(dh / h);
    let (sw, sh) = (w * scale, h * scale);
    let left = dest.left + (dw - sw) / 2.0;
    let top = dest.top + (dh - sh) / 2.0;
    c.draw_bitmap(bitmap, &Rect::new(left, top, left + sw, top + sh), 1.0);
}

// ═════════════════════════════════════════════════════════════════════════
// Avatar
// ═════════════════════════════════════════════════════════════════════════

component! {
    mod_name: avatar,
    name: "Avatar",
    // Note: The web's avatars (`ShareDialog`'s `Avatar`, `UserAvatar`): a picture, else initials on a tint.
    doc: "A person's picture in a circle, or their initials on a colour when there is no picture, with an optional presence dot.",
    ctor: kubuno_desktop_ui::display::Icon::new("User"),
    children: ChildrenModel::None,
    props: [
        PropertyMeta::new("DisplayName", PropKind::String, "", "The person's name: its initials are shown when there is no picture, and it picks the colour.").category("Appearance").localizable().bindable(),
        PropertyMeta::new("Initials", PropKind::String, "", "The letters shown instead of the name's initials.").category("Appearance").bindable(),
        PropertyMeta::new("Image", PropKind::String, "", "The picture: the path of an image file, relative to the view.").category("Appearance").editor("image").bindable(),
        PropertyMeta::new("ImageData", PropKind::String, "", "The picture as the bytes of an image file (a binding to a Shared<Vec<u8>>), for a picture the application downloaded.").category("Appearance").editor("object").bindable(),
        PropertyMeta::new("Tint", PropKind::Enum(&["Auto", "Accent", "Primary"]), "Auto", "Auto: a colour picked from the name, initials in white. Accent: the pale accent colour, initials in the accent colour. Primary: the accent colour, initials in white (the signed-in user in a header).").category("Appearance"),
        PropertyMeta::new("Shape", PropKind::Enum(&["Circle", "Rounded"]), "Circle", "A circle, or a square with rounded corners.").category("Appearance"),
        PropertyMeta::new("Presence", PropKind::Enum(&["None", "Online", "Away", "Busy", "Offline"]), "None", "The dot shown at the bottom right: whether the person is available.").category("Appearance").bindable(),
        PropertyMeta::new("AvatarSize", PropKind::F32, "36", "The diameter of the avatar, in DIP (the element's box is filled when it is smaller).").category("Layout"),
    ],
    events: [],
    smoke: |i| { i },
    build: |props, cx| {
        crate::registry::families::media::build_avatar(props, cx.base_dir.clone())
    },
}

pub(crate) fn build_avatar(props: &Props<'_>, base_dir: Option<std::path::PathBuf>) -> Result<Box<dyn ViewNode>, BuildError> {
    Ok(Box::new(AvatarNode {
        name: props.str("DisplayName", "")?,
        initials: props.str("Initials", "")?,
        image: ImageSource::read(props, base_dir)?,
        accent: props.enum_("Tint", "Auto")?,
        rounded: props.enum_("Shape", "Circle")?,
        presence: props.enum_("Presence", "None")?,
        size: props.f32("AvatarSize", 36.0)?,
    }))
}

/// `<Avatar>`'s live node.
pub struct AvatarNode {
    name: PropSource<String>,
    initials: PropSource<String>,
    image: ImageSource,
    accent: PropSource<String>,
    rounded: PropSource<String>,
    presence: PropSource<String>,
    size: PropSource<f32>,
}

/// The initials of `name`: the first letters of its first two words (one word: its first two
/// letters), upper case.
pub fn initials_of(name: &str) -> String {
    let words: Vec<&str> = name.split(|c: char| c.is_whitespace() || c == '.' || c == '@' || c == '-' || c == '_').filter(|w| !w.is_empty()).collect();
    let letters: String = match words.as_slice() {
        [] => String::new(),
        [one] => one.chars().take(2).collect(),
        [first, second, ..] => first.chars().take(1).chain(second.chars().take(1)).collect(),
    };
    letters.to_uppercase()
}

/// The web's avatar palette (`ShareDialog.tsx`), picked by a hash of the name.
fn tint_of(name: &str) -> D2D1_COLOR_F {
    const PALETTE: [u32; 8] = [0x1a73e8, 0xd93025, 0x1e8e3e, 0xf9ab00, 0x9334e6, 0xe8710a, 0x12b5cb, 0xd01884];
    let h = name.chars().fold(0u32, |h, c| h.wrapping_mul(31).wrapping_add(c as u32));
    let rgb = PALETTE[(h as usize) % PALETTE.len()];
    D2D1_COLOR_F { r: ((rgb >> 16) & 0xff) as f32 / 255.0, g: ((rgb >> 8) & 0xff) as f32 / 255.0, b: (rgb & 0xff) as f32 / 255.0, a: 1.0 }
}

impl ViewNode for AvatarNode {
    fn measure(&self, _c: &dyn Canvas, vm: &dyn ViewModel) -> Size {
        let s = self.size.resolve(vm).max(8.0);
        Size::new(s, s)
    }

    fn intrinsic_width(&self, _c: &dyn Canvas, vm: &dyn ViewModel) -> Option<f32> {
        Some(self.size.resolve(vm).max(8.0))
    }

    fn paint(&mut self, cx: &mut PaintCx<'_>, bounds: Rect) {
        let vm: &dyn ViewModel = &*cx.vm;
        let c = cx.canvas;
        let side = self.size.resolve(vm).max(8.0).min(bounds.right - bounds.left).min(bounds.bottom - bounds.top);
        if side <= 0.0 {
            return;
        }
        let rect = Rect::new(bounds.left, bounds.top, bounds.left + side, bounds.top + side);
        let radius = if self.rounded.resolve(vm) == "Rounded" { (side * 0.22).round() } else { side / 2.0 };
        let theme = c.theme().clone();
        let name = self.name.resolve(vm);
        match self.image.bitmap(c, vm) {
            Some(bitmap) => {
                c.push_clip_rounded(&rect, radius);
                draw_cover(c, &bitmap, rect);
                c.pop_clip_rounded();
            }
            None => {
                let (fill, ink) = match self.accent.resolve(vm).as_str() {
                    "Accent" => (theme.accent_light, theme.accent),
                    "Primary" => (theme.accent, theme.accent_foreground),
                    _ => (tint_of(&name), D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 1.0 }),
                };
                c.fill_rounded(&rect, radius, &fill);
                let own = self.initials.resolve(vm);
                let text = if own.trim().is_empty() { initials_of(&name) } else { own.trim().to_uppercase() };
                if text.is_empty() {
                    c.vector_icon("User", &rect, side * 0.55, &ink);
                } else {
                    let f = c.formats();
                    // The letters grow with the avatar: a profile hero is not a list row.
                    // The signed-in user's small avatar of a header writes its letters in the body face
                    // (the web's `HeaderActions`); a list's, in the strong caption face.
                    let primary_small = side < 40.0 && self.accent.resolve(vm) == "Primary";
                    let format = if side >= 72.0 { &f.heading_strong } else if side >= 40.0 { &f.body_strong } else if primary_small { &f.body } else { &f.caption_strong };
                    c.text_ellipsis_center(&text, &rect, format, &ink);
                }
            }
        }
        let presence = match self.presence.resolve(vm).as_str() {
            "Online" => Some(theme.success),
            "Away" => Some(theme.warning),
            "Busy" => Some(theme.danger),
            "Offline" => Some(theme.text_tertiary),
            _ => None,
        };
        if let Some(color) = presence {
            // A dot a quarter of the avatar, ringed with the surface it sits on.
            let d = (side * 0.28).clamp(8.0, 16.0);
            let ring = 2.0;
            let dot = Rect::new(rect.right - d, rect.bottom - d, rect.right, rect.bottom);
            c.fill_rounded(&Rect::new(dot.left - ring, dot.top - ring, dot.right + ring, dot.bottom + ring), (d + 2.0 * ring) / 2.0, &c.current_bg());
            c.fill_rounded(&dot, d / 2.0, &color);
        }
    }
}

// ═════════════════════════════════════════════════════════════════════════
// PictureBox
// ═════════════════════════════════════════════════════════════════════════

component! {
    mod_name: picture_box,
    name: "PictureBox",
    // Note: WinForms `PictureBox`: an image, laid out by `SizeMode`.
    doc: "An image, from a file or from memory, fitted into the control by SizeMode.",
    ctor: kubuno_desktop_ui::display::Icon::new("Image"),
    children: ChildrenModel::None,
    props: [
        PropertyMeta::new("Image", PropKind::String, "", "The image: the path of an image file, relative to the view.").category("Appearance").editor("image").bindable(),
        PropertyMeta::new("ImageData", PropKind::String, "", "The image as the bytes of an image file (a binding to a Shared<Vec<u8>>).").category("Appearance").editor("object").bindable(),
        PropertyMeta::new("SizeMode", PropKind::Enum(&["Normal", "Stretch", "Zoom", "Center", "Cover"]), "Normal", "Normal: at its size, top left. Stretch: to the control's size. Zoom: as large as fits, keeping its proportions. Center: at its size, centred. Cover: fills the control, keeping its proportions (cropped).").category("Behavior"),
        PropertyMeta::new("CornerRadius", PropKind::F32, "0", "Rounds the corners of the image, in DIP.").category("Appearance"),
        PropertyMeta::new("BorderStyle", PropKind::Enum(&["None", "FixedSingle"]), "None", "A line drawn around the control.").category("Appearance"),
    ],
    events: [],
    smoke: |i| { i },
    build: |props, cx| {
        Ok(Box::new(crate::registry::families::media::PictureBoxNode {
            image: crate::registry::families::media::ImageSource::read(props, cx.base_dir.clone())?,
            mode: props.enum_("SizeMode", "Normal")?,
            radius: props.f32("CornerRadius", 0.0)?,
            border: props.enum_("BorderStyle", "None")?,
        }) as Box<dyn ViewNode>)
    },
}

/// `<PictureBox>`'s live node.
pub struct PictureBoxNode {
    image: ImageSource,
    mode: PropSource<String>,
    radius: PropSource<f32>,
    border: PropSource<String>,
}

impl ViewNode for PictureBoxNode {
    fn measure(&self, _c: &dyn Canvas, _vm: &dyn ViewModel) -> Size {
        Size::new(160.0, 120.0)
    }

    fn paint(&mut self, cx: &mut PaintCx<'_>, bounds: Rect) {
        let vm: &dyn ViewModel = &*cx.vm;
        let c = cx.canvas;
        let radius = self.radius.resolve(vm).max(0.0);
        let border = self.border.resolve(vm) == "FixedSingle";
        let Some(bitmap) = self.image.bitmap(c, vm) else {
            if border {
                c.stroke_rounded(&bounds, radius, &c.theme().card_stroke);
            }
            if cx.design.is_some() {
                crate::node::custom::paint_placeholder(c, bounds, "PictureBox", "Set Image to show a picture");
            }
            return;
        };
        let clip = |c: &dyn kubuno_desktop_controls::ControlCanvas| {
            if radius > 0.0 {
                c.push_clip_rounded(&bounds, radius);
            } else {
                c.push_clip(&bounds);
            }
        };
        clip(c);
        match self.mode.resolve(vm).as_str() {
            "Cover" => draw_cover(c, &bitmap, bounds),
            mode => {
                let layout = match mode {
                    "Stretch" => ImageLayout::Stretch,
                    "Zoom" => ImageLayout::Zoom,
                    "Center" => ImageLayout::Center,
                    _ => ImageLayout::None,
                };
                styled::draw_image(c, &bitmap, bounds, layout);
            }
        }
        if radius > 0.0 {
            c.pop_clip_rounded();
        } else {
            c.pop_clip();
        }
        if border {
            c.stroke_rounded(&bounds, radius, &c.theme().card_stroke);
        }
    }
}

/// Every component this family declares.
pub const ALL: &[ComponentMeta] = &[avatar::META, picture_box::META, splash::splash_artwork::META];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initials_follow_the_web() {
        assert_eq!(initials_of("Martinien Olinga"), "MO");
        assert_eq!(initials_of("alice"), "AL");
        assert_eq!(initials_of("jean-paul.sartre@example.com"), "JP");
        assert_eq!(initials_of("  "), "");
    }

    #[test]
    fn a_name_keeps_its_tint() {
        assert_eq!(tint_of("Alice"), tint_of("Alice"));
    }

    #[test]
    fn avatars_and_pictures_compile() {
        let src = r#"<Panel><Avatar DisplayName="{Binding Name}" Presence="Online" AvatarSize="40"/><PictureBox Image="logo.png" SizeMode="Zoom" CornerRadius="8"/><Avatar ImageData="{Binding Photo}"/></Panel>"#;
        assert!(crate::compile::compile(src).is_ok(), "{:?}", crate::compile::compile(src).err());
        let bad = r#"<Panel><Avatar ImageData="photo.png"/></Panel>"#;
        assert!(crate::compile::compile(bad).is_err(), "ImageData takes a binding only");
    }
}
