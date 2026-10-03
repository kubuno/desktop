//! Clipboard payloads, platform-neutral: what is put on the clipboard and how it is read back.
//! The platform moves the bytes (Windows: `documents/src/platform/clipboard.rs`).
//!
//! Out, three formats: our own private format (the selected blocks as the stored JSON, so a copy
//! and paste inside the app is the file format's own lossless round trip), HTML (so other
//! applications get the structure and the character formatting — wrapped in the `CF_HTML`
//! envelope on Windows, whose header counts **bytes** of the final UTF-8 string), and plain text.
//!
//! In: the private format, then HTML ([`html_to_blocks`]: the structure and character
//! formatting the web editor itself writes — paragraphs, headings, lists, tables, `<b>/<i>/<u>/<s>`,
//! sub/superscript, links, `style="font-family/font-size/color/background-color/text-align"`,
//! images — and nothing else), then plain text.
//!
//! The HTML fragment starts with `data-pm-slice="0 0 []"`, byte-for-byte what ProseMirror writes
//! for a whole-block slice, so pasting into the web editor goes through ProseMirror's own parser.
//!
//! (The writer, the private format and the encodings come from the desktop's first clipboard
//! module, `documents/src/edit/clipboard.rs`; the reader and the selection slicing are new.)

use serde::{Deserialize, Serialize};
use serde_json::{Map as JsonMap, Value};

use crate::model::Node;
use crate::pm::node_at;

use super::inline;

/// The private clipboard format name.
pub const PRIVATE_FORMAT: &str = "Kubuno.Documents.Blocks";

/// The `CF_HTML` format name (Windows; registered, not predefined).
pub const HTML_FORMAT: &str = "HTML Format";

/// The version carried inside the private payload: a build never mis-reads a newer slice.
const PRIVATE_VERSION: u32 = 1;

/// The largest private payload parsed: another process can put anything on the clipboard.
pub const MAX_PAYLOAD: usize = 32 * 1024 * 1024;

#[derive(Serialize)]
struct SliceOut<'a> {
    v:       u32,
    content: &'a [Node],
}

#[derive(Deserialize)]
struct SliceIn {
    #[serde(default)]
    v:       u32,
    #[serde(default)]
    content: Vec<Node>,
}

pub fn encode_private(blocks: &[Node]) -> Result<Vec<u8>, String> {
    let slice = SliceOut { v: PRIVATE_VERSION, content: blocks };
    serde_json::to_vec(&slice).map_err(|e| format!("clipboard: cannot encode the selection: {e}"))
}

pub fn decode_private(bytes: &[u8]) -> Option<Vec<Node>> {
    if bytes.len() > MAX_PAYLOAD {
        eprintln!("[clipboard] refusing a {} byte payload", bytes.len());
        return None;
    }
    // `GlobalSize` rounds up, so the bytes we get back are usually longer than
    // the bytes that were written; the tail is NULs and serde_json rejects it.
    let bytes = trim_trailing_nuls(bytes);
    let slice: SliceIn = match serde_json::from_slice(bytes) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[clipboard] malformed private payload: {e}");
            return None;
        }
    };
    if slice.v != PRIVATE_VERSION {
        eprintln!("[clipboard] ignoring a v{} payload; this build reads v{PRIVATE_VERSION}", slice.v);
        return None;
    }
    Some(slice.content)
}

fn trim_trailing_nuls(bytes: &[u8]) -> &[u8] {
    let mut end = bytes.len();
    while end > 0 && bytes[end - 1] == 0 {
        end -= 1;
    }
    &bytes[..end]
}

// ---------------------------------------------------------------------------
// CF_UNICODETEXT
// ---------------------------------------------------------------------------

/// UTF-16 little-endian with a trailing NUL, as bytes.
///
/// Bytes rather than `u16`s so the Win32 layer never has to reinterpret a
/// pointer, and so the encoding itself is testable.
pub fn utf16_le_nul(s: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len() * 2 + 2);
    for unit in s.encode_utf16() {
        out.extend_from_slice(&unit.to_le_bytes());
    }
    out.extend_from_slice(&[0, 0]);
    out
}

/// The inverse. Stops at the first NUL, drops a trailing odd byte rather than
/// failing, and never panics on an unpaired surrogate.
pub fn utf16_le_to_string(bytes: &[u8]) -> String {
    let mut units = Vec::with_capacity(bytes.len() / 2);
    // `as_chunks` drops a trailing odd byte, which is exactly what we want: a
    // half unit at the end of someone else's buffer is not a reason to fail.
    for pair in bytes.as_chunks::<2>().0 {
        let unit = u16::from_le_bytes(*pair);
        if unit == 0 {
            break;
        }
        units.push(unit);
    }
    String::from_utf16_lossy(&units)
}

/// LF and lone CR become CRLF; an existing CRLF is left alone rather than
/// doubled.
pub fn to_crlf(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + s.len() / 16);
    let mut chars = s.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '\r' => {
                // Consume the LF of an existing CRLF so it is not re-expanded.
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                out.push_str("\r\n");
            }
            '\n' => out.push_str("\r\n"),
            _ => out.push(ch),
        }
    }
    out
}

