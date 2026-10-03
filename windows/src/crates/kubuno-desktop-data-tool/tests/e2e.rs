//! End to end: the built binary, spoken to over stdio exactly as Visual Studio does, against a
//! temporary SQLite database and a temporary tool home (`KUBUNO_DATA_TOOL_HOME`): the real user's
//! Data Explorer list and Credential Manager are never touched.
//!
//! PostgreSQL / MySQL / SQL Server are exercised only when `KUBUNO_TEST_PG_URL`,
//! `KUBUNO_TEST_MYSQL_URL`, `KUBUNO_TEST_MSSQL_URL` are set (they are not on the build machine).

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

use kubuno_desktop_data_model::DataSource;
use serde_json::{json, Value};

struct Tool {
    child: Child,
    stdin: Option<ChildStdin>,
    rx: Receiver<Value>,
    seen: Vec<Value>,
    next_id: i64,
}

impl Tool {
    fn start(home: &Path) -> Tool {
        let mut child = Command::new(env!("CARGO_BIN_EXE_kubuno-data-tool"))
            .arg("--stdio")
            .env("KUBUNO_DATA_TOOL_HOME", home)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("the tool starts");
        let stdout = child.stdout.take().expect("stdout");
        let stdin = child.stdin.take();
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                match serde_json::from_str::<Value>(&line) {
                    Ok(v) => {
                        if tx.send(v).is_err() {
                            break;
                        }
                    }
                    Err(e) => panic!("the tool wrote a line that is not JSON ({e}): {line}"),
                }
            }
        });
        Tool { child, stdin, rx, seen: Vec::new(), next_id: 1 }
    }

    fn send(&mut self, method: &str, params: Value) -> i64 {
        let id = self.next_id;
        self.next_id += 1;
        let line = json!({"id": id, "method": method, "params": params}).to_string();
        let stdin = self.stdin.as_mut().expect("stdin open");
        writeln!(stdin, "{line}").expect("write request");
        stdin.flush().expect("flush");
        id
    }

    /// The response with this id, waiting up to `secs` (other responses are kept for later).
    fn wait(&mut self, id: i64, secs: u64) -> Value {
        let deadline = Instant::now() + Duration::from_secs(secs);
        loop {
            if let Some(pos) = self.seen.iter().position(|r| r["id"] == id) {
                return self.seen.remove(pos);
            }
            let left = deadline.saturating_duration_since(Instant::now());
            match self.rx.recv_timeout(left) {
                Ok(v) => self.seen.push(v),
                Err(RecvTimeoutError::Timeout) => panic!("no response to request {id} within {secs} s"),
                Err(RecvTimeoutError::Disconnected) => panic!("the tool closed its output before answering {id}"),
            }
        }
    }

    fn call(&mut self, method: &str, params: Value) -> Value {
        let id = self.send(method, params);
        self.wait(id, 60)
    }

    /// The `result`, failing the test on an error response.
    fn ok(&mut self, method: &str, params: Value) -> Value {
        let r = self.call(method, params);
        assert!(r.get("error").is_none(), "{method} failed: {r}");
        r["result"].clone()
    }

    fn err(&mut self, method: &str, params: Value) -> Value {
        let r = self.call(method, params);
        assert!(r.get("result").is_none(), "{method} should have failed: {r}");
        r["error"].clone()
    }
}

impl Drop for Tool {
    fn drop(&mut self) {
        drop(self.stdin.take());
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("kubuno-data-tool-e2e-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array().expect("array").iter().map(|x| x.as_str().expect("string").to_string()).collect()
}

#[test]
fn version_flag_and_exit_on_end_of_input() {
    let out = Command::new(env!("CARGO_BIN_EXE_kubuno-data-tool")).arg("--version").output().expect("runs");
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), env!("CARGO_PKG_VERSION"));

