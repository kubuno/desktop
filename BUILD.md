# Kubuno Desktop — guide de build

## Organisation du dépôt

Le code est rangé par plateforme, avec un socle commun :

```
desktop/
├── common/     ← éléments communs à toutes les versions desktop
│   ├── kubuno-sync/   moteur de synchro (Rust pur, cross-plateforme)
│   └── assets/        logo, ressources partagées
├── windows/    ← la version Windows (la seule écrite aujourd'hui)
│   ├── src/           sources : shell, chat, documents, drive, crates UI
│   ├── packaging/     MSIX (Microsoft Store)
│   └── Cargo.toml     workspace Windows
├── linux/      ← à écrire
└── macos/      ← à écrire
```

`windows/`, `linux/` et `macos/` contiennent chacun le code, le build et les
assets propres à leur coque ; `common/` porte ce qui est partagé. La coque est
**native Win32 + Direct2D** : ni WebView2, ni Node, ni bundler (Tauri a été
retiré le 2026-08-17).

## Windows

Depuis `windows/` :

```bash
cargo build --release -p kubuno-desktop-shell     # → target/release/kubuno-desktop.exe (la coque)
cargo test  -p kubuno-desktop-shell               # géométrie d'interaction, champs de saisie
cargo run   -p kubuno-desktop-ui --example gallery  # galerie des composants (référence UI)
cargo build --release -p kubuno-drive-desktop          # → target/release/drive.exe (explorateur Drive)
```

### Static linking: every exe is self-contained

Every program of the workspace (shell, chat, documents, drive, the gallery, the
tools) links the design system (`kubuno-desktop-ui`, with the host `kubuno-desktop-controls` and
the painting surface `kubuno-drive-desktop-app-controls`) and Rust's `std` **statically**: an
exe runs from a folder that holds only itself — no `kubuno_desktop_ui` DLL, no
`std-*.dll`, nothing to stage after a build. `kubuno-desktop-ui` is an ordinary rlib and
the workspace sets no `rustflags` (`windows/.cargo/config.toml`).

Why (product decision of 2026-10-03): the apps will be released from their own
per-module repositories, each on its own schedule, so no Rust DLL may be shared
between them (a Rust dylib has no stable ABI: it would tie every app to one
build of it). Kubuno Desktop (the shell) stays mandatory on every PC, but as a
**service** dependency — the account/token broker over its named pipe, the sync,
the launcher — never as a binary one.

What this replaced: until 2026-10-02 `kubuno-desktop-ui` was a Rust `dylib`
(`kubuno_desktop_ui-<hash>.dll`, one file name per build, with a link shim in its
`build.rs`), the workspace was linked with `-C prefer-dynamic`, and
`tools/stage-runtime.ps1` copied the DLLs next to each exe. All of that is gone.
The global state the framework keeps (input queue, focus ring, floating
surfaces) still exists exactly once per process, since a program links one copy
of the crate. Cost: each exe carries its own copy of the framework (see the
CHANGELOG for sizes).

```powershell
pwsh ./tools/build-all.ps1 -Profile release   # every app and example in one cargo run
```

Les exécutables, au-dessus du socle partagé (`windows/src/crates` +
`windows/src/drive/crates/kubuno-drive-desktop-app-controls`, linked statically into each exe) :

| Exécutable | Crate | Rôle |
|---|---|---|
| `kubuno-desktop.exe` | `src/shell/` | coque : lanceur, comptes, activité, réglages, synchro, Explorateur |
| `drive.exe` | `src/drive/crates/kubuno-drive-desktop` | explorateur de fichiers Kubuno Drive |
| `kubuno-chat.exe`, `kubuno-documents.exe` | `src/chat/`, `src/documents/` | chat, traitement de texte |

> **target-dir** : le dépôt vit souvent sur un partage réseau (Z:), où le lien
> MSVC échoue à écrire un PDB (LNK1201). Ne PAS committer un chemin de build
> dans le dépôt — le régler **par machine** :
> `setx CARGO_TARGET_DIR C:\kubuno-build\desktop-target` (ou dans
> `%USERPROFILE%\.cargo\config.toml`). Ailleurs, le `target/` local suffit.

Sur une machine à mémoire limitée, compiler la crate `windows` en séquentiel
(`-j 1`) : en parallèle elle épuise la mémoire et le lien échoue.

### Microsoft Store (MSIX)

L'empaquetage vit dans **`windows/packaging/`** (manifeste, logos Store, script).
`MakeAppx.exe` étant Windows-only, l'empaquetage se fait sous Windows avec le
SDK Windows 10/11 :

