//! `{Res key}` in views (`vskubuno/docs/RESOURCES.md`): completion of the project's resource keys
//! (with their values, translations and comments), hover, go-to-definition into the `.kbres` file,
//! and diagnostics — an unknown key (error), an image property naming a non-image resource
//! (warning), a string not translated in one of the project's cultures (information).
//!
//! The index is the `.kbres` files of the view's package (the nearest `Cargo.toml` with a
//! `[package]`), read from disk, re-parsed only when a file changed.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use kubuno_desktop_resources_model::plural::{split_plural, PluralCategory};
use kubuno_desktop_resources_model::{culture, set, Entry, Kind, ResourceFile, Value};
use kubuno_desktop_views::ast::{AstNode, Attribute};
use kubuno_desktop_views::syntax::SyntaxKind;
use rowan::TextRange;
use lsp_types::{
    CompletionItem, CompletionItemKind, Diagnostic, DiagnosticSeverity, Documentation, Hover, HoverContents, Location, MarkupContent, MarkupKind, NumberOrString, Position, Range, Uri,
};

use crate::documents::Document;
use crate::fs_uri;

const SOURCE: &str = "kubuno-desktop-resources";
const SKIPPED: &[&str] = &["target", "bin", "obj", ".git", ".vs", "node_modules"];

/// One resource of the project.
#[derive(Debug, Clone)]
pub struct Item {
    pub set: String,
    pub entry: Entry,
    /// The neutral file and the entry's line/column there (0-based).
    pub file: PathBuf,
    pub position: Position,
    /// `(culture, entry)` of each satellite that translates it.
    pub translations: Vec<(String, Entry)>,
}

/// The resources of a package.
#[derive(Debug, Default, Clone)]
pub struct Index {
    pub items: Vec<Item>,
    /// Every culture of the package's sets.
    pub cultures: Vec<String>,
    /// `(set, culture)` pairs that exist.
    pub set_cultures: Vec<(String, String)>,
}

impl Index {
    /// The items a reference names (`set` narrows them).
    pub fn find(&self, key: &str, set: Option<&str>) -> Vec<&Item> {
        self.items.iter().filter(|i| i.entry.name == key && set.is_none_or(|s| s.eq_ignore_ascii_case(&i.set))).collect()
    }

    /// The plural forms (`key_one`, `key_other`… `String` entries) of `key` (`set` narrows them), in CLDR order.
    pub fn plural_forms(&self, key: &str, set: Option<&str>) -> Vec<&Item> {
        let mut forms: Vec<(PluralCategory, &Item)> = self
            .items
            .iter()
            .filter(|i| i.entry.kind == Kind::String && set.is_none_or(|s| s.eq_ignore_ascii_case(&i.set)))
            .filter_map(|i| split_plural(&i.entry.name).filter(|(b, _)| *b == key).map(|(_, c)| (c, i)))
            .collect();
        forms.sort_by_key(|(c, _)| *c);
        forms.into_iter().map(|(_, i)| i).collect()
    }
}

type Stamp = (Option<SystemTime>, u64);

thread_local! {
    /// Parsed files by path, with the stamp they were parsed at.
    static FILES: RefCell<HashMap<PathBuf, (Stamp, ResourceFile, String)>> = RefCell::new(HashMap::new());
}

fn stamp(path: &Path) -> Stamp {
    std::fs::metadata(path).map(|m| (m.modified().ok(), m.len())).unwrap_or((None, 0))
}

fn kbres_files(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    if depth > 8 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        match entry.file_type() {
            Ok(t) if t.is_dir() => {
                if !entry.file_name().to_str().is_some_and(|n| SKIPPED.contains(&n)) {
                    kbres_files(&path, depth + 1, out);
                }
            }
            Ok(_) if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("kbres")) => out.push(path),
            _ => {}
        }
    }
}

/// The parsed file and its text (cached by stamp).
fn parsed(path: &Path) -> Option<(ResourceFile, String)> {
    let s = stamp(path);
    if let Some(hit) = FILES.with(|f| f.borrow().get(path).filter(|(st, _, _)| *st == s).map(|(_, file, text)| (file.clone(), text.clone()))) {
        return Some(hit);
    }
    let text = std::fs::read_to_string(path).ok()?;
    let (file, _) = ResourceFile::read(&text);
    FILES.with(|f| f.borrow_mut().insert(path.to_path_buf(), (s, file.clone(), text.clone())));
    Some((file, text))
}

