//! Resource sets: what a lookup reads. Two kinds:
//!
//! - [`StaticSet`] — what `resources!` generates: the neutral file and its satellites embedded as
//!   text (`include_str!`), linked files embedded as bytes (`include_bytes!`). A culture's file is
//!   parsed the first time it is needed, then kept for the life of the process (like Drive's
//!   localisation: a process uses one or two cultures), which is what lets the generated accessors
//!   return `&'static str`.
//! - [`LoadedSet`] — files read at run time (the Visual Studio designer previewing a project's
//!   resources, a tool): parsed eagerly, linked files read from disk.

use crate::types::{Bytes, ResolvedValue};
use kubuno_resources_model::{culture, Entry, Kind, ResourceFile, Value};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// A set of resources a lookup can read (see the module doc).
pub trait Source: Send + Sync {
    /// The set's name: the neutral file's stem (`resources`).
    fn name(&self) -> &str;
    /// The value of `name` in `culture`, after the fallback chain (specific culture → neutral
    /// culture → same-language culture → the neutral file).
    fn resolve(&self, name: &str, culture: &str) -> Option<ResolvedValue>;
    /// The names and kinds of the neutral file's entries, in file order.
    fn entries(&self) -> Vec<(String, Kind)>;
    /// The satellite cultures.
    fn cultures(&self) -> Vec<String>;
}

/// The embedded files of a set — what `resources!` generates.
#[derive(Debug)]
pub struct EmbeddedSet {
    pub name: &'static str,
    pub neutral: &'static str,
    /// `(culture, text)`.
    pub satellites: &'static [(&'static str, &'static str)],
    /// The linked files: `(path as written in the .kbres file, bytes)`.
    pub files: &'static [(&'static str, &'static [u8])],
}

/// One entry of a parsed static table.
#[derive(Debug, Clone, Copy)]
enum StaticValue {
    Text(&'static str),
    Bytes { format: &'static str, bytes: &'static [u8] },
}

type StaticTable = HashMap<&'static str, (Kind, StaticValue)>;

/// A set embedded in the program (see the module doc).
pub struct StaticSet {
    embedded: &'static EmbeddedSet,
    tables: Mutex<Vec<(String, &'static StaticTable)>>,
}

impl std::fmt::Debug for StaticSet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StaticSet").field("name", &self.embedded.name).finish()
    }
}

const NEUTRAL: &str = "";

fn leak_str(s: &str) -> &'static str {
    Box::leak(s.to_string().into_boxed_str())
}

impl StaticSet {
    pub const fn new(embedded: &'static EmbeddedSet) -> Self {
        Self { embedded, tables: Mutex::new(Vec::new()) }
    }