    let dir = scratch("eof");
    let mut tool = Tool::start(&dir);
    let ping = tool.ok("ping", json!({}));
    assert_eq!(ping["version"], env!("CARGO_PKG_VERSION"));
    drop(tool.stdin.take());
    let status = tool.child.wait().expect("exits");
    assert!(status.success(), "EOF on stdin exits 0: {status}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_password_of_a_connection_string_is_never_in_an_error() {
    let dir = scratch("secret");
    let mut tool = Tool::start(&dir);
    let cs = "Host=127.0.0.1;Port=1;Database=shop;Username=kubuno;Password=S3cr3t!";
    let target = json!({"provider": "postgres", "connectionString": cs});

    let e = tool.err("connection.test", json!({"target": target, "timeoutSeconds": 2}));
    assert_eq!(e["kind"], "Database", "{e}");
    let all_output = e.to_string();
    assert!(!all_output.contains("S3cr3t"), "{all_output}");

    let e = tool.err("query.execute", json!({"target": target, "sql": "SELECT 1", "timeoutSeconds": 2}));
    assert!(!e.to_string().contains("S3cr3t"), "{e}");
    let e = tool.err("schema.load", json!({"target": target, "timeoutSeconds": 2}));
    assert!(!e.to_string().contains("S3cr3t"), "{e}");
    // A URL form too, and an unusable string.
    let url = json!({"provider": "postgres", "connectionString": "postgres://kubuno:S3cr3t!@127.0.0.1:1/shop"});
    assert!(!tool.err("connection.test", json!({"target": url, "timeoutSeconds": 2})).to_string().contains("S3cr3t"));
    let bad = json!({"provider": "postgres", "connectionString": "Host=h;Password='S3cr3t!"});
    assert!(!tool.err("connection.test", json!({"target": bad})).to_string().contains("S3cr3t"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_whole_session_on_sqlite() {
    let dir = scratch("session");
    let db = dir.join("shop.db");
    // An empty file is a valid, empty SQLite database (the tool never creates a missing one).
    std::fs::write(&db, []).expect("empty database");
    let db_text = db.to_string_lossy().into_owned();
    let mut tool = Tool::start(&dir);

    // ---- explorer: add (secret in the user secrets, redacted in the list) ----
    let cs = format!("Data Source={db_text}");
    let added = tool.ok("explorer.add", json!({"name": "Shop", "provider": "sqlite", "connectionString": cs, "store": "usersecrets", "overwrite": false}));
    assert_eq!(added["connection"]["store"], "usersecrets");
    assert_eq!(added["connection"]["display"], db_text.as_str());
    let pg_cs = "Host=db.example;Port=5432;Database=shop;Username=kubuno;Password=S3cr3t!";
    let added_pg = tool.ok("explorer.add", json!({"name": "Prod", "provider": "postgres", "connectionString": pg_cs, "store": "usersecrets", "overwrite": false}));
    assert_eq!(added_pg["connection"]["display"], "db.example:5432/shop (user kubuno)");
    let duplicate = tool.err("explorer.add", json!({"name": "shop", "provider": "sqlite", "connectionString": cs, "store": "usersecrets", "overwrite": false}));
    assert_eq!(duplicate["kind"], "Validation");
    let bad_name = tool.err("explorer.add", json!({"name": "a/b", "provider": "sqlite", "connectionString": cs, "store": "usersecrets", "overwrite": false}));
    assert_eq!(bad_name["kind"], "Validation");

    let listed = tool.ok("explorer.list", json!({}));
    let names: Vec<&str> = listed["connections"].as_array().expect("list").iter().map(|c| c["name"].as_str().expect("name")).collect();
    assert_eq!(names, ["Prod", "Shop"]);
    assert!(!listed.to_string().contains("S3cr3t"), "the list shows no secret");
    let list_file = std::fs::read_to_string(dir.join("DataExplorer").join("connections.json")).expect("list file");
    assert!(!list_file.contains("S3cr3t") && list_file.contains("db.example:5432/shop"), "{list_file}");
    let secrets_file = std::fs::read_to_string(dir.join("UserSecrets").join("DataExplorer").join("secrets.json")).expect("secrets file");
    assert!(secrets_file.contains("ConnectionStrings:Shop") && secrets_file.contains("S3cr3t!"), "the connection string is in the store only");

    // Copy a connection to a project's user secrets.
    tool.ok("secrets.copyToProject", json!({"explorer": "Shop", "userSecretsId": "e2e-app", "key": "ConnectionStrings:Shop", "store": "usersecrets"}));
    let project_secrets = std::fs::read_to_string(dir.join("UserSecrets").join("e2e-app").join("secrets.json")).expect("project secrets");
    assert!(project_secrets.contains("ConnectionStrings:Shop"));
    tool.ok("explorer.remove", json!({"name": "Prod"}));

    let target = json!({"explorer": "Shop"});
    let tested = tool.ok("connection.test", json!({"target": target}));
    assert!(tested["serverVersion"].as_str().is_some_and(|v| v.starts_with('3')), "{tested}");
    assert!(tested["elapsedMs"].is_u64());
    // The same target as an inline string, and as a project connection.
    tool.ok("connection.test", json!({"target": {"provider": "sqlite", "connectionString": cs}}));
    std::fs::write(dir.join("Cargo.toml"), "[package]\nname = \"e2e-app\"\nversion = \"0.1.0\"\n[package.metadata.kubuno]\nuser-secrets-id = \"e2e-app\"\n").expect("manifest");
    let project = json!({"project": {"manifestDir": dir.to_string_lossy(), "connection": "Shop", "provider": "sqlite"}});
    tool.ok("connection.test", json!({"target": project}));

    // ---- query.execute: DDL and data, several statements, `;` inside a literal ----
    let ddl = "CREATE TABLE customers (id INTEGER PRIMARY KEY, name TEXT NOT NULL, email VARCHAR(40) UNIQUE, vip BOOLEAN NOT NULL DEFAULT 0);\n\
               CREATE TABLE orders (id INTEGER PRIMARY KEY, customer_id INTEGER NOT NULL REFERENCES customers(id), total REAL, note TEXT);\n\
               CREATE INDEX ix_orders_customer ON orders(customer_id);\n\
               CREATE VIEW v_orders AS SELECT o.id AS order_id, c.name FROM orders o JOIN customers c ON c.id = o.customer_id;\n\
               INSERT INTO customers (name, email) VALUES ('Ada; Lovelace', 'ada@example.org'), ('Linus', NULL);\n\
               INSERT INTO orders (customer_id, total, note) VALUES (1, 12.5, 'first'), (1, 0.1, NULL);";
    let created = tool.ok("query.execute", json!({"target": target, "sql": ddl}));
    assert_eq!(created["resultSets"], json!([]));
    assert_eq!(strings(&created["messages"]), ["0 row(s) affected", "0 row(s) affected", "0 row(s) affected", "0 row(s) affected", "2 row(s) affected", "2 row(s) affected"]);

    // ---- schema.load ----
    let schema = tool.ok("schema.load", json!({"target": target}));
    assert_eq!(schema["provider"], "sqlite");
    assert!(schema["serverVersion"].as_str().is_some_and(|v| v.starts_with('3')));
    assert_eq!(schema["database"], "shop.db");
    let main = &schema["schemas"][0];
    assert_eq!(main["name"], "main");
    assert_eq!(main["functions"], json!([]));
    let table = |name: &str| main["tables"].as_array().expect("tables").iter().find(|t| t["name"] == name).cloned().unwrap_or_else(|| panic!("table {name} in {main}"));
    let table_names: Vec<String> = main["tables"].as_array().expect("tables").iter().map(|t| t["name"].as_str().expect("n").to_string()).collect();
    assert_eq!(table_names, ["customers", "orders", "v_orders"]);
    assert!(!schema.to_string().contains("sqlite_sequence") && !schema.to_string().contains("sqlite_autoindex_orders"));
    let customers = table("customers");
    assert_eq!(customers["kind"], "table");
    assert_eq!(customers["primaryKey"], json!(["id"]));
    let id = &customers["columns"][0];
    assert_eq!((id["name"].as_str(), id["dbType"].as_str(), id["rustType"].as_str()), (Some("id"), Some("INTEGER"), Some("i64")));
    assert_eq!((id["nullable"].clone(), id["primaryKey"].clone(), id["autoIncrement"].clone(), id["readOnly"].clone()), (json!(false), json!(true), json!(true), json!(false)));
    let email = &customers["columns"][2];
    assert_eq!((email["dbType"].as_str(), email["rustType"].as_str(), email["nullable"].clone(), email["maxLength"].clone()), (Some("VARCHAR(40)"), Some("String"), json!(true), json!(40)));
    let vip = &customers["columns"][3];
    assert_eq!((vip["rustType"].as_str(), vip["nullable"].clone(), vip["default"].as_str()), (Some("bool"), json!(false), Some("0")));
    assert_eq!(customers["columns"][1]["nullable"], false);
    let orders = table("orders");
    assert_eq!(orders["foreignKeys"], json!([{"name": "fk_orders_customer_id", "columns": ["customer_id"], "refSchema": "main", "refTable": "customers", "refColumns": ["id"]}]));
    assert_eq!(orders["indexes"], json!([{"name": "ix_orders_customer", "columns": ["customer_id"], "unique": false}]));
    assert_eq!(customers["indexes"], json!([{"name": "sqlite_autoindex_customers_1", "columns": ["email"], "unique": true}]));
    let view = table("v_orders");
    assert_eq!(view["kind"], "view");
    assert_eq!(view["columns"].as_array().expect("cols").len(), 2);
    // System tables only on request.
    let with_system = tool.ok("schema.load", json!({"target": target, "includeSystem": true}));
    assert!(with_system["schemas"][0]["tables"].as_array().expect("t").len() >= 3);

    // ---- data.top ----
    let top = tool.ok("data.top", json!({"target": target, "schema": "main", "table": "customers", "limit": 1}));
    assert_eq!(top["columns"].as_array().expect("cols").iter().map(|c| c["name"].as_str().expect("n")).collect::<Vec<_>>(), ["id", "name", "email", "vip"]);
    assert_eq!(top["rows"], json!([["1", "Ada; Lovelace", "ada@example.org", "false"]]));
    let all = tool.ok("data.top", json!({"target": target, "schema": "", "table": "customers", "limit": 100}));
    assert_eq!(all["rows"].as_array().expect("rows").len(), 2);
    assert_eq!(all["rows"][1][2], Value::Null, "NULL is JSON null");
    let missing = tool.err("data.top", json!({"target": target, "schema": "main", "table": "customers\"; DROP TABLE customers; --", "limit": 1}));
    assert_eq!(missing["kind"], "Validation");
    assert_eq!(tool.err("data.top", json!({"target": target, "table": "customers", "limit": 0}))["kind"], "Validation");

    // ---- query.execute: an UPDATE and a SELECT, a `;` in a literal ----
    let ran = tool.ok(
        "query.execute",
        json!({"target": target, "sql": "UPDATE customers SET name = 'Grace; Hopper' WHERE id = 1;\nSELECT c.id, c.name, o.total FROM customers c LEFT JOIN orders o ON o.customer_id = c.id ORDER BY c.id, o.id; -- done", "maxRows": 2}),
    );
    assert_eq!(strings(&ran["messages"]), ["1 row(s) affected"]);
    let sets = ran["resultSets"].as_array().expect("sets");
    assert_eq!(sets.len(), 1);
    assert_eq!(sets[0]["rows"][0], json!(["1", "Grace; Hopper", "12.5"]));
    assert_eq!(sets[0]["rows"].as_array().expect("rows").len(), 2);
    assert_eq!(sets[0]["truncated"], true, "3 rows, maxRows 2");
    assert_eq!(sets[0]["columns"][2], json!({"name": "total", "dbType": "REAL"}));
    // An error names the statement and says what already ran; SQLite session state is per connection.
    let failed = tool.err("query.execute", json!({"target": target, "sql": "UPDATE customers SET vip = 1 WHERE id = 2; SELECT * FROM nope"}));
    assert_eq!(failed["kind"], "Database");
    let message = failed["message"].as_str().expect("message");
    assert!(message.contains("statement 2 of 2") && message.contains("nope") && message.contains("not rolled back"), "{message}");
    assert_eq!(tool.err("query.execute", json!({"target": target, "sql": "  ; -- nothing"}))["kind"], "Validation");
    // A statement with an `@name` reaches the server as written.
    let literal = tool.ok("query.execute", json!({"target": target, "sql": "SELECT 'a@b' AS x"}));
    assert_eq!(literal["resultSets"][0]["rows"], json!([["a@b"]]));

    // ---- script.generate ----
    let script = |tool: &mut Tool, kind: &str| tool.ok("script.generate", json!({"target": target, "schema": "main", "table": "customers", "kind": kind}))["sql"].as_str().expect("sql").to_string();
    assert_eq!(script(&mut tool, "select"), "SELECT\n    \"id\",\n    \"name\",\n    \"email\",\n    \"vip\"\nFROM \"main\".\"customers\";\n");
    assert_eq!(script(&mut tool, "insert"), "INSERT INTO \"main\".\"customers\" (\n    \"name\",\n    \"email\",\n    \"vip\"\n)\nVALUES (\n    @name,\n    @email,\n    @vip\n);\n");
    assert_eq!(script(&mut tool, "update"), "UPDATE \"main\".\"customers\"\nSET\n    \"name\" = @name,\n    \"email\" = @email,\n    \"vip\" = @vip\nWHERE \"id\" = @id;\n");
    assert_eq!(script(&mut tool, "delete"), "DELETE FROM \"main\".\"customers\"\nWHERE \"id\" = @id;\n");
    let create = script(&mut tool, "create");
    assert!(create.starts_with("CREATE TABLE \"customers\" (\n    \"id\" INTEGER PRIMARY KEY,\n"), "{create}");
    assert!(create.contains("\"vip\" BOOLEAN NOT NULL DEFAULT 0") && create.contains("UNIQUE (\"email\")"), "{create}");
    // The generated CREATE TABLE runs: a copy of the table in a scratch database.
    let copy_db = dir.join("copy.db");
    std::fs::write(&copy_db, []).expect("empty");
    let copy_target = json!({"provider": "sqlite", "connectionString": format!("Data Source={}", copy_db.display())});
    tool.ok("query.execute", json!({"target": copy_target, "sql": create}));
    let orders_create = tool.ok("script.generate", json!({"target": target, "table": "orders", "kind": "create"}))["sql"].as_str().expect("sql").to_string();
    assert!(orders_create.contains("CONSTRAINT \"fk_orders_customer_id\" FOREIGN KEY (\"customer_id\") REFERENCES \"customers\" (\"id\")"), "{orders_create}");
    assert_eq!(tool.err("script.generate", json!({"target": target, "table": "v_orders", "kind": "create"}))["kind"], "Validation");
    assert_eq!(tool.err("script.generate", json!({"target": target, "table": "customers", "kind": "drop"}))["kind"], "Validation");

    // ---- kbdata.build / kbdata.read ----
    let built = tool.ok(
        "kbdata.build",
        json!({"target": target, "name": "Shop", "connection": "Shop", "objects": [{"schema": "main", "name": "customers"}, {"schema": "main", "name": "orders"}, {"schema": "main", "name": "v_orders"}]}),
    );
    let text = built["text"].as_str().expect("text");
    let source = DataSource::parse(text).expect("the generated .kbdata parses back");
    assert_eq!((source.name.as_str(), source.connection.as_str(), source.provider.as_str()), ("Shop", "Shop", "sqlite"));
    assert_eq!(source.schema, "", "the default schema stays implicit");
    assert_eq!(source.tables.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(), ["customers", "orders", "v_orders"]);
    let t = source.table("customers").expect("customers");
    assert_eq!(t.key, ["id"]);
    assert_eq!((t.columns[0].rust_type.as_str(), t.columns[0].auto_increment), ("i64", true));
    assert_eq!((t.columns[3].rust_type.as_str(), t.columns[3].db_type.as_str()), ("bool", "BOOLEAN"));
    assert_eq!(built["sqlxFeatures"], json!([]));
    assert!(!text.contains("Data Source") && !text.contains(&db_text), "no connection string in the .kbdata");
    let kbdata_path = dir.join("shop.kbdata");
    std::fs::write(&kbdata_path, text).expect("write kbdata");
    let read = tool.ok("kbdata.read", json!({"path": kbdata_path.to_string_lossy()}));
    assert_eq!(read["rowNames"], json!({"customers": "Customer", "orders": "Order", "v_orders": "VOrder"}));
    assert_eq!(read["tables"][0]["columns"][0]["db_type"], "INTEGER");
    assert_eq!(tool.err("kbdata.build", json!({"target": target, "name": "Shop", "connection": "Shop", "objects": [{"schema": "main", "name": "nope"}]}))["kind"], "Validation");

    // ---- migrations ----
    let migrations = dir.join("migrations");
    let first = tool.ok("migrate.add", json!({"migrationsDir": migrations.to_string_lossy(), "description": "Create widgets", "reversible": true, "schema": ""}));
    let second = tool.ok("migrate.add", json!({"migrationsDir": migrations.to_string_lossy(), "description": "Create gadgets", "reversible": true}));
    let (first_files, second_files) = (strings(&first["files"]), strings(&second["files"]));
    assert_eq!((first_files.len(), second_files.len()), (2, 2));
    std::fs::write(&first_files[0], "CREATE TABLE widgets (id INTEGER PRIMARY KEY, label TEXT);\n").expect("up 1");
    std::fs::write(&first_files[1], "DROP TABLE widgets;\n").expect("down 1");
    std::fs::write(&second_files[0], "CREATE TABLE gadgets (id INTEGER PRIMARY KEY);\n").expect("up 2");
    std::fs::write(&second_files[1], "DROP TABLE gadgets;\n").expect("down 2");
    let request = json!({"target": target, "migrationsDir": migrations.to_string_lossy()});

    let before = tool.ok("migrate.status", request.clone());
    assert_eq!(before["pendingCount"], 2);
    let listed = before["migrations"].as_array().expect("migrations");
    assert_eq!(listed[0]["description"], "create widgets");
    assert_eq!((listed[0]["applied"].clone(), listed[0]["reversible"].clone(), listed[0]["appliedAt"].clone()), (json!(false), json!(true), Value::Null));
    assert!(listed[0]["file"].as_str().is_some_and(|f| f.ends_with(".up.sql")));
    let (v1, v2) = (listed[0]["version"].as_i64().expect("v1"), listed[1]["version"].as_i64().expect("v2"));
    assert!(v1 < v2 && v1.to_string().len() == 14);

    let run = tool.ok("migrate.run", request.clone());
    assert_eq!(run["applied"], json!([v1, v2]));
    assert_eq!(tool.ok("migrate.run", request.clone())["applied"], json!([]), "nothing left to apply");
    let after = tool.ok("migrate.status", request.clone());
    assert_eq!(after["pendingCount"], 0);
    for m in after["migrations"].as_array().expect("migrations") {
        assert_eq!((m["applied"].clone(), m["checksumMatches"].clone()), (json!(true), json!(true)), "{m}");
        assert!(m["appliedAt"].as_str().is_some_and(|t| t.len() == 20 && t.ends_with('Z')), "{m}");
    }
    let widgets = tool.ok("query.execute", json!({"target": target, "sql": "INSERT INTO widgets (label) VALUES ('w1'); SELECT count(*) FROM widgets; SELECT count(*) FROM gadgets"}));
    assert_eq!(widgets["resultSets"][0]["rows"], json!([["1"]]));

    let reverted = tool.ok("migrate.revert", request.clone());
    assert_eq!(reverted["reverted"], v2);
    let status = tool.ok("migrate.status", request.clone());
    assert_eq!(status["pendingCount"], 1);
    assert_eq!(tool.err("query.execute", json!({"target": target, "sql": "SELECT * FROM gadgets"}))["kind"], "Database", "the down migration dropped the table");
    // A file edited after it was applied is reported.
    std::fs::write(&first_files[0], "CREATE TABLE widgets (id INTEGER PRIMARY KEY, label TEXT, extra TEXT);\n").expect("edit");
    let edited = tool.ok("migrate.status", request.clone());
    assert_eq!(edited["migrations"][0]["checksumMatches"], false);
    std::fs::write(&first_files[0], "CREATE TABLE widgets (id INTEGER PRIMARY KEY, label TEXT);\n").expect("restore");
    assert_eq!(tool.ok("migrate.revert", request.clone())["reverted"], v1);
    assert_eq!(tool.ok("migrate.revert", request.clone())["reverted"], Value::Null, "nothing applied any more");
    assert_eq!(tool.err("migrate.status", json!({"target": target, "migrationsDir": dir.join("nope").to_string_lossy()}))["kind"], "Validation");

    // ---- sqlx.status ----
    let krate = dir.join("krate");
    std::fs::create_dir_all(krate.join("src")).expect("src");
    std::fs::write(krate.join("Cargo.toml"), "[package]\nname = \"krate\"\nversion = \"0.1.0\"\n").expect("manifest");
    std::fs::write(krate.join("src").join("shop.kbdata"), text).expect("kbdata");
    let stale = tool.ok("sqlx.status", json!({"manifestDir": krate.to_string_lossy()}));
    assert_eq!(stale["stale"], true);
    assert!(stale["reason"].as_str().is_some_and(|r| r.contains(".sqlx")), "{stale}");
    assert_eq!(stale["queryFiles"], 0);
    std::fs::create_dir_all(krate.join(".sqlx")).expect(".sqlx");
    std::fs::write(krate.join(".sqlx").join("query-abc.json"), "{}").expect("query file");
    let fresh = tool.ok("sqlx.status", json!({"manifestDir": krate.to_string_lossy()}));
    assert_eq!((fresh["stale"].clone(), fresh["queryFiles"].clone()), (json!(false), json!(1)));

    // ---- explorer.remove ----
    tool.ok("explorer.remove", json!({"name": "Shop"}));
    assert_eq!(tool.ok("explorer.list", json!({}))["connections"], json!([]));
    let secrets_after = std::fs::read_to_string(dir.join("UserSecrets").join("DataExplorer").join("secrets.json")).expect("secrets file");
    assert!(!secrets_after.contains("ConnectionStrings:Shop") && !secrets_after.contains("S3cr3t"), "{secrets_after}");
    assert_eq!(tool.err("explorer.remove", json!({"name": "Shop"}))["kind"], "Validation");
    assert_eq!(tool.err("connection.test", json!({"target": {"explorer": "Shop"}}))["kind"], "Validation");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_slow_query_neither_blocks_the_others_nor_survives_a_cancel() {
    let dir = scratch("cancel");
    let db = dir.join("slow.db");
    std::fs::write(&db, []).expect("empty database");
    let mut tool = Tool::start(&dir);
    let target = json!({"provider": "sqlite", "connectionString": format!("Data Source={}", db.display())});

    let slow = "WITH RECURSIVE c(x) AS (SELECT 1 UNION ALL SELECT x + 1 FROM c WHERE x < 2000000000) SELECT count(*) FROM c";
    let slow_id = tool.send("query.execute", json!({"target": target, "sql": slow, "timeoutSeconds": 120}));
    std::thread::sleep(Duration::from_millis(500));

    // While it runs, other requests answer.
    let started = Instant::now();
    let ping = tool.call("ping", json!({}));
    assert!(ping.get("result").is_some());
    let quick = tool.ok("query.execute", json!({"target": target, "sql": "SELECT 41 + 1"}));
    assert_eq!(quick["resultSets"][0]["rows"], json!([["42"]]));
    assert!(started.elapsed() < Duration::from_secs(20), "other requests were not blocked: {:?}", started.elapsed());

    // Cancel it.
    let cancel = tool.ok("cancel", json!({"id": slow_id}));
    assert_eq!(cancel["cancelled"], true);
    let response = tool.wait(slow_id, 20);
    assert_eq!(response["error"]["kind"], "Cancelled", "{response}");
    assert_eq!(tool.ok("cancel", json!({"id": slow_id}))["cancelled"], false, "already gone");
    let _ = std::fs::remove_dir_all(&dir);
}

/// PostgreSQL through `KUBUNO_TEST_PG_URL` (skipped when unset).
#[test]
fn postgres_when_configured() {
    let Some(url) = std::env::var("KUBUNO_TEST_PG_URL").ok().filter(|u| !u.is_empty()) else {
        eprintln!("KUBUNO_TEST_PG_URL is not set: PostgreSQL test skipped");
        return;
    };
    let dir = scratch("pg");
    let mut tool = Tool::start(&dir);
    let target = json!({"provider": "postgres", "connectionString": url});
    let tested = tool.ok("connection.test", json!({"target": target}));
    assert!(tested["serverVersion"].as_str().is_some());
    let schema = tool.ok("schema.load", json!({"target": target}));
    assert_eq!(schema["provider"], "postgres");
    assert!(!schema.to_string().contains("pg_catalog"));
    let _ = std::fs::remove_dir_all(&dir);
}

/// MySQL through `KUBUNO_TEST_MYSQL_URL` (skipped when unset).
#[test]
fn mysql_when_configured() {
    let Some(url) = std::env::var("KUBUNO_TEST_MYSQL_URL").ok().filter(|u| !u.is_empty()) else {
        eprintln!("KUBUNO_TEST_MYSQL_URL is not set: MySQL test skipped");
        return;
    };
    let dir = scratch("mysql");
    let mut tool = Tool::start(&dir);
    let target = json!({"provider": "mysql", "connectionString": url});
    if !strings(&tool.ok("ping", json!({}))["providers"]).contains(&"mysql".to_string()) {
        eprintln!("this build has no MySQL driver: test skipped");
        return;
    }
    assert!(tool.ok("connection.test", json!({"target": target}))["serverVersion"].as_str().is_some());
    assert_eq!(tool.ok("schema.load", json!({"target": target}))["provider"], "mysql");
    let _ = std::fs::remove_dir_all(&dir);
}

/// SQL Server through `KUBUNO_TEST_MSSQL_URL` (skipped when unset).
#[test]
fn sql_server_when_configured() {
    let Some(url) = std::env::var("KUBUNO_TEST_MSSQL_URL").ok().filter(|u| !u.is_empty()) else {
        eprintln!("KUBUNO_TEST_MSSQL_URL is not set: SQL Server test skipped");
        return;
    };
    let dir = scratch("mssql");
    let mut tool = Tool::start(&dir);
    let target = json!({"provider": "sqlserver", "connectionString": url});
    if !strings(&tool.ok("ping", json!({}))["providers"]).contains(&"sqlserver".to_string()) {
        eprintln!("this build has no SQL Server driver: test skipped");
        return;
    }
    assert!(tool.ok("connection.test", json!({"target": target}))["serverVersion"].as_str().is_some());
    assert_eq!(tool.ok("schema.load", json!({"target": target}))["provider"], "sqlserver");
    assert_eq!(tool.err("migrate.status", json!({"target": target, "migrationsDir": dir.to_string_lossy()}))["kind"], "Config");
    let _ = std::fs::remove_dir_all(&dir);
}
