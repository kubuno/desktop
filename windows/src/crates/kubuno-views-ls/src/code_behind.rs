//! Reading a view's Rust code-behind as text (`vskubuno/docs/EVENTS.md` §5.4, EVT-4): the
//! `#[kubuno_views::event_handlers]` impl block a typed handler stub goes into, the legacy
//! `handlers! { … }` table and its entries (for the "convert to typed handlers" action), the
//! view model type, and the `use kubuno_views::prelude::*;` import.
//!
//! Like [`crate::definition`] and [`crate::handler_insert`], this is a small scanner, not a
//! Rust parser (the server does not link one): it skips string, raw string and char literals
//! and comments, and matches brackets. Every function answers `None` rather than guessing
//! when the text is not in the expected shape.

/// Whether `b` can continue an identifier.
fn ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// If a comment, string, raw string or char literal starts at `i`, the index just past it.
pub(crate) fn skip_non_code(text: &str, i: usize) -> Option<usize> {
    let b = text.as_bytes();
    let len = b.len();
    match b[i] {
        b'/' if b.get(i + 1) == Some(&b'/') => Some(text[i..].find('\n').map_or(len, |n| i + n)),
        b'/' if b.get(i + 1) == Some(&b'*') => {
            let (mut depth, mut j) = (1, i + 2);
            while j < len && depth > 0 {
                if b[j] == b'/' && b.get(j + 1) == Some(&b'*') {
                    depth += 1;
                    j += 2;
                } else if b[j] == b'*' && b.get(j + 1) == Some(&b'/') {
                    depth -= 1;
                    j += 2;
                } else {
                    j += 1;
                }
            }
            Some(j)
        }
        b'"' => {
            let mut j = i + 1;
            while j < len {
                match b[j] {
                    b'\\' => j += 2,
                    b'"' => return Some(j + 1),
                    _ => j += 1,
                }
            }
            Some(len)
        }
        b'r' if (i == 0 || !ident_byte(b[i - 1]) || (b[i - 1] == b'b' && (i < 2 || !ident_byte(b[i - 2])))) => {
            let mut j = i + 1;
            while j < len && b[j] == b'#' {
                j += 1;
            }
            if b.get(j) != Some(&b'"') {
                return None;
            }
            let hashes = j - i - 1;
            let closing = format!("\"{}", "#".repeat(hashes));
            Some(text[j + 1..].find(&closing).map_or(len, |n| j + 1 + n + closing.len()))
        }
        b'\'' => {
            if b.get(i + 1) == Some(&b'\\') {
                return text[i + 2..].find('\'').map(|n| i + 2 + n + 1);
            }
            let c = text[i + 1..].chars().next()?;
            let after = i + 1 + c.len_utf8();
            (b.get(after) == Some(&b'\'')).then_some(after + 1) // else a lifetime
        }
        _ => None,
    }
}

