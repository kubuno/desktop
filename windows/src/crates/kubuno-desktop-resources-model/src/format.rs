//! The `.kbres` format: reading (lenient, with positioned diagnostics) and the canonical writing.
//!
//! ```xml
//! <?xml version="1.0" encoding="utf-8"?>
//! <Resources Version="1">
//!   <String Name="welcome_text" Comment="The home page's greeting">Welcome to Kubuno</String>
//!   <Image Name="logo" File="images/logo.png"/>
//!   <Icon Name="app" File="app.ico"/>
//!   <Image Name="dot" Format="png">iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==</Image>
//!   <Audio Name="ding" File="sounds/ding.wav"/>
//!   <File Name="license" File="LICENSE.txt" Text="true"/>
//!   <Color Name="accent" Value="#3366FF"/>
//!   <Font Name="heading" Value="Segoe UI, 14pt, style=Bold"/>
//! </Resources>
//! ```
//!
//! One element per entry, its kind as the element name, in the order the developer chose. A linked
//! entry names a file (`File`, relative to the `.kbres` file, forward slashes); an embedded one holds
//! base64 bytes as its text (wrapped at 76 columns by the writer) and its `Format` (the file
//! extension: `png`, `ico`, `wav`…). A satellite (`resources.fr.kbres`) has the same shape and
//! overrides entries of the neutral file by name.

use crate::xml::{self, Element, Node};
use base64::Engine as _;
use std::ops::Range;

/// The kind of an entry — its element name, and the editor category it is listed under.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Kind {
    String,
    Image,
    Icon,
    Audio,
    File,
    Color,
    Font,
}

impl Kind {
    pub const ALL: [Kind; 7] = [Kind::String, Kind::Image, Kind::Icon, Kind::Audio, Kind::File, Kind::Color, Kind::Font];

    /// The element name (`String`, `Image`…).
    pub fn element(self) -> &'static str {
        match self {
            Kind::String => "String",
            Kind::Image => "Image",
            Kind::Icon => "Icon",
            Kind::Audio => "Audio",
            Kind::File => "File",
            Kind::Color => "Color",
            Kind::Font => "Font",
        }
    }

    pub fn from_element(name: &str) -> Option<Kind> {
        Kind::ALL.into_iter().find(|k| k.element() == name)
    }

    /// Whether the value is text written in the file (`String`, `Color`, `Font`), not bytes.
    pub fn is_text(self) -> bool {
        matches!(self, Kind::String | Kind::Color | Kind::Font)
    }
}

/// Where an entry's value lives.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// A text value (`String`'s content, `Color`/`Font`'s `Value`).
    Text(String),
    /// A file of the project, relative to the `.kbres` file (forward slashes).
    Linked { path: String },
    /// Bytes held in the `.kbres` file (base64), with their format (file extension, lower case).
    Embedded { format: String, bytes: Vec<u8> },
}

/// One entry.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub name: String,
    pub kind: Kind,
    pub value: Value,
    pub comment: Option<String>,
    /// A `File` entry whose bytes are UTF-8 text (its accessor returns `&str`).
    pub text_file: bool,
    /// The whole element in the source (empty for an entry built in memory).
    pub range: Range<usize>,
    /// The `Name` attribute's value in the source.
    pub name_range: Range<usize>,
}

impl Entry {
    /// A text entry (`String`, `Color`, `Font`).
    pub fn text(kind: Kind, name: impl Into<String>, value: impl Into<String>) -> Self {
        Self { name: name.into(), kind, value: Value::Text(value.into()), comment: None, text_file: false, range: 0..0, name_range: 0..0 }
    }

    /// A linked entry.
    pub fn linked(kind: Kind, name: impl Into<String>, path: impl Into<String>) -> Self {
        Self { name: name.into(), kind, value: Value::Linked { path: path.into() }, comment: None, text_file: false, range: 0..0, name_range: 0..0 }
    }

    /// An embedded entry.
    pub fn embedded(kind: Kind, name: impl Into<String>, format: impl Into<String>, bytes: Vec<u8>) -> Self {
        Self { name: name.into(), kind, value: Value::Embedded { format: format.into(), bytes }, comment: None, text_file: false, range: 0..0, name_range: 0..0 }
    }

