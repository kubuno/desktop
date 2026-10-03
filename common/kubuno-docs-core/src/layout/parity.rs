//! The calibration harness: does this engine break lines like the browser?
//!
//! It compares **line tops and line contents** of real paragraphs against numbers recorded from
//! the real `canvas-engine.ts` running in Chrome (`C:\kubuno-build\record-line-parity.mjs`, outside
//! the repository: it drives a browser). The recording carries the case definitions — the same
//! ProseMirror document and content width the browser was given — so the two fixture lists cannot
//! drift apart. A font-metric comparison would miss the errors that matter (a missing ×1.15 line
//! spacing, a tokenizer difference moving a word).
//!
//! The comparison needs the platform's fonts, so the test that runs it lives with the platform's
//! measurer (`documents/src/doc/fonts.rs` on Windows); this module holds the recording, the case
//! conversion and the comparator, all platform-neutral.
//!
//! (Moved from `documents/src/doc/parity.rs`, rebased on the web's span model; its first result,
//! 2026-09-19: all nine recorded paragraphs agree, line ends within 0.28 px.)

use std::path::PathBuf;

use serde::Deserialize;
use serde_json::value::RawValue;

use super::paragraph::is_js_space;
use super::{DocPx, LayoutLine, RenderParagraph};

/// A recorded case: what the browser produced for one paragraph.
#[derive(Clone, Debug)]
pub struct Recorded {
    pub name: String,
    pub lines: Vec<String>,
    /// Line tops relative to the paragraph's first line.
    pub tops: Vec<DocPx>,
    pub heights: Vec<DocPx>,
    /// The x of each line's first span (the alignment offset).
    pub firsts: Vec<DocPx>,
    /// The right edge of each line's last visible token.
    pub ends: Vec<DocPx>,
    pub width: DocPx,
    pub doc: String,
}

/// Tolerance on a line top or height.
pub const TOLERANCE: DocPx = 0.5;
/// Tolerance on a horizontal position (measured: 0.276 px worst case at the end of a full line).
pub const X_TOLERANCE: DocPx = 0.5;

/// `KUBUNO_LINE_PARITY`, or `tests/fixtures/parity/line-parity.json`.
pub fn recording_path() -> Option<PathBuf> {
    if let Some(from_env) = std::env::var_os("KUBUNO_LINE_PARITY") {
        let path = PathBuf::from(from_env);
        return path.is_file().then_some(path);
    }
    let in_repo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures").join("parity").join("line-parity.json");
    in_repo.is_file().then_some(in_repo)
}

/// The recorded cases (empty without a recording).
pub fn recorded() -> Vec<Recorded> {
    let Some(path) = recording_path() else { return Vec::new() };
    let Ok(bytes) = std::fs::read(&path) else { return Vec::new() };
    match serde_json::from_slice::<Recording>(&bytes) {
        Ok(file) => file.cases.iter().map(Recorded::from).collect(),
        Err(why) => {
            eprintln!("parity: {} does not parse: {why}", path.display());
            Vec::new()
        }
    }
}

#[derive(Deserialize)]
struct Recording {
    cases: Vec<RecCase>,
}

#[derive(Deserialize)]
struct RecCase {
    name: String,
    width: f32,
    doc: Box<RawValue>,
    lines: Vec<RecLine>,
}

#[derive(Deserialize)]
struct RecLine {
    text: String,
    top: f32,
    height: f32,
    spans: Vec<RecSpan>,
}

#[derive(Deserialize)]
struct RecSpan {
    text: String,
    x: f32,
    width: f32,
}

fn is_space_token(text: &str) -> bool {
    !text.is_empty() && text.chars().all(is_js_space)
}

impl From<&RecCase> for Recorded {
    fn from(case: &RecCase) -> Self {
        let end = |spans: &[RecSpan]| spans.iter().rposition(|s| !is_space_token(&s.text)).map(|i| spans[i].x + spans[i].width).unwrap_or(0.0);
        Self {
            name: case.name.clone(),
            lines: case.lines.iter().map(|l| l.text.clone()).collect(),
            tops: case.lines.iter().map(|l| l.top).collect(),
            heights: case.lines.iter().map(|l| l.height).collect(),
            firsts: case.lines.iter().map(|l| l.spans.first().map(|s| s.x).unwrap_or(0.0)).collect(),
            ends: case.lines.iter().map(|l| end(&l.spans)).collect(),
            width: case.width,
            doc: case.doc.get().to_string(),
        }
    }
}

/// The render paragraph of a case, through the real parse.
pub fn paragraph_of(rec: &Recorded) -> Result<RenderParagraph, String> {
    let node = crate::model::Node::from_slice(rec.doc.as_bytes()).map_err(|e| format!("the recorded document does not parse: {e}"))?;
    let paras = super::parse::parse_doc(&node);
    match paras.len() {
        1 => paras.into_iter().next().ok_or_else(|| "unreachable".to_string()),
        n => Err(format!("a case must be exactly one paragraph, this one is {n}")),
    }
}

