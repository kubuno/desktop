//! The **binding source schema** of a view (`vskubuno/docs/DESIGNER.md`, "Data bindings"): what a
//! `{Binding …}` of an element can name, read from the view and its Rust code-behind without
//! building anything — the designer's binding picker, the language server's completion,
//! diagnostics, hover and go-to-definition all answer from it.
//!
//! - **Data context** — the members the view's own view model answers:
//!   - a `#[kubuno::view("x.kbview")]` form class: its `#[bind]` fields (`#[bind("Path")]` names
//!     one), and the paths of its `#[data_context]` field's type (the arms of that type's
//!     `impl ViewModel`);
//!   - a `#[derive(UserControl)]` whose `#[user_control(view = "x.kbview")]` is the view: its
//!     `#[property]` fields (bindable ones first);
//!   - a code-behind with a hand-written `impl ViewModel`: the string patterns of its `fn get`.
//!
//!   A context is **open** when it may answer paths the scan cannot list (a `_ =>` arm that
//!   delegates, a data context type the scan cannot find): unknown paths are then not reported.
//! - **Item** — inside the template of an element that has an `ItemsSource` (a `<Repeater>`'s
//!   item, a `<DataTable>`'s `<Column>`), the fields of a row: the keys of the `d:ItemsSource`
//!   sample file, the columns of a binding source, and the `Row::new().with("Key", Value::…)`
//!   chain of the code-behind that best matches the paths the template binds.
//! - **Data components** — the view's named `BindingSource` (columns from its adapter's
//!   `SelectCommand`, and its navigation members), `ErrorProvider`, `DbConnection`.
//! - **Resources** — the `.kbres` keys (`{Res key}`).
//! - **Converters** — the built-in ones and the project's (`#[value_converter]`,
//!   `register_converter("…")`).
//!
//! Like [`crate::code_behind`], the Rust side is read with a small scanner, never by guessing:
//! every location points at the member's name in its file.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use kubuno_views::ast::{AstNode, Element};
use lsp_types::{Location, Position, Range, Uri};
use serde::{Deserialize, Serialize};

use crate::code_behind::{find_view_model_impl, fns_named, match_bracket, skip_non_code};
use crate::documents::Document;
// The schema types (`Shape`, `MemberKind`, `Member`, `Context`, `ConverterInfo`, `Schema`,
// `Resolution`) and path resolution live in the platform-neutral `kubuno-views-model`
// (`binding_sources`, WV-1), generic over where a member is declared; this server instantiates
// them with LSP locations. The names below are the historical ones.
pub use kubuno_views_model::binding_sources::{MemberKind, Shape};
/// One thing a binding can name (see `kubuno_views_model::binding_sources::Member`).
pub type Member = kubuno_views_model::binding_sources::Member<Location>;
/// The members one level of resolution answers.
pub type Context = kubuno_views_model::binding_sources::Context<Location, Uri>;
/// A converter a binding can name.
pub type ConverterInfo = kubuno_views_model::binding_sources::ConverterInfo<Location>;
/// Everything a binding of one element can name.
pub type Schema = kubuno_views_model::binding_sources::Schema<Location, Uri>;
/// How a path resolves.
pub type Resolution<'a> = kubuno_views_model::binding_sources::Resolution<'a, Location>;

// ── positions ──────────────────────────────────────────────────────────

/// The LSP position (UTF-16 columns) of byte `offset` of `text`.
pub(crate) fn position_in(text: &str, offset: usize) -> Position {
    let offset = offset.min(text.len());
    let line_start = text[..offset].rfind('\n').map_or(0, |n| n + 1);
    let line = text[..line_start].matches('\n').count();
    let character: usize = text[line_start..offset].chars().map(char::len_utf16).sum();
    Position { line: u32::try_from(line).unwrap_or(u32::MAX), character: u32::try_from(character).unwrap_or(u32::MAX) }
}

fn location(path: &Path, text: &str, start: usize, len: usize) -> Option<Location> {
    Some(Location { uri: crate::fs_uri::from_path(path)?, range: Range { start: position_in(text, start), end: position_in(text, start + len) } })
}

// ── the code-behind ────────────────────────────────────────────────────

/// The Rust files of the folder of `view`, the one named like the view first.
fn sibling_sources(view: &Path) -> Vec<PathBuf> {
    let Some(dir) = view.parent() else { return Vec::new() };
    let same = view.with_extension("rs");
    let mut out: Vec<PathBuf> = crate::definition::sibling_rs_files(dir).into_iter().filter(|p| *p != same).collect();
    out.sort();
    if same.is_file() || crate::sources::read(&same).is_some() {
        out.insert(0, same);
    }
    out
}

/// The package's `src/**/*.rs` (bounded), for types and converters declared elsewhere in the crate.
fn package_sources(view: &Path) -> Vec<PathBuf> {
    let Some(root) = crate::project::package_root(view) else { return Vec::new() };
    let mut out = Vec::new();
    let mut stack = vec![root.join("src")];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if path.file_name().is_some_and(|n| n != "target") {
                    stack.push(path);
                }
            } else if path.extension().is_some_and(|e| e == "rs") {
                out.push(path);
                if out.len() >= 600 {
                    return out;
                }
            }
        }
    }
    out.sort();
    out
}

/// Whether `text` names the view file `file_name` in a string literal (`"x.kbview"`, `"views/x.kbview"`).
fn mentions(text: &str, file_name: &str) -> bool {
    text.match_indices(file_name).any(|(at, _)| {
        let before = text[..at].chars().next_back();
        let after = text[at + file_name.len()..].chars().next();
        matches!(before, Some('"' | '/' | '\\')) && after == Some('"')
    })
}

