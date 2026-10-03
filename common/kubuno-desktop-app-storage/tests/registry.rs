//! The Windows Registry for real, below a throw-away key `HKCU\Software\Kubuno\Tests\<id>` deleted at the end
//! (even when an assertion fails): every hive the tests name is mapped below it by `RegistryRoot::under`, so
//! nothing here can reach the real `Run` key, `HKLM` or another app's settings.
#![cfg(windows)]

use kubuno_desktop_app_storage::backend::{RegistryBackend, SettingsBackend};
use kubuno_desktop_app_storage::registry::{Access, Hive, RegValue, RegistryRoot, RegistryView};
use kubuno_desktop_app_storage::{AppId, Layer, SettingDef, SettingValue, Settings, SettingsOptions, SettingsSchema, StorageError};

/// The test key, deleted when dropped.
struct TestKey(RegistryRoot, String);

impl TestKey {
    fn new(tag: &str) -> Self {
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
        let base = format!(r"Software\Kubuno\Tests\{tag}-{}-{nanos:x}", std::process::id());
        Self(RegistryRoot::under(&base).expect("root"), base)
    }
}

impl Drop for TestKey {
    fn drop(&mut self) {
        let _ = self.0.delete_redirect();
    }
}

/// Whether the test key exists (read-only look at `HKCU`, the only use of the unredirected root here).
fn exists(base: &str) -> bool {
    RegistryRoot::current().open(Hive::CurrentUser, base, RegistryView::Default, Access::Read).ok().flatten().is_some()
}

