//! `{Res key}` in web views: the keys of the project's i18n bundles (`…/locales/<lang>/<namespace>.json`, the
//! i18next resources the web runtime resolves `{Res}` through) and of its `.kbres` files (`vskubuno/docs/RESOURCES.md`,
//! the format the web plugin compiles to i18next bundles, WV-6).
//!
//! A key of a JSON bundle is its dotted path (`shell.change_photo`); a namespace other than the project's default
//! one (`core` for the core, else the first) is named with `Source=` (`{Res key, Source=notes}`), like a `.kbres` set.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const SKIPPED: &[&str] = &["node_modules", ".kubuno", ".git", "dist", "obj", "bin", "target", "coverage", ".vs"];

/// One string of the project.
#[derive(Debug, Clone, PartialEq)]
pub struct WebResource {
    pub key: String,
    /// The namespace (JSON file stem) or the `.kbres` set.
    pub set: String,
    /// The value in the preferred language (`KUBUNO_UI_LANG`, else English, else the first).
    pub value: String,
    pub file: PathBuf,
    /// Byte offset of the key in `file`.
    pub offset: usize,
    /// `(language, value)` of every bundle that has the key.
    pub translations: Vec<(String, String)>,
}

/// The strings of a web project.
#[derive(Debug, Clone, Default)]
pub struct WebResIndex {
    pub items: Vec<WebResource>,
    /// The namespace `{Res key}` uses without `Source=`.
    pub default_set: Option<String>,
}

impl WebResIndex {
    pub fn find(&self, key: &str, set: Option<&str>) -> Option<&WebResource> {
        match set {
            Some(s) => self.items.iter().find(|i| i.key == key && s.eq_ignore_ascii_case(&i.set)),
            // Without `Source=`: the default namespace first, then any set that has the key (a `.kbres`).
            None => self.items.iter().filter(|i| i.key == key).min_by_key(|i| self.default_set.as_deref() != Some(i.set.as_str())),
        }
    }
}

/// The leaf strings of a JSON document: `(dotted key, value, byte offset of the key)`.
pub fn json_leaves(text: &str) -> Vec<(String, String, usize)> {
    struct P<'a> {
        b: &'a [u8],
        s: &'a str,
        i: usize,
        out: Vec<(String, String, usize)>,
    }
    impl P<'_> {
        fn ws(&mut self) {
            while self.i < self.b.len() && (self.b[self.i] as char).is_ascii_whitespace() {
                self.i += 1;
            }
        }
        fn string(&mut self) -> Option<String> {
            if self.b.get(self.i) != Some(&b'"') {
                return None;
            }
            let start = self.i;
            self.i += 1;
            while self.i < self.b.len() {
                match self.b[self.i] {
                    b'\\' => self.i += 2,
                    b'"' => {
                        self.i += 1;
                        return serde_json::from_str::<String>(&self.s[start..self.i]).ok();
                    }
                    _ => self.i += 1,
                }
            }
            None
        }
        fn value(&mut self, path: &str, key_at: usize) -> Option<()> {
            self.ws();
            match self.b.get(self.i)? {
                b'{' => {
                    self.i += 1;
                    loop {
                        self.ws();
                        if self.b.get(self.i) == Some(&b'}') {
                            self.i += 1;
                            return Some(());
                        }
                        let at = self.i;
                        let k = self.string()?;
                        self.ws();
                        if self.b.get(self.i) != Some(&b':') {
                            return None;
                        }
                        self.i += 1;
                        let child = if path.is_empty() { k } else { format!("{path}.{k}") };
                        self.value(&child, at)?;
                        self.ws();
                        match self.b.get(self.i) {
                            Some(b',') => self.i += 1,
                            Some(b'}') => {
                                self.i += 1;
                                return Some(());
                            }
                            _ => return None,
                        }
                    }
                }
                b'"' => {
                    let v = self.string()?;
                    if !path.is_empty() {
                        self.out.push((path.to_string(), v, key_at));
                    }
                    Some(())
                }
                b'[' => {
                    // Arrays are not resource strings: skip them (balanced, strings aware).
                    let mut depth = 0usize;
                    while self.i < self.b.len() {
                        match self.b[self.i] {
                            b'"' => {
                                self.string()?;
                                continue;
                            }
                            b'[' | b'{' => depth += 1,
                            b']' | b'}' => {
                                depth -= 1;
                                if depth == 0 {
                                    self.i += 1;
                                    return Some(());
                                }
                            }
                            _ => {}
                        }
                        self.i += 1;
                    }
                    None
                }
                _ => {
                    while self.i < self.b.len() && !matches!(self.b[self.i], b',' | b'}' | b']') {
                        self.i += 1;
                    }
                    Some(())
                }
            }
        }
    }
    let s = text.strip_prefix('\u{feff}').unwrap_or(text);
    let shift = text.len() - s.len();
    let mut p = P { b: s.as_bytes(), s, i: 0, out: Vec::new() };
    let _ = p.value("", 0);
    p.out.into_iter().map(|(k, v, at)| (k, v, at + shift)).collect()
}

