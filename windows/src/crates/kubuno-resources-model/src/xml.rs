//! A small, position-tracking XML reader — enough for `.kbres` files and the `.resx`/`.resw` files
//! the conversion tool imports: elements, attributes, text, CDATA, comments, the XML declaration, a
//! skipped DOCTYPE and the predefined/numeric entities. No namespaces processing (prefixes are kept
//! in the names), no DTD. Every element and attribute value keeps its byte range in the source, so
//! the language server can point diagnostics and go-to-definition at the right place.
//!
//! Written here rather than taken from crates.io: the format is ours and tiny, the macro crate stays
//! dependency-light, and the ranges are exactly what the tooling needs.

use std::ops::Range;

/// One attribute: its name, its decoded value and the byte range of the value (inside the quotes).
#[derive(Debug, Clone, PartialEq)]
pub struct Attribute {
    pub name: String,
    pub value: String,
    pub name_range: Range<usize>,
    pub value_range: Range<usize>,
}

/// A child of an element.
#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    Element(Element),
    /// Text or CDATA, entities decoded.
    Text(String),
    Comment(String),
}

/// One element.
#[derive(Debug, Clone, PartialEq)]
pub struct Element {
    pub name: String,
    pub attributes: Vec<Attribute>,
    pub children: Vec<Node>,
    /// From `<` to the end of the closing tag (or of `/>`).
    pub range: Range<usize>,
    /// The element name in its start tag.
    pub name_range: Range<usize>,
    /// The content between the start and the end tag (empty for `<a/>`).
    pub content_range: Range<usize>,
}

impl Element {
    /// The attribute `name`, if present.
    pub fn attribute(&self, name: &str) -> Option<&Attribute> {
        self.attributes.iter().find(|a| a.name == name)
    }

    /// The value of attribute `name`, if present.
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attribute(name).map(|a| a.value.as_str())
    }

    /// The child elements.
    pub fn elements(&self) -> impl Iterator<Item = &Element> {
        self.children.iter().filter_map(|n| match n {
            Node::Element(e) => Some(e),
            _ => None,
        })
    }

    /// The concatenated text (and CDATA) children.
    pub fn text(&self) -> String {
        let mut out = String::new();
        for n in &self.children {
            if let Node::Text(t) = n {
                out.push_str(t);
            }
        }
        out
    }
}

/// A parse error, with the byte offset where it was found.
#[derive(Debug, Clone, PartialEq)]
pub struct XmlError {
    pub message: String,
    pub offset: usize,
}

impl std::fmt::Display for XmlError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} (at byte {})", self.message, self.offset)
    }
}

/// Parses `text` and returns its root element.
pub fn parse(text: &str) -> Result<Element, XmlError> {
    let mut p = Parser { s: text, i: 0 };
    // A UTF-8 byte order mark.
    if p.rest().starts_with('\u{FEFF}') {
        p.i += '\u{FEFF}'.len_utf8();
    }
    p.skip_misc()?;
    if !p.rest().starts_with('<') {
        return Err(p.err("expected the root element"));
    }
    let root = p.element()?;
    p.skip_misc()?;
    if p.i < p.s.len() {
        return Err(p.err("unexpected content after the root element"));
    }
    Ok(root)
}

struct Parser<'a> {
    s: &'a str,
    i: usize,
}