/// The 0-based line/UTF-16 column of byte `offset` in `text`.
fn position_of(text: &str, offset: usize) -> Position {
    let offset = offset.min(text.len());
    let before = &text[..offset];
    let line = before.matches('\n').count() as u32;
    let line_start = before.rfind('\n').map_or(0, |i| i + 1);
    let character = text[line_start..offset].encode_utf16().count() as u32;
    Position { line, character }
}

/// The resources of the package holding the view file `view`.
pub fn index_for(view: &Path) -> Index {
    let Some(root) = crate::project::package_root(view) else { return Index::default() };
    let mut files = Vec::new();
    kbres_files(&root, 0, &mut files);
    files.sort();
    let mut index = Index::default();
    for neutral in files.iter().filter(|p| p.file_name().and_then(|n| n.to_str()).and_then(culture::split_file_name).is_some_and(|(_, c)| c.is_none())) {
        let Some(set_files) = set::discover(neutral) else { continue };
        let Some((file, text)) = parsed(neutral) else { continue };
        let satellites: Vec<(String, ResourceFile)> = set_files.satellites.iter().filter_map(|s| parsed(&s.path).map(|(f, _)| (s.culture.clone(), f))).collect();
        for (c, _) in &satellites {
            if !index.cultures.contains(c) {
                index.cultures.push(c.clone());
            }
            index.set_cultures.push((set_files.name.clone(), c.clone()));
        }
        for e in &file.entries {
            index.items.push(Item {
                set: set_files.name.clone(),
                entry: e.clone(),
                file: neutral.clone(),
                position: position_of(&text, e.name_range.start),
                translations: satellites.iter().filter_map(|(c, f)| f.get(&e.name).map(|t| (c.clone(), t.clone()))).collect(),
            });
        }
    }
    index.cultures.sort();
    index
}

fn index_for_uri(uri: &Uri) -> Option<Index> {
    fs_uri::to_path(uri).map(|p| index_for(&p))
}

/// `(key, set)` of a `{Res …}` value.
fn reference(value: &str) -> Option<(String, Option<String>)> {
    let spec = kubuno_desktop_views::binding::parse_binding(value.trim())?;
    let (set, key) = kubuno_desktop_views::resources::reference(&spec)?;
    Some((key.to_string(), set.map(str::to_string)))
}

/// Whether an attribute takes an image (`Image`, `BackgroundImage`, `Icon`, `LargeIcon`…).
fn is_image_attribute(name: &str) -> bool {
    name.ends_with("Image") || name.ends_with("Icon")
}

fn value_text(e: &Entry) -> String {
    match &e.value {
        Value::Text(t) => t.clone(),
        Value::Linked { path } => format!("file {path}"),
        Value::Embedded { format, bytes } => format!("embedded {format}, {} bytes", bytes.len()),
    }
}

/// The documentation of an item: kind, set, value, translations, comment (and the picture for a
/// linked image, as a Markdown image).
fn markdown(item: &Item) -> String {
    let e = &item.entry;
    let mut md = format!("**{}** `{}` — *{}.kbres*\n\n", e.kind.element(), e.name, item.set);
    match &e.value {
        Value::Linked { path } if matches!(e.kind, Kind::Image | Kind::Icon) => {
            let full = item.file.parent().map(|d| d.join(path.replace('/', std::path::MAIN_SEPARATOR_STR))).unwrap_or_default();
            md.push_str(&format!("`{path}`\n\n"));
            if let Some(uri) = fs_uri::from_path(&full) {
                md.push_str(&format!("![{}]({})\n\n", e.name, uri.as_str()));
            }
        }
        _ => md.push_str(&format!("{}\n\n", value_text(e).replace('\n', "  \n"))),
    }
    for (c, t) in &item.translations {
        md.push_str(&format!("- `{c}`: {}\n", value_text(t).replace('\n', " ")));
    }
    if let Some(c) = &e.comment {
        md.push_str(&format!("\n*{c}*\n"));
    }
    md
}