/// The locale folders of the project: `(language, folder)` for every `…/locales/<lang>/` under the sources.
fn locale_dirs(dir: &Path, depth: usize, out: &mut Vec<(String, PathBuf)>) {
    if depth > 10 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !entry.file_type().is_ok_and(|t| t.is_dir()) || SKIPPED.contains(&name.as_str()) {
            continue;
        }
        let path = entry.path();
        if name == "locales" {
            if let Ok(langs) = std::fs::read_dir(&path) {
                for l in langs.flatten().filter(|l| l.file_type().is_ok_and(|t| t.is_dir())) {
                    out.push((l.file_name().to_string_lossy().into_owned(), l.path()));
                }
            }
        } else {
            locale_dirs(&path, depth + 1, out);
        }
    }
}

fn kbres_files(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    if depth > 10 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        match entry.file_type() {
            Ok(t) if t.is_dir() => {
                if !SKIPPED.contains(&entry.file_name().to_string_lossy().as_ref()) {
                    kbres_files(&path, depth + 1, out);
                }
            }
            Ok(_) if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("kbres")) => out.push(path),
            _ => {}
        }
    }
}

fn preferred_languages() -> Vec<String> {
    let ui = std::env::var("KUBUNO_UI_LANG").ok().filter(|l| !l.is_empty());
    ui.into_iter().chain(["en".to_string(), "fr".to_string()]).collect()
}