impl<'a> Parser<'a> {
    fn rest(&self) -> &'a str {
        &self.s[self.i..]
    }

    fn err(&self, message: impl Into<String>) -> XmlError {
        XmlError { message: message.into(), offset: self.i }
    }

    fn skip_ws(&mut self) {
        let trimmed = self.rest().trim_start_matches([' ', '\t', '\r', '\n']);
        self.i = self.s.len() - trimmed.len();
    }

    /// Whitespace, comments, processing instructions and a DOCTYPE outside the root element.
    fn skip_misc(&mut self) -> Result<(), XmlError> {
        loop {
            self.skip_ws();
            let r = self.rest();
            if r.starts_with("<?") {
                self.skip_past("?>")?;
            } else if r.starts_with("<!--") {
                self.skip_past("-->")?;
            } else if r.starts_with("<!DOCTYPE") {
                self.skip_doctype()?;
            } else {
                return Ok(());
            }
        }
    }

    fn skip_past(&mut self, end: &str) -> Result<(), XmlError> {
        match self.rest().find(end) {
            Some(at) => {
                self.i += at + end.len();
                Ok(())
            }
            None => Err(self.err(format!("missing `{end}`"))),
        }
    }

    fn skip_doctype(&mut self) -> Result<(), XmlError> {
        let mut depth = 0usize;
        for (k, c) in self.rest().char_indices() {
            match c {
                '[' => depth += 1,
                ']' => depth = depth.saturating_sub(1),
                '>' if depth == 0 => {
                    self.i += k + 1;
                    return Ok(());
                }
                _ => {}
            }
        }
        Err(self.err("unterminated DOCTYPE"))
    }

    fn name(&mut self) -> Result<(String, Range<usize>), XmlError> {
        let start = self.i;
        let len = self.rest().find(|c: char| c.is_whitespace() || matches!(c, '/' | '>' | '=' | '<' | '"' | '\'')).unwrap_or(self.rest().len());
        if len == 0 {
            return Err(self.err("expected a name"));
        }
        self.i += len;
        Ok((self.s[start..self.i].to_string(), start..self.i))
    }

    fn element(&mut self) -> Result<Element, XmlError> {
        let start = self.i;
        self.i += 1; // `<`
        let (name, name_range) = self.name()?;
        let mut attributes = Vec::new();
        loop {
            self.skip_ws();
            let r = self.rest();
            if r.starts_with("/>") {
                self.i += 2;
                return Ok(Element { name, attributes, children: Vec::new(), range: start..self.i, name_range, content_range: self.i..self.i });
            }
            if r.starts_with('>') {
                self.i += 1;
                break;
            }
            if r.is_empty() {
                return Err(self.err(format!("unterminated start tag `<{name}`")));
            }
            let (attr_name, attr_range) = self.name()?;
            self.skip_ws();
            if !self.rest().starts_with('=') {
                return Err(self.err(format!("attribute `{attr_name}` has no value")));
            }
            self.i += 1;
            self.skip_ws();
            let quote = match self.rest().chars().next() {
                Some(q @ ('"' | '\'')) => q,
                _ => return Err(self.err(format!("the value of `{attr_name}` must be quoted"))),
            };
            self.i += 1;
            let value_start = self.i;
            let Some(len) = self.rest().find(quote) else { return Err(self.err(format!("unterminated value of `{attr_name}`"))) };
            let raw = &self.s[value_start..value_start + len];
            self.i += len + 1;
            if attributes.iter().any(|a: &Attribute| a.name == attr_name) {
                return Err(XmlError { message: format!("duplicate attribute `{attr_name}`"), offset: attr_range.start });
            }
            let value = decode(raw).map_err(|m| XmlError { message: m, offset: value_start })?;
            attributes.push(Attribute { name: attr_name, value, name_range: attr_range, value_range: value_start..value_start + len });
        }
        let content_start = self.i;
        let mut children = Vec::new();
        loop {
            let r = self.rest();
            if r.is_empty() {
                return Err(self.err(format!("element `<{name}>` is not closed")));
            }
            if r.starts_with("</") {
                let content_end = self.i;
                self.i += 2;
                let (end_name, _) = self.name()?;
                if end_name != name {
                    return Err(self.err(format!("`</{end_name}>` closes `<{name}>`")));
                }
                self.skip_ws();
                if !self.rest().starts_with('>') {
                    return Err(self.err("expected `>`"));
                }
                self.i += 1;
                return Ok(Element { name, attributes, children, range: start..self.i, name_range, content_range: content_start..content_end });
            }
            if r.starts_with("<!--") {
                let body_start = self.i + 4;
                self.skip_past("-->")?;
                children.push(Node::Comment(self.s[body_start..self.i - 3].to_string()));
            } else if r.starts_with("<![CDATA[") {
                let body_start = self.i + 9;
                self.skip_past("]]>")?;
                push_text(&mut children, &self.s[body_start..self.i - 3]);
            } else if r.starts_with("<?") {
                self.skip_past("?>")?;
            } else if r.starts_with('<') {
                children.push(Node::Element(self.element()?));
            } else {
                let len = r.find('<').unwrap_or(r.len());
                let raw = &self.s[self.i..self.i + len];
                let text = decode(raw).map_err(|m| XmlError { message: m, offset: self.i })?;
                self.i += len;
                push_text(&mut children, &text);
            }
        }
    }
}

