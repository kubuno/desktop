//! The round-trip suite, run over the fixture files.
//!
//! This lives inside the crate rather than in `tests/` because the app is a
//! single binary: an integration test can only link against a library target,
//! and adding one to expose the model would be a structural change made for the
//! test's convenience rather than the app's.
//!
//! # The two assertions
//!
//! They are different tests, and conflating them is how the strong one gets
//! quietly weakened into the weak one.
//!
//! * **A1 — byte-exact.** `read(bytes) → write() == bytes`, with no
//!   canonicalisation on either side. It only means anything for files that are
//!   already in a serialiser's own output form: compact, keys in byte order, no
//!   trailing newline. Those are the real stored documents.
//! * **A2 — semantic, plus preservation.** For hand-written fixtures and for the
//!   templates (which come from a Postgres `JSONB` column and are therefore not
//!   byte-sorted), parse both sides and compare as values. `serde_json::Value`
//!   equality distinguishes an absent key from a null one — they are different
//!   maps — so this still catches the presence bug A1 exists for, while
//!   tolerating the formatting those files cannot control.
//!
//! Which assertion applies is decided per directory, and within a directory by
//! the bytes themselves — a file with no newline in it is a serialiser's output
//! and gets A1 — rather than by a list that would drift out of date the first
//! time somebody adds a fixture.
//!
//! The templates are the exception that has to be named rather than detected:
//! they are compact AND single-line, so the newline rule would claim them for
//! A1, but they come from a Postgres `JSONB` column
//! (`office/migrations/000003_office_shares.up.sql:28-44`) whose keys are not
//! byte-sorted. They can never satisfy A1 and are declared semantic.

use std::fs;
use std::path::{Path, PathBuf};

use super::Document;

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures")
}

/// The real-document corpus, which lives outside the repository because it is
/// somebody's documents and this repository is public. See
/// `tests/fixtures/README.md`.
fn corpus_dir() -> Option<PathBuf> {
    let dir = PathBuf::from(std::env::var_os("KUBUNO_DOC_CORPUS")?);
    dir.is_dir().then_some(dir)
}

fn json_files(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else { return Vec::new() };
    let mut out: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .collect();
    out.sort();
    out
}

/// Which assertion a set of fixtures is held to.
#[derive(Clone, Copy, PartialEq)]
enum Mode {
    /// A1 for the compact files, A2 for the pretty ones.
    Auto,
    /// A2 for all of them, whatever their formatting.
    Semantic,
}

/// A file with no newline is a serialiser's own output, so it must survive
/// byte-for-byte. One with newlines was formatted by a human or by Postgres.
fn is_byte_exact_candidate(bytes: &[u8]) -> bool {
    !bytes.contains(&b'\n')
}

fn check(path: &Path, mode: Mode) -> Result<(), String> {
    let stored = fs::read(path).map_err(|e| format!("unreadable: {e}"))?;
    let doc = Document::from_slice(&stored).map_err(|e| format!("does not parse: {e}"))?;
    let written = doc.to_vec().map_err(|e| format!("does not serialise: {e}"))?;

    if mode == Mode::Auto && is_byte_exact_candidate(&stored) {
        if written != stored {
            // Report where they diverge rather than dumping two documents.
            let at = written
                .iter()
                .zip(stored.iter())
                .position(|(a, b)| a != b)
                .unwrap_or(stored.len().min(written.len()));
            let window = |b: &[u8]| {
                let from = at.saturating_sub(40);
                let to = (at + 40).min(b.len());
                String::from_utf8_lossy(&b[from..to]).into_owned()
            };
            return Err(format!(
                "A1 failed at byte {at} ({} stored, {} written)\n  stored:  …{}…\n  written: …{}…",
                stored.len(),
                written.len(),
                window(&stored),
                window(&written),
            ));
        }
        return Ok(());
    }

    let before: serde_json::Value =
        serde_json::from_slice(&stored).map_err(|e| format!("stored is not JSON: {e}"))?;
    let after: serde_json::Value =
        serde_json::from_slice(&written).map_err(|e| format!("written is not JSON: {e}"))?;
    if before != after {
        return Err("A2 failed: the document does not parse back to the same value".into());
    }
    Ok(())
}

/// Runs `check` over a directory and reports EVERY failure, not just the first.
/// One fixture failing tells you little; the pattern across them tells you what
/// is actually wrong.
fn check_all(dir: &Path, label: &str, mode: Mode) {
    let files = json_files(dir);
    assert!(!files.is_empty(), "{label}: no fixtures found in {}", dir.display());
    let mut failures = Vec::new();
    for file in &files {
        if let Err(why) = check(file, mode) {
            let name = file.file_name().unwrap_or(file.as_os_str()).to_string_lossy();
            failures.push(format!("  {name}\n    {why}"));
        }
    }
    assert!(
        failures.is_empty(),
        "{label}: {} of {} fixtures failed the round trip:\n{}",
        failures.len(),
        files.len(),
        failures.join("\n")
    );
}

#[test]
fn the_hand_written_edge_fixtures_round_trip() {
    check_all(&fixtures_dir().join("edge"), "edge", Mode::Auto);
}

#[test]
fn the_built_in_templates_round_trip() {
    // Semantic only: they are SQL literals from a JSONB column, so their keys
    // are not byte-sorted and no writer can reproduce their order.
    check_all(&fixtures_dir(), "templates", Mode::Semantic);
}

#[test]
fn the_real_document_corpus_round_trips_byte_for_byte() {
    let Some(dir) = corpus_dir() else {
        // Not a failure: the corpus is out of the repository on purpose, so a
        // clone without it still runs the synthetic suites above. Set
        // KUBUNO_DOC_CORPUS to run this one.
        eprintln!("skipped: KUBUNO_DOC_CORPUS is unset or not a directory");
        return;
    };
    check_all(&dir, "corpus", Mode::Auto);
}

#[test]
fn the_corpus_suite_is_actually_byte_exact_when_it_runs() {
    // A guard against the suite above quietly degrading into the weak
    // assertion: if every corpus file grew a newline, `check` would silently
    // switch all of them to A2 and keep passing.
    let Some(dir) = corpus_dir() else { return };
    let files = json_files(&dir);
    let compact = files.iter().filter(|p| fs::read(p).is_ok_and(|b| is_byte_exact_candidate(&b))).count();
    assert!(
        compact * 2 > files.len(),
        "only {compact} of {} corpus fixtures are compact — the byte-exact assertion has \
         stopped covering the corpus",
        files.len()
    );
}
