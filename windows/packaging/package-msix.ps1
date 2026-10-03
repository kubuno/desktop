<#
  package-msix.ps1 — builds a Microsoft Store-ready MSIX for Kubuno Desktop.

  Run on Windows (MSIX tooling is Windows-only). Requires the Windows 10/11 SDK
  (provides makeappx.exe + signtool.exe).

  Usage:
    # 1. Build the app exe (on Windows):
    #      cargo build --release -p kubuno-desktop
    #    then point -ExePath at target\release\kubuno-desktop.exe. It links
    #    kubuno-ui and Rust's std statically: the exe is the whole program.
    # 2. Package:
    #      pwsh ./package-msix.ps1                       # unsigned, for Store upload
    #      pwsh ./package-msix.ps1 -Sign -Thumbprint ..  # signed, for local install/testing

  For the Store: replace Identity Name/Publisher in AppxManifest.xml with the
  values reserved in Partner Center, then upload the UNSIGNED .msix (the Store
  re-signs it). To sideload/test locally you must sign it (self-signed cert OK).
#>
param(
  # Defaults to the exe sitting next to this script (cross-compiled on Linux);
  # override to point at a fresh Windows build under target\…\release\.
  [string]$ExePath  = "kubuno-desktop.exe",
  [string]$Manifest = "AppxManifest.xml",
  [string]$Assets   = "Assets",
  [string]$OutMsix  = "Kubuno-Desktop.msix",
  [switch]$Sign,
  [string]$Thumbprint
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $MyInvocation.MyCommand.Path
Set-Location $root

# ── Locate the Windows SDK tools ──────────────────────────────────────────────
function Find-SdkTool($name) {
  $hit = Get-Command $name -ErrorAction SilentlyContinue
  if ($hit) { return $hit.Source }
  $bases = @("${env:ProgramFiles(x86)}\Windows Kits\10\bin", "${env:ProgramFiles}\Windows Kits\10\bin")
  foreach ($b in $bases) {
    if (Test-Path $b) {
      $found = Get-ChildItem -Path $b -Recurse -Filter $name -ErrorAction SilentlyContinue |
               Where-Object { $_.FullName -match 'x64' } | Select-Object -First 1
      if ($found) { return $found.FullName }
    }
  }
  throw "$name introuvable. Installe le 'Windows 10/11 SDK'."
}
$makeappx = Find-SdkTool "makeappx.exe"
$signtool = Find-SdkTool "signtool.exe"

# ── Stage the package layout ──────────────────────────────────────────────────
$layout = Join-Path $root "layout"
if (Test-Path $layout) { Remove-Item $layout -Recurse -Force }
New-Item -ItemType Directory -Force $layout | Out-Null

if (-not (Test-Path $ExePath)) { throw "Exécutable introuvable : $ExePath (build d'abord avec 'cargo build --release -p kubuno-desktop')." }
Copy-Item $ExePath (Join-Path $layout "kubuno-desktop.exe")
# The exe links the design system (kubuno-ui) and Rust's std statically
# (.cargo/config.toml): no DLL ships beside it. An exe built before that change
# would still import a `kubuno_ui-<hash>.dll`: refuse it rather than ship a
# package that cannot start.
$exeText = [System.Text.Encoding]::GetEncoding(28591).GetString([System.IO.File]::ReadAllBytes((Resolve-Path $ExePath).Path))
if ($exeText -match 'kubuno_ui(-[0-9a-f]{16})?\.dll|std-[0-9a-f]{16}\.dll') {
  throw "$ExePath importe encore une DLL Rust ($($Matches[0])) : reconstruis-le ('cargo build --release -p kubuno-desktop')."
}
Copy-Item $Manifest (Join-Path $layout "AppxManifest.xml")
Copy-Item $Assets   (Join-Path $layout "Assets") -Recurse

# ── Pack ──────────────────────────────────────────────────────────────────────
Write-Host "→ makeappx pack…" -ForegroundColor Cyan
& $makeappx pack /d $layout /p (Join-Path $root $OutMsix) /o
if ($LASTEXITCODE -ne 0) { throw "makeappx a échoué ($LASTEXITCODE)." }
Write-Host "✓ $OutMsix créé." -ForegroundColor Green

# ── Optional signing (local install/testing only; the Store re-signs) ─────────
if ($Sign) {
  if (-not $Thumbprint) { throw "-Sign nécessite -Thumbprint <empreinte du certificat>." }
  Write-Host "→ signtool sign…" -ForegroundColor Cyan
  & $signtool sign /fd SHA256 /sha1 $Thumbprint (Join-Path $root $OutMsix)
  if ($LASTEXITCODE -ne 0) { throw "signtool a échoué ($LASTEXITCODE)." }
  Write-Host "✓ $OutMsix signé." -ForegroundColor Green
}

Write-Host ""
Write-Host "Store : remplace Identity Name/Publisher dans AppxManifest.xml par les" -ForegroundColor Yellow
Write-Host "valeurs de Partner Center, puis téléverse le .msix NON signé." -ForegroundColor Yellow
