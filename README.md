# Kubuno Desktop — moved to kubuno/core (`desktop/`)

This repository is retired. Kubuno Desktop (the shell, the desktop framework and the common desktop crates) now lives
in the core repository, under [`desktop/`](https://github.com/kubuno/core/tree/main/desktop), with its whole history.

- **Sources**: `desktop/` of [kubuno/core](https://github.com/kubuno/core): one Cargo workspace for every operating
  system, `desktop/common` (the complete, portable app and crates) and `desktop/windows`, `desktop/linux`,
  `desktop/macos` (only what each system does differently). See `desktop/README.md` and `desktop/BUILD.md` there.
- **Crates by git tag**: take them from the core repository, e.g.
  `kubuno-desktop = { git = "https://github.com/kubuno/core", tag = "desktop-v0.1.1-alpha" }`; the web views compiler
  is tagged `web-views-compiler-core-v0.2.1` there. The tags of this repository (`desktop-v0.1.0-alpha`,
  `web-views-compiler-core-v0.1.0`, `web-views-compiler-core-v0.2.0`, `v0.1.0-alpha`) keep resolving here.
- **History**: every commit of this repository is in kubuno/core, its paths under `desktop/`
  (`git log -- desktop/` there); this repository's own history stays below this commit.

Licence: AGPL-3.0-or-later © Kubuno contributors.
