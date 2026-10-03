# Kubuno Desktop — Windows

The Windows version of Kubuno Desktop: native Win32 applications written in Rust,
drawn with Direct2D, DirectWrite and DirectComposition — no web view, no UI
framework, no runtime to install.

It is built on the shared foundation in [`../common`](../common), above all the
[`kubuno-sync`](../common/kubuno-desktop-sync) file synchronisation engine. See the
[repository README](../README.md) for the whole picture and [`BUILD.md`](../BUILD.md)
for build notes common to every platform.

---

## What's inside

```
windows/
├── Cargo.toml            the Windows workspace
├── src/
│   ├── shell/            kubuno-desktop.exe — the desktop shell
│   ├── drive/            drive.exe — the Kubuno Drive file manager (nested workspace)
│   ├── chat/             kubuno-chat — two-pane messaging
│   ├── documents/        kubuno-documents — word processor for the Office module
│   └── crates/
│       ├── kubuno-desktop-controls/   the control library, drawn natively on Direct2D
│       └── kubuno-desktop-ui/         the design system built on those controls
├── packaging/            Microsoft Store (MSIX): manifest, Store logos, packaging script
└── tools/                UI reference and parity tooling (PowerShell)
```

### The shell — `kubuno-desktop.exe`

The launcher that stays with the user: accounts, activity, settings, favourites and
labels, the applications of each Kubuno instance, and file synchronisation driven
by `kubuno-sync`. It integrates with Windows itself:

- **Cloud Files API** — each sync folder is registered as a sync root, so Explorer
  shows the standard in-sync / syncing overlays and a Status column, without
  administrator rights;
- **Explorer navigation pane** — each instance's sync folder appears as a root node
  in Explorer's left pane (a per-user namespace extension under `HKEY_CURRENT_USER`);
- **Administration** — a native port of the web administration console (dashboard,
  users, groups, organisational units, audiences, modules, storage, settings), with
  an "open in the browser" fallback for the sections not ported yet.

### Kubuno Drive — `drive.exe`

A native file manager for Kubuno Drive, with its own nested workspace in
`src/drive` (see [its README](src/drive/README.md)): window, tabs, views and actions
in `kubuno-drive-desktop`, custom Direct2D controls, the shell/storage layer, localisation for
49 cultures. Those crates are MIT-licensed.

### Chat and Documents

- **`kubuno-chat`** — two-pane messaging for the Chat module.
- **`kubuno-documents`** — a native word processor for the Office module's documents.

## Requirements

- Windows 10 or 11, x86-64
- Rust stable with the MSVC toolchain (`x86_64-pc-windows-msvc`)
- For MSIX packaging: the Windows 10/11 SDK (`makeappx.exe`, `signtool.exe`)

## Build

From `windows/`:

```powershell
cargo build --release -p kubuno-desktop-shell     # → target\release\kubuno-desktop.exe (the shell)
cargo test  -p kubuno-desktop-shell               # interaction geometry, text fields
cargo run   -p kubuno-desktop-ui --example gallery  # component gallery (UI reference)

cd src\drive; cargo build --release         # → drive.exe (nested workspace)
```

**Build directory.** When the repository lives on a network share, the MSVC linker
cannot reliably write its PDB there (`LNK1201`). Keep the target directory local,
per machine, rather than in the repository:

```powershell
setx CARGO_TARGET_DIR C:\kubuno-build\desktop-target
```

On a machine with little memory, build the `windows` crate sequentially (`-j 1`):
in parallel it can exhaust memory and the link fails.

## Packaging (MSIX)

Everything lives in [`packaging/`](packaging): the MSIX manifest, the Store logos and
`package-msix.ps1`, which wraps the executable as a full-trust desktop package.
`makeappx.exe` only runs on Windows.

```powershell
cargo build --release -p kubuno-desktop-shell
cd packaging
pwsh ./package-msix.ps1 -ExePath ..\target\release\kubuno-desktop.exe
# or, signed for local installation:
pwsh ./package-msix.ps1 -Sign -Thumbprint <certificate-thumbprint>
```

The script produces `Kubuno-Desktop.msix`. The unsigned package is the one uploaded
to Partner Center, where the Store signs it. The steps to reserve the app and submit
it are in [`packaging/README.md`](packaging/README.md).

## Continuous integration

`.github/workflows/app-release.yml` runs on a Windows runner for every `v*` tag (or
on demand): it tests and builds `kubuno-desktop`, packages the MSIX, and attaches
`kubuno-desktop.exe` and `Kubuno-Desktop.msix` to a draft GitHub Release.

The command-line sync client (`kubuno-sync.exe`) is released separately, as a zip,
by `release.yml`.

## License

[AGPL-3.0-or-later](../LICENSE) © Kubuno contributors. The Drive crates under
`src/drive` are MIT-licensed.
