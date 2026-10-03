//! `<SplashArtwork>` — the artwork of a Kubuno splash screen (`kubuno_ui::splash`): the module's
//! procedural ground, motif and hero mark, the product name, tagline and version, the status line
//! and its progress bar, the legal and credits lines. Part of the `media` family.
//!
//! It is what the Visual Studio designer shows of a splash screen view (`WindowKind="Splash"`,
//! the « Kubuno Splash Screen » item template), and what `kubuno::SplashScreen::from_kbview`
//! reads its settings from: the same design draws the real splash window, on its own thread, at
//! start-up. The artwork keeps its 16:10 proportions, centred in the element's box.

#[allow(unused_imports)] // Used by the `component!` invocation below.
use crate::registry::macros::component;

use crate::binding::{PropSource, ViewModel};
use crate::node::{PaintCx, ViewNode};
use crate::props::{BuildError, Props};

use kubuno_ui::splash::{Artwork, Parts, SplashContent};
use kubuno_ui::{Canvas, Rect, Size};

/// The artworks, by their `.kbview` names.
pub const ARTWORKS: &[&str] = &["Kubuno", "Drive", "Chat", "Documents"];

component! {
    mod_name: splash_artwork,
    name: "SplashArtwork",
    doc: "The artwork of a splash screen: the module's procedural artwork and mark, the product name, version, status line, progress bar and legal lines.",
    ctor: kubuno_ui::splash::SplashContent::new(kubuno_ui::splash::Artwork::Kubuno),
    children: ChildrenModel::None,
    props: [
        PropertyMeta::new("Artwork", PropKind::Enum(crate::registry::families::media::splash::ARTWORKS), "Kubuno", "Which application's artwork: Kubuno (the shell), Drive, Chat or Documents.").category("Appearance"),
        PropertyMeta::new("Product", PropKind::String, "", "The product name, « Kubuno Drive » (empty: the artwork's).").category("Appearance").bindable(),
        PropertyMeta::new("Version", PropKind::String, "", "The version line, « Version 0.1.0 ».").category("Appearance").bindable(),
        PropertyMeta::new("Tagline", PropKind::String, "", "The line under the product name (empty: the artwork's).").category("Appearance").bindable(),
        PropertyMeta::new("Status", PropKind::String, "Initialisation des modules…", "The status line: the start-up step in progress.").category("Appearance").bindable(),
        PropertyMeta::new("Progress", PropKind::F32, "-1", "The progress of the start-up, 0 to 1; below 0, an indeterminate shimmer.").category("Behavior").bindable(),
        PropertyMeta::new("License", PropKind::String, "AGPL-3.0-or-later", "The licence named on the legal line.").category("Appearance"),
        PropertyMeta::new("Credits", PropKind::String, "", "The credits line (empty: the default one).").category("Appearance").bindable(),
    ],
    events: [],
    smoke: |content| {
        let _ = content.title_parts();
        content
    },
    build: |props, _cx| {
        Ok(Box::new(crate::registry::families::media::splash::SplashArtworkNode::read(props)?) as Box<dyn ViewNode>)
    },
}

/// `<SplashArtwork>`'s live node.
pub struct SplashArtworkNode {
    artwork: PropSource<String>,
    product: PropSource<String>,
    version: PropSource<String>,
    tagline: PropSource<String>,
    status: PropSource<String>,
    progress: PropSource<f32>,
    license: PropSource<String>,
    credits: PropSource<String>,
}

impl SplashArtworkNode {
    fn read(props: &Props<'_>) -> Result<Self, BuildError> {
        Ok(Self {
            artwork: props.enum_("Artwork", "Kubuno")?,
            product: props.str("Product", "")?,
            version: props.str("Version", "")?,
            tagline: props.str("Tagline", "")?,
            status: props.str("Status", "Initialisation des modules…")?,
            progress: props.f32("Progress", -1.0)?,
            license: props.str("License", "AGPL-3.0-or-later")?,
            credits: props.str("Credits", "")?,
        })
    }

    /// What the splash shows, from the element's properties.
    pub fn content(&self, vm: &dyn ViewModel) -> SplashContent {
        content_of(
            &self.artwork.resolve(vm),
            &self.product.resolve(vm),
            &self.version.resolve(vm),
            &self.tagline.resolve(vm),
            &self.status.resolve(vm),
            self.progress.resolve(vm),
            &self.license.resolve(vm),
            &self.credits.resolve(vm),
        )
    }
}