/// The index of the bracket closing the one at `open` (`{`, `(` or `[`), skipping literals
/// and comments.
pub(crate) fn match_bracket(text: &str, open: usize) -> Option<usize> {
    let b = text.as_bytes();
    let mut depth = 0i32;
    let mut i = open;
    while i < b.len() {
        if let Some(next) = skip_non_code(text, i) {
            i = next;
            continue;
        }
        match b[i] {
            b'{' | b'(' | b'[' => depth += 1,
            b'}' | b')' | b']' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// Every index at which `needle` occurs in `text` as code (not inside a literal or comment).
fn find_in_code(text: &str, needle: &str) -> Vec<usize> {
    let b = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if let Some(next) = skip_non_code(text, i) {
            i = next;
            continue;
        }
        if text[i..].starts_with(needle) {
            out.push(i);
            i += needle.len();
        } else {
            i += text[i..].chars().next().map_or(1, char::len_utf8);
        }
    }
    out
}

/// Skips whitespace and comments from `i`.
fn skip_trivia(text: &str, mut i: usize) -> usize {
    let b = text.as_bytes();
    while i < b.len() {
        if b[i].is_ascii_whitespace() {
            i += 1;
        } else if b[i] == b'/' && matches!(b.get(i + 1), Some(b'/') | Some(b'*')) {
            i = skip_non_code(text, i).unwrap_or(b.len());
        } else {
            break;
        }
    }
    i
}

/// Byte offset of the start of the line containing `offset`.
pub(crate) fn line_start_of(text: &str, offset: usize) -> usize {
    text[..offset].rfind('\n').map(|nl| nl + 1).unwrap_or(0)
}

pub(crate) fn leading_whitespace(line_onward: &str) -> String {
    line_onward.chars().take_while(|c| *c == ' ' || *c == '\t').collect()
}

/// An `impl` block: where it starts (its first attribute, if any), its braces, its type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ImplBlock {
    /// Start of the item (the `#[…]` in front of the `impl` for a typed impl).
    pub start: usize,
    pub open: usize,
    pub close: usize,
    /// The implementing type as written (`MainViewModel`).
    pub self_ty: String,
    /// The inherent `impl` of a `#[kubuno::view]` struct (the Windows Forms-like form class, whose
    /// handlers are plain methods: `fn hello_click(&mut self, sender: &Button, e: &MouseEventArgs)`).
    pub view: bool,
}

/// The struct marked `#[kubuno::view(…)]` (or `#[view(…)]` after an import): its name and the offset
/// just past the struct item (its closing `}` or `;`).
pub(crate) fn find_view_struct(text: &str) -> Option<(String, usize)> {
    for at in find_in_code(text, "view") {
        let b = text.as_bytes();
        if b.get(at + 4).is_some_and(|c| ident_byte(*c)) {
            continue;
        }
        let mut before = text[..at].trim_end();
        if let Some(path) = before.strip_suffix("::") {
            match path.trim_end().strip_suffix("kubuno") {
                Some(p) => before = p.trim_end(),
                None => continue,
            }
        }
        if !before.ends_with('[') || !before[..before.len() - 1].trim_end().ends_with('#') {
            continue;
        }
        let open = skip_trivia(text, at + 4);
        if b.get(open) != Some(&b'(') {
            continue;
        }
        let close_paren = match_bracket(text, open)?;
        let close_attr = skip_trivia(text, close_paren + 1);
        if b.get(close_attr) != Some(&b']') {
            continue;
        }
        // Other attributes (`#[derive(Default)]`), visibility, then `struct Name`.
        let mut i = close_attr + 1;
        loop {
            i = skip_trivia(text, i);
            if text[i..].starts_with("#[") {
                i = match_bracket(text, i + 1)? + 1;
                continue;
            }
            match ident_at(text, i) {
                Some("struct") => {
                    let name_at = skip_trivia(text, i + "struct".len());
                    let name = ident_at(text, name_at)?.to_string();
                    let mut j = name_at + name.len();
                    while j < text.len() {
                        if let Some(next) = skip_non_code(text, j) {
                            j = next;
                            continue;
                        }
                        match b[j] {
                            b';' => return Some((name, j + 1)),
                            b'{' => return Some((name, match_bracket(text, j)? + 1)),
                            _ => j += 1,
                        }
                    }
                    return None;
                }
                Some(word) if word == "pub" || word == "crate" || word == "super" || word == "in" => {
                    i += word.len();
                    let k = skip_trivia(text, i);
                    if b.get(k) == Some(&b'(') {
                        i = match_bracket(text, k)? + 1;
                    }
                }
                _ => break,
            }
        }
    }
    None
}

/// The first inherent `impl Name { … }` of `name` (no trait, no generics).
pub(crate) fn find_inherent_impl(text: &str, name: &str) -> Option<ImplBlock> {
    for at in find_in_code(text, "impl") {
        if at > 0 && ident_byte(text.as_bytes()[at - 1]) || text.as_bytes().get(at + 4).is_some_and(|b| ident_byte(*b)) {
            continue;
        }
        let Some(open_rel) = text[at..].find('{') else { continue };
        let open = at + open_rel;
        if text[at + 4..open].trim() != name {
            continue;
        }
        let close = match_bracket(text, open)?;
        return Some(ImplBlock { start: at, open, close, self_ty: name.to_string(), view: true });
    }
    None
}

/// The handler impl of a `#[kubuno::view]` code-behind: the view struct's inherent `impl`.
pub(crate) fn find_view_impl(text: &str) -> Option<ImplBlock> {
    let (name, _) = find_view_struct(text)?;
    find_inherent_impl(text, &name)
}

/// The prelude a code-behind imports: `kubuno::prelude::*` for a `#[kubuno::view]` form class,
/// `kubuno_views::prelude::*` otherwise.
pub(crate) fn prelude_path(text: &str) -> &'static str {
    if find_view_struct(text).is_some() || text.contains("kubuno::prelude::*") {
        "kubuno::prelude"
    } else {
        "kubuno_views::prelude"
    }
}

