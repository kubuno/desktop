//! The `.kbsettings` format: an app's declared settings (Windows Forms' `Settings.settings`;
//! vskubuno `docs/STORAGE-COMPONENTS.md` §5.3). Read by the `settings!` macro (the typed class), the `.kbview`
//! language server (`{Binding …, Source=settings}` completion and diagnostics) and the Visual Studio settings
//! editor, which writes it back in the canonical form of [`SettingsFile::to_text`].
//!
//! ```xml
//! <?xml version="1.0" encoding="utf-8"?>
//! <Settings Version="2" App="kubuno-notes">
//!   <Setting Name="Theme" Type="String" Default="System" Values="System|Light|Dark" Description="The colour theme."/>
//!   <Setting Name="SyncIntervalMinutes" Type="Int" Default="5" PreviousNames="SyncInterval"/>
//!   <Setting Name="WindowBounds" Type="String" Roaming="false"/>
//!   <Setting Name="UpdateChannel" Type="String" Scope="Application" Default="stable"/>
//!   <Setting Name="RecentFiles" Type="StringList" Roaming="false">
//!     <Item>welcome.kbdoc</Item>
//!   </Setting>
//! </Settings>
//! ```
//!
//! - `Version` (≥ 1): the schema version, bumped when a setting is renamed, retyped or removed (the stored values
//!   are upgraded when the app opens them). `App`: the app id the values are stored under (default: the Cargo
//!   package name).
//! - `Type`: `String`, `Bool`, `Int`, `Float`, `StringList`. `Scope`: `User` (default, read-write) or
//!   `Application` (machine-wide, read-only for the app). `Roaming="false"`: a user setting kept on this machine.
//! - `Default`: invariant text (`true`, `42`, `1.5`); a `StringList`'s items are `<Item>` children. `Values`: the
//!   accepted values of a `String`, `|`-separated. `PreviousNames`: earlier names, `|`-separated.
//!
//! The types and rules are those of `kubuno-app-storage` (`SettingType`, `SettingDef`), kept equal by a test of
//! `kubuno-app-storage-components`; this crate depends on neither, like the rest of the format crates.

use std::ops::Range;

use crate::format::{Diagnostic, Severity};
use crate::xml::{self, Element, Node};

/// The current format version (of the file layout, not of the app's schema).
pub const FORMAT: &str = "1";

/// The setting types, canonical spelling.
pub const TYPES: &[&str] = &["String", "Bool", "Int", "Float", "StringList"];

/// One declared setting.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SettingEntry {
    pub name: String,
    /// Canonical type name (one of [`TYPES`]).
    pub ty: String,
    /// `User` or `Application`.
    pub scope: String,
    pub roaming: bool,
    /// Invariant text; a `StringList`'s items, one per line.
    pub default: String,
    pub values: Vec<String>,
    pub previous_names: Vec<String>,
    pub description: String,
    /// The byte range of the name's value in the file (diagnostics, go-to-definition).
    pub name_range: Range<usize>,
}

impl SettingEntry {
    /// A user, roaming setting.
    pub fn new(name: &str, ty: &str, default: &str) -> Self {
        Self { name: name.into(), ty: ty.into(), scope: "User".into(), roaming: true, default: default.into(), ..Default::default() }
    }

    pub fn is_application(&self) -> bool {
        self.scope == "Application"
    }

    /// The default's items of a `StringList`.
    pub fn default_items(&self) -> Vec<String> {
        if self.default.is_empty() {
            Vec::new()
        } else {
            self.default.lines().map(str::to_string).collect()
        }
    }
}

/// A parsed `.kbsettings` file.
#[derive(Debug, Clone, PartialEq)]
pub struct SettingsFile {
    /// The schema version (`Version`).
    pub version: u32,
    /// `App`, when written.
    pub app: Option<String>,
    /// `AccountScoped="true"`: the values belong to the signed-in account (`docs/STORAGE-COMPONENTS.md`, decision Q6).
    pub account_scoped: bool,
    pub entries: Vec<SettingEntry>,
}

impl Default for SettingsFile {
    fn default() -> Self {
        Self { version: 1, app: None, account_scoped: false, entries: Vec::new() }
    }
}

