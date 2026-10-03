# Kubuno Drive — desktop file manager

The desktop app of Kubuno Drive: a native Windows file manager built in Rust
with a pure Win32 UI (Direct2D / DirectWrite / DirectComposition, Mica
backdrop) — no web view, no UI framework.

## Credits

This code base is a Rust port of, and is heavily inspired by,
**[Files](https://github.com/files-community/Files)** (files-community, MIT
license) — the modern file manager for Windows. The application structure,
views, controls, vector icon geometries, localization strings and behaviours
mirror the original C#/WinUI 3 implementation; `docs/ARCHITECTURE.md` maps
each Rust module to its C# counterpart, and `docs/PORTING.md` is the detailed
porting journal. Huge thanks to the Files community for their outstanding
work.

The crates are licensed MIT, like the original project.

## Layout

| Crate | Role |
|---|---|
| `crates/drive-app` | The application: window, tabs, views, actions, settings |
| `crates/drive-app-controls` | Custom Direct2D controls |
| `crates/drive-app-storage` | Shell/storage layer (IShellItem, IFileOperation, watchers, FTP) |
| `crates/drive-core-storage` | Storage model traits (OwlCore.Storage model) |
| `crates/drive-localization` | 49 cultures ported from the original `.resw` resources |
| `crates/drive-shared` | Shared helpers |

`suppa/` vendors the `suppaftp` crate. The binary is `drive.exe`. These crates are
members of the desktop's Windows workspace (`windows/Cargo.toml`), so Drive loads
the same shared component library, `kubuno_ui-<hash>.dll` (one file name per build), as the other Kubuno apps.
From `windows/`:

```powershell
cargo build --release -p drive-app
pwsh ./tools/stage-runtime.ps1 -Profile release   # its kubuno_ui-<hash>.dll + Rust's std DLL beside the exe
target\release\drive.exe [folder-to-open]
```
