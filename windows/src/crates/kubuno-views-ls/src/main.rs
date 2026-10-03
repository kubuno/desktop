//! `kubuno-views-ls` — LSP server for `.kbview` XML views, spoken over
//! stdio (the transport VS's `ILanguageClient` and VS Code both expect;
//! `vskubuno/docs/ARCHITECTURE.md`'s phase 3 row). See `lib.rs` for the
//! module map; this binary is a thin wrapper around [`kubuno_views_ls::
//! server::run`].

use lsp_server::Connection;

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Logs must never touch stdout: stdout is the LSP message stream itself,
    // and a stray log line there would corrupt the framing the client is
    // parsing. stderr is free for diagnostics (rust-analyzer does the same).
    tracing_subscriber::fmt().with_writer(std::io::stderr).with_ansi(false).init();

    tracing::info!("kubuno-views-ls starting (stdio)");
    let (connection, io_threads) = Connection::stdio();

    let result = kubuno_views_ls::server::run(&connection);

    // The writer thread only ends once every sender of the outgoing channel
    // is gone, and `connection` holds one: without this drop, `join` below
    // blocks forever after `exit` or stdin EOF, and the process outlives its
    // client (seen live: kubuno-views-ls kept running after Visual Studio
    // closed, locking the extension's own tools folder).
    drop(connection);

    // Always attempt to join the I/O threads, even if `run` returned an
    // error, so a malformed shutdown does not also leak the reader/writer
    // threads — then surface whichever error actually happened first.
    io_threads.join()?;
    result?;

    tracing::info!("kubuno-views-ls exiting");
    Ok(())
}
