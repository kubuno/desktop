# `kubuno-desktop-ui` — the shared brief

Read this before writing a line. It is the contract every family in this crate
obeys; the mistakes it forbids are ones this codebase has already paid for.

## What we are building

The Kubuno desktop design system, **on top of** the WinForms replicas in
`kubuno-desktop-controls`. Two layers already exist:

| layer | crate | owns |
|---|---|---|
| replicas | `kubuno-desktop-controls` | the .NET property surface, defaults, state machines, layout engines — verified against the real toolkit (52/52 layout cases, 659/659 properties) |
| painting | `kubuno-drive-desktop-app-controls` | `Canvas` (theme, DirectWrite formats, vector icons), and the design tokens in `themes::shape` |

A Kubuno primitive is a **replica with Kubuno pixels**. Nothing more.

```rust
pub struct Button {
    inner: kc::buttons::Button,   // the model — never restated
    pub variant: Variant,         // what Kubuno adds
    pub size: Size,
}
impl Deref for Button { type Target = kc::buttons::Button; … }
```

## The rules

1. **Own a replica, never restate it.** `text`, `enabled`, `padding`, `dock`,
   `anchor`, `min_size`, `fore_color`… all live in the replica and are reached
   through `Deref`. Add a field here only for a concept .NET does not have.
   *If you find yourself declaring `pub enabled: bool`, stop — it already
   exists one layer down, and two copies will disagree.*

2. **Take geometry and state from the replica.** `CheckBox::toggle()` already
   cycles three states in the toolkit's order; `ScrollBar` already computes the
   thumb from `value/minimum/maximum/large_change`; `layout::layout` already
   resolves Dock and Anchor the way WinForms does, including the
   `⌊cur/2⌋ − ⌊prev/2⌋` re-centring nobody guesses right. Re-deriving any of it
   here re-derives its bugs.

3. **Paint through `Canvas` only.** Colours from `c.theme()`, type from
   `c.formats()`, glyphs from `c.vector_icon(...)`. **No colour literal in a
   paint body.** If a colour you need is not a theme token, add the token to
   `kubuno-drive-desktop-app-controls/src/themes/mod.rs` (both palettes) — that is what was
   done for `link_visited`, and it is the only acceptable answer.

4. **Never multiply by `c.scale()`.** The renderer calls `SetDpi`, so D2D space
   is already DIP. `kubuno-drive-desktop-app-controls` scales in exactly zero places. A
   previous round put 30 double-scaling bugs into this codebase by ignoring
   this line.

5. **One metric table.** `crate::metrics` re-exports `themes::shape` and adds
   only what the web never described. A literal `36.0` in a paint body is a
   defect even when the number is right.

6. **Glyphs are geometry, not text.** Arrows, chevrons, ticks: `c.vector_icon`
   with a name from `assets/lucide-icons.txt`. Drawing them as characters gives
   tofu — the embedded face does not carry them. Missing geometry? Add it to
   that file.

7. **Never read `self.bounds` to paint.** Paint into the `bounds` **argument**.
   Reading the model's own rectangle instead is a bug we have shipped, found
   and tested for three times (`Label`, `PictureBox`, `LinkLabel`).

8. **No `unwrap()` outside tests.** Comments in **English**.

## The non-regression gate

Several of these primitives already ship, hand-written, in
`kubuno-drive-desktop-app-controls`, and the shell and Drive paint with them **today**
(`button` is called 149 times by the shell alone). Rebuilding them is only
worth doing if nobody can tell.

So: when your family has a predecessor, its port must be **pixel-identical** to
it for the same inputs. Prove it — a unit test that compares the derived
geometry, plus a page in the gallery that paints old and new side by side.

| new family | predecessor to match |
|---|---|
| `buttons` | `button.rs` (`Button`, `IconButton`), `switch.rs` |
| `text` | `edit_box/`, `omnibar/` |
| `range` | `scrollbar/` |
| `containers` | `grid_splitter/` |
| `navigation` | `toolbar/`, `sidebar/`, `breadcrumb_bar/` |

Where there is **no** predecessor (check box, radio, combo, tabs, group box,
progress, slider, tooltip, badge, list box, tree, panel), the reference is the
**web** design system: `core/frontend/src/ui/*.tsx` and `theme.css`. Measure it
if you can reach it, read it if you cannot, and say in a doc comment which you
did. Do not invent a number.

## Deliverables per family

* `src/<family>.rs` — the primitives, each `impl Widget` + `Deref` to its
  replica.
* Unit tests: measurement, hit-testing, state transitions, and — where a
  predecessor exists — geometry equality with it.
* A gallery page at `examples/gallery/pages/<family>.rs` exposing
  `pub fn draw(c: &dyn Canvas, f: &Frame)`, showing every variant × state, and
  the old/new pair where one exists.
* Doc comments that say **where each number came from**.

`cargo clippy -p kubuno-desktop-ui --all-targets -- -D warnings` must pass.
