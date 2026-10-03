//! In the designer, a `<Settings>` shows the defaults its project's `.kbsettings` declares, before the project is
//! built (the typed class is not linked into the design surface). Its own binary: design time is process-wide.

extern crate kubuno_desktop_app_storage_components as _;

use kubuno_desktop_views::binding::{Value, ViewModel};

#[derive(Default)]
struct Vm;

impl ViewModel for Vm {
    fn get(&self, _path: &str) -> Option<Value> {
        None
    }
    fn set(&mut self, _path: &str, _value: Value) {}
}

#[test]
fn the_designer_shows_the_declared_defaults() {
    let dir = std::env::temp_dir().join(format!("kubuno-design-defaults-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).expect("mkdir");
    std::fs::write(dir.join("Cargo.toml"), "[package]\nname = \"design-app\"\n").expect("manifest");
    std::fs::write(dir.join("src").join("prefs.kbsettings"), "<Settings><Setting Name=\"Theme\" Default=\"Dark\" Values=\"System|Dark\"/><Setting Name=\"Zoom\" Type=\"Int\" Default=\"125\"/></Settings>").expect("settings");
    kubuno_desktop_views::design::set_design_time(true);
    let mut rt = kubuno_desktop_views::runtime::Runtime::new();
    rt.set_base_dir(Some(dir.join("src")));
    assert!(rt.reload_from_text(r#"<Panel DesignWidth="400" DesignHeight="300"><Settings x:Name="settings" Schema="prefs"/></Panel>"#), "{:?}", rt.diagnostics());
    let scope = rt.components();
    let mut vm = Vm;
    // The design surface never syncs the providers: the declared defaults are read without it,
    let scoped = scope.view_model(&mut vm, false);
    assert_eq!(scoped.get("settings.Theme"), Some(Value::Str("Dark".into())));
    assert_eq!(scoped.get("settings.Zoom"), Some(Value::F32(125.0)));
    // and from the (memory) store once one is opened.
    scope.sync_all(&mut vm, false);
    let scoped = scope.view_model(&mut vm, false);
    assert_eq!(scoped.get("settings.Theme"), Some(Value::Str("Dark".into())));
    assert_eq!(scoped.get("settings.Zoom"), Some(Value::F32(125.0)));
    kubuno_desktop_views::design::set_design_time(false);
    let _ = std::fs::remove_dir_all(&dir);
}
