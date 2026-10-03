//! What an icon attribute names, resolved for the painters — shared by every node that lets an XML
//! attribute name an icon (`Button`'s `Icon`, `IconButton`'s `Icon`, `<Icon Name="…">`,
//! `EmptyState`'s `Icon`, a menu item's, the ribbon's `SmallIcon`/`LargeIcon`…).
//!
//! An icon value (`vskubuno/docs/ICONS.md`) is one of:
//!
//! * a glyph of the embedded Kubuno icon set — Lucide's icons (`Save`, `ChevronsUpDown`), the
//!   Kubuno themed icons and the module logos, exactly as `kubuno_drive_desktop_app_controls`' embedded data
//!   spells them — or one of the short lower-case [`alias`]es (`close`, `trash`);
//! * an image file, relative to the view like `BackgroundImage` (`resources/save.svg`): SVG, PNG,
//!   JPEG, BMP, GIF, ICO, TIFF or WebP, drawn by `kubuno_desktop_controls::icon_image`;
//! * a resource of the project, `{Res key}`, whose value is one of the above (resolved by the
//!   resources runtime through [`set_resource_resolver`]);
//! * a binding, `{Binding Path}`, whose value is one of the above.
//!
//! The element's `IconColor`, `IconSize` and `IconScaling` travel with the value as drawing
//! options ([`with_options`], `kubuno_drive_desktop_app_controls::icon_source`), so every control that paints an
//! icon honours them without code of its own.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use kubuno_drive_desktop_app_controls::icon_source::{self, IconScaling, IconSpec};

/// A short, memorable lower-case alias for a few of the icons `.kbview` files are most likely to
/// name, kept for readability in view markup (`Icon="close"` vs. `Icon="X"`) and for backward
/// compatibility with the families' original hand-written tables. Tried first; [`resolve`] falls
/// through to the real Lucide name (case-sensitive, exactly as `kubuno_drive_desktop_app_controls`'s embedded
/// data spells it) for anything not listed here.
pub fn alias(name: &str) -> Option<&'static str> {
    Some(match name {
        "check" => "Check",
        "close" | "x" => "X",
        "plus" => "Plus",
        "search" => "Search",
        "save" => "Save",
        "arrow-right" => "ArrowRight",
        "arrow-left" => "ArrowLeft",
        "trash" => "Trash2",
        "download" => "Download",
        "upload" => "Upload",
        "chevron-down" => "ChevronDown",
        "chevron-right" => "ChevronRight",
        "chevron-up" => "ChevronUp",
        "chevron-left" => "ChevronLeft",
        "caret-down" => "ChevronDown",
        "help" => "HelpCircle",
        "more-vertical" => "MoreVertical",
        "more-horizontal" => "MoreHorizontal",
        "inbox" => "Inbox",
        "users" => "Users",
        "info" => "Info",
        "alert-circle" => "AlertCircle",
        "alert-triangle" => "AlertTriangle",
        "check-circle" => "CheckCircle2",
        _ => return None,
    })
}

/// Every alias of [`alias`], with the glyph it names (the language server offers them too).
pub const ALIASES: &[&str] = &[
    "check", "close", "x", "plus", "search", "save", "arrow-left", "trash", "download", "upload", "chevron-down", "chevron-right",
    "chevron-up", "chevron-left", "caret-down", "help", "more-vertical", "more-horizontal", "inbox", "users", "info", "alert-circle",
    "alert-triangle", "check-circle",
];

/// The named sizes of `IconSize` (`Small` 16, `Medium` 20, `Large` 24, `XLarge` 32).
pub const ICON_SIZES: &[(&str, f32)] = &[("Small", 16.0), ("Medium", 20.0), ("Large", 24.0), ("XLarge", 32.0)];

// ── Where relative files are ───────────────────────────────────────────────────────────────────

thread_local! {
    /// The folder of the view being built or painted on this thread: relative image files are
    /// resolved against it, like `BackgroundImage`.
    static BASE_DIR: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
    /// The folder of the view this thread last loaded (`Runtime::set_base_dir`): what a relative
    /// file resolves against outside a build or a frame (the designer's picture of the title bar).
    static DEFAULT_BASE_DIR: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
}

/// Restores the previous base folder when dropped (see [`enter_base_dir`]).
pub(crate) struct BaseDirScope(Option<PathBuf>);