fn push_text(children: &mut Vec<Node>, text: &str) {
    if let Some(Node::Text(last)) = children.last_mut() {
        last.push_str(text);
    } else {
        children.push(Node::Text(text.to_string()));
    }
}

/// Decodes the predefined and numeric entities of `raw`.
pub fn decode(raw: &str) -> Result<String, String> {
    if !raw.contains('&') {
        return Ok(raw.to_string());
    }
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        rest = &rest[at + 1..];
        let Some(end) = rest.find(';') else { return Err("`&` without `;` (write `&amp;`)".to_string()) };
        let entity = &rest[..end];
        let c = match entity {
            "lt" => '<',
            "gt" => '>',
            "amp" => '&',
            "quot" => '"',
            "apos" => '\'',
            _ => {
                let code = if let Some(hex) = entity.strip_prefix("#x").or_else(|| entity.strip_prefix("#X")) {
                    u32::from_str_radix(hex, 16).ok()
                } else if let Some(dec) = entity.strip_prefix('#') {
                    dec.parse::<u32>().ok()
                } else {
                    None
                };
                code.and_then(char::from_u32).ok_or_else(|| format!("unknown entity `&{entity};`"))?
            }
        };
        out.push(c);
        rest = &rest[end + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

/// Escapes text content (`&`, `<`, `>`; a `\r` as `&#13;` so it survives a round trip).
pub fn escape_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '\r' => out.push_str("&#13;"),
            _ => out.push(c),
        }
    }
    out
}

/// Escapes an attribute value written between double quotes (line breaks and tabs as character
/// references, so they survive attribute-value normalisation).
pub fn escape_attr(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\n' => out.push_str("&#10;"),
            '\r' => out.push_str("&#13;"),
            '\t' => out.push_str("&#9;"),
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_elements_attributes_text_and_ranges() {
        let src = "<?xml version=\"1.0\"?>\n<!-- c -->\n<R a=\"1 &amp; 2\"><S Name='x'>hi &lt;there&gt; &#233;<![CDATA[<raw>]]></S><E/></R>\n";
        let root = parse(src).unwrap();
        assert_eq!(root.name, "R");
        assert_eq!(root.attr("a"), Some("1 & 2"));
        let kids: Vec<_> = root.elements().collect();
        assert_eq!(kids.len(), 2);
        assert_eq!(kids[0].text(), "hi <there> é<raw>");
        assert_eq!(&src[kids[0].attribute("Name").unwrap().value_range.clone()], "x");
        assert_eq!(&src[kids[0].name_range.clone()], "S");
        assert_eq!(kids[1].content_range, kids[1].range.end..kids[1].range.end);
    }

    #[test]
    fn reports_errors_with_offsets() {
        assert!(parse("<a><b></a>").unwrap_err().message.contains("closes"));
        assert!(parse("<a x=1/>").unwrap_err().message.contains("quoted"));
        assert!(parse("<a>&nope;</a>").unwrap_err().message.contains("entity"));
        assert!(parse("<a x='1' x='2'/>").unwrap_err().message.contains("duplicate"));
        assert!(parse("<a/><b/>").is_err());
    }

    #[test]
    fn escapes_round_trip() {
        let s = "a & b < c > \"d\"\nline\ttab\r";
        let attr = format!("<a v=\"{}\">{}</a>", escape_attr(s), escape_text(s));
        let root = parse(&attr).unwrap();
        assert_eq!(root.attr("v"), Some(s));
        assert_eq!(root.text(), s);
    }
}