/// Completion of resource keys after `{Res ` inside an attribute value; `None` when the cursor is
/// not in such a place (the ordinary completion then runs).
pub fn completion(doc: &Document, uri: &Uri, pos: Position) -> Option<Vec<CompletionItem>> {
    let offset = doc.position_index.position_to_offset(&doc.text, pos);
    let token = crate::tree::token_at_offset(&doc.parse.syntax(), offset)?;
    if token.kind() != SyntaxKind::STRING || token.parent().map(|p| p.kind()) != Some(SyntaxKind::ATTRIBUTE) {
        return None;
    }
    let start: usize = token.text_range().start().into();
    let cursor: usize = offset.into();
    let before = token.text().get(1..cursor.checked_sub(start)?)?;
    let at = before.rfind("{Res")?;
    let after = &before[at + 4..];
    if after.contains('}') || !(after.is_empty() || after.starts_with(' ')) || after.contains(',') {
        return None;
    }
    let attribute = token.parent().and_then(Attribute::cast)?;
    let wants_image = attribute.name().is_some_and(|n| is_image_attribute(&n));
    let index = index_for_uri(uri)?;
    // A key that only exists as plural forms is offered once, as its base (`files` for `files_one` /
    // `files_other`), documented with its forms.
    let mut bases: Vec<(String, String)> = Vec::new();
    let mut plural_items: Vec<CompletionItem> = Vec::new();
    for item in &index.items {
        let Some((base, _)) = split_plural(&item.entry.name).filter(|_| item.entry.kind == Kind::String) else { continue };
        if !index.find(base, Some(&item.set)).is_empty() || bases.iter().any(|(b, s)| b == base && *s == item.set) {
            continue;
        }
        bases.push((base.to_string(), item.set.clone()));
        let duplicate = index.items.iter().any(|i| i.set != item.set && (i.entry.name == base || split_plural(&i.entry.name).is_some_and(|(b, _)| b == base)));
        let insert = if duplicate { format!("{base}, Source={}", item.set) } else { base.to_string() };
        plural_items.push(CompletionItem {
            label: base.to_string(),
            kind: Some(CompletionItemKind::TEXT),
            detail: Some(format!("plural String · {}.kbres", item.set)),
            documentation: Some(Documentation::MarkupContent(MarkupContent { kind: MarkupKind::Markdown, value: plural_markdown(base, &index.plural_forms(base, Some(&item.set))) })),
            insert_text: Some(if after.is_empty() { format!(" {insert}") } else { insert }),
            filter_text: Some(base.to_string()),
            sort_text: Some(format!("{}{base}", if wants_image { "1" } else { "0" })),
            ..Default::default()
        });
    }
    let mut items: Vec<CompletionItem> = index
        .items
        .iter()
        .filter(|item| !(item.entry.kind == Kind::String && split_plural(&item.entry.name).is_some_and(|(b, _)| bases.iter().any(|(base, s)| base == b && *s == item.set))))
        .map(|item| {
            let duplicate = index.items.iter().filter(|i| i.entry.name == item.entry.name).count() > 1;
            let insert = if duplicate { format!("{}, Source={}", item.entry.name, item.set) } else { item.entry.name.clone() };
            let fits = !wants_image || matches!(item.entry.kind, Kind::Image | Kind::Icon);
            CompletionItem {
                label: item.entry.name.clone(),
                kind: Some(match item.entry.kind {
                    Kind::String => CompletionItemKind::TEXT,
                    Kind::Image | Kind::Icon => CompletionItemKind::FILE,
                    Kind::Color => CompletionItemKind::COLOR,
                    _ => CompletionItemKind::VALUE,
                }),
                detail: Some(format!("{} · {}.kbres", item.entry.kind.element(), item.set)),
                documentation: Some(Documentation::MarkupContent(MarkupContent { kind: MarkupKind::Markdown, value: markdown(item) })),
                insert_text: Some(if after.is_empty() { format!(" {insert}") } else { insert }),
                filter_text: Some(item.entry.name.clone()),
                // Images first on an image property, strings first elsewhere.
                sort_text: Some(format!("{}{}", if fits { "0" } else { "1" }, item.entry.name)),
                ..Default::default()
            }
        })
        .collect();
    items.extend(plural_items);
    items.sort_by(|a, b| a.sort_text.cmp(&b.sort_text));
    Some(items)
}

/// The documentation of a plural key: its forms, their values and translations.
fn plural_markdown(base: &str, forms: &[&Item]) -> String {
    let mut md = format!("**Plural** `{base}` — *{}.kbres*\n\n", forms.first().map(|i| i.set.as_str()).unwrap_or_default());
    for item in forms {
        md.push_str(&format!("- `{}`: {}\n", item.entry.name, value_text(&item.entry).replace('\n', " ")));
        for (c, t) in &item.translations {
            md.push_str(&format!("  - `{c}`: {}\n", value_text(t).replace('\n', " ")));
        }
    }
    md.push_str("\n*The form follows `Count={Binding …}`; `{{count}}` and the other `{{name}}` placeholders are filled from the arguments.*\n");
    md
}

