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
cargo build --release -p kubuno-desktop     # → target/release/kubuno-desktop.exe (la coque)
cargo test  -p kubuno-desktop               # géométrie d'interaction, champs de saisie
cargo run   -p kubuno-ui --example gallery  # galerie des composants (référence UI)
cargo build --release -p drive-app          # → target/release/drive.exe (explorateur Drive)
```

### Bibliothèque de composants partagée : `kubuno_ui-<hash>.dll`

Le design system (`kubuno-ui`, avec l'hôte `kubuno-controls` et la surface de
peinture `drive-app-controls`) est compilé en **DLL Rust** (`dylib`) chargée par
toutes les applis du workspace (coque, chat, documents, drive, galerie). Le workspace
est donc lié avec `-C prefer-dynamic` (`windows/.cargo/config.toml`) : chaque
exe a besoin, à côté de lui (ou dans le PATH), de **sa** `kubuno_ui-<hash>.dll`
**et** de la `std-*.dll` de Rust. Après un build, pour lancer un exe par double-clic :

```powershell
pwsh ./tools/build-all.ps1 -Profile release   # toutes les applis + exemples, puis les DLL à côté
```

**Un nom de fichier par build.** Une DLL Rust n'a pas d'ABI stable : toute
recompilation peut renommer ou modifier les symboles exportés. Chaque build de la
DLL porte donc son propre nom, `kubuno_ui-<16 chiffres hexa>.dll`, et chaque exe
importe exactement celui avec lequel il a été lié. C'est le script de build de
`kubuno-ui` (`windows/src/crates/kubuno-ui/build.rs`) qui s'en charge : il fait
passer l'édition de liens de ce seul paquet par un relais (`link.exe` copié dans
son `OUT_DIR`) qui renomme la sortie, le hash couvrant toutes les entrées de
l'édition de liens. `cargo build`, `cargo run`, `cargo test`, les projets `.rsproj`
de Visual Studio et le concepteur de vues en profitent sans configuration.
Conséquences :

- deux builds coexistent dans un même dossier ou dans le PATH, chaque exe charge
  le sien ; un `cargo build -p <appli>` isolé ne casse plus les autres applis (elles
  gardent leur build, conservé dans `deps` parmi les trois plus récents) mais elles
  ne voient pas la modification : `build-all.ps1` reste la commande à utiliser après
  une modification de `kubuno-ui` ;
- si la DLL d'un exe manque, Windows l'indique sous son nom (« `kubuno_ui-<hash>.dll`
  introuvable », `0xC0000135`) au lieu d'un « point d'entrée introuvable » ;
- `kubuno_ui.dll` (sans hash) n'existe plus que comme alias du dernier build, pour
  Cargo et rustc (métadonnées de la crate) : aucun exe ne l'importe ;
- `stage-runtime.ps1` copie à côté de chaque exe le build qu'il importe (avec son
  PDB) et liste les exes dont le build a quitté `deps` ;
- le code qui doit savoir quel fichier il a chargé appelle
  `kubuno_ui::library::module_path()`.

Pourquoi ce mécanisme plutôt qu'un autre (noms de Cargo, `-C extra-filename`,
bibliothèque d'import régénérée, manifeste side-by-side…) : voir
`vskubuno/docs/DESIGNER.md`, section 16.

(`cargo run` / `cargo test` n'ont besoin d'aucune copie : Cargo met leurs dossiers
dans le PATH.) La DLL et les applis doivent quand même être compilées **ensemble**,
par le même compilateur, pour partager une seule instance de la DLL — c'est le
cas : tout vit dans ce seul workspace (Drive, autrefois workspace à part dans
`src/drive`, l'a rejoint pour partager la même DLL).

Les exécutables, au-dessus du socle partagé (`windows/src/crates` +
`windows/src/drive/crates/drive-app-controls`, réunis dans `kubuno_ui-<hash>.dll`) :

| Exécutable | Crate | Rôle |
|---|---|---|
| `kubuno-desktop.exe` | `src/shell/` | coque : lanceur, comptes, activité, réglages, synchro, Explorateur |
| `drive.exe` | `src/drive/crates/drive-app` | explorateur de fichiers Kubuno Drive |
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
cargo build --release -p kubuno-desktop        # depuis windows/
cd packaging
pwsh ./package-msix.ps1 -ExePath ..\target\release\kubuno-desktop.exe
#   → Kubuno-Desktop.msix (non signé, pour envoi au Store)
pwsh ./package-msix.ps1 -Sign -Thumbprint <empreinte>   # pour installer localement
```

Avant un envoi au Store : reprendre dans `AppxManifest.xml` les valeurs
`Identity/Name` et `Identity/Publisher` réservées dans Partner Center, et
incrémenter `Identity/Version` (le 4ᵉ composant doit rester à 0).

## Le démon de synchro (commun)

`common/kubuno-sync` est du Rust pur (rustls + SQLite embarqué) et compile sur
les trois OS de bureau. Depuis `common/` :

```bash
cargo build --release -p kubuno-sync
bash build_deb.sh            # → .deb + .rpm (Linux)
```

## Socle de la synchro hors ligne (commun)

Quatre crates de `common/` portent la synchro hors ligne des données (vskubuno
`docs/DESKTOP-OFFLINE-SYNC.md`, lots SE-0 à SE-3) ; aucune ne dépend de l'UI :

| Crate | Rôle |
|---|---|
| `kubuno-secrets` | magasin d'identifiants de l'OS (Gestionnaire d'identification Windows, Trousseau macOS, Secret Service Linux) |
| `kubuno-api-client` | client HTTP typé de l'API (Kubuno Delta Protocol v1, en-têtes `If-Match`/`Idempotency-Key`, reprises) |
| `kubuno-account` | comptes (serveur + id utilisateur), propriétaire des jetons, courtier de jetons (tube nommé / socket Unix), migration de `creds.json` |
| `kubuno-sync-engine` | base locale SQLite/SQLCipher par compte et par appli, outbox, flux, conflits, planificateur |

```bash
cargo test -p kubuno-secrets -p kubuno-api-client -p kubuno-account
cargo test -p kubuno-sync-engine                        # avec SQLCipher (défaut)
cargo test -p kubuno-sync-engine --no-default-features  # SQLite en clair, sans OpenSSL
```

### SQLCipher : prérequis de build

`kubuno-sync-engine` active par défaut la fonctionnalité `sqlcipher` : la base locale
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
