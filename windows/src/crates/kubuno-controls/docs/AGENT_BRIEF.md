# Control-library brief — the shared contract

Everyone implementing a control family works from this file. It exists so that
ten families, written independently, come out as **one** library rather than ten
dialects.

## 1. The reference is the toolkit, not prose

Two artefacts are generated from the shipping `System.Windows.Forms` assembly.
Never guess a property, a default or a metric that one of them can answer.

| Artefact | Path | What it answers |
|---|---|---|
| Property catalogue | `C:\kubuno-build\winforms-ref\out\winforms-catalog.json` | Every control type, its inheritance chain, and every designer-visible property it **declares** — with type, settability, category, declared default and the docs' own description. |
| Hierarchy map | `C:\kubuno-build\winforms-ref\out\winforms-hierarchy.txt` | The full tree with the count of properties each level adds. |
| Reference sheets | `C:\kubuno-build\winforms-ref\shots\NN-family.png` | The real controls, painted by the real toolkit, in the states that change their painting. |

Regenerate with:

```powershell
cd C:\kubuno-build\winforms-ref\catalog ; dotnet run -- C:\kubuno-build\winforms-ref\out
cd C:\kubuno-build\winforms-ref\gallery ; dotnet build ; .\bin\Debug\net9.0-windows\gallery.exe C:\kubuno-build\winforms-ref\shots
```

Microsoft's own documentation (learn.microsoft.com) is the right source for
*semantics* — what a property means, how two of them interact, what order they
must be set in. Use it, and cite the behaviour in a comment when it is
surprising. One real example already found: `NumericUpDown.Value` throws if it
falls outside the current `Minimum`/`Maximum`, so `Maximum` must be set first.

## 2. Do not re-implement what a base already carries

This is the single most important rule. `Control` declares **52 settable
properties** inherited by all 64 other types. `ButtonBase` adds 19 shared by
`Button`, `CheckBox` and `RadioButton` — and `Button` itself declares **two**.

The chain is mirrored with **composition + `Deref`**, so each type owns exactly
what its .NET counterpart *declares*:

```rust
pub struct ButtonBase { control: ControlBase, /* the 19 */ }
impl std::ops::Deref for ButtonBase {
    type Target = ControlBase;
    fn deref(&self) -> &ControlBase { &self.control }
}
impl std::ops::DerefMut for ButtonBase { /* … */ }

pub struct Button { base: ButtonBase, /* the 2 */ }
// Button derefs to ButtonBase, which derefs to ControlBase.
```

Before writing a property, check the catalogue for which type **declares** it.
If it is declared on a base, it belongs on the base — even if only your control
happens to use it today.

### The re-declaration trap

A subclass often **re-declares** a property its base already owns, purely to
change an attribute (a new default, a new designer category, a `Browsable`
flag). `ButtonBase` re-declares `Text`, `BackColor` and `AutoSize`; `Button`
re-declares `AutoSizeMode`; `RadioButton` re-declares `TabStop`. These appear in
the catalogue's `declaredProperties` for the subclass, but they are **not new
storage** — the value still lives on `ControlBase`, reached through `Deref`.

So: a re-declared property gets **no field**. What it may get is a different
**default** in that type's `Default` impl (`RadioButton.TabStop` starts `false`
where `Control`'s starts `true`; `TextAlign` is `MiddleCenter` on `ButtonBase`
but `MiddleLeft` on `CheckBox`/`RadioButton`). Give each such case a test.

### Two counts, one source of truth

`winforms-hierarchy.txt` prints a **raw** property count; the JSON catalogue's
`declaredPropertyCount` is **filtered** to the designer-visible surface. They
differ (ButtonBase: 19 raw vs 18 filtered). **The JSON is the source of truth.**

## 3. The contract each control implements

```rust
impl Control for Button {
    fn control(&self) -> &ControlBase;
    fn control_mut(&mut self) -> &mut ControlBase;
    fn preferred_size(&self, c: &dyn Canvas) -> Size;  // GetPreferredSize
    fn paint(&self, c: &dyn Canvas, bounds: Rect);
    fn type_name(&self) -> &'static str;
    // `hit_test` has a default: the whole box, when visible and enabled.
}
```

Rules that hold for every control:

* **Paint only through `Canvas`.** Colours come from `c.theme()` and text
  formats from `c.formats()`. A hard-coded colour, font family or point size is
  a defect — the library is themed and DPI-aware.
* **Never multiply a metric by `c.scale()`.** `Renderer` calls
  `ctx.SetDpi(dpi, dpi)`, so the Direct2D space is **already DIP**: a rectangle
  at `y = 10.0` lands at 10 DIP, which is 17.5 physical pixels at 175 % — with
  no arithmetic from you. `drive-app-controls` scales in exactly **zero**
  places, and so should a control. Multiplying makes everything 1.75× too big
  at 175 % and *invisible at 100 %*, which is how it slips past tests.
  `c.scale()` is legitimate only when you want a **physical-pixel** thickness,
  and then you divide (`1.0 / c.scale()`) — but `stroke_rounded` already draws
  crisp hairlines, so in practice you need it nowhere.
* **Glyphs are geometry, not text.** Arrows, chevrons, ticks and check marks go
  through `c.vector_icon(name, …)`. The embedded face has no `◄ ► ▲ ▼ ✓`
  characters, so drawing them as text renders tofu boxes. If a geometry you need
  is missing from `assets/lucide-icons.txt`, ask for it to be added.
* **No timer, thread, I/O or global state.** A control is a value that knows how
  to measure and paint itself.
* **Geometry stays pure.** Anything a test could check without a window must be
  a free function or a method that takes no canvas — the way `layout::layout`
  resolves `Dock` then `Anchor`.
* **Honour or declare.** A property you do not yet honour is documented as such
  in a doc comment on the field. It is never silently treated as another value,
  and never quietly dropped from the struct.
* **Defaults come from the catalogue**, via `Default` impls, and are covered by
  a test that asserts them.

## 4. Comments and language

Source comments and doc comments are **in English**, and they explain *why*, not
*what* — matching the house style of `control.rs` and `layout.rs`. Read those two
files first; write code that looks like them. User-visible strings in demo forms
are in French, like the rest of the product.

## 5. Tests

Every family carries unit tests for, at least:

* the declared defaults (asserted against the catalogue),
* the measurement arithmetic (`preferred_size` under padding/auto-size),
* any state machine the control owns (`CheckState` cycling, `Value` clamping,
  selection rules),
* the ordering traps found in the reference (document them in the test name).

`cargo test -p kubuno-controls` and
`cargo clippy -p kubuno-controls --all-targets -- -D warnings` must both pass.

## 6. Demo form

Each family adds a builder to the demo application so its controls can be
compared side by side with the reference sheet. Keep the same groupings and the
same captions as the corresponding `NN-family.png`, so the two can be put next to
each other.

## 7. What is out of scope for this wave

`DataGridView` (82 declared properties), `PropertyGrid`, `WebBrowser` and the
`AxHost` interop family. They are a later wave; do not stub them.
