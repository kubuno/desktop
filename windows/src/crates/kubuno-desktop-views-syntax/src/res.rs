//! `{Res key}` — the grammar of resource references (`vskubuno/docs/RESOURCES.md`, `VIEWS-SPEC.md`
//! §6.2, `WEB-VIEWS.md` §2.4): `{Res key}`, `{Res key, Source=set}`, `{Res Key=key, Set=set}`, and with
//! arguments (WV-6) `{Res key, Count={Binding n}, Name={Binding user.name}, Sep=', '}`.
//!
//! `Key=` / `Path=` / `ResourceKey=` name the key, `Source=` / `Set=` / `File=` the set (the `.kbres` file
//! stem); any other `Name=value` is an **argument**: its value is a nested `{Binding …}` (brace-balanced,
//! commas allowed inside) or a literal up to the next top-level comma (`'…'` quotes may hold commas, `''`
//! is one quote). Arguments fill the string's `{{name}}` placeholders, and `Count` selects its plural form
//! (`kubuno_desktop_resources_model::plural`).
//!
//! A reference is carried like a one-way binding whose path starts with [`RES_PREFIX`] — a path no view
//! model can hold — with the arguments beside it (`BindingSyntax::res_args`). Resolving it (the `.kbres`
//! lookup, the UI culture) is the runtime's (`kubuno_desktop_views::resources`, which re-exports this
//! module's items).

use std::ops::Range;

/// The path prefix of a resource reference's binding.
pub const RES_PREFIX: &str = "@res:";

/// The value of a `{Res}` argument.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResArgValue {
    /// A nested binding, as written with its braces (`{Binding n, Mode=OneWay}`): the text
    /// [`crate::binding::parse_binding_syntax`] takes.
    Binding(String),
    /// A literal, trimmed, its `'…'` quotes removed.
    Literal(String),
}

/// One argument of a `{Res}` reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResArg {
    /// The name as written (`Count`, `Name`).
    pub name: String,
    pub value: ResArgValue,
    /// The byte range of the whole argument (`Count={Binding n}`) in the text given to [`parse_res`].
    pub range: Range<usize>,
    /// The byte range of the value as written (the nested binding with its braces, a literal with its quotes).
    pub value_range: Range<usize>,
}

/// A problem of a `{Res}` expression that still parses (the part is ignored): an argument without a name
/// or a value, a name that is not an identifier, a value that is neither a `{Binding …}` nor a literal, an
/// unclosed brace or quote. The range is in the text given to [`parse_res`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResIssue {
    pub message: String,
    pub range: Range<usize>,
}

/// A parsed `{Res …}` expression.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ResSyntax {
    pub key: String,
    /// The set (`.kbres` stem, without the extension).
    pub set: Option<String>,
    pub args: Vec<ResArg>,
    pub issues: Vec<ResIssue>,
}

impl ResSyntax {
    /// The binding path of the reference (`@res:set/key`, `@res:key`).
    pub fn path(&self) -> String {
        match &self.set {
            Some(set) => format!("{RES_PREFIX}{set}/{}", self.key),
            None => format!("{RES_PREFIX}{}", self.key),
        }
    }
}

fn is_identifier(s: &str) -> bool {
    let mut c = s.chars();
    c.next().is_some_and(|f| f.is_ascii_alphabetic() || f == '_') && c.all(|x| x.is_ascii_alphanumeric() || x == '_')
}

