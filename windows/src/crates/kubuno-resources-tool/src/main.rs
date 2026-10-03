//! `kubuno-resources-tool` — command-line helper for `.kbres` resource files
//! (`vskubuno/docs/RESOURCES.md`).
//!
//! ```text
//! kubuno-resources-tool import-locales <locales-dir> <out-dir> [--name Strings] [--neutral en-US]
//!     One folder per culture holding a Resources.resw/.resx (drive-localization's layout):
//!     <out-dir>/Strings.kbres (the neutral culture) and Strings.<culture>.kbres for the others.
//! kubuno-resources-tool import-resx <file.resx|.resw> <out.kbres>
//! kubuno-resources-tool check <file.kbres>
//!     Errors, warnings and missing translations of the set the file belongs to.
//! ```

use kubuno_resources_model::{culture, import, set, ResourceFile, Severity};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("import-locales") => import_locales(&args[1..]),
        Some("import-resx") => import_resx(&args[1..]),
        Some("check") => check(&args[1..]),
        _ => Err(USAGE.to_string()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

const USAGE: &str = "usage:\n  kubuno-resources-tool import-locales <locales-dir> <out-dir> [--name Strings] [--neutral en-US]\n  kubuno-resources-tool import-resx <file.resx> <out.kbres>\n  kubuno-resources-tool check <file.kbres>";

fn option(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
}

/// The `.resw`/`.resx` file of a culture folder.
fn resource_file_in(dir: &Path) -> Option<PathBuf> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("resw") || x.eq_ignore_ascii_case("resx")))
        .collect();
    found.sort();
    found.into_iter().next()
}

fn import_locales(args: &[String]) -> Result<(), String> {
    let (Some(src), Some(out)) = (args.first(), args.get(1)) else { return Err(USAGE.to_string()) };
    let name = option(args, "--name").unwrap_or_else(|| "Strings".to_string());
    let neutral_culture = culture::canonical(&option(args, "--neutral").unwrap_or_else(|| "en-US".to_string()));
    let mut cultures: Vec<(String, PathBuf)> = std::fs::read_dir(src)
        .map_err(|e| format!("{src}: {e}"))?
        .flatten()
        .filter(|e| e.path().is_dir())
        .filter_map(|e| {
            let c = e.file_name().to_string_lossy().into_owned();
            culture::is_culture(&c.replace('_', "-")).then(|| resource_file_in(&e.path()).map(|f| (culture::canonical(&c), f))).flatten()
        })
        .collect();
    cultures.sort();
    let Some((_, neutral_path)) = cultures.iter().find(|(c, _)| *c == neutral_culture) else {
        return Err(format!("no `{neutral_culture}` folder in {src} (choose the neutral culture with --neutral)"));
    };
    std::fs::create_dir_all(out).map_err(|e| format!("{out}: {e}"))?;
    let read = |p: &Path| -> Result<import::Imported, String> { import::import_resx(&std::fs::read_to_string(p).map_err(|e| format!("{}: {e}", p.display()))?).map_err(|e| format!("{}: {e}", p.display())) };
    let neutral = read(neutral_path)?;
    let mut neutral_file = neutral.file;
    neutral_file.culture = None;
    let target = Path::new(out).join(format!("{name}.kbres"));
    std::fs::write(&target, neutral_file.to_text()).map_err(|e| format!("{}: {e}", target.display()))?;
    println!("{} ({} strings, neutral = {neutral_culture})", target.display(), neutral_file.entries.len());
    for s in &neutral.skipped {
        println!("  skipped {s}");
    }
    for (c, path) in &cultures {
        if *c == neutral_culture {
            continue;
        }
        let imported = read(path)?;
        // Only the keys of the neutral file (a satellite never adds keys).
        let mut file = ResourceFile { entries: imported.file.entries.into_iter().filter(|e| neutral_file.get(&e.name).is_some()).collect(), culture: None };
        file.entries.sort_by_key(|e| neutral_file.entries.iter().position(|n| n.name == e.name));
        let target = Path::new(out).join(format!("{name}.{c}.kbres"));
        std::fs::write(&target, file.to_text()).map_err(|e| format!("{}: {e}", target.display()))?;
        let missing = set::missing_translations(&neutral_file, &file).len();
        println!("{} ({} strings, {missing} missing)", target.display(), file.entries.len());
    }
    Ok(())
}

fn import_resx(args: &[String]) -> Result<(), String> {
    let (Some(src), Some(out)) = (args.first(), args.get(1)) else { return Err(USAGE.to_string()) };
    let text = std::fs::read_to_string(src).map_err(|e| format!("{src}: {e}"))?;
    let imported = import::import_resx(&text).map_err(|e| format!("{src}: {e}"))?;
    std::fs::write(out, imported.file.to_text()).map_err(|e| format!("{out}: {e}"))?;
    println!("{out} ({} strings)", imported.file.entries.len());
    for s in &imported.skipped {
        println!("  skipped {s}");
    }
    Ok(())
}

fn check(args: &[String]) -> Result<(), String> {
    let Some(path) = args.first() else { return Err(USAGE.to_string()) };
    let files = set::discover(Path::new(path)).ok_or_else(|| format!("{path}: not a .kbres file"))?;
    let mut errors = 0;
    let neutral_text = std::fs::read_to_string(&files.neutral).map_err(|e| format!("{}: {e}", files.neutral.display()))?;
    let (neutral, diags) = ResourceFile::read(&neutral_text);
    for d in diags {
        errors += usize::from(d.severity == Severity::Error);
        println!("{}: {:?}: {}", files.neutral.display(), d.severity, d.message);
    }
    for s in &files.satellites {
        let text = std::fs::read_to_string(&s.path).map_err(|e| format!("{}: {e}", s.path.display()))?;
        let (file, diags) = ResourceFile::read(&text);
        for d in diags {
            errors += usize::from(d.severity == Severity::Error);
            println!("{}: {:?}: {}", s.path.display(), d.severity, d.message);
        }
        for d in set::check_satellite(&neutral, &s.culture, &file) {
            errors += usize::from(d.diagnostic.severity == Severity::Error);
            println!("{}: {:?}: {}", s.path.display(), d.diagnostic.severity, d.diagnostic.message);
        }
        let missing = set::missing_translations(&neutral, &file);
        if !missing.is_empty() {
            println!("{}: {} missing translation(s): {}", s.path.display(), missing.len(), missing.iter().take(10).copied().collect::<Vec<_>>().join(", "));
        }
    }
    println!("{} entries, {} culture(s), {errors} error(s)", neutral.entries.len(), files.satellites.len());
    if errors > 0 {
        Err(format!("{errors} error(s)"))
    } else {
        Ok(())
    }
}
