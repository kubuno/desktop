//! Character formatting: reading a run's marks, and the mark arrays the commands write.
//!
//! [`TextMark`] is the web engine's `TextMark` (`canvas-engine.ts:26-52`), read by the port of
//! `extractMarks` (`:646-682`) with the same quirks: `code` forces Courier New on a grey ground,
//! `highlight` without a colour is `#fff176`, `fontSize` is a number or a `"13pt"` string (points),
//! `letterSpacing` is stored in points and used in px.
//!
//! Writing goes the other way: a run's `marks` array is a JSON array of `{type, attrs?}` objects
//! and the commands edit it as values ([`add_mark`], [`remove_mark`], [`set_text_style`]), keeping
//! every mark they do not touch (comments, bookmarks, tracked changes, text effects…) verbatim and
//! in place.

use serde_json::{json, Map, Value};

use crate::model::Node;

/// 1 pt in CSS px (96 dpi).
pub const PT_PX: f32 = 96.0 / 72.0;
/// `DEFAULT_PT` (`canvas-engine.ts:232`).
pub const DEFAULT_PT: f32 = 11.0;
/// `DEFAULT_FAM` (`:233`).
pub const DEFAULT_FAMILY: &str = "Arial";
/// `SCRIPT_SCALE` (`:525`): the size of a superscript or subscript relative to its run.
pub const SCRIPT_SCALE: f32 = 0.66;
/// `DEFAULT_CLR` (`:237`): pure black, like Word.
pub const DEFAULT_COLOR: &str = "#000000";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Script {
    Sub,
    Super,
}

/// The formatting of a run, as the engine lays it out and paints it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TextMark {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    pub code: bool,
    /// Points.
    pub font_size: Option<f32>,
    pub font_family: Option<String>,
    /// CSS colour.
    pub color: Option<String>,
    /// Highlight / code ground (CSS colour).
    pub background: Option<String>,
    pub script: Option<Script>,
    /// Small capitals.
    pub caps: bool,
    /// Px (+ expanded, − condensed).
    pub letter_spacing: f32,
    /// The link's `href`, when the run is in a `link` mark (painted blue and underlined by the web's
    /// stylesheet; the canvas only paints what the marks say).
    pub link: Option<String>,
    /// A tracked insertion / deletion is on the run.
    pub insertion: bool,
    pub deletion: bool,
}

impl TextMark {
    /// The size in points (the default when unset).
    pub fn size_pt(&self) -> f32 {
        self.font_size.unwrap_or(DEFAULT_PT)
    }

    /// The family (the default when unset).
    pub fn family(&self) -> &str {
        self.font_family.as_deref().unwrap_or(DEFAULT_FAMILY)
    }

    /// The font size in px as it is measured and painted: points × 4/3, × 0.66 for a script
    /// (`fontStr`, `:527-536`).
    pub fn font_px(&self) -> f32 {
        let px = self.size_pt() * PT_PX;
        if self.script.is_some() {
            px * SCRIPT_SCALE
        } else {
            px
        }
    }

    /// The CSS font string the web measures with (`fontStr`): also the measurement cache key, so a
    /// desktop cache and a browser cache agree on what is "the same font".
    pub fn font_key(&self) -> String {
        let style = if self.italic { "italic" } else { "normal" };
        let variant = if self.caps { "small-caps " } else { "" };
        let weight = if self.bold { "bold" } else { "normal" };
        let base = format!("{style} {variant}{weight} {}px {}", self.font_px(), self.family());
        if self.letter_spacing != 0.0 {
            format!("{base}|{}", self.letter_spacing)
        } else {
            base
        }
    }
}

fn attr_str(mark: &Value, key: &str) -> Option<String> {
    match mark.get("attrs")?.get(key)? {
        Value::Null => None,
        Value::String(s) => Some(s.clone()),
        other => Some(other.to_string()),
    }
}

/// `parseFloat` of a JSON value: a number, or the leading number of a string (`"13pt"`).
pub fn parse_float(v: &Value) -> Option<f32> {
    match v {
        Value::Number(n) => n.as_f64().map(|n| n as f32),
        Value::String(s) => {
            let t = s.trim();
            let end = t
                .char_indices()
                .find(|(i, c)| !(c.is_ascii_digit() || *c == '.' || ((*c == '-' || *c == '+') && *i == 0)))
                .map(|(i, _)| i)
                .unwrap_or(t.len());
            t[..end].parse::<f32>().ok()
        }
        _ => None,
    }
}