/// The first `#[kubuno_views::event_handlers]` (or `#[event_handlers]`) `impl` block.
pub(crate) fn find_typed_impl(text: &str) -> Option<ImplBlock> {
    for at in find_in_code(text, "event_handlers") {
        // `#[` [`kubuno_views` `::`] `event_handlers` `]`
        let mut before = text[..at].trim_end();
        if let Some(path) = before.strip_suffix("::") {
            // `kubuno_views::event_handlers`, or `kubuno::views::event_handlers` in an application that reaches
            // `kubuno_views` through the `kubuno` facade (what the item templates write there).
            let path = path.trim_end();
            match path.strip_suffix("kubuno_views").or_else(|| path.strip_suffix("views").map(str::trim_end).and_then(|p| p.strip_suffix("::")).map(str::trim_end).and_then(|p| p.strip_suffix("kubuno"))) {
                Some(b) => before = b.trim_end(),
                None => continue,
            }
        }
        let Some(hash_bracket) = before.strip_suffix('[').map(str::trim_end).and_then(|b| b.strip_suffix('#')) else { continue };
        let start = hash_bracket.len();
        let after = skip_trivia(text, at + "event_handlers".len());
        if text.as_bytes().get(after) != Some(&b']') {
            continue;
        }
        let impl_at = skip_trivia(text, after + 1);
        if !text[impl_at..].starts_with("impl") {
            continue;
        }
        let open = impl_at + text[impl_at..].find('{')?;
        let close = match_bracket(text, open)?;
        let header = text[impl_at + 4..open].trim();
        let self_ty = header.strip_prefix('<').map_or(header, |g| g.split_once('>').map_or(g, |(_, rest)| rest)).trim();
        let self_ty = self_ty.split(" where").next().unwrap_or(self_ty).trim().to_string();
        return Some(ImplBlock { start, open, close, self_ty, view: false });
    }
    // A `#[kubuno::view]` form class: its handlers are the plain methods of its inherent impl.
    find_view_impl(text)
}

/// The `impl ViewModel for X` block (any path to `ViewModel`), when there is exactly one.
pub(crate) fn find_view_model_impl(text: &str) -> Option<ImplBlock> {
    let mut found = Vec::new();
    for at in find_in_code(text, "impl") {
        if at > 0 && ident_byte(text.as_bytes()[at - 1]) || text.as_bytes().get(at + 4).is_some_and(|b| ident_byte(*b)) {
            continue;
        }
        let Some(open_rel) = text[at..].find('{') else { continue };
        let open = at + open_rel;
        let header = text[at + 4..open].trim();
        let Some((trait_path, ty)) = header.split_once(" for ") else { continue };
        if trait_path.trim().rsplit(|c: char| c.is_whitespace() || c == ':').next() != Some("ViewModel") {
            continue;
        }
        let Some(close) = match_bracket(text, open) else { continue };
        let self_ty = ty.split(" where").next().unwrap_or(ty).trim().to_string();
        found.push(ImplBlock { start: at, open, close, self_ty, view: false });
    }
    (found.len() == 1).then(|| found.remove(0))
}

/// Whether the file already imports the prelude.
pub(crate) fn has_prelude(text: &str) -> bool {
    text.contains("kubuno_views::prelude::*") || text.contains("kubuno::prelude::*") || text.contains("kubuno::views::prelude::*")
}

/// Where `use kubuno_views::prelude::*;` (`use kubuno::prelude::*;` in a `#[kubuno::view]` code-behind,
/// see [`prelude_path`]) goes: after the last top-level `use …;` line, else
/// after the leading `//!` doc comments and inner attributes, else at the very start. Returns
/// the offset (a line start) and the text to insert there.
pub(crate) fn prelude_insertion(text: &str, nl: &str) -> (usize, String) {
    let prelude = prelude_path(text);
    let mut last_use_end = None;
    let mut offset = 0;
    let mut header_end = 0;
    let mut in_header = true;
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim_start();
        let top_level = line.len() == trimmed.len();
        if in_header && (trimmed.starts_with("//!") || trimmed.starts_with("#![") || trimmed.trim().is_empty()) {
            header_end = offset + line.len();
        } else {
            in_header = false;
        }
        if top_level && (trimmed.starts_with("use ") || trimmed.starts_with("pub use ")) {
            // A multi-line `use` ends at its `;`.
            let end = text[offset..].find(';').map_or(offset + line.len(), |n| offset + n + 1);
            let end = text[end..].find('\n').map_or(text.len(), |n| end + n + 1);
            last_use_end = Some(end);
        }
        offset += line.len();
    }
    match last_use_end {
        Some(end) => (end, format!("use {prelude}::*;{nl}")),
        None if header_end > 0 => (header_end, format!("use {prelude}::*;{nl}{nl}")),
        None => (0, format!("use {prelude}::*;{nl}{nl}")),
    }
}

