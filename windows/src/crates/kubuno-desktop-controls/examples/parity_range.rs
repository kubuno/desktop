//! Numeric parity probe — the PORT half.
//!
//! Replays every case in `tools/winforms-ref/parity/range-cases.txt` through the
//! `kubuno-desktop-controls` range family and writes `range-port.json` beside it, in the
//! same shape the .NET probe (`parity/range-winforms`) writes
//! `range-winforms.json`. `compare-range.ps1` diffs the two.
//!
//! ## Why the comparison is numeric and not visual
//!
//! The port paints in the Kubuno design system, so comparing pixels would only
//! re-measure a difference that is deliberate. What must match to the digit is
//! the **arithmetic and the state machine**: where a value lands, what a step
//! button does at a boundary, which assignments are refused.
//!
//! ## "Throws" and "returns Err" are the same outcome
//!
//! WinForms signals an out-of-range assignment by throwing
//! `ArgumentOutOfRangeException`; the port returns
//! [`kubuno_desktop_controls::range::OutOfRange`]. Both are recorded as status `err`
//! with the mechanism in `detail`, and the comparer compares only the status —
//! so the *shape* of the refusal is allowed to differ while the *decision* is
//! not. A case where one side accepts what the other refuses is a mismatch.
//!
//! Run with:
//!
//! ```text
//! cargo run -p kubuno-desktop-controls --example parity_range -j 1
//! ```

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use kubuno_desktop_controls::range::{
    DomainUpDown, HScrollBar, NumberFormat, NumericUpDown, ScrollBar, TrackBar, VScrollBar,
};

// ─────────────────────────────────────────────────────────────────────────────
// The shared case file
// ─────────────────────────────────────────────────────────────────────────────

struct Case {
    id: String,
    kind: String,
    steps: Vec<String>,
}

/// Parses the shared case file. The grammar is deliberately tiny — `id | kind |
/// step; step` — so this parser and the .NET one can be read as twins.
fn load_cases(text: &str) -> Result<Vec<Case>, String> {
    let mut cases = Vec::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.split('|');
        let id = parts.next().unwrap_or_default().trim().to_string();
        let kind = match parts.next() {
            Some(k) => k.trim().to_string(),
            None => return Err(format!("malformed case line: {raw}")),
        };
        let steps = parts
            .next()
            .unwrap_or("")
            .split(';')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect();
        cases.push(Case { id, kind, steps });
    }
    Ok(cases)
}

// ─────────────────────────────────────────────────────────────────────────────
// The controls under test
// ─────────────────────────────────────────────────────────────────────────────

enum Port {
    HScroll(HScrollBar),
    VScroll(VScrollBar),
    Track(TrackBar),
    Numeric(NumericUpDown),
    Domain(DomainUpDown),
}

struct StepResult {
    op: String,
    status: &'static str,
    detail: String,
}

struct CaseResult {
    id: String,
    kind: String,
    steps: Vec<StepResult>,
    state: Vec<(String, String)>,
    extra: Vec<(String, String)>,
}

impl Port {
    fn new(kind: &str) -> Result<Self, String> {
        Ok(match kind {
            "hscrollbar" => Port::HScroll(HScrollBar::new()),
            "vscrollbar" => Port::VScroll(VScrollBar::new()),
            "trackbar" => Port::Track(TrackBar::new()),
            "numericupdown" => Port::Numeric(NumericUpDown::new()),
            "domainupdown" => Port::Domain(DomainUpDown::new()),
            other => return Err(format!("unknown kind '{other}'")),
        })
    }

    fn apply(&mut self, step: &str) -> Result<(), String> {
        let (op, arg) = match step.split_once('=') {
            Some((o, a)) => (o, Some(a)),
            None => (step, None),
        };
        match self {
            Port::HScroll(h) => apply_scrollbar(h, op, arg),
            Port::VScroll(v) => apply_scrollbar(v, op, arg),
            Port::Track(t) => apply_trackbar(t, op, arg),
            Port::Numeric(n) => apply_numeric(n, op, arg),
            Port::Domain(d) => apply_domain(d, op, arg),
        }
    }

