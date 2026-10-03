//! The storage components are registered as non-visual components (the component tray, the Toolbox tab
//! "Storage"), views using them validate and compile, and a view's bindings read and write the shared settings —
//! all on in-memory back-ends (nothing here touches the user's profile, the Registry or the credential store).

extern crate kubuno_app_storage_components as _;

use kubuno_app_storage::backend::BackendKind;
use kubuno_app_storage::settings::register_schema;
use kubuno_app_storage::{AppId, SettingDef, SettingType, SettingValue, Settings as Store, SettingsSchema};
use kubuno_app_storage_components::{SecretStore, Settings};
use kubuno_views::binding::{Value, ViewModel};
use kubuno_views::registry::{self, ClassKind, Origin, PropKind};

#[derive(Default)]
struct Vm;

impl ViewModel for Vm {
    fn get(&self, _path: &str) -> Option<Value> {
        None
    }
    fn set(&mut self, _path: &str, _value: Value) {}
}

#[test]
fn storage_components_are_non_visual_classes_of_the_registry() {
    for name in kubuno_app_storage_components::ELEMENTS {
        let info = registry::project_info(name).unwrap_or_else(|| panic!("{name} is registered"));
        assert_eq!(info.origin, Origin::Linked);
        assert_eq!(info.kind, ClassKind::Component, "{name}");
        assert_eq!(info.crate_name, Some("kubuno_app_storage_components"));
        assert_eq!(info.toolbox_category, Some("Storage"));
        assert!(registry::is_non_visual(name), "{name} goes to the component tray");
    }
    let s = registry::lookup("Settings").expect("meta");
    assert_eq!(s.default_event(), Some("OnSettingChanged"));
    assert!(matches!(s.property("Backend").map(|p| p.kind), Some(PropKind::Enum(v)) if v.contains(&"Registry") && v.contains(&"Memory")));
    assert!(s.property("Schema").is_some() && s.property("AutoSave").is_some() && s.property("AppId").is_some());
    assert!(s.event("OnSettingsSaving").is_some_and(|e| e.cancelable));
    let r = registry::lookup("RegistryKey").expect("meta");
    assert!(matches!(r.property("Hive").map(|p| p.kind), Some(PropKind::Enum(v)) if v.contains(&"LocalMachine")));
    assert!(matches!(r.property("View").map(|p| p.kind), Some(PropKind::Enum(v)) if v.contains(&"Registry32")));
    assert!(r.property("Writable").is_some());
    let k = registry::lookup("SecretStore").expect("meta");
    assert!(matches!(k.property("Backend").map(|p| p.kind), Some(PropKind::Enum(v)) if v.contains(&"Os")));
}

/// `#[kubuno::view]` accepts `x:Name`d storage components through `kubuno_views_meta::kbview::LIBRARY_ELEMENTS`,
/// whose `STORAGE_ELEMENTS` are exactly what this crate registers.
#[test]
fn the_view_macro_knows_every_class_this_crate_registers() {
    let mut ours: Vec<&str> = registry::all().iter().map(|c| c.name).filter(|n| registry::project_info(n).is_some_and(|i| i.crate_name == Some("kubuno_app_storage_components"))).collect();
    ours.sort_unstable();
    assert_eq!(ours, kubuno_app_storage_components::ELEMENTS);
    let mut table = kubuno_views_meta::kbview::STORAGE_ELEMENTS.to_vec();
    table.sort_unstable();
    assert_eq!(table, ours);
    for t in kubuno_views_meta::kbview::STORAGE_TYPED {
        assert!(ours.contains(t));
    }
}