/// The canonical spelling of a type name (with the .NET aliases of a migrated `Settings.settings`).
pub fn canonical_type(name: &str) -> Option<&'static str> {
    Some(match name.trim().to_ascii_lowercase().as_str() {
        "string" | "system.string" => "String",
        "bool" | "boolean" | "system.boolean" => "Bool",
        "int" | "integer" | "int32" | "int64" | "long" | "system.int32" | "system.int64" => "Int",
        "float" | "double" | "single" | "system.double" | "system.single" => "Float",
        "stringlist" | "stringcollection" | "system.collections.specialized.stringcollection" => "StringList",
        _ => return None,
    })
}

/// Whether `name` is a valid setting name (a letter or `_`, then letters, digits and `_`, at most 128).
pub fn is_valid_setting_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= 128 && name.chars().next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_') && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Whether `text` is a valid default of type `ty` (canonical).
pub fn valid_default(ty: &str, text: &str) -> bool {
    match ty {
        "Bool" => matches!(text.trim(), "true" | "false" | "True" | "False" | "1" | "0"),
        "Int" => text.trim().parse::<i64>().is_ok(),
        "Float" => text.trim().parse::<f64>().is_ok_and(f64::is_finite),
        _ => true,
    }
}

/// The value a setting of type `ty` has with no `Default`.
pub fn zero_default(ty: &str) -> &'static str {
    match ty {
        "Bool" => "false",
        "Int" => "0",
        "Float" => "0.0",
        _ => "",
    }
}

/// Whether a name suggests a secret (`ApiToken`, `SmtpPassword`): such values belong in a `<SecretStore>`.
pub fn looks_like_secret(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    ["password", "passwd", "secret", "token", "apikey", "api_key", "credential", "privatekey", "private_key"].iter().any(|w| n.contains(w))
}

fn split_list(s: &str) -> Vec<String> {
    s.split('|').map(str::trim).filter(|v| !v.is_empty()).map(str::to_string).collect()
}

impl SettingsFile {
    pub fn get(&self, name: &str) -> Option<&SettingEntry> {
        self.entries.iter().find(|e| e.name == name)
    }

