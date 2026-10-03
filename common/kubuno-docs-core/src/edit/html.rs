//! Reading HTML from the clipboard into blocks of this format.
//!
//! What is read is what the web editor itself writes and what a word processor's HTML carries
//! that this format can hold: paragraphs and headings (with `text-align`), `<br>`, lists, tables
//! (`colspan`/`rowspan`), block quotes, `<pre>`, rules, images, and the character formatting
//! `<b>/<strong>`, `<i>/<em>`, `<u>`, `<s>/<strike>/<del>`, `<sub>`, `<sup>`, `<code>`, `<a href>`,
//! `<font face size color>` and `style` (`font-weight`, `font-style`, `text-decoration`,
//! `font-family`, `font-size` in pt/px, `color`, `background-color`). Everything else — classes,
//! Word's `mso-` properties, conditional comments, `<o:p>` — is dropped rather than half-kept.
//! Whitespace collapses as in HTML (except in `<pre>`).

use serde_json::{json, Map, Value};

use super::inline;
use super::structure::with_children;
use crate::marks;
use crate::model::Node;

/// One HTML token.
#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Open { name: String, attrs: Vec<(String, String)>, self_closing: bool },
    Close(String),
    Text(String),
}

/// Decodes HTML entities.
pub fn decode_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let semi = rest.find(';').filter(|&e| e > 1 && e <= 12);
        let decoded = semi.and_then(|e| {
            let name = &rest[1..e];
            let c = match name {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                "nbsp" => Some('\u{00A0}'),
                "ndash" => Some('–'),
                "mdash" => Some('—'),
                "hellip" => Some('…'),
                "laquo" => Some('«'),
                "raquo" => Some('»'),
                "rsquo" => Some('’'),
                "lsquo" => Some('‘'),
                "ldquo" => Some('“'),
                "rdquo" => Some('”'),
                "euro" => Some('€'),
                "copy" => Some('©'),
                "reg" => Some('®'),
                _ if name.starts_with("#x") || name.starts_with("#X") => u32::from_str_radix(&name[2..], 16).ok().and_then(char::from_u32),
                _ if name.starts_with('#') => name[1..].parse::<u32>().ok().and_then(char::from_u32),
                _ => None,
            };
            c.map(|c| (c, e))
        });
        match decoded {
            Some((c, e)) => {
                out.push(c);
                rest = &rest[e + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

fn find_tag_end(html: &str, i: usize) -> Option<usize> {
    let b = html.as_bytes();
    let mut q: Option<u8> = None;
    let mut j = i + 1;
    while j < b.len() {
        match (q, b[j]) {
            (Some(c), x) if x == c => q = None,
            (None, b'"') | (None, b'\'') => q = Some(b[j]),
            (None, b'>') => return Some(j),
            _ => {}
        }
        j += 1;
    }
    None
}

fn parse_attrs(s: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let b = s.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        let ns = i;
        while i < b.len() && !b[i].is_ascii_whitespace() && b[i] != b'=' {
            i += 1;
        }
        if ns == i {
            i += 1;
            continue;
        }
        let name = s[ns..i].to_ascii_lowercase();
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        let mut value = String::new();
        if i < b.len() && b[i] == b'=' {
            i += 1;
            while i < b.len() && b[i].is_ascii_whitespace() {
                i += 1;
            }
            if i < b.len() && (b[i] == b'"' || b[i] == b'\'') {
                let q = b[i];
                let vs = i + 1;
                i = vs;
                while i < b.len() && b[i] != q {
                    i += 1;
                }
                value = decode_entities(&s[vs..i.min(b.len())]);
                i += 1;
            } else {
                let vs = i;
                while i < b.len() && !b[i].is_ascii_whitespace() {
                    i += 1;
                }
                value = decode_entities(&s[vs..i]);
            }
        }
        out.push((name, value));
    }
    out
}

fn tokenize(html: &str) -> Vec<Tok> {
    let mut out = Vec::new();
    let b = html.as_bytes();
    let mut i = 0usize;
    let mut text_start = 0usize;
    let flush = |out: &mut Vec<Tok>, from: usize, to: usize| {
        if to > from {
            out.push(Tok::Text(decode_entities(&html[from..to])));
        }
    };
    while i < b.len() {
        if b[i] != b'<' {
            i += 1;
            continue;
        }
        if html[i..].starts_with("<!--") {
            flush(&mut out, text_start, i);
            i = html[i + 4..].find("-->").map(|e| i + 4 + e + 3).unwrap_or(b.len());
            text_start = i;
            continue;
        }
        if html[i..].starts_with("<!") || html[i..].starts_with("<?") {
            flush(&mut out, text_start, i);
            i = html[i..].find('>').map(|e| i + e + 1).unwrap_or(b.len());
            text_start = i;
            continue;
        }
        let Some(gt) = find_tag_end(html, i) else {
            i += 1;
            continue;
        };
        let inner = &html[i + 1..gt];
        let first = inner.as_bytes().first().copied().unwrap_or(b' ');
        if !(first.is_ascii_alphabetic() || first == b'/') {
            i += 1;
            continue;
        }
        flush(&mut out, text_start, i);
        if let Some(name) = inner.strip_prefix('/') {
            out.push(Tok::Close(name.trim().to_ascii_lowercase()));
            i = gt + 1;
            text_start = i;
            continue;
        }
        let self_closing = inner.trim_end().ends_with('/');
        let inner = inner.trim_end().trim_end_matches('/');
        let name_end = inner.find(|c: char| c.is_whitespace()).unwrap_or(inner.len());
        let name = inner[..name_end].to_ascii_lowercase();
        out.push(Tok::Open { name: name.clone(), attrs: parse_attrs(&inner[name_end..]), self_closing });
        i = gt + 1;
        if matches!(name.as_str(), "script" | "style" | "head" | "title" | "xml") {
            let close = format!("</{name}");
            let lower = html[i..].to_ascii_lowercase();
            let end = lower.find(&close).map(|e| i + e).unwrap_or(b.len());
            i = html[end..].find('>').map(|e| end + e + 1).unwrap_or(b.len());
            out.push(Tok::Close(name));
        }
        text_start = i;
    }
    flush(&mut out, text_start, b.len());
    out
}

fn attr<'a>(attrs: &'a [(String, String)], key: &str) -> Option<&'a str> {
    attrs.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
}