/// The `.kbsettings` format (kubuno-resources-model) and the engine agree on the types and their spellings.
#[test]
fn the_kbsettings_format_and_the_engine_agree() {
    let engine: Vec<&str> = SettingType::ALL.iter().map(|t| t.name()).collect();
    assert_eq!(kubuno_resources_model::settings::TYPES, engine.as_slice());
    for alias in ["bool", "Boolean", "int32", "Int64", "double", "single", "string", "StringCollection", "System.String"] {
        assert_eq!(kubuno_resources_model::settings::canonical_type(alias), SettingType::parse(alias).map(|t| t.name()), "{alias}");
    }
    for (ty, text) in [("Bool", "true"), ("Int", "-3"), ("Float", "2.5"), ("String", "x")] {
        assert!(kubuno_resources_model::settings::valid_default(ty, text));
        assert!(SettingValue::parse(text, SettingType::parse(ty).expect("type")).is_some(), "{ty} {text}");
    }
    assert!(!kubuno_resources_model::settings::valid_default("Int", "1.5"));
    assert!(SettingValue::parse("1.5", SettingType::Int).is_none());
    assert_eq!(kubuno_resources_model::settings::looks_like_secret("SmtpPassword"), kubuno_app_storage::settings::looks_like_secret("SmtpPassword"));
}