    /// The compared surface. Key order matches the .NET probe's.
    fn state(&self) -> Vec<(String, String)> {
        let pairs: Vec<(&str, String)> = match self {
            Port::HScroll(h) => scrollbar_state(h),
            Port::VScroll(v) => scrollbar_state(v),
            Port::Track(t) => vec![
                ("minimum", num_i32(t.minimum())),
                ("maximum", num_i32(t.maximum())),
                ("value", num_i32(t.value())),
                ("smallChange", num_i32(t.small_change())),
                ("largeChange", num_i32(t.large_change())),
                ("tickFrequency", num_i32(t.tick_frequency())),
            ],
            Port::Numeric(n) => vec![
                ("minimum", num_f64(n.minimum())),
                ("maximum", num_f64(n.maximum())),
                ("value", num_f64(n.value())),
                ("increment", num_f64(n.increment())),
                ("decimalPlaces", num_i32(n.decimal_places())),
                ("hexadecimal", flag(n.hexadecimal())),
                ("thousandsSeparator", flag(n.thousands_separator())),
                ("text", n.display_text()),
            ],
            Port::Domain(d) => vec![
                ("items", d.items().join("|")),
                ("selectedIndex", num_i32(d.selected_index())),
                ("sorted", flag(d.sorted())),
                ("wrap", flag(d.wrap())),
                // `DomainUpDown.Text` is NOT `SelectedItem`: the toolkit keeps
                // displaying the last item after the selection is cleared, so
                // the port carries a real `Text` and it is compared directly.
                ("text", d.text().to_string()),
            ],
        };
        pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
    }

    /// Port-side observations that have no counterpart in the reference JSON's
    /// `state`. Never compared; printed by the comparer as context.
    fn extra(&self) -> Vec<(String, String)> {
        match self {
            // The reference reports the NATIVE scroll info (`nMax`, `nPage`);
            // the port's equivalent is the derived ceiling, so both files carry
            // the ingredients of `Maximum - LargeChange + 1` side by side.
            Port::HScroll(h) => vec![("maxReachable".to_string(), num_i32(h.max_reachable_value()))],
            Port::VScroll(v) => vec![("maxReachable".to_string(), num_i32(v.max_reachable_value()))],
            // The counterpart of the reference's `numTics`. Context only, not
            // compared: the native control drops marks it has no pixels for on
            // an unsized bar, so its count is a rendering artefact for wide
            // ranges rather than a semantic — see the report.
            Port::Track(t) => vec![("tickCount".to_string(), t.tick_count().to_string())],
            _ => Vec::new(),
        }
    }
}

fn scrollbar_state(s: &ScrollBar) -> Vec<(&'static str, String)> {
    vec![
        ("minimum", num_i32(s.minimum())),
        ("maximum", num_i32(s.maximum())),
        ("value", num_i32(s.value())),
        ("smallChange", num_i32(s.small_change())),
        ("largeChange", num_i32(s.large_change())),
    ]
}

fn apply_scrollbar(s: &mut ScrollBar, op: &str, arg: Option<&str>) -> Result<(), String> {
    match op {
        "min" => s.set_minimum(int(arg)?),
        "max" => s.set_maximum(int(arg)?),
        "small" => s.set_small_change(int(arg)?).map_err(|e| e.to_string())?,
        "large" => s.set_large_change(int(arg)?).map_err(|e| e.to_string())?,
        "value" => s.set_value(int(arg)?).map_err(|e| e.to_string())?,
        // The four user gestures, plus the two ends. `first`/`last` are
        // `scroll_to` at the extremes, which is what the native bar's SB_LEFT /
        // SB_RIGHT do — and `last` is how the reachable ceiling is measured.
        "lineup" => s.line_up(),
        "linedown" => s.line_down(),
        "pageup" => s.page_up(),
        "pagedown" => s.page_down(),
        "first" => s.scroll_to(i32::MIN).map_err(|e| e.to_string())?,
        "last" => s.scroll_to(i32::MAX).map_err(|e| e.to_string())?,
        other => return Err(format!("step '{other}' is not defined for ScrollBar")),
    }
    Ok(())
}

