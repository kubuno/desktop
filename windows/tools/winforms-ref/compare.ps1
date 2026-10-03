# Side-by-side comparison sheet: the real WinForms control on the left, the
# Kubuno reproduction on the right, captioned and separated by a rule.
#
# The two are captured at the same DPI by construction (both run on this
# machine), so a difference in the composite is a real difference in the port —
# not a scaling artefact.
#
#   .\compare.ps1 -Reference ..\..\shots\01-buttonbase.png `
#                 -Port      ..\..\port\01-buttonbase.png `
#                 -Out       ..\..\compare\01-buttonbase.png
param(
    [Parameter(Mandatory = $true)][string]$Reference,
    [Parameter(Mandatory = $true)][string]$Port,
    [Parameter(Mandatory = $true)][string]$Out
)

Add-Type -AssemblyName System.Drawing

foreach ($p in @($Reference, $Port)) {
    if (-not (Test-Path $p)) { Write-Error "missing: $p"; exit 1 }
}

$a = [System.Drawing.Image]::FromFile((Resolve-Path $Reference))
$b = [System.Drawing.Image]::FromFile((Resolve-Path $Port))

# The two captures do not share a pixel basis: WinForms' `DrawToBitmap` writes
# the form's LOGICAL client size, while a screen grab of the port is PHYSICAL
# pixels (1.75× at 175 % DPI). Composing them as-is would show the port as
# "bigger" and invite a conclusion about the port that is really a fact about
# the capture. So the port is scaled to the reference's basis, and the factor is
# stated on the sheet.
$factor = 1.0
if ($b.Height -gt 0 -and $a.Height -gt 0) { $factor = $a.Height / $b.Height }
$bW = [int][Math]::Round($b.Width * $factor)
$bH = [int][Math]::Round($b.Height * $factor)

$gap     = 24
$caption = 34
$width   = $a.Width + $gap + $bW
$height  = [Math]::Max($a.Height, $bH) + $caption

$bmp = New-Object System.Drawing.Bitmap($width, $height)
$g   = [System.Drawing.Graphics]::FromImage($bmp)
$g.Clear([System.Drawing.Color]::FromArgb(245, 246, 248))
$g.TextRenderingHint = 'ClearTypeGridFit'

$font  = New-Object System.Drawing.Font('Segoe UI', 11)
$ink   = New-Object System.Drawing.SolidBrush([System.Drawing.Color]::FromArgb(32, 33, 36))
$rule  = New-Object System.Drawing.Pen([System.Drawing.Color]::FromArgb(200, 203, 207))

$g.InterpolationMode = 'HighQualityBicubic'
$g.DrawString('WinForms (référence)', $font, $ink, 8, 8)
$portCaption = 'Kubuno (port natif)'
if ([Math]::Abs($factor - 1.0) -gt 0.01) {
    $portCaption += (' — ramené à l''échelle de la référence (×{0:N2})' -f $factor)
}
$g.DrawString($portCaption, $font, $ink, ($a.Width + $gap + 8), 8)

$g.DrawImage($a, 0, $caption, $a.Width, $a.Height)
$g.DrawImage($b, ($a.Width + $gap), $caption, $bW, $bH)

# The separating rule sits in the gutter, so neither capture is overdrawn.
$x = $a.Width + ($gap / 2)
$g.DrawLine($rule, $x, 4, $x, $height - 4)

$dir = Split-Path -Parent $Out
if ($dir -and -not (Test-Path $dir)) { New-Item -ItemType Directory -Force -Path $dir | Out-Null }
$bmp.Save($Out, [System.Drawing.Imaging.ImageFormat]::Png)

$a.Dispose(); $b.Dispose(); $g.Dispose(); $bmp.Dispose()
"saved $Out"