/// The inverse, so the rest of the app only ever sees `\n`.
pub fn to_lf(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\r' {
            if chars.peek() == Some(&'\n') {
                chars.next();
            }
            out.push('\n');
        } else {
            out.push(ch);
        }
    }
    out
}

// ---------------------------------------------------------------------------
// CF_HTML
// ---------------------------------------------------------------------------

const CF_HTML_PREFIX: &str = "<html>\r\n<body>\r\n<!--StartFragment-->";
const CF_HTML_SUFFIX: &str = "<!--EndFragment-->\r\n</body>\r\n</html>";

/// The CF_HTML header is a fixed size because the four offsets are zero-padded
/// to ten digits. This is the well-known 105, computed rather than asserted.
pub const CF_HTML_HEADER_LEN: usize = "Version:0.9\r\n".len()
    + "StartHTML:0000000000\r\n".len()
    + "EndHTML:0000000000\r\n".len()
    + "StartFragment:0000000000\r\n".len()
    + "EndFragment:0000000000\r\n".len();

/// Wraps an HTML fragment in the CF_HTML envelope.
///
/// Every offset is a byte count into the string this function returns, so the
/// body is measured before the header is written and the header's own length is
/// a constant rather than something measured after the fact.
pub fn cf_html_wrap(fragment: &str) -> String {
    let start_html = CF_HTML_HEADER_LEN;
    let start_fragment = start_html + CF_HTML_PREFIX.len();
    let end_fragment = start_fragment + fragment.len();
    let end_html = end_fragment + CF_HTML_SUFFIX.len();

    let mut out = String::with_capacity(end_html);
    out.push_str("Version:0.9\r\n");
    out.push_str(&format!("StartHTML:{start_html:010}\r\n"));
    out.push_str(&format!("EndHTML:{end_html:010}\r\n"));
    out.push_str(&format!("StartFragment:{start_fragment:010}\r\n"));
    out.push_str(&format!("EndFragment:{end_fragment:010}\r\n"));
    out.push_str(CF_HTML_PREFIX);
    out.push_str(fragment);
    out.push_str(CF_HTML_SUFFIX);
    out
}

// ---------------------------------------------------------------------------
// Blocks to an HTML fragment
// ---------------------------------------------------------------------------

/// Serialises whole blocks to an HTML fragment.
pub fn blocks_to_html(blocks: &[Node]) -> String {
    let mut out = String::new();
    // ProseMirror puts the slice descriptor on the first element of the
    // fragment; `pending` guarantees it lands on the first element actually
    // emitted, whatever the first block turns out to be.
    let mut pending = Some(" data-pm-slice=\"0 0 []\"".to_string());
    for block in blocks {
        node_html(block, &mut pending, &mut out);
    }
    out
}

/// Opens an element, consuming the pending first-element attribute if any.
fn open_tag(out: &mut String, name: &str, attrs: &str, pending: &mut Option<String>) {
    out.push('<');
    out.push_str(name);
    out.push_str(attrs);
    if let Some(extra) = pending.take() {
        out.push_str(&extra);
    }
    out.push('>');
}

fn close_tag(out: &mut String, name: &str) {
    out.push_str("</");
    out.push_str(name);
    out.push('>');
}

/// Wraps the node's children in one element.
fn wrap(node: &Node, name: &str, attrs: &str, pending: &mut Option<String>, out: &mut String) {
    open_tag(out, name, attrs, pending);
    for child in node.children() {
        node_html(child, pending, out);
    }
    close_tag(out, name);
}

fn node_html(node: &Node, pending: &mut Option<String>, out: &mut String) {
    let kind = node.node_type().unwrap_or_default();
    let attrs = attrs_of(node);
    match kind {
        "text" => text_html(node, out),
        "hardBreak" => {
            open_tag(out, "br", "", pending);
        }
        "horizontalRule" => {
            open_tag(out, "hr", "", pending);
        }
        // Not a document-format fact, a writer's choice: the CSS property is the
        // only thing a foreign consumer understands by "page break here".
        "pageBreak" => {
            open_tag(out, "br", " style=\"page-break-before:always\"", pending);
        }
        "paragraph" => wrap(node, "p", &block_style(&attrs), pending, out),
        "heading" => {
            let level = attr_i64(&attrs, "level").unwrap_or(1).clamp(1, 6);
            wrap(node, &format!("h{level}"), &block_style(&attrs), pending, out);
        }
        "blockquote" => wrap(node, "blockquote", "", pending, out),
        "codeBlock" => {
            open_tag(out, "pre", "", pending);
            out.push_str("<code>");
            for child in node.children() {
                node_html(child, pending, out);
            }
            out.push_str("</code></pre>");
        }
        "bulletList" | "taskList" => wrap(node, "ul", "", pending, out),
        "orderedList" => {
            let start = attr_i64(&attrs, "start").unwrap_or(1);
            let a = if start == 1 { String::new() } else { format!(" start=\"{start}\"") };
            wrap(node, "ol", &a, pending, out);
        }
        "listItem" | "taskItem" => wrap(node, "li", "", pending, out),
        "table" => wrap(node, "table", "", pending, out),
        "tableRow" => wrap(node, "tr", "", pending, out),
        "tableCell" => wrap(node, "td", &cell_attrs(&attrs), pending, out),
        "tableHeader" => wrap(node, "th", &cell_attrs(&attrs), pending, out),
        "image" | "inlineImage" => image_html(&attrs, pending, out),
        // A field's `cached` value is the text Word last computed for it; it is
        // the only sensible thing to hand another application.
        "field" => {
            if let Some(cached) = attr_str(&attrs, "cached") {
                escape_text(&cached, out);
            }
        }
        // Anything we do not model: keep the prose, invent no element. Wrapping
        // an unknown inline node in a block element is how a fragment ends up
        // structurally wrong in the consumer.
        _ => {
            for child in node.children() {
                node_html(child, pending, out);
            }
        }
    }
}

