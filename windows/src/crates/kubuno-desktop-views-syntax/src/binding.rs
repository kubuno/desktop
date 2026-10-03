//! The `{Binding …}` grammar (`vskubuno/docs/VIEWS-SPEC.md` §6.1): its parts with their byte ranges,
//! the accepted keys, the problems of an expression that still parses, and the parsed form
//! [`BindingSyntax`]. What a binding *does* at run time (`BindingSpec`, view models, converters) is
//! `kubuno_desktop_views::binding`, which re-exports everything here under its historical path.

/// Which way a binding moves values (WPF `BindingMode`): `OneWay` (the default) reads the source,
/// `TwoWay` also writes the user's changes back (§3's "the event side of the same mechanism"),
/// `OneTime` reads the source once and keeps that value, `OneWayToSource` only writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BindingMode {
    #[default]
    OneWay,
    TwoWay,
    OneTime,
    OneWayToSource,
}

impl BindingMode {
    /// Every mode, as written in XML.
    pub const ALL: [BindingMode; 4] = [BindingMode::OneWay, BindingMode::TwoWay, BindingMode::OneTime, BindingMode::OneWayToSource];

    /// The mode written `name` (`TwoWay`…), `None` for an unknown one.
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|m| m.name() == name.trim())
    }

    /// The XML name of the mode.
    pub fn name(self) -> &'static str {
        match self {
            BindingMode::OneWay => "OneWay",
            BindingMode::TwoWay => "TwoWay",
            BindingMode::OneTime => "OneTime",
            BindingMode::OneWayToSource => "OneWayToSource",
        }
    }

    /// Whether the property reads the source (every mode but `OneWayToSource`).
    pub fn reads_source(self) -> bool {
        self != BindingMode::OneWayToSource
    }

    /// Whether a user change of the property is written back to the source (`TwoWay`,
    /// `OneWayToSource`): what a control checks before calling `BindingSpec::update_source` (`kubuno_desktop_views::binding`).
    pub fn writes_back(self) -> bool {
        matches!(self, BindingMode::TwoWay | BindingMode::OneWayToSource)
    }
}

/// When a write-back reaches the source (WPF `UpdateSourceTrigger`): at each change (the default),
/// when the element loses the focus, or only when the code asks (`kubuno_desktop_views::binding::update_sources`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum UpdateSourceTrigger {
    #[default]
    PropertyChanged,
    LostFocus,
    Explicit,
}

impl UpdateSourceTrigger {
    /// Every trigger, as written in XML.
    pub const ALL: [UpdateSourceTrigger; 3] = [UpdateSourceTrigger::PropertyChanged, UpdateSourceTrigger::LostFocus, UpdateSourceTrigger::Explicit];

    /// The trigger written `name`; `Default` is `PropertyChanged`. `None` for an unknown one.
    pub fn parse(name: &str) -> Option<Self> {
        match name.trim() {
            "Default" => Some(UpdateSourceTrigger::PropertyChanged),
            n => Self::ALL.into_iter().find(|t| t.name() == n),
        }
    }

    /// The XML name of the trigger.
    pub fn name(self) -> &'static str {
        match self {
            UpdateSourceTrigger::PropertyChanged => "PropertyChanged",
            UpdateSourceTrigger::LostFocus => "LostFocus",
            UpdateSourceTrigger::Explicit => "Explicit",
        }
    }
}


/// How a binding formats and parses its value (WinForms `Binding.FormatString`, `NullValue`,
/// `FormatInfo`): see `kubuno_desktop_views::format`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct BindingFormat {
    /// `N2`, `C`, `d`, `dd/MM/yyyy`, `#,##0.00`…
    pub format_string: Option<String>,
    /// What an empty (NULL) value shows, and what text writes NULL back.
    pub null_value: Option<String>,
    /// `fr-FR`, `en-US`, `invariant`… (default: the process default, else the user's locale).
    pub culture: Option<String>,
}

impl BindingFormat {
    /// No formatting at all (values are shown as held).
    pub fn is_empty(&self) -> bool {
        self.format_string.is_none() && self.null_value.is_none() && self.culture.is_none()
    }
}

