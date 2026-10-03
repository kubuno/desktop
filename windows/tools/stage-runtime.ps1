<#
.SYNOPSIS
  Copies the shared runtime next to every exe of a build, so the apps start by
  double-click: the `kubuno_ui-<hash>.dll` each exe imports (the Kubuno desktop
  design system, shared by every app of the workspace) with its PDB, and Rust's
  own `std-*.dll`.

.DESCRIPTION
  The workspace links `kubuno-ui` as a Rust dylib with `-C prefer-dynamic`
  (see `.cargo/config.toml`), so each exe imports both DLLs. Cargo leaves them
  in the build's own folders and Windows only looks beside the exe (and on
  PATH), hence this step after `cargo build`. `cargo run` / `cargo test` do not
  need it: Cargo puts both folders on PATH itself.

  Every build of the design system has its own file name, `kubuno_ui-<hash>.dll`
  (see `src/crates/kubuno-ui/build.rs`), and an exe imports exactly the one it
  was linked against: this script reads that name from each exe, so an exe that
  was not relinked after the last kubuno-ui build still gets - and keeps running
  on - its own DLL, as long as the build folder still holds it (the three most
  recent builds are kept). Staged copies no exe of a folder imports any more are
  removed.

.EXAMPLE
  cargo build --release -p kubuno-desktop
  pwsh ./tools/stage-runtime.ps1 -Profile release
#>
param(
  [ValidateSet('debug', 'release')]
  [string]$Profile = 'debug',
  # Defaults to CARGO_TARGET_DIR, then to the workspace's own target/.
  [string]$TargetDir = $env:CARGO_TARGET_DIR
)

$ErrorActionPreference = 'Stop'
if (-not $TargetDir) { $TargetDir = Join-Path $PSScriptRoot '..\target' }
$out = Join-Path $TargetDir $Profile
if (-not (Test-Path $out)) { throw "No $Profile build in $TargetDir - run cargo build first." }
$deps = Join-Path $out 'deps'

# The kubuno_ui DLL name an exe imports: `kubuno_ui-<16 hex>.dll`, or the plain
# `kubuno_ui.dll` of a build made before per-build names. Import names are
# plain ASCII in the PE file; Latin-1 maps every byte to one character.
$latin1 = [System.Text.Encoding]::GetEncoding(28591)
function Get-KubunoUiImport([string]$exe) {
  $text = $latin1.GetString([System.IO.File]::ReadAllBytes($exe))
  $hashed = [regex]::Match($text, 'kubuno_ui-[0-9a-f]{16}\.dll')
  if ($hashed.Success) { return $hashed.Value }
  $plain = [regex]::Match($text, 'kubuno_ui\.dll(?!\.)')
  if ($plain.Success) { return $plain.Value }
  return $null
}

# Rust's standard library, as the DLL this toolchain's exes were linked against.
$sysroot = (& rustc --print sysroot).Trim()
$std = Get-ChildItem (Join-Path $sysroot 'lib\rustlib\x86_64-pc-windows-msvc\lib') -Filter 'std-*.dll'
if (-not $std) { throw "std-*.dll not found in $sysroot" }

function Copy-Staged([string]$source, [string]$dir) {
  $dest = Join-Path $dir (Split-Path $source -Leaf)
  if ((Resolve-Path $source).Path -eq $dest) { return }
  $src = Get-Item $source
  if ((Test-Path $dest) -and ((Get-Item $dest).Length -eq $src.Length) -and ((Get-Item $dest).LastWriteTimeUtc -eq $src.LastWriteTimeUtc)) { return }
  try {
    Copy-Item $source $dest -Force
  } catch {
    # A running app locks its DLLs; it keeps the one it loaded.
    Write-Warning "skipped $dest (in use: close the running app and stage again)"
  }
}

$dirs = @($out, (Join-Path $out 'examples')) | Where-Object { Test-Path $_ }
$stale = @()
# When this build names its DLLs per build, the plain `kubuno_ui.dll` is only
# Cargo's alias of the latest one: an exe importing that name was linked before
# and must be relinked, not given a DLL of another build.
$hashedBuild = [bool](Get-ChildItem $deps -Filter 'kubuno_ui-*.dll' -ErrorAction SilentlyContinue | Where-Object { $_.Name -match '^kubuno_ui-[0-9a-f]{16}\.dll$' })
foreach ($dir in $dirs) {
  $exes = @(Get-ChildItem $dir -Filter '*.exe' -ErrorAction SilentlyContinue)
  if (-not $exes) { continue }
  $wanted = @{}
  foreach ($exe in $exes) {
    $name = Get-KubunoUiImport $exe.FullName
    if (-not $name) { continue }
    if ($hashedBuild -and $name -eq 'kubuno_ui.dll') { $stale += "$($exe.FullName) (imports the unversioned $name)"; continue }
    $source = @((Join-Path $deps $name), (Join-Path $out $name)) | Where-Object { Test-Path $_ } | Select-Object -First 1
    if (-not $source) { $stale += "$($exe.FullName) (imports $name)"; continue }
    $wanted[$name] = $source
  }
  foreach ($name in $wanted.Keys) {
    Copy-Staged $wanted[$name] $dir
    # Its PDB, under the name the DLL records (`kubuno_ui-<hash>.pdb`): the debugger looks beside the DLL.
    $pdb = [System.IO.Path]::ChangeExtension($wanted[$name], 'pdb')
    if (Test-Path $pdb) { Copy-Staged $pdb $dir }
  }
  foreach ($dll in $std) { Copy-Staged $dll.FullName $dir }

  # Staged builds no exe of this folder imports any more (Cargo's own uplifted
  # `kubuno_ui.dll`/`.pdb` in the profile folder are left alone).
  foreach ($old in Get-ChildItem $dir -File -ErrorAction SilentlyContinue | Where-Object { $_.Name -match '^kubuno_ui-[0-9a-f]{16}\.(dll|pdb)$' }) {
    $dllName = [System.IO.Path]::ChangeExtension($old.Name, 'dll')
    if (-not $wanted.ContainsKey($dllName)) {
      try { Remove-Item $old.FullName -Force } catch { }
    }
  }
  "staged runtime in $dir ($(@($wanted.Keys) -join ', '))"
}

# Each exe imports the kubuno_ui build it was linked against, so an exe that
# predates the last kubuno-ui build still runs on its own DLL - until that build
# leaves the build folder (the three most recent are kept). Those can no longer
# start ("kubuno_ui-<hash>.dll was not found") until relinked.
if ($stale) {
  Write-Warning ("these exes import a kubuno_ui build that $deps no longer holds and will not start until relinked:`n  " + ($stale -join "`n  "))
  Write-Warning "run: cargo build --workspace --bins --examples$(if ($Profile -eq 'release') { ' --release' })"
}
