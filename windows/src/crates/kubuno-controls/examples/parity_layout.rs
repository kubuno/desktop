//! The port's side of the layout parity harness.
//!
//! It reads the SHARED case list (`tools/winforms-ref/parity/cases.json`), runs
//! every case through the real layout engine — [`kubuno_controls::layout::layout`]
//! and [`Panel::local_display_rect`], nothing re-implemented — and writes
//! `tools/winforms-ref/parity/out/layout-port.json` in the same shape the
//! WinForms probe writes. `compare-layout.ps1` then diffs the two.
//!
//! ## Why the case list is shared rather than duplicated
//!
//! Two hand-written case lists agree on the day they are written and drift
//! silently afterwards, and a parity harness whose two halves test different
//! things reports « everything matches » for the wrong reason. Both probes
//! therefore parse the same file at run time. The parser below is tiny and
//! hand-rolled because this crate has no serde dependency and this example may
//! not add one; it accepts exactly the subset of JSON the case file uses.
//!
//! ## What « the same case » means on this side
//!
//! Each node is a [`Panel`] — leaf and container differ only in whether they
//! have children, so no control-specific preferred size can leak into the
//! numbers. The tree is walked exactly as [`Children::perform_layout`] walks one
//! level, recursing into each child afterwards:
//!
//! * the display rect comes from `Panel::local_display_rect()`,
//! * the items come from `Item::from(&ControlBase)`,
//! * the rectangles come from `layout::layout(display, previous, &items)`,
//! * `previous` is the display rect of this node's PREVIOUS pass, `None` on the
//!   first — the same rule `Children` applies.
//!
//! The recursion is the only thing this file adds, because `Children` lays out
//! one level and stores its children as `Box<dyn Control>`, which cannot be
//! walked back down as concrete containers.
//!
//! ## Coordinate spaces — read this before judging a delta
//!
//! WinForms reports a child's `Bounds` relative to its parent's CLIENT
//! rectangle. The port's layout engine returns rectangles relative to the
//! parent's DISPLAY rectangle (the client rect already deflated by padding —
//! see `local_display_rect`, which deliberately puts its top-left at the
//! origin). With zero padding the two coincide; with padding they differ by the
//! padding's top-left. Both probes therefore also record the container's own
//! display rect, so the compare script can tell that systematic offset apart
//! from a genuine geometry error.
//!
//! Run:
//! ```text
//! cargo run -p kubuno-controls --example parity_layout
//! ```

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use drive_app_controls::Rect;
use kubuno_controls::containers::Panel;
use kubuno_controls::control::{Control, ControlBase};
use kubuno_controls::enums::{AnchorStyles, DockStyle, Padding, Size};
use kubuno_controls::layout::{self, Item};

// ─────────────────────────────────────────────────────────────────────────────
// A minimal JSON reader — just enough for the case file.
// ─────────────────────────────────────────────────────────────────────────────

/// The subset of JSON the case file uses. `Null` exists only so a missing key
/// and an explicit `null` read the same way.
#[derive(Clone, Debug)]
enum Json {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Json>),
    Obj(BTreeMap<String, Json>),
}

impl Json {
    /// Field access that treats a missing key as `Null`, so every reader below
    /// can be written as « take it or fall back ».
    fn get(&self, key: &str) -> &Json {
        const NULL: &Json = &Json::Null;
        match self {
            Json::Obj(m) => m.get(key).unwrap_or(NULL),
            _ => NULL,
        }
    }

    fn arr(&self) -> &[Json] {
        match self {
            Json::Arr(v) => v,
            _ => &[],
        }
    }

    fn str(&self) -> Option<&str> {
        match self {
            Json::Str(s) => Some(s),
            _ => None,
        }
    }

    fn num(&self) -> Option<f64> {
        match self {
            Json::Num(n) => Some(*n),
            _ => None,
        }
    }

    fn bool(&self) -> Option<bool> {
        match self {
            Json::Bool(b) => Some(*b),
            _ => None,
        }
    }

    /// The `n` numbers of a fixed-length array such as `rect` or `padding`.
    fn nums<const N: usize>(&self) -> Option<[f32; N]> {
        let v = self.arr();
        if v.len() != N {
            return None;
        }
        let mut out = [0.0_f32; N];
        for (slot, item) in out.iter_mut().zip(v) {
            *slot = item.num()? as f32;
        }
        Some(out)
    }
}

struct Parser<'a> {
    b: &'a [u8],
    i: usize,
}