#[test]
fn keys_values_views_and_cleanup() {
    let base;
    {
        let t = TestKey::new("keys");
        base = t.1.clone();
        let root = &t.0;
        assert!(root.open(Hive::LocalMachine, r"SOFTWARE\Contoso", RegistryView::Registry64, Access::Read).expect("open").is_none());
        let key = root.create(Hive::LocalMachine, r"SOFTWARE\Contoso\App", RegistryView::Registry64).expect("create");
        assert!(key.name().starts_with(r"HKCU\Software\Kubuno\Tests\"), "{}", key.name());
        assert!(key.name().ends_with(r"\HKLM\SOFTWARE\Contoso\App"), "{}", key.name());
        let values = [
            ("Name", RegValue::String("Kubuno é".into())),
            ("Path", RegValue::ExpandString(r"%LOCALAPPDATA%\Kubuno".into())),
            ("List", RegValue::MultiString(vec!["a".into(), "b".into()])),
            ("Small", RegValue::DWord(42)),
            ("Big", RegValue::QWord(1 << 40)),
            ("Blob", RegValue::Binary(vec![1, 2, 3])),
            ("", RegValue::String("default value".into())),
        ];
        for (n, v) in &values {
            key.set_value(n, v).expect("set");
        }
        for (n, v) in &values {
            assert_eq!(key.get_value(n).expect("get").as_ref(), Some(v), "{n}");
        }
        let mut names = key.value_names().expect("names");
        names.sort();
        assert_eq!(names, vec!["", "Big", "Blob", "List", "Name", "Path", "Small"]);
        key.create_subkey(r"Child\Grand").expect("sub");
        assert_eq!(key.subkey_names().expect("subkeys"), vec!["Child"]);
        assert!(key.delete_value("Small").expect("delete"));
        assert!(!key.delete_value("Small").expect("again"));
        assert!(key.get_value("Small").expect("get").is_none());
        let ro = root.open(Hive::LocalMachine, r"SOFTWARE\Contoso\App", RegistryView::Registry64, Access::Read).expect("open").expect("exists");
        assert!(matches!(ro.set_value("X", &RegValue::DWord(1)), Err(StorageError::ReadOnly(_))));
        assert!(root.delete_tree(Hive::LocalMachine, r"SOFTWARE\Contoso", RegistryView::Registry64).expect("delete tree"));
        assert!(root.open(Hive::LocalMachine, r"SOFTWARE\Contoso", RegistryView::Registry64, Access::Read).expect("open").is_none());
        assert!(exists(&base), "the test key exists while the test runs");
    }
    assert!(!exists(&base), "the test key is deleted afterwards");
}

#[test]
fn settings_on_the_registry_backend() {
    let t = TestKey::new("settings");
    let app = AppId::new("registry-test").expect("id");
    let schema = || {
        SettingsSchema::new("settings", 1)
            .with(SettingDef::new("Dark", false))
            .with(SettingDef::new("Interval", 5i64))
            .with(SettingDef::new("Zoom", 1.0f64))
            .with(SettingDef::new("Name", "x"))
            .with(SettingDef::new("Recent", Vec::<String>::new()).local())
            .with(SettingDef::new("Channel", "stable").application())
    };
    let backend = || Box::new(RegistryBackend::with_root(&app, "settings", t.0.clone())) as Box<dyn SettingsBackend>;
    // An administrator's machine value (a test may write the redirected HKLM).
    let admin = Settings::with_backend(&app, schema(), backend(), SettingsOptions { allow_machine_writes: true, ..Default::default() }).expect("admin");
    admin.set("Channel", "beta").expect("machine write");
    admin.save().expect("save");

    let a = Settings::with_backend(&app, schema(), backend(), SettingsOptions::default()).expect("a");
    let b = Settings::with_backend(&app, schema(), backend(), SettingsOptions::default()).expect("b");
    assert_eq!(a.get_as::<String>("Channel").as_deref(), Some("beta"));
    a.set("Dark", true).expect("set");
    a.set("Interval", 30i64).expect("set");
    a.set("Zoom", 1.25f64).expect("set");
    a.set("Name", "Été").expect("set");
    a.set("Recent", vec!["one".to_string(), "two".to_string()]).expect("set");
    assert!(a.set("Recent", vec![String::new()]).is_ok(), "accepted in memory");
    assert!(a.save().is_err(), "but the Registry cannot store an empty list item");
    a.set("Recent", vec!["one".to_string(), "two".to_string()]).expect("set");
    a.save().expect("save");

    // `b` is another process: it notices the change through the key's last write time.
    std::thread::sleep(std::time::Duration::from_millis(20));
    assert!(b.refresh_if_changed());
    assert_eq!(b.get_as::<bool>("Dark"), Some(true));
    assert_eq!(b.get_as::<i64>("Interval"), Some(30));
    assert_eq!(b.get_as::<f64>("Zoom"), Some(1.25));
    assert_eq!(b.get_as::<String>("Name").as_deref(), Some("Été"));
    assert_eq!(b.get_as::<Vec<String>>("Recent"), Some(vec!["one".to_string(), "two".to_string()]));

    // The raw layout: the types and the local key below Local Settings.
    let reg = RegistryBackend::with_root(&app, "settings", t.0.clone());
    let (h, p, view) = reg.key(Layer::UserRoaming);
    let k = t.0.open(h, &p, view, Access::Read).expect("open").expect("exists");
    assert_eq!(k.get_value("Dark").expect("get"), Some(RegValue::DWord(1)));
    assert_eq!(k.get_value("Interval").expect("get"), Some(RegValue::QWord(30)));
    assert_eq!(k.get_value("Zoom").expect("get"), Some(RegValue::String("1.25".into())));
    assert_eq!(k.get_value("$version").expect("get"), Some(RegValue::DWord(1)));
    assert!(reg.location(Layer::UserLocal).contains(r"\HKCU\Software\Classes\Local Settings\Software\Kubuno\Apps\registry-test\settings"), "{}", reg.location(Layer::UserLocal));
    assert!(reg.location(Layer::Machine).contains(r"\HKLM\Software\Kubuno\Apps\registry-test\settings"));
    let local = reg.read(Layer::UserLocal).expect("read");
    assert_eq!(local.values.get("Recent"), Some(&SettingValue::StringList(vec!["one".into(), "two".into()])));
}
