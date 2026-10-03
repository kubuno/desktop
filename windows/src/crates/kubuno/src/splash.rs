//! `kubuno::SplashScreen`: the startup window of an application (`kubuno_ui::splash`), shown
//! before the main form is built and faded out once it is on screen.
//!
//! ```no_run
//! use kubuno::prelude::*;
//!
//! fn main() -> kubuno::Result {
//!     let splash = kubuno::SplashScreen::new()
//!         .artwork(kubuno::Artwork::Documents)
//!         .product("Kubuno Documents")
//!         .version(env!("CARGO_PKG_VERSION"))
//!         .show();
//!     splash.step("Chargement des polices…", 0.4);
//!     let main_form = Form::new().text("Documents").client_size(1100.0, 720.0);
//!     splash.close_when(&main_form);
//!     kubuno::Application::run(main_form)
//! }
//! ```
//!
//! A splash screen designed in Visual Studio (the « Kubuno Splash Screen » item: a `.kbview` with
//! `WindowKind="Splash"` holding a `<SplashArtwork>`) is shown the same way, set up from its view
//! with [`from_kbview`].

use std::time::Duration;

pub use kubuno_ui::splash::{is_disabled_by_user, Artwork, Splash, SplashScreen, SplashTarget};

use crate::forms::Form;
use kubuno_views::ast::{AstNode, Document, Element};

/// A form is waited for through its window: once it is open, that window; before (the usual case,
/// `close_when` called before `Application::run`), the first window the process shows — which is
/// the main form's.
impl SplashTarget for Form {
    fn splash_target_hwnd(&self) -> Option<isize> {
        self.handle()
    }
}

/// A splash screen set up from a splash view: the `<SplashArtwork>` it holds gives the artwork,
/// the product, tagline, version, first status line, licence and credits, and the root's
/// `SplashDuration` the shortest time on screen. Attributes left empty or bound
/// (`{Binding …}`) keep the defaults; chain the builder to set more (`.version(env!(…))`).
pub fn from_kbview(text: &str) -> SplashScreen {
    let mut splash = SplashScreen::new();
    let parse = kubuno_views::syntax::parse(text);
    let Some(root) = Document::cast(parse.syntax()).and_then(|d| d.root_element()) else { return splash };
    if let Some(ms) = attr(&root, "SplashDuration").and_then(|v| v.trim().parse::<f64>().ok()).filter(|ms| *ms > 0.0) {
        splash = splash.min_duration(Duration::from_millis(ms as u64));
    }
    let Some(artwork) = find(&root, "SplashArtwork") else { return splash };
    if let Some(a) = attr(&artwork, "Artwork").and_then(|a| Artwork::from_name(&a)) {
        splash = splash.artwork(a);
    }
    if let Some(product) = attr(&artwork, "Product") {
        splash = splash.product(product);
    }
    if let Some(version) = attr(&artwork, "Version") {
        // The attribute is the line as shown (« Version 0.1.0 »); the builder takes the number.
        splash = splash.version(version.trim().strip_prefix("Version ").unwrap_or(version.trim()).to_string());
    }
    if let Some(tagline) = attr(&artwork, "Tagline") {
        splash = splash.tagline(tagline);
    }
    if let Some(status) = attr(&artwork, "Status") {
        splash = splash.status(status);
    }
    if let Some(license) = attr(&artwork, "License") {
        splash = splash.license(&license);
    }
    if let Some(credits) = attr(&artwork, "Credits") {
        splash = splash.credits(credits);
    }
    splash
}

/// The literal value of `name` on `element`: `None` when absent, empty or bound.
fn attr(element: &Element, name: &str) -> Option<String> {
    element.attribute(name).and_then(|a| a.value()).filter(|v| !v.trim().is_empty() && !v.trim_start().starts_with('{'))
}

/// The first element named `name` under `element` (itself included), depth first.
fn find(element: &Element, name: &str) -> Option<Element> {
    if element.name().as_deref() == Some(name) {
        return Some(element.clone());
    }
    element.children().find_map(|child| find(&child, name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_splash_view_sets_up_the_splash_screen() {
        let view = r#"<Panel DesignWidth="800" DesignHeight="500" WindowKind="Splash" SplashDuration="2000">
            <SplashArtwork x:Name="artwork" Artwork="Chat" Product="Kubuno Réunions" Version="Version 2.1.0" Status="Connexion…" License="MIT" Credits="{Binding Credits}" Dock="Fill"/>
        </Panel>"#;
        let splash = from_kbview(view);
        let c = splash.content();
        assert_eq!(c.artwork, Artwork::Chat);
        assert_eq!(c.product, "Kubuno Réunions");
        assert_eq!(c.version, "Version 2.1.0");
        assert_eq!(c.status, "Connexion…");
        assert_eq!(c.legal, "© Kubuno contributors · MIT");
        assert_eq!(c.credits, kubuno_ui::splash::DEFAULT_CREDITS, "a bound attribute keeps the default");
        assert_eq!(c.tagline, Artwork::Chat.tagline());
    }

    #[test]
    fn a_view_without_artwork_gives_the_defaults() {
        let splash = from_kbview(r#"<Panel WindowKind="Splash"/>"#);
        assert_eq!(splash.content().artwork, Artwork::Kubuno);
        assert_eq!(from_kbview("not xml").content().product, "Kubuno Desktop");
    }
}