impl<'a> Parser<'a> {
    fn parse(text: &'a str) -> Result<Json, String> {
        let mut p = Parser { b: text.as_bytes(), i: 0 };
        let v = p.value()?;
        p.ws();
        if p.i != p.b.len() {
            return Err(format!("trailing input at byte {}", p.i));
        }
        Ok(v)
    }

    fn ws(&mut self) {
        while self.i < self.b.len() && self.b[self.i].is_ascii_whitespace() {
            self.i += 1;
        }
    }

    fn peek(&self) -> Option<u8> {
        self.b.get(self.i).copied()
    }

    fn expect(&mut self, c: u8) -> Result<(), String> {
        if self.peek() == Some(c) {
            self.i += 1;
            Ok(())
        } else {
            Err(format!("expected '{}' at byte {}", c as char, self.i))
        }
    }

    fn value(&mut self) -> Result<Json, String> {
        self.ws();
        match self.peek().ok_or_else(|| "unexpected end".to_string())? {
            b'{' => self.object(),
            b'[' => self.array(),
            b'"' => Ok(Json::Str(self.string()?)),
            b't' => self.literal("true", Json::Bool(true)),
            b'f' => self.literal("false", Json::Bool(false)),
            b'n' => self.literal("null", Json::Null),
            _ => self.number(),
        }
    }

    fn literal(&mut self, word: &str, v: Json) -> Result<Json, String> {
        if self.b[self.i..].starts_with(word.as_bytes()) {
            self.i += word.len();
            Ok(v)
        } else {
            Err(format!("bad literal at byte {}", self.i))
        }
    }

