//! Kubuno's shape and size tokens — **re-exported, not restated**.
//!
//! The design system's tokens already live in
//! [`drive_app_controls::themes::shape`] (radii, control heights, the six text
//! sizes, the spacing scale, the shadow ramps), measured off the running web
//! app rather than read from class names. Declaring a second table here is how
//! two tables drift, so this module re-exports that one and adds **only** the
//! metrics the new primitives need and the web design system did not already
//! give a name.
//!
//! Every value is a DIP. Nothing here is multiplied by
//! [`Canvas::scale`](crate::Canvas::scale) — the renderer sets the D2D dpi, so
//! these are already device-independent.
//!
//! **Every constant below cites where it came from.** The first draft of this
//! file did not, and three of its numbers turned out to be invented: a 4 DIP
//! progress bar the web draws at 6 or 8, a 4 DIP slider track the web draws at
//! 6, and an 8 DIP radio dot the web draws at 10. A number without a source is
//! a defect here even when it happens to look right.

pub use drive_app_controls::themes::shape::{
    height, pill, radius, space, text, ShadowLayer, SHADOW_FLOAT, SHADOW_GREY, SHADOW_MENU,
};

/// Metrics for the controls Kubuno gains from the replica layer.
///
/// Sources, in the order of preference the brief sets out:
///
/// 1. a **named geometry** the web exports (`CHECKBOX_GEOMETRY`,
///    `RADIO_GEOMETRY`) — the strongest, since the web itself draws these on a
///    canvas and had to write the numbers down;
/// 2. the **Tailwind utilities** on the web component, converted (`h-1.5` = 6,
///    `py-2` = 8, `w-4` = 16);
/// 3. the **shipping desktop predecessor**, when the control exists there and
///    the port must match it pixel for pixel.
///
/// Where none of the three answers, the constant either goes away or says so
/// out loud. Exactly one is in the second case — [`control::GROUP_LABEL_INSET`],
/// for a control the web never had — and its documentation names it as a
/// decision rather than a measurement.
pub mod control {
    /// The check box, from `core/frontend/src/ui/checkboxCanvas.ts`:
    /// `CHECKBOX_GEOMETRY = { size: 18, border: 2, radius: 4, tick: 11 }`.
    /// The web draws its check box on a canvas too, so this is the same
    /// geometry running twice, not an interpretation of it.
    pub const CHECK_BOX: f32 = 18.0;
    pub const CHECK_BORDER: f32 = 2.0;
    pub const CHECK_RADIUS: f32 = 4.0;
    pub const CHECK_TICK: f32 = 11.0;

    /// The radio, from `core/frontend/src/ui/radioCanvas.ts`:
    /// `RADIO_GEOMETRY = { size: 18, ring: 2, dot: 10 }`, whose own comment
    /// reads « matches the CSS control it replaces to the pixel ».
    pub const RADIO_BOX: f32 = 18.0;
    pub const RADIO_RING: f32 = 2.0;
    pub const RADIO_DOT: f32 = 10.0;

    /// `gap-2` between the box and its label — `Checkbox.tsx` and `Radio.tsx`
    /// both open with `inline-flex items-start gap-2`.
    pub const CHECK_GAP: f32 = 8.0;

    /// The combo's chevron column: `w-4` on the indicator span
    /// (`Combobox.tsx`). The field itself is an input — take its height from
    /// [`height::BUTTON_MD`](super::height::BUTTON_MD) and its inset from [`space::MD`](super::space::MD) (`h-9`, `px-3`),
    /// which is why neither is restated here.
    pub const COMBO_ARROW: f32 = 16.0;

    /// A row in a combo's popup: `py-1.5` over a 20 DIP line (`Combobox.tsx`).
    /// A **menu** row is a different control and already has its own token,
    /// [`height::MENU_ITEM`](super::height::MENU_ITEM) — do not use this one for menus.
    pub const COMBO_ROW: f32 = 32.0;

    /// The scroll bar — **re-exported from the shipping predecessor**, not
    /// copied. `drive-app-controls/src/scrollbar/mod.rs` already publishes the
    /// whole set, sourced there against `::-webkit-scrollbar` in
    /// `core/frontend/src/index.css` and against WinUI's `ScrollBarSize`. A
    /// second copy here is exactly the drift this module exists to prevent.
    pub use drive_app_controls::scrollbar::{
        ARROW_SIZE as SCROLLBAR_ARROW, INDICATOR_WIDTH as SCROLLBAR_INDICATOR,
        MIN_THUMB as SCROLLBAR_THUMB_MIN, SCROLLBAR_SIZE as SCROLLBAR,
        THUMB_WIDTH as SCROLLBAR_THUMB,
    };

    /// A splitter's grab band: `w-3` on the web shell's pane resizer, which
    /// `drive-app/src/ui/metrics.rs` already carries as `RESIZE_RAIL` and
    /// documents as « wider than the 4 DIP blade resizer: it has to hold the
    /// grip pill that appears on hover ». The visible line inside it is the
    /// same hairline as any other rule, hence [`SEPARATOR`].
    pub const SPLITTER: f32 = 12.0;
    pub const SPLITTER_LINE: f32 = SEPARATOR;

    /// Where a group box's caption starts, measured from the frame's left
    /// edge.
    ///
    /// This one has **no external source, and says so**: the web design system
    /// has no group box at all, and the WinForms replica's own caption metrics
    /// are private to that crate. It is therefore a Kubuno decision — one step
    /// of the spacing scale ([`space::MD`](super::space::MD)) — rather than a measurement, and
    /// it is written here so that the decision is visible instead of hiding as
    /// a literal in a paint body.
    pub const GROUP_LABEL_INSET: f32 = super::space::MD;

    /// The slider, from `core/frontend/src/ui/RangeSlider.tsx`: an `h-1.5`
    /// track and a `thumb(size = 12)` disc. [`SLIDER_THUMB`] is the disc plus
    /// the `0 0 0 2px` halo on each side, which is what the pointer actually
    /// has to hit.
    pub const SLIDER_TRACK: f32 = 6.0;
    pub const SLIDER_DISC: f32 = 12.0;
    pub const SLIDER_THUMB: f32 = 16.0;

    /// The progress bar, from `core/frontend/src/ui/ProgressBar.tsx`:
    /// `TRACK = { sm: 'h-1.5', md: 'h-2' }`. There is no 4 DIP progress bar in
    /// this design system — the first draft of this file invented one.
    pub const PROGRESS_SM: f32 = 6.0;
    pub const PROGRESS_MD: f32 = 8.0;

    /// Tabs, from `core/frontend/src/ui/Tabs.tsx`. The underline is
    /// `before:h-[3px] before:mx-2 before:rounded-t-[3px]`; the tab's own
    /// height is its padding plus a 20 DIP line — `pt-2 pb-[11px]` for `md`
    /// and `pt-1.5 pb-[9px]` for `sm`. Note that neither lands on 40: the
    /// asymmetric bottom padding is what leaves room for the indicator.
    pub const TAB_UNDERLINE: f32 = 3.0;
    pub const TAB_UNDERLINE_INSET: f32 = 8.0;
    pub const TAB_MD: f32 = 39.0;
    pub const TAB_SM: f32 = 35.0;

    /// A rule: `h-px` / `w-px` on `Separator.tsx`. One DIP, which
    /// `Canvas::stroke_rounded` already snaps to one physical pixel — do not
    /// try to out-clever it.
    pub const SEPARATOR: f32 = 1.0;
}