fn text_html(node: &Node, out: &mut String) {
    let Some(text) = node.text() else { return };
    let mut closers: Vec<&'static str> = Vec::new();
    for mark in marks_of(node) {
        if let Some((open, close)) = mark_tags(&mark) {
            out.push_str(&open);
            closers.push(close);
        }
    }
    escape_text(&text, out);
    for close in closers.iter().rev() {
        out.push_str(close);
    }
}

fn image_html(attrs: &JsonMap<String, Value>, pending: &mut Option<String>, out: &mut String) {
    let Some(src) = attr_str(attrs, "src") else { return };
    let mut a = format!(" src=\"{}\"", escape_attr(&src));
    if let Some(alt) = attr_str(attrs, "alt") {
        a.push_str(&format!(" alt=\"{}\"", escape_attr(&alt)));
    }
    for key in ["width", "height"] {
        if let Some(v) = attr_f64(attrs, key) {
            if v > 0.0 {
                a.push_str(&format!(" {key}=\"{}\"", trim_number(v)));
            }
        }
    }
    open_tag(out, "img", &a, pending);
}

fn cell_attrs(attrs: &JsonMap<String, Value>) -> String {
    let mut a = String::new();
    for key in ["colspan", "rowspan"] {
        if let Some(n) = attr_i64(attrs, key) {
            if n > 1 {
                a.push_str(&format!(" {key}=\"{n}\""));
            }
        }
    }
    a
}

/// The only paragraph-level property a foreign consumer reliably honours.
fn block_style(attrs: &JsonMap<String, Value>) -> String {
    match attr_str(attrs, "textAlign") {
        Some(align) if matches!(align.as_str(), "left" | "center" | "right" | "justify") => {
            format!(" style=\"text-align:{align}\"")
        }
        _ => String::new(),
    }
}

// ---------------------------------------------------------------------------
// Marks
// ---------------------------------------------------------------------------

struct Mark {
    kind:  String,
    attrs: JsonMap<String, Value>,
}

fn marks_of(node: &Node) -> Vec<Mark> {
    let Some(raw) = node.raw("marks") else { return Vec::new() };
    let Ok(values) = serde_json::from_str::<Vec<Value>>(raw.get()) else { return Vec::new() };
    values
        .into_iter()
        .filter_map(|v| {
            let obj = v.as_object()?;
            let kind = obj.get("type")?.as_str()?.to_string();
            let attrs = obj
                .get("attrs")
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            Some(Mark { kind, attrs })
        })
        .collect()
}

fn mark_tags(mark: &Mark) -> Option<(String, &'static str)> {
    match mark.kind.as_str() {
        "bold" => Some(("<strong>".into(), "</strong>")),
        "italic" => Some(("<em>".into(), "</em>")),
        "underline" => Some(("<u>".into(), "</u>")),
        "strike" => Some(("<s>".into(), "</s>")),
        "code" => Some(("<code>".into(), "</code>")),
        "superscript" => Some(("<sup>".into(), "</sup>")),
        "subscript" => Some(("<sub>".into(), "</sub>")),
        // `href` is nullable in the schema. An anchor with no destination is
        // worse than no anchor: it is a live-looking dead link in the consumer.
        "link" => {
            let href = attr_str(&mark.attrs, "href")?;
            Some((format!("<a href=\"{}\">", escape_attr(&href)), "</a>"))
        }
        "highlight" => {
            let color = attr_str(&mark.attrs, "color")?;
            Some((
                format!("<span style=\"background-color:{}\">", escape_attr(&color)),
                "</span>",
            ))
        }
        "textStyle" => {
            let css = text_style_css(&mark.attrs);
            if css.is_empty() {
                None
            } else {
                Some((format!("<span style=\"{}\">", escape_attr(&css)), "</span>"))
            }
        }
        _ => None,
    }
}

fn text_style_css(attrs: &JsonMap<String, Value>) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some(family) = attr_str(attrs, "fontFamily") {
        parts.push(format!("font-family:{family}"));
    }
    if let Some(size) = attrs.get("fontSize").and_then(css_font_size) {
        parts.push(format!("font-size:{size}"));
    }
    if let Some(color) = attr_str(attrs, "color") {
        parts.push(format!("color:{color}"));
    }
    parts.join(";")
}