fn style_of(attrs: &[(String, String)]) -> Vec<(String, String)> {
    attr(attrs, "style")
        .map(|s| {
            s.split(';')
                .filter_map(|d| {
                    let (k, v) = d.split_once(':')?;
                    Some((k.trim().to_ascii_lowercase(), v.trim().to_string()))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn css<'a>(style: &'a [(String, String)], key: &str) -> Option<&'a str> {
    style.iter().rev().find(|(k, _)| k == key).map(|(_, v)| v.as_str()).filter(|v| !v.is_empty())
}

fn trim_number(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else {
        format!("{n}")
    }
}

/// A CSS font size → `"Npt"` (px converted; keywords ignored).
fn css_size_to_pt(v: &str) -> Option<String> {
    let v = v.trim().to_ascii_lowercase();
    let num = |s: &str| s.trim().parse::<f64>().ok().filter(|n| n.is_finite() && *n > 0.0);
    let pt = match (v.strip_suffix("pt"), v.strip_suffix("px")) {
        (Some(p), _) => num(p)?,
        (None, Some(p)) => num(p)? * 0.75,
        _ => return None,
    };
    Some(format!("{}pt", trim_number((pt * 2.0).round() / 2.0)))
}

fn css_family(v: &str) -> Option<String> {
    let f = v.split(',').next()?.trim().trim_matches(|c| c == '"' || c == '\'').trim();
    (!f.is_empty() && !f.eq_ignore_ascii_case("inherit")).then(|| f.to_string())
}

/// The marks an inline element (or the style of a block) adds.
fn inline_marks(name: &str, attrs: &[(String, String)]) -> Vec<Value> {
    let mut out = Vec::new();
    match name {
        "b" | "strong" => out.push(marks::simple("bold")),
        "i" | "em" => out.push(marks::simple("italic")),
        "u" | "ins" => out.push(marks::simple("underline")),
        "s" | "strike" | "del" => out.push(marks::simple("strike")),
        "sub" => out.push(marks::simple("subscript")),
        "sup" => out.push(marks::simple("superscript")),
        "code" | "kbd" | "tt" => out.push(marks::simple("code")),
        "a" => {
            if let Some(h) = attr(attrs, "href").filter(|h| !h.is_empty()) {
                out.push(json!({ "type": "link", "attrs": { "href": h, "target": "_blank", "rel": "noopener noreferrer nofollow", "class": Value::Null } }));
            }
        }
        _ => {}
    }
    let style = style_of(attrs);
    let mut ts = Map::new();
    if name == "font" {
        if let Some(f) = attr(attrs, "face").and_then(css_family) {
            ts.insert("fontFamily".into(), json!(f));
        }
        if let Some(c) = attr(attrs, "color").filter(|c| !c.is_empty()) {
            ts.insert("color".into(), json!(c));
        }
    }
    if let Some(w) = css(&style, "font-weight") {
        if w == "bold" || w == "bolder" || w.parse::<u32>().is_ok_and(|n| n >= 600) {
            out.push(marks::simple("bold"));
        }
    }
    if css(&style, "font-style").is_some_and(|s| s == "italic" || s == "oblique") {
        out.push(marks::simple("italic"));
    }
    if let Some(d) = css(&style, "text-decoration").or_else(|| css(&style, "text-decoration-line")) {
        if d.contains("underline") {
            out.push(marks::simple("underline"));
        }
        if d.contains("line-through") {
            out.push(marks::simple("strike"));
        }
    }
    if let Some(f) = css(&style, "font-family").and_then(css_family) {
        ts.insert("fontFamily".into(), json!(f));
    }
    if let Some(s) = css(&style, "font-size").and_then(css_size_to_pt) {
        ts.insert("fontSize".into(), json!(s));
    }
    if let Some(c) = css(&style, "color").filter(|c| !c.eq_ignore_ascii_case("inherit") && !c.eq_ignore_ascii_case("windowtext")) {
        ts.insert("color".into(), json!(c));
    }
    if !ts.is_empty() {
        out.push(json!({ "type": "textStyle", "attrs": Value::Object(ts) }));
    }
    if let Some(bg) = css(&style, "background-color").or_else(|| css(&style, "background")) {
        if !bg.eq_ignore_ascii_case("transparent") && !bg.eq_ignore_ascii_case("white") && !bg.eq_ignore_ascii_case("#ffffff") {
            out.push(json!({ "type": "highlight", "attrs": { "color": bg } }));
        }
    }
    out
}

/// Merges `add` into `base` (text styles merged attribute by attribute).
fn merge_marks(base: &[Value], add: &[Value]) -> Vec<Value> {
    let mut out = base.to_vec();
    for m in add {
        if m.get("type").and_then(Value::as_str) == Some("textStyle") {
            let mut attrs = Map::new();
            if let Some(cur) = out
                .iter()
                .find(|x| x.get("type").and_then(Value::as_str) == Some("textStyle"))
                .and_then(|x| x.get("attrs"))
                .and_then(Value::as_object)
            {
                attrs = cur.clone();
            }
            if let Some(a) = m.get("attrs").and_then(Value::as_object) {
                for (k, v) in a {
                    attrs.insert(k.clone(), v.clone());
                }
            }
            out = marks::add_mark(&out, json!({ "type": "textStyle", "attrs": Value::Object(attrs) }));
        } else {
            out = marks::add_mark(&out, m.clone());
        }
    }
    out
}

struct Para {
    node: Node,
    kids: Vec<Node>,
    explicit: bool,
}

struct Builder {
    blocks: Vec<Node>,
    stack: Vec<(Node, Vec<Node>)>,
    para: Option<Para>,
    /// Marks per open inline element (popped by tag name).
    inline: Vec<(String, Vec<Value>)>,
    pre: usize,
}

impl Builder {
    fn marks(&self) -> Vec<Value> {
        let mut out = Vec::new();
        for (_, m) in &self.inline {
            out = merge_marks(&out, m);
        }
        out
    }

    fn push_block(&mut self, node: Node) {
        match self.stack.last_mut() {
            Some((_, kids)) => kids.push(node),
            None => self.blocks.push(node),
        }
    }

    fn open_para(&mut self, node: Node) {
        self.close_para();
        self.para = Some(Para { node, kids: Vec::new(), explicit: true });
    }

    fn close_para(&mut self) {
        let Some(Para { node, mut kids, explicit }) = self.para.take() else { return };
        if self.pre == 0 {
            if let Some(t) = kids.first().and_then(Node::text) {
                let tt = t.trim_start_matches(' ').to_string();
                if tt.is_empty() {
                    kids.remove(0);
                } else if let Some(f) = kids.first_mut() {
                    f.set_text(&tt);
                }
            }
            if let Some(t) = kids.last().and_then(Node::text) {
                let tt = t.trim_end_matches(' ').to_string();
                if tt.is_empty() {
                    kids.pop();
                } else if let Some(l) = kids.last_mut() {
                    l.set_text(&tt);
                }
            }
        }
        if kids.is_empty() && !explicit {
            return;
        }
        let n = if kids.is_empty() { node } else { with_children(&node, kids) };
        self.push_block(n);
    }

    fn text(&mut self, t: &str) {
        let s = if self.pre > 0 {
            t.to_string()
        } else {
            let mut s = String::with_capacity(t.len());
            let mut space = false;
            for c in t.chars() {
                if matches!(c, ' ' | '\t' | '\n' | '\r' | '\u{000C}') {
                    if !space {
                        s.push(' ');
                    }
                    space = true;
                } else {
                    s.push(c);
                    space = false;
                }
            }
            s
        };
        if s.is_empty() {
            return;
        }
        if self.para.is_none() {
            if s.trim().is_empty() {
                return;
            }
            self.para = Some(Para { node: Node::of_type("paragraph"), kids: Vec::new(), explicit: false });
        }
        let raw = marks::to_raw(&self.marks());
        let Some(p) = self.para.as_mut() else { return };
        let mut s = s;
        if s.starts_with(' ') && (p.kids.is_empty() || p.kids.last().and_then(Node::text).is_some_and(|x| x.ends_with(' '))) && self.pre == 0 {
            s = s[1..].to_string();
        }
        if !s.is_empty() {
            inline::push_merged(&mut p.kids, Node::text_node(&s, raw.as_deref()));
        }
    }

    fn inline_node(&mut self, node: Node) {
        if self.para.is_none() {
            self.para = Some(Para { node: Node::of_type("paragraph"), kids: Vec::new(), explicit: false });
        }
        if let Some(p) = self.para.as_mut() {
            p.kids.push(node);
        }
    }

    fn open_container(&mut self, node: Node) {
        self.close_para();
        self.stack.push((node, Vec::new()));
    }

    fn close_container(&mut self, types: &[&str]) {
        self.close_para();
        let Some(at) = self.stack.iter().rposition(|(n, _)| n.node_type().is_some_and(|t| types.contains(&t))) else { return };
        while self.stack.len() > at {
            let Some((node, kids)) = self.stack.pop() else { break };
            let t = node.node_type().unwrap_or("").to_string();
            let kids: Vec<Node> = match t.as_str() {
                "listItem" | "tableCell" | "tableHeader" | "blockquote" if kids.is_empty() => vec![Node::of_type("paragraph")],
                "bulletList" | "orderedList" => kids
                    .into_iter()
                    .map(|k| if k.node_type() == Some("listItem") { k } else { Node::element("listItem", None, vec![k]) })
                    .collect(),
                "tableRow" => kids
                    .into_iter()
                    .map(|k| if matches!(k.node_type(), Some("tableCell") | Some("tableHeader")) { k } else { Node::element("tableCell", None, vec![k]) })
                    .collect(),
                _ => kids,
            };
            if kids.is_empty() && matches!(t.as_str(), "bulletList" | "orderedList" | "table" | "tableRow") {
                continue;
            }
            self.push_block(with_children(&node, kids));
        }
    }

    fn token(&mut self, tok: Tok) {
        match tok {
            Tok::Text(t) => self.text(&t),
            Tok::Open { name, attrs, self_closing } => self.open(&name, &attrs, self_closing),
            Tok::Close(name) => self.close(&name),
        }
    }

    fn block_attrs(attrs: &[(String, String)]) -> Option<Value> {
        let style = style_of(attrs);
        let align = css(&style, "text-align").or_else(|| attr(attrs, "align")).map(str::to_ascii_lowercase);
        match align.as_deref() {
            Some(a @ ("left" | "center" | "right" | "justify")) => Some(json!({ "textAlign": a })),
            _ => None,
        }
    }

    fn open(&mut self, name: &str, attrs: &[(String, String)], self_closing: bool) {
        match name {
            "p" | "div" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "dt" | "dd" | "address" | "center" => {
                let a = Self::block_attrs(attrs);
                let node = if let Some(l) = name.strip_prefix('h').and_then(|d| d.parse::<u32>().ok()) {
                    let mut m = a.and_then(|v| v.as_object().cloned()).unwrap_or_default();
                    m.insert("level".into(), json!(l));
                    Node::element("heading", Some(Value::Object(m)), vec![])
                } else {
                    Node::element("paragraph", a, vec![])
                };
                // A block's own style (Word puts the font on the <p>) applies to its text.
                self.open_para(node);
                let block_marks = inline_marks("", attrs);
                self.inline.push((name.to_string(), block_marks));
                if self_closing {
                    self.close(name);
                }
            }
            "br" => {
                if self.para.is_some() {
                    self.inline_node(Node::of_type("hardBreak"));
                } else {
                    self.para = Some(Para { node: Node::of_type("paragraph"), kids: Vec::new(), explicit: true });
                    self.close_para();
                }
            }
            "hr" => {
                self.close_para();
                self.push_block(Node::of_type("horizontalRule"));
            }
            "img" => {
                if let Some(src) = attr(attrs, "src").filter(|s| !s.is_empty()) {
                    let mut a = Map::new();
                    a.insert("src".into(), json!(src));
                    if let Some(alt) = attr(attrs, "alt") {
                        a.insert("alt".into(), json!(alt));
                    }
                    for k in ["width", "height"] {
                        if let Some(v) = attr(attrs, k).and_then(|v| v.trim_end_matches("px").parse::<f64>().ok()) {
                            a.insert(k.into(), json!(v));
                        }
                    }
                    self.close_para();
                    self.push_block(Node::element("image", Some(Value::Object(a)), vec![]));
                }
            }
            "ul" => self.open_container(Node::of_type("bulletList")),
            "ol" => self.open_container(Node::of_type("orderedList")),
            "li" => {
                // An item opened while the previous one is still open closes it.
                if self.stack.last().is_some_and(|(n, _)| n.node_type() == Some("listItem")) {
                    self.close_container(&["listItem"]);
                }
                self.open_container(Node::of_type("listItem"));
            }
            "blockquote" => self.open_container(Node::of_type("blockquote")),
            "table" => self.open_container(Node::of_type("table")),
            "tr" => {
                if self.stack.last().is_some_and(|(n, _)| matches!(n.node_type(), Some("tableCell") | Some("tableHeader"))) {
                    self.close_container(&["tableCell", "tableHeader"]);
                }
                if self.stack.last().is_some_and(|(n, _)| n.node_type() == Some("tableRow")) {
                    self.close_container(&["tableRow"]);
                }
                self.open_container(Node::of_type("tableRow"));
            }
            "td" | "th" => {
                if self.stack.last().is_some_and(|(n, _)| matches!(n.node_type(), Some("tableCell") | Some("tableHeader"))) {
                    self.close_container(&["tableCell", "tableHeader"]);
                }
                let mut a = Map::new();
                for k in ["colspan", "rowspan"] {
                    if let Some(n) = attr(attrs, k).and_then(|v| v.parse::<u32>().ok()).filter(|n| *n > 1) {
                        a.insert(k.into(), json!(n));
                    }
                }
                self.open_container(Node::element("tableCell", (!a.is_empty()).then_some(Value::Object(a)), vec![]));
            }
            "pre" => {
                self.pre += 1;
                self.open_para(Node::of_type("codeBlock"));
            }
            "thead" | "tbody" | "tfoot" | "colgroup" | "col" | "html" | "body" | "span" | "font" | "b" | "strong" | "i" | "em" | "u" | "ins"
            | "s" | "strike" | "del" | "sub" | "sup" | "code" | "kbd" | "tt" | "a" | "small" | "big" | "mark" | "label" | "abbr" | "cite"
            | "q" | "dfn" | "var" | "samp" | "bdi" | "bdo" | "time" | "o:p"
                if !self_closing =>
            {
                let mut m = inline_marks(name, attrs);
                if name == "mark" {
                    m.push(json!({ "type": "highlight", "attrs": { "color": "#fff176" } }));
                }
                self.inline.push((name.to_string(), m));
            }
            _ => {}
        }
    }

    fn close(&mut self, name: &str) {
        match name {
            "p" | "div" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "dt" | "dd" | "address" | "center" => {
                self.close_para();
                self.pop_inline(name);
            }
            "pre" => {
                self.close_para();
                self.pre = self.pre.saturating_sub(1);
            }
            "ul" | "ol" => self.close_container(&[if name == "ul" { "bulletList" } else { "orderedList" }]),
            "li" => self.close_container(&["listItem"]),
            "blockquote" => self.close_container(&["blockquote"]),
            "table" => self.close_container(&["table"]),
            "tr" => self.close_container(&["tableRow"]),
            "td" | "th" => self.close_container(&["tableCell", "tableHeader"]),
            _ => self.pop_inline(name),
        }
    }

    fn pop_inline(&mut self, name: &str) {
        if let Some(i) = self.inline.iter().rposition(|(n, _)| n == name) {
            self.inline.truncate(i);
        }
    }
}

/// Parses an HTML fragment into blocks.
pub fn html_to_blocks(html: &str) -> Vec<Node> {
    let mut b = Builder { blocks: Vec::new(), stack: Vec::new(), para: None, inline: Vec::new(), pre: 0 };
    for tok in tokenize(html) {
        b.token(tok);
    }
    b.close_para();
    while let Some(t) = b.stack.last().and_then(|(n, _)| n.node_type().map(str::to_string)) {
        b.close_container(&[t.as_str()]);
    }
    b.blocks
}

#[cfg(test)]
mod tests {
    use super::*;

    fn outline(n: &Node) -> String {
        match n.node_type().unwrap_or("") {
            "text" => {
                let m = marks::marks_of(n);
                let tags: Vec<String> = m.iter().filter_map(|x| x.get("type").and_then(Value::as_str).map(str::to_string)).collect();
                if tags.is_empty() { n.text().unwrap_or_default() } else { format!("{}<{}>", n.text().unwrap_or_default(), tags.join("+")) }
            }
            t => format!("{}[{}]", t, n.children().iter().map(outline).collect::<Vec<_>>().join(" ")),
        }
    }

    fn read(html: &str) -> String {
        html_to_blocks(html).iter().map(outline).collect::<Vec<_>>().join(" ")
    }

    #[test]
    fn paragraphs_and_inline_formatting() {
        let b = html_to_blocks("<p>Hello <b>big</b> <i>world</i></p>");
        let texts: Vec<String> = b[0].children().iter().map(outline).collect();
        assert_eq!(texts, vec!["Hello ", "big<bold>", " ", "world<italic>"]);
        assert_eq!(read("<h2>T</h2><p>x</p>"), "heading[T] paragraph[x]");
    }

    #[test]
    fn whitespace_collapses_and_entities_decode() {
        assert_eq!(read("<p>  a \n\n  b&nbsp;&amp;c </p>"), "paragraph[a b\u{a0}&c]");
    }

    #[test]
    fn lists_and_tables() {
        assert_eq!(read("<ul><li>a</li><li>b</li></ul>"), "bulletList[listItem[paragraph[a]] listItem[paragraph[b]]]");
        assert_eq!(read("<table><tr><td>1</td><td>2</td></tr></table>"), "table[tableRow[tableCell[paragraph[1]] tableCell[paragraph[2]]]]");
    }

    #[test]
    fn word_html_keeps_text_and_drops_junk() {
        let word = r#"<html><head><style>p.MsoNormal{}</style></head><body><!--StartFragment--><p class=MsoNormal><span style='font-size:14.0pt;font-family:"Times New Roman",serif;mso-fareast-font-family:x'>Bonjour<o:p></o:p></span></p><!--[if !supportLists]--><!--EndFragment--></body></html>"#;
        let blocks = html_to_blocks(word);
        assert_eq!(blocks.len(), 1);
        let t = &blocks[0].children()[0];
        assert_eq!(t.text().as_deref(), Some("Bonjour"));
        let m = marks::marks_of(t);
        assert_eq!(marks::text_style_attr(&m, "fontSize"), Some(json!("14pt")));
        assert_eq!(marks::text_style_attr(&m, "fontFamily"), Some(json!("Times New Roman")));
    }

    #[test]
    fn styles_become_marks() {
        let b = html_to_blocks(r#"<p style="text-align:center"><span style="color:#ff0000;font-weight:bold;background-color:yellow">r</span></p>"#);
        assert_eq!(b[0].attr("textAlign"), Some(json!("center")));
        let m = marks::text_mark_of(&b[0].children()[0]);
        assert!(m.bold);
        assert_eq!(m.color.as_deref(), Some("#ff0000"));
        assert_eq!(m.background.as_deref(), Some("yellow"));
    }

    #[test]
    fn what_the_web_writes_reads_back() {
        let b = html_to_blocks(r#"<p data-pm-slice="0 0 []">a<a href="https://x.y">link</a><br>b</p>"#);
        assert_eq!(b.len(), 1);
        assert_eq!(b[0].children().len(), 4);
        assert_eq!(b[0].children()[2].node_type(), Some("hardBreak"));
        assert!(marks::text_mark_of(&b[0].children()[1]).link.is_some());
    }
}