    pub fn with_comment(mut self, comment: impl Into<String>) -> Self {
        let c = comment.into();
        self.comment = (!c.is_empty()).then_some(c);
        self
    }

    /// The text of a text entry.
    pub fn as_text(&self) -> Option<&str> {
        match &self.value {
            Value::Text(t) => Some(t),
            _ => None,
        }
    }

    /// The format of a binary entry: the embedded `Format`, else the linked file's extension.
    pub fn format(&self) -> Option<String> {
        match &self.value {
            Value::Text(_) => None,
            Value::Embedded { format, .. } => Some(format.clone()),
            Value::Linked { path } => path.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).filter(|e| !e.contains('/')),
        }
    }
}

/// How serious a diagnostic is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

/// A problem found while reading a file.
#[derive(Debug, Clone, PartialEq)]
pub struct Diagnostic {
    pub severity: Severity,
    pub message: String,
    pub range: Range<usize>,
}

/// A parsed `.kbres` file.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ResourceFile {
    pub entries: Vec<Entry>,
    /// The `Culture` attribute of the root, when written. On a satellite it is informative (the file name
    /// decides); on a neutral file it names the language of the neutral strings (`Culture="en"`): the web
    /// compiles the neutral file as that language's bundle. Absent, the file is written without it.
    pub culture: Option<String>,
}

/// The current format version.
pub const VERSION: &str = "1";

/// The image formats offered for `Image`/`Icon` entries.
pub const IMAGE_FORMATS: &[&str] = &["svg", "png", "jpg", "jpeg", "bmp", "gif", "ico", "tif", "tiff", "webp"];
/// The audio formats offered for `Audio` entries.
pub const AUDIO_FORMATS: &[&str] = &["wav", "mp3", "wma", "ogg", "flac", "m4a"];

/// The kind an imported file gets by its extension (`Image`, `Icon` for `.ico`, `Audio`, else `File`).
pub fn kind_for_extension(ext: &str) -> Kind {
    let ext = ext.trim_start_matches('.').to_ascii_lowercase();
    if ext == "ico" {
        Kind::Icon
    } else if IMAGE_FORMATS.contains(&ext.as_str()) {
        Kind::Image
    } else if AUDIO_FORMATS.contains(&ext.as_str()) {
        Kind::Audio
    } else {
        Kind::File
    }
}

impl ResourceFile {
    /// The entry named `name`.
    pub fn get(&self, name: &str) -> Option<&Entry> {
        self.entries.iter().find(|e| e.name == name)
    }

    /// The `String` entries, in file order: `(name, value, comment)`. With [`ResourceFile::from_strings`],
    /// [`ResourceFile::read`] and [`ResourceFile::to_text`], what a converter between `.kbres` files and
    /// i18next bundles needs (the web's WASM shim: `parse` → JSON, `write` from JSON).
    pub fn strings(&self) -> impl Iterator<Item = (&str, &str, Option<&str>)> {
        self.entries.iter().filter(|e| e.kind == Kind::String).map(|e| (e.name.as_str(), e.as_text().unwrap_or_default(), e.comment.as_deref()))
    }

    /// A file of `String` entries `(name, value, comment)`, in the given order, with an optional root
    /// `Culture` (the language of a neutral file's strings). A name given twice keeps its first place and
    /// its last value (a JSON object's semantics). Names are not checked: [`ResourceFile::read`] rejects
    /// only an empty one.
    pub fn from_strings<N, V, C>(culture: Option<String>, strings: impl IntoIterator<Item = (N, V, Option<C>)>) -> ResourceFile
    where
        N: Into<String>,
        V: Into<String>,
        C: Into<String>,
    {
        let mut file = ResourceFile { entries: Vec::new(), culture: culture.filter(|c| !c.is_empty()) };
        for (name, value, comment) in strings {
            let mut entry = Entry::text(Kind::String, name, value);
            if let Some(c) = comment {
                entry = entry.with_comment(c);
            }
            match file.entries.iter_mut().find(|e| e.name == entry.name) {
                Some(existing) => *existing = entry,
                None => file.entries.push(entry),
            }
        }
        file
    }

