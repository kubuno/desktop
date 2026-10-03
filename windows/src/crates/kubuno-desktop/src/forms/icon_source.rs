//! The icon of a control, written in code (`vskubuno/docs/ICONS.md`): what an `Icon="…"`
//! attribute holds in a `.kbview`, typed.
//!
//! ```no_run
//! use kubuno_desktop::prelude::*;
//! use kubuno_desktop::IconSource;
//!
//! let save = Button::new().text("Save").icon("Save");                       // the Kubuno icon set
//! let open = Button::new().text("Open").icon(IconSource::file("images/open.svg")); // an image file
//! let logo = Button::new().text("Kubuno").icon(IconSource::resource("Logo"));  // a project resource
//! let big = Button::new().text("Print").icon("Printer").icon_size(32.0).icon_color("Primary");
//! ```

use std::path::Path;

use kubuno_desktop_views::binding::Value;

/// An icon: a name of the Kubuno icon set (`"Save"`, `"FolderOpen"`, Lucide's names), an image file
/// (SVG, PNG, JPEG, BMP, GIF, ICO, TIFF, WebP), or an image resource of the project. A `&str` or a
/// `String` converts to one as written (a name, or a path when it has an image extension); a
/// `&Path`/`PathBuf` is always a file.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct IconSource {
    value: String,
}

impl IconSource {
    /// A glyph of the Kubuno icon set (`"Save"`, `"ChevronDown"`, or an alias such as `"trash"`).
    pub fn named(name: impl Into<String>) -> Self {
        Self { value: name.into() }
    }

    /// An image file: relative to the view's folder (to the working folder for a form built only in
    /// code), or absolute.
    pub fn file(path: impl AsRef<Path>) -> Self {
        Self { value: path.as_ref().to_string_lossy().replace('\\', "/") }
    }

    /// An image resource of the project (`{Res key}`).
    pub fn resource(key: &str) -> Self {
        Self { value: format!("{{Res {key}}}") }
    }

    /// No icon.
    pub fn none() -> Self {
        Self::default()
    }

    /// The attribute value it stands for (`"Save"`, `"images/open.svg"`, `"{Res Logo}"`).
    pub fn as_str(&self) -> &str {
        &self.value
    }

    /// Whether it names nothing.
    pub fn is_none(&self) -> bool {
        self.value.trim().is_empty()
    }
}

impl From<&str> for IconSource {
    fn from(value: &str) -> Self {
        Self { value: value.to_string() }
    }
}

impl From<String> for IconSource {
    fn from(value: String) -> Self {
        Self { value }
    }
}

impl From<&Path> for IconSource {
    fn from(path: &Path) -> Self {
        Self::file(path)
    }
}

impl From<std::path::PathBuf> for IconSource {
    fn from(path: std::path::PathBuf) -> Self {
        Self::file(path)
    }
}

impl From<IconSource> for Value {
    fn from(icon: IconSource) -> Self {
        Value::Str(icon.value)
    }
}

impl std::fmt::Display for IconSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.value)
    }
}

/// How an image that is not square fills the icon's box (`IconScaling`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum IconScaling {
    /// All of it, as large as fits.
    #[default]
    Fit,
    /// The whole box covered, the overflow clipped.
    Fill,
    /// Fitted to the box exactly.
    Stretch,
    /// Its own size.
    None,
}

impl IconScaling {
    pub(crate) fn name(self) -> &'static str {
        match self {
            IconScaling::Fit => "Fit",
            IconScaling::Fill => "Fill",
            IconScaling::Stretch => "Stretch",
            IconScaling::None => "None",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_are_the_attribute_text() {
        assert_eq!(IconSource::from("Save").as_str(), "Save");
        assert_eq!(IconSource::file("images\\open.svg").as_str(), "images/open.svg");
        assert_eq!(IconSource::from(Path::new("a/b.png")).as_str(), "a/b.png");
        assert_eq!(IconSource::resource("Logo").as_str(), "{Res Logo}");
        assert!(IconSource::none().is_none());
        assert_eq!(Value::from(IconSource::named("Printer")), Value::Str("Printer".into()));
    }
}