/// One `Key=Value` (or bare) part of a `{Binding …}` expression, with its byte ranges in the
/// attribute value as written — what the language server underlines, completes and navigates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BindingPart {
    /// The key (`Mode`), `None` for a bare part (the path, first).
    pub key: Option<String>,
    pub key_range: Option<std::ops::Range<usize>>,
    /// The value as written (quotes included), trimmed.
    pub value: String,
    pub value_range: std::ops::Range<usize>,
}

/// The keys a `{Binding …}` accepts (aliases included).
pub const BINDING_KEYS: &[&str] = &[
    "Path",
    "Source",
    "Mode",
    "UpdateSourceTrigger",
    "Converter",
    "ConverterParameter",
    "FallbackValue",
    "StringFormat",
    "FormatString",
    "TargetNullValue",
    "NullValue",
    "Culture",
    "ConverterCulture",
    "FormatInfo",
];

/// A problem of a `{Binding …}` expression that still parses (the runtime ignores the part): an
/// unknown key, an unknown `Mode` / `UpdateSourceTrigger`, a repeated key, a part after the path
/// without a key. The range is in the attribute value as written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BindingIssue {
    pub message: String,
    pub range: std::ops::Range<usize>,
}

/// The parts of the `{Binding …}` expression `raw` (`None` when it is not one: a `{Res}`, a
/// literal…). Commas inside `'…'` quotes do not split.
pub fn binding_parts(raw: &str) -> Option<Vec<BindingPart>> {
    let open = raw.find('{')?;
    let close = raw.rfind('}')?;
    if close <= open || !raw[..open].trim().is_empty() || !raw[close + 1..].trim().is_empty() {
        return None;
    }
    let inner_start = open + 1 + (raw[open + 1..close].len() - raw[open + 1..close].trim_start().len());
    let rest = raw[inner_start..close].strip_prefix("Binding")?;
    if rest.chars().next().is_some_and(|c| !c.is_whitespace() && c != ',') {
        return None;
    }
    let body_start = inner_start + "Binding".len();
    let body = &raw[body_start..close];
    let mut parts = Vec::new();
    let mut quoted = false;
    let mut start = 0usize;
    let mut push = |from: usize, to: usize| {
        let seg = &body[from..to];
        let lead = seg.len() - seg.trim_start().len();
        let text = seg.trim();
        let at = body_start + from + lead;
        if text.is_empty() {
            return;
        }
        match text.split_once('=') {
            Some((k, v)) => {
                let k_trim = k.trim();
                let v_lead = v.len() - v.trim_start().len();
                let v_at = at + k.len() + 1 + v_lead;
                parts.push(BindingPart {
                    key: Some(k_trim.to_string()),
                    key_range: Some(at..at + k_trim.len()),
                    value: v.trim().to_string(),
                    value_range: v_at..v_at + v.trim().len(),
                });
            }
            None => parts.push(BindingPart { key: None, key_range: None, value: text.to_string(), value_range: at..at + text.len() }),
        }
    };
    for (i, c) in body.char_indices() {
        match c {
            '\'' => quoted = !quoted,
            ',' if !quoted => {
                push(start, i);
                start = i + 1;
            }
            _ => {}
        }
    }
    push(start, body.len());
    Some(parts)
}

/// The canonical name of a binding key (`StringFormat` for `FormatString`…).
pub fn canonical_key(key: &str) -> &'static str {
    match key {
        "FormatString" | "StringFormat" => "StringFormat",
        "NullValue" | "TargetNullValue" => "TargetNullValue",
        "Culture" | "ConverterCulture" | "FormatInfo" => "ConverterCulture",
        other => BINDING_KEYS.iter().find(|k| **k == other).copied().unwrap_or(""),
    }
}

/// A part's value without its `'…'` quotes (`''` inside is one quote).
pub fn unquote(v: &str) -> String {
    let v = v.trim();
    match v.strip_prefix('\'').and_then(|x| x.strip_suffix('\'')) {
        Some(inner) => inner.replace("''", "'"),
        None => v.to_string(),
    }
}

/// Whether `s` is a `{Binding …}` expression by shape alone (`{` … `}`) — the
/// same test [`crate::validate`] (and `kubuno_desktop_views::validate`) uses to skip static type-checking. Shared
/// rather than duplicated: both modules must agree on what "this is a
/// binding, not a literal" means.
pub fn is_binding_expr(s: &str) -> bool {
    s.starts_with('{') && s.ends_with('}') && s.len() >= 2
}


