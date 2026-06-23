# Kubuno Desktop — packaging Microsoft Store (MSIX)

Le Microsoft Store distribue les applications de bureau au format **MSIX**.
Tauri ne génère pas de MSIX nativement : on emballe donc l'exécutable Win32
(full-trust, *Desktop Bridge*) avec les outils du **Windows SDK**.

> ⚠️ La création du `.msix` (`MakeAppx.exe`) ne fonctionne **que sous Windows**.
> L'exécutable, lui, est déjà cross-compilé sous Linux (`kubuno-desktop.exe`,
> cible `x86_64-pc-windows-msvc`, présent dans ce dossier et dans `dist/windows/`).

## Contenu

| Fichier | Rôle |
|---|---|
| `AppxManifest.xml` | Manifeste MSIX (identité, capacités, tuiles) |
| `Assets/` | Logos aux tailles Store (44, 71, 150, 310, wide, splash, StoreLogo) |
| `package-msix.ps1` | Assemble le layout + `makeappx pack` (+ `signtool` optionnel) |
| `kubuno-desktop.exe` | App cross-compilée (le binaire à empaqueter) |

## 1. Réserver l'app dans Partner Center

Sur https://partner.microsoft.com → réserve le nom, puis relève dans
**Identité du produit** :
- `Package/Identity/Name`  (ex. `1234Kubuno.KubunoDesktop`)
- `Package/Identity/Publisher`  (ex. `CN=ABCD1234-...`)

Reporte ces deux valeurs **exactes** dans `AppxManifest.xml` (sinon le Store
refuse le paquet). Incrémente `Version` (`1.0.0.0`, 4ᵉ champ toujours `0`) à
chaque téléversement.

## 2. Construire le MSIX (sous Windows)

Prérequis : **Windows 10/11 SDK** (fournit `makeappx.exe` et `signtool.exe`).

```powershell
# Paquet non signé, prêt pour le Store (le Store re-signe) :
pwsh ./package-msix.ps1

# OU paquet signé pour installer/tester en local (certificat auto-signé) :
pwsh ./package-msix.ps1 -Sign -Thumbprint <empreinte-du-certificat>
```

Le script produit `Kubuno-Desktop.msix`.

### (Re)compiler l'exe sous Windows plutôt que d'utiliser le binaire cross-compilé
```powershell
cargo tauri build --no-bundle           # → target\release\kubuno-desktop.exe
pwsh ./package-msix.ps1 -ExePath ..\..\..\target\release\kubuno-desktop.exe
```

## 3. Soumettre

Téléverse le `.msix` **non signé** dans Partner Center (Packages). Le Store
gère la signature et la distribution.

## Notes

- **WebView2** : l'app utilise le runtime *Evergreen WebView2* (préinstallé sur
  Windows 11, et sur la plupart des Windows 10). Aucun installeur n'est embarqué.
- **Capacités** : `runFullTrust` (process Win32 normal → accès fichiers/réseau)
  + `internetClient`. La synchro d'un dossier arbitraire fonctionne grâce au
  full-trust ; si tu cibles un dossier hors profil utilisateur et que le Store
  le demande, ajoute la capacité restreinte `broadFileSystemAccess` (justifiée).
- **Architecture** : x64 (`ProcessorArchitecture="x64"`). Pour ARM64, recompile
  avec la cible `aarch64-pc-windows-msvc` et duplique le manifeste.

## Hors Store (distribution directe)

Pour une distribution hors Store, un installeur **NSIS** est aussi configuré
(`tauri.windows.conf.json`, cible `nsis`) :
```bash
cargo tauri build --runner cargo-xwin --target x86_64-pc-windows-msvc
```
