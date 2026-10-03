# Opens a gallery page, captures its client area, kills it.
#
#   .\shoot.ps1 -Page buttons -Out C:\kubuno-build\ui\buttons.png
#
# The one rule that matters here is the first line of code: **this process must
# be DPI-aware before it asks a window anything**. A DPI-unaware caller gets
# `GetClientRect` in VIRTUALISED pixels (1486 where the client is really 2601)
# and `CopyFromScreen` then crops the capture to that same virtual box, so
# whatever sits on the right simply vanishes from the image. Both invent
# geometry bugs that do not exist -- one such phantom cost two investigations in
# this repo before a probe measured the window directly and showed the port had
# been right all along.

param(
    [string]$Page = 'buttons',
    # Which binary to shoot. Defaults to the gallery; point it at
    # `kubuno-desktop.exe` to capture the shipping shell itself, which is the
    # only way to check that a migration carried the *call sites* over
    # correctly -- parity on the gallery only proves the control, not its use.
    [string]$Exe,
    [Parameter(Mandatory = $true)][string]$Out,
    [switch]$Dark,
    [int]$WaitMs = 3500,
    # Printed by the gallery when KUBUNO_UI_DUMP is set: the geometry of every
    # old/new pair, so `split-diff.ps1` never has to be aimed by eye.
    [switch]$Dump
)

Add-Type -AssemblyName System.Drawing
Add-Type @"
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;
public class Win {
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h, out R r);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out R r);
  [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr dc, uint flags);
  [DllImport("user32.dll")] static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] static extern bool AttachThreadInput(uint from, uint to, bool attach);
  [DllImport("kernel32.dll")] static extern uint GetCurrentThreadId();

  // Windows refuses SetForegroundWindow to a process that does not already own
  // the foreground -- the call returns false and the window stays buried, which
  // is why the first captures here came back showing the editor on top of the
  // gallery. Attaching our input queue to the foreground thread's makes us,
  // briefly, part of the window that DOES own it, and the request is granted.
  public static void ForceForeground(IntPtr h) {
    IntPtr fg = GetForegroundWindow();
    uint other;
    uint fgThread = GetWindowThreadProcessId(fg, out other);
    uint mine = GetCurrentThreadId();
    AttachThreadInput(fgThread, mine, true);
    // Raise it to the front WITHOUT making it sticky-topmost. The old code set
    // HWND_TOPMOST (-1) and never cleared it, so the captured window stayed
    // pinned above everything else long after the shot — a real annoyance. The
    // TOPMOST-then-NOTOPMOST pair forces it to the front for this instant and
    // leaves it in the normal z-order.
    SetWindowPos(h, (IntPtr)(-1), 0, 0, 0, 0, 0x1 | 0x40);   // TOPMOST | NOSIZE | SHOWWINDOW
    SetWindowPos(h, (IntPtr)(-2), 0, 0, 0, 0, 0x1 | 0x2);    // NOTOPMOST | NOSIZE | NOMOVE
    BringWindowToTop(h);
    SetForegroundWindow(h);
    AttachThreadInput(fgThread, mine, false);
  }
  [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc cb, IntPtr p);
  [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetClassNameW(IntPtr h, StringBuilder s, int n);
  delegate bool EnumProc(IntPtr h, IntPtr p);

  // The gallery is a CONSOLE binary, so Process.MainWindowHandle hands back its
  // console window -- capturing that gives a screenshot of a black terminal
  // sitting on the desktop, which is exactly what happened the first time.
  // Walk the process' top-level windows instead and skip the console classes.
  public static IntPtr FindPaintWindow(uint want) {
    IntPtr found = IntPtr.Zero;
    EnumWindows(delegate(IntPtr h, IntPtr p) {
      uint pid; GetWindowThreadProcessId(h, out pid);
      if (pid != want || !IsWindowVisible(h)) return true;
      var sb = new StringBuilder(256);
      GetClassNameW(h, sb, sb.Capacity);
      string cls = sb.ToString();
      if (cls == "ConsoleWindowClass" || cls == "CASCADIA_HOSTING_WINDOW_CLASS" ||
          cls == "PseudoConsoleWindow") return true;
      found = h;
      return false;
    }, IntPtr.Zero);
    return found;
  }
  [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr h, ref P p);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool BringWindowToTop(IntPtr h);
  [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr after, int x, int y, int cx, int cy, uint f);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int c);
  [StructLayout(LayoutKind.Sequential)] public struct R { public int L, T, Rr, B; }
  [StructLayout(LayoutKind.Sequential)] public struct P { public int X, Y; }
}
"@
[void][Win]::SetProcessDPIAware()