/// `extractMarks` over a parsed marks array.
pub fn extract(marks: &[Value]) -> TextMark {
    let mut m = TextMark::default();
    for mark in marks {
        match mark.get("type").and_then(Value::as_str) {
            Some("bold") => m.bold = true,
            Some("italic") => m.italic = true,
            Some("underline") => m.underline = true,
            Some("strike") => m.strike = true,
            Some("subscript") => m.script = Some(Script::Sub),
            Some("superscript") => m.script = Some(Script::Super),
            Some("code") => {
                m.code = true;
                m.font_family = Some("Courier New".into());
                m.background = Some("#f1f3f4".into());
            }
            Some("highlight") => {
                m.background = Some(attr_str(mark, "color").unwrap_or_else(|| "#fff176".into()));
            }
            Some("link") => m.link = Some(attr_str(mark, "href").unwrap_or_default()),
            Some("insertion") => m.insertion = true,
            Some("deletion") => m.deletion = true,
            Some("textStyle") => {
                let attrs = mark.get("attrs");
                if let Some(size) = attrs.and_then(|a| a.get("fontSize")).and_then(parse_float) {
                    // `if (mark.attrs?.fontSize)`: 0 and NaN are ignored.
                    if size > 0.0 {
                        m.font_size = Some(size);
                    }
                }
                if let Some(c) = attr_str(mark, "color").filter(|c| !c.is_empty()) {
                    m.color = Some(c);
                }
                if let Some(f) = attr_str(mark, "fontFamily").filter(|f| !f.is_empty()) {
                    m.font_family = Some(f);
                }
                if attrs.and_then(|a| a.get("smallCaps")).is_some_and(truthy) {
                    m.caps = true;
                }
                if let Some(ls) = attrs.and_then(|a| a.get("letterSpacing")).filter(|v| !v.is_null()).and_then(parse_float) {
                    if ls != 0.0 {
                        m.letter_spacing = ls * PT_PX;
                    }
                }
            }
            _ => {}
        }
    }
    m
}

/// JavaScript truthiness of a JSON value.
pub fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0 && !f.is_nan()),
        Value::String(s) => !s.is_empty(),
        _ => true,
    }
}

/// The parsed marks array of a node (empty when absent, `null` or invalid).
pub fn marks_of(node: &Node) -> Vec<Value> {
    match node.value("marks") {
        Some(Value::Array(a)) => a,
        _ => Vec::new(),
    }
}

/// `extractMarks(node)`.
pub fn text_mark_of(node: &Node) -> TextMark {
    extract(&marks_of(node))
}

/// Whether a marks array holds a mark of `mark_type`.
pub fn has_mark(marks: &[Value], mark_type: &str) -> bool {
    marks.iter().any(|m| m.get("type").and_then(Value::as_str) == Some(mark_type))
}

/// The marks that exclude each other (ProseMirror `excludes`): sub- and superscript.
fn excluded_by(mark_type: &str) -> &'static [&'static str] {
    match mark_type {
        "subscript" => &["superscript"],
        "superscript" => &["subscript"],
        _ => &[],
    }
}

/// Adds (or replaces) a mark of `mark` 's type, removing the marks it excludes. The order of the
/// other marks is kept; a new mark goes last (ProseMirror's `addToSet` sorts by schema rank, which
/// the JSON order then follows — appending is what an unranked reader sees as equivalent).
pub fn add_mark(marks: &[Value], mark: Value) -> Vec<Value> {
    let ty = mark.get("type").and_then(Value::as_str).unwrap_or("").to_string();
    let excluded = excluded_by(&ty);
    let mut out: Vec<Value> = Vec::with_capacity(marks.len() + 1);
    let mut placed = false;
    for m in marks {
        let t = m.get("type").and_then(Value::as_str).unwrap_or("");
        if excluded.contains(&t) {
            continue;
        }
        if t == ty {
            if !placed {
                out.push(mark.clone());
                placed = true;
            }
            continue;
        }
        out.push(m.clone());
    }
    if !placed {
        out.push(mark);
    }
    out
}

/// Removes every mark of `mark_type`.
pub fn remove_mark(marks: &[Value], mark_type: &str) -> Vec<Value> {
    marks.iter().filter(|m| m.get("type").and_then(Value::as_str) != Some(mark_type)).cloned().collect()
}

