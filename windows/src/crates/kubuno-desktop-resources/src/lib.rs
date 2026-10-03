//! Resources of Kubuno desktop applications — the equivalent of Windows Forms' `Resources.resx`
//! and its strongly typed `Properties.Resources` class (`vskubuno/docs/RESOURCES.md`).
//!
//! ```ignore
//! kubuno_desktop::resources!("resources.kbres");          // → `pub struct Resources;`
//!
//! let title: &str = Resources::welcome_text();     // in the current UI culture
//! let logo = Resources::logo();                    // an `Image`: bytes, format, `uri()`
//! kubuno_desktop::resources::set_culture("fr");            // `{Res …}` views repaint in French
//! ```
//!
//! - **Files.** A `.kbres` file (XML, see [`kubuno_desktop_resources_model::format`]) holds strings, images,
//!   icons, sounds, files, colours and fonts, linked (a project file) or embedded (base64), each
//!   with a comment. Satellites `resources.fr.kbres`, `resources.de-DE.kbres` translate it.
//! - **Generated class.** [`resources!`] reads the neutral file at compile time (an invalid file
//!   is a compile error), embeds it, its satellites and every linked file (`include_str!` /
//!   `include_bytes!`: nothing is read from disk at run time) and generates one accessor per entry.
//! - **Culture.** [`culture`] follows Windows' display language until [`set_culture`] is called;
//!   lookups fall back specific culture → neutral culture → same-language culture → neutral file.
//!   [`on_culture_changed`] tells the view runtime to repaint, so `{Res key}` properties refresh live.
//! - **Views.** `Text="{Res welcome_text}"`, `Image="{Res logo}"`: [`lookup`] across the registered
//!   sets; image properties receive the URI `kbres:<set>/<name>` ([`uri`]), resolved when painted
//!   ([`resolve_uri`]).

mod culture;
mod set;
mod types;

pub use culture::{culture, follow_system_culture, generation, on_culture_changed, set_culture, system_culture, Subscription};
pub use kubuno_desktop_resources_model as model;
pub use kubuno_desktop_resources_model::Kind;
pub use set::{EmbeddedSet, LoadedSet, Source, StaticSet};
pub use types::{image_size, ico_sizes, Audio, Bytes, Color, FontSpec, Icon, Image, ResolvedValue};

use std::sync::{Arc, RwLock};

/// Generates the strongly typed class of a `.kbres` file (see the crate doc):
///
/// ```ignore
/// kubuno_desktop::resources!("resources.kbres");              // pub struct Resources
/// kubuno_desktop::resources!(pub(crate) Strings, "strings.kbres");
/// ```
///
/// The path is relative to the file holding the call (like `include_str!`); tools that cannot say
/// which file that is (rust-analyzer) find it under the package's `src` folder, then its root.
/// Linked files are relative to the `.kbres` file.
#[macro_export]
macro_rules! resources {
    ($($input:tt)*) => {
        $crate::__resources_impl! { krate = $crate; $($input)* }
    };
}

#[doc(hidden)]
pub use kubuno_desktop_resources_macros::resources as __resources_impl;

/// Where a registered set comes from: generated (embedded in the program) or loaded at run time
/// (the designer); a loaded set hides a generated one of the same name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    Embedded,
    Loaded,
}

struct Registered {
    origin: Origin,
    source: Arc<dyn Source>,
    /// The address of a generated set (two crates may both have a `resources.kbres`), 0 otherwise.
    id: usize,
}

static SETS: RwLock<Vec<Registered>> = RwLock::new(Vec::new());

/// Registers a set for `{Res …}` lookups (the generated class does it the first time one of its
/// accessors runs, and when the program starts). Registering a set again replaces it.
pub fn register(source: Arc<dyn Source>, origin: Origin) {
    if let Ok(mut sets) = SETS.write() {
        sets.retain(|r| !(r.origin == origin && r.source.name() == source.name()));
        sets.push(Registered { origin, source, id: 0 });
    }
    culture::bump();
}

/// Registers a generated set once (what the generated `register()` calls).
#[doc(hidden)]
pub fn register_static(set: &'static StaticSet) {
    let id = set as *const StaticSet as usize;
    if let Ok(mut sets) = SETS.write() {
        if sets.iter().any(|r| r.id == id) {
            return;
        }
        sets.push(Registered { origin: Origin::Embedded, source: Arc::new(StaticRef(set)), id });
    }
    culture::bump();
}

