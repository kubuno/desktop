<#
.SYNOPSIS
  Builds every app and example of the workspace in one cargo invocation.

.DESCRIPTION
  Every program links kubuno-desktop-ui and Rust's std statically: each exe in
  target\<profile>\ (and target\<profile>\examples\) runs on its own, with no
  DLL to stage beside it. Building a single package (cargo build -p X) is fine
  too; this script only saves typing when every app should pick up a change.

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
} finally {
  Pop-Location
}
