# Compares a rebuilt Kubuno primitive against the hand-written one it replaces,
# INSIDE A SINGLE CAPTURE.
#
# The gallery's `sheet::pair` paints the predecessor and its replacement into
# two rectangles of identical size, a known distance apart. Comparing the two
# halves of one screenshot -- rather than two screenshots -- removes every source
# of variance that has cost us false alarms before: window placement, DPI
# virtualisation, theme resolution, even the frame the compositor happened to
# hand back. If the two halves differ, the paint differs. Nothing else can.
#
#   .\split-diff.ps1 -Image shot.png -Rect 24,120,220,36 -Dx 260 -Out diff.png
#
# -Rect is the LEFT ("actuel") rectangle in PHYSICAL pixels: x,y,w,h.
# -Dx   is the distance to its rebuilt twin, also in physical pixels.
# Both come from the gallery itself: run it with KUBUNO_UI_DUMP=1 and it prints
# every pair's geometry, so no number here is measured by eye.

param(
    [Parameter(Mandatory = $true)][string]$Image,
    [Parameter(Mandatory = $true)][int[]]$Rect,
    [Parameter(Mandatory = $true)][int]$Dx,
    [string]$Out,
    # Channel distance under which two pixels count as equal. 0 demands an
    # exact match, which is what a replacement should achieve; a couple of
    # units is the most a text-antialiasing difference should ever produce, and
    # anything above that is a real difference in the paint.
    [int]$Tolerance = 0
)

Add-Type -AssemblyName System.Drawing

if ($Rect.Count -ne 4) { Write-Error 'Rect must be x,y,w,h'; exit 2 }
$x, $y, $w, $h = $Rect

$bmp = [System.Drawing.Bitmap]::FromFile((Resolve-Path $Image))
try {
    if ($x + $Dx + $w -gt $bmp.Width -or $y + $h -gt $bmp.Height) {
        Write-Error "the pair does not fit in the capture ($($bmp.Width)x$($bmp.Height))"
        exit 2
    }

    $diff = if ($Out) { New-Object System.Drawing.Bitmap($w, $h) } else { $null }
    $bad = 0
    $worst = 0
    $firstBad = $null

    for ($j = 0; $j -lt $h; $j++) {
        for ($i = 0; $i -lt $w; $i++) {
            $a = $bmp.GetPixel($x + $i, $y + $j)
            $b = $bmp.GetPixel($x + $Dx + $i, $y + $j)
            $d = [Math]::Max([Math]::Max([Math]::Abs($a.R - $b.R), [Math]::Abs($a.G - $b.G)),
                             [Math]::Abs($a.B - $b.B))
            if ($d -gt $worst) { $worst = $d }
            if ($d -gt $Tolerance) {
                $bad++
                if ($null -eq $firstBad) { $firstBad = "($i,$j) actuel=$($a.R),$($a.G),$($a.B) reconstruit=$($b.R),$($b.G),$($b.B)" }
                if ($diff) { $diff.SetPixel($i, $j, [System.Drawing.Color]::FromArgb(255, 255, 0, 0)) }
            }
            elseif ($diff) {
                # Keep the shape readable under the red: the matching pixels
                # stay, washed out, so a diff map still shows WHAT was compared.
                $g = [int](($a.R + $a.G + $a.B) / 3)
                $g = 200 + [int]($g * 55 / 255)
                $diff.SetPixel($i, $j, [System.Drawing.Color]::FromArgb(255, $g, $g, $g))
            }
        }
    }

    $total = $w * $h
    $pct = if ($total) { [Math]::Round(100.0 * $bad / $total, 3) } else { 0 }
    "compared ${w}x${h} = $total px   different: $bad ($pct %)   worst channel delta: $worst"
    if ($firstBad) { "first difference at $firstBad" }

    if ($diff) {
        $dir = Split-Path -Parent $Out
        if ($dir -and -not (Test-Path $dir)) { New-Item -ItemType Directory -Force -Path $dir | Out-Null }
        $diff.Save($Out, [System.Drawing.Imaging.ImageFormat]::Png)
        "diff map: $Out"
        $diff.Dispose()
    }

    if ($bad -gt 0) { exit 1 } else { "IDENTICAL"; exit 0 }
}
finally { $bmp.Dispose() }