impl Drop for BaseDirScope {
    fn drop(&mut self) {
        let previous = self.0.take();
        BASE_DIR.with(|d| *d.borrow_mut() = previous);
    }
}

/// Resolves relative icon files against `dir` until the returned scope ends (the runtime enters
/// it while it builds and paints a view).
pub(crate) fn enter_base_dir(dir: Option<&Path>) -> BaseDirScope {
    let previous = BASE_DIR.with(|d| std::mem::replace(&mut *d.borrow_mut(), dir.map(Path::to_path_buf)));
    BaseDirScope(previous)
}

/// The folder of the view this thread last loaded (see `DEFAULT_BASE_DIR`).
pub(crate) fn default_base_dir() -> Option<PathBuf> {
    DEFAULT_BASE_DIR.with(|d| d.borrow().clone())
}

/// The folder of the view this thread loads, when known (the design surface and the file watcher set it): where a
/// component that reads a file of its project starts from — the storage components' `.kbsettings` in the designer
/// (vskubuno docs/STORAGE-COMPONENTS.md).
pub fn view_folder() -> Option<PathBuf> {
    default_base_dir()
}

/// Records the folder of the view this thread loads (see `DEFAULT_BASE_DIR`).
pub(crate) fn set_default_base_dir(dir: Option<&Path>) {
    DEFAULT_BASE_DIR.with(|d| *d.borrow_mut() = dir.map(Path::to_path_buf));
}

/// `path` made absolute against the view's folder (unchanged when absolute or when no view folder
/// is known).
fn absolute(path: &str) -> String {
    // A resource image (`kbres:…`) is not a file.
    if Path::new(path).is_absolute() || icon_source::is_resource(path) {
        return path.to_string();
    }
    let dir = BASE_DIR.with(|d| d.borrow().clone()).or_else(|| DEFAULT_BASE_DIR.with(|d| d.borrow().clone()));
    match dir.as_deref() {
        Some(dir) => dir.join(path.replace('/', std::path::MAIN_SEPARATOR_STR)).to_string_lossy().into_owned(),
        None => path.to_string(),
    }
}

// ── Resources ──────────────────────────────────────────────────────────────────────────────────

static RESOURCE_RESOLVER: OnceLock<fn(&str) -> Option<String>> = OnceLock::new();

/// Installs how `{Res key}` finds the icon a resource of the project holds (its value: a glyph
/// name, or the absolute path of an image file) — the resources runtime installs it. The first
/// call wins.
pub fn set_resource_resolver(resolver: fn(&str) -> Option<String>) {
    let _ = RESOURCE_RESOLVER.set(resolver);
}

/// The key of a `{Res key}` value.
pub fn resource_key(value: &str) -> Option<&str> {
    let inner = value.trim().strip_prefix('{')?.strip_suffix('}')?.trim();
    let key = inner.strip_prefix("Res")?;
    key.starts_with(char::is_whitespace).then(|| key.trim()).filter(|k| !k.is_empty())
}

/// Teaches the painters this crate's colours: an `IconColor` naming a theme colour (`Accent`) or a
/// web colour (`Red`) is resolved like `ForeColor`. Idempotent; the compiler calls it.
pub fn install() {
    fn tint(text: &str, theme: &kubuno_drive_desktop_app_controls::Theme) -> Option<windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F> {
        crate::style::parse_color(text).ok().flatten().map(|c| c.resolve_with(theme, crate::style::high_contrast()))
    }
    kubuno_desktop_controls::icon_image::set_tint_resolver(tint);
}

// ── Resolution ─────────────────────────────────────────────────────────────────────────────────

/// The glyph `name` names: an [`alias`] or a glyph of the embedded set (not an image file).
pub fn glyph(name: &str) -> Option<&'static str> {
    if name.is_empty() || icon_source::is_image_path(name) || name.contains(icon_source::OPTIONS_SEPARATOR) {
        return None;
    }
    // An alias whose glyph the set does not ship (`arrow-right`) names nothing.
    alias(name).filter(|g| kubuno_drive_desktop_app_controls::icon_name(g).is_some()).or_else(|| kubuno_drive_desktop_app_controls::icon_name(name))
}

