//! Real-time remote trigger over the core WebSocket.
//!
//! Runs in its own thread (blocking `tungstenite`). It connects to
//! `ws(s)://host/ws?token=<access>` and, on a drive change event, signals the
//! daemon to pull immediately — so a change on another device shows up in
//! seconds instead of waiting for the periodic poll. The poll stays as a
//! fallback for when the socket is down.
//!
//! The access token is borrowed from the process's token provider (the shell's
//! token owner, or its broker); on any socket error we back off and reconnect
//! with a current token.

use std::sync::mpsc::Sender;
use std::time::Duration;

/// Spawns the listener thread. `id` selects the instance whose token to use;
/// `tx` is the daemon's wake channel.
pub fn spawn_listener(id: String, server_url: String, tx: Sender<()>) {
    std::thread::spawn(move || loop {
        let mut wait = Duration::from_secs(5);
        if let Err(e) = run(&id, &server_url, &tx) {
            if crate::daemon::is_session_over(&e) {
                // The session ended: nothing to listen to until the user signs in again (checked locally, no
                // network), so look less often and stay quiet.
                wait = Duration::from_secs(60);
            } else {
                eprintln!("  websocket : {e} (reconnexion dans 5 s)");
            }
        }
        std::thread::sleep(wait);
    });
}

fn run(id: &str, server_url: &str, tx: &Sender<()>) -> anyhow::Result<()> {
    let token = crate::api::Api::new(id.to_string(), server_url.to_string()).access_token()?;
    let url = ws_url(server_url, &token);
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
