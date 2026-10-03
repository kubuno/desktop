//! `kubuno::settings!`: the typed class of a `.kbsettings` file, in a sandboxed profile (`KUBUNO_SANDBOX_DIR`, set
//! before anything opens the settings: one test in its own binary). Nothing here reaches the user's profile.

kubuno::settings!(AppSettings, "fixtures/storage.kbsettings");

#[test]
fn the_typed_class_reads_writes_upgrades_and_registers() {
    let dir = std::env::temp_dir().join(format!("kubuno-settings-macro-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("sandbox");
    std::env::set_var("KUBUNO_SANDBOX_DIR", &dir);

    // Version 1 of the app stored `SyncInterval`; this version renamed it (PreviousNames).
    let roaming = dir.join("config").join("kubuno-facade-test").join("storage.settings.json");
    std::fs::create_dir_all(roaming.parent().expect("dir")).expect("mkdir");
    std::fs::write(&roaming, r#"{"$version": 1, "SyncInterval": 15}"#).expect("v1");
    // An administrator's machine-wide value of an application setting.
    let machine = dir.join("machine").join("kubuno-facade-test").join("storage.settings.json");
    std::fs::create_dir_all(machine.parent().expect("dir")).expect("mkdir");
    std::fs::write(&machine, r#"{"UpdateChannel": "beta"}"#).expect("machine");

    assert_eq!(AppSettings::SET, "storage");
    assert_eq!(AppSettings::VERSION, 2);
    assert_eq!(AppSettings::NAMES, ["Theme", "SyncIntervalMinutes", "Zoom", "ShowHidden", "UpdateChannel", "RecentFiles"]);
    assert_eq!(AppSettings::app().as_str(), "kubuno-facade-test");
    assert_eq!(AppSettings::theme(), "System");
    assert_eq!(AppSettings::sync_interval_minutes(), 15, "upgraded from the v1 name");
    assert_eq!(AppSettings::update_channel(), "beta", "the machine's value");
    assert_eq!(AppSettings::recent_files(), vec!["welcome.kbdoc".to_string()]);
    assert!(!AppSettings::show_hidden());

    let changes = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let sub = {
        let changes = changes.clone();
        AppSettings::on_changed(move |c| changes.lock().expect("lock").push(c.name.clone()))
    };
    AppSettings::set_theme("Dark");
    AppSettings::set_zoom(1.25);
    AppSettings::set_show_hidden(true);
    AppSettings::set_recent_files(vec!["a.kbdoc".to_string(), "b.kbdoc".to_string()]);
    AppSettings::set_theme("Purple"); // refused (not an accepted value): logged, nothing changes
    assert_eq!(AppSettings::theme(), "Dark");
    assert_eq!(*changes.lock().expect("lock"), ["Theme", "Zoom", "ShowHidden", "RecentFiles"]);
    drop(sub);

    // Saved at once, roaming and local apart, the version raised.
    let text = std::fs::read_to_string(&roaming).expect("roaming");
    let json: serde_json::Value = serde_json::from_str(&text).expect("json");
    assert_eq!(json["$version"], 2);
    assert_eq!(json["Theme"], "Dark");
    assert_eq!(json["SyncIntervalMinutes"], 15);
    assert!(json.get("SyncInterval").is_none());
    let local = dir.join("data").join("kubuno-facade-test").join("storage.local.settings.json");
    let local: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&local).expect("local")).expect("json");
    assert_eq!(local["RecentFiles"], serde_json::json!(["a.kbdoc", "b.kbdoc"]));

    // The schema is registered: a view's `<Settings Schema="storage">` finds it.
    let (app, schema) = kubuno::storage::settings::registered_schema("storage").expect("registered");
    assert_eq!(app.as_str(), "kubuno-facade-test");
    assert_eq!(schema.find("Zoom").map(|d| d.ty), Some(kubuno::storage::SettingType::Float));

    // Another instance changes the file: the class notices it.
    std::thread::sleep(std::time::Duration::from_millis(30));
    let mut json = json;
    json["Theme"] = serde_json::Value::String("Light".into());
    std::fs::write(&roaming, serde_json::to_string(&json).expect("json")).expect("write");
    assert!(AppSettings::refresh_if_changed());
    assert_eq!(AppSettings::theme(), "Light");

    AppSettings::reset_all().expect("reset");
    assert_eq!(AppSettings::theme(), "System");
    assert_eq!(AppSettings::update_channel(), "beta", "application settings are not the user's to reset");
    let _ = std::fs::remove_dir_all(&dir);
}
