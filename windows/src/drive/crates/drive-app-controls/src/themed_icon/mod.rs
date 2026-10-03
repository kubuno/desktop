//! The original app's ThemedIcon vector paths, rendered as Direct2D
//! geometries. Path data is extracted verbatim from
//! `Files.App.Controls/ThemedIcon/Styles/*.xaml` (OutlineIconData) into
//! `assets/themed-icons.txt`; the mini-language is SVG path syntax
//! (M/L/H/V/C/S/Z + relatives), 16×16 design units.

use std::collections::HashMap;

use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_BEZIER_SEGMENT, D2D1_COLOR_F, D2D1_FIGURE_BEGIN_FILLED, D2D1_FIGURE_END_CLOSED,
    D2D1_FIGURE_END_OPEN, D2D1_FILL_MODE_ALTERNATE, D2D1_FILL_MODE_WINDING, D2D_SIZE_F,
};
use windows::Win32::Graphics::Direct2D::{
    ID2D1Factory, ID2D1PathGeometry, ID2D1StrokeStyle, D2D1_ARC_SEGMENT, D2D1_ARC_SIZE_LARGE,
    D2D1_ARC_SIZE_SMALL, D2D1_CAP_STYLE_ROUND, D2D1_LINE_JOIN_ROUND, D2D1_STROKE_STYLE_PROPERTIES,
    D2D1_SWEEP_DIRECTION_CLOCKWISE, D2D1_SWEEP_DIRECTION_COUNTER_CLOCKWISE,
};

pub mod data;

pub use data::{LayerRole, ThemedIconColorType, ThemedIconLayers, ThemedIconTypes, ToggleBehaviors};

/// Icon geometry, searched in order. The Files-derived set first (it is what
/// the file explorer draws), then the module glyphs the launcher needs.
const DATA_FILES: [&str; 4] = [
    include_str!("../../assets/themed-icons.txt"),
    // lucide-react (ISC) — the very icons the web sidebar and launcher use.
    include_str!("../../assets/lucide-icons.txt"),
    // The modules' own brand logos, in their own fixed colours.
    include_str!("../../assets/module-logos.txt"),
    // Every other Lucide icon and the package's other names for them (generated, see its header).
    include_str!("../../assets/lucide-all.txt"),
];

/// Every section header of [`DATA_FILES`], first occurrence wins: name → (file, byte offset of its header, alias target).
/// Built once — a lookup by name is then a hash probe rather than a scan of the data.
type Headers = std::collections::HashMap<&'static str, (usize, usize, Option<&'static str>)>;

fn headers() -> &'static Headers {
    static HEADERS: std::sync::OnceLock<Headers> = std::sync::OnceLock::new();
    HEADERS.get_or_init(|| {
        let mut map = std::collections::HashMap::new();
        for (i, data) in DATA_FILES.iter().enumerate() {
            for line in data.lines() {
                let Some(header) = line.trim().strip_prefix("=== ") else { continue };
                let mut words = header.split_whitespace();
                let Some(name) = words.next() else { continue };
                let alias = (words.next() == Some("->")).then(|| words.next()).flatten();
                map.entry(name).or_insert((i, line.as_ptr() as usize - data.as_ptr() as usize, alias));
            }
        }
        map
    })
}

/// A geometric layer ready to paint: its path, role and opacity.
pub struct IconLayer {
    pub geometry: ID2D1PathGeometry,
    pub role: LayerRole,
    pub opacity: f32,
    /// A fixed brand colour (`@#rrggbb`), which overrides the role. Module
    /// logos are multi-coloured with hard-coded fills, exactly as on the web.
    pub color: Option<D2D1_COLOR_F>,
    /// Stroke width in DESIGN units when the layer is drawn as an outline
    /// rather than filled — Lucide icons are strokes, not solid shapes.
    pub stroke: Option<f32>,
    /// The enclosing `<g transform="…">`, as `[a, b, c, d, e, f]` (identity by
    /// default). Kept as a transform so the path data stays verbatim from the
    /// source — a translate is the common case, but a full matrix occurs too.
    pub transform: [f32; 6],
}

#[derive(Default)]
pub struct VectorIcons {
    cache: HashMap<&'static str, Option<(Vec<IconLayer>, f32)>>,
    stroke_style: Option<ID2D1StrokeStyle>,
}

