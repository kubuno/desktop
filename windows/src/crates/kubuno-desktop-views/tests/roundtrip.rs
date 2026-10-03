//! Round-trip tests over a small corpus (`XML_VIEWS.md` §7's worked example
//! included) plus a lightweight, deterministic "property test": many
//! pseudo-random byte-level mutations of a handful of seeds, all checked
//! against the same invariant — `parse(text).text() == text` — the property
//! phase 2a's brief asks be pinned down for "any input, including malformed
//! input". No `proptest`/`quickcheck` dependency: a hand-rolled xorshift is
//! enough for a deterministic, reproducible spread of inputs without adding
//! to what this crate pulls in (`XML_VIEWS.md` §4's "does not pull `syn`/
//! `quote`" spirit applied to dev-dependencies too).

use kubuno_desktop_views::syntax::parse;

fn assert_round_trips(text: &str) {
    let p = parse(text);
    assert_eq!(p.text(), text, "round-trip failed for {text:?}");
}

// ── the corpus ──────────────────────────────────────────────────────────

#[test]
fn corpus_settings_view_worked_example_round_trips() {
    // `XML_VIEWS.md` §7, verbatim.
    assert_round_trips(include_str!("corpus/settings_view.kbview"));
}

#[test]
fn corpus_minimal_with_prolog_round_trips() {
    assert_round_trips(include_str!("corpus/minimal.kbview"));
}

#[test]
fn corpus_comments_and_cdata_round_trips() {
    assert_round_trips(include_str!("corpus/comments_and_cdata.kbview"));
}

#[test]
fn corpus_malformed_unterminated_round_trips_and_reports_errors() {
    let src = include_str!("corpus/malformed_unterminated.kbview");
    let p = parse(src);
    assert_eq!(p.text(), src);
    assert!(!p.diagnostics.is_empty(), "expected diagnostics for deliberately malformed input");
    // Every diagnostic has a real 1-based position.
    for d in &p.diagnostics {
        assert!(d.line >= 1);
        assert!(d.column >= 1);
    }
}

#[test]
fn corpus_malformed_mismatched_and_garbage_round_trips_and_reports_errors() {
    let src = include_str!("corpus/malformed_mismatched_and_garbage.kbview");
    let p = parse(src);
    assert_eq!(p.text(), src);
    assert!(!p.diagnostics.is_empty());
}

// ── property test: many pseudo-random mutations, one invariant ────────────

/// A tiny xorshift64 PRNG — deterministic, no dependency, good enough to
/// spread mutation points across a seed string reproducibly.
struct Xorshift64(u64);

impl Xorshift64 {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    fn range(&mut self, bound: usize) -> usize {
        if bound == 0 {
            0
        } else {
            (self.next() % bound as u64) as usize
        }
    }
}

/// Applies one small, byte-level mutation (insert/delete/replace a byte) at
/// a pseudo-random position — including positions that land mid-tag,
/// mid-string, mid-comment. The point is not to produce *valid* XML; it is
/// to throw byte soup at the lexer/parser and check the tree still accounts
/// for every byte.
fn mutate(rng: &mut Xorshift64, input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = bytes.to_vec();
    let punctuation: &[u8] = b"<>/=\"'!?& \n\t-[]C";
    match rng.range(3) {
        0 if !out.is_empty() => {
            // Delete one byte.
            let i = rng.range(out.len());
            out.remove(i);
        }
        1 => {
            // Insert one byte.
            let i = rng.range(out.len() + 1);
            let b = punctuation[rng.range(punctuation.len())];
            out.insert(i, b);
        }
        _ if !out.is_empty() => {
            // Replace one byte.
            let i = rng.range(out.len());
            out[i] = punctuation[rng.range(punctuation.len())];
        }
        _ => {}
    }
    // Mutating arbitrary bytes of valid UTF-8 (the seeds are ASCII-only in
    // their structural parts, but not in `Text="hôte:port"`-style content)
    // can produce invalid UTF-8; fall back to a lossy re-encoding rather than
    // panic — the round-trip property only needs to hold for what the lexer
    // actually receives as `&str`.
    String::from_utf8(out).unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned())
}

#[test]
fn round_trip_holds_under_many_pseudo_random_mutations() {
    let seeds = [
        include_str!("corpus/settings_view.kbview"),
        include_str!("corpus/minimal.kbview"),
        include_str!("corpus/comments_and_cdata.kbview"),
        include_str!("corpus/malformed_unterminated.kbview"),
        include_str!("corpus/malformed_mismatched_and_garbage.kbview"),
        r#"<Button Text="Ok"/>"#,
        "",
        "<",
        "</",
        "<!--",
        "<![CDATA[",
        "<?xml",
    ];

    let mut rng = Xorshift64(0x9E3779B97F4A7C15); // Fixed seed: reproducible failures.
    for seed in seeds {
        let mut current = seed.to_string();
        // Each seed is mutated repeatedly, compounding small changes —
        // checked after every single mutation, so a failure pinpoints
        // exactly which one-byte edit broke the invariant.
        for _ in 0..40 {
            current = mutate(&mut rng, &current);
            assert_round_trips(&current);
        }
    }
}

#[test]
fn round_trip_holds_for_every_single_prefix_of_the_worked_example() {
    // A different, exhaustive-rather-than-random way to hit "cut off in the
    // middle of a tag/string/comment": every truncation of the corpus's
    // richest file is its own round-trip case.
    let src = include_str!("corpus/settings_view.kbview");
    for (i, _) in src.char_indices() {
        assert_round_trips(&src[..i]);
    }
    assert_round_trips(src);
}
