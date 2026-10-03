# Numeric layout parity: real WinForms against the Kubuno port.
#
# Diffs the two JSON files the probes write and prints one row per child, with
# the WinForms rectangle, the port's rectangle and the delta. Exits non-zero if
# any delta exceeds the tolerance (0.5 DIP by default).
#
# WHY NUMBERS AND NOT PIXELS. The port paints in the Kubuno design system —
# rounded corners, its own palette, its own face — so a pixel diff of appearance
# would only re-measure a difference we already know about. What must match
# exactly is the geometry: the same container, the same children with the same
# Dock/Anchor/Margin/Padding/MinimumSize/MaximumSize, the same numbers out.
#
# TWO SPACES, ONE TABLE. WinForms reports a child's `Bounds` relative to its
# parent's CLIENT rectangle; the port's layout engine returns rectangles
# relative to the parent's DISPLAY rectangle (client deflated by padding, moved
# to the origin). With zero padding the two coincide. With padding they differ
# by a constant, and reporting that constant as a geometry error on every child
# of a padded container would bury the real defects. So each row is judged in
# DISPLAY-RELATIVE space — each side's rectangle minus its own container's
# display origin — and the origin convention is reported ONCE per container, in
# its own section. `-Absolute` turns that normalisation off and compares the raw
# numbers instead.
#
#   .\compare-layout.ps1
#   .\compare-layout.ps1 -Tolerance 0.5 -OnlyMismatches
#   .\compare-layout.ps1 -Absolute
param(
    [string]$WinForms = (Join-Path $PSScriptRoot 'out\layout-winforms.json'),
    [string]$Port     = (Join-Path $PSScriptRoot 'out\layout-port.json'),
    [double]$Tolerance = 0.5,
    [switch]$OnlyMismatches,
    [switch]$Absolute
)

$ErrorActionPreference = 'Stop'

foreach ($p in @($WinForms, $Port)) {
    if (-not (Test-Path $p)) {
        Write-Error "missing: $p  (run the two probes first — see the header of this script)"
        exit 2
    }
}

$wfDoc   = Get-Content $WinForms -Raw | ConvertFrom-Json
$portDoc = Get-Content $Port     -Raw | ConvertFrom-Json

function Format-Rect($r) {
    if ($null -eq $r) { return '(absent)' }
    '{0},{1},{2},{3}' -f $r[0], $r[1], $r[2], $r[3]
}

# `$rect` minus `$origin`, component by component. Written as a function rather
# than inline: PowerShell's comma binds TIGHTER than its arithmetic operators,
# so `@($a[0] - $b[0], $a[1] - $b[1])` parses as `$a[0] - (@($b[0], $a[1])) - $b[1]`
# and fails at run time with "Object[] has no op_Subtraction".
function Move-Rect($rect, $origin) {
    $out = @(0.0, 0.0, 0.0, 0.0)
    for ($k = 0; $k -lt 4; $k++) { $out[$k] = $rect[$k] - $origin[$k] }
    return , $out
}

# The origin a child's rectangle is measured from on a given side: its parent's
# display rect. Path "1.0.2" has parent "1.0"; a top-level path has the case's
# own container.
function Get-Origin($case, $path) {
    $parent = ''
    if ($path -match '^(.*)\.[0-9]+$') { $parent = $Matches[1] }
    if ($parent -eq '') { return $case.display }
    $node = $case.children | Where-Object { $_.path -eq $parent } | Select-Object -First 1
    if ($null -ne $node -and $null -ne $node.display) { return $node.display }
    return @(0, 0, 0, 0)
}

$rows            = New-Object System.Collections.Generic.List[object]
$originNotes     = New-Object System.Collections.Generic.List[object]
$structureErrors = New-Object System.Collections.Generic.List[string]

$portById = @{}
foreach ($c in $portDoc.cases) { $portById[$c.id] = $c }

