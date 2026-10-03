# Kubuno Desktop — Microsoft Store packaging (MSIX)

The Microsoft Store distributes desktop applications as **MSIX**: the Win32 executable
(full trust, *Desktop Bridge*) is wrapped with the **Windows SDK** tools. Packaging does not
depend on any UI framework — it simply wraps the executable.

> Creating the `.msix` (`MakeAppx.exe`) works **on Windows only**.
> Build the executable first: `cargo build --release -p kubuno-desktop-shell`.

## Contents

| File | Role |
|---|---|
| `AppxManifest.xml` | MSIX manifest (identity, capabilities, tiles) |
| `Assets/` | Logos at the Store sizes (44, 71, 150, 310, wide, splash, StoreLogo) |
| `package-msix.ps1` | Assembles the layout and runs `makeappx pack` (plus optional `signtool`) |
| `icons/` | Application icons (ico and png) |

## 1. Reserve the app in Partner Center

On https://partner.microsoft.com, reserve the name, then note under **Product identity**:

- `Package/Identity/Name` (for example `1234Kubuno.KubunoDesktop`)
- `Package/Identity/Publisher` (for example `CN=ABCD1234-...`)

Copy both values **exactly** into `AppxManifest.xml` (the Store rejects the package
otherwise). Increase `Version` (`1.0.0.0`, the fourth field always `0`) on every upload.

## 2. Build the MSIX (on Windows)

Requirement: the **Windows 10/11 SDK** (provides `makeappx.exe` and `signtool.exe`).

```powershell
# Unsigned package, ready for the Store (the Store signs it):
pwsh ./package-msix.ps1

# OR a signed package to install and test locally (self-signed certificate):
pwsh ./package-msix.ps1 -Sign -Thumbprint <certificate-thumbprint>
```

The script produces `Kubuno-Desktop.msix`.

### Build the executable, then package

```powershell
cargo build --release -p kubuno-desktop-shell   # -> target\release\kubuno-desktop.exe
pwsh ./package-msix.ps1 -ExePath ..\target\release\kubuno-desktop.exe
```

## 3. Submit

Upload the **unsigned** `.msix` to Partner Center (Packages). The Store handles signing and
distribution.

## Notes

- **No runtime dependency** — the app is native Win32 (Direct2D / DirectWrite /
  DirectComposition, all provided by Windows). No WebView2, no redistributable to bundle.
- **Capabilities** — `runFullTrust` (a normal Win32 process, with file and network access)
  and `internetClient`. Syncing an arbitrary folder works thanks to full trust; if you target
  a folder outside the user profile and the Store asks for it, add the restricted
  capability `broadFileSystemAccess` (with a justification).
- **Architecture** — x64 (`ProcessorArchitecture="x64"`). For ARM64, rebuild for the
  `aarch64-pc-windows-msvc` target and duplicate the manifest.

## Outside the Store (direct distribution)

Two options: distribute the **signed** `.msix` (installed with a double-click), or just the
executable — it needs no runtime. A classic installer (NSIS/WiX) is still to be written for
shortcuts and an entry in "Add or remove programs".