/// A `'static` generated set behind an `Arc` (whose data pointer is the set itself).
struct StaticRef(&'static StaticSet);

impl Source for StaticRef {
    fn name(&self) -> &str {
        self.0.name()
    }
    fn resolve(&self, name: &str, culture: &str) -> Option<ResolvedValue> {
        self.0.resolve(name, culture)
    }
    fn entries(&self) -> Vec<(String, Kind)> {
        self.0.entries()
    }
    fn cultures(&self) -> Vec<String> {
        self.0.cultures()
    }
}

/// Replaces every loaded set at once (the designer, when the project's resource files change).
pub fn replace_loaded(sources: Vec<Arc<dyn Source>>) {
    if let Ok(mut sets) = SETS.write() {
        sets.retain(|r| r.origin != Origin::Loaded);
        sets.extend(sources.into_iter().map(|source| Registered { origin: Origin::Loaded, source, id: 0 }));
    }
    culture::bump();
}

/// The names of the registered sets (loaded ones first).
pub fn set_names() -> Vec<String> {
    ordered().iter().map(|s| s.name().to_string()).collect()
}

fn ordered() -> Vec<Arc<dyn Source>> {
    let Ok(sets) = SETS.read() else { return Vec::new() };
    let loaded = sets.iter().filter(|r| r.origin == Origin::Loaded);
    let embedded = sets.iter().filter(|r| r.origin == Origin::Embedded && !sets.iter().any(|l| l.origin == Origin::Loaded && l.source.name() == r.source.name()));
    loaded.chain(embedded).map(|r| r.source.clone()).collect()
}

/// The value of resource `name` in the current culture: in set `set` when given, else in the set
/// named `scope` first (a view's own resources: the set of the view's file stem), then in every
/// registered set in turn.
pub fn lookup(scope: Option<&str>, set: Option<&str>, name: &str) -> Option<ResolvedValue> {
    lookup_in(scope, set, name, &culture())
}

/// [`lookup`] in a given culture.
pub fn lookup_in(scope: Option<&str>, set: Option<&str>, name: &str, culture: &str) -> Option<ResolvedValue> {
    let sets = ordered();
    if let Some(set) = set {
        return sets.iter().find(|s| s.name() == set)?.resolve(name, culture);
    }
    if let Some(scope) = scope {
        if let Some(v) = sets.iter().find(|s| s.name() == scope).and_then(|s| s.resolve(name, culture)) {
            return Some(v);
        }
    }
    sets.iter().filter(|s| Some(s.name()) != scope).find_map(|s| s.resolve(name, culture))
}

/// The text of string resource `name` in the current culture, searched in every set (`""` when
/// there is none) — the dynamic counterpart of the generated accessors, e.g. for keys computed at
/// run time (Drive's `tr("Home")`).
pub fn string(name: &str) -> String {
    lookup(None, None, name).and_then(|v| v.text()).unwrap_or_default()
}

/// The URI scheme of resource images.
pub const URI_SCHEME: &str = "kbres:";

/// `kbres:<set>/<name>` — what an image property holds for a resource.
pub fn uri(set: &str, name: &str) -> String {
    if set.is_empty() {
        format!("{URI_SCHEME}{name}")
    } else {
        format!("{URI_SCHEME}{set}/{name}")
    }
}

/// `(set, name)` of a `kbres:` URI (`kbres:logo` has no set).
pub fn parse_uri(uri: &str) -> Option<(Option<&str>, &str)> {
    let rest = uri.trim().strip_prefix(URI_SCHEME)?;
    Some(match rest.split_once('/') {
        Some((set, name)) => (Some(set).filter(|s| !s.is_empty()), name),
        None => (None, rest),
    })
}

/// The bytes a `kbres:` URI names in the current culture: `(content id, bytes, format)`. What the
/// image loader of `kubuno_desktop_controls` calls for an image property holding such a URI.
pub fn resolve_uri(uri: &str) -> Option<(u64, Bytes, String)> {
    let (set, name) = parse_uri(uri)?;
    match lookup(None, set, name)? {
        ResolvedValue::Bytes { bytes, format, .. } => Some((bytes.content_id(), bytes, format)),
        ResolvedValue::Text { .. } => None,
    }
}

#[cfg(test)]
mod tests;