    /// Reads `text` leniently: the settings that could be read and every problem found (an error for what the
    /// macro refuses, a warning for what it ignores or advises against).
    pub fn read(text: &str) -> (SettingsFile, Vec<Diagnostic>) {
        let mut diags = Vec::new();
        let err = |diags: &mut Vec<Diagnostic>, message: String, range: Range<usize>| diags.push(Diagnostic { severity: Severity::Error, message, range });
        let warn = |diags: &mut Vec<Diagnostic>, message: String, range: Range<usize>| diags.push(Diagnostic { severity: Severity::Warning, message, range });
        let root = match xml::parse(text) {
            Ok(root) => root,
            Err(e) => {
                err(&mut diags, e.message, e.offset..e.offset);
                return (SettingsFile::default(), diags);
            }
        };
        if root.name != "Settings" {
            err(&mut diags, format!("the root element must be `<Settings>`, not `<{}>`", root.name), root.name_range.clone());
            return (SettingsFile::default(), diags);
        }
        let mut file = SettingsFile::default();
        if let Some(v) = root.attribute("Version") {
            match v.value.trim().parse::<u32>() {
                Ok(n) if n >= 1 => file.version = n,
                _ => err(&mut diags, format!("`Version` must be a whole number from 1 (`{}`)", v.value), v.value_range.clone()),
            }
        }
        if let Some(a) = root.attribute("App") {
            let ok = !a.value.is_empty()
                && a.value.len() <= 40
                && a.value.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '-' | '_'))
                && a.value.starts_with(|c: char| c.is_ascii_alphanumeric());
            if ok {
                file.app = Some(a.value.clone());
            } else {
                err(&mut diags, format!("`{}` is not a valid app id (1 to 40 characters a-z, 0-9, '.', '-', '_')", a.value), a.value_range.clone());
            }
        }
        for a in &root.attributes {
            if !matches!(a.name.as_str(), "Version" | "App" | "AccountScoped" | "Format" | "xmlns") {
                warn(&mut diags, format!("unknown attribute `{}` is ignored", a.name), a.name_range.clone());
            }
        }
        if let Some(a) = root.attribute("AccountScoped") {
            match a.value.trim() {
                "true" | "True" => file.account_scoped = true,
                "false" | "False" => {}
                _ => err(&mut diags, format!("`AccountScoped` is `true` or `false`, not `{}`", a.value), a.value_range.clone()),
            }
        }
        let mut seen: Vec<String> = Vec::new();
        for node in &root.children {
            match node {
                Node::Element(e) => {
                    let Some(entry) = read_entry(e, &mut diags) else { continue };
                    let mut clash = false;
                    for n in std::iter::once(&entry.name).chain(&entry.previous_names) {
                        let key = n.to_ascii_lowercase();
                        if seen.contains(&key) {
                            err(&mut diags, format!("`{n}` is declared twice (setting names are compared ignoring case)"), entry.name_range.clone());
                            clash = true;
                        }
                        seen.push(key);
                    }
                    if !clash {
                        file.entries.push(entry);
                    }
                }
                Node::Text(t) if !t.trim().is_empty() => warn(&mut diags, "text outside a setting is ignored".into(), root.content_range.clone()),
                _ => {}
            }
        }
        (file, diags)
    }

    /// Reads `text`, failing on the first error.
    pub fn parse(text: &str) -> Result<SettingsFile, Diagnostic> {
        let (file, diags) = Self::read(text);
        match diags.into_iter().find(|d| d.severity == Severity::Error) {
            Some(e) => Err(e),
            None => Ok(file),
        }
    }

    /// The canonical text: the XML declaration, `<Settings Version="N"[ App="…"]>`, one setting per line
    /// (attributes in the order Name, Type, Scope, Roaming, Default, Values, PreviousNames, Description; the
    /// defaults of Scope and Roaming omitted), list items one per line, two-space indentation, `\n` and a final
    /// newline. Writing what [`SettingsFile::parse`] read gives the same file (the Visual Studio editor writes the
    /// same bytes).
    pub fn to_text(&self) -> String {
        let mut out = format!("<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<Settings Version=\"{}\"", self.version.max(1));
        if let Some(app) = &self.app {
            out.push_str(&format!(" App=\"{}\"", xml::escape_attr(app)));
        }
        if self.account_scoped {
            out.push_str(" AccountScoped=\"true\"");
        }
        if self.entries.is_empty() {
            out.push_str("/>\n");
            return out;
        }
        out.push_str(">\n");
        for e in &self.entries {
            out.push_str(&format!("  <Setting Name=\"{}\" Type=\"{}\"", xml::escape_attr(&e.name), xml::escape_attr(&e.ty)));
            if e.scope == "Application" {
                out.push_str(" Scope=\"Application\"");
            } else if !e.roaming {
                out.push_str(" Roaming=\"false\"");
            }
            let list = e.ty == "StringList";
            if !list && !e.default.is_empty() {
                out.push_str(&format!(" Default=\"{}\"", xml::escape_attr(&e.default)));
            }
            if !e.values.is_empty() {
                out.push_str(&format!(" Values=\"{}\"", xml::escape_attr(&e.values.join("|"))));
            }
            if !e.previous_names.is_empty() {
                out.push_str(&format!(" PreviousNames=\"{}\"", xml::escape_attr(&e.previous_names.join("|"))));
            }
            if !e.description.is_empty() {
                out.push_str(&format!(" Description=\"{}\"", xml::escape_attr(&e.description)));
            }
            let items = if list { e.default_items() } else { Vec::new() };
            if items.is_empty() {
                out.push_str("/>\n");
            } else {
                out.push_str(">\n");
                for i in items {
                    out.push_str(&format!("    <Item>{}</Item>\n", xml::escape_text(&i)));
                }
                out.push_str("  </Setting>\n");
            }
        }
        out.push_str("</Settings>\n");
        out
    }
}

