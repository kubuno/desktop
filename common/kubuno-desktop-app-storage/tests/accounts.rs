//! Account-scoped data (decision Q6), key/value stores, file stores and local database paths, for real in a
//! sandboxed profile (one test per binary: the sandbox variable and the current account are process-wide).

use kubuno_desktop_app_storage::backend::BackendKind;
use kubuno_desktop_app_storage::files::local_database_path;
use kubuno_desktop_app_storage::{paths, set_current_account, AccountKey, AppId, FileKind, FileStore, KeyValueStore, Persistence, SettingDef, Settings, SettingsSchema};

#[test]
fn account_scoped_data_follows_the_current_account_inside_the_sandbox() {
    let dir = tempfile::tempdir().expect("tmp");
    std::env::set_var(paths::SANDBOX_ENV, dir.path());
    let app = AppId::new("account-test").expect("id");
    let schema = SettingsSchema::new("inbox", 1).with(SettingDef::new("Signature", "")).per_account();

    // No account: kept in memory, nothing written.
    let none = Settings::shared(&app, &schema, BackendKind::Auto).expect("open");
    none.set("Signature", "nobody").expect("set");
    none.save().expect("save");
    assert!(!dir.path().join("data").join("accounts").exists());

    let a = AccountKey::new("0123456789abcdef").expect("a");
    let b = AccountKey::new("fedcba9876543210").expect("b");
    set_current_account(Some(a.clone()));
    let sa = Settings::shared(&app, &schema, BackendKind::Auto).expect("a");
    sa.set("Signature", "Alice").expect("set");
    sa.save().expect("save");
    let file_a = dir.path().join("data").join("accounts").join(a.as_str()).join("account-test").join("inbox.settings.json");
    assert!(file_a.is_file(), "{file_a:?}");

    set_current_account(Some(b.clone()));
    let sb = Settings::shared(&app, &schema, BackendKind::Auto).expect("b");
    assert_eq!(sb.get_as::<String>("Signature").as_deref(), Some(""), "another account starts from the defaults");
    sb.set("Signature", "Bob").expect("set");
    sb.save().expect("save");
    set_current_account(Some(a.clone()));
    assert_eq!(Settings::shared(&app, &schema, BackendKind::Auto).expect("a again").get_as::<String>("Signature").as_deref(), Some("Alice"));

    // Key/value stores, file stores and local databases of the account.
    let kv = KeyValueStore::open(&app, "state", Persistence::Persistent, Some(&a)).expect("kv");
    kv.set("lastFolder", "Inbox", None).expect("set");
    assert!(kv.location().expect("file").starts_with(dir.path().join("data").join("accounts").join(a.as_str())));
    let files = FileStore::open(&app, "attachments", FileKind::Data, Some(&a)).expect("files");
    files.write("a.txt", b"x").expect("write");
    assert!(files.root().starts_with(dir.path().join("data").join("accounts")));
    let db = local_database_path("account:account-test/mail").expect("spec").expect("path");
    assert_eq!(db, dir.path().join("data").join("accounts").join(a.as_str()).join("account-test").join("databases").join("mail.db"));
    let appdb = local_database_path("app:account-test/cache").expect("spec").expect("path");
    assert_eq!(appdb, dir.path().join("data").join("account-test").join("databases").join("cache.db"));
    let temp = FileStore::open(&app, "work", FileKind::Temp, None).expect("temp");
    assert!(temp.root().starts_with(dir.path().join("temp")));
    let cache = FileStore::open(&app, "thumbs", FileKind::Cache, None).expect("cache");
    assert!(cache.root().starts_with(dir.path().join("cache")));

    // Forgetting the account's data of the app removes all of it.
    assert!(kubuno_desktop_app_storage::account::delete_account_app_data(&a, &app).expect("delete"));
    assert!(!file_a.exists());
    set_current_account(None);
}
