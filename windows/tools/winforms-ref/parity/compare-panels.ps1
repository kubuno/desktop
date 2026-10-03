# Numeric parity diff for the layout-panel family.
#
# Reads the two probe outputs — `panels-winforms.json` (the real toolkit) and
# `panels-port.json` (kubuno-controls) — and prints every place their arithmetic
# disagrees by more than the tolerance, then exits NON-ZERO if any did.
#
# Why numeric and not pixel: the port paints in the Kubuno design system, so a
# picture diff would only re-measure a difference we chose. What must match is
# the geometry: same inputs, same rectangles.
#
#   .\compare-panels.ps1
#   .\compare-panels.ps1 -Tolerance 0.5 -All
#
#   -All        also list the metrics that MATCH, not just the deltas.
#   -Tolerance  maximum accepted difference, in DIP (default 0.5).
param(
    [string]$WinForms = (Join-Path $PSScriptRoot 'panels-winforms.json'),
    [string]$Port     = (Join-Path $PSScriptRoot 'panels-port.json'),
    [double]$Tolerance = 0.5,
    [switch]$All
)

$ErrorActionPreference = 'Stop'

foreach ($p in @($WinForms, $Port)) {
    if (-not (Test-Path $p)) {
        $how = 'run both probes first: `dotnet run --project .\panels-winforms`, then ' +
               '`cargo run -p kubuno-controls --example parity_panels`'
        # Write-Host, not Write-Error: $ErrorActionPreference = 'Stop' would make
        # Write-Error terminate before `exit 2` could set the code the caller reads.
        Write-Host ('missing: {0} - {1}' -f $p, $how) -ForegroundColor Red
        exit 2
    }
}

# NOT `$ref` / `$port`: PowerShell variable names are case-insensitive, so
# assigning the parsed document to `$port` would write it through the `[string]`
# parameter of the same name and silently turn it into "System.Object[]".
$refDoc  = Get-Content -Raw -Path $WinForms | ConvertFrom-Json
$portDoc = Get-Content -Raw -Path $Port     | ConvertFrom-Json

# Index the port's cases by id: the two files are written in case order today,
# but nothing should depend on that.
$portById = @{}
foreach ($c in $portDoc.cases) { $portById[$c.id] = $c }

# ConvertFrom-Json gives PSCustomObjects; their property names are the metric
# keys. (Windows PowerShell 5.1 has no -AsHashtable.)
function Get-Keys($obj) {
    if ($null -eq $obj) { return @() }
    return $obj.PSObject.Properties.Name
}

$fmt = '{0,-32} {1,-14} {2,-26} {3,-26} {4}'
$failures  = 0
$within   = 0
$missing   = 0
$compared  = 0

Write-Host ''
Write-Host ('  {0} cases, tolerance {1} DIP' -f $refDoc.cases.Count, $Tolerance) -ForegroundColor DarkGray
Write-Host ''
Write-Host ($fmt -f 'case', 'metric', 'WinForms', 'port', 'delta') -ForegroundColor DarkGray
Write-Host ('-' * 118) -ForegroundColor DarkGray

foreach ($rc in $refDoc.cases) {
    $pc = $portById[$rc.id]
    if ($null -eq $pc) {
        Write-Host ($fmt -f $rc.id, '(whole case)', 'present', 'ABSENT', '-') -ForegroundColor Red
        $failures++
        continue
    }

    $refKeys  = Get-Keys $rc.values
    $portKeys = Get-Keys $pc.values

    foreach ($key in $refKeys) {
        $a = @($rc.values.$key)
        if ($portKeys -notcontains $key) {
            Write-Host ($fmt -f $rc.id, $key, ('[' + ($a -join ', ') + ']'), 'NOT MODELLED', '-') -ForegroundColor Magenta
            $missing++
            $failures++
            continue
        }
        $b = @($pc.values.$key)
        $compared++

        if ($a.Count -ne $b.Count) {
            Write-Host ($fmt -f $rc.id, $key, ('[' + ($a -join ', ') + ']'), ('[' + ($b -join ', ') + ']'), 'ARITY') -ForegroundColor Red
            $failures++
            continue
        }

        $deltas = @()
        $worst  = 0.0
        for ($i = 0; $i -lt $a.Count; $i++) {
            $d = [double]$b[$i] - [double]$a[$i]
            $deltas += ('{0:0.##}' -f $d)
            if ([Math]::Abs($d) -gt $worst) { $worst = [Math]::Abs($d) }
        }

        if ($worst -gt $Tolerance) {
            Write-Host ($fmt -f $rc.id, $key,
                ('[' + ($a -join ', ') + ']'),
                ('[' + ($b -join ', ') + ']'),
                ('[' + ($deltas -join ', ') + ']')) -ForegroundColor Yellow
            $failures++
        }
        else {
            $within++
            if ($All) {
                Write-Host ($fmt -f $rc.id, $key,
                    ('[' + ($a -join ', ') + ']'),
                    ('[' + ($b -join ', ') + ']'), 'ok') -ForegroundColor DarkGreen
            }
        }
    }

    # A metric the port reports and the toolkit does not is worth knowing about
    # too — it means the port models something the reference never stated.
    foreach ($key in $portKeys) {
        if ($refKeys -notcontains $key) {
            $b = @($pc.values.$key)
            Write-Host ($fmt -f $rc.id, $key, 'NOT MEASURED', ('[' + ($b -join ', ') + ']'), '-') -ForegroundColor DarkMagenta
            $failures++
        }
    }
}

Write-Host ('-' * 118) -ForegroundColor DarkGray
Write-Host ''
Write-Host ('  metrics compared : {0}' -f $compared)
Write-Host ('  within tolerance : {0}' -f $within) -ForegroundColor Green
Write-Host ('  mismatched       : {0}' -f $failures) -ForegroundColor $(if ($failures -gt 0) { 'Yellow' } else { 'Green' })
if ($missing -gt 0) {
    Write-Host ('  of which the port does not model at all : {0}' -f $missing) -ForegroundColor Magenta
}
Write-Host ''

if ($failures -gt 0) { exit 1 }
exit 0