/// The view's code-behind: the sibling `.rs` that names the view (a `#[kubuno::view]` or a
/// `#[user_control(view = …)]`), else the one named like the view.
pub fn code_behind(view: &Path) -> Option<(PathBuf, String)> {
    let file_name = view.file_name()?.to_string_lossy().to_string();
    let sources = sibling_sources(view);
    for path in &sources {
        if let Some(text) = crate::sources::read(path) {
            if mentions(&text, &file_name) {
                return Some((path.clone(), text));
            }
        }
    }
    let same = view.with_extension("rs");
    crate::sources::read(&same).map(|t| (same, t))
}

/// One field of a struct, as written.
#[derive(Debug, Clone)]
struct FieldText {
    name: String,
    name_at: usize,
    ty: String,
    attrs: Vec<String>,
    doc: String,
}

/// The fields of the struct whose body opens at `open` (a scanner: attributes, doc comments,
/// visibility, `name: Type`; commas inside `<…>` do not split).
fn struct_fields(text: &str, open: usize) -> Vec<FieldText> {
    let Some(close) = match_bracket(text, open) else { return Vec::new() };
    let mut out = Vec::new();
    let mut i = open + 1;
    let b = text.as_bytes();
    let mut attrs: Vec<String> = Vec::new();
    let mut doc = String::new();
    while i < close {
        let c = b[i];
        if c.is_ascii_whitespace() || c == b',' {
            i += 1;
            continue;
        }
        if text[i..].starts_with("///") {
            let end = text[i..].find('\n').map_or(close, |n| i + n);
            let line = text[i + 3..end].trim();
            if !doc.is_empty() {
                doc.push(' ');
            }
            doc.push_str(line);
            i = end;
            continue;
        }
        if let Some(next) = skip_non_code(text, i).filter(|n| *n > i) {
            i = next;
            continue;
        }
        if text[i..].starts_with("#[") {
            let Some(end) = match_bracket(text, i + 1) else { break };
            attrs.push(text[i + 2..end].trim().to_string());
            i = end + 1;
            continue;
        }
        // `pub`, `pub(crate)`…
        if text[i..].starts_with("pub") && !b.get(i + 3).is_some_and(|c| c.is_ascii_alphanumeric() || *c == b'_') {
            i += 3;
            while i < close && b[i].is_ascii_whitespace() {
                i += 1;
            }
            if b.get(i) == Some(&b'(') {
                i = match_bracket(text, i).map_or(close, |e| e + 1);
            }
            continue;
        }
        // `name: Type`
        let name_at = i;
        while i < close && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
            i += 1;
        }
        let name = text[name_at..i].to_string();
        while i < close && b[i].is_ascii_whitespace() {
            i += 1;
        }
        if name.is_empty() || b.get(i) != Some(&b':') {
            // Not a field: skip to the next comma.
            i = (i + 1).max(name_at + 1);
            continue;
        }
        i += 1;
        let ty_at = i;
        let mut angle = 0i32;
        while i < close {
            if let Some(next) = skip_non_code(text, i).filter(|n| *n > i) {
                i = next;
                continue;
            }
            match b[i] {
                b'<' => angle += 1,
                b'>' if i > 0 && b[i - 1] != b'-' => angle -= 1,
                b'(' | b'[' | b'{' => {
                    i = match_bracket(text, i).unwrap_or(close);
                }
                b',' if angle <= 0 => break,
                _ => {}
            }
            i += 1;
        }
        out.push(FieldText { name, name_at, ty: text[ty_at..i.min(close)].trim().to_string(), attrs: std::mem::take(&mut attrs), doc: std::mem::take(&mut doc) });
    }
    out
}

/// `snake_case` → `PascalCase` (the path a `#[bind]` field answers, the attribute of a property).
pub fn pascal(name: &str) -> String {
    name.split('_').filter(|p| !p.is_empty()).map(|p| {
        let mut c = p.chars();
        c.next().map(|f| f.to_uppercase().chain(c).collect::<String>()).unwrap_or_default()
    }).collect()
}

/// The body offset (`{`) of `struct name` in `text`.
fn struct_body(text: &str, name: &str) -> Option<usize> {
    for at in crate::code_behind::ident_occurrences(text, "struct") {
        let rest = &text[at + "struct".len()..];
        let trimmed = rest.trim_start();
        if !trimmed.starts_with(name) || trimmed[name.len()..].chars().next().is_some_and(|c| c.is_alphanumeric() || c == '_') {
            continue;
        }
        let from = at + "struct".len() + (rest.len() - trimmed.len()) + name.len();
        let open = from + text[from..].find(['{', ';'])?;
        return (text.as_bytes()[open] == b'{').then_some(open);
    }
    None
}

/// The arms of `fn get` of the `impl ViewModel for` `self_ty` (any one when `self_ty` is
/// `None`): `(path, offset, shape)`, and whether a `_ =>` arm may answer more.
/// An arm of `fn get`: its path, the offset of the path, the shape it builds.
type Arm = (String, usize, Shape);
/// The arms of an `impl ViewModel`, whether a `_ =>` arm answers more, and the type it is for.
type ArmScan = (Vec<Arm>, bool, String);
/// [`ArmScan`] of a type found in a file: the arms, open, the file and its text.
type TypeArms = (Vec<Arm>, bool, PathBuf, String);
/// A sample file: its path, its text, its fields.
type Sample = (PathBuf, String, Vec<(String, Shape)>);

