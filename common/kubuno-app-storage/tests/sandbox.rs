//! A sandboxed profile (`KUBUNO_SANDBOX_DIR`): every location, the Registry and the secret names move away from the
//! real profile. One test in its own binary, because the variable is process-wide.

use std::sync::{Arc, Mutex};

use kubuno_app_storage::{paths, AppId, AppSecrets, Layer, SettingDef, Settings, SettingsOptions, SettingsSchema};
use kubuno_secrets::{Secret, SecretError, SecretName, SecretStore};

/// Records the names it is asked for (never the values).
#[derive(Debug, Default)]
struct Recorder(Mutex<Vec<String>>);

impl SecretStore for Recorder {
    fn backend(&self) -> &'static str {
        "recorder"
    }
    fn get(&self, name: &SecretName) -> Result<Option<Secret>, SecretError> {
        self.0.lock().expect("lock").push(name.target());
        Ok(None)
    }
    fn set(&self, name: &SecretName, _value: &Secret) -> Result<(), SecretError> {
        self.0.lock().expect("lock").push(name.target());
        Ok(())
    }
    fn delete(&self, name: &SecretName) -> Result<bool, SecretError> {
        self.0.lock().expect("lock").push(name.target());
        Ok(false)
    }
}

#[test]
fn a_sandbox_moves_everything() {
    let dir = tempfile::tempdir().expect("tmp");
    // SAFETY (edition 2021: plain call): the only test of this binary, set before anything reads it.
    std::env::set_var(paths::SANDBOX_ENV, dir.path());
    let tag = paths::sandbox_tag().expect("tag");
    assert!(!paths::system_integration_allowed());
    for p in [paths::user_config_dir(), paths::user_data_dir(), paths::user_cache_dir(), paths::machine_config_dir()] {
        assert!(p.expect("dir").starts_with(dir.path()));
    }

    let app = AppId::new("sandbox-test").expect("id");
    let schema = SettingsSchema::new("settings", 1).with(SettingDef::new("Theme", "System")).with(SettingDef::new("Pane", 240i64).local());
    let s = Settings::open(&app, schema.clone(), SettingsOptions::default()).expect("open");
    s.set("Theme", "Dark").expect("set");
    s.set("Pane", 300i64).expect("set");
    s.save().expect("save");
    assert!(dir.path().join("config").join("sandbox-test").join("settings.settings.json").is_file());
    assert!(dir.path().join("data").join("sandbox-test").join("settings.local.settings.json").is_file());
    assert!(s.location(Layer::Machine).starts_with(&dir.path().display().to_string()));

    #[cfg(windows)]
    {
        use kubuno_app_storage::registry::{Access, Hive, RegistryRoot, RegistryView};
        let root = RegistryRoot::current();
        assert!(root.is_redirected());
        let base = format!(r"{}\{tag}", RegistryRoot::SANDBOX_BASE);
        assert_eq!(root.display(Hive::CurrentUser, r"Software\Microsoft\Windows\CurrentVersion\Run"), format!(r"HKCU\{base}\HKCU\Software\Microsoft\Windows\CurrentVersion\Run"));
        let r = Settings::open(&app, schema, SettingsOptions { backend: kubuno_app_storage::backend::BackendKind::Registry, ..Default::default() }).expect("registry");
        r.set("Theme", "Light").expect("set");
        r.save().expect("save");
        assert!(r.location(Layer::UserRoaming).starts_with(&format!(r"HKCU\{base}\HKCU\Software\Kubuno\Apps\sandbox-test")), "{}", r.location(Layer::UserRoaming));
        // Clean up the sandbox's own key (what deleting a sandbox directory leaves behind).
        let unredirected_base = RegistryRoot::under(&base).expect("base");
        assert!(unredirected_base.delete_redirect().expect("delete"));
        assert!(root.open(Hive::CurrentUser, "", RegistryView::Default, Access::Read).expect("open").is_none(), "the sandbox key is gone");
    }

    let recorder = Arc::new(Recorder::default());
    let secrets = AppSecrets::with_store(&app, recorder.clone()).expect("secrets");
    secrets.set_str("ApiKey", "value").expect("set");
    assert_eq!(*recorder.0.lock().expect("lock"), vec![format!("Kubuno/sandbox-{tag}.app.sandbox-test/ApiKey")]);
}
