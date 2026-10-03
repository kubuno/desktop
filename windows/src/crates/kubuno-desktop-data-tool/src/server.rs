//! The JSON-lines protocol over a reader and a writer (stdin/stdout in the real tool).
//!
//! One request per line, one response per line. Requests run concurrently on the data runtime; a
//! single writer thread owns the output so a line is never interleaved with another. `cancel {id}`
//! aborts a running request, whose response is then a `Cancelled` error.

use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tokio::task::AbortHandle;

use crate::ctx::Ctx;
use crate::error::{ToolError, ToolResult};
use crate::explorer::{self, AddRequest, Store};
use crate::home::Home;
use crate::params::{opt_bool, opt_int, req_str, target};
use crate::schema::{self, Filter};
use crate::targets::{open, OpenOptions};
use crate::{kbdata, migrate, query, scripts, sqlxcmd};

/// The tool's version (`--version`, `ping`).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// How long the end of the input waits for running requests before the process exits.
const DRAIN: Duration = Duration::from_secs(5);

/// The providers this build has, in the protocol's order.
pub fn providers() -> Vec<&'static str> {
    use kubuno_desktop_data::Provider;
    [(Provider::Postgres, "postgres"), (Provider::Sqlite, "sqlite"), (Provider::MySql, "mysql"), (Provider::SqlServer, "sqlserver")]
        .into_iter()
        .filter(|(p, _)| p.is_available())
        .map(|(_, name)| name)
        .collect()
}

/// Runs one request to its result.
pub async fn dispatch(home: &ToolResult<Home>, method: &str, params: Value) -> ToolResult<Value> {
    if method == "ping" {
        return Ok(json!({"version": VERSION, "providers": providers()}));
    }
    let ctx = Ctx::new(home.clone()?);
    let outcome = dispatch_with(&ctx, method, &params).await;
    // Whatever the request touched, no secret leaves in an error message.
    outcome.map_err(|e| ctx.redactor.error(e))
}

async fn dispatch_with(ctx: &Ctx, method: &str, params: &Value) -> ToolResult<Value> {
    match method {
        "connection.test" => query::connection_test(ctx, params).await,
        "explorer.list" => Ok(json!({"connections": explorer::list(&ctx.home)?})),
        "explorer.add" => {
            let req: AddRequest = serde_json::from_value(params.clone()).map_err(|e| ToolError::validation(format!("invalid explorer.add parameters: {e}")))?;
            let entry = explorer::add(&ctx.home, &req, |s| ctx.redactor.add(s))?;
            Ok(json!({"connection": entry}))
        }
        "explorer.remove" => {
            explorer::remove(&ctx.home, req_str(params, "name")?)?;
            Ok(json!({}))
        }
        "secrets.copyToProject" => {
            let store = Store::parse(req_str(params, "store")?)?;
            explorer::copy_to_project(&ctx.home, req_str(params, "explorer")?, req_str(params, "userSecretsId")?, req_str(params, "key")?, store, |s| ctx.redactor.add(s))?;
            Ok(json!({}))
        }
        "schema.load" => schema_load(ctx, params).await,
        "data.top" => query::data_top(ctx, params).await,
        "query.execute" => query::execute(ctx, params).await,
        "script.generate" => scripts::generate(ctx, params).await,
        "kbdata.build" => kbdata::build_request(ctx, params).await,
        "kbdata.read" => kbdata::read(params),
        "migrate.add" => migrate::add(params),
        "migrate.status" => migrate::status(ctx, params).await,
        "migrate.run" => migrate::run(ctx, params).await,
        "migrate.revert" => migrate::revert(ctx, params).await,
        "sqlx.status" => sqlxcmd::status(params),
        "sqlx.prepare" => sqlxcmd::prepare(ctx, params).await,
        other => Err(ToolError::protocol(format!("unknown method `{other}`"))),
    }
}

async fn schema_load(ctx: &Ctx, params: &Value) -> ToolResult<Value> {
    let include_system = opt_bool(params, "includeSystem", false)?;
    let connect = opt_int(params, "timeoutSeconds", 15, 1, 300)? as u32;
    let session = open(ctx, &target(params)?, OpenOptions { connect_timeout_secs: connect, ..OpenOptions::default() })?;
    let database = session.resolved.info.database_name();
    let info = schema::load(&session.handle, session.provider(), &Filter::default(), include_system, &database).await?;
    serde_json::to_value(info).map_err(|e| ToolError::new("Io", format!("cannot serialize the schema: {e}")))
}

fn response_ok(id: &Value, result: Value) -> String {
    json!({"id": id, "result": result}).to_string()
}

fn response_err(id: &Value, e: &ToolError) -> String {
    json!({"id": id, "error": {"kind": e.kind, "message": e.message}}).to_string()
}

type InFlight = Arc<Mutex<HashMap<String, AbortHandle>>>;