foreach ($wc in $wfDoc.cases) {
    $pc = $portById[$wc.id]
    if ($null -eq $pc) {
        $structureErrors.Add("case '$($wc.id)' is missing from the port's output")
        continue
    }

    $portByPath = @{}
    foreach ($ch in $pc.children) { $portByPath[$ch.path] = $ch }

    # The container's own display rect. A difference here is the origin
    # convention, not a child's geometry — reported once, not per child.
    for ($k = 0; $k -lt 4; $k++) {
        if ([Math]::Abs($wc.display[$k] - $pc.display[$k]) -gt $Tolerance) {
            $originNotes.Add([pscustomobject]@{
                Case     = $wc.id
                Node     = '(container)'
                WinForms = Format-Rect $wc.display
                Port     = Format-Rect $pc.display
            })
            break
        }
    }

    foreach ($wch in $wc.children) {
        $pch = $portByPath[$wch.path]
        if ($null -eq $pch) {
            $structureErrors.Add("case '$($wc.id)': child '$($wch.path)' ($($wch.name)) is missing from the port's output")
            continue
        }
        if ($pch.name -ne $wch.name) {
            $structureErrors.Add("case '$($wc.id)': child '$($wch.path)' is '$($wch.name)' in WinForms but '$($pch.name)' in the port")
        }

        if ($null -ne $wch.display -and $null -ne $pch.display) {
            for ($k = 0; $k -lt 4; $k++) {
                if ([Math]::Abs($wch.display[$k] - $pch.display[$k]) -gt $Tolerance) {
                    $originNotes.Add([pscustomobject]@{
                        Case     = $wc.id
                        Node     = "$($wch.path) $($wch.name)"
                        WinForms = Format-Rect $wch.display
                        Port     = Format-Rect $pch.display
                    })
                    break
                }
            }
        }

        $wo = @(0, 0, 0, 0)
        $po = @(0, 0, 0, 0)
        if (-not $Absolute) {
            $w = Get-Origin $wc $wch.path
            $p = Get-Origin $pc $wch.path
            $wo = @($w[0], $w[1], $w[0], $w[1])
            $po = @($p[0], $p[1], $p[0], $p[1])
        }

        $delta = @(0.0, 0.0, 0.0, 0.0)
        $worst = 0.0
        for ($k = 0; $k -lt 4; $k++) {
            $delta[$k] = ($pch.bounds[$k] - $po[$k]) - ($wch.bounds[$k] - $wo[$k])
            $a = [Math]::Abs($delta[$k])
            if ($a -gt $worst) { $worst = $a }
        }

        $rows.Add([pscustomobject]@{
            Case     = $wc.id
            Index    = $wch.path
            Child    = $wch.name
            WinForms = Format-Rect (Move-Rect $wch.bounds $wo)
            Port     = Format-Rect (Move-Rect $pch.bounds $po)
            Delta    = Format-Rect $delta
            Worst    = $worst
            Match    = ($worst -le $Tolerance)
        })
    }
}

$space = if ($Absolute) { 'absolute (raw Bounds)' } else { 'display-relative (each side minus its own container origin)' }
Write-Output ''
Write-Output "Layout parity — WinForms $($wfDoc.meta.framework) @ $($wfDoc.meta.deviceDpi) DPI (AnchorLayoutV2=$($wfDoc.meta.anchorLayoutV2))  vs  $($portDoc.meta.crate)"
Write-Output "Comparison space: $space.  Tolerance: $Tolerance DIP.  Rectangles are left,top,right,bottom."
Write-Output ''

$shown = if ($OnlyMismatches) { $rows | Where-Object { -not $_.Match } } else { $rows }
if ($shown.Count -gt 0) {
    $shown |
        Format-Table -AutoSize @{ n = 'Case'; e = { $_.Case } },
                               @{ n = 'Child'; e = { '{0} {1}' -f $_.Index, $_.Child } },
                               @{ n = 'WinForms'; e = { $_.WinForms } },
                               @{ n = 'Port'; e = { $_.Port } },
                               @{ n = 'Delta'; e = { $_.Delta } },
                               @{ n = 'OK'; e = { if ($_.Match) { 'yes' } else { 'NO' } } } |
        Out-String -Width 200 | Write-Output
}

$bad      = @($rows | Where-Object { -not $_.Match })
$badCases = @($bad | Select-Object -ExpandProperty Case -Unique)

if ($originNotes.Count -gt 0) {
    Write-Output '--- Container display-rectangle differences (origin convention, not child geometry) ---'
    $originNotes | Format-Table -AutoSize Case, Node, WinForms, Port | Out-String -Width 200 | Write-Output
}

if ($structureErrors.Count -gt 0) {
    Write-Output '--- Structural differences (the two probes did not build the same tree) ---'
    $structureErrors | ForEach-Object { Write-Output "  $_" }
    Write-Output ''
}

Write-Output ('Rectangles: {0} compared, {1} within {2} DIP, {3} over.' -f $rows.Count, ($rows.Count - $bad.Count), $Tolerance, $bad.Count)
Write-Output ('Cases:      {0} compared, {1} clean, {2} with at least one mismatch.' -f $wfDoc.cases.Count, ($wfDoc.cases.Count - $badCases.Count), $badCases.Count)
if ($badCases.Count -gt 0) {
    Write-Output ('Cases with mismatches: {0}' -f ($badCases -join ', '))
}

if ($bad.Count -gt 0 -or $structureErrors.Count -gt 0) { exit 1 }
exit 0
