//! Shape and size tokens of the Kubuno design system.
//!
//! The web system exposes these as CSS custom properties; the desktop had no
//! equivalent (every radius and height was an inline literal), which is how the
//! two drifted apart. Values come from `core/frontend/src/theme.css` and
//! `index.css`, cross-checked against the live DOM.
//!
//! All values are DIPs, matching the web's CSS pixels one to one.

/// Corner radii. `--radius-*` in the web, plus the two float/window radii.
pub mod radius {
    /// `--radius-sm` / `--radius-md`: buttons, inputs, small controls.
    pub const SM: f32 = 4.0;
    /// `--radius-lg`: dialogs (`--kb-window-radius`), callouts.
    pub const LG: f32 = 6.0;
    /// `--radius-xl` / `--radius-2xl`: cards, panels, module area.
    pub const XL: f32 = 8.0;
    /// `--kb-float-radius`: menus, popovers, toasts.
    pub const FLOAT: f32 = 10.0;
    /// The "New" button's distinctive squircle.
    pub const NEW_BUTTON: f32 = 16.0;
    /// A menu row's highlight pill.
    pub const MENU_ITEM: f32 = 6.0;
    /// A launcher tile (`WaffleMenu`: `rounded-[16px]`).
    pub const TILE: f32 = 16.0;
    /// Stand-in for `rounded-full` on a row: half of the row height. Use
    /// `pill(height)` rather than this constant when the height is known.
    pub const PILL: f32 = 9999.0;
}

/// Radius that renders a rect as a full pill, given its height.
pub fn pill(height: f32) -> f32 {
    height / 2.0
}

/// Control heights.
pub mod height {
    /// `Button size="sm"` (h-8).
    pub const BUTTON_SM: f32 = 32.0;
    /// `Button size="md"` (h-9) and `Input` — the default.
    pub const BUTTON_MD: f32 = 36.0;
    /// `Button size="lg"` (h-11).
    pub const BUTTON_LG: f32 = 44.0;
    /// A `MenuDropdown` action row (5px + 20px line + 5px).
    pub const MENU_ITEM: f32 = 30.0;
    /// A sidebar nav row.
    pub const SIDEBAR_ROW: f32 = 36.0;
    /// A file row at the default ("normal") density.
    pub const FILE_ROW: f32 = 40.0;
    /// The "New" button, expanded and collapsed.
    pub const NEW_BUTTON: f32 = 56.0;
    pub const NEW_BUTTON_COLLAPSED: f32 = 48.0;
    /// A dialog's accent title bar (`min-h-11`).
    pub const DIALOG_TITLEBAR: f32 = 44.0;
}

/// Text sizes. The web system allows SIX and no others.
///
/// Aligned role by role on `core/frontend/src/theme.css` (`--kb-text-*`): a web
/// CSS px is one DIP, so a role renders at the same physical size on both
/// targets at any scale (at 175 %, 13.5 px = 13.5 DIP = 23.6 device pixels).
/// Only the face differs: Segoe UI Variable here, Plus Jakarta Sans on the web.
pub mod text {
    /// `--kb-text-micro` (10.5 px): badges, counters.
    pub const MICRO: f32 = 10.5;
    /// `--kb-text-meta` (11.5 px; also the host's `text-xs`): metadata,
    /// captions, section labels.
    pub const META: f32 = 11.5;
    /// `--kb-text-body` (13.5 px; also the host's `text-sm`): the default —
    /// labels, fields, menus, tabs. It used to be the OS form size (12); the
    /// desktop now follows the web's body step like every other role.
    pub const BODY: f32 = 13.5;
    /// `--kb-text-heading` (15.5 px): section headers, card and window titles.
    pub const HEADING: f32 = 15.5;
    /// `--kb-text-title` (21.5 px): the object name heading a panel.
    pub const TITLE: f32 = 21.5;
    /// `--kb-text-page` (22.5 px): a page title (`h1`), the page header's own step
    /// as on the web (`Role::Page` in `kubuno_desktop_ui`, `Role="Page"` on a view's `Label`).
    pub const PAGE: f32 = 22.5;
    /// `--kb-text-page` inside the administration console (`.kb-admin`).
    pub const PAGE_ADMIN: f32 = 27.5;
    /// `--font-weight-medium`. Note that buttons are NEVER bold in this system.
    pub const WEIGHT_MEDIUM: f32 = 600.0;
}

