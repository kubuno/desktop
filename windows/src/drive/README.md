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
| `crates/kubuno-drive-desktop` | The application: window, tabs, views, actions, settings |
| `crates/kubuno-drive-desktop-app-controls` | Custom Direct2D controls |
| `crates/kubuno-drive-desktop-app-storage` | Shell/storage layer (IShellItem, IFileOperation, watchers, FTP) |
| `crates/kubuno-drive-desktop-core-storage` | Storage model traits (OwlCore.Storage model) |
| `crates/kubuno-drive-desktop-localization` | 49 cultures ported from the original `.resw` resources |
| `crates/kubuno-drive-desktop-shared` | Shared helpers |

`suppa/` vendors the `suppaftp` crate. The binary is `drive.exe`. These crates are
members of the desktop's Windows workspace (`windows/Cargo.toml`) and link the same component library,
`kubuno-desktop-ui`, as the other Kubuno apps - statically: `drive.exe` needs no DLL beside it.
From `windows/`:

```powershell
cargo build --release -p kubuno-drive-desktop
target\release\drive.exe [folder-to-open]
```
