//! `<RegistryKey>` and `<Settings Backend="Registry">` for real, in a sandboxed profile: every key they touch is
//! the sandbox's copy below `HKCU\Software\Kubuno\Sandbox\<tag>`, deleted at the end. One test in its own binary,
//! because `KUBUNO_SANDBOX_DIR` is process-wide.
#![cfg(windows)]

use kubuno_app_storage::paths;
use kubuno_app_storage::registry::{Access, Hive, RegValue, RegistryRoot, RegistryView};
use kubuno_app_storage_components::RegistryKey;
use kubuno_views::binding::{Value, ViewModel};

#[derive(Default)]
struct Vm;

impl ViewModel for Vm {
    fn get(&self, _path: &str) -> Option<Value> {
        None
    }
    fn set(&mut self, _path: &str, _value: Value) {}
}

/// Deletes the sandbox's Registry copy when dropped (also after a failed assertion).
struct Cleanup(String, std::path::PathBuf);

impl Drop for Cleanup {
    fn drop(&mut self) {
        if let Ok(root) = RegistryRoot::under(&self.0) {
            let _ = root.delete_redirect();
        }
        let _ = std::fs::remove_dir_all(&self.1);
    }
}

#[test]
fn registry_components_stay_inside_the_sandbox() {
    let dir = tempfile_dir();
    std::env::set_var(paths::SANDBOX_ENV, &dir);
    let tag = paths::sandbox_tag().expect("tag");
    let _cleanup = Cleanup(format!(r"{}\{tag}", RegistryRoot::SANDBOX_BASE), dir.clone());

    let view = r#"<Panel DesignWidth="400" DesignHeight="300">
        <RegistryKey x:Name="key" Hive="CurrentUser" Path="Software\Microsoft\Windows\CurrentVersion\Run" Writable="true" OnValueChanged="changed"/>
        <RegistryKey x:Name="ro" Hive="LocalMachine" Path="SOFTWARE\Contoso" View="Registry64"/>
        <Settings x:Name="settings" AppId="sandbox-registry" Schema="prefs" Backend="Registry"/>
    </Panel>"#;
    let mut rt = kubuno_views::runtime::Runtime::new();
    assert!(rt.reload_from_text(view), "{:?}", rt.diagnostics());
    let scope = rt.components();
    let mut vm = Vm;
    scope.sync_all(&mut vm, false);
    assert_eq!(scope.view_model(&mut vm, false).get("key"), Some(Value::Bool(false)), "the sandbox's Run key does not exist yet");

    // A two-way binding writes the sandbox's copy of the Run key, never the real one.
    scope.view_model(&mut vm, false).set("key.KubunoTest", Value::Str("\"C:\\x.exe\" --background".into()));
    let full = rt.with_component::<RegistryKey, _>("key", |k| k.full_name()).expect("component");
    assert_eq!(full, format!(r"HKCU\Software\Kubuno\Sandbox\{tag}\HKCU\Software\Microsoft\Windows\CurrentVersion\Run"));
    let real = RegistryRoot::under(&format!(r"Software\Kubuno\Sandbox\{tag}")).expect("root");
    let k = real.open(Hive::CurrentUser, r"Software\Microsoft\Windows\CurrentVersion\Run", RegistryView::Default, Access::Read).expect("open").expect("created");
    assert_eq!(k.get_value("KubunoTest").expect("get"), Some(RegValue::String("\"C:\\x.exe\" --background".into())));
    scope.sync_all(&mut vm, false);
    assert_eq!(scope.view_model(&mut vm, false).get("key.KubunoTest"), Some(Value::Str("\"C:\\x.exe\" --background".into())));

    // A read-only key refuses writes.
    scope.view_model(&mut vm, false).set("ro.X", Value::Str("y".into()));
    assert!(rt.with_component::<RegistryKey, _>("ro", |k| k.last_error().map(|e| e.contains("Writable"))).flatten().unwrap_or(false));

    // Settings on the Registry back-end land in the sandbox's copy too.
    scope.view_model(&mut vm, false).set("settings.Zoom", Value::F32(1.5));
    scope.sync_all(&mut vm, false);
    let prefs = real.open(Hive::CurrentUser, r"Software\Kubuno\Apps\sandbox-registry\prefs", RegistryView::Default, Access::Read).expect("open").expect("saved");
    assert_eq!(prefs.get_value("Zoom").expect("get"), Some(RegValue::String("1.5".into())));
}

fn tempfile_dir() -> std::path::PathBuf {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let d = std::env::temp_dir().join(format!("kubuno-sbx-{}-{nanos:x}", std::process::id()));
    let _ = std::fs::create_dir_all(&d);
    d
}
