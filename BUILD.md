# Kubuno Desktop & Mobile — guide de build

L'app est une coquille **Tauri 2** (`app/src-tauri`) au-dessus du moteur de
synchronisation `kubuno-sync` (`crates/kubuno-sync`). La logique Tauri vit dans
une **bibliothèque** (`src/lib.rs`, `run()`), partagée par le binaire desktop
(`main.rs`) et les points d'entrée mobiles (`#[tauri::mobile_entry_point]`).

## Desktop

### Linux (.deb / AppImage)
```bash
cd app && cargo tauri build          # cibles deb + appimage (tauri.conf.json)
```

### Windows — Microsoft Store (MSIX) ⭐
La cible Store est le **MSIX**. L'exécutable est cross-compilé depuis Linux
(`cargo-xwin`, cible `x86_64-pc-windows-msvc`), puis empaqueté en MSIX **sous
Windows** (l'outil `MakeAppx.exe` est Windows-only).

```bash
# 1. Cross-compiler l'exe (Linux) :
cd app && cargo tauri build --runner cargo-xwin --target x86_64-pc-windows-msvc --no-bundle
#    → target/x86_64-pc-windows-msvc/release/kubuno-desktop.exe  (PE32+ GUI)

# 2. Empaqueter en MSIX (Windows, voir app/src-tauri/msix/README.md) :
#    pwsh ./package-msix.ps1
```
Tout est prêt dans **`app/src-tauri/msix/`** (manifeste, logos Store, script).
L'exe cross-compilé est déjà copié dans `dist/windows/` et dans le dossier msix.

### Windows — distribution hors Store (NSIS)
```bash
cd app && cargo tauri build --runner cargo-xwin --target x86_64-pc-windows-msvc
#    → installeur NSIS (config dans tauri.windows.conf.json)
```

### macOS (.dmg) — sur un runner macOS
```bash
cd app && cargo tauri build            # .app + .dmg
```

## Android (à compiler dans Android Studio)

Prérequis : **Android Studio** avec le **SDK** + **NDK** (SDK Manager →
*NDK (Side by side)*), `JAVA_HOME` (JDK 17+), et les cibles Rust Android :
```bash
rustup target add aarch64-linux-android armv7-linux-androideabi i686-linux-android x86_64-linux-android
```

Pointer Tauri vers le SDK/NDK, puis générer le projet Gradle :
```bash
export ANDROID_HOME="$HOME/Android/Sdk"          # ou le SDK d'Android Studio
export NDK_HOME="$ANDROID_HOME/ndk/<version>"

cd app
cargo tauri android init     # génère app/src-tauri/gen/android (projet Gradle)
cargo tauri android open     # ouvre le projet dans Android Studio
# … ou en CLI :
cargo tauri android build    # APK/AAB (release : --aab pour le Play Store)
```

Dans **Android Studio** : ouvrir `app/src-tauri/gen/android`, laisser Gradle
synchroniser, puis *Run* (émulateur/appareil) ou *Build → Generate Signed
Bundle/APK* pour publier sur le Play Store (`.aab`).

> Le projet `gen/android` est régénérable (`gitignore`) — `android init` le
> recrée à partir de la config Tauri (`identifier = com.kubuno.desktop`).

### Notes mobiles
- Le frontend est un simple `index.html` statique (chargé par la WebView).
- `kubuno-sync` (rustls + rusqlite *bundled* + notify) compile via le NDK.
  La synchro suppose un dossier accessible ; sur Android le chemin par défaut
  devra être adapté au stockage applicatif (à affiner selon l'usage).