/// Resolves an icon value (see the module doc) to the `&'static str` the painters take
/// (`Canvas::vector_icon`): a glyph name, or an image file made absolute against the view's folder,
/// with its drawing options. `None` when it names nothing — the caller decides what "no such icon"
/// means for it (skip the glyph, or paint the set's own missing-glyph placeholder — see
/// [`resolve_or`]). A missing image file still resolves: it simply draws nothing.
pub fn resolve(value: &str) -> Option<&'static str> {
    resolve_depth(value, 0)
}

fn resolve_depth(value: &str, depth: u8) -> Option<&'static str> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    if let Some(key) = resource_key(value) {
        let found = RESOURCE_RESOLVER.get().and_then(|r| r(key))?;
        return if depth < 2 { resolve_depth(&found, depth + 1) } else { None };
    }
    let spec = icon_source::parse(value);
    let source: String = if let Some(g) = glyph(spec.source) {
        if !spec.has_options() {
            return Some(g);
        }
        g.to_string()
    } else if icon_source::is_image_path(spec.source) {
        absolute(spec.source)
    } else {
        return None;
    };
    Some(icon_source::intern(&icon_source::compose(&IconSpec { source: &source, ..spec })))
}

/// [`resolve`], falling back to `fallback` (typically `""`, which `kubuno_desktop_ui::display::Icon`/
/// `EmptyState` already render as their own missing-glyph box rather than nothing) when the value
/// names nothing.
#[allow(dead_code)]
pub(crate) fn resolve_or(name: &str, fallback: &'static str) -> &'static str {
    resolve(name).unwrap_or(fallback)
}

// ── The element's options ──────────────────────────────────────────────────────────────────────

/// `value` (an icon attribute's literal) with the element's `IconColor`, `IconSize` and
/// `IconScaling` (read through `attribute`) as drawing options, and an image file made absolute
/// against the view's folder. A value that is a binding or a resource, or names nothing, is
/// returned unchanged (resolved later, at paint).
pub fn with_options(value: &str, attribute: impl Fn(&str) -> Option<String>) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.starts_with('{') {
        return value.to_string();
    }
    let literal = |name: &str| attribute(name).map(|v| v.trim().to_string()).filter(|v| !v.is_empty() && !v.starts_with('{'));
    let tint = literal("IconColor");
    let size = literal("IconSize").and_then(|s| parse_icon_size(&s).ok().flatten());
    let scaling = literal("IconScaling").and_then(|s| IconScaling::parse(&s)).unwrap_or_default();
    let spec = icon_source::parse(trimmed);
    let source = if icon_source::is_image_path(spec.source) { absolute(spec.source) } else { spec.source.to_string() };
    icon_source::compose(&IconSpec {
        source: &source,
        tint: tint.as_deref().or(spec.tint),
        size: size.or(spec.size),
        scaling: if scaling != IconScaling::Fit { scaling } else { spec.scaling },
        mirror: spec.mirror,
    })
}

/// An element's icon attribute `name` as the painters take it ([`with_options`] then [`resolve`]),
/// `None` when absent or naming nothing — for a family that reads its elements directly.
pub fn attribute(element: &crate::ast::Element, name: &str) -> Option<&'static str> {
    let read = |n: &str| element.attribute(n).and_then(|a| a.value());
    resolve(&with_options(&read(name)?, read))
}

/// An `IconSize` value: a named size ([`ICON_SIZES`]), a number of pixels, or `width, height`;
/// `Ok(None)` for an empty value (the control's own size). Pure.
pub fn parse_icon_size(text: &str) -> Result<Option<(f32, f32)>, String> {
    let t = text.trim();
    if t.is_empty() || t.eq_ignore_ascii_case("Auto") {
        return Ok(None);
    }
    if let Some((_, px)) = ICON_SIZES.iter().find(|(n, _)| n.eq_ignore_ascii_case(t)) {
        return Ok(Some((*px, *px)));
    }
    let numbers: Vec<Result<f32, _>> = t.split(',').map(|p| p.trim().trim_end_matches("px").trim().parse::<f32>()).collect();
    let bad = || format!("`{t}` is not an icon size: write Small, Medium, Large, XLarge, a number of pixels, or width, height");
    match numbers.as_slice() {
        [Ok(n)] if *n > 0.0 && *n <= 512.0 => Ok(Some((*n, *n))),
        [Ok(w), Ok(h)] if *w > 0.0 && *h > 0.0 && *w <= 512.0 && *h <= 512.0 => Ok(Some((*w, *h))),
        _ => Err(bad()),
    }
}