#[test]
fn views_with_storage_components_validate_and_compile() {
    let view = r#"
        <Panel DesignWidth="600" DesignHeight="400">
          <Settings x:Name="settings" Schema="ui" Backend="Memory" AutoSave="false" OnSettingChanged="setting_changed"/>
          <SecretStore x:Name="secrets" Backend="Memory"/>
          <RegistryKey x:Name="key" Hive="CurrentUser" Path="Software\Kubuno\Example" View="Registry64"/>
          <Switch On="{Binding Source=settings, Path=Compact, Mode=TwoWay}" X="8" Y="8" Width="60" Height="24"/>
          <Label Visible="{Binding Source=secrets, Path=ApiKey.Exists}" Text="saved" X="8" Y="40" Width="80" Height="20"/>
        </Panel>"#;
    let diagnostics = kubuno_views::validate::validate_with_default_registry(&kubuno_views::syntax::parse(view));
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    assert!(kubuno_views::compile::compile(view).is_ok());
    let bad = kubuno_views::validate::validate_with_default_registry(&kubuno_views::syntax::parse(r#"<Settings Backend="Cloud"/>"#));
    assert!(!bad.is_empty(), "an unknown back-end is reported");
}

/// A view's `<Settings>` binds to the shared settings of its app: what the bindings write, the typed code reads
/// (and the reverse), with the registered schema's types and defaults.
#[test]
fn a_view_binds_to_the_shared_settings() {
    let app = AppId::new("components-test").expect("id");
    let schema = SettingsSchema::new("ui", 1).with(SettingDef::new("Compact", false)).with(SettingDef::new("Zoom", 100i64)).with(SettingDef::new("Theme", "System").one_of(&["System", "Dark"]));
    register_schema(&app, schema.clone());
    let view = r#"<Panel DesignWidth="400" DesignHeight="300"><Settings x:Name="settings" Schema="ui" AppId="components-test" Backend="Memory" OnSettingChanged="changed"/></Panel>"#;
    let mut rt = kubuno_views::runtime::Runtime::new();
    assert!(rt.reload_from_text(view), "{:?}", rt.diagnostics());
    let scope = rt.components();
    let mut vm = Vm;
    scope.sync_all(&mut vm, false);
    let shared = Store::shared(&app, &schema, BackendKind::Memory).expect("shared");
    {
        let mut scoped = scope.view_model(&mut vm, false);
        assert_eq!(scoped.get("settings.Zoom"), Some(Value::F32(100.0)), "the registered default");
        scoped.set("settings.Compact", Value::Bool(true));
        scoped.set("settings.Zoom", Value::Str("125".into()));
        scoped.set("settings.Theme", Value::Str("Purple".into()));
    }
    assert_eq!(shared.get_as::<bool>("Compact"), Some(true), "the binding wrote the shared settings");
    assert_eq!(shared.get_as::<i64>("Zoom"), Some(125), "converted to the declared type");
    assert_eq!(shared.get_as::<String>("Theme").as_deref(), Some("System"), "a refused value leaves the setting alone");
    assert_eq!(rt.with_component::<Settings, _>("settings", |s| s.last_error().map(str::to_string)).flatten().map(|e| e.contains("Theme")), Some(true));
    shared.set("Theme", "Dark").expect("set from code");
    assert!(scope.sync_all(&mut vm, false), "the view repaints after a change made elsewhere");
    assert_eq!(scope.view_model(&mut vm, false).get("settings.Theme"), Some(Value::Str("Dark".into())));
    // AutoSave: the next sync saved what the bindings changed (the memory back-end of the shared instance).
    assert!(!shared.is_dirty());
}

#[test]
fn a_secret_store_never_binds_a_value() {
    let view = r#"<Panel DesignWidth="400" DesignHeight="300"><SecretStore x:Name="secrets" AppId="secret-test" Backend="Memory"/></Panel>"#;
    let mut rt = kubuno_views::runtime::Runtime::new();
    assert!(rt.reload_from_text(view));
    let scope = rt.components();
    let mut vm = Vm;
    scope.sync_all(&mut vm, false);
    rt.with_component::<SecretStore, _>("secrets", |s| s.set_str("ApiKey", "s3cr3t")).expect("component").expect("set");
    let scoped = scope.view_model(&mut vm, false);
    assert_eq!(scoped.get("secrets.ApiKey.Exists"), Some(Value::Bool(true)));
    assert_eq!(scoped.get("secrets.Other.Exists"), Some(Value::Bool(false)));
    assert_eq!(scoped.get("secrets.Available"), Some(Value::Bool(true)));
    assert_eq!(scoped.get("secrets.ApiKey"), None, "the value itself is never a binding path");
    let got = rt.with_component::<SecretStore, _>("secrets", |s| s.get("ApiKey")).expect("component").expect("get").expect("some");
    assert_eq!(got.expose(), b"s3cr3t");
    assert_eq!(format!("{got:?}"), "Secret(<redacted>)");
}

#[test]
fn a_key_value_store_binds_keys_and_writes_back() {
    let view = r#"<Panel DesignWidth="400" DesignHeight="300"><KeyValueStore x:Name="state" Store="kv-test" Persistence="Session"/></Panel>"#;
    let mut rt = kubuno_views::runtime::Runtime::new();
    assert!(rt.reload_from_text(view), "{:?}", rt.diagnostics());
    let scope = rt.components();
    let mut vm = Vm;
    scope.sync_all(&mut vm, false);
    scope.view_model(&mut vm, false).set("state.lastSearch", Value::Str("invoices".into()));
    scope.view_model(&mut vm, false).set("state.zoom", Value::F32(1.5));
    let got = rt.with_component::<kubuno_app_storage_components::KeyValueStore, _>("state", |s| (s.get_string("lastSearch"), s.keys())).expect("component");
    assert_eq!(got.0.as_deref(), Some("invoices"));
    assert_eq!(got.1, vec!["lastSearch".to_string(), "zoom".to_string()]);
    assert_eq!(scope.view_model(&mut vm, false).get("state.zoom"), Some(Value::F32(1.5)));
    assert_eq!(scope.view_model(&mut vm, false).get("state.missing"), None);
}

#[test]
fn a_file_store_lists_its_files_for_bindings() {
    let view = r#"<Panel DesignWidth="400" DesignHeight="300"><FileStore x:Name="thumbs" Folder="fs-test" Kind="Temp" AppId="components-test"/></Panel>"#;
    let mut rt = kubuno_views::runtime::Runtime::new();
    assert!(rt.reload_from_text(view), "{:?}", rt.diagnostics());
    let scope = rt.components();
    let mut vm = Vm;
    rt.with_component::<kubuno_app_storage_components::FileStore, _>("thumbs", |f| {
        f.clear().expect("clear");
        f.write("a.png", &[1, 2, 3]).expect("write");
        f.write("b.png", &[4]).expect("write");
        assert!(f.write("../x", &[0]).is_err(), "names only");
    })
    .expect("component");
    scope.sync_all(&mut vm, false);
    let scoped = scope.view_model(&mut vm, false);
    assert_eq!(scoped.get("thumbs.Count"), Some(Value::F32(2.0)));
    assert_eq!(scoped.get("thumbs.Size"), Some(Value::F32(4.0)));
    assert!(matches!(scoped.get("thumbs.Files"), Some(Value::List(rows)) if rows.len() == 2));
    rt.with_component::<kubuno_app_storage_components::FileStore, _>("thumbs", |f| f.clear().expect("clear")).expect("component");
}