    /// The plural forms of `base` (`items_one`, `items_other`… — `String` entries, see [`crate::plural`]),
    /// in CLDR category order.
    pub fn plural_forms(&self, base: &str) -> Vec<(crate::plural::PluralCategory, &Entry)> {
        let mut forms: Vec<_> = self
            .entries
            .iter()
            .filter(|e| e.kind == Kind::String)
            .filter_map(|e| crate::plural::split_plural(&e.name).filter(|(b, _)| *b == base).map(|(_, c)| (c, e)))
            .collect();
        forms.sort_by_key(|(c, _)| *c);
        forms
    }

    /// Whether `name` is a key `{Res}` can name: an entry, or the base of plural forms (`items` when only
    /// `items_one` / `items_other` exist).
    pub fn has_key(&self, name: &str) -> bool {
        self.get(name).is_some() || !self.plural_forms(name).is_empty()
    }

    /// Whether `name` is a plural form (`items_few`) of a key this file knows (an entry `items` or another
    /// form `items_one`): a satellite may hold forms its neutral file's language does not have (Russian
    /// `_few`, Arabic `_two`).
    pub fn knows_plural_form(&self, name: &str) -> bool {
        crate::plural::split_plural(name).is_some_and(|(base, _)| self.get(base).is_some_and(|e| e.kind == Kind::String) || !self.plural_forms(base).is_empty())
    }

    /// Reads `text`, leniently: the entries that could be read, and every problem found (an error
    /// for anything the runtime would not accept, a warning for what it ignores). Use
    /// [`ResourceFile::parse`] for the strict form.
    pub fn read(text: &str) -> (ResourceFile, Vec<Diagnostic>) {
        let mut diags = Vec::new();
        let root = match xml::parse(text) {
            Ok(root) => root,
            Err(e) => {
                diags.push(Diagnostic { severity: Severity::Error, message: e.message, range: e.offset..e.offset });
                return (ResourceFile::default(), diags);
            }
        };
        if root.name != "Resources" {
            diags.push(Diagnostic { severity: Severity::Error, message: format!("the root element must be `<Resources>`, not `<{}>`", root.name), range: root.name_range.clone() });
            return (ResourceFile::default(), diags);
        }
        if let Some(v) = root.attribute("Version") {
            if v.value != VERSION {
                diags.push(Diagnostic { severity: Severity::Warning, message: format!("unknown format version `{}` (this tool reads version {VERSION})", v.value), range: v.value_range.clone() });
            }
        }
        let mut file = ResourceFile { entries: Vec::new(), culture: root.attr("Culture").map(str::to_string).filter(|c| !c.is_empty()) };
        for node in &root.children {
            match node {
                Node::Element(e) => {
                    if let Some(entry) = read_entry(e, &mut diags) {
                        if let Some(first) = file.get(&entry.name) {
                            diags.push(Diagnostic {
                                severity: Severity::Error,
                                message: format!("duplicate resource `{}` (first defined as {} {})", entry.name, a_or_an(first.kind.element()), first.kind.element()),
                                range: entry.name_range.clone(),
                            });
                            continue;
                        }
                        file.entries.push(entry);
                    }
                }
                Node::Text(t) if !t.trim().is_empty() => {
                    diags.push(Diagnostic { severity: Severity::Warning, message: "text outside an entry is ignored".to_string(), range: root.content_range.clone() });
                }
                _ => {}
            }
        }
        (file, diags)
    }

    /// Reads `text`, failing on the first error (warnings are ignored).
    pub fn parse(text: &str) -> Result<ResourceFile, Diagnostic> {
        let (file, diags) = Self::read(text);
        match diags.into_iter().find(|d| d.severity == Severity::Error) {
            Some(e) => Err(e),
            None => Ok(file),
        }
    }

