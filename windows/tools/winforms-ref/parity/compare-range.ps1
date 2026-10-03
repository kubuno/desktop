# Numeric parity report for the RANGE family.
#
# Diffs `range-winforms.json` (the real toolkit) against `range-port.json` (the
# `kubuno-desktop-controls` reproduction), case by case, and exits NON-ZERO on any
# mismatch. What it compares:
#
#   * the OUTCOME of every step — `ok` or `err`. WinForms throws
#     `ArgumentOutOfRangeException` where the port returns `Err(OutOfRange)`;
#     those are the same decision expressed two ways, so only the status is
#     compared and the mechanism is printed. A case where one side ACCEPTS what
#     the other REFUSES is a mismatch, and is called out as such.
#   * every key of `state` — the resulting Value / Text / SelectedIndex and the
#     properties around them, as strings, exactly as each side rendered them.
#
# `extra` is never compared: it holds observations only one side can make (the
# native SCROLLINFO and TBM_GETNUMTICS on the toolkit side, the derived
# reachable ceiling on the port side). It is printed next to a mismatch because
# it usually explains it.
#
# Produce the two inputs first:
#
#   cd tools\winforms-ref\parity\range-winforms ; dotnet run
#   cd ..\..\..\.. ; $env:CARGO_TARGET_DIR='C:\kubuno-build\desktop-target'
#   cargo run -p kubuno-desktop-controls --example parity_range -j 1
#
# Then:
#
#   .\compare-range.ps1            # report + exit code
#   .\compare-range.ps1 -All       # also list the cases that match

[CmdletBinding()]
param(
    [string]$WinForms = (Join-Path $PSScriptRoot 'range-winforms.json'),
    [string]$Port     = (Join-Path $PSScriptRoot 'range-port.json'),
    [switch]$All
)

$ErrorActionPreference = 'Stop'

foreach ($p in @($WinForms, $Port)) {
    if (-not (Test-Path $p)) {
        Write-Host "missing input: $p" -ForegroundColor Red
        exit 2
    }
}

$ref = (Get-Content -Raw -Encoding UTF8 $WinForms) | ConvertFrom-Json
$prt = (Get-Content -Raw -Encoding UTF8 $Port)     | ConvertFrom-Json

# Fold an "extra" object into a one-line string for the context column.
function Format-Extra($obj) {
    if ($null -eq $obj) { return '' }
    $parts = @()
    foreach ($p in $obj.PSObject.Properties) { $parts += "$($p.Name)=$($p.Value)" }
    return ($parts -join ' ')
}

# Render a control character-free, eye-readable form of a value: the fr-FR group
# separator is U+202F and would otherwise look like a plain space in the report,
# hiding the very difference the report exists to show.
function Show-Value($s) {
    if ($null -eq $s) { return '<null>' }
    $out = ''
    foreach ($ch in $s.ToCharArray()) {
        $code = [int]$ch
        if ($code -lt 0x20 -or $code -gt 0x7E) {
            $out += ('<U+{0:X4}>' -f $code)
        } else {
            $out += $ch
        }
    }
    if ($out -eq '') { return "''" }
    return $out
}

$refById = @{}
foreach ($c in $ref.cases) { $refById[$c.id] = $c }
$prtById = @{}
foreach ($c in $prt.cases) { $prtById[$c.id] = $c }

Write-Host ''
Write-Host 'Range family — numeric parity' -ForegroundColor Cyan
Write-Host ("  reference : {0}  ({1} cases, culture {2})" -f $WinForms, $ref.cases.Count, $ref.culture)
Write-Host ("  port      : {0}  ({1} cases, culture {2})" -f $Port, $prt.cases.Count, $prt.culture)
if ($ref.decimalSeparator -ne $prt.decimalSeparator -or $ref.groupSeparator -ne $prt.groupSeparator) {
    Write-Host ("  SEPARATORS DIFFER: reference dec='{0}' grp='{1}' vs port dec='{2}' grp='{3}'" -f `
        (Show-Value $ref.decimalSeparator), (Show-Value $ref.groupSeparator), `
        (Show-Value $prt.decimalSeparator), (Show-Value $prt.groupSeparator)) -ForegroundColor Yellow
}
Write-Host ''