/// `fontSize` is a CSS string in the editor (`"14pt"`) but a bare number in
/// documents the importer wrote. The format keeps character sizes in points, so
/// a bare number is points.
fn css_font_size(value: &Value) -> Option<String> {
    if let Some(s) = value.as_str() {
        let s = s.trim();
        return if s.is_empty() { None } else { Some(s.to_string()) };
    }
    let n = value.as_f64()?;
    if n.is_finite() && n > 0.0 {
        Some(format!("{}pt", trim_number(n)))
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Small helpers
// ---------------------------------------------------------------------------

/// Reads a node's `attrs` object. Read-only: nothing here is ever written back,
/// so a value we do not understand cannot be reformatted by being looked at.
fn attrs_of(node: &Node) -> JsonMap<String, Value> {
    node.raw("attrs")
        .and_then(|raw| serde_json::from_str::<Value>(raw.get()).ok())
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default()
}

fn attr_str(attrs: &JsonMap<String, Value>, key: &str) -> Option<String> {
    let s = attrs.get(key)?.as_str()?;
    if s.is_empty() {
        None
    } else {
        Some(s.to_string())
    }
}

fn attr_i64(attrs: &JsonMap<String, Value>, key: &str) -> Option<i64> {
    let v = attrs.get(key)?;
    v.as_i64().or_else(|| v.as_f64().map(|f| f as i64))
}

fn attr_f64(attrs: &JsonMap<String, Value>, key: &str) -> Option<f64> {
    attrs.get(key)?.as_f64()
}

/// Formats a number the way a CSS or HTML attribute wants it: `2` not `2.0`.
fn trim_number(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else {
        format!("{n}")
    }
}

fn escape_text(s: &str, out: &mut String) {
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(ch),
        }
    }
}

fn escape_attr(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
    out
}


// ---------------------------------------------------------------------------
// Reading
// ---------------------------------------------------------------------------

/// The fragment of a `CF_HTML` payload: by its `StartFragment`/`EndFragment` byte offsets, else
/// between the `<!--StartFragment-->` markers, else the whole text.
pub fn cf_html_fragment(cf: &str) -> &str {
    let offset = |key: &str| -> Option<usize> {
        let at = cf.find(key)? + key.len();
        let digits: String = cf[at..].chars().take_while(|c| c.is_ascii_digit()).collect();
        digits.parse().ok()
    };
    if let (Some(a), Some(b)) = (offset("StartFragment:"), offset("EndFragment:")) {
        if a <= b && b <= cf.len() && cf.is_char_boundary(a) && cf.is_char_boundary(b) {
            return &cf[a..b];
        }
    }
    if let (Some(a), Some(b)) = (cf.find("<!--StartFragment-->"), cf.find("<!--EndFragment-->")) {
        let a = a + "<!--StartFragment-->".len();
        if a <= b {
            return &cf[a..b];
        }
    }
    cf
}

/// Blocks read from HTML (see [`super::html`]).
pub fn html_to_blocks(html: &str) -> Vec<Node> {
    super::html::html_to_blocks(html)
}

// ---------------------------------------------------------------------------
// The selection as a slice
// ---------------------------------------------------------------------------

/// The selection `from..to` as blocks: one textblock with the selected inline content when it is
/// inside one block; otherwise the blocks it covers, the first and last cut at the selection's
/// ends, their containers (lists, quotes) kept — what ProseMirror's `slice` gives, closed.
pub fn slice(doc: &Node, from: usize, to: usize) -> Vec<Node> {
    use super::structure::with_children;
    use crate::pm::textblock_at;
    let (from, to) = (from.min(to), from.max(to));
    let (Some((pa, sa)), Some((pb, sb))) = (textblock_at(doc, from), textblock_at(doc, to)) else { return Vec::new() };
    let Some(a) = node_at(doc, &pa) else { return Vec::new() };
    if pa == pb {
        let c = match inline::cut(a.children(), from - sa, to - sa) {
            Ok(c) => c,
            Err(_) => return Vec::new(),
        };
        return vec![with_children(a, c.removed)];
    }
    let c = pa.iter().zip(pb.iter()).take_while(|(x, y)| x == y).count();
    let Some(cnode) = node_at(doc, &pa[..c]) else { return Vec::new() };
    // Inside a table across cells, copy the whole table rows' cells as they are.
    let (i, j) = (pa[c], pb[c]);
    let a_tail = match inline::cut(a.children(), from - sa, inline::content_len(a.children())) {
        Ok(cut) => with_children(a, cut.removed),
        Err(_) => return Vec::new(),
    };
    let Some(b) = node_at(doc, &pb) else { return Vec::new() };
    let b_head = match inline::cut(b.children(), 0, to - sb) {
        Ok(cut) => with_children(b, cut.removed),
        Err(_) => return Vec::new(),
    };
    fn tail_of(node: &Node, path: &[usize], leaf: &Node) -> Node {
        match path.split_first() {
            None => leaf.clone(),
            Some((&k, rest)) => {
                let kids = node.children();
                let mut out = Vec::new();
                if let Some(child) = kids.get(k) {
                    out.push(tail_of(child, rest, leaf));
                }
                out.extend(kids.iter().skip(k + 1).cloned());
                super::structure::with_children(node, out)
            }
        }
    }
    fn head_of(node: &Node, path: &[usize], leaf: &Node) -> Node {
        match path.split_first() {
            None => leaf.clone(),
            Some((&k, rest)) => {
                let kids = node.children();
                let mut out: Vec<Node> = kids[..k.min(kids.len())].to_vec();
                if let Some(child) = kids.get(k) {
                    out.push(head_of(child, rest, leaf));
                }
                super::structure::with_children(node, out)
            }
        }
    }
    let kids = cnode.children();
    let mut out = Vec::new();
    if let Some(l) = kids.get(i) {
        out.push(tail_of(l, &pa[c + 1..], &a_tail));
    }
    for k in kids.iter().take(j).skip(i + 1) {
        out.push(k.clone());
    }
    if let Some(r) = kids.get(j) {
        out.push(head_of(r, &pb[c + 1..], &b_head));
    }
    // A slice taken inside a table row (cells) or a list (items) is wrapped in its container so it
    // stays a valid block sequence.
    match cnode.node_type() {
        Some("tableRow") | Some("table") | Some("bulletList") | Some("orderedList") | Some("taskList") if !pa[..c].is_empty() => {
            vec![with_children(cnode, out)]
        }
        _ => out,
    }
}