/// Builds the index of the project rooted at `root` (its `sources`, `src` by default).
pub fn build(root: &Path, sources: &[String]) -> WebResIndex {
    let dirs: Vec<PathBuf> = if sources.is_empty() { vec![root.join("src")] } else { sources.iter().map(|s| root.join(s)).collect() };
    let mut locales = Vec::new();
    let mut kbres = Vec::new();
    for d in &dirs {
        locale_dirs(d, 0, &mut locales);
        kbres_files(d, 0, &mut kbres);
    }
    locales.sort();
    let preferred = preferred_languages();
    let primary = preferred.iter().find(|p| locales.iter().any(|(l, _)| l == *p)).cloned().or_else(|| locales.first().map(|(l, _)| l.clone()));
    let mut index = WebResIndex::default();
    // key → translations, gathered over every language.
    let mut by_key: HashMap<(String, String), Vec<(String, String)>> = HashMap::new();
    let mut namespaces: Vec<String> = Vec::new();
    for (lang, dir) in &locales {
        let Ok(files) = std::fs::read_dir(dir) else { continue };
        let mut files: Vec<PathBuf> = files.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("json"))).collect();
        files.sort();
        for file in files {
            let Some(ns) = file.file_stem().and_then(|s| s.to_str()).map(str::to_string) else { continue };
            let Ok(text) = std::fs::read_to_string(&file) else { continue };
            if !namespaces.contains(&ns) {
                namespaces.push(ns.clone());
            }
            for (key, value, offset) in json_leaves(&text) {
                by_key.entry((ns.clone(), key.clone())).or_default().push((lang.clone(), value.clone()));
                if Some(lang) == primary.as_ref() {
                    index.items.push(WebResource { key, set: ns.clone(), value, file: file.clone(), offset, translations: Vec::new() });
                }
            }
        }
    }
    for item in &mut index.items {
        item.translations = by_key.remove(&(item.set.clone(), item.key.clone())).unwrap_or_default();
    }
    index.default_set = if namespaces.iter().any(|n| n == "core") { Some("core".into()) } else { namespaces.first().cloned() };
    // `.kbres`: neutral files (no culture in the name), their satellites as translations.
    kbres.sort();
    for neutral in kbres.iter().filter(|p| p.file_name().and_then(|n| n.to_str()).and_then(kubuno_desktop_resources_model::culture::split_file_name).is_some_and(|(_, c)| c.is_none())) {
        let Ok(text) = std::fs::read_to_string(neutral) else { continue };
        let (file, _) = kubuno_desktop_resources_model::ResourceFile::read(&text);
        let set = neutral.file_stem().and_then(|s| s.to_str()).unwrap_or_default().to_string();
        for e in &file.entries {
            if let kubuno_desktop_resources_model::Value::Text(value) = &e.value {
                index.items.push(WebResource { key: e.name.clone(), set: set.clone(), value: value.clone(), file: neutral.clone(), offset: e.name_range.start, translations: Vec::new() });
            }
        }
    }
    index
}

const TTL: Duration = Duration::from_secs(10);

thread_local! {
    static INDEXES: RefCell<HashMap<PathBuf, (WebResIndex, Instant)>> = RefCell::new(HashMap::new());
}

/// The (cached) index of the project rooted at `root`.
pub fn index(root: &Path, sources: &[String]) -> WebResIndex {
    if let Some(hit) = INDEXES.with(|i| i.borrow().get(root).filter(|(_, at)| at.elapsed() < TTL).map(|(x, _)| x.clone())) {
        return hit;
    }
    let built = build(root, sources);
    INDEXES.with(|i| i.borrow_mut().insert(root.to_path_buf(), (built.clone(), Instant::now())));
    built
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_leaves_are_dotted_paths_with_offsets() {
        let text = "{\n  \"shell\": { \"change_photo\": \"Changer la photo\", \"n\": 3, \"list\": [\"a\", {\"b\": \"c\"}] },\n  \"esc\": \"a \\\"q\\\"\"\n}";
        let leaves = json_leaves(text);
        assert_eq!(leaves.iter().map(|(k, v, _)| (k.as_str(), v.as_str())).collect::<Vec<_>>(), vec![("shell.change_photo", "Changer la photo"), ("esc", "a \"q\"")]);
        let (_, _, at) = &leaves[0];
        assert!(text[*at..].starts_with("\"change_photo\""));
    }

    #[test]
    fn the_index_reads_every_language_and_kbres() {
        let root = std::env::temp_dir().join(format!("kubuno-webls-res-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("src/i18n/locales/en")).expect("dirs");
        std::fs::create_dir_all(root.join("src/i18n/locales/fr")).expect("dirs");
        std::fs::write(root.join("src/i18n/locales/en/core.json"), r#"{"shell":{"add_account":"Add account"}}"#).expect("en");
        std::fs::write(root.join("src/i18n/locales/fr/core.json"), r#"{"shell":{"add_account":"Ajouter un compte"}}"#).expect("fr");
        std::fs::write(root.join("src/strings.kbres"), "<Resources>\n  <String Name=\"title\">Hello</String>\n</Resources>\n").expect("kbres");
        let index = build(&root, &[]);
        let item = index.find("shell.add_account", None).expect("key");
        assert_eq!(item.set, "core");
        assert_eq!(item.translations.len(), 2);
        assert!(index.find("title", Some("strings")).is_some());
        let _ = std::fs::remove_dir_all(&root);
    }
}