fn view_model_arms(text: &str, self_ty: Option<&str>) -> Option<ArmScan> {
    let imp = find_view_model_impl(text)?;
    if self_ty.is_some_and(|t| imp.self_ty.rsplit("::").next().unwrap_or(&imp.self_ty).split('<').next() != Some(t)) {
        return None;
    }
    let mut arms = Vec::new();
    let mut open = false;
    let body = &text[imp.open..=imp.close];
    for (_, _, f_open, f_close) in fns_named(body, "get") {
        let (found, wildcard) = match_arms(&body[f_open..=f_close]);
        open |= wildcard;
        for (path, at, shape) in found {
            if !arms.iter().any(|(p, _, _): &(String, usize, Shape)| *p == path) {
                arms.push((path, imp.open + f_open + at, shape));
            }
        }
    }
    Some((arms, open, imp.self_ty))
}

/// The string patterns of a `match` (followed by `=>`, `|` or an `if` guard), their offsets and
/// the shape their arm builds, and whether a `_ =>` arm answers something else than `None`.
fn match_arms(code: &str) -> (Vec<(String, usize, Shape)>, bool) {
    let bytes = code.as_bytes();
    let mut out = Vec::new();
    let mut wildcard = false;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'_' && (i == 0 || !(bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'_')) && !bytes.get(i + 1).is_some_and(|c| c.is_ascii_alphanumeric() || *c == b'_') {
            let after = code[i + 1..].trim_start();
            if let Some(arm) = after.strip_prefix("=>") {
                let arm = arm.trim_start();
                let end = arm.find([',', '\n', '}']).unwrap_or(arm.len());
                let value = arm[..end].trim();
                if !(value == "None" || value.is_empty() || value == "{" || value.starts_with("::core::option::Option::None") || value.starts_with("Option::None")) {
                    wildcard = true;
                }
            }
            i += 1;
            continue;
        }
        if bytes[i] != b'"' {
            if let Some(next) = skip_non_code(code, i).filter(|n| *n > i) {
                i = next;
                continue;
            }
            i += 1;
            continue;
        }
        let start = i + 1;
        let end = skip_non_code(code, i).unwrap_or(bytes.len());
        let value = code[start..end.saturating_sub(1).max(start)].to_string();
        let after = code[end.min(code.len())..].trim_start();
        let is_pattern = after.starts_with("=>") || (after.starts_with('|') && !after.starts_with("||")) || after.starts_with("if ");
        if is_pattern && !value.is_empty() && !value.contains('\\') {
            // The arm's body: up to the next arm (a comma or a closing brace at depth 0).
            let arm_at = end + code[end..].find("=>").unwrap_or(0);
            let shape = arm_shape(&code[arm_at..]);
            out.push((value, start, shape));
        }
        i = end;
    }
    (out, wildcard)
}

/// The shape of the first `Value::Variant` of an arm (up to the next arm).
fn arm_shape(arm: &str) -> Shape {
    let stop = arm.find("\n        \"").or_else(|| arm.find("=>\n")).unwrap_or(arm.len()).min(400).min(arm.len());
    let mut s = &arm[..stop];
    if let Some(next_arm) = s[2.min(s.len())..].find("=>") {
        s = &s[..next_arm + 2];
    }
    match s.find("Value::") {
        Some(at) => {
            let v: String = s[at + 7..].chars().take_while(|c| c.is_alphanumeric()).collect();
            Shape::of_value_variant(&v)
        }
        None => Shape::Any,
    }
}

/// The paths of a hand-written `impl ViewModel` in `text` (for `kubuno/bindingPaths`).
pub fn legacy_paths(text: &str) -> Option<Vec<String>> {
    view_model_arms(text, None).map(|(arms, _, _)| arms.into_iter().map(|(p, _, _)| p).collect())
}