/// Serves requests from `input` until it ends, writing responses to `output`.
pub fn serve(mut input: impl BufRead, output: impl Write + Send + 'static, home: ToolResult<Home>) -> ToolResult<()> {
    let runtime = kubuno_desktop_data::rt::runtime()?;
    let (tx, rx) = std::sync::mpsc::channel::<String>();
    let writer = std::thread::spawn(move || {
        let mut out = output;
        for line in rx {
            if out.write_all(line.as_bytes()).and_then(|()| out.write_all(b"\n")).and_then(|()| out.flush()).is_err() {
                break;
            }
        }
    });
    let inflight: InFlight = Arc::new(Mutex::new(HashMap::new()));
    let pending = Arc::new(AtomicUsize::new(0));
    let send = |tx: &Sender<String>, line: String| {
        if tx.send(line).is_err() {
            tracing::error!("the output is closed");
        }
    };

    let mut line = String::new();
    loop {
        line.clear();
        match input.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {}
            Err(e) => {
                tracing::error!(error = %e, "cannot read the input");
                break;
            }
        }
        let text = line.trim();
        if text.is_empty() {
            continue;
        }
        let request: Value = match serde_json::from_str(text) {
            Ok(v) => v,
            Err(e) => {
                send(&tx, response_err(&Value::Null, &ToolError::protocol(format!("the request is not valid JSON: {e}"))));
                continue;
            }
        };
        let id = request.get("id").cloned().unwrap_or(Value::Null);
        let Some(method) = request.get("method").and_then(Value::as_str).map(str::to_string) else {
            send(&tx, response_err(&id, &ToolError::protocol("the request has no `method`")));
            continue;
        };
        let params = request.get("params").cloned().unwrap_or_else(|| json!({}));
        tracing::debug!(method = %method, "request");

        if method == "cancel" {
            let key = params.get("id").map(Value::to_string).unwrap_or_default();
            let handle = inflight.lock().unwrap_or_else(PoisonError::into_inner).remove(&key);
            let cancelled = handle.is_some();
            if let Some(h) = handle {
                h.abort();
            }
            send(&tx, response_ok(&id, json!({"cancelled": cancelled})));
            continue;
        }

        let key = id.to_string();
        if inflight.lock().unwrap_or_else(PoisonError::into_inner).contains_key(&key) {
            send(&tx, response_err(&id, &ToolError::protocol("a request with this id is already running")));
            continue;
        }
        let task_home = home.clone();
        let job = runtime.spawn(async move { dispatch(&task_home, &method, params).await });
        inflight.lock().unwrap_or_else(PoisonError::into_inner).insert(key.clone(), job.abort_handle());
        pending.fetch_add(1, Ordering::SeqCst);
        let (tx2, inflight2, pending2) = (tx.clone(), inflight.clone(), pending.clone());
        runtime.spawn(async move {
            let outcome = job.await;
            inflight2.lock().unwrap_or_else(PoisonError::into_inner).remove(&key);
            let response = match outcome {
                Ok(Ok(result)) => response_ok(&id, result),
                Ok(Err(e)) => response_err(&id, &e),
                Err(join) if join.is_cancelled() => response_err(&id, &ToolError::cancelled()),
                Err(join) => {
                    tracing::error!(error = %join, "a request panicked");
                    response_err(&id, &ToolError::new("Internal", "the request failed unexpectedly"))
                }
            };
            if tx2.send(response).is_err() {
                tracing::error!("the output is closed");
            }
            pending2.fetch_sub(1, Ordering::SeqCst);
        });
    }

    // End of input: let the running requests answer, briefly.
    let deadline = Instant::now() + DRAIN;
    while pending.load(Ordering::SeqCst) > 0 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    for (_, h) in inflight.lock().unwrap_or_else(PoisonError::into_inner).drain() {
        h.abort();
    }
    drop(tx);
    let _ = writer.join();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    /// A writer whose content the test can read after `serve` returns.
    #[derive(Clone, Default)]
    struct Shared(Arc<Mutex<Vec<u8>>>);

    impl Write for Shared {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap_or_else(PoisonError::into_inner).extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn run(input: &str) -> Vec<Value> {
        let out = Shared::default();
        let home = Ok(Home::at(std::env::temp_dir().join(format!("kubuno-data-tool-server-{}", std::process::id()))));
        serve(Cursor::new(input.to_string()), out.clone(), home).expect("serve");
        let bytes = out.0.lock().unwrap_or_else(PoisonError::into_inner).clone();
        String::from_utf8(bytes).expect("utf8").lines().map(|l| serde_json::from_str(l).expect("json line")).collect()
    }

    #[test]
    fn ping_and_protocol_errors() {
        let out = run("{\"id\":1,\"method\":\"ping\"}\nnot json\n{\"id\":3}\n{\"id\":4,\"method\":\"nope\",\"params\":{}}\n\n");
        assert_eq!(out.len(), 4, "{out:?}");
        let by_id = |id: Value| out.iter().find(|r| r["id"] == id).cloned().expect("response");
        let ping = by_id(json!(1));
        assert_eq!(ping["result"]["version"], VERSION);
        assert!(ping["result"]["providers"].as_array().is_some_and(|p| p.iter().any(|x| x == "sqlite")));
        assert_eq!(by_id(Value::Null)["error"]["kind"], "Protocol");
        assert_eq!(by_id(json!(3))["error"]["kind"], "Protocol");
        assert_eq!(by_id(json!(4))["error"]["message"], "unknown method `nope`");
    }

    #[test]
    fn string_ids_are_echoed_and_missing_params_are_validation_errors() {
        let out = run("{\"id\":\"abc\",\"method\":\"schema.load\",\"params\":{}}\n");
        assert_eq!(out[0]["id"], "abc");
        assert_eq!(out[0]["error"]["kind"], "Validation");
    }

    #[test]
    fn cancel_of_an_unknown_request_says_so() {
        let out = run("{\"id\":9,\"method\":\"cancel\",\"params\":{\"id\":123}}\n");
        assert_eq!(out[0]["result"]["cancelled"], false);
    }
}