fn apply_trackbar(t: &mut TrackBar, op: &str, arg: Option<&str>) -> Result<(), String> {
    match op {
        "min" => t.set_minimum(int(arg)?),
        "max" => t.set_maximum(int(arg)?),
        "small" => t.set_small_change(int(arg)?).map_err(|e| e.to_string())?,
        "large" => t.set_large_change(int(arg)?).map_err(|e| e.to_string())?,
        "freq" => t.set_tick_frequency(int(arg)?),
        "value" => t.set_value(int(arg)?).map_err(|e| e.to_string())?,
        other => return Err(format!("step '{other}' is not defined for TrackBar")),
    }
    Ok(())
}

fn apply_numeric(n: &mut NumericUpDown, op: &str, arg: Option<&str>) -> Result<(), String> {
    match op {
        "min" => n.set_minimum(dec(arg)?),
        "max" => n.set_maximum(dec(arg)?),
        "inc" => n.set_increment(dec(arg)?).map_err(|e| e.to_string())?,
        "dp" => n.set_decimal_places(int(arg)?).map_err(|e| e.to_string())?,
        "hex" => n.set_hexadecimal(flag_of(arg)),
        "sep" => n.set_thousands_separator(flag_of(arg)),
        "value" => n.set_value(dec(arg)?).map_err(|e| e.to_string())?,
        "up" => n.up_button(),
        "down" => n.down_button(),
        other => return Err(format!("step '{other}' is not defined for NumericUpDown")),
    }
    Ok(())
}