impl VectorIcons {
    /// Round caps and joins — how Lucide draws every icon.
    pub fn stroke_style(&mut self, factory: &ID2D1Factory) -> Option<&ID2D1StrokeStyle> {
        if self.stroke_style.is_none() {
            let props = D2D1_STROKE_STYLE_PROPERTIES {
                startCap: D2D1_CAP_STYLE_ROUND,
                endCap: D2D1_CAP_STYLE_ROUND,
                dashCap: D2D1_CAP_STYLE_ROUND,
                lineJoin: D2D1_LINE_JOIN_ROUND,
                miterLimit: 10.0,
                ..Default::default()
            };
            self.stroke_style = unsafe { factory.CreateStrokeStyle(&props, None).ok() };
        }
        self.stroke_style.as_ref()
    }

    /// All layers of an icon, with its viewbox (16 for ThemedIcon, 20 for
    /// `PathIcons.xaml @20`). A single-layer icon (one line, no `@` prefix)
    /// yields a single `Base` layer.
    pub fn get_layers(&mut self, factory: &ID2D1Factory, name: &'static str) -> Option<(&[IconLayer], f32)> {
        self.cache
            .entry(name)
            .or_insert_with(|| {
                let (layers, viewbox) = section(name)?;
                let built: Vec<IconLayer> = layers
                    .into_iter()
                    .filter_map(|spec| {
                        build_geometry(factory, spec.path).map(|geometry| IconLayer {
                            geometry,
                            role: spec.role,
                            opacity: spec.opacity,
                            color: spec.color,
                            stroke: spec.stroke,
                            transform: spec.transform,
                        })
                    })
                    .collect();
                (!built.is_empty()).then_some((built, viewbox))
            })
            .as_ref()
            .map(|(l, v)| (l.as_slice(), *v))
    }

    /// The `Base` geometry (foreground) and the viewbox — for single-color
    /// callers (the vast majority of icons).
    pub fn get(&mut self, factory: &ID2D1Factory, name: &'static str) -> Option<(&ID2D1PathGeometry, f32)> {
        let (layers, v) = self.get_layers(factory, name)?;
        layers.first().map(|l| (&l.geometry, v))
    }
}

/// The embedded name matching `name`, if this icon exists.
///
/// Drawing takes a `&'static str`, and the names a server sends are runtime
/// strings; this hands back the embedded one. It also answers "do we have this
/// icon?" from the data itself, so no caller has to keep a list in sync.
///
/// Also accepts what [`crate::icon_source`] describes — an image file (`images/save.svg`) and a
/// source followed by drawing options — as the interned `&'static str` the painters take: the
/// painters that know images draw those (the host painter of `kubuno_controls`).
pub fn icon_name(name: &str) -> Option<&'static str> {
    if let Some(found) = embedded_name(name) {
        return Some(found);
    }
    let spec = crate::icon_source::parse(name);
    let known = spec.is_image() || (spec.source.len() != name.len() && embedded_name(spec.source).is_some());
    known.then(|| crate::icon_source::intern(name))
}

/// Every glyph of the embedded set: `(name, set, viewbox, keywords)`, the set being `"Kubuno"`
/// (the Files-derived themed icons), `"Lucide"` or `"Modules"` (the module logos), and the keywords
/// those of the comment above the section (Lucide's own kebab-case name). Aliases
/// (`=== Name -> Target`) are listed with their target's viewbox.
pub fn catalog() -> Vec<CatalogEntry> {
    const SETS: [&str; 4] = ["Kubuno", "Lucide", "Modules", "Lucide"];
    let mut out = Vec::new();
    // A name is listed once, as the lookups see it (the first section of that name).
    let mut seen = std::collections::HashSet::new();
    for (data, set) in DATA_FILES.iter().zip(SETS) {
        let mut comment: &'static str = "";
        for line in data.lines() {
            let line = line.trim();
            if let Some(c) = line.strip_prefix('#') {
                comment = c.trim();
                continue;
            }
            let Some(header) = line.strip_prefix("=== ") else { continue };
            let mut words = header.split_whitespace();
            let Some(name) = words.next() else { continue };
            let alias_of = (words.next() == Some("->")).then(|| words.next()).flatten();
            if !seen.insert(name) {
                comment = "";
                continue;
            }
            out.push(CatalogEntry { name, set, alias_of, comment: std::mem::take(&mut comment) });
        }
    }
    out
}