/// The data context of `view` (see the module doc).
pub fn data_context(view: &Path) -> Context {
    let Some((path, text)) = code_behind(view) else { return Context { label: String::new(), members: Vec::new(), open: true, file: None } };
    let file = crate::fs_uri::from_path(&path);
    let file_name = view.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    // A `#[kubuno::view]` form class.
    if let Some((name, _)) = crate::code_behind::find_view_struct(&text) {
        if let Some(open) = struct_body(&text, &name) {
            let mut ctx = Context { label: name.clone(), members: Vec::new(), open: false, file: file.clone() };
            for f in struct_fields(&text, open) {
                for attr in &f.attrs {
                    if attr == "bind" || attr.starts_with("bind(") || attr.starts_with("bind (") {
                        let p = attr.split_once('(').and_then(|(_, a)| a.trim_end_matches(')').trim().strip_prefix('"')).and_then(|a| a.strip_suffix('"')).map(str::to_string).unwrap_or_else(|| pascal(&f.name));
                        let mut m = Member::new(&p, &p, MemberKind::Field, Shape::of_rust(&f.ty));
                        m.rust_type = Some(f.ty.clone());
                        m.doc = f.doc.clone();
                        m.location = location(&path, &text, f.name_at, f.name.len());
                        ctx.members.push(m);
                    } else if attr == "data_context" {
                        let ty = f.ty.rsplit("::").next().unwrap_or(&f.ty).split('<').next().unwrap_or("").trim().to_string();
                        match arms_of_type(view, &ty) {
                            Some((arms, open, file_path, file_text)) => {
                                ctx.open |= open;
                                for (p, at, shape) in arms {
                                    let mut m = Member::new(&p, &p, MemberKind::Path, shape);
                                    m.doc = format!("{} (`{}`)", f.name, ty);
                                    m.location = location(&file_path, &file_text, at, p.len());
                                    ctx.members.push(m);
                                }
                            }
                            None => ctx.open = true,
                        }
                    }
                }
            }
            return ctx;
        }
    }
    // A user control.
    let scan = kubuno_views_meta::scan_source(&text);
    if let Some(uc) = scan.components.iter().find(|c| c.view.as_deref().is_some_and(|v| v.replace('\\', "/").rsplit('/').next() == Some(file_name.as_str()))) {
        let mut ctx = Context { label: uc.name.clone(), members: Vec::new(), open: false, file: file.clone() };
        let mut props: Vec<&kubuno_views_meta::PropertyDecl> = uc.properties.iter().collect();
        props.sort_by_key(|p| !p.bindable);
        // The fields of the user control's own struct (another struct of the file may have a field of the same name).
        let fields: Vec<FieldText> = struct_body(&text, &uc.name).map(|open| struct_fields(&text, open)).unwrap_or_default();
        for p in props {
            let mut m = Member::new(&p.name, &p.name, MemberKind::Property, Shape::of_rust(&p.ty));
            if matches!(p.kind, kubuno_views_meta::ValueKind::Enum(_)) {
                m.shape = Shape::Text;
            }
            m.rust_type = Some(p.ty.clone());
            m.doc = p.description.clone();
            m.location = match fields.iter().find(|f| f.name == p.field) {
                Some(f) => location(&path, &text, f.name_at, f.name.len()),
                None => field_location(&path, &text, &p.field),
            };
            ctx.members.push(m);
        }
        return ctx;
    }
    // A hand-written view model.
    if let Some((arms, open, self_ty)) = view_model_arms(&text, None) {
        let mut ctx = Context { label: self_ty, members: Vec::new(), open, file };
        for (p, at, shape) in arms {
            let mut m = Member::new(&p, &p, MemberKind::Path, shape);
            m.location = location(&path, &text, at, p.len());
            ctx.members.push(m);
        }
        return ctx;
    }
    Context { label: String::new(), members: Vec::new(), open: true, file }
}

/// Where `field: …` is declared in `text`.
fn field_location(path: &Path, text: &str, field: &str) -> Option<Location> {
    let at = crate::code_behind::ident_occurrences(text, field).into_iter().find(|&at| {
        let after = text[at + field.len()..].trim_start();
        after.starts_with(':') && !after.starts_with("::")
    })?;
    location(path, text, at, field.len())
}

/// The arms of `impl ViewModel for ty` anywhere in the package of `view`.
fn arms_of_type(view: &Path, ty: &str) -> Option<TypeArms> {
    for path in sibling_sources(view).into_iter().chain(package_sources(view)) {
        let Some(text) = crate::sources::read(&path) else { continue };
        if !text.contains(ty) {
            continue;
        }
        if let Some((arms, open, _)) = view_model_arms(&text, Some(ty)) {
            return Some((arms, open, path, text));
        }
    }
    None
}

// ── items ──────────────────────────────────────────────────────────────

/// The element whose `ItemsSource` gives `element` its row (the nearest ancestor that has one), if any.
pub fn items_owner(element: &Element) -> Option<Element> {
    let mut node = element.syntax().parent();
    while let Some(n) = node {
        if let Some(e) = Element::cast(n.clone()) {
            if e.attribute("ItemsSource").is_some() || e.attribute("d:ItemsSource").is_some() {
                return Some(e);
            }
        }
        node = n.parent();
    }
    None
}

/// The first path segments of the `{Binding …}` values inside `owner` (not its own).
fn template_paths(owner: &Element) -> Vec<String> {
    owner
        .syntax()
        .descendants()
        .filter_map(Element::cast)
        .filter(|e| e.syntax() != owner.syntax())
        .flat_map(|e| e.attributes().collect::<Vec<_>>())
        .filter_map(|a| a.value())
        .filter_map(|v| kubuno_views::binding::parse_binding(&v))
        .map(|s| s.path)
        .collect()
}

/// The `Row::new().with("Key", Value::…)` chains of `text`: their fields, offsets and shapes.
fn row_chains(text: &str) -> Vec<Vec<(String, usize, Shape)>> {
    let mut out = Vec::new();
    for (at, _) in text.match_indices("Row::new()") {
        let mut i = at + "Row::new()".len();
        let mut fields = Vec::new();
        loop {
            let rest = &text[i..];
            let skipped = rest.len() - rest.trim_start().len();
            let j = i + skipped;
            if !text[j..].starts_with(".with(") {
                break;
            }
            let open = j + ".with".len();
            let Some(close) = match_bracket(text, open) else { break };
            let args = &text[open + 1..close];
            let a = args.trim_start();
            if let Some(lit) = a.strip_prefix('"') {
                if let Some(end) = lit.find('"') {
                    let key = &lit[..end];
                    let key_at = open + 1 + (args.len() - a.len()) + 1;
                    let value = lit[end + 1..].trim_start().trim_start_matches(',').trim_start();
                    let shape = value.strip_prefix("Value::").map(|v| Shape::of_value_variant(&v.chars().take_while(|c| c.is_alphanumeric()).collect::<String>())).unwrap_or(Shape::Any);
                    fields.push((key.to_string(), key_at, shape));
                }
            }
            i = close + 1;
        }
        if !fields.is_empty() {
            out.push(fields);
        }
    }
    out
}