/// A plain mark `{"type": …}`.
pub fn simple(mark_type: &str) -> Value {
    json!({ "type": mark_type })
}

/// Sets (or, `None`, clears) one attribute of the run's `textStyle` mark, creating the mark when
/// needed and dropping it when it ends up with no attribute set — TipTap's
/// `setMark('textStyle', …)` + `removeEmptyTextStyle`.
pub fn set_text_style(marks: &[Value], key: &str, value: Option<Value>) -> Vec<Value> {
    let existing = marks.iter().find(|m| m.get("type").and_then(Value::as_str) == Some("textStyle"));
    let mut attrs: Map<String, Value> = existing
        .and_then(|m| m.get("attrs"))
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    match value {
        Some(v) => {
            attrs.insert(key.to_string(), v);
        }
        None => {
            attrs.insert(key.to_string(), Value::Null);
        }
    }
    let empty = attrs.values().all(Value::is_null);
    if empty {
        return remove_mark(marks, "textStyle");
    }
    add_mark(marks, json!({ "type": "textStyle", "attrs": Value::Object(attrs) }))
}

/// The `textStyle` attribute of a run, if set.
pub fn text_style_attr(marks: &[Value], key: &str) -> Option<Value> {
    marks
        .iter()
        .find(|m| m.get("type").and_then(Value::as_str) == Some("textStyle"))
        .and_then(|m| m.get("attrs"))
        .and_then(|a| a.get(key))
        .filter(|v| !v.is_null())
        .cloned()
}

/// A marks array as the raw JSON of a `marks` key: `None` for an empty array (the browser omits
/// the key; a text node with no marks has none).
pub fn to_raw(marks: &[Value]) -> Option<Box<serde_json::value::RawValue>> {
    if marks.is_empty() {
        return None;
    }
    serde_json::value::to_raw_value(&marks).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tm(json: &str) -> TextMark {
        let v: Vec<Value> = serde_json::from_str(json).expect("marks parse");
        extract(&v)
    }

    #[test]
    fn the_web_mark_rules_are_read() {
        let m = tm(r##"[{"type":"bold"},{"type":"textStyle","attrs":{"fontSize":"13pt","color":"#ff0000","fontFamily":"Georgia"}}]"##);
        assert!(m.bold);
        assert_eq!(m.font_size, Some(13.0));
        assert_eq!(m.color.as_deref(), Some("#ff0000"));
        assert_eq!(m.family(), "Georgia");
        let code = tm(r##"[{"type":"code"}]"##);
        assert_eq!(code.family(), "Courier New");
        assert_eq!(code.background.as_deref(), Some("#f1f3f4"));
        assert_eq!(tm(r##"[{"type":"highlight"}]"##).background.as_deref(), Some("#fff176"));
        assert_eq!(tm(r##"[{"type":"highlight","attrs":{"color":"#00ff00"}}]"##).background.as_deref(), Some("#00ff00"));
        let s = tm(r##"[{"type":"superscript"}]"##);
        assert!((s.font_px() - 11.0 * PT_PX * SCRIPT_SCALE).abs() < 1e-4);
    }

    #[test]
    fn the_font_key_is_the_webs_font_string() {
        let m = tm(r##"[{"type":"bold"},{"type":"italic"}]"##);
        assert_eq!(m.font_key(), format!("italic bold {}px Arial", 11.0 * PT_PX));
    }

    #[test]
    fn sub_and_superscript_exclude_each_other() {
        let marks = vec![simple("bold"), simple("subscript")];
        let out = add_mark(&marks, simple("superscript"));
        assert_eq!(out, vec![simple("bold"), simple("superscript")]);
    }

    #[test]
    fn text_style_attributes_are_merged_and_an_empty_style_is_dropped() {
        let marks = set_text_style(&[], "fontSize", Some(json!("14pt")));
        assert_eq!(text_style_attr(&marks, "fontSize"), Some(json!("14pt")));
        let marks = set_text_style(&marks, "color", Some(json!("#123456")));
        assert_eq!(text_style_attr(&marks, "fontSize"), Some(json!("14pt")));
        let marks = set_text_style(&marks, "fontSize", None);
        let marks = set_text_style(&marks, "color", None);
        assert!(marks.is_empty());
    }
}