/// The splash content of a `<SplashArtwork>`'s property values (empty strings take the artwork's
/// defaults).
#[allow(clippy::too_many_arguments)]
pub fn content_of(artwork: &str, product: &str, version: &str, tagline: &str, status: &str, progress: f32, license: &str, credits: &str) -> SplashContent {
    let art = Artwork::from_name(artwork).unwrap_or(Artwork::Kubuno);
    let mut content = SplashContent::new(art);
    if !product.trim().is_empty() {
        content.product = product.trim().to_string();
    }
    content.version = version.to_string();
    if !tagline.is_empty() {
        content.tagline = tagline.to_string();
    }
    content.status = status.to_string();
    content.progress = (0.0..=1.0).contains(&progress).then_some(progress);
    content.legal = kubuno_ui::splash::legal_line(license);
    if !credits.is_empty() {
        content.credits = credits.to_string();
    }
    content
}

/// The design space scaled to fit `bounds`, centred: its origin and scale.
pub fn fit(bounds: Rect) -> ((f32, f32), f32) {
    let (w, h) = (bounds.right - bounds.left, bounds.bottom - bounds.top);
    let scale = (w / kubuno_ui::splash::art::WIDTH).min(h / kubuno_ui::splash::art::HEIGHT).max(0.0);
    let origin = (bounds.left + (w - kubuno_ui::splash::art::WIDTH * scale) / 2.0, bounds.top + (h - kubuno_ui::splash::art::HEIGHT * scale) / 2.0);
    (origin, scale)
}

impl ViewNode for SplashArtworkNode {
    fn measure(&self, _c: &dyn Canvas, _vm: &dyn ViewModel) -> Size {
        Size::new(kubuno_ui::splash::art::WIDTH, kubuno_ui::splash::art::HEIGHT)
    }

    fn paint(&mut self, cx: &mut PaintCx<'_>, bounds: Rect) {
        let vm: &dyn ViewModel = &*cx.vm;
        let c = cx.canvas;
        let content = self.content(vm);
        let (origin, scale) = fit(bounds);
        let painted = match c.graphics_renderer() {
            Some(renderer) if scale > 0.0 => {
                let rt: &windows::Win32::Graphics::Direct2D::ID2D1RenderTarget = &renderer.d2d_context;
                kubuno_ui::splash::art::paint(rt, origin, scale, &content, Parts::All { progress: content.progress, phase: 0.45 }).is_ok()
            }
            _ => false,
        };
        if painted {
            c.note_drawn(&bounds);
        } else if cx.design.is_some() {
            crate::node::custom::paint_placeholder(c, bounds, "SplashArtwork", "The splash artwork is drawn with Direct2D");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_splash_artwork_compiles_and_takes_the_artwork_defaults() {
        let src = r#"<Panel WindowKind="Splash"><SplashArtwork Artwork="Drive" Version="Version 0.1.0" Progress="0.4" Dock="Fill"/></Panel>"#;
        assert!(crate::compile::compile(src).is_ok(), "{:?}", crate::compile::compile(src).err());
        let bad = r#"<Panel><SplashArtwork Artwork="Calendar"/></Panel>"#;
        assert!(crate::compile::compile(bad).is_err(), "an unknown artwork is refused");
        let c = content_of("Drive", "", "Version 0.1.0", "", "Démarrage…", 0.4, "MIT", "");
        assert_eq!(c.product, "Kubuno Drive");
        assert_eq!(c.tagline, Artwork::Drive.tagline());
        assert_eq!(c.progress, Some(0.4));
        assert_eq!(c.legal, "© Kubuno contributors · MIT");
        assert_eq!(content_of("Chat", "", "", "", "", -1.0, "", "").progress, None);
    }

    #[test]
    fn the_artwork_fits_its_box_centred() {
        let ((x, y), s) = fit(Rect::new(0.0, 0.0, 400.0, 400.0));
        assert_eq!(s, 0.5);
        assert_eq!((x, y), (0.0, 75.0));
        let ((x, y), s) = fit(Rect::new(10.0, 10.0, 810.0, 510.0));
        assert_eq!((s, x, y), (1.0, 10.0, 10.0));
    }
}