/// The selection as plain text: textblocks separated by `\n`, hard breaks as `\n`, atoms dropped
/// (a field gives its cached text).
pub fn plain_text(doc: &Node, from: usize, to: usize) -> String {
    crate::pm::text_between(doc, from.min(to), from.max(to), "\n", &|n| match n.node_type() {
        Some("hardBreak") => "\n".into(),
        Some("field") => crate::layout::parse::field_text(n),
        _ => String::new(),
    })
}

/// What was read off the clipboard, richest first.
#[derive(Debug, Clone)]
pub enum Pasted {
    /// Our own format, or parsed HTML: blocks.
    Blocks(Vec<Node>),
    /// Plain text.
    Text(String),
}

/// Chooses what to paste from the formats the platform found (`private`, `html` — a `CF_HTML`
/// payload or a bare fragment — and `text`). `plain` forces plain text (Ctrl+Shift+V).
pub fn choose(private: Option<&[u8]>, html: Option<&str>, text: Option<&str>, plain: bool) -> Option<Pasted> {
    if !plain {
        if let Some(blocks) = private.and_then(decode_private).filter(|b| !b.is_empty()) {
            return Some(Pasted::Blocks(blocks));
        }
        if let Some(h) = html {
            let blocks = html_to_blocks(cf_html_fragment(h));
            if !blocks.is_empty() {
                return Some(Pasted::Blocks(blocks));
            }
        }
    }
    text.map(to_lf).filter(|t| !t.is_empty()).map(Pasted::Text)
}

/// The three payloads of a copy: private bytes, the `CF_HTML` string and the plain text (CRLF).
pub fn payloads(blocks: &[Node], plain: &str) -> Result<(Vec<u8>, String, String), String> {
    let private = encode_private(blocks)?;
    Ok((private, cf_html_wrap(&blocks_to_html(blocks)), to_crlf(plain)))
}
#[cfg(test)]
mod tests {
    use super::*;

    fn node(json: &str) -> Node {
        Node::from_slice(json.as_bytes()).expect("fixture must parse")
    }

    fn fragment(json: &str) -> String {
        blocks_to_html(&[node(json)])
    }

    // -- CF_HTML header arithmetic ------------------------------------------

    /// Reads the four offsets back out of a CF_HTML string.
    fn offsets(s: &str) -> (usize, usize, usize, usize) {
        let field = |name: &str| -> usize {
            let at = s.find(&format!("{name}:")).expect("field present") + name.len() + 1;
            s[at..at + 10].parse().expect("ten digits")
        };
        (field("StartHTML"), field("EndHTML"), field("StartFragment"), field("EndFragment"))
    }

    #[test]
    fn the_cf_html_header_is_a_fixed_105_bytes() {
        // The four numbers are zero-padded to ten digits precisely so that
        // writing the header does not move the body it points at.
        assert_eq!(CF_HTML_HEADER_LEN, 105);
        let wrapped = cf_html_wrap("<p>x</p>");
        let (start_html, ..) = offsets(&wrapped);
        assert_eq!(start_html, 105);
        assert!(wrapped[..105].ends_with("\r\n"));
    }

    #[test]
    fn the_cf_html_offsets_point_at_the_fragment_itself() {
        let wrapped = cf_html_wrap("<p>hello</p>");
        let (start_html, end_html, start_fragment, end_fragment) = offsets(&wrapped);
        assert_eq!(&wrapped[start_fragment..end_fragment], "<p>hello</p>");
        assert!(wrapped[start_html..end_html].starts_with("<html>"));
        assert!(wrapped[start_html..end_html].ends_with("</html>"));
        assert_eq!(end_html, wrapped.len());
    }

    #[test]
    fn the_cf_html_offsets_are_bytes_not_characters() {
        // The classic failure: a fragment with an accent in it makes a
        // char-counted StartFragment land mid-character.
        let frag = "<p>é — 🙂 ünïcodé</p>";
        assert_ne!(frag.len(), frag.chars().count());
        let wrapped = cf_html_wrap(frag);
        let (_, end_html, start_fragment, end_fragment) = offsets(&wrapped);
        assert_eq!(&wrapped[start_fragment..end_fragment], frag);
        assert_eq!(end_html, wrapped.len());
    }