fn text_of(line: &LayoutLine) -> String {
    line.spans.iter().filter(|s| !s.is_marker()).map(|s| s.text.as_str()).collect()
}

fn visible_end(line: &LayoutLine) -> Option<DocPx> {
    let spans: Vec<_> = line.spans.iter().filter(|s| !s.is_marker()).collect();
    let last = spans.iter().rposition(|s| !is_space_token(&s.text))?;
    Some(spans[last].x + spans[last].width)
}

/// Every way the laid-out lines differ from the recording.
pub fn compare(rec: &Recorded, got: &[LayoutLine]) -> Vec<String> {
    let mut out = Vec::new();
    if got.len() != rec.lines.len() {
        out.push(format!(
            "line count: browser {}, here {}\n    browser: {:?}\n    here:    {:?}",
            rec.lines.len(),
            got.len(),
            rec.lines,
            got.iter().map(text_of).collect::<Vec<_>>()
        ));
    }
    let y0 = got.first().map(|l| l.y).unwrap_or(0.0);
    for (i, (want, line)) in rec.lines.iter().zip(got.iter()).enumerate() {
        let have = text_of(line);
        if &have != want {
            out.push(format!("line {i} text:\n    browser: {want:?}\n    here:    {have:?}"));
        }
        let top = line.y - y0;
        if let Some(&t) = rec.tops.get(i) {
            if (top - t).abs() > TOLERANCE {
                out.push(format!("line {i} top: browser {t:.3}, here {top:.3}"));
            }
        }
        if let Some(&h) = rec.heights.get(i) {
            if (line.height - h).abs() > TOLERANCE {
                out.push(format!("line {i} height: browser {h:.3}, here {:.3}", line.height));
            }
        }
        if let (Some(&want_x), Some(first)) = (rec.firsts.get(i), line.spans.iter().find(|s| !s.is_marker())) {
            if (first.x - want_x).abs() > X_TOLERANCE {
                out.push(format!("line {i} starts at: browser {want_x:.3}, here {:.3}", first.x));
            }
        }
        if let (Some(&want_end), Some(end)) = (rec.ends.get(i), visible_end(line)) {
            if (end - want_end).abs() > X_TOLERANCE {
                out.push(format!("line {i} ends at: browser {want_end:.3}, here {end:.3}"));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::{LayoutSpan, SpanKind};
    use crate::marks::TextMark;

    fn line(text: &str, y: f32, h: f32) -> LayoutLine {
        LayoutLine {
            spans: vec![LayoutSpan { text: text.into(), marks: TextMark::default(), x: 0.0, width: 10.0, pm_pos: 1, kind: SpanKind::Text, pm_len: None }],
            y,
            height: h,
            ..Default::default()
        }
    }

    fn rec(texts: &[&str], tops: &[f32], heights: &[f32]) -> Recorded {
        Recorded {
            name: "t".into(),
            lines: texts.iter().map(|s| s.to_string()).collect(),
            tops: tops.to_vec(),
            heights: heights.to_vec(),
            firsts: vec![0.0; texts.len()],
            ends: vec![10.0; texts.len()],
            width: 600.0,
            doc: String::new(),
        }
    }

    #[test]
    fn a_naive_uniform_line_height_is_rejected() {
        // ×1.15 dropped: 17.6 instead of 20.24 per line.
        let r = rec(&["a", "b", "c"], &[0.0, 20.24, 40.48], &[20.24, 20.24, 20.24]);
        let got = vec![line("a", 0.0, 17.6), line("b", 17.6, 17.6), line("c", 35.2, 17.6)];
        assert!(!compare(&r, &got).is_empty());
    }

    #[test]
    fn a_line_within_the_tolerance_is_accepted_and_tops_are_relative() {
        let r = rec(&["a", "b"], &[0.0, 20.24], &[20.24, 20.24]);
        let got = vec![line("a", 100.0, 20.3), line("b", 120.4, 20.2)];
        assert!(compare(&r, &got).is_empty(), "{:?}", compare(&r, &got));
    }

    #[test]
    fn a_wrong_break_and_a_missing_line_are_reported() {
        let r = rec(&["ab", "c"], &[0.0, 20.0], &[20.0, 20.0]);
        let got = vec![line("a", 0.0, 20.0)];
        let out = compare(&r, &got);
        assert!(out.iter().any(|s| s.starts_with("line count")));
        assert!(out.iter().any(|s| s.starts_with("line 0 text")));
    }

    #[test]
    fn the_recording_loads_and_its_cases_parse_into_one_paragraph() {
        let cases = recorded();
        if cases.is_empty() {
            eprintln!("parity: no recording, skipped");
            return;
        }
        for c in &cases {
            assert!(paragraph_of(c).is_ok(), "{}: {:?}", c.name, paragraph_of(c).err());
        }
    }
}