// ── For the tools (the language server, the Visual Studio icon picker) ─────────────────────────

/// The words a glyph is found by: its name split at its capitals (`FolderOpen` → folder, open), and
/// the source name of the comment above it (Lucide's `folder-open`).
pub fn keywords(name: &str, comment: &str) -> Vec<String> {
    let mut words: Vec<String> = Vec::new();
    let mut current = String::new();
    for c in name.chars() {
        if (c.is_uppercase() || c.is_ascii_digit() && !current.chars().last().is_some_and(|l| l.is_ascii_digit())) && !current.is_empty() {
            words.push(std::mem::take(&mut current).to_lowercase());
        }
        current.push(c);
    }
    if !current.is_empty() {
        words.push(current.to_lowercase());
    }
    if let Some(quoted) = comment.split('"').nth(1).filter(|q| !q.is_empty()) {
        words.push(quoted.to_string());
    }
    words.dedup();
    words
}

/// The whole icon set for a tool that draws it itself: `{"icons": [{name, set, aliasOf, keywords,
/// viewBox, layers: [{d, stroke, role, opacity, color, transform}]}], "aliases": {alias: glyph},
/// "sizes": {Small: 16…}}` — the Visual Studio icon picker's gallery (`kubuno/icons`).
pub fn catalog_json() -> serde_json::Value {
    use kubuno_drive_desktop_app_controls::LayerRole;
    let catalog = kubuno_drive_desktop_app_controls::themed_icon::catalog();
    // Another name of a glyph (`CheckSquare` for `SquareCheckBig`) is not a tile of its own: it is one
    // more keyword of its glyph, so a search by either name finds the one drawing.
    let mut other_names: std::collections::HashMap<&str, Vec<String>> = std::collections::HashMap::new();
    for entry in catalog.iter().filter(|e| e.alias_of.is_some()) {
        if let Some(target) = entry.alias_of {
            other_names.entry(target).or_default().push(entry.name.to_string());
        }
    }
    let icons: Vec<serde_json::Value> = catalog
        .iter()
        .filter(|entry| entry.alias_of.is_none())
        .filter_map(|entry| {
            let (viewbox, layers) = kubuno_drive_desktop_app_controls::themed_icon::glyph_source(entry.name)?;
            let layers: Vec<serde_json::Value> = layers
                .iter()
                .map(|l| {
                    serde_json::json!({
                        "d": l.path,
                        "stroke": l.stroke,
                        "role": match l.role { LayerRole::Base => "base", LayerRole::Alt => "alt", LayerRole::Accent => "accent", LayerRole::AccentContrast => "accentcontrast" },
                        "opacity": l.opacity,
                        "color": l.color,
                        "transform": l.transform,
                    })
                })
                .collect();
            Some(serde_json::json!({
                "name": entry.name,
                "set": entry.set,
                "aliasOf": entry.alias_of,
                "keywords": keywords(entry.name, entry.comment).into_iter().chain(other_names.get(entry.name).into_iter().flatten().cloned()).collect::<Vec<_>>(),
                "viewBox": viewbox,
                "layers": layers,
            }))
        })
        .collect();
    let aliases: serde_json::Map<String, serde_json::Value> = ALIASES.iter().filter_map(|a| Some(((*a).to_string(), serde_json::Value::from(alias(a)?)))).collect();
    let sizes: serde_json::Map<String, serde_json::Value> = ICON_SIZES.iter().map(|(n, px)| ((*n).to_string(), serde_json::Value::from(*px))).collect();
    serde_json::json!({ "icons": icons, "aliases": aliases, "sizes": sizes })
}