    fn number(&mut self) -> Result<Json, String> {
        let start = self.i;
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() || matches!(c, b'-' | b'+' | b'.' | b'e' | b'E') {
                self.i += 1;
            } else {
                break;
            }
        }
        std::str::from_utf8(&self.b[start..self.i])
            .ok()
            .and_then(|s| s.parse::<f64>().ok())
            .map(Json::Num)
            .ok_or_else(|| format!("bad number at byte {start}"))
    }

    fn string(&mut self) -> Result<String, String> {
        self.expect(b'"')?;
        let mut s = String::new();
        loop {
            let c = self.peek().ok_or_else(|| "unterminated string".to_string())?;
            self.i += 1;
            match c {
                b'"' => return Ok(s),
                b'\\' => {
                    let e = self.peek().ok_or_else(|| "bad escape".to_string())?;
                    self.i += 1;
                    match e {
                        b'n' => s.push('\n'),
                        b't' => s.push('\t'),
                        b'r' => s.push('\r'),
                        b'b' => s.push('\u{8}'),
                        b'f' => s.push('\u{c}'),
                        b'u' => {
                            let hex = std::str::from_utf8(&self.b[self.i..self.i + 4])
                                .map_err(|_| "bad \\u".to_string())?;
                            let n = u32::from_str_radix(hex, 16).map_err(|e| e.to_string())?;
                            self.i += 4;
                            s.push(char::from_u32(n).unwrap_or('\u{fffd}'));
                        }
                        other => s.push(other as char),
                    }
                }
                // The case file is UTF-8 and carries accented prose; copy the
                // raw bytes through and decode at the end of the string.
                _ => {
                    let start = self.i - 1;
                    let mut end = start + 1;
                    while end < self.b.len() && self.b[end] != b'"' && self.b[end] != b'\\' {
                        end += 1;
                    }
                    s.push_str(
                        std::str::from_utf8(&self.b[start..end]).map_err(|e| e.to_string())?,
                    );
                    self.i = end;
                }
            }
        }
    }

    fn array(&mut self) -> Result<Json, String> {
        self.expect(b'[')?;
        let mut items = Vec::new();
        self.ws();
        if self.peek() == Some(b']') {
            self.i += 1;
            return Ok(Json::Arr(items));
        }
        loop {
            items.push(self.value()?);
            self.ws();
            match self.peek() {
                Some(b',') => self.i += 1,
                Some(b']') => {
                    self.i += 1;
                    return Ok(Json::Arr(items));
                }
                _ => return Err(format!("bad array at byte {}", self.i)),
            }
        }
    }

    fn object(&mut self) -> Result<Json, String> {
        self.expect(b'{')?;
        let mut map = BTreeMap::new();
        self.ws();
        if self.peek() == Some(b'}') {
            self.i += 1;
            return Ok(Json::Obj(map));
        }
        loop {
            self.ws();
            let key = self.string()?;
            self.ws();
            self.expect(b':')?;
            map.insert(key, self.value()?);
            self.ws();
            match self.peek() {
                Some(b',') => self.i += 1,
                Some(b'}') => {
                    self.i += 1;
                    return Ok(Json::Obj(map));
                }
                _ => return Err(format!("bad object at byte {}", self.i)),
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// The tree under test.
// ─────────────────────────────────────────────────────────────────────────────

/// One node of a case: a real [`Panel`], its children, and the display rect of
/// its previous layout pass.
///
/// `previous` is the state [`kubuno_controls::containers::Children`] keeps for
/// exactly the same reason — anchoring is defined against the CHANGE in size, so
/// a first pass legitimately moves nothing.
struct Node {
    panel:    Panel,
    kids:     Vec<Node>,
    previous: Option<Rect>,
}

/// Builds a node from its case-file spec.
///
/// The property order matters and mirrors the C# probe: `MinimumSize` /
/// `MaximumSize` first, then the bounds (through `set_bounds`, which clamps them
/// exactly as WinForms' setter does), then dock/anchor. Building them the other
/// way round would feed the two probes different inputs on any case that
/// deliberately violates its own constraints.
fn build(spec: &Json) -> Node {
    let mut panel = Panel::new();
    {
        let c: &mut ControlBase = panel.control_mut();
        c.name = spec.get("name").str().unwrap_or_default().to_string();
        c.margin = read_padding(spec.get("margin"), Padding::all(3.0));
        c.padding = read_padding(spec.get("padding"), Padding::ZERO);
        c.minimum_size = read_size(spec.get("min"));
        c.maximum_size = read_size(spec.get("max"));

        let r = spec.get("rect").nums::<4>().expect("rect must be [x, y, w, h]");
        c.set_bounds(Rect::new(r[0], r[1], r[0] + r[2], r[1] + r[3]));

        c.dock = read_dock(spec.get("dock"));
        c.anchor = read_anchor(spec.get("anchor"));
        c.visible = spec.get("visible").bool().unwrap_or(true);
    }

    let kids = spec.get("children").arr().iter().map(build).collect();
    Node { panel, kids, previous: None }
}

/// Lays a node's children out, then recurses — one pass over the whole subtree.
///
/// The three lines that matter are the ones `Children::perform_layout` runs:
/// take the container's display rect in its own local space, feed the engine the
/// items and the previous rect, write the results back onto the children.
fn lay_out(node: &mut Node) {
    // `Panel::local_display_rect` — the port's own padding/border model, not a
    // reimplementation of it. A `Panel` defaults to `BorderStyle::None`, so the
    // only inset is the padding, which is what the WinForms `Panel` does too.
    let display = node.panel.local_display_rect();
    let items: Vec<Item> = node.kids.iter().map(|k| Item::from(k.panel.control())).collect();
    let previous = node.previous.unwrap_or(display);
    let rects = layout::layout(display, previous, &items);
    node.previous = Some(display);

    for (kid, r) in node.kids.iter_mut().zip(rects) {
        kid.panel.control_mut().bounds = r;
        lay_out(kid);
    }
}

/// Walks the laid-out tree in collection order — the same index a child has in
/// the WinForms `Controls` collection — and records each rectangle.
fn emit(node: &Node, prefix: &str, sink: &mut String) {
    for (i, kid) in node.kids.iter().enumerate() {
        let path =
            if prefix.is_empty() { i.to_string() } else { format!("{prefix}.{i}") };
        let b = kid.panel.control().bounds;
        if !sink.ends_with('[') {
            sink.push(',');
        }
        let _ = write!(
            sink,
            "\n      {{ \"path\": \"{}\", \"name\": \"{}\", \"bounds\": {}",
            path,
            escape(&kid.panel.control().name),
            rect_json(b)
        );
        if !kid.kids.is_empty() {
            // The nested container's display rect, in the SAME space its own
            // children's bounds are expressed in — which for the port is its
            // local space, top-left at the origin. The WinForms probe records
            // `DisplayRectangle`, which is in the container's client space. The
            // compare script uses the pair to separate that origin convention
            // from a real geometry error.
            let _ = write!(sink, ", \"display\": {}", rect_json(kid.panel.local_display_rect()));
        }
        sink.push_str(" }");
        emit(kid, &path, sink);
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Reading the case file's vocabulary.
// ─────────────────────────────────────────────────────────────────────────────

fn read_padding(v: &Json, fallback: Padding) -> Padding {
    match v.nums::<4>() {
        Some(p) => Padding::new(p[0], p[1], p[2], p[3]),
        None => fallback,
    }
}

fn read_size(v: &Json) -> Size {
    match v.nums::<2>() {
        Some(s) => Size::new(s[0], s[1]),
        None => Size::EMPTY,
    }
}

fn read_dock(v: &Json) -> DockStyle {
    match v.str().unwrap_or("None") {
        "None" => DockStyle::None,
        "Top" => DockStyle::Top,
        "Bottom" => DockStyle::Bottom,
        "Left" => DockStyle::Left,
        "Right" => DockStyle::Right,
        "Fill" => DockStyle::Fill,
        other => panic!("unknown DockStyle in the case file: {other}"),
    }
}

fn read_anchor(v: &Json) -> AnchorStyles {
    let Some(s) = v.str() else {
        return AnchorStyles::default();   // Top | Left, as `Control` declares
    };
    let mut a = AnchorStyles::NONE;
    for part in s.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        a = a.union(match part {
            "None" => AnchorStyles::NONE,
            "Top" => AnchorStyles::TOP,
            "Bottom" => AnchorStyles::BOTTOM,
            "Left" => AnchorStyles::LEFT,
            "Right" => AnchorStyles::RIGHT,
            other => panic!("unknown AnchorStyles member in the case file: {other}"),
        });
    }
    a
}

// ─────────────────────────────────────────────────────────────────────────────
// Writing.
// ─────────────────────────────────────────────────────────────────────────────

/// Formats a rectangle the way the C# probe does: `[left, top, right, bottom]`.
/// The port works in `f32` and WinForms in `int`; a whole number is written
/// without a decimal point so the two files read the same, and a fractional one
/// is written as-is rather than rounded — a rounded fraction would hide exactly
/// the sub-DIP disagreement the harness exists to catch.
fn rect_json(r: Rect) -> String {
    format!(
        "[{}, {}, {}, {}]",
        num(r.left),
        num(r.top),
        num(r.right),
        num(r.bottom)
    )
}

fn num(v: f32) -> String {
    if v.fract() == 0.0 && v.is_finite() {
        format!("{}", v as i64)
    } else {
        format!("{v}")
    }
}

fn escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

// ─────────────────────────────────────────────────────────────────────────────

fn main() {
    let parity = parity_dir();
    let case_file = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| parity.join("cases.json"));
    let out_file = std::env::args()
        .nth(2)
        .map(PathBuf::from)
        .unwrap_or_else(|| parity.join("out").join("layout-port.json"));

    let text = std::fs::read_to_string(&case_file)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", case_file.display()));
    let doc = Parser::parse(&text)
        .unwrap_or_else(|e| panic!("cannot parse {}: {e}", case_file.display()));

    let mut body = String::from("[");
    let cases = doc.get("cases").arr();
    for spec in cases {
        // The container itself is a node, so it goes through exactly the same
        // construction path as its children; only its size is driven by the case.
        let size = spec.get("size").nums::<2>().expect("size must be [w, h]");
        let mut root = Node {
            panel: Panel::new(),
            kids: spec.get("children").arr().iter().map(build).collect(),
            previous: None,
        };
        {
            let c = root.panel.control_mut();
            c.name = "container".to_string();
            c.margin = Padding::ZERO;
            c.padding = read_padding(spec.get("padding"), Padding::ZERO);
            c.set_bounds(Rect::new(0.0, 0.0, size[0], size[1]));
        }

        lay_out(&mut root);
        if let Some(r) = spec.get("resize").nums::<2>() {
            root.panel.control_mut().set_bounds(Rect::new(0.0, 0.0, r[0], r[1]));
            lay_out(&mut root);
        }

        let mut children = String::from("[");
        emit(&root, "", &mut children);
        children.push_str(if children.len() == 1 { "]" } else { "\n    ]" });

        if body.len() > 1 {
            body.push(',');
        }
        let _ = write!(
            body,
            "\n    {{\n      \"id\": \"{}\",\n      \"display\": {},\n      \"children\": {}\n    }}",
            escape(spec.get("id").str().unwrap_or("?")),
            rect_json(root.panel.local_display_rect()),
            children
        );
    }
    body.push_str("\n  ]");

    let doc_out = format!(
        "{{\n  \"meta\": {{\n    \"probe\": \"port\",\n    \"crate\": \"kubuno-controls\",\n    \
         \"caseFile\": \"{}\"\n  }},\n  \"cases\": {}\n}}\n",
        escape(&case_file.display().to_string()),
        body
    );

    if let Some(dir) = out_file.parent() {
        std::fs::create_dir_all(dir).expect("cannot create the output directory");
    }
    std::fs::write(&out_file, doc_out)
        .unwrap_or_else(|e| panic!("cannot write {}: {e}", out_file.display()));
    println!("{} cases -> {}", cases.len(), out_file.display());
}

/// `tools/winforms-ref/parity/`, resolved from this crate's manifest so the
/// example runs the same from any working directory.
fn parity_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("tools")
        .join("winforms-ref")
        .join("parity")
}