/// Spacing scale (`--kb-space-*`).
pub mod space {
    pub const XXS: f32 = 2.0;
    pub const XS: f32 = 4.0;
    pub const SM: f32 = 8.0;
    pub const MD: f32 = 12.0;
    pub const LG: f32 = 16.0;
    pub const XL: f32 = 24.0;
    pub const XXL: f32 = 32.0;
}

/// One layer of a CSS `box-shadow`: vertical offset, blur radius, SPREAD and
/// opacity — in that order, as they are written in CSS.
///
/// `spread` is the fourth value and it is easy to skip: it inflates the shape
/// BEFORE the blur, so dropping it makes a shadow markedly tighter than the
/// one the web draws.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShadowLayer {
    pub dy:      f32,
    pub blur:    f32,
    pub spread:  f32,
    pub opacity: f32,
}

/// `--kb-shadow-float`: menus, popovers, toasts. Four layers, so the surface
/// reads as lifted well off the page rather than merely outlined.
///
/// Careful with this one on a SMALL surface: its outer layers reach 40 px, and
/// a suggestion panel wearing all four reads as a grey cloud rather than a
/// lifted sheet. Use [`SHADOW_MENU`] for those.
pub const SHADOW_FLOAT: [ShadowLayer; 4] = [
    ShadowLayer { dy: 2.0, blur: 6.0, spread: 0.0, opacity: 0.14 },
    ShadowLayer { dy: 8.0, blur: 18.0, spread: 0.0, opacity: 0.12 },
    ShadowLayer { dy: 18.0, blur: 28.0, spread: 0.0, opacity: 0.08 },
    ShadowLayer { dy: 32.0, blur: 40.0, spread: 0.0, opacity: 0.05 },
];

/// Google's own elevation 2 — what its menus and dropdowns actually wear,
/// verbatim from the CSS its web apps serve:
/// `0 1px 2px 0 rgba(60,64,67,.3), 0 2px 6px 2px rgba(60,64,67,.15)`.
/// Note the `2px` SPREAD on the second layer: it is what gives the shadow its
/// body. Paired with [`SHADOW_GREY`].
pub const SHADOW_MENU: [ShadowLayer; 2] = [
    ShadowLayer { dy: 1.0, blur: 2.0, spread: 0.0, opacity: 0.30 },
    ShadowLayer { dy: 2.0, blur: 6.0, spread: 2.0, opacity: 0.15 },
];

/// Google shadows are never pure black: they use this grey, which keeps the
/// edge from looking sooty over a blue-tinted page background.
pub const SHADOW_GREY: (f32, f32, f32) = (60.0 / 255.0, 64.0 / 255.0, 67.0 / 255.0);

/// The waffle panel's own shadow, verbatim from the style it sets inline:
/// `0 4px 8px 3px rgba(0,0,0,.15), 0 1px 3px rgba(0,0,0,.3)`. This one really is
/// pure BLACK, not [`SHADOW_GREY`] — hence the colour being a parameter.
pub const SHADOW_WAFFLE: [ShadowLayer; 2] = [
    ShadowLayer { dy: 4.0, blur: 8.0, spread: 3.0, opacity: 0.15 },
    ShadowLayer { dy: 1.0, blur: 3.0, spread: 0.0, opacity: 0.30 },
];

pub const SHADOW_BLACK: (f32, f32, f32) = (0.0, 0.0, 0.0);

/// `--kb-shadow-window`: dialogs.
pub const SHADOW_WINDOW: [ShadowLayer; 1] =
    [ShadowLayer { dy: 6.0, blur: 18.0, spread: 0.0, opacity: 0.24 }];

/// The "New" button's Material elevation 1, and elevation 2 on hover.
pub const SHADOW_NEW_BUTTON: [ShadowLayer; 3] = [
    ShadowLayer { dy: 3.0, blur: 1.0, spread: -2.0, opacity: 0.20 },
    ShadowLayer { dy: 2.0, blur: 2.0, spread: 0.0, opacity: 0.14 },
    ShadowLayer { dy: 1.0, blur: 5.0, spread: 0.0, opacity: 0.12 },
];