/// Renders icon `value` (anything an icon attribute takes, relative files against `base_dir`) at
/// `size` × `size` pixels in `color` (`#rrggbb`, the colour of a glyph and an SVG's
/// `currentColor`): `{"width", "height", "bgra"}` — straight-alpha BGRA rows, base64 — or
/// `{"error"}`. What the Visual Studio icon picker previews an image file with, so it shows exactly
/// what the application will (`kubuno/renderIcon`).
pub fn render_json(value: &str, base_dir: Option<&Path>, size: u32, color: &str) -> serde_json::Value {
    // SAFETY: COM for WIC on the calling thread; a second call on the same thread is harmless.
    unsafe {
        let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
    }
    install();
    let _scope = enter_base_dir(base_dir);
    let Some(resolved) = resolve(value) else {
        return serde_json::json!({ "error": check(value).err().unwrap_or_else(|| "nothing to draw".to_string()) });
    };
    let [r, g, b, a] = icon_source::parse_hex_color(color).unwrap_or([0.13, 0.13, 0.14, 1.0]);
    let color = windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F { r, g, b, a };
    let size = size.clamp(1, 512);
    match kubuno_desktop_controls::icon_image::rasterize(resolved, size, size, color, Some(&kubuno_drive_desktop_app_controls::Theme::light())) {
        Some(raster) => serde_json::json!({ "width": raster.width, "height": raster.height, "bgra": base64(&raster.straight_alpha()) }),
        None => serde_json::json!({ "error": format!("`{value}` could not be read or drawn") }),
    }
}