/// A parsed `{Binding Path[, Mode=…][, Converter=…]…}` (or `{Res key}`) attribute value: the grammar's
/// result, without the run-time state `kubuno_desktop_views::binding::BindingSpec` adds to it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct BindingSyntax {
    /// The path (`Source=customers, Path=Name` is `customers.Name`); a resource reference's path
    /// starts with [`crate::res::RES_PREFIX`].
    pub path: String,
    pub mode: BindingMode,
    /// `FormatString=`, `NullValue=`, `Culture=`.
    pub format: BindingFormat,
    /// `Converter=Name`.
    pub converter: Option<String>,
    /// `ConverterParameter=…`.
    pub converter_parameter: Option<String>,
    /// `FallbackValue=…`.
    pub fallback_value: Option<String>,
    /// `UpdateSourceTrigger=…`.
    pub update_trigger: UpdateSourceTrigger,
}

/// Parses a `{Binding Path}` / `{Binding Path, Mode=TwoWay}` / `{Res key}` attribute value. `None`
/// for anything that is not one of these shapes at all — a caller that already checked
/// [`is_binding_expr`] treats `None` here as a malformed binding, not "not a binding".
pub fn parse_binding_syntax(raw: &str) -> Option<BindingSyntax> {
    let inner = raw.strip_prefix('{')?.strip_suffix('}')?.trim();
    // `{Res key[, Source=set]}` (vskubuno docs/RESOURCES.md): a resource reference, carried as a
    // one-way binding whose path cannot be a view-model path (`crate::res`).
    if let Some(path) = crate::res::parse_res_path(inner) {
        return Some(BindingSyntax { path, mode: BindingMode::OneWay, ..BindingSyntax::default() });
    }
    // `Binding` must be a whole word (`BindingX` is not one): `binding_parts` checks it.
    let parts = binding_parts(raw.trim())?;
    let mut path: Option<String> = None;
    let mut source: Option<String> = None;
    let mut spec = BindingSyntax::default();
    for (i, part) in parts.iter().enumerate() {
        let v = part.value.as_str();
        match part.key.as_deref() {
            // An unknown mode stays the default (the language server warns about it).
            Some("Mode") => spec.mode = BindingMode::parse(v).unwrap_or_default(),
            Some("UpdateSourceTrigger") => spec.update_trigger = UpdateSourceTrigger::parse(v).unwrap_or_default(),
            Some("Path") if !v.is_empty() => path = Some(v.to_string()),
            Some("Source") if !v.is_empty() => source = Some(v.to_string()),
            Some("Converter") => spec.converter = Some(v.to_string()).filter(|c| !c.is_empty()),
            Some("ConverterParameter") => spec.converter_parameter = Some(unquote(v)),
            Some("FallbackValue") => spec.fallback_value = Some(unquote(v)),
            Some("FormatString" | "StringFormat") => spec.format.format_string = Some(unquote(v)).filter(|f| !f.is_empty()),
            Some("NullValue" | "TargetNullValue") => spec.format.null_value = Some(unquote(v)),
            Some("Culture" | "ConverterCulture" | "FormatInfo") => spec.format.culture = Some(unquote(v)).filter(|c| !c.is_empty()),
            Some(_) => {}
            // The bare first part is the path (`{Binding Name, Mode=TwoWay}`).
            None if i == 0 && !v.is_empty() => path = Some(v.to_string()),
            None => {}
        }
    }
    // `Source=customers, Path=Name` (a named data component, `vskubuno/docs/DATA.md` §7) is the
    // path `customers.Name`; `Source=customers` alone is `customers`.
    let path = match (source, path) {
        (Some(source), Some(path)) => format!("{source}.{path}"),
        (Some(source), None) => source,
        (None, Some(path)) => path,
        (None, None) => return None,
    };
    spec.path = path;
    Some(spec)
}