/// One glyph of [`catalog`].
#[derive(Debug, Clone, Copy)]
pub struct CatalogEntry {
    pub name: &'static str,
    pub set: &'static str,
    /// The glyph it is another name of (`=== Name -> Target`).
    pub alias_of: Option<&'static str>,
    /// The comment line just above its section (`FolderOpen — lucide-react icon "folder-open"`).
    pub comment: &'static str,
}

/// The layers of glyph `name` as their source text, without building any geometry: `(viewbox,
/// layers)`, each layer `(path data, role, opacity, fixed colour as #rrggbb, stroke width, group
/// transform)` — what a tool draws the glyph with outside Direct2D (the Visual Studio icon picker).
pub fn glyph_source(name: &str) -> Option<(f32, Vec<GlyphLayer>)> {
    let mut name = name;
    for _ in 0..4 {
        match alias_of(name) {
            Some(target) => name = target,
            None => break,
        }
    }
    let (file, at, _) = headers().get(name)?;
    let (layers, viewbox) = section_in(&DATA_FILES[*file][*at..], name)?;
    Some((
        viewbox,
        layers
            .into_iter()
            .map(|l| GlyphLayer {
                path: l.path,
                role: l.role,
                opacity: l.opacity,
                color: l.color.map(|c| format!("#{:02x}{:02x}{:02x}", (c.r * 255.0).round() as u8, (c.g * 255.0).round() as u8, (c.b * 255.0).round() as u8)),
                stroke: l.stroke,
                transform: l.transform,
            })
            .collect(),
    ))
}

/// One layer of [`glyph_source`].
#[derive(Clone)]
pub struct GlyphLayer {
    pub path: &'static str,
    pub role: LayerRole,
    pub opacity: f32,
    pub color: Option<String>,
    pub stroke: Option<f32>,
    pub transform: [f32; 6],
}

/// The embedded glyph named exactly `name`.
fn embedded_name(name: &str) -> Option<&'static str> {
    headers().get_key_value(name).map(|(n, _)| *n)
}

/// One parsed layer line, before its geometry is built.
struct LayerSpec {
    path: &'static str,
    role: LayerRole,
    opacity: f32,
    color: Option<D2D1_COLOR_F>,
    stroke: Option<f32>,
    transform: [f32; 6],
}

const IDENTITY: [f32; 6] = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

/// Parses the comma-separated numbers of a `t:`/`m:` prefix.
fn numbers<const N: usize>(s: &str) -> Option<[f32; N]> {
    let mut out = [0.0; N];
    let mut parts = s.split(',');
    for slot in out.iter_mut() {
        *slot = parts.next()?.trim().parse().ok()?;
    }
    parts.next().is_none().then_some(out)
}

/// `#rrggbb` → a colour. Anything malformed is ignored rather than guessed.
fn parse_hex(s: &str) -> Option<D2D1_COLOR_F> {
    let s = s.strip_prefix('#')?;
    if s.len() != 6 {
        return None;
    }
    let v = u32::from_str_radix(s, 16).ok()?;
    Some(D2D1_COLOR_F {
        r: ((v >> 16) & 0xFF) as f32 / 255.0,
        g: ((v >> 8) & 0xFF) as f32 / 255.0,
        b: (v & 0xFF) as f32 / 255.0,
        a: 1.0,
    })
}

/// Parses the layers under a `=== Name @<viewbox> [stroke:<width>]` header.
/// A header of the form `=== Name -> Other` is an alias for `Other`.
///
/// A layer line is `[<prefix>…] <path>`, where each prefix is one of:
///   * `@<role>[@<opacity>]` — themed role (base, alt, accent, accentcontrast);
///   * `@#rrggbb` — a fixed brand colour, overriding the role;
///   * `t:<dx>,<dy>` — offset in design units (an SVG group transform);
///   * `m:<a>,<b>,<c>,<d>,<e>,<f>` — the same, as a full matrix, for the group
///     transforms that scale as well as translate;
///   * `s:<width>` — stroke width in design units, for an outlined layer.
///
/// A line with no prefix is a filled `Base` layer (the legacy form, and what
/// nearly every ThemedIcon uses). `t:`/`s:` always carry the colon, which is
/// what tells them apart from the `t`/`s` SVG path commands.
fn section(name: &str) -> Option<(Vec<LayerSpec>, f32)> {
    // Follow `=== Alias -> Target` hops first. Lucide ships several names for
    // one drawing, and modules use both; an alias line spares us a byte-for-byte
    // copy of the geometry. Bounded, so a cycle in the data cannot hang the UI.
    let mut name = name;
    for _ in 0..4 {
        match alias_of(name) {
            Some(target) => name = target,
            None => break,
        }
    }
    let (file, at, _) = headers().get(name)?;
    section_in(&DATA_FILES[*file][*at..], name)
}

