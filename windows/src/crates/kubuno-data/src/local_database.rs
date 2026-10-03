//! `<LocalDatabase>`: a SQLite database of the app on the device (vskubuno `docs/STORAGE-COMPONENTS.md` §3.5,
//! lot ST-2) — a `DbConnection` whose file the app never names by path:
//!
//! ```xml
//! <LocalDatabase x:Name="db" DatabaseName="notes"/>
//! <TableAdapter x:Name="notesAdapter" Connection="db" SelectCommand="SELECT * FROM notes" UpdateTable="notes"/>
//! ```
//!
//! The file is `<user_data_dir>/<app>/databases/<DatabaseName>.db` (`AccountScoped`: the signed-in account's,
//! `<user_data_dir>/accounts/<key>/<app>/databases/…`, the folder an account's sign-out removes), resolved by
//! `kubuno_app_storage::files::local_database_path` — the same spec a plain `DbConnection` takes as
//! `ConnectionString="Data Source=app:notes"`. In a sandboxed profile it is the sandbox's.

use kubuno_views::prelude::*;

use crate::connection::DbConnection;
use crate::provider::Provider;

/// A SQLite database of the app on the device, named rather than pathed (<data>/<app>/databases/<name>.db): a DbConnection for table adapters and commands.
#[derive(Component)]
#[kubuno(extends = DbConnection, levels(Component))]
#[toolbox(icon = "database-zap", category = "Storage")]
#[default_property("DatabaseName")]
pub struct LocalDatabase {
    base: DbConnection,
    /// The database's name: a SQLite file of the app's data folder (<data>/<app>/databases/<name>.db).
    #[property(on_change = "sync_target")]
    #[category("Storage")]
    #[default_value("app")]
    pub database_name: String,
    /// The app the database belongs to (its id); empty for the application's own.
    #[property(on_change = "sync_target")]
    #[category("Storage")]
    pub app_id: String,
    /// Whether the database belongs to the signed-in account (each account its own file).
    #[property(on_change = "sync_target")]
    #[category("Storage")]
    #[default_value(false)]
    pub account_scoped: bool,
}

impl Default for LocalDatabase {
    fn default() -> Self {
        let mut db = Self { base: DbConnection::new(Provider::Sqlite), database_name: "app".into(), app_id: String::new(), account_scoped: false };
        db.sync_target();
        db
    }
}

impl std::fmt::Debug for LocalDatabase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalDatabase").field("database_name", &self.database_name).field("account_scoped", &self.account_scoped).finish()
    }
}

impl LocalDatabase {
    /// The database `name` of the app.
    pub fn named(name: &str) -> Self {
        let mut db = Self { database_name: name.to_string(), ..Self::default() };
        db.sync_target();
        db
    }

    /// The data source spec of the properties (`app:notes`, `account:kubuno-mail/inbox`).
    pub fn spec(&self) -> String {
        let scope = if self.account_scoped { "account" } else { "app" };
        let name = self.database_name.trim();
        match self.app_id.trim() {
            "" => format!("{scope}:{name}"),
            app => format!("{scope}:{app}/{name}"),
        }
    }

    /// Keeps the underlying connection on SQLite and on the file the properties name.
    fn sync_target(&mut self) {
        self.base.provider = Provider::Sqlite;
        self.base.connection_string_name.clear();
        self.base.connection_string = format!("Data Source={}", self.spec());
    }

    /// The `DbConnection` it is (open, commands, transactions).
    pub fn connection(&mut self) -> &mut DbConnection {
        &mut self.base
    }
}