    pub fn embedded(&self) -> &'static EmbeddedSet {
        self.embedded
    }

    fn table(&self, culture: &str) -> Option<&'static StaticTable> {
        let mut tables = self.tables.lock().ok()?;
        if let Some((_, t)) = tables.iter().find(|(c, _)| c == culture) {
            return Some(t);
        }
        let text = if culture == NEUTRAL { self.embedded.neutral } else { self.embedded.satellites.iter().find(|(c, _)| *c == culture)?.1 };
        let file = match ResourceFile::parse(text) {
            Ok(f) => f,
            Err(e) => {
                tracing::warn!("resources `{}` ({}): {}", self.embedded.name, if culture.is_empty() { "neutral" } else { culture }, e.message);
                ResourceFile::default()
            }
        };
        let mut table: StaticTable = HashMap::with_capacity(file.entries.len());
        for e in &file.entries {
            let value = match &e.value {
                Value::Text(t) => StaticValue::Text(leak_str(t)),
                Value::Linked { path } => match self.embedded.files.iter().find(|(p, _)| p == path) {
                    Some((_, bytes)) => StaticValue::Bytes { format: leak_str(&e.format().unwrap_or_default()), bytes },
                    None => continue,
                },
                Value::Embedded { format, bytes } => StaticValue::Bytes { format: leak_str(format), bytes: Box::leak(bytes.clone().into_boxed_slice()) },
            };
            table.insert(leak_str(&e.name), (e.kind, value));
        }
        let leaked: &'static StaticTable = Box::leak(Box::new(table));
        tables.push((culture.to_string(), leaked));
        Some(leaked)
    }

    fn chain(&self, culture_name: &str) -> Vec<&'static str> {
        let available: Vec<&'static str> = self.embedded.satellites.iter().map(|(c, _)| *c).collect();
        let mut chain = culture::fallback_chain(culture_name, &available);
        chain.push(NEUTRAL);
        chain
    }

    fn find(&self, name: &str, culture_name: &str) -> Option<(Kind, StaticValue)> {
        let neutral = self.table(NEUTRAL)?;
        let (kind, _) = neutral.get(name)?;
        for c in self.chain(culture_name) {
            if let Some((k, v)) = self.table(c).and_then(|t| t.get(name)) {
                if k == kind {
                    return Some((*k, *v));
                }
            }
        }
        None
    }

    /// The text of `name` (a `String`, `Color` or `Font`, or a text `File`) in the current culture;
    /// `""` when there is none.
    pub fn text(&self, name: &str) -> &'static str {
        match self.find(name, &crate::culture()) {
            Some((_, StaticValue::Text(t))) => t,
            Some((_, StaticValue::Bytes { bytes, .. })) => std::str::from_utf8(bytes).unwrap_or_default(),
            None => "",
        }
    }

    /// The bytes and format of `name` in the current culture; empty when there are none.
    pub fn bytes(&self, name: &str) -> (&'static [u8], &'static str) {
        match self.find(name, &crate::culture()) {
            Some((_, StaticValue::Bytes { bytes, format })) => (bytes, format),
            Some((_, StaticValue::Text(t))) => (t.as_bytes(), ""),
            None => (&[], ""),
        }
    }

    /// The text of `name` in `culture` (what a translation table helper or a test reads).
    pub fn text_in(&self, name: &str, culture: &str) -> &'static str {
        match self.find(name, culture) {
            Some((_, StaticValue::Text(t))) => t,
            _ => "",
        }
    }
}

impl Source for StaticSet {
    fn name(&self) -> &str {
        self.embedded.name
    }

    fn resolve(&self, name: &str, culture: &str) -> Option<ResolvedValue> {
        let (kind, value) = self.find(name, culture)?;
        Some(match value {
            StaticValue::Text(t) => ResolvedValue::Text { kind, text: t.to_string() },
            StaticValue::Bytes { format, bytes } => ResolvedValue::Bytes { kind, format: format.to_string(), bytes: Bytes::Static(bytes) },
        })
    }

    fn entries(&self) -> Vec<(String, Kind)> {
        // In file order: parse the neutral text again (cheap, tools only).
        ResourceFile::parse(self.embedded.neutral).map(|f| f.entries.iter().map(|e| (e.name.clone(), e.kind)).collect()).unwrap_or_default()
    }

    fn cultures(&self) -> Vec<String> {
        self.embedded.satellites.iter().map(|(c, _)| c.to_string()).collect()
    }
}

/// A set read at run time (see the module doc).
pub struct LoadedSet {
    name: String,
    /// The folder linked paths are relative to.
    base_dir: PathBuf,
    neutral: ResourceFile,
    satellites: Vec<(String, ResourceFile)>,
    files: Mutex<HashMap<String, Option<Arc<[u8]>>>>,
}

impl std::fmt::Debug for LoadedSet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LoadedSet").field("name", &self.name).field("base_dir", &self.base_dir).finish()
    }
}