    /// The canonical text of the file: the XML declaration, `<Resources Version="1">`, one entry per
    /// line (embedded bytes as base64 lines of 76 characters), two-space indentation, `\n` line
    /// ends and a final newline. Writing what [`ResourceFile::parse`] read gives the same file.
    pub fn to_text(&self) -> String {
        let mut out = String::from("<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<Resources Version=\"1\"");
        if let Some(c) = &self.culture {
            out.push_str(&format!(" Culture=\"{}\"", xml::escape_attr(c)));
        }
        if self.entries.is_empty() {
            out.push_str("/>\n");
            return out;
        }
        out.push_str(">\n");
        for e in &self.entries {
            write_entry(&mut out, e);
        }
        out.push_str("</Resources>\n");
        out
    }
}

fn a_or_an(word: &str) -> &'static str {
    if word.starts_with(['A', 'E', 'I', 'O', 'U']) {
        "an"
    } else {
        "a"
    }
}

fn read_entry(e: &Element, diags: &mut Vec<Diagnostic>) -> Option<Entry> {
    let Some(kind) = Kind::from_element(&e.name) else {
        diags.push(Diagnostic {
            severity: Severity::Error,
            message: format!("unknown entry kind `<{}>` (expected String, Image, Icon, Audio, File, Color or Font)", e.name),
            range: e.name_range.clone(),
        });
        return None;
    };
    let Some(name_attr) = e.attribute("Name") else {
        diags.push(Diagnostic { severity: Severity::Error, message: format!("`<{}>` has no `Name`", e.name), range: e.name_range.clone() });
        return None;
    };
    let name = name_attr.value.clone();
    let valid = if kind == Kind::String { crate::names::is_valid_string_name(&name) } else { crate::names::is_valid_name(&name) };
    if !valid {
        diags.push(Diagnostic {
            severity: Severity::Error,
            message: if kind == Kind::String {
                "a String needs a non-empty `Name`".to_string()
            } else {
                format!("`{name}` is not a valid resource name (letters, digits, `_`, `.`, `-` and `+`, starting with a letter or `_`)")
            },
            range: name_attr.value_range.clone(),
        });
        return None;
    }
    for a in &e.attributes {
        if !matches!(a.name.as_str(), "Name" | "Comment" | "File" | "Format" | "Value" | "Text" | "xml:space") {
            diags.push(Diagnostic { severity: Severity::Warning, message: format!("unknown attribute `{}` is ignored", a.name), range: a.name_range.clone() });
        }
    }
    let comment = e.attr("Comment").map(str::to_string).filter(|c| !c.is_empty());
    let text_file = e.attr("Text") == Some("true");
    let value = match kind {
        Kind::String => {
            if e.elements().next().is_some() {
                diags.push(Diagnostic { severity: Severity::Error, message: "a String holds text only".to_string(), range: e.range.clone() });
                return None;
            }
            Value::Text(e.text())
        }
        Kind::Color | Kind::Font => match e.attr("Value") {
            Some(v) => {
                if kind == Kind::Color && parse_color(v).is_none() {
                    let range = e.attribute("Value").map(|a| a.value_range.clone()).unwrap_or(e.range.clone());
                    diags.push(Diagnostic { severity: Severity::Error, message: format!("`{v}` is not a colour (`#RRGGBB`, `#AARRGGBB` or `r, g, b`)"), range });
                    return None;
                }
                Value::Text(v.to_string())
            }
            None => {
                diags.push(Diagnostic { severity: Severity::Error, message: format!("`<{}>` needs a `Value`", e.name), range: e.name_range.clone() });
                return None;
            }
        },
        _ => match e.attr("File") {
            Some(path) if !path.trim().is_empty() => {
                if !e.text().trim().is_empty() {
                    diags.push(Diagnostic { severity: Severity::Warning, message: "a linked entry's content is ignored (it has a `File`)".to_string(), range: e.content_range.clone() });
                }
                Value::Linked { path: path.replace('\\', "/") }
            }
            _ => {
                let compact: String = e.text().chars().filter(|c| !c.is_whitespace()).collect();
                let Some(format) = e.attr("Format").map(|f| f.trim_start_matches('.').to_ascii_lowercase()).filter(|f| !f.is_empty()) else {
                    diags.push(Diagnostic { severity: Severity::Error, message: format!("`<{}>` needs a `File` (linked) or a `Format` and base64 content (embedded)", e.name), range: e.name_range.clone() });
                    return None;
                };
                match base64::engine::general_purpose::STANDARD.decode(compact.as_bytes()) {
                    Ok(bytes) => Value::Embedded { format, bytes },
                    Err(err) => {
                        diags.push(Diagnostic { severity: Severity::Error, message: format!("the embedded content is not valid base64: {err}"), range: e.content_range.clone() });
                        return None;
                    }
                }
            }
        },
    };
    if let (Kind::Image | Kind::Icon, Some(fmt)) = (kind, value_format(&value)) {
        if !IMAGE_FORMATS.contains(&fmt.as_str()) {
            diags.push(Diagnostic { severity: Severity::Warning, message: format!("`{fmt}` is not an image format the runtime decodes ({})", IMAGE_FORMATS.join(", ")), range: e.range.clone() });
        }
    }
    Some(Entry { name, kind, value, comment, text_file, range: e.range.clone(), name_range: name_attr.value_range.clone() })
}