/// The rows of a sample file (`d:ItemsSource`): its keys (of every object), shapes and positions.
fn sample_fields(view: &Path, sample: &str) -> Option<Sample> {
    let path = view.parent()?.join(sample);
    let text = crate::sources::read(&path)?;
    let rows: Vec<serde_json::Map<String, serde_json::Value>> = serde_json::from_str(&text).ok()?;
    let mut fields: Vec<(String, Shape)> = Vec::new();
    for row in &rows {
        for (k, v) in row {
            if !fields.iter().any(|(f, _)| f == k) {
                fields.push((k.clone(), Shape::of_json(v)));
            }
        }
    }
    Some((path, text, fields))
}

/// The row context of the template `owner` holds.
fn item_context(view: &Path, owner: &Element, components: &[Member], code: Option<&(PathBuf, String)>) -> Context {
    let source = owner.attribute("ItemsSource").and_then(|a| a.value()).unwrap_or_default();
    let source_path = kubuno_views::binding::parse_binding(&source).map(|s| s.path).unwrap_or_default();
    let owner_name = owner.name().unwrap_or_default();
    let mut ctx = Context { label: format!("{owner_name} · {source_path}"), members: Vec::new(), open: true, file: None };
    // A binding source's rows: its columns.
    if let Some(c) = components.iter().find(|c| c.path == source_path) {
        let columns: Vec<Member> = c.children.iter().filter(|m| m.kind == MemberKind::Column).cloned().collect();
        if !columns.is_empty() {
            ctx.open = false;
            for mut m in columns {
                m.path = m.name.clone();
                m.expression = format!("{{Binding {}}}", m.name);
                m.kind = MemberKind::RowField;
                ctx.members.push(m);
            }
        }
        return ctx;
    }
    // The sample rows of the designer.
    if let Some(sample) = owner.attribute("d:ItemsSource").and_then(|a| a.value()) {
        if let Some((file, text, fields)) = sample_fields(view, &sample) {
            ctx.open = false;
            ctx.file = crate::fs_uri::from_path(&file);
            for (k, shape) in fields {
                let mut m = Member::new(&k, &k, MemberKind::RowField, shape);
                m.doc = format!("d:ItemsSource ({sample})");
                m.location = text.find(&format!("\"{k}\"")).and_then(|at| location(&file, &text, at + 1, k.len()));
                ctx.members.push(m);
            }
        }
    }
    // The code-behind's row chain that best matches what the template binds.
    if let Some((file, text)) = code {
        let wanted: Vec<String> = template_paths(owner).into_iter().map(|p| p.split('.').next().unwrap_or("").to_string()).collect();
        let best = row_chains(text)
            .into_iter()
            .map(|chain| (chain.iter().filter(|(k, _, _)| wanted.contains(k)).count(), chain))
            .filter(|(score, _)| *score > 0)
            .max_by_key(|(score, chain)| (*score, chain.len()));
        if let Some((_, chain)) = best {
            ctx.open = false;
            for (k, at, shape) in chain {
                match ctx.members.iter_mut().find(|m| m.path == k) {
                    Some(m) => {
                        m.location = location(file, text, at, k.len());
                        if m.shape == Shape::Any {
                            m.shape = shape;
                        }
                    }
                    None => {
                        let mut m = Member::new(&k, &k, MemberKind::RowField, shape);
                        m.location = location(file, text, at, k.len());
                        ctx.members.push(m);
                    }
                }
            }
        }
    }
    ctx
}

// ── data components ────────────────────────────────────────────────────

/// The column names of a `SELECT a, b AS c FROM …` (`None` for `*` or a text it does not read).
pub fn select_columns(sql: &str) -> Option<Vec<String>> {
    let lower = sql.to_ascii_lowercase();
    let select = lower.find("select")? + "select".len();
    let from = lower[select..].find(" from ").map(|n| select + n)?;
    let list = &sql[select..from];
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut current = String::new();
    for c in list.chars().chain(std::iter::once(',')) {
        match c {
            '(' => {
                depth += 1;
                current.push(c);
            }
            ')' => {
                depth -= 1;
                current.push(c);
            }
            ',' if depth == 0 => {
                let item = current.trim().to_string();
                current.clear();
                if item.is_empty() {
                    continue;
                }
                if item == "*" || item.ends_with(".*") {
                    return None;
                }
                let lower_item = item.to_ascii_lowercase();
                let name = match lower_item.rfind(" as ") {
                    Some(at) => item[at + 4..].trim().to_string(),
                    None => item.rsplit(['.', ' ']).next().unwrap_or(&item).to_string(),
                };
                out.push(name.trim_matches(|c| c == '"' || c == '[' || c == ']' || c == '`').to_string());
            }
            _ => current.push(c),
        }
    }
    let lower_first = lower[..select - "select".len()].trim();
    if !lower_first.is_empty() {
        return None;
    }
    (!out.is_empty()).then_some(out)
}

/// The navigation members of a binding source (`vskubuno/docs/DATA.md` §7).
const SOURCE_STATE: &[(&str, Shape, bool, &str)] = &[
    ("Position", Shape::Number, true, "The current row's index (two-way: SelectedIndex of a list)."),
    ("Count", Shape::Number, false, "The number of rows."),
    ("PositionText", Shape::Text, false, "« 3 / 12 »."),
    ("HasChanges", Shape::Bool, false, "Whether rows were changed and not saved."),
    ("IsEditing", Shape::Bool, false, "Whether the current row is being edited."),
    ("CanMovePrevious", Shape::Bool, false, "Whether there is a previous row."),
    ("CanMoveNext", Shape::Bool, false, "Whether there is a next row."),
];

