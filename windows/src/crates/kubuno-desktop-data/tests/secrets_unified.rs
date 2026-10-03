//! The unified secret naming scheme (vskubuno docs/STORAGE-COMPONENTS.md, decision Q4): the app's secrets are read
//! first, an older credential found later is copied into them. In-memory stores only (no OS credential store).

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use kubuno_desktop_app_storage::{AppId, AppSecrets};
use kubuno_desktop_data::{AppSecretsSource, DataError, MigratingSource, SecretResolver, SecretSource};

/// The older `Kubuno:<id>:<key>` credentials, counting their reads.
struct Legacy(Arc<AtomicUsize>);

impl SecretSource for Legacy {
    fn name(&self) -> &'static str {
        "credential manager"
    }
    fn get(&self, key: &str) -> Result<Option<String>, DataError> {
        self.0.fetch_add(1, Ordering::Relaxed);
        Ok((key == "ConnectionStrings:Northwind").then(|| "Host=db;Password=old".to_string()))
    }
}

#[test]
fn older_credentials_move_to_the_unified_scheme() {
    let secrets = AppSecrets::in_memory(&AppId::new("northwind-app").expect("id"));
    let unified = AppSecretsSource::with(secrets.clone());
    let reads = Arc::new(AtomicUsize::new(0));
    let resolver = SecretResolver::empty().with(unified.clone()).with(MigratingSource::new(Legacy(reads.clone()), unified.clone()));
    assert_eq!(AppSecretsSource::name_of("ConnectionStrings:Northwind"), "ConnectionStrings.Northwind");

    assert_eq!(resolver.resolve("ConnectionStrings:Northwind").expect("found"), "Host=db;Password=old");
    assert_eq!(reads.load(Ordering::Relaxed), 1);
    assert!(secrets.contains("ConnectionStrings.Northwind").expect("copied"), "copied as Kubuno/app.northwind-app/ConnectionStrings.Northwind");
    // The next resolution is served by the unified scheme: the older store is not read again.
    assert_eq!(resolver.resolve("ConnectionStrings:Northwind").expect("found"), "Host=db;Password=old");
    assert_eq!(reads.load(Ordering::Relaxed), 1);

    unified.set("ConnectionStrings:Other", "Host=x").expect("set");
    assert_eq!(resolver.resolve("ConnectionStrings:Other").expect("found"), "Host=x");
    let err = resolver.resolve("ConnectionStrings:Missing").expect_err("missing").to_string();
    assert!(err.contains("app secrets") && !err.contains("Password"), "{err}");
    assert!(unified.remove("ConnectionStrings:Other").expect("remove"));
}