/// The target of `=== <name> -> <target>`, if this section is an alias.
fn alias_of(name: &str) -> Option<&'static str> {
    headers().get(name).and_then(|(_, _, alias)| *alias)
}

fn section_in(data: &'static str, name: &str) -> Option<(Vec<LayerSpec>, f32)> {
    let mut lines = data.lines();
    while let Some(line) = lines.next() {
        let Some(rest) = line.trim().strip_prefix("=== ") else { continue };
        let mut header = rest.split_whitespace();
        let n = header.next()?;
        let mut viewbox = 16.0;
        let mut section_stroke = None;
        for token in header {
            if let Some(v) = token.strip_prefix('@') {
                viewbox = v.parse().unwrap_or(16.0);
            } else if let Some(w) = token.strip_prefix("stroke:") {
                section_stroke = w.parse::<f32>().ok();
            }
        }
        if n != name {
            continue;
        }
        let mut layers: Vec<LayerSpec> = Vec::new();
        for l in lines.by_ref() {
            let l = l.trim();
            if l.is_empty() || l.starts_with("=== ") {
                break;
            }
            if l.starts_with('#') {
                continue; // a comment line
            }
            let mut spec = LayerSpec {
                path: l,
                role: LayerRole::Base,
                opacity: 1.0,
                color: None,
                stroke: section_stroke,
                transform: IDENTITY,
            };
            // Peel the prefixes off the front; the rest is path data.
            while let Some((token, rest)) = spec.path.split_once(' ') {
                if let Some(tag) = token.strip_prefix('@') {
                    match parse_hex(tag) {
                        Some(c) => spec.color = Some(c),
                        None => {
                            let (role, opacity) = match tag.split_once('@') {
                                Some((r, o)) => (r, o.parse().unwrap_or(1.0)),
                                None => (tag, 1.0),
                            };
                            spec.role = match role {
                                "alt" => LayerRole::Alt,
                                "accent" => LayerRole::Accent,
                                "accentcontrast" => LayerRole::AccentContrast,
                                _ => LayerRole::Base,
                            };
                            spec.opacity = opacity;
                        }
                    }
                } else if let Some(t) = token.strip_prefix("t:") {
                    if let Some([dx, dy]) = numbers::<2>(t) {
                        spec.transform = [1.0, 0.0, 0.0, 1.0, dx, dy];
                    }
                } else if let Some(m) = token.strip_prefix("m:") {
                    if let Some(matrix) = numbers::<6>(m) {
                        spec.transform = matrix;
                    }
                } else if let Some(w) = token.strip_prefix("s:") {
                    spec.stroke = w.parse::<f32>().ok();
                } else {
                    break; // path data starts here
                }
                spec.path = rest.trim_start();
            }
            if !spec.path.is_empty() {
                layers.push(spec);
            }
        }
        return (!layers.is_empty()).then_some((layers, viewbox));
    }
    None
}

