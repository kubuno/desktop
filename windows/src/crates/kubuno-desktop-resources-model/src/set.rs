//! A resource set: a neutral file (`resources.kbres`) and its satellites (`resources.fr.kbres`,
//! `resources.de-DE.kbres`) in the same folder — what one `resources!` call embeds, and what the
//! designer and the language server load. Also the cross-file checks (missing translations,
//! satellite entries the neutral file does not have, kind mismatches).

use crate::culture;
use crate::format::{Diagnostic, Kind, ResourceFile, Severity};
use std::path::{Path, PathBuf};

/// A satellite file of a set.
#[derive(Debug, Clone)]
pub struct Satellite {
    pub culture: String,
    pub path: PathBuf,
}

/// The files of a set on disk.
#[derive(Debug, Clone)]
pub struct SetFiles {
    /// The set's name: the neutral file's stem (`resources`).
    pub name: String,
    pub neutral: PathBuf,
    /// Sorted by culture name.
    pub satellites: Vec<Satellite>,
}

/// The set a `.kbres` file belongs to — `path` may be the neutral file or one of its satellites.
/// `None` when the name is not a `.kbres` name.
pub fn discover(path: &Path) -> Option<SetFiles> {
    let file_name = path.file_name()?.to_str()?;
    let (stem, _) = culture::split_file_name(file_name)?;
    let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
    let neutral = dir.join(format!("{stem}.kbres"));
    let mut satellites = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let Some(name) = entry.file_name().to_str().map(str::to_string) else { continue };
            if let Some((s, Some(c))) = culture::split_file_name(&name) {
                if s == stem {
                    satellites.push(Satellite { culture: c, path: entry.path() });
                }
            }
        }
    }
    satellites.sort_by(|a, b| a.culture.cmp(&b.culture));
    Some(SetFiles { name: stem, neutral, satellites })
}

/// A finding of [`check_satellite`], tied to a file.
#[derive(Debug, Clone, PartialEq)]
pub struct SetDiagnostic {
    /// The culture of the file the diagnostic is about (`None`: the neutral file).
    pub culture: Option<String>,
    /// The entry it is about.
    pub name: String,
    pub diagnostic: Diagnostic,
}

/// Checks a satellite against its neutral file: entries the neutral file lacks (warnings, on the
/// satellite: they are never used) and entries of another kind (errors, on the satellite).
pub fn check_satellite(neutral: &ResourceFile, culture: &str, satellite: &ResourceFile) -> Vec<SetDiagnostic> {
    let mut out = Vec::new();
    for e in &satellite.entries {
        match neutral.get(&e.name) {
            // A plural form the neutral language does not have (`items_few` in Russian) is used.
            None if e.kind == Kind::String && neutral.knows_plural_form(&e.name) => {}
            None => out.push(SetDiagnostic {
                culture: Some(culture.to_string()),
                name: e.name.clone(),
                diagnostic: Diagnostic { severity: Severity::Warning, message: format!("`{}` is not in the neutral file: this translation is never used", e.name), range: e.name_range.clone() },
            }),
            Some(n) if n.kind != e.kind => out.push(SetDiagnostic {
                culture: Some(culture.to_string()),
                name: e.name.clone(),
                diagnostic: Diagnostic {
                    severity: Severity::Error,
                    message: format!("`{}` is {} {} in the neutral file, {} {} here", e.name, article(n.kind), n.kind.element(), article(e.kind), e.kind.element()),
                    range: e.name_range.clone(),
                },
            }),
            Some(_) => {}
        }
    }
    out
}

/// The `String` entries of `neutral` that `satellite` does not translate.
pub fn missing_translations<'a>(neutral: &'a ResourceFile, satellite: &ResourceFile) -> Vec<&'a str> {
    neutral.entries.iter().filter(|e| e.kind == Kind::String && satellite.get(&e.name).is_none()).map(|e| e.name.as_str()).collect()
}

fn article(kind: Kind) -> &'static str {
    if matches!(kind, Kind::Image | Kind::Icon | Kind::Audio) {
        "an"
    } else {
        "a"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checks_satellites() {
        let neutral = ResourceFile::parse(r#"<Resources><String Name="a">A</String><String Name="b">B</String><Image Name="logo" File="l.png"/></Resources>"#).unwrap();
        let fr = ResourceFile::parse(r#"<Resources><String Name="a">À</String><String Name="logo">x</String><String Name="extra">x</String></Resources>"#).unwrap();
        let d = check_satellite(&neutral, "fr", &fr);
        assert_eq!(d.len(), 2);
        assert!(d.iter().any(|x| x.name == "logo" && x.diagnostic.severity == Severity::Error && x.diagnostic.message.contains("an Image")));
        assert!(d.iter().any(|x| x.name == "extra" && x.diagnostic.severity == Severity::Warning));
        assert_eq!(missing_translations(&neutral, &fr), vec!["b"]);
    }

    #[test]
    fn discovers_satellites_on_disk() {
        let dir = std::env::temp_dir().join(format!("kbres-discover-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for f in ["resources.kbres", "resources.fr.kbres", "resources.de-DE.kbres", "other.kbres", "other.fr.kbres", "resources.notes.kbres"] {
            std::fs::write(dir.join(f), "<Resources/>").unwrap();
        }
        let set = discover(&dir.join("resources.fr.kbres")).unwrap();
        assert_eq!(set.name, "resources");
        assert_eq!(set.neutral, dir.join("resources.kbres"));
        assert_eq!(set.satellites.iter().map(|s| s.culture.as_str()).collect::<Vec<_>>(), vec!["de-DE", "fr"]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