impl LoadedSet {
    /// A set from the texts of its files (an unreadable file reads as empty, with a warning).
    pub fn from_texts(name: impl Into<String>, base_dir: impl Into<PathBuf>, neutral: &str, satellites: &[(String, String)]) -> Self {
        let name = name.into();
        let parse = |what: &str, text: &str| match ResourceFile::parse(text) {
            Ok(f) => f,
            Err(e) => {
                tracing::warn!("resources `{name}` ({what}): {}", e.message);
                ResourceFile::default()
            }
        };
        Self {
            base_dir: base_dir.into(),
            neutral: parse("neutral", neutral),
            satellites: satellites.iter().map(|(c, t)| (culture::canonical(c), parse(c, t))).collect(),
            files: Mutex::new(HashMap::new()),
            name,
        }
    }

    /// The set of the `.kbres` file at `path` (the neutral file or one of its satellites) with every
    /// satellite beside it, read from disk.
    pub fn from_disk(path: &std::path::Path) -> std::io::Result<Self> {
        let files = kubuno_resources_model::set::discover(path).ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "not a .kbres file"))?;
        let neutral = std::fs::read_to_string(&files.neutral)?;
        let satellites: Vec<(String, String)> = files.satellites.iter().filter_map(|s| std::fs::read_to_string(&s.path).ok().map(|t| (s.culture.clone(), t))).collect();
        let base = files.neutral.parent().map(std::path::Path::to_path_buf).unwrap_or_default();
        Ok(Self::from_texts(files.name, base, &neutral, &satellites))
    }

    fn entry(&self, name: &str, culture_name: &str) -> Option<&Entry> {
        let kind = self.neutral.get(name)?.kind;
        let available: Vec<&str> = self.satellites.iter().map(|(c, _)| c.as_str()).collect();
        for c in culture::fallback_chain(culture_name, &available) {
            if let Some(e) = self.satellites.iter().find(|(sc, _)| sc == c).and_then(|(_, f)| f.get(name)) {
                if e.kind == kind {
                    return Some(e);
                }
            }
        }
        self.neutral.get(name)
    }

    fn file(&self, rel: &str) -> Option<Arc<[u8]>> {
        let path = self.base_dir.join(rel.replace('/', std::path::MAIN_SEPARATOR_STR));
        let mut files = self.files.lock().ok()?;
        files
            .entry(path.to_string_lossy().into_owned())
            .or_insert_with(|| match std::fs::read(&path) {
                Ok(b) => Some(Arc::from(b.into_boxed_slice())),
                Err(e) => {
                    tracing::warn!("resources `{}`: cannot read {}: {e}", self.name, path.display());
                    None
                }
            })
            .clone()
    }
}

impl Source for LoadedSet {
    fn name(&self) -> &str {
        &self.name
    }

    fn resolve(&self, name: &str, culture: &str) -> Option<ResolvedValue> {
        let e = self.entry(name, culture)?;
        Some(match &e.value {
            Value::Text(t) => ResolvedValue::Text { kind: e.kind, text: t.clone() },
            Value::Linked { path } => ResolvedValue::Bytes { kind: e.kind, format: e.format().unwrap_or_default(), bytes: Bytes::Shared(self.file(path)?) },
            Value::Embedded { format, bytes } => {
                // One shared copy per entry (keyed by its address: the parsed files never change), so the
                // image caches keyed by the bytes' address decode it once.
                let key = format!("embedded:{:p}", e);
                let shared = self.files.lock().ok()?.entry(key).or_insert_with(|| Some(Arc::from(bytes.clone().into_boxed_slice()))).clone()?;
                ResolvedValue::Bytes { kind: e.kind, format: format.clone(), bytes: Bytes::Shared(shared) }
            }
        })
    }

    fn entries(&self) -> Vec<(String, Kind)> {
        self.neutral.entries.iter().map(|e| (e.name.clone(), e.kind)).collect()
    }

    fn cultures(&self) -> Vec<String> {
        self.satellites.iter().map(|(c, _)| c.clone()).collect()
    }
}