/// The top-level comma-separated parts of `body` (braces balanced, `'…'` quotes at depth 0 respected):
/// `(start, end)` byte ranges in `body`, and whether a brace or a quote was left open.
fn split_parts(body: &str) -> (Vec<(usize, usize)>, Option<&'static str>) {
    let mut parts = Vec::new();
    let mut depth = 0usize;
    let mut quoted = false;
    let mut start = 0usize;
    for (i, c) in body.char_indices() {
        match c {
            '\'' if depth == 0 => quoted = !quoted,
            '{' if !quoted => depth += 1,
            '}' if !quoted => depth = depth.saturating_sub(1),
            ',' if !quoted && depth == 0 => {
                parts.push((start, i));
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push((start, body.len()));
    let open = if depth > 0 {
        Some("a `{` is not closed")
    } else if quoted {
        Some("a `'` quote is not closed")
    } else {
        None
    };
    (parts, open)
}

/// A literal's text: trimmed, `'…'` quotes removed (`''` inside is one quote).
fn unquote(v: &str) -> String {
    let v = v.trim();
    match v.strip_prefix('\'').and_then(|x| x.strip_suffix('\'')) {
        Some(inner) if v.len() >= 2 => inner.replace("''", "'"),
        _ => v.to_string(),
    }
}

/// Parses the inside of `{Res …}` (without the outer braces); `None` when it is not a `Res` expression or
/// names no key. Ranges are byte offsets in `inner`.
pub fn parse_res(inner: &str) -> Option<ResSyntax> {
    let lead = inner.len() - inner.trim_start().len();
    let rest = inner[lead..].strip_prefix("Res")?;
    if let Some(c) = rest.chars().next() {
        if !c.is_whitespace() && c != ',' {
            return None;
        }
    }
    let body_start = lead + 3;
    let body = &inner[body_start..];
    let (parts, open) = split_parts(body);
    let mut out = ResSyntax::default();
    let mut key: Option<String> = None;
    if let Some(message) = open {
        out.issues.push(ResIssue { message: message.to_string(), range: body_start..inner.len() });
    }
    for (i, (from, to)) in parts.into_iter().enumerate() {
        let seg = &body[from..to];
        let text = seg.trim();
        if text.is_empty() {
            continue;
        }
        let at = body_start + from + (seg.len() - seg.trim_start().len());
        let range = at..at + text.len();
        let Some((name, value)) = text.split_once('=') else {
            if i == 0 {
                key = Some(text.to_string());
            } else {
                out.issues.push(ResIssue { message: format!("`{text}` has no name: write `Name=value` (only the first part of `{{Res}}` is the key)"), range });
            }
            continue;
        };
        let name = name.trim();
        let value_text = value.trim();
        let value_at = at + text.len() - value.len() + (value.len() - value.trim_start().len());
        let value_range = value_at..value_at + value_text.len();
        match name {
            "Key" | "Path" | "ResourceKey" if !value_text.is_empty() => key = Some(value_text.to_string()),
            "Source" | "Set" | "File" if !value_text.is_empty() => out.set = Some(value_text.trim_end_matches(".kbres").to_string()),
            "Key" | "Path" | "ResourceKey" | "Source" | "Set" | "File" => out.issues.push(ResIssue { message: format!("`{name}` is empty"), range }),
            _ if !is_identifier(name) => out.issues.push(ResIssue { message: format!("`{name}` is not an argument name (letters, digits and `_`, not starting with a digit)"), range }),
            _ if value_text.is_empty() => out.issues.push(ResIssue { message: format!("the argument `{name}` has no value"), range }),
            _ if value_text.starts_with('{') => {
                let ok = value_text.ends_with('}') && value_text[1..].trim_start().starts_with("Binding") && crate::binding::parse_binding_syntax(value_text).is_some();
                if ok {
                    out.args.push(ResArg { name: name.to_string(), value: ResArgValue::Binding(value_text.to_string()), range, value_range });
                } else {
                    out.issues.push(ResIssue {
                        message: format!("the value of `{name}` must be a `{{Binding path}}` or a literal, found `{value_text}`"),
                        range: value_range,
                    });
                }
            }
            _ => {
                if value_text.starts_with('\'') && (value_text.len() < 2 || !value_text.ends_with('\'')) {
                    out.issues.push(ResIssue { message: format!("the value of `{name}` opens a quote it does not close"), range: value_range });
                    continue;
                }
                out.args.push(ResArg { name: name.to_string(), value: ResArgValue::Literal(unquote(value_text)), range, value_range });
            }
        }
    }
    let mut seen: Vec<&str> = Vec::new();
    for a in &out.args {
        if seen.contains(&a.name.as_str()) {
            out.issues.push(ResIssue { message: format!("the argument `{}` is given twice: the last one wins", a.name), range: a.range.clone() });
        }
        seen.push(&a.name);
    }
    out.key = key?;
    Some(out)
}

/// Parses the inside of `{Res key[, Source=set][, Name=value]…}` (without the braces) into its binding path
/// (`@res:set/key`, `@res:key`); `None` when it is not a `Res` expression. Arguments are ignored.
pub fn parse_res_path(inner: &str) -> Option<String> {
    parse_res(inner).map(|r| r.path())
}

/// `(set, key)` of a resource reference's binding path; `None` for an ordinary binding path.
pub fn res_reference(path: &str) -> Option<(Option<&str>, &str)> {
    let rest = path.strip_prefix(RES_PREFIX)?;
    Some(match rest.split_once('/') {
        Some((set, key)) => (Some(set), key),
        None => (None, rest),
    })
}

/// Whether `raw` (an attribute value as written) is a `{Res …}` expression.
pub fn is_res_expr(raw: &str) -> bool {
    raw.trim().strip_prefix('{').and_then(|r| r.strip_suffix('}')).and_then(parse_res_path).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_key_and_the_set() {
        assert_eq!(parse_res_path("Res title").as_deref(), Some("@res:title"));
        assert_eq!(parse_res_path("Res title, Source=strings.kbres").as_deref(), Some("@res:strings/title"));
        assert_eq!(parse_res_path("Res Key=save, Set=icons").as_deref(), Some("@res:icons/save"));
        assert_eq!(parse_res_path("Resource title"), None);
        assert_eq!(parse_res_path("Res"), None);
        assert_eq!(res_reference("@res:strings/title"), Some((Some("strings"), "title")));
        assert_eq!(res_reference("@res:title"), Some((None, "title")));
        assert_eq!(res_reference("Title"), None);
        assert!(is_res_expr(" {Res title} "));
        assert!(!is_res_expr("{Binding title}"));
    }

    #[test]
    fn parses_arguments() {
        let inner = "Res files, Count={Binding n, Mode=OneWay}, Name={Binding user.name}, Sep=', ', Source=drive, Plain = x ";
        let r = parse_res(inner).expect("res");
        assert_eq!(r.key, "files");
        assert_eq!(r.set.as_deref(), Some("drive"));
        assert!(r.issues.is_empty(), "{:?}", r.issues);
        let args: Vec<(&str, &ResArgValue)> = r.args.iter().map(|a| (a.name.as_str(), &a.value)).collect();
        assert_eq!(
            args,
            vec![
                ("Count", &ResArgValue::Binding("{Binding n, Mode=OneWay}".into())),
                ("Name", &ResArgValue::Binding("{Binding user.name}".into())),
                ("Sep", &ResArgValue::Literal(", ".into())),
                ("Plain", &ResArgValue::Literal("x".into())),
            ]
        );
        assert_eq!(&inner[r.args[0].range.clone()], "Count={Binding n, Mode=OneWay}");
        assert_eq!(&inner[r.args[0].value_range.clone()], "{Binding n, Mode=OneWay}");
        assert_eq!(&inner[r.args[2].value_range.clone()], "', '");
        assert_eq!(&inner[r.args[3].range.clone()], "Plain = x");
        // The path ignores the arguments; the older helpers keep working on the new forms.
        assert_eq!(parse_res_path(inner).as_deref(), Some("@res:drive/files"));
        assert!(is_res_expr(&format!("{{{inner}}}")));
        assert_eq!(parse_res("Res Key=a, It=''''").map(|r| r.args[0].value.clone()), Some(ResArgValue::Literal("'".into())));
    }

    #[test]
    fn reports_malformed_arguments() {
        let inner = "Res k, 1x=2, Count={Res other}, Name=, bare, Sep='a, N={Binding}, N2=2, N2=3";
        let r = parse_res(inner).expect("still a reference");
        let messages: Vec<&str> = r.issues.iter().map(|i| i.message.as_str()).collect();
        assert!(messages.iter().any(|m| m.contains("`1x` is not an argument name")), "{messages:?}");
        assert!(messages.iter().any(|m| m.contains("value of `Count` must be a `{Binding path}`")), "{messages:?}");
        assert!(messages.iter().any(|m| m.contains("`Name` has no value")), "{messages:?}");
        assert!(messages.iter().any(|m| m.contains("`bare` has no name")), "{messages:?}");
        assert!(messages.iter().any(|m| m.contains("quote is not closed")), "{messages:?}");
        let count = r.issues.iter().find(|i| i.message.contains("`Count`")).expect("count issue");
        assert_eq!(&inner[count.range.clone()], "{Res other}");
        let unclosed = parse_res("Res k, Count={Binding n").expect("res");
        assert!(unclosed.issues.iter().any(|i| i.message.contains("`{` is not closed")));
        assert!(unclosed.args.is_empty());
        let twice = parse_res("Res k, A=1, A=2").expect("res");
        assert!(twice.issues.iter().any(|i| i.message.contains("given twice")));
    }
}
