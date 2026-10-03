# Completeness audit: every property the toolkit DECLARES, checked against the
# port — mechanically, so "nothing was forgotten" is a measurement and not a
# claim.
#
# For each control type we implement, each declared property is classified:
#
#   field       the snake_case field exists in the family file          → done
#   mentioned   the PascalCase name appears (doc comment: deferred,
#               re-declared on a base, or a computed getter)            → accounted for
#   MISSING     neither                                                 → a real gap
#
# The middle class matters: the port deliberately does NOT restate a property a
# base already carries, and deliberately keeps unhonoured ones documented on the
# field. Only the third class is a defect.
#
#   .\audit-coverage.ps1 [-Catalog <path>] [-Src <path>] [-Verbose]

param(
    [string]$Catalog = 'C:\kubuno-build\winforms-ref\out\winforms-catalog.json',
    [string]$Src     = 'Z:\projects\kubuno\desktop\crates\kubuno-desktop-controls\src'
)

# Which family file owns which .NET type.
$OWNER = @{
    'Control'                 = 'control.rs'
    'ButtonBase'              = 'buttons.rs'; 'Button' = 'buttons.rs'
    'CheckBox'                = 'buttons.rs'; 'RadioButton' = 'buttons.rs'
    'TextBoxBase'             = 'text.rs'; 'TextBox' = 'text.rs'
    'MaskedTextBox'           = 'text.rs'; 'RichTextBox' = 'text.rs'
    'ListControl'             = 'lists.rs'; 'ComboBox' = 'lists.rs'
    'ListBox'                 = 'lists.rs'; 'CheckedListBox' = 'lists.rs'
    'ScrollableControl'       = 'containers.rs'; 'ContainerControl' = 'containers.rs'
    'Form'                    = 'containers.rs'; 'UserControl' = 'containers.rs'
    'Panel'                   = 'containers.rs'; 'GroupBox' = 'containers.rs'
    'FlowLayoutPanel'         = 'layout_panels.rs'; 'TableLayoutPanel' = 'layout_panels.rs'
    'SplitContainer'          = 'layout_panels.rs'; 'SplitterPanel' = 'layout_panels.rs'
    'Splitter'                = 'layout_panels.rs'; 'TabControl' = 'layout_panels.rs'
    'TabPage'                 = 'layout_panels.rs'
    'Label'                   = 'labels.rs'; 'LinkLabel' = 'labels.rs'
    'PictureBox'              = 'labels.rs'; 'ProgressBar' = 'labels.rs'
    'ScrollBar'               = 'range.rs'; 'HScrollBar' = 'range.rs'; 'VScrollBar' = 'range.rs'
    'TrackBar'                = 'range.rs'; 'UpDownBase' = 'range.rs'
    'NumericUpDown'           = 'range.rs'; 'DomainUpDown' = 'range.rs'
    'DateTimePicker'          = 'datetime.rs'; 'MonthCalendar' = 'datetime.rs'
    'TreeView'                = 'views.rs'; 'ListView' = 'views.rs'
    'ToolStrip'               = 'toolstrip.rs'; 'MenuStrip' = 'toolstrip.rs'
    'StatusStrip'             = 'toolstrip.rs'; 'ToolStripDropDown' = 'toolstrip.rs'
    'ToolStripDropDownMenu'   = 'toolstrip.rs'; 'ContextMenuStrip' = 'toolstrip.rs'
}

# `AutoEllipsis` -> `auto_ellipsis`, `RightToLeft` -> `right_to_left`,
# `UseWaitCursor` -> `use_wait_cursor`, `MdiWindowListItem` -> `mdi_window_list_item`.
function ConvertTo-SnakeCase([string]$name) {
    # One rule: a separator before every capital that is not the first letter.
    # Two chained rules produced `back__color` for `BackColor` — a silent double
    # underscore that made every such property look missing.
    (($name -creplace '(?<!^)([A-Z])', '_$1') -replace '_+', '_').ToLower()
}

$json = Get-Content $Catalog -Raw | ConvertFrom-Json
$cache = @{}
$rows = @()

foreach ($type in $json) {
    $file = $OWNER[$type.name]
    if (-not $file) { continue }                       # out of scope for this wave
    $path = Join-Path $Src $file
    if (-not $cache.ContainsKey($file)) { $cache[$file] = Get-Content $path -Raw }
    $text = $cache[$file]

    foreach ($p in $type.declaredProperties) {
        $snake = ConvertTo-SnakeCase $p.name
        # A field declaration, or the same name used as a method/accessor.
        $isField = $text -match ("(?m)^\s*(pub\s+)?" + [regex]::Escape($snake) + "\s*:") `
                -or $text -match ("(?m)fn\s+" + [regex]::Escape($snake) + "\s*[(<]")
        # Named anywhere else: a doc comment saying deferred, re-declared, or
        # reached through Deref.
        $isMentioned = $text -match ('\b' + [regex]::Escape($p.name) + '\b')

        # A subclass that merely re-declares a base property must NOT restate it
        # — the value lives on the base and is reached through `Deref`. So
        # before calling anything missing, look for the field on `ControlBase`,
        # which is what every chain bottoms out at.
        if (-not $cache.ContainsKey('control.rs')) {
            $cache['control.rs'] = Get-Content (Join-Path $Src 'control.rs') -Raw
        }
        $onBase = $type.name -ne 'Control' -and
                  ($cache['control.rs'] -match ("(?m)^\s*(pub\s+)?" + [regex]::Escape($snake) + "\s*:"))

        $state = if ($isField) { 'field' }
                 elseif ($onBase) { 'inherited' }
                 elseif ($isMentioned) { 'mentioned' }
                 else { 'MISSING' }
        $rows += [pscustomobject]@{
            Type = $type.name; Property = $p.name; Snake = $snake; State = $state; File = $file
        }
    }
}

$byState = $rows | Group-Object State | Sort-Object Name
"=== coverage over $($rows.Count) declared properties across $(($rows | Select-Object -Expand Type -Unique).Count) types ==="
$byState | ForEach-Object { '{0,-10} {1,5}' -f $_.Name, $_.Count }

$missing = $rows | Where-Object State -eq 'MISSING'
if ($missing) {
    ''
    "=== $($missing.Count) NOT FOUND AT ALL (neither implemented nor documented) ==="
    $missing | Sort-Object Type, Property | ForEach-Object {
        '{0,-22} {1,-28} ({2})' -f $_.Type, $_.Property, $_.File
    }
} else {
    ''
    'No property is unaccounted for.'
}

# Machine-readable, for a diff between runs.
$rows | Export-Csv -NoTypeInformation -Encoding UTF8 `
    -Path (Join-Path (Split-Path $Catalog) 'coverage.csv')