/// The named data components of the view, with their members.
pub fn components(view: &Path, root: &Element) -> Vec<Member> {
    let elements: Vec<Element> = root.syntax().descendants().filter_map(Element::cast).collect();
    let text = root.syntax().ancestors().last().map(|d| d.text().to_string()).unwrap_or_default();
    let name_of =|e: &Element| e.attribute("x:Name").and_then(|a| a.value()).filter(|n| !n.is_empty());
    let mut out = Vec::new();
    for e in &elements {
        let Some(class) = e.name() else { continue };
        let Some(name) = name_of(e) else { continue };
        let mut c = Member::new(&name, &name, MemberKind::Component, Shape::Any);
        c.rust_type = Some(class.clone());
        c.expression = format!("{{Binding Source={name}}}");
        c.writable = false;
        // F12 on `Source=name`: the component's `x:Name` in this view.
        if let Some(range) = e.attribute("x:Name").and_then(|a| a.value_range()) {
            c.location = location(view, &text, usize::from(range.start()), name.len());
        }
        match class.as_str() {
            "BindingSource" => {
                c.shape = Shape::List;
                c.doc = "BindingSource".into();
                let adapter = e.attribute("DataSource").and_then(|a| a.value()).unwrap_or_default();
                let sql = elements.iter().find(|a| name_of(a).as_deref() == Some(adapter.as_str())).and_then(|a| a.attribute("SelectCommand")).and_then(|a| a.value());
                if let Some(columns) = sql.as_deref().and_then(select_columns) {
                    for col in columns {
                        let path = format!("{name}.{col}");
                        let mut m = Member::new(&col, &path, MemberKind::Column, Shape::Any);
                        m.expression = format!("{{Binding Source={name}, Path={col}}}");
                        m.doc = format!("{adapter} · SelectCommand");
                        c.children.push(m);
                    }
                }
                for (state, shape, writable, doc) in SOURCE_STATE {
                    let path = format!("{name}.{state}");
                    let mut m = Member::new(state, &path, MemberKind::State, *shape);
                    m.expression = format!("{{Binding Source={name}, Path={state}}}");
                    m.writable = *writable;
                    m.doc = doc.to_string();
                    c.children.push(m);
                }
            }
            "ErrorProvider" => {
                c.doc = "ErrorProvider".into();
                for (state, shape, doc) in [("HasErrors", Shape::Bool, "Whether the current row has errors."), ("Summary", Shape::Text, "Every error of the current row, one per line.")] {
                    let path = format!("{name}.{state}");
                    let mut m = Member::new(state, &path, MemberKind::State, shape);
                    m.expression = format!("{{Binding Source={name}, Path={state}}}");
                    m.writable = false;
                    m.doc = doc.into();
                    c.children.push(m);
                }
            }
            "DbConnection" => {
                c.doc = "DbConnection".into();
                let path = format!("{name}.State");
                let mut m = Member::new("State", &path, MemberKind::State, Shape::Text);
                m.expression = format!("{{Binding Source={name}, Path=State}}");
                m.writable = false;
                c.children.push(m);
            }
            // The storage components (vskubuno docs/STORAGE-COMPONENTS.md): the settings of the `.kbsettings`
            // file; a secret store tells only whether it can be used (`<Name>.Exists` is open); a Registry key's
            // values are not known before run time.
            "Settings" => crate::storage::settings_members(view, e, &name, &mut c),
            "SecretStore" => {
                c.doc = "SecretStore".into();
                let mut m = Member::new("Available", &format!("{name}.Available"), MemberKind::State, Shape::Bool);
                m.expression = format!("{{Binding Source={name}, Path=Available}}");
                m.writable = false;
                m.doc = "Whether the credential store can be used (secrets themselves are never bound: `<Name>.Exists`).".into();
                c.children.push(m);
            }
            "RegistryKey" => c.doc = "RegistryKey".into(),
            // A key/value store's keys are not known before run time (open).
            "KeyValueStore" => c.doc = "KeyValueStore".into(),
            "FileStore" => {
                c.doc = "FileStore".into();
                for (state, shape, doc) in [
                    ("Count", Shape::Number, "How many files the store holds."),
                    ("Size", Shape::Number, "The bytes the store holds."),
                    ("Files", Shape::List, "The files (rows with Name and Size)."),
                ] {
                    let path = format!("{name}.{state}");
                    let mut m = Member::new(state, &path, MemberKind::State, shape);
                    m.expression = format!("{{Binding Source={name}, Path={state}}}");
                    m.writable = false;
                    m.doc = doc.into();
                    c.children.push(m);
                }
            }
            "LocalDatabase" => {
                c.doc = "LocalDatabase".into();
                let path = format!("{name}.State");
                let mut m = Member::new("State", &path, MemberKind::State, Shape::Text);
                m.expression = format!("{{Binding Source={name}, Path=State}}");
                m.writable = false;
                c.children.push(m);
            }
            _ => continue,
        }
        out.push(c);
    }
    out
}

// ── converters and resources ───────────────────────────────────────────

/// The built-in converters and those the package of `view` declares.
pub fn converters(view: &Path) -> Vec<ConverterInfo> {
    let mut out: Vec<ConverterInfo> = kubuno_views::binding::BUILTIN_CONVERTERS
        .iter()
        .map(|b| ConverterInfo { name: b.name.to_string(), output: Shape::of_name(b.output), two_way: b.two_way, doc: b.doc.to_string(), project: false, location: None })
        .collect();
    for path in package_sources(view) {
        for (name, location) in cached_converters(&path) {
            out.retain(|c| c.name != name);
            out.push(ConverterInfo { name, output: Shape::Any, two_way: true, doc: String::new(), project: true, location });
        }
    }
    out
}

