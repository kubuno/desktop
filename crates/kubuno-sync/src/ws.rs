//! Real-time remote trigger over the core WebSocket.
//!
//! Runs in its own thread (blocking `tungstenite`). It connects to
//! `ws(s)://host/ws?token=<access>` and, on a drive change event, signals the
//! daemon to pull immediately — so a change on another device shows up in
//! seconds instead of waiting for the periodic poll. The poll stays as a
//! fallback for when the socket is down.
//!
//! The access token is read from `creds.json`, which the main sync thread keeps
//! refreshed; on any socket error we back off and reconnect with the latest
//! token.

use std::sync::mpsc::Sender;
use std::time::Duration;

use crate::config::Creds;

/// Spawns the listener thread. `id` selects the instance whose token to use;
/// `tx` is the daemon's wake channel.
pub fn spawn_listener(id: String, server_url: String, tx: Sender<()>) {
    std::thread::spawn(move || loop {
        if let Err(e) = run(&id, &server_url, &tx) {
            eprintln!("  websocket : {e} (reconnexion dans 5 s)");
        }
        std::thread::sleep(Duration::from_secs(5));
    });
}

fn run(id: &str, server_url: &str, tx: &Sender<()>) -> anyhow::Result<()> {
    let creds = Creds::load(id)?;
    let url = ws_url(server_url, &creds.access_token);
    let (mut socket, _resp) = tungstenite::connect(&url)?;

    loop {
        match socket.read()? {
            tungstenite::Message::Text(t) => {
                if is_drive_change(&t) {
                    let _ = tx.send(());
                }
            }
            tungstenite::Message::Ping(p) => {
                let _ = socket.send(tungstenite::Message::Pong(p));
            }
            tungstenite::Message::Close(_) => break,
            _ => {}
        }
    }
    Ok(())
}

/// True if the WS message denotes a change to the user's drive.
fn is_drive_change(msg: &str) -> bool {
    msg.contains("drive.changed")
        || msg.contains("\"module_id\":\"drive\"")
        || msg.contains("FileUploaded")
        || msg.contains("FileDeleted")
        || msg.contains("FileMoved")
}

/// Maps the HTTP server URL to the WebSocket URL with the auth token.
fn ws_url(server_url: &str, token: &str) -> String {
    let base = server_url.trim_end_matches('/');
    let ws = if let Some(rest) = base.strip_prefix("https://") {
        format!("wss://{rest}")
    } else if let Some(rest) = base.strip_prefix("http://") {
        format!("ws://{rest}")
    } else {
        base.to_string()
    };
    format!("{ws}/ws?token={token}")
}
