//! The legacy `creds.json` migration against the **real OS credential store** (vskubuno
//! `docs/DESKTOP-OFFLINE-SYNC.md` §19.4 step 1): a *sample* legacy profile in a temporary directory (fake tokens,
//! never a real `creds.json`), secrets under a throw-away prefix (`kubuno-selftest.<key>`, deleted at the end).
//! Ignored by default (it writes to the user's credential store; on Linux CI there is usually no Secret Service):
//! `cargo test -p kubuno-desktop-account --test migrate_os -- --ignored`.

use kubuno_desktop_account::migrate::{adopt_legacy_instances, InstanceOutcome};
use kubuno_desktop_account::{AccountKey, AccountStore};
use kubuno_desktop_secrets::{OsSecretStore, PrefixedSecretStore, SecretName, SecretStore};

/// Every file under `dir` whose bytes contain `needle`.
fn files_containing(dir: &std::path::Path, needle: &str) -> Vec<std::path::PathBuf> {
    let mut hits = Vec::new();
    let Ok(rd) = std::fs::read_dir(dir) else { return hits };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            hits.extend(files_containing(&p, needle));
        } else if std::fs::read(&p).is_ok_and(|b| b.windows(needle.len()).any(|w| w == needle.as_bytes())) {
            hits.push(p);
        }
    }
    hits
}

#[test]
#[ignore = "writes to the real OS credential store; run with --ignored"]
fn legacy_creds_move_to_the_os_store_and_leave_no_plaintext() {
    let root = tempfile::tempdir().expect("tmp");
    let legacy = root.path().join("legacy");
    let data = root.path().join("data");
    // A sample legacy instance: fake tokens (`sub` = selftest-user), a unique refresh token to look for afterwards.
    let refresh = format!("rt-selftest-{}", std::process::id());
    let access = "eyJhbGciOiJub25lIn0.eyJzdWIiOiJzZWxmdGVzdC11c2VyIiwiaWF0IjoxLCJleHAiOjkwMX0.sig";
    let inst = legacy.join("instances").join("selftest-0000aaaa");
    std::fs::create_dir_all(&inst).expect("dir");
    std::fs::write(inst.join("config.json"), r#"{"id":"selftest-0000aaaa","server_url":"https://selftest.invalid","sync_root":"C:/nowhere"}"#).expect("config");
    std::fs::write(inst.join("creds.json"), format!(r#"{{"refresh_token":"{refresh}","access_token":"{access}"}}"#)).expect("creds");

    let store = PrefixedSecretStore::new(OsSecretStore::new(), "kubuno-selftest").expect("store");
    let accounts = AccountStore::new(&data);
    let key = AccountKey::new("https://selftest.invalid", "selftest-user").expect("key");
    let name = SecretName::refresh_token(key.as_str()).expect("name");
    let _ = store.delete_scope(key.as_str());

    let report = adopt_legacy_instances(&legacy, &accounts, &store, &|_| None);
    assert_eq!(report.instances, vec![("selftest-0000aaaa".to_string(), InstanceOutcome::Migrated { account: key.clone() })]);
    // The secret is in the OS store, the file is gone, and no file of the profile holds the token any more.
    assert_eq!(store.get(&name).expect("get").expect("stored").expose(), refresh.as_bytes());
    assert!(!inst.join("creds.json").exists());
    assert_eq!(files_containing(root.path(), &refresh), Vec::<std::path::PathBuf>::new());
    assert_eq!(accounts.account_of_instance("selftest-0000aaaa").expect("read").map(|a| a.key), Some(key.clone()));

    // Idempotent: a second start finds nothing to migrate and keeps the secret.
    let again = adopt_legacy_instances(&legacy, &accounts, &store, &|_| None);
    assert!(again.instances.is_empty(), "{again:?}");
    assert!(store.get(&name).expect("get").is_some());

    store.delete_scope(key.as_str()).expect("cleanup");
    assert!(store.get(&name).expect("get").is_none());
}