fn read_entry(e: &Element, diags: &mut Vec<Diagnostic>) -> Option<SettingEntry> {
    let err = |diags: &mut Vec<Diagnostic>, message: String, range: Range<usize>| diags.push(Diagnostic { severity: Severity::Error, message, range });
    let warn = |diags: &mut Vec<Diagnostic>, message: String, range: Range<usize>| diags.push(Diagnostic { severity: Severity::Warning, message, range });
    if e.name != "Setting" {
        err(diags, format!("unknown element `<{}>` (expected `<Setting>`)", e.name), e.name_range.clone());
        return None;
    }
    let Some(name_attr) = e.attribute("Name") else {
        err(diags, "`<Setting>` has no `Name`".into(), e.name_range.clone());
        return None;
    };
    let name = name_attr.value.clone();
    if !is_valid_setting_name(&name) {
        err(diags, format!("`{name}` is not a valid setting name (a letter or `_`, then letters, digits and `_`)"), name_attr.value_range.clone());
        return None;
    }
    for a in &e.attributes {
        if !matches!(a.name.as_str(), "Name" | "Type" | "Scope" | "Roaming" | "Default" | "Values" | "PreviousNames" | "Description") {
            warn(diags, format!("unknown attribute `{}` is ignored", a.name), a.name_range.clone());
        }
    }
    let ty = match e.attribute("Type") {
        None => "String",
        Some(t) => match canonical_type(&t.value) {
            Some(t) => t,
            None => {
                err(diags, format!("unknown type `{}` (expected String, Bool, Int, Float or StringList)", t.value), t.value_range.clone());
                return None;
            }
        },
    };
    let scope = match e.attribute("Scope") {
        None => "User",
        Some(s) => match s.value.trim().to_ascii_lowercase().as_str() {
            "user" => "User",
            "application" => "Application",
            _ => {
                err(diags, format!("unknown scope `{}` (expected User or Application)", s.value), s.value_range.clone());
                return None;
            }
        },
    };
    let roaming = match e.attribute("Roaming") {
        None => true,
        Some(r) => match r.value.trim() {
            "true" | "True" => true,
            "false" | "False" => false,
            _ => {
                err(diags, format!("`Roaming` is `true` or `false`, not `{}`", r.value), r.value_range.clone());
                return None;
            }
        },
    };
    if scope == "Application" && e.attribute("Roaming").is_some() {
        warn(diags, "`Roaming` is ignored on an Application setting (machine-wide)".into(), e.attribute("Roaming").map_or(e.name_range.clone(), |a| a.name_range.clone()));
    }
    let values = e.attr("Values").map(split_list).unwrap_or_default();
    if !values.is_empty() && ty != "String" {
        err(diags, "`Values` (accepted values) is only for String settings".into(), e.attribute("Values").map_or(e.name_range.clone(), |a| a.value_range.clone()));
        return None;
    }
    let mut default = match e.attribute("Default") {
        Some(d) => {
            if ty == "StringList" {
                warn(diags, "a StringList's default is written as `<Item>` children: `Default` is ignored".into(), d.name_range.clone());
                String::new()
            } else if !valid_default(ty, &d.value) {
                err(diags, format!("`{}` is not a valid {ty} default", d.value), d.value_range.clone());
                return None;
            } else if ty == "Bool" {
                matches!(d.value.trim(), "true" | "True" | "1").to_string()
            } else {
                d.value.clone()
            }
        }
        None => match values.first() {
            Some(first) => first.clone(),
            None => zero_default(ty).to_string(),
        },
    };
    if ty == "StringList" {
        let mut items = Vec::new();
        for child in e.elements() {
            if child.name != "Item" {
                warn(diags, format!("`<{}>` is ignored (a StringList's default items are `<Item>`s)", child.name), child.name_range.clone());
                continue;
            }
            let t = child.text();
            if t.contains('\n') {
                err(diags, "a list item is one line".into(), child.range.clone());
                return None;
            }
            items.push(t);
        }
        default = items.join("\n");
    } else if let Some(child) = e.elements().next() {
        warn(diags, format!("`<{}>` is ignored (only a StringList has items)", child.name), child.name_range.clone());
    }
    if !values.is_empty() && !values.contains(&default) {
        err(diags, format!("the default `{default}` is not one of the accepted values"), e.attribute("Default").map_or(e.name_range.clone(), |a| a.value_range.clone()));
        return None;
    }
    let previous_names = e.attr("PreviousNames").map(split_list).unwrap_or_default();
    if let Some(bad) = previous_names.iter().find(|n| !is_valid_setting_name(n)) {
        err(diags, format!("`{bad}` is not a valid setting name"), e.attribute("PreviousNames").map_or(e.name_range.clone(), |a| a.value_range.clone()));
        return None;
    }
    if looks_like_secret(&name) {
        warn(
            diags,
            format!("`{name}` looks like a secret: settings are stored in plain files or the Registry; keep secrets in a `<SecretStore>`"),
            name_attr.value_range.clone(),
        );
    }
    Some(SettingEntry {
        name,
        ty: ty.to_string(),
        scope: scope.to_string(),
        roaming: scope == "User" && roaming,
        default,
        values,
        previous_names,
        description: e.attr("Description").unwrap_or_default().to_string(),
        name_range: name_attr.value_range.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<Settings Version=\"2\" App=\"kubuno-notes\">\n  <Setting Name=\"Theme\" Type=\"String\" Default=\"System\" Values=\"System|Light|Dark\" Description=\"The colour &amp; theme.\"/>\n  <Setting Name=\"SyncIntervalMinutes\" Type=\"Int\" Default=\"5\" PreviousNames=\"SyncInterval\"/>\n  <Setting Name=\"WindowBounds\" Type=\"String\" Roaming=\"false\"/>\n  <Setting Name=\"ShowHidden\" Type=\"Bool\" Default=\"false\"/>\n  <Setting Name=\"UpdateChannel\" Type=\"String\" Scope=\"Application\" Default=\"stable\"/>\n  <Setting Name=\"RecentFiles\" Type=\"StringList\" Roaming=\"false\">\n    <Item>welcome.kbdoc</Item>\n    <Item>a &lt; b</Item>\n  </Setting>\n</Settings>\n";

    #[test]
    fn reads_and_writes_the_canonical_form() {
        let (file, diags) = SettingsFile::read(SAMPLE);
        assert!(diags.is_empty(), "{diags:?}");
        assert_eq!((file.version, file.app.as_deref()), (2, Some("kubuno-notes")));
        assert_eq!(file.entries.len(), 6);
        let theme = file.get("Theme").expect("theme");
        assert_eq!((theme.values.len(), theme.description.as_str()), (3, "The colour & theme."));
        assert_eq!(file.get("SyncIntervalMinutes").map(|e| e.previous_names.clone()), Some(vec!["SyncInterval".to_string()]));
        assert!(!file.get("WindowBounds").expect("w").roaming);
        assert!(file.get("UpdateChannel").expect("u").is_application());
        assert_eq!(file.get("RecentFiles").expect("r").default_items(), vec!["welcome.kbdoc", "a < b"]);
        assert_eq!(file.to_text(), SAMPLE, "canonical round trip");
    }

    #[test]
    fn reports_errors_with_their_place() {
        let bad = "<Settings Version=\"0\">\n  <Setting Name=\"1x\"/>\n  <Setting Name=\"N\" Type=\"Date\"/>\n  <Setting Name=\"B\" Type=\"Bool\" Default=\"yes\"/>\n  <Setting Name=\"T\" Default=\"Blue\" Values=\"Red|Green\"/>\n  <Setting Name=\"V\" Type=\"Int\" Values=\"1|2\"/>\n  <Setting Name=\"a\"/>\n  <Setting Name=\"A\"/>\n  <Other/>\n</Settings>";
        let (file, diags) = SettingsFile::read(bad);
        let errors: Vec<&str> = diags.iter().filter(|d| d.severity == Severity::Error).map(|d| d.message.as_str()).collect();
        assert_eq!(errors.len(), 8, "{errors:#?}");
        assert_eq!(file.entries.len(), 1, "only `a` is kept");
        let at = |needle: &str| diags.iter().find(|d| d.message.contains(needle)).map(|d| &bad[d.range.clone()]);
        assert_eq!(at("unknown type"), Some("Date"));
        assert_eq!(at("not a valid Bool"), Some("yes"));
    }

    #[test]
    fn account_scoped_files_round_trip() {
        let text = "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<Settings Version=\"1\" App=\"mail\" AccountScoped=\"true\">\n  <Setting Name=\"Signature\" Type=\"String\"/>\n</Settings>\n";
        let file = SettingsFile::parse(text).expect("valid");
        assert!(file.account_scoped);
        assert_eq!(file.to_text(), text);
        assert!(SettingsFile::parse("<Settings AccountScoped=\"maybe\"/>").is_err());
    }

    #[test]
    fn defaults_and_warnings() {
        let (file, diags) = SettingsFile::read("<Settings>\n  <Setting Name=\"Mode\" Values=\"Fast|Safe\"/>\n  <Setting Name=\"Count\" Type=\"Int\"/>\n  <Setting Name=\"SmtpPassword\"/>\n  <Setting Name=\"X\" Scope=\"Application\" Roaming=\"false\"/>\n</Settings>");
        assert_eq!(file.version, 1);
        assert_eq!(file.get("Mode").map(|e| e.default.as_str()), Some("Fast"), "the first accepted value");
        assert_eq!(file.get("Count").map(|e| e.default.as_str()), Some("0"));
        assert!(diags.iter().all(|d| d.severity == Severity::Warning));
        assert!(diags.iter().any(|d| d.message.contains("looks like a secret")));
        assert!(diags.iter().any(|d| d.message.contains("ignored on an Application")));
        assert!(!file.get("X").expect("x").roaming);
    }
}
