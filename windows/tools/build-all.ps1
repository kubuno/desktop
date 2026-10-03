<#
.SYNOPSIS
  Builds every app and example of the workspace, then stages the shared
  runtime next to them - the one command to use after touching kubuno-ui.

.DESCRIPTION
  kubuno-ui is a Rust dylib with no stable ABI, so every build of it gets its
  own file name, kubuno_ui-<hash>.dll, and each exe imports the one it was
  linked against (see src/crates/kubuno-ui/build.rs). Building a single package
  (cargo build -p X) therefore no longer breaks the other apps: they keep
  running on their previous build, which stays in the build folder for a while
  (the three most recent are kept) - but they do not see the change either.
  Building everything together keeps every app on the latest DLL; the
  release/MSIX build does the same.

.EXAMPLE
  pwsh ./tools/build-all.ps1
  pwsh ./tools/build-all.ps1 -Profile release
#>
param(
  [ValidateSet('debug', 'release')]
  [string]$Profile = 'debug'
)

$ErrorActionPreference = 'Stop'
Push-Location (Join-Path $PSScriptRoot '..')
try {
  $cargoArgs = @('build', '--workspace', '--bins', '--examples', '-j', '1')
  if ($Profile -eq 'release') { $cargoArgs += '--release' }
  & cargo @cargoArgs
  if ($LASTEXITCODE -ne 0) { throw "cargo build failed ($LASTEXITCODE)" }
  & (Join-Path $PSScriptRoot 'stage-runtime.ps1') -Profile $Profile
} finally {
  Pop-Location
}