fn value_format(v: &Value) -> Option<String> {
    match v {
        Value::Text(_) => None,
        Value::Embedded { format, .. } => Some(format.clone()),
        Value::Linked { path } => path.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()),
    }
}

fn write_entry(out: &mut String, e: &Entry) {
    let tag = e.kind.element();
    out.push_str(&format!("  <{tag} Name=\"{}\"", xml::escape_attr(&e.name)));
    match &e.value {
        Value::Text(t) if e.kind == Kind::String => {
            if let Some(c) = &e.comment {
                out.push_str(&format!(" Comment=\"{}\"", xml::escape_attr(c)));
            }
            if t.is_empty() {
                out.push_str("/>\n");
            } else {
                out.push_str(&format!(">{}</{tag}>\n", xml::escape_text(t)));
            }
        }
        Value::Text(t) => {
            out.push_str(&format!(" Value=\"{}\"", xml::escape_attr(t)));
            if let Some(c) = &e.comment {
                out.push_str(&format!(" Comment=\"{}\"", xml::escape_attr(c)));
            }
            out.push_str("/>\n");
        }
        Value::Linked { path } => {
            out.push_str(&format!(" File=\"{}\"", xml::escape_attr(path)));
            if e.text_file {
                out.push_str(" Text=\"true\"");
            }
            if let Some(c) = &e.comment {
                out.push_str(&format!(" Comment=\"{}\"", xml::escape_attr(c)));
            }
            out.push_str("/>\n");
        }
        Value::Embedded { format, bytes } => {
            out.push_str(&format!(" Format=\"{}\"", xml::escape_attr(format)));
            if e.text_file {
                out.push_str(" Text=\"true\"");
            }
            if let Some(c) = &e.comment {
                out.push_str(&format!(" Comment=\"{}\"", xml::escape_attr(c)));
            }
            let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
            if b64.len() <= 76 {
                out.push_str(&format!(">{b64}</{tag}>\n"));
            } else {
                out.push_str(">\n");
                for chunk in b64.as_bytes().chunks(76) {
                    out.push_str("    ");
                    out.push_str(std::str::from_utf8(chunk).unwrap_or_default());
                    out.push('\n');
                }
                out.push_str(&format!("  </{tag}>\n"));
            }
        }
    }
}

