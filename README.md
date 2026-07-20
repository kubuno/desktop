# Kubuno Desktop

Desktop client for [Kubuno](https://github.com/kubuno/core), the self-hosted cloud platform.

This repository hosts two layers, both implemented:

- **`crates/kubuno-sync`** — a Nextcloud-style file synchronisation engine (Rust),
  usable as a library and as a CLI daemon. Bidirectional (pull + push) with an
  offline outbox, conflict handling, a continuous `watch` mode and a real-time
  WebSocket trigger.
- **`app/`** — a **Tauri** desktop application (window + system tray) that
  embeds the engine as a library and grows it into a full desktop shell for
  the platform: an application **launcher**, **native per-app windows**, and a
  **local-first runtime** that downloads the modules' WebAssembly backends and
  runs them on the device, so Kubuno apps keep working entirely offline. A
  background thread runs the continuous `watch` loop; the tray offers
  *Sync now / Open folder / Show / Quit*. Builds to `.deb`/AppImage (Linux),
  MSIX/NSIS (Windows) and DMG (macOS); an Android build is available through
  Tauri's mobile entry point (see `BUILD.md`).

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
| Windows | `.exe` (zip) | `cargo build` + zip (MSIX/NSIS installers ship with the Tauri shell) |
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
DMG natively on each runner). `BUILD.md` documents the other packaging paths:
the Microsoft Store **MSIX** (exe cross-compiled from Linux with `cargo-xwin`,
packaged under Windows), the out-of-store **NSIS** installer, and the
**Android** build (`cargo tauri android init` generates the Gradle project;
`kubuno-sync` compiles for the NDK targets thanks to rustls + bundled SQLite).

### Application launcher

The main window opens on a waffle-style **launcher**: one tile per application
of the connected server, with the same branding and icons as the web shell.
Tiles carry a **local-first status badge** — grey (*backend available for
download*), blue (*installed, initial sync pending*), green (*offline ready*) —
and a context menu to install, update or remove the local backend. Mono-app
modules are grouped under a single alphabetised *Apps* group. Everything the
launcher shows is driven by the server's component manifest and claims: no
tile→component mapping is hardcoded.

### Local-first WASM backends

The desktop can download `<module>-core.wasm` components published by the core
and execute them in an embedded **wasmtime** host. A local proxy fronts every
app window and routes requests by **longest claimed prefix** to the local WASM
backend, falling back to the remote core for everything else. The routing
table is derived entirely from the persisted component manifest
(`components.json`) — a newly published module component routes, syncs and
appears in the launcher **without any desktop code change**. Two guards keep
this safe: a prefix only routes once *primed* (a first full pull has
completed), and mutations are only accepted once a replay loop exists for that
prefix (*pushable*).

Three sync drivers keep local backends and the core converging:

- **Dedicated loops** for the historical pair — office *documents* (delta pull
  with content, granular push, collab-aware) and *drive* files (listings
  served from the local store, outbox replay to the core).
- A **generic entity engine** for everything else (spreadsheets,
  presentations, diagrams, whiteboards, notes, tasks, contacts, jarvis…):
  delta pull → verbatim `_ingest` into the WASM store (idempotent by
  `change_seq`), then replay of a durable local outbox (verbatim
  method/path/body + `Idempotency-Key`, server-wins guard on stale base,
  `_ack` after consumption). The entity list is derived from the manifest's
  `sync` surface (falling back to claims).
- A **blob driver** (`sync_mode: "blob"`) for opaque per-user blobs such as
  keestore's `.kdbx` vault — versioned PUT/GET with conflict detection
  (`X-Sync-Version`, server-wins on 409), enabling offline vault unlock.

The local-first layer is resilient by design: when the core answers **401**
(revoked/expired session) or is unreachable, the proxy serves the cached copy
of cacheable GETs, so locally synced apps keep opening with their full UI
instead of a login screen.

### Native windows & pop-outs

Apps and documents open in dedicated **frameless native windows** with a
custom title bar, served by the local proxy (stable origin, offline-capable).
Every such window is injected with `window.kubunoDesktop.openWindow(route,
label, {width, height})`, so floating windows of the web UI can be detached
into real OS windows — the call goes through the same-origin proxy endpoint,
keeping Tauri IPC away from remote-origin webviews.

### Windows shell integration

On Windows the sync folder integrates with Explorer the way established sync
clients do, without admin rights:

- **Cloud Files API sync root** — native status overlays (✓ in-sync,
  ⟳ syncing) and a *Status* column, registered per user via WinRT's
  `StorageProviderSyncRootManager`.
- **Navigation-pane entry** — each instance's folder appears as a root node in
  Explorer's left pane through a per-user shell namespace extension, with
  stale entries pruned automatically.
- **On-demand ("virtual") files** — content can be downloaded on first access
  instead of eagerly.

## Roadmap

- New local folders → create on the server (currently only files in known
  folders are pushed).
- Server-side idempotency for drive writes (so a lost-response retry of a
  create can't duplicate).
- Code-signing / notarisation for the MSI (Windows) and DMG (macOS) installers.
- OS keyring for the refresh token.
