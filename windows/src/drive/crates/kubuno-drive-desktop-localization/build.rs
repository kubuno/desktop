//! Localization code generation (replaces `StringsPropertyGenerator`).
//!
//! Hybrid approach (volume: ~49 cultures × ~1450 keys):
//! - **en-US** (reference/fallback culture) is parsed at build time into a
//!   static array **sorted by key** (`EN_US`) → binary search lookup, zero
//!   cost at startup, and negligible compile time (a single culture
//!   hardcoded).
//! - The 49 cultures are embedded **raw** via `include_str!` in the
//!   `LOCALES` table and parsed lazily at runtime on the first
//!   `set_culture` (see `src/lib.rs`). Generating 49 static arrays
//!   (~70,000 literals) would blow up compile time for cultures never used
//!   in a given process.

use std::env;
use std::fmt::Write as _;
use std::fs;
use std::path::PathBuf;

// Reuses the runtime's .resw parser (self-contained module, no dependency).
include!("src/resw.rs");

fn main() {
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let locales_dir = manifest.join("locales");
    // Detects cultures being added/removed…
    println!("cargo:rerun-if-changed={}", locales_dir.display());

    let mut cultures: Vec<(String, PathBuf)> = fs::read_dir(&locales_dir)
        .expect("dossier locales/ introuvable")
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .filter_map(|e| {
            let resw = e.path().join("Resources.resw");
            resw.is_file()
                .then(|| (e.file_name().to_string_lossy().into_owned(), resw))
        })
        .collect();
    cultures.sort_by(|a, b| a.0.cmp(&b.0));

    let en_path = &cultures
        .iter()
        .find(|(c, _)| c == "en-US")
        .expect("locales/en-US/Resources.resw manquant (culture de référence)")
        .1;

    // en-US: sorted static table (binary search).
    let en_xml = fs::read_to_string(en_path).unwrap();
    let mut pairs = parse_resw(&en_xml);
    pairs.sort_by(|a, b| a.0.cmp(&b.0));
    pairs.dedup_by(|a, b| a.0 == b.0);
    assert!(!pairs.is_empty(), "aucune clé parsée depuis en-US");

    let mut out = String::new();
    writeln!(
        out,
        "/// Table en-US pré-parsée au build, triée par clé (recherche binaire)."
    )
    .unwrap();
    writeln!(out, "pub(crate) static EN_US: &[(&str, &str)] = &[").unwrap();
    for (k, v) in &pairs {
        writeln!(out, "    ({k:?}, {v:?}),").unwrap();
    }
    writeln!(out, "];").unwrap();

    writeln!(
        out,
        "/// (tag de culture, XML .resw brut) pour toutes les cultures embarquées, trié par tag."
    )
    .unwrap();
    writeln!(out, "pub(crate) static LOCALES: &[(&str, &str)] = &[").unwrap();
    for (culture, path) in &cultures {
        // …and each file's content changing.
        println!("cargo:rerun-if-changed={}", path.display());
        writeln!(
            out,
            "    ({culture:?}, include_str!({:?})),",
            path.display().to_string()
        )
        .unwrap();
    }
    writeln!(out, "];").unwrap();

    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap()).join("locales_gen.rs");
    fs::write(out_path, out).unwrap();
}