/// Minimal SVG path parser feeding a D2D geometry sink.
fn build_geometry(factory: &ID2D1Factory, data: &str) -> Option<ID2D1PathGeometry> {
    unsafe {
        let geometry = factory.CreatePathGeometry().ok()?;
        let sink = geometry.Open().ok()?;
        // XAML default fill rule: EvenOdd; an "F1" prefix selects Nonzero.
        let mut data = data.trim();
        let mut fill_mode = D2D1_FILL_MODE_ALTERNATE;
        if let Some(rest) = data.strip_prefix("F1") {
            fill_mode = D2D1_FILL_MODE_WINDING;
            data = rest.trim_start();
        } else if let Some(rest) = data.strip_prefix("F0") {
            data = rest.trim_start();
        }
        sink.SetFillMode(fill_mode);

        let mut chars = data.chars().peekable();
        let mut command = ' ';
        let mut current = windows_numerics::Vector2::default();
        let mut start = windows_numerics::Vector2::default();
        let mut last_c2 = windows_numerics::Vector2::default();
        let mut in_figure = false;

        let read_number = |chars: &mut std::iter::Peekable<std::str::Chars>| -> Option<f32> {
            let mut s = String::new();
            while let Some(&c) = chars.peek() {
                if c == ',' || c.is_whitespace() {
                    chars.next();
                    if !s.is_empty() {
                        break;
                    }
                } else if c == '-' && !s.is_empty() && !s.ends_with('e') {
                    break;
                } else if c.is_ascii_digit() || c == '.' && !s.contains('.') || c == '-' {
                    // A second '.' starts a new number ("1.5.5" = 1.5, 0.5).
                    if c == '.' && s.contains('.') {
                        break;
                    }
                    s.push(c);
                    chars.next();
                } else {
                    break;
                }
            }
            s.parse().ok()
        };

        loop {
            // Next command letter, or implicit repeat of the previous one.
            while let Some(&c) = chars.peek() {
                if c.is_whitespace() || c == ',' {
                    chars.next();
                } else {
                    break;
                }
            }
            let Some(&next) = chars.peek() else { break };
            if next.is_alphabetic() {
                command = next;
                chars.next();
            } else if command == 'm' {
                command = 'l'; // SVG: subsequent implicit pairs after m are lineto
            } else if command == 'M' {
                command = 'L';
            }

            let rel = command.is_lowercase();
            match command.to_ascii_lowercase() {
                'm' => {
                    let x = read_number(&mut chars)?;
                    let y = read_number(&mut chars)?;
                    if in_figure {
                        sink.EndFigure(D2D1_FIGURE_END_OPEN);
                    }
                    current = if rel {
                        windows_numerics::Vector2 { X: current.X + x, Y: current.Y + y }
                    } else {
                        windows_numerics::Vector2 { X: x, Y: y }
                    };
                    start = current;
                    sink.BeginFigure(current, D2D1_FIGURE_BEGIN_FILLED);
                    in_figure = true;
                }
                'l' => {
                    let x = read_number(&mut chars)?;
                    let y = read_number(&mut chars)?;
                    current = if rel {
                        windows_numerics::Vector2 { X: current.X + x, Y: current.Y + y }
                    } else {
                        windows_numerics::Vector2 { X: x, Y: y }
                    };
                    sink.AddLine(current);
                }
                'h' => {
                    let x = read_number(&mut chars)?;
                    current.X = if rel { current.X + x } else { x };
                    sink.AddLine(current);
                }
                'v' => {
                    let y = read_number(&mut chars)?;
                    current.Y = if rel { current.Y + y } else { y };
                    sink.AddLine(current);
                }
                'c' | 's' => {
                    let (p1, p2, p3);
                    if command.eq_ignore_ascii_case(&'c') {
                        let (x1, y1) = (read_number(&mut chars)?, read_number(&mut chars)?);
                        let (x2, y2) = (read_number(&mut chars)?, read_number(&mut chars)?);
                        let (x3, y3) = (read_number(&mut chars)?, read_number(&mut chars)?);
                        p1 = offset(current, x1, y1, rel);
                        p2 = offset(current, x2, y2, rel);
                        p3 = offset(current, x3, y3, rel);
                    } else {
                        // Smooth curve: first control = reflection of last c2.
                        let (x2, y2) = (read_number(&mut chars)?, read_number(&mut chars)?);
                        let (x3, y3) = (read_number(&mut chars)?, read_number(&mut chars)?);
                        p1 = windows_numerics::Vector2 {
                            X: 2.0 * current.X - last_c2.X,
                            Y: 2.0 * current.Y - last_c2.Y,
                        };
                        p2 = offset(current, x2, y2, rel);
                        p3 = offset(current, x3, y3, rel);
                    }
                    sink.AddBezier(&D2D1_BEZIER_SEGMENT { point1: p1, point2: p2, point3: p3 });
                    last_c2 = p2;
                    current = p3;
                    continue; // skip the reset of last_c2 below
                }
                'a' => {
                    // Elliptical arc: rx ry x-rotation large-arc sweep x y.
                    let rx = read_number(&mut chars)?;
                    let ry = read_number(&mut chars)?;
                    let rotation = read_number(&mut chars)?;
                    let large = read_number(&mut chars)?;
                    let sweep = read_number(&mut chars)?;
                    let x = read_number(&mut chars)?;
                    let y = read_number(&mut chars)?;
                    let end = offset(current, x, y, rel);
                    sink.AddArc(&D2D1_ARC_SEGMENT {
                        point: end,
                        size: D2D_SIZE_F { width: rx, height: ry },
                        rotationAngle: rotation,
                        sweepDirection: if sweep != 0.0 {
                            D2D1_SWEEP_DIRECTION_CLOCKWISE
                        } else {
                            D2D1_SWEEP_DIRECTION_COUNTER_CLOCKWISE
                        },
                        arcSize: if large != 0.0 { D2D1_ARC_SIZE_LARGE } else { D2D1_ARC_SIZE_SMALL },
                    });
                    current = end;
                }
                'z' => {
                    if in_figure {
                        sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                        in_figure = false;
                    }
                    current = start;
                }
                _ => {
                    tracing::warn!("unsupported path command '{command}'");
                    return None;
                }
            }
            last_c2 = current;
        }
        if in_figure {
            sink.EndFigure(D2D1_FIGURE_END_OPEN);
        }
        sink.Close().ok()?;
        Some(geometry)
    }
}