```powershell
cargo build --release -p kubuno-desktop-shell        # depuis windows/
cd packaging
pwsh ./package-msix.ps1 -ExePath ..\target\release\kubuno-desktop.exe
#   → Kubuno-Desktop.msix (non signé, pour envoi au Store)
pwsh ./package-msix.ps1 -Sign -Thumbprint <empreinte>   # pour installer localement
```

Avant un envoi au Store : reprendre dans `AppxManifest.xml` les valeurs
`Identity/Name` et `Identity/Publisher` réservées dans Partner Center, et
incrémenter `Identity/Version` (le 4ᵉ composant doit rester à 0).

## Le démon de synchro (commun)

`common/kubuno-desktop-sync` est du Rust pur (rustls + SQLite embarqué) et compile sur
les trois OS de bureau. Depuis `common/` :

```bash
cargo build --release -p kubuno-desktop-sync
bash build_deb.sh            # → .deb + .rpm (Linux)
```

## Socle de la synchro hors ligne (commun)

Quatre crates de `common/` portent la synchro hors ligne des données (vskubuno
`docs/DESKTOP-OFFLINE-SYNC.md`, lots SE-0 à SE-3) ; aucune ne dépend de l'UI :

| Crate | Rôle |
|---|---|
| `kubuno-desktop-secrets` | magasin d'identifiants de l'OS (Gestionnaire d'identification Windows, Trousseau macOS, Secret Service Linux) |
| `kubuno-desktop-api-client` | client HTTP typé de l'API (Kubuno Delta Protocol v1, en-têtes `If-Match`/`Idempotency-Key`, reprises) |
| `kubuno-desktop-account` | comptes (serveur + id utilisateur), propriétaire des jetons, courtier de jetons (tube nommé / socket Unix), migration de `creds.json` |
| `kubuno-desktop-sync-engine` | base locale SQLite/SQLCipher par compte et par appli, outbox, flux, conflits, planificateur |

```bash
cargo test -p kubuno-desktop-secrets -p kubuno-desktop-api-client -p kubuno-desktop-account
cargo test -p kubuno-desktop-sync-engine                        # avec SQLCipher (défaut)
cargo test -p kubuno-desktop-sync-engine --no-default-features  # SQLite en clair, sans OpenSSL
```

### SQLCipher : prérequis de build

`kubuno-desktop-sync-engine` active par défaut la fonctionnalité `sqlcipher` : la base locale
est chiffrée (AES-256, clé par compte dans le magasin de l'OS). Cargo ne lie qu'un
seul `libsqlite3-sys` par build : SQLCipher remplace donc aussi le SQLite de
`rusqlite` (kubuno-sync) dans tout exécutable qui lie le moteur ; sans clé, il lit
et écrit les bases en clair comme avant.

| OS | Fonctionnalité de `libsqlite3-sys` | Prérequis |
|---|---|---|
| Windows (MSVC) | `bundled-sqlcipher-vendored-openssl` | un **Perl natif** (Strawberry Perl ; le Perl de Git Bash/MSYS ne suffit pas : `Locale::Maketext::Simple` manquant, chemins MSYS) via `PERL=C:\…\perl.exe` ou dans le `PATH` ; **NASM** fortement recommandé (sans lui, `openssl-src` ajoute `no-asm` : pas d'AES-NI, chiffrement 3 à 4 fois plus lent) ; les outils MSVC (déjà requis) |
| Linux | `bundled-sqlcipher-vendored-openssl` | `perl` et `make` (présents sur les images de build usuelles) |
| macOS | `bundled-sqlcipher` | rien : SQLCipher utilise CommonCrypto (framework Security) — se compile sur un Mac (le SDK Apple est nécessaire : pas de contrôle croisé depuis Windows) |

Tout est hors ligne une fois les crates vendorisées (`cargo vendor`) : les sources
d'OpenSSL viennent de la crate `openssl-src`. Premier build d'OpenSSL : ~12 min sur
la VM de dev (nmake est séquentiel), puis en cache dans `target/`. Coût mesuré
(spike SE-0, Windows, NASM) : +4,3 Mo par exécutable ; écritures +3 %, lectures
ponctuelles +2 %, parcours complets +70 % avec le cache de pages de 32 Mo que pose
le moteur (+300 % avec le cache par défaut).

En CI : Strawberry Perl est préinstallé sur les runners GitHub Windows ; NASM ne
l'est pas (l'ajouter à l'image, ou accepter `no-asm`).

## Coques Linux et macOS

À écrire (`linux/`, `macos/`). Aujourd'hui seule la coque graphique Windows
existe (Win32/Direct2D) ; le socle commun (`common/`) est déjà cross-plateforme.
Pour Android, ce dépôt n'est plus concerné : les applications mobiles sont
natives et vivent dans leur propre dépôt.