$root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
if ($Exe) {
    $exe = $Exe
    # Decide by the BINARY, not by whether -Exe was passed: pointing -Exe at the
    # gallery used to silently drop -Page and capture whatever page came first,
    # which reads as "my page renders like the buttons page" rather than as a
    # tooling slip.
    $isGallery = (Split-Path -Leaf $exe) -eq 'gallery.exe'
}
else {
    $exe = Join-Path $env:CARGO_TARGET_DIR 'debug\examples\gallery.exe'
    if (-not (Test-Path $exe)) { $exe = Join-Path $root 'target\debug\examples\gallery.exe' }
    $isGallery = $true
}
if (-not (Test-Path $exe)) { Write-Error "$exe not found - build it first"; exit 2 }

if ($Dark) { $env:KUBUNO_UI_DARK = '1' } else { Remove-Item Env:\KUBUNO_UI_DARK -ErrorAction SilentlyContinue }
if ($Dump) { $env:KUBUNO_UI_DUMP = '1' } else { Remove-Item Env:\KUBUNO_UI_DUMP -ErrorAction SilentlyContinue }

$dumpFile = [System.IO.Path]::ChangeExtension($Out, '.pairs.txt')
$dir = Split-Path -Parent $Out
if ($dir -and -not (Test-Path $dir)) { New-Item -ItemType Directory -Force -Path $dir | Out-Null }

# Not named $args: that is a PowerShell automatic variable, and -ArgumentList
# rejects an empty collection outright, so the two cases are separate calls.
$stdout = [System.IO.Path]::ChangeExtension($Out, '.log')
$p = if ($isGallery) {
    Start-Process -FilePath $exe -ArgumentList @('--page', $Page) -PassThru `
        -RedirectStandardError $dumpFile -RedirectStandardOutput $stdout
}
else {
    Start-Process -FilePath $exe -PassThru `
        -RedirectStandardError $dumpFile -RedirectStandardOutput $stdout
}
Start-Sleep -Milliseconds $WaitMs
$p.Refresh()
$h = [Win]::FindPaintWindow([uint32]$p.Id)
if ($h -eq [IntPtr]::Zero) { Write-Error 'the window never opened'; $p.Kill(); exit 1 }

[void][Win]::ShowWindow($h, 5)
[Win]::ForceForeground($h)
Start-Sleep -Milliseconds 1500

$r = New-Object Win+R
[void][Win]::GetClientRect($h, [ref]$r)
$pt = New-Object Win+P
[void][Win]::ClientToScreen($h, [ref]$pt)
$w = $r.Rr - $r.L
$ht = $r.B - $r.T

# Read the screen -- but only after ForceForeground has actually raised the
# window. PrintWindow is NOT an alternative here: the gallery draws through a
# DXGI swap chain, whose contents never reach the GDI device context, so
# PrintWindow (even with PW_RENDERFULLCONTENT) returns a solid black frame.
# That was tried, and the black PNG is the reason this comment exists.
$bmp = New-Object System.Drawing.Bitmap($w, $ht)
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.CopyFromScreen($pt.X, $pt.Y, 0, 0, (New-Object System.Drawing.Size($w, $ht)))
$bmp.Save($Out, [System.Drawing.Imaging.ImageFormat]::Png)
$g.Dispose(); $bmp.Dispose()
$p.Kill()

"saved $Out  (${w}x${ht} physical px, client at $($pt.X),$($pt.Y))"
if ($Dump -and (Test-Path $dumpFile)) { "pairs: $dumpFile" }