    #[test]
    fn the_fragment_sits_between_the_start_and_end_markers() {
        let wrapped = cf_html_wrap("<p>x</p>");
        let (_, _, start_fragment, end_fragment) = offsets(&wrapped);
        assert!(wrapped[..start_fragment].ends_with("<!--StartFragment-->"));
        assert!(wrapped[end_fragment..].starts_with("<!--EndFragment-->"));
    }

    #[test]
    fn an_empty_fragment_still_produces_consistent_offsets() {
        let wrapped = cf_html_wrap("");
        let (_, end_html, start_fragment, end_fragment) = offsets(&wrapped);
        assert_eq!(start_fragment, end_fragment);
        assert_eq!(end_html, wrapped.len());
    }

    // -- The private format -------------------------------------------------

    #[test]
    fn the_private_format_round_trips_attributes_it_does_not_model() {
        // The whole point of the private format: an internal copy and paste is
        // the file format's own round trip, so a mark added next year survives.
        let src = r#"{"attrs":{"indent":47.999999999999996,"somethingNew":{"a":[1,null]}},"content":[{"marks":[{"attrs":{"author":"L","id":"rev-7"},"type":"insertion"}],"text":"hi","type":"text"}],"type":"paragraph"}"#;
        let blocks = vec![node(src)];
        let bytes = encode_private(&blocks).expect("encodes");
        let back = decode_private(&bytes).expect("decodes");
        assert_eq!(back.len(), 1);
        let out = String::from_utf8(back[0].to_vec().expect("serialises")).expect("utf-8");
        assert_eq!(out, src);
    }

    #[test]
    fn a_payload_from_a_newer_version_is_refused_rather_than_mis_read() {
        let bytes = br#"{"v":2,"content":[{"type":"paragraph"}]}"#;
        assert!(decode_private(bytes).is_none());
    }

    #[test]
    fn a_payload_with_no_version_is_refused() {
        // Somebody else's data under our registered name, or a truncated write.
        let bytes = br#"{"content":[{"type":"paragraph"}]}"#;
        assert!(decode_private(bytes).is_none());
    }

    #[test]
    fn an_oversized_payload_is_refused_before_it_is_parsed() {
        // Untrusted input from another process must not become an allocation.
        let mut bytes = vec![b' '; MAX_PAYLOAD + 1];
        bytes[0] = b'{';
        assert!(decode_private(&bytes).is_none());
    }