/// One entry of a legacy table: `"name" => |vm, value| body`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TableEntry {
    pub name: String,
    /// The two closure parameter patterns as written (`vm`, `_v`).
    pub vm_pat: String,
    pub value_pat: String,
    /// The closure body as written: a block `{ … }` or an expression.
    pub body: String,
    /// Offset of the entry's opening `"`, of the name inside the quotes, of the body, and just past the
    /// entry (its trailing `,` included when it has one).
    pub start: usize,
    pub name_at: usize,
    pub body_at: usize,
    pub end: usize,
}

/// A `handlers! { … }` invocation: the offset of `handlers!`, its braces, its entries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HandlersTable {
    pub at: usize,
    pub open: usize,
    pub close: usize,
    pub entries: Vec<TableEntry>,
}

/// The first `handlers! { … }` table, with its entries parsed; `None` when there is none or an
/// entry is not in the `"name" => |a, b| body` shape.
pub(crate) fn find_handlers_table(text: &str) -> Option<HandlersTable> {
    let at = find_in_code(text, "handlers!").into_iter().find(|&at| at == 0 || !ident_byte(text.as_bytes()[at - 1]))?;
    let open = skip_trivia(text, at + "handlers!".len());
    if !matches!(text.as_bytes().get(open), Some(b'{') | Some(b'(') | Some(b'[')) {
        return None;
    }
    let close = match_bracket(text, open)?;
    let mut entries = Vec::new();
    let b = text.as_bytes();
    let mut i = skip_trivia(text, open + 1);
    while i < close {
        // "name"
        let i_entry = i;
        if b[i] != b'"' {
            return None;
        }
        let name_end = skip_non_code(text, i)?;
        let name = text[i + 1..name_end - 1].to_string();
        // =>
        i = skip_trivia(text, name_end);
        if !text[i..].starts_with("=>") {
            return None;
        }
        i = skip_trivia(text, i + 2);
        if text[i..].starts_with("move") && !b.get(i + 4).is_some_and(|c| ident_byte(*c)) {
            i = skip_trivia(text, i + 4);
        }
        // |a, b|
        if b.get(i) != Some(&b'|') {
            return None;
        }
        let params_end = i + 1 + text[i + 1..close].find('|')?;
        let params: Vec<&str> = text[i + 1..params_end].split(',').map(str::trim).collect();
        let [vm_pat, value_pat] = params[..] else { return None };
        // body, up to a top-level `,` or the table's end
        let body_start = skip_trivia(text, params_end + 1);
        let mut j = body_start;
        let mut depth = 0i32;
        while j < close {
            if let Some(next) = skip_non_code(text, j) {
                j = next;
                continue;
            }
            match b[j] {
                b'{' | b'(' | b'[' => depth += 1,
                b'}' | b')' | b']' => depth -= 1,
                b',' if depth == 0 => break,
                _ => {}
            }
            j += 1;
        }
        let body = text[body_start..j].trim_end().to_string();
        if body.is_empty() {
            return None;
        }
        let entry_start = i_entry;
        let end = if j < close { j + 1 } else { j };
        entries.push(TableEntry {
            name,
            vm_pat: vm_pat.to_string(),
            value_pat: value_pat.to_string(),
            body,
            start: entry_start,
            name_at: entry_start + 1,
            body_at: body_start,
            end: if j < close { end } else { body_start + text[body_start..j].trim_end().len() },
        });
        i = skip_trivia(text, end);
    }
    Some(HandlersTable { at, open, close, entries })
}

/// The `.frame(` calls of a `Runtime` that pass a handler table (five arguments): the offsets
/// of their `frame` identifier.
pub(crate) fn legacy_frame_calls(text: &str) -> Vec<usize> {
    find_in_code(text, ".frame(")
        .into_iter()
        .filter(|&at| {
            let open = at + ".frame".len();
            let Some(close) = match_bracket(text, open) else { return false };
            let mut args = 1;
            let mut i = open + 1;
            let mut depth = 0;
            while i < close {
                if let Some(next) = skip_non_code(text, i) {
                    i = next;
                    continue;
                }
                match text.as_bytes()[i] {
                    b'{' | b'(' | b'[' => depth += 1,
                    b'}' | b')' | b']' => depth -= 1,
                    b',' if depth == 0 && !text[i + 1..close].trim().is_empty() => args += 1,
                    _ => {}
                }
                i += 1;
            }
            args == 5
        })
        .map(|at| at + 1)
        .collect()
}