/// The problems of the `{Binding …}` expression `raw` that still parses (see [`BindingIssue`]);
/// empty for anything that is not a `{Binding …}` (a `{Res}`, a literal).
pub fn binding_issues(raw: &str) -> Vec<BindingIssue> {
    let mut issues = Vec::new();
    let Some(parts) = binding_parts(raw) else { return issues };
    let mut seen: Vec<&str> = Vec::new();
    for (i, part) in parts.iter().enumerate() {
        let Some(key) = part.key.as_deref() else {
            if i > 0 {
                issues.push(BindingIssue { message: format!("`{}` has no key: only the first part of a binding is its path (write `Key=Value`)", part.value), range: part.value_range.clone() });
            }
            continue;
        };
        let range = part.key_range.clone().unwrap_or(part.value_range.clone());
        if !BINDING_KEYS.contains(&key) {
            let close = BINDING_KEYS.iter().find(|k| k.eq_ignore_ascii_case(key)).map(|k| format!(" (did you mean `{k}`?)")).unwrap_or_default();
            issues.push(BindingIssue { message: format!("unknown binding key `{key}`{close}: it is ignored. Known keys: {}", BINDING_KEYS.join(", ")), range });
            continue;
        }
        let canonical = canonical_key(key);
        if seen.contains(&canonical) {
            issues.push(BindingIssue { message: format!("`{key}` is given twice: the last one wins"), range: range.clone() });
        }
        seen.push(canonical);
        let v = part.value.as_str();
        match canonical {
            "Mode" if BindingMode::parse(v).is_none() => issues.push(BindingIssue {
                message: format!("unknown binding mode `{v}`: expected {}", BindingMode::ALL.map(BindingMode::name).join(", ")),
                range: part.value_range.clone(),
            }),
            "UpdateSourceTrigger" if UpdateSourceTrigger::parse(v).is_none() => issues.push(BindingIssue {
                message: format!("unknown UpdateSourceTrigger `{v}`: expected {}", UpdateSourceTrigger::ALL.map(UpdateSourceTrigger::name).join(", ")),
                range: part.value_range.clone(),
            }),
            "Path" | "Source" | "Converter" if v.is_empty() => issues.push(BindingIssue { message: format!("`{key}` is empty"), range: range.clone() }),
            _ => {}
        }
        if v.starts_with('\'') && (v.len() < 2 || !v.ends_with('\'')) {
            issues.push(BindingIssue { message: format!("the value of `{key}` opens a quote it does not close"), range: part.value_range.clone() });
        }
    }
    issues
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_paths_keys_and_sources() {
        let b = parse_binding_syntax("{Binding Name, Mode=TwoWay, StringFormat='N2', UpdateSourceTrigger=LostFocus}").expect("a binding");
        assert_eq!(b.path, "Name");
        assert_eq!(b.mode, BindingMode::TwoWay);
        assert_eq!(b.format.format_string.as_deref(), Some("N2"));
        assert_eq!(b.update_trigger, UpdateSourceTrigger::LostFocus);
        assert_eq!(parse_binding_syntax("{Binding Source=customers, Path=Name}").map(|b| b.path).as_deref(), Some("customers.Name"));
        assert_eq!(parse_binding_syntax("{Binding ConverterParameter='x'}"), None);
        assert_eq!(parse_binding_syntax("{BindingX Name}"), None);
        assert_eq!(parse_binding_syntax("plain"), None);
    }

    #[test]
    fn a_resource_reference_is_a_one_way_binding_of_a_resource_path() {
        let b = parse_binding_syntax("{Res title, Source=strings}").expect("a resource");
        assert_eq!(b.path, format!("{}strings/title", crate::res::RES_PREFIX));
        assert_eq!(b.mode, BindingMode::OneWay);
    }

    #[test]
    fn reports_the_problems_with_their_ranges() {
        let raw = "{Binding Name, mode=TwoWay, Mode=Sideways, Name}";
        let issues = binding_issues(raw);
        let messages: Vec<&str> = issues.iter().map(|i| i.message.as_str()).collect();
        assert!(messages.iter().any(|m| m.starts_with("unknown binding key `mode` (did you mean `Mode`?)")), "{messages:?}");
        assert!(messages.iter().any(|m| m.starts_with("unknown binding mode `Sideways`")), "{messages:?}");
        assert!(messages.iter().any(|m| m.starts_with("`Name` has no key")), "{messages:?}");
        let sideways = issues.iter().find(|i| i.message.contains("Sideways")).expect("mode issue");
        assert_eq!(&raw[sideways.range.clone()], "Sideways");
        assert!(binding_issues("{Res title}").is_empty());
    }
}
