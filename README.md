# Kubuno Desktop

Desktop client for [Kubuno](https://github.com/kubuno/core), the self-hosted cloud platform.

This repository hosts two layers, both implemented:

- **`crates/kubuno-sync`** — a Nextcloud-style file synchronisation engine (Rust),
  usable as a library and as a CLI daemon. Bidirectional (pull + push) with an
  offline outbox, conflict handling, a continuous `watch` mode and a real-time
  WebSocket trigger.
- **`app/`** — a **Tauri** desktop application (window + system tray) that
  embeds the engine as a library. A small UI handles login and manual sync; a
  background thread runs the continuous `watch` loop; the tray offers
  *Sync now / Open folder / Show / Quit*. Builds to `.deb`/AppImage (Linux),
  MSI (Windows) and DMG (macOS).

## Architecture (offline-first)

```
kubuno-sync
├── api      auth (native refresh-token flow) + delta + download + content/upload/trash
├── store    local SQLite: cursor, folder tree (id→path), file index (id, etag), outbox
├── push     detect local changes → outbox → drain to server (If-Match conflicts)
├── engine   pull delta → apply (folders → files → tombstones) into the sync folder
├── ws       WebSocket listener → real-time remote-change trigger
└── daemon   `watch`: FS watcher + WebSocket + poll fallback → auto push+pull
```

The daemon authenticates with `client_type=desktop` to obtain a rotating refresh
token (stored 0600; OS keyring is a follow-up). Each `sync` runs **push then
pull**:

- **Push** detects local creates/modifies/deletes (by comparing on-disk content
  hashes against the stored etags), records them in a persistent **outbox**, and
  replays them to the server. A modify sends `If-Match: <etag>`; if the server
  changed meanwhile the push gets a **412**, the local edit is renamed to
  `… (conflit <host> <ts>)` (never overwritten) and the server version is
  restored by the pull — then the conflict copy is uploaded as a new file. Ops
  that fail (offline) stay in the outbox and are replayed on the next sync.
- **Pull** downloads server changes from a monotonic cursor; files are fetched
  only when their etag changed, deletions propagate via tombstones, and the
  cursor is persisted after every page so an interrupted sync resumes cleanly.

## Usage

```bash
cargo build --release

# Connect and choose the local sync folder
./target/release/kubuno-sync login \
  --server http://localhost:8080 \
  --login admin@kubuno.local \
  --password '••••••••' \
  --folder ~/Kubuno

# Sync once (push local edits, then pull server changes)
./target/release/kubuno-sync sync

# Or run continuously: filesystem watcher + periodic server poll
./target/release/kubuno-sync watch --interval 30

# Show current server, folder and cursor
./target/release/kubuno-sync status
```

Config and state live under the OS config dir (`~/.config/kubuno-desktop` on
Linux, `~/Library/Application Support` on macOS, `%APPDATA%` on Windows).

## Platforms & packaging

`kubuno-sync` is pure Rust and builds for the three desktop OSes. TLS uses
**rustls** and SQLite is **vendored**, so there is no system OpenSSL/sqlite
dependency — builds are uniform across platforms.

| OS | Artifact | Produced by |
|----|----------|-------------|
| Linux | `.deb`, `.rpm` | `cargo deb` / `cargo generate-rpm` |
| Windows | `.exe` (zip) | `cargo build` + zip (MSI installer ships with the Tauri shell) |
| macOS | universal binary (zip) | `cargo build` x86_64 + aarch64 (`.dmg` ships with the Tauri shell) |

The release CI (`.github/workflows/release.yml`) builds every target on its
**native runner** (a `.dmg`/`.msi` cannot be produced from Linux) and attaches
the artifacts to a GitHub Release on a `v*` tag.

Build a Linux package locally:

```bash
cargo install cargo-deb cargo-generate-rpm
cargo deb -p kubuno-sync                 # → target/debian/kubuno-sync_*.deb
cargo generate-rpm -p crates/kubuno-sync # → target/generate-rpm/*.rpm
```

## Desktop app (GUI)

The Tauri app lives in `app/`. It needs the platform WebView toolchain
(WebKitGTK + GTK on Linux, WebView2 on Windows, WKWebView on macOS) and the
Tauri CLI.

```bash
cargo install tauri-cli --version "^2"
cd app
cargo tauri build                # → target/release/bundle/{deb,appimage,...}
cargo tauri dev                  # run with hot-reload (needs a display)
```

`.github/workflows/app-release.yml` builds the app for all desktop OSes via
`tauri-action` on a `v*` tag (Tauri's bundler produces `.deb`/AppImage, MSI and
DMG natively on each runner).

## Roadmap

- New local folders → create on the server (currently only files in known
  folders are pushed).
- Server-side idempotency for drive writes (so a lost-response retry of a
  create can't duplicate).
- Code-signing / notarisation for the MSI (Windows) and DMG (macOS) installers.
- OS keyring for the refresh token.