fn range_of(doc: &Document, r: TextRange) -> Range {
    Range { start: doc.position_index.offset_to_position(&doc.text, r.start()), end: doc.position_index.offset_to_position(&doc.text, r.end()) }
}

/// The `{Res …}` attributes of the document: `(attribute name, key, set, value range)`.
fn references(doc: &Document) -> Vec<(String, String, Option<String>, TextRange)> {
    doc.parse
        .syntax()
        .descendants()
        .filter_map(Attribute::cast)
        .filter_map(|a| {
            let (key, set) = reference(&a.value()?)?;
            Some((a.name()?, key, set, a.value_range()?))
        })
        .collect()
}

/// Malformed `{Res}` arguments (`kubuno_desktop_views_syntax::res::ResIssue`, WV-6), as errors on the argument;
/// a repeated argument is a warning. Reported even when the project has no resource file.
fn res_argument_diagnostics(doc: &Document) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for a in doc.parse.syntax().descendants().filter_map(Attribute::cast) {
        let (Some(raw), Some(range)) = (a.raw_value(), a.value_range()) else { continue };
        let Some(inner) = raw.get(1..raw.len().saturating_sub(1)) else { continue };
        let (Some(open), Some(close)) = (inner.find('{'), inner.rfind('}')) else { continue };
        if close <= open || !inner[..open].trim().is_empty() {
            continue;
        }
        let Some(res) = kubuno_desktop_views_syntax::res::parse_res(&inner[open + 1..close]) else { continue };
        let base = u32::from(range.start()) as usize + open + 1;
        for issue in res.issues {
            let r = TextRange::new(((base + issue.range.start) as u32).into(), ((base + issue.range.end) as u32).into());
            let severity = if issue.message.contains("given twice") { DiagnosticSeverity::WARNING } else { DiagnosticSeverity::ERROR };
            out.push(Diagnostic {
                range: range_of(doc, r),
                severity: Some(severity),
                code: Some(NumberOrString::String("res-argument".into())),
                source: Some(SOURCE.to_string()),
                message: issue.message,
                ..Default::default()
            });
        }
    }
    out
}

/// Diagnostics of the document's `{Res …}` references (see the module doc).
pub fn diagnostics(doc: &Document, uri: &Uri) -> Vec<Diagnostic> {
    let refs = references(doc);
    if refs.is_empty() {
        return Vec::new();
    }
    let Some(index) = index_for_uri(uri) else { return res_argument_diagnostics(doc) };
    let diag = |range: TextRange, severity: DiagnosticSeverity, code: &str, message: String| Diagnostic {
        range: range_of(doc, range),
        severity: Some(severity),
        code: Some(NumberOrString::String(code.to_string())),
        source: Some(SOURCE.to_string()),
        message,
        ..Default::default()
    };
    let mut out = res_argument_diagnostics(doc);
    for (attr, key, set, range) in refs {
        let found = index.find(&key, set.as_deref());
        if found.is_empty() && !index.plural_forms(&key, set.as_deref()).is_empty() {
            // A key that only exists as plural forms (`files_one` / `files_other`).
            continue;
        }
        let Some(item) = found.first() else {
            let where_ = set.map(|s| format!("`{s}.kbres`")).unwrap_or_else(|| "the project's .kbres files".to_string());
            out.push(diag(range, DiagnosticSeverity::ERROR, "unknown-resource", format!("no resource `{key}` in {where_}")));
            continue;
        };
        if is_image_attribute(&attr) && !matches!(item.entry.kind, Kind::Image | Kind::Icon) && !attr.ends_with("Icon") {
            out.push(diag(range, DiagnosticSeverity::WARNING, "resource-kind", format!("`{attr}` takes an image, `{key}` is a {}", item.entry.kind.element())));
        }
        if item.entry.kind == Kind::String {
            let missing: Vec<&str> =
                index.set_cultures.iter().filter(|(s, c)| *s == item.set && !item.translations.iter().any(|(tc, _)| tc == c)).map(|(_, c)| c.as_str()).collect();
            if !missing.is_empty() {
                out.push(diag(
                    range,
                    DiagnosticSeverity::INFORMATION,
                    "missing-translation",
                    format!("`{key}` has no translation in {} (the neutral value is shown)", missing.join(", ")),
                ));
            }
        }
    }
    out
}