/// A colour value: `#RRGGBB`, `#AARRGGBB`, `#RGB` or `r, g, b[, a]` → `(r, g, b, a)`.
pub fn parse_color(s: &str) -> Option<(u8, u8, u8, u8)> {
    let s = s.trim();
    if let Some(hex) = s.strip_prefix('#') {
        let v = |i: usize| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok();
        return match hex.len() {
            3 => {
                let d = |i: usize| u8::from_str_radix(hex.get(i..i + 1)?, 16).ok().map(|x| x * 17);
                Some((d(0)?, d(1)?, d(2)?, 255))
            }
            6 => Some((v(0)?, v(2)?, v(4)?, 255)),
            8 => Some((v(2)?, v(4)?, v(6)?, v(0)?)),
            _ => None,
        };
    }
    let parts: Vec<u8> = s.split(',').map(|p| p.trim().parse::<u8>()).collect::<Result<_, _>>().ok()?;
    match parts[..] {
        [r, g, b] => Some((r, g, b, 255)),
        [r, g, b, a] => Some((r, g, b, a)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r##"<?xml version="1.0" encoding="utf-8"?>
<Resources Version="1">
  <String Name="welcome_text" Comment="The greeting">Welcome to &lt;Kubuno&gt; &amp; co</String>
  <String Name="empty"/>
  <String Name="multi">line 1
line 2</String>
  <Image Name="logo" File="images/logo.png" Comment="Header logo"/>
  <Icon Name="app" File="app.ico"/>
  <Image Name="dot" Format="png">AAECAwQF</Image>
  <Audio Name="ding" File="sounds/ding.wav"/>
  <File Name="license" File="LICENSE.txt" Text="true"/>
  <Color Name="accent" Value="#3366FF"/>
  <Font Name="heading" Value="Segoe UI, 14pt, style=Bold"/>
</Resources>
"##;

    #[test]
    fn reads_every_kind() {
        let file = ResourceFile::parse(SAMPLE).unwrap();
        assert_eq!(file.entries.len(), 10);
        assert_eq!(file.get("welcome_text").unwrap().as_text(), Some("Welcome to <Kubuno> & co"));
        assert_eq!(file.get("welcome_text").unwrap().comment.as_deref(), Some("The greeting"));
        assert_eq!(file.get("multi").unwrap().as_text(), Some("line 1\nline 2"));
        assert_eq!(file.get("logo").unwrap().value, Value::Linked { path: "images/logo.png".into() });
        assert_eq!(file.get("dot").unwrap().value, Value::Embedded { format: "png".into(), bytes: vec![0, 1, 2, 3, 4, 5] });
        assert_eq!(file.get("app").unwrap().kind, Kind::Icon);
        assert!(file.get("license").unwrap().text_file);
        assert_eq!(file.get("accent").unwrap().as_text(), Some("#3366FF"));
        let name = &file.get("logo").unwrap().name_range;
        assert_eq!(&SAMPLE[name.clone()], "logo");
    }

    #[test]
    fn round_trips_canonically() {
        let file = ResourceFile::parse(SAMPLE).unwrap();
        let text = file.to_text();
        assert_eq!(text, SAMPLE, "the sample is already canonical");
        let again = ResourceFile::parse(&text).unwrap();
        assert_eq!(again.to_text(), text);
        // Long embedded content is wrapped, and still reads back.
        let mut big = ResourceFile::default();
        big.entries.push(Entry::embedded(Kind::Image, "big", "png", (0..=255u8).cycle().take(1000).collect()).with_comment("x\ny"));
        let t = big.to_text();
        assert!(t.lines().all(|l| l.len() <= 90));
        let back = ResourceFile::parse(&t).unwrap();
        assert_eq!(back.entries[0].value, big.entries[0].value);
        assert_eq!(back.entries[0].comment.as_deref(), Some("x\ny"));
        assert_eq!(back.to_text(), t);
        assert_eq!(ResourceFile::default().to_text(), "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<Resources Version=\"1\"/>\n");
    }

    /// Web strings (i18next keys and values) survive write → read byte for byte.
    #[test]
    fn web_strings_round_trip_losslessly() {
        let strings: Vec<(&str, &str, Option<&str>)> = vec![
            ("header.settings", "Settings", None),
            ("drive-shared", "Shared with me", Some("Sidebar <entry> & \"note\"\n2nd line")),
            ("2fa_title", "  leading and trailing  ", None),
            ("files_one", "{{count}} file", None),
            ("files_other", "{{count}} files", None),
            ("a key: with spaces", "line 1\nline 2\r\nline 3\r", None),
            ("tabs", "\ta\tb\t", None),
            ("markup", "<1>Bold</1> & <2/> < > &amp; ]]> \"q\" 'a'", None),
            ("emoji", "📁 Fichiers 👍🏽", None),
            ("rtl", "مرحبا بك في كوبونو — שלום", None),
            ("empty", "", None),
            ("ws", " ", None),
            ("nl", "\n", None),
            ("crlf-only", "\r\n", None),
            ("é.clé", "valeur", None),
        ];
        let file = ResourceFile::from_strings(Some("en".to_string()), strings.iter().map(|(n, v, c)| (*n, *v, *c)));
        let text = file.to_text();
        assert!(text.starts_with("<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<Resources Version=\"1\" Culture=\"en\">\n"), "{text}");
        let (back, diags) = ResourceFile::read(&text);
        assert!(diags.is_empty(), "{diags:?}");
        assert_eq!(back.culture.as_deref(), Some("en"));
        let read: Vec<(&str, &str, Option<&str>)> = back.strings().collect();
        assert_eq!(read, strings);
        assert_eq!(back.to_text(), text, "canonical");
        // A name given twice keeps its first place and its last value.
        let dup = ResourceFile::from_strings::<&str, &str, &str>(None, [("a", "1", None), ("b", "2", None), ("a", "3", None)]);
        assert_eq!(dup.strings().map(|(n, v, _)| (n, v)).collect::<Vec<_>>(), vec![("a", "3"), ("b", "2")]);
        // No `Culture`: the output is unchanged.
        assert!(!dup.to_text().contains("Culture"));
        // Only an empty String name is rejected.
        assert!(ResourceFile::parse("<Resources><String Name=\"\">x</String></Resources>").is_err());
    }

    #[test]
    fn lists_plural_forms() {
        let file = ResourceFile::parse(r##"<Resources><String Name="files_other">{{count}} files</String><String Name="files_one">{{count}} file</String><String Name="title">T</String><Color Name="c_one" Value="#fff"/></Resources>"##).unwrap();
        let forms: Vec<_> = file.plural_forms("files").into_iter().map(|(c, e)| (c.as_str(), e.name.as_str())).collect();
        assert_eq!(forms, vec![("one", "files_one"), ("other", "files_other")]);
        assert!(file.has_key("files") && file.has_key("title") && !file.has_key("nope"));
        assert!(file.plural_forms("c").is_empty(), "a Color is not a plural form");
        assert!(file.knows_plural_form("files_few") && file.knows_plural_form("title_one") && !file.knows_plural_form("nope_one"));
    }

    #[test]
    fn reports_problems_with_ranges() {
        let src = "<Resources><String Name=\"a\">x</String><String Name=\"a\">y</String><Blob Name=\"b\"/><Image Name=\"c\"/><Color Name=\"d\" Value=\"teal-ish\"/><Color Name=\"9x\" Value=\"#fff\"/></Resources>";
        let (file, diags) = ResourceFile::read(src);
        assert_eq!(file.entries.len(), 1);
        let msgs: Vec<_> = diags.iter().map(|d| d.message.as_str()).collect();
        assert!(msgs.iter().any(|m| m.contains("duplicate resource `a`")), "{msgs:?}");
        assert!(msgs.iter().any(|m| m.contains("unknown entry kind `<Blob>`")), "{msgs:?}");
        assert!(msgs.iter().any(|m| m.contains("needs a `File`")), "{msgs:?}");
        assert!(msgs.iter().any(|m| m.contains("not a colour")), "{msgs:?}");
        assert!(msgs.iter().any(|m| m.contains("not a valid resource name")), "{msgs:?}");
        let dup = diags.iter().find(|d| d.message.contains("duplicate")).unwrap();
        assert_eq!(&src[dup.range.clone()], "a");
        assert!(ResourceFile::parse(src).is_err());
        assert!(ResourceFile::parse("<Other/>").is_err());
    }

    #[test]
    fn parses_colors_and_kinds() {
        assert_eq!(parse_color("#3366FF"), Some((0x33, 0x66, 0xFF, 255)));
        assert_eq!(parse_color("#803366FF"), Some((0x33, 0x66, 0xFF, 0x80)));
        assert_eq!(parse_color("#fff"), Some((255, 255, 255, 255)));
        assert_eq!(parse_color("10, 20, 30"), Some((10, 20, 30, 255)));
        assert_eq!(parse_color("red"), None);
        assert_eq!(kind_for_extension("ICO"), Kind::Icon);
        assert_eq!(kind_for_extension(".svg"), Kind::Image);
        assert_eq!(kind_for_extension("wav"), Kind::Audio);
        assert_eq!(kind_for_extension("pdf"), Kind::File);
    }
}