/// Whether `name` is a plain Rust identifier usable as a method name.
pub(crate) fn is_method_ident(name: &str) -> bool {
    const KEYWORDS: &[&str] = &[
        "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum", "extern", "false", "fn", "for", "if", "impl",
        "in", "let", "loop", "match", "mod", "move", "mut", "pub", "ref", "return", "self", "Self", "static", "struct", "super", "trait",
        "true", "type", "unsafe", "use", "where", "while", "abstract", "become", "box", "do", "final", "macro", "override", "priv",
        "typeof", "unsized", "virtual", "yield", "try", "gen",
    ];
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
        && name != "_"
        && !KEYWORDS.contains(&name)
}

// ── the methods of a typed impl (EVT-5) ─────────────────────────────────

/// The shape of one parameter after the receiver, as far as handler/event compatibility goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Param {
    /// `&ElementRef` (any element) or `&Sender<AnyElement>`.
    AnySender,
    /// `&Sender<C>`: the control type's last path segment.
    Sender(String),
    /// `&dyn EventArgs` / `&mut dyn EventArgs`: any event.
    AnyArgs,
    /// `&A` / `&mut A`: the type's last path segment and, for a generic type
    /// (`ValueChangedEventArgs<String>`), its type argument as written.
    Args { ty: String, generic: Option<String> },
}

/// One method of a `#[event_handlers]` impl.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Method {
    /// Start of the item: its first doc comment or attribute, else its first keyword.
    pub item_start: usize,
    /// The method's identifier and where it is.
    pub name: String,
    pub name_at: usize,
    /// The handler name the macro dispatches by: the `#[handler(name = "…")]` string, else `name`.
    pub handler_name: String,
    /// The byte range of that string's content, when the method has one.
    pub name_attr: Option<(usize, usize)>,
    /// A handler of the sink: a `self` receiver and no `#[handler(skip)]`.
    pub is_handler: bool,
    /// The parameters after the receiver.
    pub params: Vec<Param>,
    pub body_open: usize,
    pub body_close: usize,
}

/// A parsed `fn`: name, its offset, the top-level parameters as written, the body's braces.
type ParsedFn = (String, usize, Vec<String>, usize, usize);

/// The identifier starting at `i` (ASCII identifiers, like the macro's handler names).
fn ident_at(text: &str, i: usize) -> Option<&str> {
    let b = text.as_bytes();
    if !b.get(i).is_some_and(|c| c.is_ascii_alphabetic() || *c == b'_') {
        return None;
    }
    let mut j = i;
    while j < b.len() && ident_byte(b[j]) {
        j += 1;
    }
    Some(&text[i..j])
}