    #[test]
    fn the_trailing_nuls_globalsize_hands_back_do_not_break_the_parse() {
        // GlobalSize reports the allocated size, which the allocator rounds up;
        // the tail is NULs and serde_json rejects them as trailing characters.
        let blocks = vec![node(r#"{"type":"paragraph"}"#)];
        let mut bytes = encode_private(&blocks).expect("encodes");
        bytes.extend_from_slice(&[0u8; 7]);
        assert_eq!(decode_private(&bytes).expect("decodes").len(), 1);
    }

    #[test]
    fn malformed_json_is_refused_without_panicking() {
        assert!(decode_private(b"{\"v\":1,\"content\":").is_none());
        assert!(decode_private(&[0xff, 0xfe, 0x00]).is_none());
        assert!(decode_private(b"").is_none());
    }

    // -- CF_UNICODETEXT -----------------------------------------------------

    #[test]
    fn utf16_round_trips_through_the_byte_encoding() {
        for s in ["", "plain", "é — 🙂", "line\r\nline"] {
            let bytes = utf16_le_nul(s);
            assert_eq!(utf16_le_to_string(&bytes), s);
        }
    }

    #[test]
    fn text_from_the_clipboard_stops_at_the_first_nul() {
        // GlobalSize over-reports, so the buffer has junk after the terminator.
        let mut bytes = utf16_le_nul("hello");
        bytes.extend_from_slice(&utf16_le_nul("garbage"));
        assert_eq!(utf16_le_to_string(&bytes), "hello");
    }

    #[test]
    fn an_odd_trailing_byte_is_dropped_rather_than_panicking() {
        let mut bytes = utf16_le_nul("ok");
        bytes.pop();
        assert_eq!(utf16_le_to_string(&bytes), "ok");
    }

    #[test]
    fn an_unpaired_surrogate_decodes_lossily_instead_of_failing() {
        // 0xD800 with no low surrogate: possible on a real clipboard.
        let bytes = vec![0x00u8, 0xD8, 0x00, 0x00];
        assert!(!utf16_le_to_string(&bytes).is_empty());
    }

    #[test]
    fn line_endings_become_crlf_on_copy_without_doubling_existing_ones() {
        assert_eq!(to_crlf("a\nb"), "a\r\nb");
        assert_eq!(to_crlf("a\r\nb"), "a\r\nb");
        assert_eq!(to_crlf("a\rb"), "a\r\nb");
        assert_eq!(to_crlf("a\r\n\nb"), "a\r\n\r\nb");
    }

    #[test]
    fn line_endings_become_lf_on_paste_so_the_rest_of_the_app_sees_one_shape() {
        assert_eq!(to_lf("a\r\nb"), "a\nb");
        assert_eq!(to_lf("a\rb"), "a\nb");
        assert_eq!(to_lf("a\nb"), "a\nb");
        assert_eq!(to_lf(&to_crlf("a\nb\nc")), "a\nb\nc");
    }

    // -- The HTML fragment --------------------------------------------------

    #[test]
    fn text_and_attribute_values_are_escaped() {
        let html = fragment(r#"{"content":[{"text":"a < b & \"c\"","type":"text"}],"type":"paragraph"}"#);
        assert!(html.contains("a &lt; b &amp; \"c\""), "{html}");

        let link = fragment(
            r#"{"content":[{"marks":[{"attrs":{"href":"https://x/?a=1&b=\"2\""},"type":"link"}],"text":"t","type":"text"}],"type":"paragraph"}"#,
        );
        assert!(link.contains("href=\"https://x/?a=1&amp;b=&quot;2&quot;\""), "{link}");
    }

    #[test]
    fn marks_close_in_the_reverse_of_the_order_they_opened() {
        // Overlapping tags are the one way to produce HTML a consumer silently
        // reinterprets.
        let html = fragment(
            r#"{"content":[{"marks":[{"type":"bold"},{"type":"italic"},{"type":"underline"}],"text":"x","type":"text"}],"type":"paragraph"}"#,
        );
        assert!(html.contains("<strong><em><u>x</u></em></strong>"), "{html}");
    }

    #[test]
    fn a_link_with_no_href_does_not_become_a_dead_anchor() {
        // `href` is nullable in the schema.
        let html = fragment(
            r#"{"content":[{"marks":[{"attrs":{"href":null},"type":"link"}],"text":"x","type":"text"}],"type":"paragraph"}"#,
        );
        assert!(!html.contains("<a"), "{html}");
        assert!(html.contains('x'), "{html}");
    }

    #[test]
    fn a_font_size_is_points_whether_it_was_stored_as_a_string_or_a_number() {
        let as_string = fragment(
            r#"{"content":[{"marks":[{"attrs":{"fontSize":"13pt"},"type":"textStyle"}],"text":"x","type":"text"}],"type":"paragraph"}"#,
        );
        assert!(as_string.contains("font-size:13pt"), "{as_string}");

        for src in ["11.0", "18"] {
            let html = fragment(&format!(
                r#"{{"content":[{{"marks":[{{"attrs":{{"fontSize":{src}}},"type":"textStyle"}}],"text":"x","type":"text"}}],"type":"paragraph"}}"#
            ));
            let expect = if src == "11.0" { "font-size:11pt" } else { "font-size:18pt" };
            assert!(html.contains(expect), "{src} -> {html}");
        }
    }

    #[test]
    fn an_empty_text_style_does_not_emit_an_empty_span() {
        let html = fragment(
            r#"{"content":[{"marks":[{"attrs":{"color":null,"fontFamily":null,"fontSize":null},"type":"textStyle"}],"text":"x","type":"text"}],"type":"paragraph"}"#,
        );
        assert_eq!(html, "<p data-pm-slice=\"0 0 []\">x</p>");
    }

    #[test]
    fn the_first_element_carries_the_slice_descriptor_exactly_once() {
        // ProseMirror reads it with querySelector("[data-pm-slice]"); a second
        // copy on a later block would be read instead of the first.
        let html = blocks_to_html(&[
            node(r#"{"content":[{"text":"a","type":"text"}],"type":"paragraph"}"#),
            node(r#"{"content":[{"text":"b","type":"text"}],"type":"paragraph"}"#),
        ]);
        assert_eq!(html.matches("data-pm-slice").count(), 1, "{html}");
        assert!(html.starts_with("<p data-pm-slice=\"0 0 []\">"), "{html}");
    }

    #[test]
    fn the_slice_descriptor_lands_on_the_first_element_even_after_an_unknown_node() {
        // An unmodelled node emits no element of its own, so the descriptor
        // must fall through to whatever element comes next.
        let html = blocks_to_html(&[
            node(r#"{"type":"sectionBreak"}"#),
            node(r#"{"content":[{"text":"a","type":"text"}],"type":"paragraph"}"#),
        ]);
        assert_eq!(html, "<p data-pm-slice=\"0 0 []\">a</p>");
    }

    #[test]
    fn an_unknown_node_type_keeps_its_prose_and_invents_no_element() {
        let html = fragment(
            r#"{"content":[{"content":[{"text":"kept","type":"text"}],"type":"fromNextYear"}],"type":"paragraph"}"#,
        );
        assert!(html.contains("kept"), "{html}");
        assert!(!html.contains("fromNextYear"), "{html}");
        assert!(!html.contains("<div"), "{html}");
    }

    #[test]
    fn a_heading_level_outside_one_to_six_is_clamped() {
        for (level, tag) in [("0", "h1"), ("3", "h3"), ("9", "h6")] {
            let html = fragment(&format!(
                r#"{{"attrs":{{"level":{level}}},"content":[{{"text":"t","type":"text"}}],"type":"heading"}}"#
            ));
            assert!(html.contains(&format!("<{tag} ")) || html.contains(&format!("<{tag}>")), "{html}");
            assert!(html.ends_with(&format!("</{tag}>")), "{html}");
        }
    }

    #[test]
    fn text_alignment_is_carried_and_a_nonsense_value_is_not() {
        let good = fragment(r#"{"attrs":{"textAlign":"center"},"type":"paragraph"}"#);
        assert!(good.contains("text-align:center"), "{good}");
        let bad = fragment(r#"{"attrs":{"textAlign":"javascript:x"},"type":"paragraph"}"#);
        assert!(!bad.contains("text-align"), "{bad}");
    }

    #[test]
    fn a_list_starting_at_one_does_not_emit_a_redundant_start_attribute() {
        let one = fragment(r#"{"attrs":{"start":1},"content":[{"type":"listItem"}],"type":"orderedList"}"#);
        assert!(!one.contains("start="), "{one}");
        let five = fragment(r#"{"attrs":{"start":5},"content":[{"type":"listItem"}],"type":"orderedList"}"#);
        assert!(five.contains("start=\"5\""), "{five}");
    }

    #[test]
    fn a_table_cell_only_declares_a_span_it_actually_has() {
        let plain = fragment(r#"{"attrs":{"colspan":1,"rowspan":1},"type":"tableCell"}"#);
        assert_eq!(plain, "<td data-pm-slice=\"0 0 []\"></td>");
        let spanned = fragment(r#"{"attrs":{"colspan":3,"rowspan":2},"type":"tableCell"}"#);
        assert!(spanned.contains("colspan=\"3\""), "{spanned}");
        assert!(spanned.contains("rowspan=\"2\""), "{spanned}");
    }

    #[test]
    fn an_image_without_a_source_emits_nothing_rather_than_a_broken_tag() {
        let html = fragment(r#"{"attrs":{"alt":"a","src":null},"type":"image"}"#);
        assert_eq!(html, "");
    }

    #[test]
    fn an_image_dimension_is_a_whole_number_not_a_float_literal() {
        let html = fragment(r#"{"attrs":{"height":120.0,"src":"data:image/png,x","width":240.0},"type":"image"}"#);
        assert!(html.contains("width=\"240\""), "{html}");
        assert!(html.contains("height=\"120\""), "{html}");
    }

    #[test]
    fn a_rich_text_box_payload_hidden_in_alt_survives_into_the_fragment() {
        // The web stores a whole sub-document there; re-encoding it loses prose.
        let html = fragment(
            r#"{"attrs":{"alt":"kbtextrich:%7B%22type%22%3A%22doc%22%7D","src":"data:image/svg+xml,x"},"type":"image"}"#,
        );
        assert!(html.contains("alt=\"kbtextrich:%7B%22type%22%3A%22doc%22%7D\""), "{html}");
    }


    // -- The selection, reading, choosing ------------------------------------

    fn body(json: &str) -> Node {
        Node::from_slice(json.as_bytes()).expect("fixture must parse")
    }

    #[test]
    fn a_selection_inside_one_block_is_one_textblock() {
        let d = body(r#"{"content":[{"content":[{"text":"hello","type":"text"}],"type":"paragraph"}],"type":"doc"}"#);
        let s = slice(&d, 2, 5);
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].children()[0].text().as_deref(), Some("ell"));
        assert_eq!(plain_text(&d, 2, 5), "ell");
    }

    #[test]
    fn a_selection_across_blocks_keeps_its_containers() {
        let d = body(r#"{"content":[{"content":[{"text":"ab","type":"text"}],"type":"paragraph"},{"content":[{"content":[{"content":[{"text":"cd","type":"text"}],"type":"paragraph"}],"type":"listItem"}],"type":"bulletList"}],"type":"doc"}"#);
        // p 0..4 ("ab" 1..3), list 4, item 5, p 6, "cd" 7..9
        let s = slice(&d, 2, 8);
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].children()[0].text().as_deref(), Some("b"));
        assert_eq!(s[1].node_type(), Some("bulletList"));
        assert_eq!(plain_text(&d, 2, 8), "b\nc");
    }

    #[test]
    fn the_cf_html_fragment_is_found_by_offsets() {
        let wrapped = cf_html_wrap("<p>x</p>");
        assert_eq!(cf_html_fragment(&wrapped), "<p>x</p>");
        assert_eq!(cf_html_fragment("<html><!--StartFragment--><b>y</b><!--EndFragment--></html>"), "<b>y</b>");
    }

    #[test]
    fn the_richest_readable_format_wins_unless_plain_is_asked() {
        let blocks = vec![body(r#"{"content":[{"text":"p","type":"text"}],"type":"paragraph"}"#)];
        let private = encode_private(&blocks).expect("encodes");
        assert!(matches!(choose(Some(&private), Some("<p>h</p>"), Some("t"), false), Some(Pasted::Blocks(_))));
        assert!(matches!(choose(None, Some("<p>h</p>"), Some("t"), false), Some(Pasted::Blocks(_))));
        assert!(matches!(choose(Some(&private), Some("<p>h</p>"), Some("t\r\nu"), true), Some(Pasted::Text(t)) if t == "t\nu"));
    }
}
