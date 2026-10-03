//! What the storage components show in the designer before the project is built (lot ST-2): the declared defaults
//! of a `<Settings>`, read from the project's `.kbsettings` file beside the view (the typed class that registers them
//! at run time is not linked into the design surface yet).

use std::path::{Path, PathBuf};

use kubuno_app_storage::{AppId, SettingDef, SettingScope, SettingType, SettingValue, SettingsSchema};
use kubuno_resources_model::settings::SettingsFile;

const SKIPPED: &[&str] = &["target", "bin", "obj", ".git", ".vs", "node_modules"];

/// The package folder of `dir` (the nearest one with a `Cargo.toml`), or `dir` itself.
fn package_root(dir: &Path) -> PathBuf {
    dir.ancestors().find(|d| d.join("Cargo.toml").is_file()).unwrap_or(dir).to_path_buf()
}

fn find(dir: &Path, file_name: &str, depth: usize) -> Option<PathBuf> {
    if depth > 6 {
        return None;
    }
    let entries = std::fs::read_dir(dir).ok()?;
    let mut subdirs = Vec::new();
    for e in entries.flatten() {
        let p = e.path();
        let name = e.file_name().to_string_lossy().into_owned();
        match e.file_type() {
            Ok(t) if t.is_dir() => {
                if !SKIPPED.contains(&name.as_str()) && !p.join("Cargo.toml").is_file() {
                    subdirs.push(p);
                }
            }
            Ok(_) if name.eq_ignore_ascii_case(file_name) => return Some(p),
            _ => {}
        }
    }
    subdirs.sort();
    subdirs.iter().find_map(|d| find(d, file_name, depth + 1))
}

/// The schema of set `set` declared by the `.kbsettings` file of the package of `view_dir` (and its `App`).
pub(crate) fn schema_from_project(view_dir: &Path, set: &str) -> Option<(Option<AppId>, SettingsSchema)> {
    let root = package_root(view_dir);
    let path = find(&root, &format!("{set}.kbsettings"), 0)?;
    let text = std::fs::read_to_string(&path).ok()?;
    let (file, _) = SettingsFile::read(&text);
    Some((file.app.as_deref().and_then(|a| AppId::new(a).ok()), to_schema(set, &file)))
}

/// A `.kbsettings` file as the engine's schema (entries the engine refuses are left out).
pub(crate) fn to_schema(set: &str, file: &SettingsFile) -> SettingsSchema {
    let mut schema = SettingsSchema::new(set, file.version.max(1));
    schema.account_scoped = file.account_scoped;
    for e in &file.entries {
        let Some(ty) = SettingType::parse(&e.ty) else { continue };
        let default = if ty == SettingType::StringList { SettingValue::StringList(e.default_items()) } else { SettingValue::parse(&e.default, ty).unwrap_or_else(|| ty.zero()) };
        let def = SettingDef {
            name: e.name.clone(),
            ty,
            scope: if e.is_application() { SettingScope::Application } else { SettingScope::User },
            roaming: e.roaming,
            default,
            description: e.description.clone(),
            previous_names: e.previous_names.clone(),
            values: e.values.clone(),
        };
        if def.accept(&def.default).is_ok() {
            schema.defs.push(def);
        }
    }
    schema
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_project_file_is_found_from_the_view_folder() {
        let dir = std::env::temp_dir().join(format!("kubuno-design-schema-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src").join("views")).expect("mkdir");
        std::fs::write(dir.join("Cargo.toml"), "[package]\nname = \"x\"\n").expect("manifest");
        std::fs::write(
            dir.join("src").join("settings.kbsettings"),
            "<Settings Version=\"3\" App=\"design-app\"><Setting Name=\"Zoom\" Type=\"Int\" Default=\"125\"/><Setting Name=\"Recent\" Type=\"StringList\"><Item>a</Item></Setting></Settings>",
        )
        .expect("settings");
        let (app, schema) = schema_from_project(&dir.join("src").join("views"), "settings").expect("found");
        assert_eq!(app.map(|a| a.to_string()).as_deref(), Some("design-app"));
        assert_eq!(schema.version, 3);
        assert_eq!(schema.find("Zoom").map(|d| d.default.clone()), Some(SettingValue::Int(125)));
        assert_eq!(schema.find("Recent").map(|d| d.default.clone()), Some(SettingValue::StringList(vec!["a".into()])));
        assert!(schema_from_project(&dir.join("src"), "other").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