type ConverterScan = Vec<(String, Option<Location>)>;
type FileStamp = (Option<std::time::SystemTime>, u64);
type ConverterCache = HashMap<PathBuf, (FileStamp, ConverterScan)>;

thread_local! {
    /// The converters of each scanned file, with the file's (modified time, length) when it was read.
    static CONVERTERS: std::cell::RefCell<ConverterCache> = std::cell::RefCell::new(HashMap::new());
}

/// The converters `path` declares, read again only when the file changed (every diagnostic pass
/// asks for the whole package's).
fn cached_converters(path: &Path) -> ConverterScan {
    let meta = std::fs::metadata(path).ok();
    let stamp = (meta.as_ref().and_then(|m| m.modified().ok()), meta.map_or(0, |m| m.len()));
    if let Some(found) = CONVERTERS.with(|c| c.borrow().get(path).filter(|(s, _)| *s == stamp).map(|(_, v)| v.clone())) {
        return found;
    }
    let found: ConverterScan = crate::sources::read(path)
        .map(|text| project_converters(&text).into_iter().map(|(name, at, len)| (name, location(path, &text, at, len))).collect())
        .unwrap_or_default();
    CONVERTERS.with(|c| c.borrow_mut().insert(path.to_path_buf(), (stamp, found.clone())));
    found
}

/// The converters `text` declares: `#[value_converter("N")]` / `#[value_converter] impl … for T`,
/// `register_converter("N", …)`, `register_value_converter!("N", …)`: `(name, offset, length)`.
fn project_converters(text: &str) -> Vec<(String, usize, usize)> {
    let mut out = Vec::new();
    for (at, _) in text.match_indices("value_converter") {
        let before = text[..at].trim_end();
        let attr = before.ends_with("#[") || before.ends_with("::");
        let after = &text[at + "value_converter".len()..];
        if after.starts_with('!') || !attr {
            continue;
        }
        let a = after.trim_start();
        if let Some(lit) = a.strip_prefix("(\"") {
            if let Some(end) = lit.find('"') {
                let lit_at = at + "value_converter".len() + (after.len() - a.len()) + 2;
                out.push((lit[..end].to_string(), lit_at, end));
                continue;
            }
        }
        // `impl ValueConverter for T`
        if let Some(for_at) = after.find(" for ") {
            let rest = &after[for_at + 5..];
            let name: String = rest.trim_start().chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
            if !name.is_empty() {
                let name_at = at + "value_converter".len() + for_at + 5 + (rest.len() - rest.trim_start().len());
                let len = name.len();
                out.push((name, name_at, len));
            }
        }
    }
    for needle in ["register_converter(\"", "register_value_converter!(\""] {
        for (at, _) in text.match_indices(needle) {
            let lit = &text[at + needle.len()..];
            if let Some(end) = lit.find('"') {
                out.push((lit[..end].to_string(), at + needle.len(), end));
            }
        }
    }
    out
}

/// The `.kbres` keys of the package of `view`.
pub fn resources(view: &Path) -> Vec<Member> {
    let index = crate::resources::index_for(view);
    let duplicate = |name: &str| index.items.iter().filter(|i| i.entry.name == name).count() > 1;
    index
        .items
        .iter()
        .map(|item| {
            let name = &item.entry.name;
            let mut m = Member::new(name, name, MemberKind::Resource, Shape::Text);
            m.expression = if duplicate(name) { format!("{{Res {name}, Source={}}}", item.set) } else { format!("{{Res {name}}}") };
            m.writable = false;
            m.rust_type = Some(format!("{}.kbres", item.set));
            m.doc = item.entry.comment.clone().unwrap_or_default();
            m.location = crate::fs_uri::from_path(&item.file).map(|uri| Location { uri, range: Range { start: item.position, end: item.position } });
            m
        })
        .collect()
}

// ── the whole schema ───────────────────────────────────────────────────

/// What the schema of a document is computed from, cached per request: the data context and the
/// components do not depend on the element.
pub struct ViewSources {
    pub view: PathBuf,
    pub context: Context,
    pub components: Vec<Member>,
    pub converters: Vec<ConverterInfo>,
    code: Option<(PathBuf, String)>,
    items: HashMap<String, Context>,
}

impl ViewSources {
    /// Reads the sources of the view at `view` (`doc` is its text).
    pub fn read(view: &Path, doc: &Document) -> Self {
        let root = kubuno_views::ast::Document::cast(doc.parse.syntax()).and_then(|d| d.root_element());
        let components = root.as_ref().map(|r| components(view, r)).unwrap_or_default();
        Self { view: view.to_path_buf(), context: data_context(view), components, converters: converters(view), code: code_behind(view), items: HashMap::new() }
    }

    /// The schema of `element` (its item context when it is in a template); `resources` too when asked.
    pub fn schema_for(&mut self, element: Option<&Element>, with_resources: bool) -> Schema {
        let item = element.and_then(items_owner).map(|owner| {
            let key = owner.stable_id();
            if let Some(ctx) = self.items.get(&key) {
                return ctx.clone();
            }
            let ctx = item_context(&self.view, &owner, &self.components, self.code.as_ref());
            self.items.insert(key, ctx.clone());
            ctx
        });
        Schema {
            context: self.context.clone(),
            item,
            components: self.components.clone(),
            resources: if with_resources { resources(&self.view) } else { Vec::new() },
            converters: self.converters.clone(),
        }
    }
}

// ── `kubuno/bindingSources` ────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BindingSourcesParams {
    pub uri: Uri,
    /// The element whose bindings are edited (its item context), when there is one.
    #[serde(default)]
    pub element_id: Option<String>,
    #[serde(default)]
    pub open_files: HashMap<String, String>,
}