fn offset(base: windows_numerics::Vector2, x: f32, y: f32, rel: bool) -> windows_numerics::Vector2 {
    if rel {
        windows_numerics::Vector2 { X: base.X + x, Y: base.Y + y }
    } else {
        windows_numerics::Vector2 { X: x, Y: y }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Names travel from the server as runtime strings; they must resolve
    /// against the embedded data without any hand-kept list.
    #[test]
    fn the_whole_lucide_set_is_embedded_and_indexed() {
        let lucide = catalog().iter().filter(|e| e.set == "Lucide" && e.alias_of.is_none()).count();
        assert!(lucide > 1500, "{lucide} Lucide icons");
        assert_eq!(icon_name("Accessibility"), Some("Accessibility"));
        assert_eq!(icon_name("ALargeSmall"), Some("ALargeSmall"));
        // A generated alias draws its target.
        let (_, layers) = glyph_source("SquareCheckBig").expect("SquareCheckBig");
        assert!(!layers.is_empty());
        // A primitive rewritten as path data (a rect and circles).
        assert!(glyph_source("Accessibility").is_some_and(|(v, l)| v == 24.0 && l.len() == 5));
    }

    #[test]
    fn embedded_names_resolve() {
        assert_eq!(icon_name("Inbox"), Some("Inbox"));
        assert_eq!(icon_name("DriveLogo"), Some("DriveLogo"));
        assert_eq!(icon_name("Folder"), Some("Folder"));
        assert_eq!(icon_name("NoSuchGlyph"), None);
    }

    /// An alias yields the geometry of its target, not an empty icon.
    #[test]
    fn an_alias_resolves_to_its_target() {
        let (aliased, viewbox) = section("ChartColumn").expect("alias resolves");
        let (target, target_viewbox) = section("BarChart3").expect("target exists");
        assert_eq!(aliased.len(), target.len());
        assert_eq!(viewbox, target_viewbox);
        assert!(!aliased.is_empty());
    }

    /// Lucide icons are outlines on a 24 unit grid; a filled rendering would
    /// turn them into blobs.
    #[test]
    fn lucide_icons_are_stroked() {
        let (layers, viewbox) = section("Inbox").expect("Inbox exists");
        assert_eq!(viewbox, 24.0);
        assert!(layers.iter().all(|l| l.stroke == Some(2.0)));
    }

    /// A module logo keeps its own colours, and the Files icons keep none —
    /// they take the caller's.
    #[test]
    fn brand_logos_carry_fixed_colours() {
        let (layers, viewbox) = section("DriveLogo").expect("DriveLogo exists");
        assert_eq!(viewbox, 572.0);
        assert!(layers.iter().all(|l| l.color.is_some()));
        // Its first layer sits in a translated group.
        assert!(layers.iter().any(|l| l.transform != IDENTITY));

        let (plain, _) = section("Folder").expect("Folder exists");
        assert!(plain.iter().all(|l| l.color.is_none() && l.stroke.is_none()));
    }

    /// Every path must actually start with a move — a layer whose prefixes were
    /// mis-parsed would leave path data starting mid-command.
    #[test]
    fn every_layer_starts_with_a_move() {
        for name in ["Inbox", "DriveLogo", "MailLogo", "PaintsharpLogo", "WikiLogo"] {
            let (layers, _) = section(name).expect(name);
            for layer in layers {
                let first = layer.path.trim_start_matches(['F', '0', '1', ' ']).chars().next();
                assert!(
                    matches!(first, Some('M' | 'm')),
                    "{name}: a layer starts with {first:?}, not a move"
                );
            }
        }
    }
}