fn apply_domain(d: &mut DomainUpDown, op: &str, arg: Option<&str>) -> Result<(), String> {
    match op {
        "items" => {
            for item in arg.unwrap_or("").split(',') {
                d.add(item);
            }
        }
        "sel" => d.set_selected_index(int(arg)?).map_err(|e| e.to_string())?,
        "sorted" => d.set_sorted(flag_of(arg)),
        "wrap" => d.set_wrap(flag_of(arg)),
        // `UpButton` walks toward the start of the list and `DownButton` toward
        // the end — the direction the toolkit uses, verified by the reference.
        "up" => d.select_previous(),
        "down" => d.select_next(),
        other => return Err(format!("step '{other}' is not defined for DomainUpDown")),
    }
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// Value rendering — must agree with the .NET probe's, digit for digit
// ─────────────────────────────────────────────────────────────────────────────

fn int(arg: Option<&str>) -> Result<i32, String> {
    let s = arg.ok_or_else(|| "step needs a value".to_string())?;
    s.parse::<i32>().map_err(|e| format!("bad integer '{s}': {e}"))
}

fn dec(arg: Option<&str>) -> Result<f64, String> {
    let s = arg.ok_or_else(|| "step needs a value".to_string())?;
    s.parse::<f64>().map_err(|e| format!("bad number '{s}': {e}"))
}

fn flag_of(arg: Option<&str>) -> bool {
    arg == Some("true")
}

fn flag(b: bool) -> String {
    if b { "true" } else { "false" }.to_string()
}

fn num_i32(v: i32) -> String {
    v.to_string()
}

/// The f64 is printed at full round-trip precision on purpose. WinForms stores
/// these as `System.Decimal`; the port stores `f64`, and where the two disagree
/// (0.1 + 0.1 + 0.1) the comparison must SHOW the disagreement rather than round
/// it away. `-0.0` is normalised to `0`, which is the one difference that is a
/// spelling and not a value.
fn num_f64(v: f64) -> String {
    if v == 0.0 {
        return "0".to_string();
    }
    format!("{v}")
}

// ─────────────────────────────────────────────────────────────────────────────
// JSON, laid out byte-for-byte like the .NET probe's
// ─────────────────────────────────────────────────────────────────────────────

/// Escapes to pure ASCII: fr-FR's group separator is U+202F, and a literal
/// narrow no-break space in a file read by three different tools is a bug
/// waiting to happen.
fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 || (c as u32) > 0x7E => {
                // Non-BMP characters would need a surrogate pair; the case file
                // has none, and a `?` here would be a silent lie, so it is left
                // as an explicit escape of the scalar value.
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn render_map(pairs: &[(String, String)], indent: usize) -> String {
    if pairs.is_empty() {
        return "{}".to_string();
    }
    let mut out = String::from("{\n");
    for (i, (k, v)) in pairs.iter().enumerate() {
        let _ = write!(out, "{:indent$}{}: {}", "", quote(k), quote(v), indent = indent + 2);
        out.push_str(if i + 1 == pairs.len() { "\n" } else { ",\n" });
    }
    let _ = write!(out, "{:indent$}}}", "", indent = indent);
    out
}

fn render(header: &[(&str, &str)], cases: &[CaseResult]) -> String {
    let mut out = String::from("{\n");
    for (k, v) in header {
        let _ = writeln!(out, "  {}: {},", quote(k), quote(v));
    }
    out.push_str("  \"cases\": [\n");
    for (i, c) in cases.iter().enumerate() {
        out.push_str("    {\n");
        let _ = writeln!(out, "      \"id\": {},", quote(&c.id));
        let _ = writeln!(out, "      \"kind\": {},", quote(&c.kind));
        out.push_str("      \"steps\": [");
        for (s, st) in c.steps.iter().enumerate() {
            out.push_str(if s == 0 { "\n" } else { ",\n" });
            let _ = write!(
                out,
                "        {{\"op\": {}, \"status\": {}, \"detail\": {}}}",
                quote(&st.op),
                quote(st.status),
                quote(&st.detail)
            );
        }
        out.push_str(if c.steps.is_empty() { "],\n" } else { "\n      ],\n" });
        let _ = writeln!(out, "      \"state\": {},", render_map(&c.state, 6));
        let _ = writeln!(out, "      \"extra\": {}", render_map(&c.extra, 6));
        out.push_str("    }");
        out.push_str(if i + 1 == cases.len() { "\n" } else { ",\n" });
    }
    out.push_str("  ]\n}\n");
    out
}

// ─────────────────────────────────────────────────────────────────────────────

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // The parity directory is found from the crate, not from the working
    // directory, so `cargo run` behaves the same wherever it is invoked.
    let parity: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("tools")
        .join("winforms-ref")
        .join("parity");
    let cases_path = parity.join("range-cases.txt");
    let out_path = parity.join("range-port.json");

    let text = std::fs::read_to_string(&cases_path)
        .map_err(|e| format!("case file {}: {e}", cases_path.display()))?;
    let cases = load_cases(&text)?;

    let mut results = Vec::with_capacity(cases.len());
    for case in &cases {
        let mut port = Port::new(&case.kind)?;
        let mut steps = Vec::with_capacity(case.steps.len());
        for step in &case.steps {
            // A refused step is recorded and the case CONTINUES, so one
            // rejection does not hide the state that follows it.
            let (status, detail) = match port.apply(step) {
                Ok(()) => ("ok", String::new()),
                Err(e) => ("err", e),
            };
            steps.push(StepResult { op: step.clone(), status, detail });
        }
        results.push(CaseResult {
            id: case.id.clone(),
            kind: case.kind.clone(),
            steps,
            state: port.state(),
            extra: port.extra(),
        });
    }

    let fr = NumberFormat::FR;
    let header = [
        ("probe", "port"),
        ("culture", "fr-FR"),
        ("decimalSeparator", fr.decimal_separator),
        ("groupSeparator", fr.group_separator),
    ];
    std::fs::write(&out_path, render(&header, &results))?;
    println!("{} cases -> {}", results.len(), out_path.display());
    Ok(())
}
