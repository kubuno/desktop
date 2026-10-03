# WinForms reference — how the control library knows what to reproduce

`kubuno-desktop-controls` reproduces the WinForms control surface. Its reference is not
prose and not a screenshot found on the web: it is the **shipping
`System.Windows.Forms` assembly**, read two ways.

Both tools need the .NET SDK with the Windows Desktop runtime (`dotnet
--list-runtimes` must list `Microsoft.WindowsDesktop.App`).

## 1. `catalog/` — the property surface, by reflection

Walks every public `Control` subclass in the assembly and writes, for each one:
its inheritance chain, the properties it **declares** (name, type, settability,
category, the `[DefaultValue]` the toolkit itself carries, and the docs'
description) and its declared events.

```powershell
cd tools\winforms-ref\catalog
dotnet run -- C:\kubuno-build\winforms-ref\out
```

Produces:

* `winforms-catalog.json` — the full surface (~300 KB), the file the
  implementation is written against.
* `winforms-hierarchy.txt` — the tree at a glance, with how many properties each
  level adds. A copy is checked in next to this README.

**Why it matters.** `Control` declares 52 settable properties inherited by all 64
other types; `ButtonBase` adds 19 shared by `Button`, `CheckBox` and
`RadioButton` — and `Button` itself declares **two**. Writing each control from
scratch would restate that surface 65 times and let it drift. The library
mirrors the chain with composition + `Deref` instead, so a property lives in one
place, exactly where .NET declares it.

## 2. `gallery/` — the visual reference, painted by the real toolkit

Builds one form per control family, in the states that change how a control is
painted (every `FlatStyle`, disabled, `CheckState.Indeterminate`,
`Appearance.Button`, each `BorderStyle`, each `TickStyle`, each `SizeMode`…),
and captures each form with `Control.DrawToBitmap` — so the PNG is exactly what
the toolkit painted, not a screen scrape.

```powershell
cd tools\winforms-ref\gallery
dotnet build
.\bin\Debug\net9.0-windows\gallery.exe C:\kubuno-build\winforms-ref\shots
```

Produces `01-buttonbase.png` … `10-grid.png`.

Three details worth keeping:

* The form names its **design DPI** (`AutoScaleDimensions = 96,96`). `AutoScaleMode.Dpi` scales against that baseline, and an unset `(0,0)` means it scales by nothing — which produced a sheet that was *internally inconsistent*: fonts and system metrics at the real 175 %, every explicit `Width`/`Height` still at 96 dpi. A 240-wide combo then carried 175 % text, and every comparison against that sheet read as a port defect when it was a harness one.


* The sheets **auto-size**. WinForms scales the font before it scales
  design-time bounds, so a fixed-pixel sheet overlaps at 175 % DPI and would
  misreport every control's metrics.
* The layout is re-run depth-first once the window handle (and its real DPI)
  exists, for the same reason.

### Known limits of the capture

`DrawToBitmap` asks a control to print itself (`WM_PRINTCLIENT`). A few **native
common controls** honour that only partially, so the sheet shows less than the
screen would:

* `ScrollBar` (`HScrollBar` / `VScrollBar`) prints its **thumb but not its arrow
  buttons or track**. The thumb's length is still meaningful — it is
  proportional to `LargeChange`, which is why the sheet shows two bars with
  different `LargeChange` values. Take the arrows and track geometry from the
  documentation and from `SystemInformation`, not from the PNG.

Where a sheet is silent, the catalogue and the documentation decide — never fill
the gap by guessing.

## 3. `audit-coverage.ps1` — is anything missing?

Cross-checks every property the catalogue says a type **declares** against the
port's source, and classifies it:

| class | meaning |
|---|---|
| `field` | the snake_case field exists in the family file |
| `inherited` | a base carries it — the port deliberately does **not** restate a re-declared property |
| `mentioned` | named in a doc comment: deferred, or a computed getter |
| `MISSING` | neither — a real gap |

```powershell
.\audit-coverage.ps1        # summary + the gap list; writes coverage.csv
```

Only the last class is a defect. Diff `coverage.csv` between runs to see whether
a change closed gaps or opened them.

## 4. `parity/` — do the numbers agree?

The deepest check: the same cases run through **real WinForms controls** and
through the port, with the results diffed. The case list is declared **once** and
read by both probes, so the two cannot silently drift apart in what they test.

```powershell
cd parity\layout-winforms ; dotnet run          # then, from the repo root:
cargo run -p kubuno-desktop-controls --example parity_layout -j 1
cd parity ; .\compare-layout.ps1                # exit 1 on any delta > 0.5 DIP
```

Same shape for `panels-*` and `range-*`.

Two rules for any probe you add:

* **Set `Application.SetUnhandledExceptionMode(UnhandledExceptionMode.ThrowException)`
  before the first window.** An exception thrown inside a control's `WndProc`
  does not propagate out of the `SendMessage` that provoked it — WinForms hands
  it to `Application.OnThreadException`, which shows a modal dialog and waits.
  An unattended run then hangs, and a stuck run looks exactly like a green one.
* **Compare in one space, and say which.** The probes report display-relative
  rectangles; a container's own origin convention is reported separately rather
  than charged to every child.
* **Anything that measures or captures a window must be DPI-aware.** Call
  `SetProcessDPIAware` (or run per-monitor-v2) first. A DPI-unaware
  `GetClientRect` reports the *virtualised* size — 1486 px where the client is
  really 2601 — and a DPI-unaware `CopyFromScreen` crops the capture to that
  same virtual box, so content on the right simply vanishes from the image.
  Both invent geometry bugs that do not exist: one such phantom cost two
  investigations here before `examples/frame_probe.rs` measured the host
  directly and showed `Frame.size` and the painter's scale had agreed all along.

## 5. What the reference has already caught

Facts found by running the real toolkit rather than reading about it — each is
reproduced, and tested, in the port:

* `NumericUpDown.Value` **throws** when set outside the current `Minimum` /
  `Maximum`. `Maximum` must be raised *before* `Value`: the default maximum is
  100, so `Value = 255` fails.

Add to this list whenever the toolkit contradicts an assumption.
