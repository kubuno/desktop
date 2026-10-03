<!--
  SPDX-FileCopyrightText: 2026 Kubuno contributors
  SPDX-License-Identifier: AGPL-3.0-or-later
-->

<div align="center">

<img src=".github/logo.svg" alt="Kubuno Desktop logo" width="120">

# Kubuno — Desktop

[![License: AGPL v3](https://img.shields.io/badge/License-AGPL_v3-blue.svg)](LICENSE)
![Rust](https://img.shields.io/badge/Rust-edition_2021-orange.svg)
![Windows](https://img.shields.io/badge/Windows-native_Win32-0078D4.svg)
![Sync](https://img.shields.io/badge/sync-Linux_%7C_Windows_%7C_macOS-4D38DB.svg)
![Status](https://img.shields.io/badge/status-alpha-yellow.svg)

**Desktop clients and the offline-first file synchronisation engine of [Kubuno](https://github.com/kubuno/core) — the self-hosted, libre (AGPLv3) cloud platform, a sovereign alternative to Google Workspace and Microsoft 365.**

A cross-platform sync daemon, and native desktop applications that draw every pixel
themselves — no web view, no bundler, no Node.

</div>

---

## What's inside

The repository is organised by platform, around a common foundation:

```
desktop/
├── common/     shared by every desktop version
│   ├── kubuno-sync/   file synchronisation engine (pure Rust, cross-platform)
│   └── assets/        Kubuno and application logos
├── windows/    the Windows version (the only shell written so far)
│   ├── src/shell/        kubuno-desktop.exe — launcher, accounts, sync, settings
│   ├── src/drive/        drive.exe — the Kubuno Drive file manager
│   ├── src/chat/         kubuno-chat — two-pane messaging
│   ├── src/documents/    kubuno-documents — word processor for the Office module
│   ├── src/crates/       kubuno-ui (design system) + kubuno-controls (controls)
│   └── packaging/        Microsoft Store (MSIX)
├── linux/      shell to be written
└── macos/      shell to be written
```

## Features

### Sync engine — `common/kubuno-sync`

A file synchronisation engine usable **as a library and as a CLI daemon**, on Linux,
Windows and macOS:

- **Bidirectional** — every sync runs **push then pull**. Local creates, edits and
  deletions are detected by comparing on-disk content hashes with the stored etags.
- **Offline outbox** — local operations are recorded in a persistent outbox and
  replayed when the server is reachable again.
- **Safe conflicts** — an edit is sent with `If-Match: <etag>`; if the server changed
  meanwhile, the local copy is renamed `… (conflit <host> <ts>)` (never overwritten),
  the server version is restored, then the conflict copy is uploaded as a new file.
- **Resumable pull** — server changes come from a monotonic cursor, files are fetched
  only when their etag changed, deletions propagate through tombstones, and the cursor
  is saved after every page.
- **Real time** — the `watch` mode combines a filesystem watcher, a WebSocket
  change trigger and a polling fallback.
- **Several instances** — each account has its own server, credentials, folder and
  local state; a folder can be moved without losing it.

```
kubuno-sync
├── api      auth (native refresh-token flow) + delta + download + content/upload/trash
├── store    local SQLite: cursor, folder tree (id→path), file index (id, etag), outbox
├── push     detect local changes → outbox → drain to server (If-Match conflicts)
├── engine   pull delta → apply (folders → files → tombstones) into the sync folder
├── ws       WebSocket listener → real-time remote-change trigger
└── daemon   `watch`: FS watcher + WebSocket + poll fallback → auto push + pull
```

### Windows desktop — `windows/`

Native **Win32 + Direct2D / DirectWrite / DirectComposition** applications sharing one
design system (`kubuno-ui`), so they share one palette, one set of shape tokens, one
icon set and one set of controls:

- **Kubuno Desktop** (`kubuno-desktop.exe`) — opens on an **application launcher** with
  one tile per app of the connected server, drawn with each module's own logo; a tile
  opens its app in the browser. Pages for **accounts** (several instances side by side),
  **activity** (what the sync loop has been doing) and **settings** (theme, sync
  interval, notifications, start with Windows, forced offline mode, outbound proxy).
  It embeds the sync engine and runs it in the background, with a system-tray menu
  (*Sync now / Open folder / Show / Quit*).
- **Explorer integration, without admin rights** — a Cloud Files API sync root with
  native status overlays (in sync, syncing) and a *Status* column, a navigation-pane
  entry per instance, and on-demand files downloaded on first access.
- **Kubuno Drive** (`drive.exe`) — a native Windows file manager with tabs, views and
  settings, sharing the Kubuno component library with the other apps. It is a Rust port of the MIT-licensed
  *Files* project; credits and architecture notes are in
  [`windows/src/drive/README.md`](windows/src/drive/README.md).
- **Kubuno Chat** and **Kubuno Documents** — native two-pane messaging, and a native
  word processor for the Office module.

## Usage (sync daemon)

```bash
cd common && cargo build --release -p kubuno-sync

# Connect and choose the local sync folder
./target/release/kubuno-sync login \
  --server https://cloud.example.com \
  --login you@example.com \
  --password '••••••••' \
  --folder ~/Kubuno

./target/release/kubuno-sync sync                 # once: push local edits, then pull
./target/release/kubuno-sync watch --interval 30  # continuously
./target/release/kubuno-sync status               # server, folder, cursor
./target/release/kubuno-sync move --id <instance> --to <new-folder>   # relocate a folder
```

Configuration and state live under the OS configuration directory
(`~/.config/kubuno-desktop` on Linux, `~/Library/Application Support` on macOS,
`%APPDATA%` on Windows). The daemon signs in as a desktop client and keeps a rotating
refresh token (file mode `0600`).

## Build & packaging

`kubuno-sync` is pure Rust: TLS uses **rustls** and SQLite is **vendored**, so there is
no system OpenSSL or SQLite dependency and builds are identical across platforms.

| Artifact | Platform | Produced by |
|---|---|---|
| `kubuno-sync` `.deb`, `.rpm` | Linux | `common/build_deb.sh` (`cargo deb` / `cargo generate-rpm`) |
| `kubuno-sync` `.exe` (zip) | Windows | `cargo build` + zip |
| `kubuno-sync` binaries (zip) | macOS (Apple Silicon and Intel) | `cargo build` per target |
| `kubuno-desktop` `.exe` / MSIX | Windows 10/11 | `windows/` + `windows/packaging/package-msix.ps1` |

On a `v*` tag, CI builds every target on its **native runner** and attaches the
artifacts to a GitHub Release (`release.yml` for the sync daemon, `app-release.yml`
for the Windows shell).

Windows shell, from `windows/`:

```bash
cargo build --release -p kubuno-desktop     # → target/release/kubuno-desktop.exe
cargo run   -p kubuno-ui --example gallery  # the component gallery (UI reference)
cargo build --release -p drive-app          # → target/release/drive.exe
pwsh ./tools/stage-runtime.ps1 -Profile release   # puts each program's kubuno_ui-<hash>.dll next to it
```

The full guide — per-machine build directory, memory-constrained builds, Microsoft
Store submission — is in **[`BUILD.md`](BUILD.md)**.

## Roadmap

- Shells for Linux and macOS (only the Windows shell exists today).
- New local folders created on the server (today only files in known folders are pushed).
- Server-side idempotency for drive writes, so a retried create cannot duplicate.
- Code signing for the MSIX package.
- OS keyring for the refresh token.

## Security

Please report vulnerabilities privately — see [`SECURITY.md`](SECURITY.md).

## Contributing

Issues and pull requests are welcome. For any significant change, please open an issue first.

## License

[AGPL-3.0-or-later](LICENSE) © Kubuno contributors. The `windows/src/drive` crates are
MIT-licensed, like the project they are ported from.