/// The answer: the schema, and the binding problems of the element's attributes.
#[derive(Debug, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct BindingSourcesResult {
    pub schema: Schema,
    pub issues: Vec<crate::binding_lsp::AttributeIssue>,
}

/// `kubuno/bindingSources` (call inside [`crate::sources::with_overlays`]).
pub fn binding_sources(doc: Option<&Document>, p: &BindingSourcesParams) -> BindingSourcesResult {
    let (Some(doc), Some(view)) = (doc, crate::fs_uri::to_path(&p.uri)) else { return BindingSourcesResult::default() };
    let mut sources = ViewSources::read(&view, doc);
    let ast = kubuno_views::ast::Document::cast(doc.parse.syntax());
    let element = p.element_id.as_deref().and_then(|id| ast.as_ref()?.resolve_id(id));
    let schema = sources.schema_for(element.as_ref(), true);
    let issues = element.as_ref().map(|e| crate::binding_lsp::element_issues(doc, &mut sources, e)).unwrap_or_default();
    BindingSourcesResult { schema, issues }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shapes_of_rust_types() {
        assert_eq!(Shape::of_rust("bool"), Shape::Bool);
        assert_eq!(Shape::of_rust("Option<u32>"), Shape::Number);
        assert_eq!(Shape::of_rust("String"), Shape::Text);
        assert_eq!(Shape::of_rust("&'static str"), Shape::Text);
        assert_eq!(Shape::of_rust("kubuno::Rows"), Shape::List);
        assert_eq!(Shape::of_rust("Shared<SectionState<StorageData>>"), Shape::Object);
        assert_eq!(Shape::of_rust("MyEnum"), Shape::Any);
    }

    #[test]
    fn fields_of_a_struct() {
        let text = "pub struct V {\n    /// The title.\n    #[bind]\n    pub title: String,\n    #[bind(\"Total\")] amount: HashMap<String, f32>,\n    #[data_context]\n    data: Prefs,\n    plain: Vec<(u8, u8)>,\n}\n";
        let open = struct_body(text, "V").unwrap();
        let f = struct_fields(text, open);
        let names: Vec<&str> = f.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, ["title", "amount", "data", "plain"]);
        assert_eq!(f[0].doc, "The title.");
        assert_eq!(f[0].attrs, ["bind"]);
        assert_eq!(f[1].ty, "HashMap<String, f32>");
        assert_eq!(f[1].attrs, ["bind(\"Total\")"]);
        assert_eq!(&text[f[2].name_at..f[2].name_at + 4], "data");
        assert_eq!(f[3].ty, "Vec<(u8, u8)>");
    }

    #[test]
    fn arms_with_shapes_and_wildcards() {
        let code = "fn get(&self, path: &str) -> Option<Value> {\n    match path {\n        \"Title\" => Some(Value::Str(self.t.clone())),\n        \"Count\" | \"Total\" => Some(Value::F32(1.0)),\n        \"On\" if true => Some(Value::Bool(true)),\n        _ => None,\n    }\n}";
        let (arms, wildcard) = match_arms(code);
        let got: Vec<(&str, Shape)> = arms.iter().map(|(p, _, s)| (p.as_str(), *s)).collect();
        assert_eq!(got, [("Title", Shape::Text), ("Count", Shape::Number), ("Total", Shape::Number), ("On", Shape::Bool)]);
        assert!(!wildcard);
        let (_, wildcard) = match_arms("match path { \"A\" => None, _ => self.data.get(path) }");
        assert!(wildcard);
        assert_eq!(&code[arms[0].1..arms[0].1 + 5], "Title");
    }

    #[test]
    fn row_chains_are_read_with_their_shapes() {
        let text = "Row::new()\n    .with(\"Name\", Value::Str(n))\n    .with(\"Share\", Value::F32(s))\n    .with(\"Units\", Value::List(units));\nlet x = Row::new().with(\"A\", v);";
        let chains = row_chains(text);
        assert_eq!(chains.len(), 2);
        let first: Vec<(&str, Shape)> = chains[0].iter().map(|(k, _, s)| (k.as_str(), *s)).collect();
        assert_eq!(first, [("Name", Shape::Text), ("Share", Shape::Number), ("Units", Shape::List)]);
        assert_eq!(&text[chains[0][0].1..chains[0][0].1 + 4], "Name");
        assert_eq!(chains[1][0].2, Shape::Any);
    }

    #[test]
    fn select_columns_are_read() {
        assert_eq!(select_columns("SELECT id, name AS Nom, c.city, COUNT(x, y) AS n FROM customers c"), Some(vec!["id".into(), "Nom".into(), "city".into(), "n".into()]));
        assert_eq!(select_columns("SELECT * FROM t"), None);
        assert_eq!(select_columns("UPDATE t SET a = 1"), None);
    }

    #[test]
    fn project_converters_are_found() {
        let text = "#[derive(Default)] struct Initials;\n#[kubuno::views::value_converter]\nimpl ValueConverter for Initials {}\n#[value_converter(\"Money\")]\nimpl ValueConverter for M {}\nfn main() { register_converter(\"Neg\", Neg); }";
        let names: Vec<String> = project_converters(text).into_iter().map(|(n, _, _)| n).collect();
        assert_eq!(names, ["Initials", "Money", "Neg"]);
    }

    #[test]
    fn pascal_case() {
        assert_eq!(pascal("status_mode"), "StatusMode");
        assert_eq!(pascal("title"), "Title");
    }
}