/// The `{Res …}` reference under the cursor: `(key, set, value range)`.
fn reference_at(doc: &Document, pos: Position) -> Option<(String, Option<String>, TextRange)> {
    let offset = doc.position_index.position_to_offset(&doc.text, pos);
    let token = crate::tree::token_at_offset(&doc.parse.syntax(), offset)?;
    let attribute = token.parent().and_then(Attribute::cast)?;
    let range = attribute.value_range()?;
    let (key, set) = reference(&attribute.value()?)?;
    Some((key, set, range))
}

/// Hover on a `{Res …}` value: the resource's values per culture.
pub fn hover(doc: &Document, uri: &Uri, pos: Position) -> Option<Hover> {
    let (key, set, range) = reference_at(doc, pos)?;
    let index = index_for_uri(uri)?;
    let value = match index.find(&key, set.as_deref()).into_iter().next() {
        Some(item) => markdown(item),
        None => {
            let forms = index.plural_forms(&key, set.as_deref());
            if forms.is_empty() {
                return None;
            }
            plural_markdown(&key, &forms)
        }
    };
    Some(Hover { contents: HoverContents::Markup(MarkupContent { kind: MarkupKind::Markdown, value }), range: Some(range_of(doc, range)) })
}

/// Go to definition on a `{Res …}` value: the entry in its `.kbres` file.
pub fn definition(doc: &Document, uri: &Uri, pos: Position) -> Option<Location> {
    let (key, set, _) = reference_at(doc, pos)?;
    let index = index_for_uri(uri)?;
    let item = index.find(&key, set.as_deref()).into_iter().next().or_else(|| index.plural_forms(&key, set.as_deref()).into_iter().next())?;
    Some(Location { uri: fs_uri::from_path(&item.file)?, range: Range { start: item.position, end: item.position } })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn package(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("kbres-ls-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("src/img")).expect("dirs");
        std::fs::write(root.join("Cargo.toml"), "[package]\nname = \"app\"\n").expect("manifest");
        std::fs::write(
            root.join("src/resources.kbres"),
            "<Resources>\n  <String Name=\"title\" Comment=\"Window title\">Hello</String>\n  <String Name=\"bye\">Bye</String>\n  <Image Name=\"logo\" File=\"img/logo.png\"/>\n</Resources>\n",
        )
        .expect("neutral");
        std::fs::write(root.join("src/resources.fr.kbres"), "<Resources><String Name=\"title\">Bonjour</String></Resources>").expect("fr");
        std::fs::write(root.join("src/img/logo.png"), b"png").expect("png");
        root
    }

    fn store(text: &str, uri: &Uri) -> crate::documents::DocumentStore {
        let mut store = crate::documents::DocumentStore::new();
        store.open(uri.clone(), text.to_string(), 1);
        store
    }

    /// WV-6: a key that only exists as plural forms is known, completed once and hovered with its forms;
    /// malformed arguments are diagnosed on the argument; `{Binding` inside an argument completes like a binding.
    #[test]
    fn plural_keys_and_res_arguments() {
        let root = package("plural");
        std::fs::write(
            root.join("src/resources.kbres"),
            "<Resources>\n  <String Name=\"files_one\">{{count}} file</String>\n  <String Name=\"files_other\">{{count}} files</String>\n  <String Name=\"title\">T</String>\n</Resources>\n",
        )
        .expect("neutral");
        std::fs::write(root.join("src/resources.fr.kbres"), "<Resources><String Name=\"files_one\">{{count}} fichier</String><String Name=\"title\">T</String></Resources>").expect("fr");
        let view = root.join("src/main_view.kbview");
        let uri = fs_uri::from_path(&view).expect("uri");
        let text = "<Window>\n  <Label Text=\"{Res files, Count={Binding n}, Sep=', '}\"/>\n  <Label Text=\"{Res title, 1x=2, Count={Res a}}\"/>\n  <Label Text=\"{Res \"/>\n</Window>";
        let docs = store(text, &uri);
        let doc = docs.get(&uri).expect("doc");

        let d = diagnostics(doc, &uri);
        let msgs: Vec<&str> = d.iter().map(|x| x.message.as_str()).collect();
        assert!(!msgs.iter().any(|m| m.contains("no resource `files`")), "{msgs:?}");
        let bad = d.iter().find(|x| x.message.contains("`1x` is not an argument name")).expect("argument diagnostic");
        assert_eq!(bad.severity, Some(DiagnosticSeverity::ERROR));
        let start = doc.position_index.position_to_offset(&doc.text, bad.range.start);
        assert!(text[usize::from(start)..].starts_with("1x=2"));
        assert!(msgs.iter().any(|m| m.contains("value of `Count` must be a `{Binding path}`")), "{msgs:?}");

        let at = text.find("{Res \"").expect("site") + 5;
        let pos = doc.position_index.offset_to_position(&doc.text, (at as u32).into());
        let labels: Vec<String> = completion(doc, &uri, pos).expect("completion").into_iter().map(|i| i.label).collect();
        assert_eq!(labels, vec!["files", "title"], "the forms are offered once, as their base");

        let at = text.find("{Res files").expect("ref") + 6;
        let pos = doc.position_index.offset_to_position(&doc.text, (at as u32).into());
        let HoverContents::Markup(m) = hover(doc, &uri, pos).expect("hover").contents else { panic!("markup") };
        assert!(m.value.contains("**Plural** `files`") && m.value.contains("`files_other`: {{count}} files") && m.value.contains("`fr`: {{count}} fichier"), "{}", m.value);
        assert!(definition(doc, &uri, pos).is_some());

        // `{Binding ` inside an argument gets the binding completion (its keys after a comma).
        let edited = text.replacen("{Binding n}", "{Binding n, }", 1);
        let at = edited.find("{Binding n, ").expect("arg") + "{Binding n,".len();
        let docs = store(&edited, &uri);
        let doc = docs.get(&uri).expect("doc");
        let pos = doc.position_index.offset_to_position(&doc.text, ((at + 1) as u32).into());
        let keys: Vec<String> = crate::binding_lsp::completion(doc, &uri, pos).expect("binding completion").into_iter().map(|i| i.label).collect();
        assert!(keys.iter().any(|k| k == "Mode"), "{keys:?}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn completes_hovers_navigates_and_diagnoses_resource_references() {
        let root = package("all");
        let view = root.join("src/main_view.kbview");
        let uri = fs_uri::from_path(&view).expect("uri");
        let text = "<Window>\n  <Button Text=\"{Res title}\" Image=\"{Res logo}\" ToolTip=\"{Res nope}\" BackgroundImage=\"{Res bye}\"/>\n  <Label Text=\"{Res \"/>\n</Window>";
        let docs = store(text, &uri);
        let doc = docs.get(&uri).expect("doc");

        // Completion right after `{Res `.
        let at = text.find("{Res \"").expect("site") + 5;
        let pos = doc.position_index.offset_to_position(&doc.text, (at as u32).into());
        let items = completion(doc, &uri, pos).expect("resource completion");
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert_eq!(labels, vec!["bye", "logo", "title"]);
        let title = items.iter().find(|i| i.label == "title").expect("title");
        let Some(Documentation::MarkupContent(md)) = &title.documentation else { panic!("doc") };
        assert!(md.value.contains("Hello") && md.value.contains("`fr`: Bonjour") && md.value.contains("Window title"), "{}", md.value);

        // Diagnostics: unknown key, image property with a string, missing translation.
        let d = diagnostics(doc, &uri);
        let msgs: Vec<&str> = d.iter().map(|x| x.message.as_str()).collect();
        assert!(msgs.iter().any(|m| m.contains("no resource `nope`")), "{msgs:?}");
        assert!(msgs.iter().any(|m| m.contains("`BackgroundImage` takes an image, `bye` is a String")), "{msgs:?}");
        assert!(msgs.iter().any(|m| m.contains("`bye` has no translation in fr")), "{msgs:?}");
        assert!(!msgs.iter().any(|m| m.contains("`title` has no translation")), "{msgs:?}");

        // Hover and go-to-definition on `{Res title}`.
        let at = text.find("{Res title}").expect("ref") + 6;
        let pos = doc.position_index.offset_to_position(&doc.text, (at as u32).into());
        let h = hover(doc, &uri, pos).expect("hover");
        let HoverContents::Markup(m) = h.contents else { panic!("markup") };
        assert!(m.value.contains("**String** `title`"), "{}", m.value);
        let loc = definition(doc, &uri, pos).expect("definition");
        assert!(loc.uri.as_str().ends_with("resources.kbres"), "{}", loc.uri.as_str());
        assert_eq!(loc.range.start, Position { line: 1, character: 16 });
        let _ = std::fs::remove_dir_all(&root);
    }
}