$onlyRef = @($refById.Keys | Where-Object { -not $prtById.ContainsKey($_) })
$onlyPrt = @($prtById.Keys | Where-Object { -not $refById.ContainsKey($_) })
foreach ($id in $onlyRef) { Write-Host "case only in the reference: $id" -ForegroundColor Red }
foreach ($id in $onlyPrt) { Write-Host "case only in the port:      $id" -ForegroundColor Red }

$matched      = 0
$mismatched   = 0
$findings     = 0
$decisionDiff = 0

foreach ($c in $ref.cases) {
    $r = $c
    $p = $prtById[$c.id]
    if ($null -eq $p) { continue }

    $problems = @()

    # 1. step outcomes
    $n = [Math]::Max($r.steps.Count, $p.steps.Count)
    for ($i = 0; $i -lt $n; $i++) {
        $rs = if ($i -lt $r.steps.Count) { $r.steps[$i] } else { $null }
        $ps = if ($i -lt $p.steps.Count) { $p.steps[$i] } else { $null }
        if ($null -eq $rs -or $null -eq $ps) {
            $problems += "step $i present on only one side"
            continue
        }
        if ($rs.op -ne $ps.op) {
            $problems += "step $i op '$($rs.op)' vs '$($ps.op)' — the probes have drifted"
            continue
        }
        if ($rs.status -ne $ps.status) {
            $accepts = if ($rs.status -eq 'ok') { 'toolkit ACCEPTS, port REFUSES' } else { 'toolkit REFUSES, port ACCEPTS' }
            $problems += ("step '{0}': {1}  (toolkit {2} {3} | port {4} {5})" -f `
                $rs.op, $accepts, $rs.status, $rs.detail, $ps.status, $ps.detail)
            $decisionDiff++
        }
    }

    # 2. resulting state
    $keys = @($r.state.PSObject.Properties.Name)
    foreach ($k in $keys) {
        $rv = $r.state.$k
        $pv = $p.state.$k
        if ($null -eq $p.state.PSObject.Properties[$k]) {
            $problems += "state key '$k' missing from the port output"
            continue
        }
        if ($rv -ne $pv) {
            $problems += ("{0}: toolkit {1}  |  port {2}" -f $k, (Show-Value $rv), (Show-Value $pv))
        }
    }
    foreach ($k in @($p.state.PSObject.Properties.Name)) {
        if ($null -eq $r.state.PSObject.Properties[$k]) {
            $problems += "state key '$k' missing from the reference output"
        }
    }

    if ($problems.Count -eq 0) {
        $matched++
        if ($All) { Write-Host ("  OK   {0}" -f $c.id) -ForegroundColor DarkGray }
    } else {
        $mismatched++
        $findings += $problems.Count
        $steps = ($r.steps | ForEach-Object { $_.op }) -join '; '
        Write-Host ("MISMATCH  {0}  [{1}]" -f $c.id, $c.kind) -ForegroundColor Red
        Write-Host ("          steps: {0}" -f $(if ($steps) { $steps } else { '(defaults)' }))
        foreach ($problem in $problems) { Write-Host ("          - {0}" -f $problem) -ForegroundColor Yellow }
        $rx = Format-Extra $r.extra
        $px = Format-Extra $p.extra
        if ($rx -or $px) {
            Write-Host ("          context: toolkit {{{0}}}  port {{{1}}}" -f $rx, $px) -ForegroundColor DarkGray
        }
    }
}

$total = $ref.cases.Count
Write-Host ''
Write-Host ("{0} cases | {1} exact matches | {2} mismatched cases | {3} differing fields | {4} accept/refuse disagreements" -f `
    $total, $matched, $mismatched, $findings, $decisionDiff)

if ($mismatched -gt 0 -or $onlyRef.Count -gt 0 -or $onlyPrt.Count -gt 0) {
    Write-Host 'PARITY FAILED' -ForegroundColor Red
    exit 1
}
Write-Host 'PARITY OK' -ForegroundColor Green
exit 0