/// Splits `s` at its top-level commas (brackets and `<…>` nest).
fn split_top_level(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let (mut depth, mut start) = (0i32, 0);
    for (i, c) in s.char_indices() {
        match c {
            '(' | '[' | '{' | '<' => depth += 1,
            ')' | ']' | '}' | '>' => depth -= 1,
            ',' if depth == 0 => {
                out.push(&s[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(&s[start..]);
    out.into_iter().map(str::trim).filter(|p| !p.is_empty()).collect()
}

/// `&Sender<Button>` → `Sender(Button)`, `&mut MouseEventArgs` → `Args`… (`None` for a type that
/// is not a reference, which the macro rejects anyway).
pub(crate) fn classify_param(ty: &str) -> Option<Param> {
    let mut rest = ty.trim().strip_prefix('&')?.trim_start();
    if let Some(lifetime) = rest.strip_prefix('\'') {
        rest = lifetime.trim_start_matches(|c: char| c.is_ascii_alphanumeric() || c == '_').trim_start();
    }
    if let Some(r) = rest.strip_prefix("mut") {
        if r.starts_with(char::is_whitespace) {
            rest = r.trim_start();
        }
    }
    if let Some(r) = rest.strip_prefix("dyn") {
        return (r.trim().rsplit("::").next().map(str::trim) == Some("EventArgs")).then_some(Param::AnyArgs);
    }
    let (path, generic) = match rest.find('<') {
        Some(lt) => (&rest[..lt], rest[lt + 1..].trim_end().strip_suffix('>').map(str::trim)),
        None => (rest, None),
    };
    let last = path.rsplit("::").next().unwrap_or(path).trim().to_string();
    // Lifetime arguments (`Sender<'_, Button>`) carry nothing here.
    let generic = generic.and_then(|g| split_top_level(g).into_iter().rfind(|a| !a.starts_with('\'')).map(str::to_string));
    Some(match last.as_str() {
        "ElementRef" => Param::AnySender,
        // A `#[kubuno::view]` form class types its sender with the control handle (`&Button`), or
        // `&Control` / `&Form` for any element.
        "Control" | "Form" => Param::AnySender,
        name if !name.ends_with("Args") && kubuno_views::registry::lookup(name).is_some() => Param::Sender(name.to_string()),
        "Sender" => match generic.as_deref().map(|g| g.rsplit("::").next().unwrap_or(g).trim()) {
            Some("AnyElement") | None => Param::AnySender,
            Some(control) => Param::Sender(control.to_string()),
        },
        _ => Param::Args { ty: last, generic },
    })
}

/// Reads a `#[handler(…)]` attribute whose `#` is at `at` and `]` at `close`: whether it says
/// `skip`, and the range of its `name = "…"` string's content.
fn handler_attribute(text: &str, at: usize, close: usize) -> Option<(bool, Option<(usize, usize)>)> {
    let inner_start = skip_trivia(text, at + 2);
    if ident_at(text, inner_start) != Some("handler") {
        return None;
    }
    let open = skip_trivia(text, inner_start + "handler".len());
    if text.as_bytes().get(open) != Some(&b'(') {
        return None;
    }
    let paren_close = match_bracket(text, open).filter(|&c| c < close)?;
    let inner = &text[open + 1..paren_close];
    let skip = split_top_level(inner).contains(&"skip");
    let name = inner.find("name").and_then(|n| {
        let q = open + 1 + n + inner[n..].find('"')?;
        let end = skip_non_code(text, q)?;
        Some((q + 1, end - 1))
    });
    Some((skip, name))
}

/// Every method of the impl `imp` (the items between its braces), in order.
pub(crate) fn impl_methods(text: &str, imp: &ImplBlock) -> Vec<Method> {
    let b = text.as_bytes();
    let mut out = Vec::new();
    let mut i = imp.open + 1;
    let mut item_start: Option<usize> = None;
    let mut skip = false;
    let mut name_attr = None;
    while i < imp.close {
        if b[i].is_ascii_whitespace() {
            i += 1;
            continue;
        }
        if text[i..].starts_with("///") || text[i..].starts_with("/**") {
            item_start.get_or_insert(i);
            i = skip_non_code(text, i).unwrap_or(imp.close);
            continue;
        }
        if let Some(next) = skip_non_code(text, i) {
            i = next;
            continue;
        }
        if text[i..].starts_with("#[") {
            item_start.get_or_insert(i);
            let Some(close) = match_bracket(text, i + 1) else { break };
            if let Some((s, n)) = handler_attribute(text, i, close) {
                skip |= s;
                name_attr = name_attr.or(n);
            }
            i = close + 1;
            continue;
        }
        // An item: find its `fn`, or skip it whole.
        let start = *item_start.get_or_insert(i);
        let mut j = i;
        let mut method: Option<ParsedFn> = None;
        while j < imp.close {
            if let Some(next) = skip_non_code(text, j) {
                j = next;
                continue;
            }
            if b[j] == b';' {
                j += 1;
                break;
            }
            if b[j] == b'{' {
                j = match_bracket(text, j).map_or(imp.close, |c| c + 1);
                break;
            }
            if let Some(word) = ident_at(text, j) {
                if word == "fn" && (j == 0 || !ident_byte(b[j - 1])) {
                    method = parse_fn(text, j, imp.close);
                    j = method.as_ref().map_or(imp.close, |m| m.4 + 1);
                    break;
                }
                j += word.len();
                continue;
            }
            j += 1;
        }
        if let Some((name, name_at, params, body_open, body_close)) = method {
            let receiver = params.first().is_some_and(|p| is_self_receiver(p));
            let handler_name = name_attr.map_or_else(|| name.clone(), |(s, e): (usize, usize)| text[s..e].to_string());
            out.push(Method {
                item_start: start,
                name,
                name_at,
                handler_name,
                name_attr,
                is_handler: receiver && !skip,
                params: params.iter().skip(1).filter_map(|p| p.split_once(':').and_then(|(_, ty)| classify_param(ty))).collect(),
                body_open,
                body_close,
            });
        }
        item_start = None;
        skip = false;
        name_attr = None;
        i = j.max(i + 1);
    }
    out
}

/// `fn name<…>(params) -> R where … { body }` from the `fn` at `at`. `None` for a declaration
/// without a body.
fn parse_fn(text: &str, at: usize, limit: usize) -> Option<ParsedFn> {
    let name_at = skip_trivia(text, at + 2);
    let name = ident_at(text, name_at)?.to_string();
    let mut i = skip_trivia(text, name_at + name.len());
    if text.as_bytes().get(i) == Some(&b'<') {
        let mut depth = 0;
        while i < limit {
            match text.as_bytes()[i] {
                b'<' => depth += 1,
                b'>' => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                _ => {}
            }
            i += 1;
        }
        i = skip_trivia(text, i + 1);
    }
    if text.as_bytes().get(i) != Some(&b'(') {
        return None;
    }
    let params_close = match_bracket(text, i)?;
    let params = split_top_level(&text[i + 1..params_close]).into_iter().map(str::to_string).collect();
    let mut j = params_close + 1;
    while j < limit {
        if let Some(next) = skip_non_code(text, j) {
            j = next;
            continue;
        }
        match text.as_bytes()[j] {
            b';' => return None,
            b'{' => {
                let close = match_bracket(text, j)?;
                return Some((name, name_at, params, j, close));
            }
            _ => j += 1,
        }
    }
    None
}

fn is_self_receiver(param: &str) -> bool {
    let p = param.trim();
    let p = p.strip_prefix('&').map_or(p, str::trim_start);
    let p = p.strip_prefix('\'').map_or(p, |l| l.trim_start_matches(|c: char| c.is_ascii_alphanumeric() || c == '_').trim_start());
    let p = p.strip_prefix("mut").filter(|r| r.starts_with(char::is_whitespace)).map_or(p, str::trim_start);
    p == "self" || p.strip_prefix("self").is_some_and(|r| r.trim_start().starts_with(':'))
}

/// Every occurrence of the identifier `name` in code (word-bounded, outside literals and comments).
pub(crate) fn ident_occurrences(text: &str, name: &str) -> Vec<usize> {
    let b = text.as_bytes();
    find_in_code(text, name)
        .into_iter()
        .filter(|&at| (at == 0 || !ident_byte(b[at - 1])) && !b.get(at + name.len()).is_some_and(|c| ident_byte(*c)))
        .collect()
}

/// The `fn name` items of a file with a body (the legacy stubs `createHandler` writes): the
/// offset of `fn`, of the name, and the body's braces.
pub(crate) fn fns_named(text: &str, name: &str) -> Vec<(usize, usize, usize, usize)> {
    ident_occurrences(text, "fn")
        .into_iter()
        .filter_map(|at| {
            let (n, name_at, _, open, close) = parse_fn(text, at, text.len())?;
            (n == name).then_some((at, name_at, open, close))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn methods_of_a_typed_impl() {
        let text = r#"#[event_handlers]
impl Vm {
    /// Doc.
    fn a(&mut self, sender: &Sender<Button>, e: &MouseEventArgs) { let s = "}"; }

    #[handler(name = "save-file")]
    pub fn save(&mut self, e: &mut dyn EventArgs) {}
    #[handler(skip)]
    fn helper(&self) {}
    fn new() -> Self { Self }
    const X: u32 = 1;
    fn c<'a>(&'a mut self, s: &ElementRef<'_>, e: &ValueChangedEventArgs<String>) where Self: Sized {
        // TODO
    }
    fn d(&mut self, s: &kubuno_views::events::Sender<'_, Switch>) {}
}
"#;
        let imp = find_typed_impl(text).unwrap();
        let methods = impl_methods(text, &imp);
        let names: Vec<_> = methods.iter().map(|m| (m.name.as_str(), m.handler_name.as_str(), m.is_handler)).collect();
        assert_eq!(names, [("a", "a", true), ("save", "save-file", true), ("helper", "helper", false), ("new", "new", false), ("c", "c", true), ("d", "d", true)]);
        assert!(text[methods[0].item_start..].starts_with("/// Doc."));
        assert!(text[methods[1].item_start..].starts_with("#[handler(name"));
        assert_eq!(&text[methods[0].name_at..methods[0].name_at + 1], "a");
        assert_eq!(methods[0].params, [Param::Sender("Button".into()), Param::Args { ty: "MouseEventArgs".into(), generic: None }]);
        assert_eq!(methods[1].params, [Param::AnyArgs]);
        let (s, e) = methods[1].name_attr.unwrap();
        assert_eq!(&text[s..e], "save-file");
        assert_eq!(methods[4].params, [Param::AnySender, Param::Args { ty: "ValueChangedEventArgs".into(), generic: Some("String".into()) }]);
        assert_eq!(methods[5].params, [Param::Sender("Switch".into())]);
        assert_eq!(&text[methods[0].body_close..methods[0].body_close + 1], "}");
    }

    #[test]
    fn fns_and_identifiers_in_code() {
        let text = "fn go(vm: &mut dyn ViewModel, value: Value) {}\nfn go2() {}\nlet x = \"go\"; go(vm, v); // go\n";
        assert_eq!(fns_named(text, "go").len(), 1);
        assert_eq!(ident_occurrences(text, "go").len(), 2, "the declaration and the call, not the string or the comment");
    }

    #[test]
    fn brackets_skip_strings_chars_and_comments() {
        let text = r#"fn f() { let a = "}"; let c = '}'; // }
            /* } */ let r = r"}"; let l: &'static str = "x"; }"#;
        let open = text.find('{').unwrap();
        assert_eq!(match_bracket(text, open), Some(text.len() - 1));
    }

    #[test]
    fn finds_the_typed_impl_in_both_spellings() {
        let text = "use x;\n\n/// Doc.\n#[kubuno_views::event_handlers]\nimpl MainViewModel {\n    fn a(&mut self) { let s = \"}\"; }\n}\n";
        let imp = find_typed_impl(text).expect("impl");
        assert_eq!(imp.self_ty, "MainViewModel");
        assert_eq!(&text[imp.start..imp.start + 2], "#[");
        assert_eq!(imp.close, text.len() - 2);
        let short = "#[event_handlers] impl<T: Clone> Vm<T> where T: Copy { }";
        assert_eq!(find_typed_impl(short).map(|i| i.self_ty), Some("Vm<T>".to_string()));
        assert!(find_typed_impl("// #[event_handlers]\nimpl X {}").is_none());
        assert!(find_typed_impl("#[other_event_handlers] impl X {}").is_none());
    }

    #[test]
    fn finds_the_single_view_model_impl() {
        let text = "impl Default for State { fn default() -> Self { todo!() } }\nimpl ViewModel for State {\n}\nimpl kubuno_views::binding::ViewModel for Other {}\n";
        assert!(find_view_model_impl(text).is_none(), "two view models: ambiguous");
        let one = "impl Default for State {}\nimpl ViewModel for State {\n    fn get(&self) {}\n}\n";
        let imp = find_view_model_impl(one).expect("one");
        assert_eq!(imp.self_ty, "State");
        assert_eq!(&one[imp.close..imp.close + 1], "}");
    }

    #[test]
    fn parses_table_entries_of_every_legacy_shape() {
        let text = r#"fn t() -> HandlerTable {
    handlers! {
        "a" => |vm, _v| {
            vm.set("Status", Value::Str("{},".to_string()));
        },
        "b" => |vm, value| b(vm, value),
        "c" => move |_, on| { let _ = on; }
    }
}"#;
        let table = find_handlers_table(text).expect("table");
        let names: Vec<_> = table.entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["a", "b", "c"]);
        assert_eq!(table.entries[1].body, "b(vm, value)");
        assert_eq!((table.entries[2].vm_pat.as_str(), table.entries[2].value_pat.as_str()), ("_", "on"));
        assert!(table.entries[0].body.starts_with('{') && table.entries[0].body.ends_with('}'));
        assert_eq!(find_handlers_table("handlers! {}").map(|t| t.entries.len()), Some(0));
        assert!(find_handlers_table("handlers! { 42 }").is_none());
    }

    #[test]
    fn prelude_goes_after_the_last_use() {
        let text = "//! Doc.\n\nuse a::b;\nuse c::{\n    d,\n};\n\nstruct S;\n";
        let (at, insert) = prelude_insertion(text, "\n");
        assert_eq!(&text[..at], "//! Doc.\n\nuse a::b;\nuse c::{\n    d,\n};\n");
        assert_eq!(insert, "use kubuno_views::prelude::*;\n");
        let (at, _) = prelude_insertion("//! Doc.\n\nstruct S;\n", "\n");
        assert_eq!(at, "//! Doc.\n\n".len());
    }

    #[test]
    fn finds_legacy_frame_calls_only() {
        let text = "let e = runtime.frame(canvas, frame, &mut vm, &mut handlers, body);\nlet f = other.frame(a);\n";
        let calls = legacy_frame_calls(text);
        assert_eq!(calls.len(), 1);
        assert!(text[calls[0]..].starts_with("frame(canvas"));
    }

    #[test]
    fn method_idents() {
        assert!(is_method_ident("say_hello_clicked"));
        assert!(!is_method_ident("save-file") && !is_method_ident("fn") && !is_method_ident("1a") && !is_method_ident(""));
    }
}