/// Standard base64 (RFC 4648, padded). Pure.
pub fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = (u32::from(chunk[0]) << 16) | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8) | u32::from(*chunk.get(2).unwrap_or(&0));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(char::from(ALPHABET[((n >> (18 - 6 * i)) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

// ── Validation ─────────────────────────────────────────────────────────────────────────────────

/// Checks an icon attribute's literal value: empty, a binding or a resource (checked when they
/// resolve), a glyph or alias of the set, or an image file of a supported format. The message
/// suggests the glyph of another case (`save` → `Save`) or a near name.
pub fn check(value: &str) -> Result<(), String> {
    let v = value.trim();
    if v.is_empty() || v.starts_with('{') || glyph(v).is_some() {
        return Ok(());
    }
    if icon_source::is_image_path(v) {
        return Ok(());
    }
    let file = v.rsplit(['/', '\\']).next().unwrap_or(v);
    if file.contains('.') || v.contains(['/', '\\']) {
        return Err(format!(
            "`{v}` is not an image an icon can show: use an SVG, PNG, JPEG, BMP, GIF, ICO, TIFF or WebP file, or a name of the Kubuno icon set"
        ));
    }
    match suggestion(v) {
        Some(near) => Err(format!("`{v}` is not an icon of the Kubuno icon set; did you mean `{near}`?")),
        None => Err(format!("`{v}` is not an icon of the Kubuno icon set (Save, FolderOpen, ChevronDown…) nor an image file")),
    }
}

/// The glyph nearest `name`: the same name in another case or without its dashes, else the one
/// fewest edits away (at most a third of its length).
pub fn suggestion(name: &str) -> Option<&'static str> {
    let squash = |s: &str| s.chars().filter(|c| *c != '-' && *c != '_' && *c != ' ').flat_map(char::to_lowercase).collect::<String>();
    let wanted = squash(name);
    let names: Vec<&'static str> = kubuno_drive_desktop_app_controls::themed_icon::catalog().into_iter().map(|e| e.name).collect();
    if let Some(same) = names.iter().find(|n| squash(n) == wanted) {
        return Some(same);
    }
    let limit = (wanted.chars().count() / 3).max(1);
    names
        .iter()
        .map(|n| (edit_distance(&squash(n), &wanted), *n))
        .filter(|(d, _)| *d <= limit)
        .min_by_key(|(d, _)| *d)
        .map(|(_, n)| n)
}

fn edit_distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut prev = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cur = row[j + 1];
            row[j + 1] = (prev + usize::from(ca != *cb)).min(row[j] + 1).min(cur + 1);
            prev = cur;
        }
    }
    row[b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alias_resolves_to_a_real_lucide_name() {
        assert_eq!(resolve("check"), Some("Check"));
        assert_eq!(resolve("close"), Some("X"));
        assert_eq!(resolve("trash"), Some("Trash2"));
        for a in ALIASES {
            assert!(alias(a).and_then(kubuno_drive_desktop_app_controls::icon_name).is_some(), "{a}");
        }
    }

    #[test]
    fn a_real_lucide_name_passed_directly_still_resolves() {
        assert_eq!(resolve("ChevronsUpDown"), Some("ChevronsUpDown"));
    }

    #[test]
    fn unknown_name_is_none_and_falls_back_when_asked() {
        assert_eq!(resolve("NotAnIcon"), None);
        assert_eq!(resolve_or("NotAnIcon", ""), "");
        assert_eq!(resolve_or("NotAnIcon", "MoreHorizontal"), "MoreHorizontal");
    }

    #[test]
    fn empty_name_is_none() {
        assert_eq!(resolve(""), None);
        assert_eq!(resolve("   "), None);
    }

    #[test]
    fn an_image_file_is_made_absolute_against_the_view() {
        let dir = std::env::temp_dir().join("kubuno-icon-test");
        let _scope = enter_base_dir(Some(&dir));
        let resolved = resolve("images/save.svg").expect("an image file resolves");
        assert_eq!(Path::new(resolved), dir.join("images").join("save.svg"));
        // Absolute paths are kept.
        let abs = dir.join("x.png").to_string_lossy().into_owned();
        assert_eq!(resolve(&abs), Some(icon_source::intern(&abs)));
    }

    #[test]
    fn the_base_folder_is_restored_after_its_scope() {
        {
            let _scope = enter_base_dir(Some(Path::new("C:/views")));
            assert!(resolve("a.png").is_some_and(|p| p.starts_with("C:")));
        }
        assert_eq!(resolve("a.png"), Some("a.png"));
    }

    #[test]
    fn element_options_travel_with_the_value() {
        let attrs = |n: &str| match n {
            "IconColor" => Some("Accent".to_string()),
            "IconSize" => Some("Large".to_string()),
            "IconScaling" => Some("Fill".to_string()),
            _ => None,
        };
        let value = with_options("Save", attrs);
        let spec = icon_source::parse(&value);
        assert_eq!((spec.source, spec.tint, spec.size, spec.scaling), ("Save", Some("Accent"), Some((24.0, 24.0)), IconScaling::Fill));
        // Resolved: the glyph with its options, interned.
        let resolved = resolve(&value).expect("resolves");
        assert_eq!(icon_source::parse(resolved).source, "Save");
        // An alias is replaced by its glyph.
        assert_eq!(icon_source::parse(resolve(&with_options("trash", attrs)).expect("alias")).source, "Trash2");
        // Bindings and resources are left for later.
        assert_eq!(with_options("{Binding Icon}", attrs), "{Binding Icon}");
        assert_eq!(with_options("{Res SaveIcon}", attrs), "{Res SaveIcon}");
        // Nothing to add: the value as written.
        assert_eq!(with_options("Save", |_| None), "Save");
    }

    #[test]
    fn resources_go_through_the_installed_resolver() {
        assert_eq!(resource_key("{Res SaveIcon}"), Some("SaveIcon"));
        assert_eq!(resource_key("{Resource X}"), None);
        assert_eq!(resource_key("{Binding X}"), None);
        set_resource_resolver(|key| (key == "SaveIcon").then(|| "Save".to_string()));
        assert_eq!(resolve("{Res SaveIcon}"), Some("Save"));
        assert_eq!(resolve("{Res Missing}"), None);
        // What a `{Res key}` becomes at run time: never joined to the view's folder.
        let _scope = enter_base_dir(Some(Path::new("C:/views")));
        assert_eq!(resolve("kbres:images/logo"), Some("kbres:images/logo"));
    }

    #[test]
    fn icon_sizes() {
        assert_eq!(parse_icon_size(""), Ok(None));
        assert_eq!(parse_icon_size("Large"), Ok(Some((24.0, 24.0))));
        assert_eq!(parse_icon_size("xlarge"), Ok(Some((32.0, 32.0))));
        assert_eq!(parse_icon_size("18"), Ok(Some((18.0, 18.0))));
        assert_eq!(parse_icon_size("24, 16"), Ok(Some((24.0, 16.0))));
        assert!(parse_icon_size("huge").is_err());
        assert!(parse_icon_size("0").is_err());
        assert!(parse_icon_size("1, 2, 3").is_err());
    }

    #[test]
    fn validation_accepts_the_set_images_bindings_and_resources() {
        for ok in ["", "Save", "trash", "FolderOpen", "images/a.svg", "b.PNG", "c.webp", "{Binding Icon}", "{Res Logo}"] {
            assert_eq!(check(ok), Ok(()), "{ok}");
        }
        let near = check("save2").unwrap_err();
        assert!(near.contains("did you mean `Save`"), "{near}");
        assert!(check("FolderOpne").unwrap_err().contains("FolderOpen"));
        assert!(check("readme.txt").unwrap_err().contains("not an image"));
        assert!(check("Zzzzzzzzzzzz").unwrap_err().contains("not an icon of the Kubuno icon set"));
    }
}
