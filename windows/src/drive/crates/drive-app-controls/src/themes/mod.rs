//! Kubuno palette + system theme detection.
//!
//! The values mirror the web design system so the desktop app and the web
//! module read as one product: light comes from `core/frontend/src/theme.css`
//! (`@theme` block) and `index.css`, dark from `core/themes/kubuno-dark/
//! theme.json`. Drive-specific selection tints come from the theme's
//! `modules/drive.css`.
//!
//! Field names are kept from the original Fluent port so the drawing code did
//! not have to be rewritten wholesale; the comment on each field names the web
//! token it now carries.

use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;

pub mod shape;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ThemeMode {
    Light,
    Dark,
}

/// Resolved color palette for the current theme. `Clone` so a subtree can paint with a copy whose
/// tokens are overridden (a control's `BackColor`/`ForeColor`, `kubuno_controls::styled`).
#[derive(Clone)]
pub struct Theme {
    pub mode: ThemeMode,
    /// Fallback window background when Mica is unavailable.
    pub window_background: D2D1_COLOR_F,
    /// Title-bar band tint. Derived from the DWM accent color when the user
    /// enables "accent color on title bars"; alpha 0 lets Mica show instead.
    pub titlebar_background: D2D1_COLOR_F,
    /// Title-bar tint when the window has LOST focus
    /// (`AccentColorInactive` — desaturated gray instead of the accent).
    pub titlebar_background_inactive: D2D1_COLOR_F,
    /// Separator between inactive tabs on the title-bar band.
    pub tab_separator: D2D1_COLOR_F,
    /// Content layer background (the rounded panel over Mica).
    pub layer_background: D2D1_COLOR_F,
    pub card_background: D2D1_COLOR_F,
    pub card_stroke: D2D1_COLOR_F,
    /// `SystemFillColorNeutralBackgroundBrush`: a card's preview box
    /// (`CardsBrowserTemplate`).
    pub card_preview_background: D2D1_COLOR_F,
    /// `--color-surface-2` and `--color-surface-3`, named after the web's own
    /// steps because that is how its components pick them: a `ghost` button
    /// hovers to surface-2 and presses to surface-3, a `secondary` one hovers to
    /// surface-1 (`card_background`). Other tokens happen to carry the same
    /// values, but reading `card_preview_background` on a button would say
    /// nothing about why that colour is right.
    pub surface_2: D2D1_COLOR_F,
    pub surface_3: D2D1_COLOR_F,
    pub text_primary: D2D1_COLOR_F,
    pub text_secondary: D2D1_COLOR_F,
    /// `TextFillColorTertiary`: column headers, sort glyph, subtle labels.
    /// Paler than `text_secondary`.
    pub text_tertiary: D2D1_COLOR_F,
    /// `SystemFillColorCautionBrush`: the conflict dialog's warning (amber)
    /// text.
    pub caution: D2D1_COLOR_F,
    /// `--color-primary`.
    pub accent: D2D1_COLOR_F,
    /// `--color-primary-hover`.
    pub accent_hover: D2D1_COLOR_F,
    /// `--color-primary-light`: the filled pill behind an ACTIVE nav row, and
    /// the resting fill of `text` buttons on hover.
    pub accent_light: D2D1_COLOR_F,
    /// `--color-text-nav-active`: the label of an active nav row (it sits on
    /// `accent_light`, so it is a deep blue, not the accent itself).
    pub text_nav_active: D2D1_COLOR_F,
    /// What sits ON the accent (`--color-white` in the web).
    pub accent_foreground: D2D1_COLOR_F,
    /// A hyperlink that has already been followed. The accent covers the
    /// unvisited state; a visited one is the platform's purple, and it is a
    /// token rather than a constant so `LinkLabel` never hard-codes a colour.
    pub link_visited: D2D1_COLOR_F,
    /// `--color-danger`: destructive menu entries, delete buttons.
    pub danger: D2D1_COLOR_F,
    /// `--color-danger-light`: the fill behind a hovered destructive button.
    pub danger_light: D2D1_COLOR_F,
    /// `--color-success` / `--color-warning`, plus the tinted surface behind a
    /// warning callout (`--color-warning-light`).
    pub success: D2D1_COLOR_F,
    /// `--color-success-light`: the tinted pill behind a « success » badge, the
    /// counterpart of `warning_light` / `danger_light`. Added because the web's
    /// `@ui/Badge` needs all three tinted grounds and only this one was missing.
    pub success_light: D2D1_COLOR_F,
    pub warning: D2D1_COLOR_F,
    pub warning_light: D2D1_COLOR_F,
    /// Tooltip bubble (`TOOLTIP_STYLE` in `@ui/Tooltip`). Deliberately the
    /// SAME dark grey in both themes — the web ships one style, no dark
    /// variant, so a tooltip always reads as an overlay rather than a surface.
    pub tooltip_background: D2D1_COLOR_F,
    pub tooltip_foreground: D2D1_COLOR_F,
    /// The veil a MODAL dialog puts over what it covers — `bg-black/30` on
    /// `FloatingWindow`'s backdrop (`backdrop-blur-[1px]` has no desktop
    /// counterpart: `Canvas` publishes no blur). Like the tooltip pair it is
    /// the SAME in both palettes, because the web writes one value: a scrim
    /// darkens whatever is behind it, and a pale one over a dark page would
    /// stop separating the dialog from its host.
    pub dialog_scrim: D2D1_COLOR_F,
    /// Drive selection tints (`modules/drive.css`): a selected FILE card, a
    /// selected FOLDER card, and the hover of either.
    pub selected_card: D2D1_COLOR_F,
    pub selected_folder: D2D1_COLOR_F,
    pub row_hover: D2D1_COLOR_F,
    pub control_fill_hover: D2D1_COLOR_F,
    pub control_fill_pressed: D2D1_COLOR_F,
    /// `ListViewItemBackgroundSelected` (`SubtleFillColorSecondary`): the fill
    /// of a SELECTED row — must be at least as pronounced as hover (in dark
    /// mode, `control_fill_pressed` 0.04 < hover 0.06 would invert the
    /// reading).
    pub list_selected: D2D1_COLOR_F,
    pub tab_active_background: D2D1_COLOR_F,
    /// App.xaml overrides `TabViewItemHeaderBackgroundPointerOver` with
    /// `SubtleFillColorSecondary`. (The resting background is
    /// `SubtleFillColorTransparent` — nothing is painted.)
    pub tab_hover_background: D2D1_COLOR_F,
    /// WinUI `TabViewBorderBrush` = `CardStrokeColorDefault`: the tab strip's
    /// BottomBorderLine and the two 4×4 foot crescents. It is BLACK in both
    /// themes — a light stroke here reads as bright nubs at the tab's feet.
    pub tab_border: D2D1_COLOR_F,
    pub divider: D2D1_COLOR_F,
    /// `ScrollBarThumbFill` = `ControlStrongFillColorDefault`. Sampled pixel
    /// by pixel from the original app: #9C9EA4 on a #262B38 background, i.e.
    /// white at 54.5%.
    pub scrollbar_thumb: D2D1_COLOR_F,
    /// `--scrollbar-thumb-hover`. The thumb is OPAQUE in both themes, so a
    /// hover state has to change hue — brightening its alpha does nothing.
    pub scrollbar_thumb_hover: D2D1_COLOR_F,
    /// The expanded bar's fill (`ScrollBarBackground`).
    pub scrollbar_track: D2D1_COLOR_F,
    /// `--color-border-strong`: a control's outline on hover (checkbox…).
    pub border_strong: D2D1_COLOR_F,
    pub drive_bar_track: D2D1_COLOR_F,
    /// Opaque flyout/menu surface — the acrylic FALLBACK colour, used when the
    /// popup's DWM `DWMSBT_TRANSIENTWINDOW` backdrop is unavailable.
    pub flyout_background: D2D1_COLOR_F,
    /// `MenuFlyoutPresenterBorderBrush` = `SurfaceStrokeColorFlyoutBrush`
    /// (#0F000000 light / #33000000 dark) — darker than `card_stroke`, which
    /// is why the original's menus read as detached from the page.
    pub flyout_border: D2D1_COLOR_F,
    /// Toolbar card fill (CardBackgroundFillColorSecondary).
    pub toolbar_background: D2D1_COLOR_F,
    /// LayerOnMicaBaseAltFillColorDefault (theme-preview surfaces).
    pub layer_fill: D2D1_COLOR_F,
    /// `ThemedIconAltColor`: the color of a ThemedIcon's `@alt` layers when
    /// TINTED (menus). It's the INVERSE color of the foreground at 40%
    /// (dark = black #66161616, light = white #66F0F0F0). Renders a menu
    /// icon's cutouts/recesses as a translucent gray, instead of the solid
    /// foreground.
    pub icon_alt: D2D1_COLOR_F,

    // ── Colour picker chrome ────────────────────────────────────────────────
    //
    // The four below are the ONLY colours the colour picker family needs that
    // the palette did not already name. They are deliberately IDENTICAL in
    // both palettes, because the web writes them as literals rather than as
    // theme variables: a transparency chequer and a handle outline have to
    // stay readable over whatever arbitrary colour the user has picked, so
    // they do not follow the light/dark surface.

    /// The light square of the transparency chequer — `#fff` in
    /// `GradientPicker.tsx`'s `repeating-conic-gradient(#bbb 0% 25%, #fff 0%
    /// 50%)`.
    pub picker_chequer_a: D2D1_COLOR_F,
    /// The dark square of the same chequer — `#bbb`.
    pub picker_chequer_b: D2D1_COLOR_F,
    /// The white ring every picker handle wears — `border: '2px solid #fff'`
    /// on the SV handle, the hue handle and the gradient stop markers
    /// (`ColorPicker.tsx`, `GradientPicker.tsx`).
    pub picker_handle: D2D1_COLOR_F,
    /// The hairline under that ring — `boxShadow: '0 0 0 1px rgba(0,0,0,.5)'`.
    /// The web writes `.5` on the SV handle and `.6` on the slider thumb; the
    /// difference is 0.1 of alpha on a one-DIP outline and is not modelled.
    pub picker_handle_shadow: D2D1_COLOR_F,
}

const fn rgba(r: u8, g: u8, b: u8, a: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: r as f32 / 255.0,
        g: g as f32 / 255.0,
        b: b as f32 / 255.0,
        a,
    }
}

impl Theme {
    pub fn light() -> Self {
        Self {
            mode: ThemeMode::Light,
            // --body-bg #f8fafd.
            window_background: rgba(248, 250, 253, 1.0),
            titlebar_background: titlebar_tint(ThemeMode::Light, true),
            titlebar_background_inactive: titlebar_tint(ThemeMode::Light, false),
            // TabViewItemSeparator = DividerStrokeColorDefault: #0F000000.
            tab_separator: rgba(0, 0, 0, 0.0588),
            // --color-surface-0 #ffffff: the module panel the file area sits on.
            layer_background: rgba(255, 255, 255, 1.0),
            // --color-surface-1 #f8f9fa.
            card_background: rgba(248, 249, 250, 1.0),
            // --color-border #e0e0e0.
            card_stroke: rgba(224, 224, 224, 1.0),
            // --color-surface-2 #f1f3f4.
            card_preview_background: rgba(241, 243, 244, 1.0),
            surface_2: rgba(241, 243, 244, 1.0),
            surface_3: rgba(232, 234, 237, 1.0),
            // --color-text-primary #202124.
            text_primary: rgba(32, 33, 36, 1.0),
            // --color-text-secondary #5f6368.
            text_secondary: rgba(95, 99, 104, 1.0),
            // --color-text-tertiary #80868b.
            text_tertiary: rgba(128, 134, 139, 1.0),
            // --color-warning #f9ab00, darkened for text contrast on white.
            caution: rgba(157, 93, 0, 1.0),
            accent: rgba(26, 115, 232, 1.0),
            accent_hover: rgba(21, 87, 176, 1.0),
            accent_light: rgba(211, 227, 253, 1.0),
            text_nav_active: rgba(0, 29, 53, 1.0),
            accent_foreground: rgba(255, 255, 255, 1.0),
            link_visited: rgba(102, 51, 153, 1.0),
            danger: rgba(217, 48, 37, 1.0),
            danger_light: rgba(252, 232, 230, 1.0),
            success: rgba(30, 142, 62, 1.0),
            // --color-success-light #e6f4ea (theme.css).
            success_light: rgba(230, 244, 234, 1.0),
            warning: rgba(249, 171, 0, 1.0),
            // --color-warning-light #fef7e0.
            warning_light: rgba(254, 247, 224, 1.0),
            // rgba(60, 64, 67, 0.95) / #fff — identical in both themes.
            tooltip_background: rgba(60, 64, 67, 0.95),
            tooltip_foreground: rgba(255, 255, 255, 1.0),
            // `bg-black/30` — identical in both themes (see the field).
            dialog_scrim: rgba(0, 0, 0, 0.30),
            selected_card: rgba(221, 234, 252, 1.0),
            selected_folder: rgba(201, 222, 250, 1.0),
            row_hover: rgba(228, 236, 247, 1.0),
            // --kb-sidebar-hover #e8eaed (= --color-surface-3).
            control_fill_hover: rgba(232, 234, 237, 1.0),
            // --color-surface-2 pressed a touch further.
            control_fill_pressed: rgba(218, 220, 224, 1.0),
            // The drive row selection tint #e8f0fe.
            list_selected: rgba(232, 240, 254, 1.0),
            // TabViewItemHeaderBackgroundSelected = App.Theme.AddressBar
            // .BackgroundBrush = LayerOnMicaBaseAltFillColorDefault: the tab
            // and the toolbar strip share the SAME translucent layer, so
            // they merge seamlessly over the backdrop like the original.
            tab_active_background: rgba(255, 255, 255, 0.70),
            // SubtleFillColorSecondary: #09000000.
            tab_hover_background: rgba(0, 0, 0, 0.0353),
            // CardStrokeColorDefault (light): #0F000000.
            tab_border: rgba(0, 0, 0, 0.0588),
            // --color-border #e0e0e0.
            divider: rgba(224, 224, 224, 1.0),
            // --scrollbar-thumb #dadce0.
            scrollbar_thumb: rgba(218, 220, 224, 1.0),
            // --scrollbar-thumb-hover #bdc1c6.
            scrollbar_thumb_hover: rgba(189, 193, 198, 1.0),
            scrollbar_track: rgba(248, 250, 253, 0.90),
            // --color-border-strong #bdc1c6.
            border_strong: rgba(189, 193, 198, 1.0),
            drive_bar_track: rgba(0, 0, 0, 0.10),
            // --kb-float-surface rgb(246 248 252 / 56%): the frosted menu
            // surface. Drawn over the popup's real acrylic blur.
            flyout_background: rgba(246, 248, 252, 0.56),
            flyout_border: rgba(0, 0, 0, 0.0588),
            // Surfaces sitting on the page background are white like the web
            // module panel.
            toolbar_background: rgba(255, 255, 255, 1.0),
            // WinUI: #B3FFFFFF.
            layer_fill: rgba(255, 255, 255, 0.70),
            // ThemedIconAltColor (light): #66F0F0F0 — white at 40%.
            icon_alt: rgba(240, 240, 240, 0.4),
            // Picker chrome — literals in the web, identical in both palettes.
            picker_chequer_a: rgba(255, 255, 255, 1.0),
            picker_chequer_b: rgba(187, 187, 187, 1.0),
            picker_handle: rgba(255, 255, 255, 1.0),
            picker_handle_shadow: rgba(0, 0, 0, 0.5),
        }
    }

    pub fn dark() -> Self {
        Self {
            mode: ThemeMode::Dark,
            // kubuno-dark --body-bg #17181b.
            window_background: rgba(23, 24, 27, 1.0),
            titlebar_background: titlebar_tint(ThemeMode::Dark, true),
            titlebar_background_inactive: titlebar_tint(ThemeMode::Dark, false),
            // TabViewItemSeparator = DividerStrokeColorDefault: #15FFFFFF.
            tab_separator: rgba(255, 255, 255, 0.0824),
            // --color-surface-0 #202124.
            layer_background: rgba(32, 33, 36, 1.0),
            // --color-surface-1 #292a2d.
            card_background: rgba(41, 42, 45, 1.0),
            // --color-border #5f6368.
            card_stroke: rgba(95, 99, 104, 1.0),
            // --color-surface-2 #35363a.
            card_preview_background: rgba(53, 54, 58, 1.0),
            surface_2: rgba(53, 54, 58, 1.0),
            surface_3: rgba(68, 71, 70, 1.0),
            // --color-text-primary #e8eaed.
            text_primary: rgba(232, 234, 237, 1.0),
            // --color-text-secondary #9aa0a6.
            text_secondary: rgba(154, 160, 166, 1.0),
            // --color-text-tertiary #80868b.
            text_tertiary: rgba(128, 134, 139, 1.0),
            // --color-warning #fdd663.
            caution: rgba(253, 214, 99, 1.0),
            accent: rgba(138, 180, 248, 1.0),
            accent_hover: rgba(174, 203, 250, 1.0),
            accent_light: rgba(26, 58, 92, 1.0),
            text_nav_active: rgba(174, 203, 250, 1.0),
            // The dark accent is a PALE blue, so what sits on it must be dark.
            accent_foreground: rgba(32, 33, 36, 1.0),
            // Lightened for dark ground, like every other hue in this palette.
            link_visited: rgba(197, 160, 232, 1.0),
            danger: rgba(242, 139, 130, 1.0),
            danger_light: rgba(61, 28, 28, 1.0),
            success: rgba(129, 201, 149, 1.0),
            // kubuno-dark --color-success-light #1a3a27 (themes/kubuno-dark/theme.json).
            success_light: rgba(26, 58, 39, 1.0),
            warning: rgba(253, 214, 99, 1.0),
            // --color-warning-light #3d3218.
            warning_light: rgba(61, 50, 24, 1.0),
            // rgba(60, 64, 67, 0.95) / #fff — identical in both themes.
            tooltip_background: rgba(60, 64, 67, 0.95),
            tooltip_foreground: rgba(255, 255, 255, 1.0),
            // `bg-black/30` — identical in both themes (see the field).
            dialog_scrim: rgba(0, 0, 0, 0.30),
            selected_card: rgba(40, 65, 95, 1.0),
            selected_folder: rgba(40, 65, 95, 1.0),
            row_hover: rgba(43, 44, 48, 1.0),
            // --color-surface-2 #35363a.
            control_fill_hover: rgba(53, 54, 58, 1.0),
            // --color-surface-3 #444746.
            control_fill_pressed: rgba(68, 71, 70, 1.0),
            // The drive row selection tint #22344d.
            list_selected: rgba(34, 52, 77, 1.0),
            // Same translucent layer as the toolbar strip (see light theme).
            tab_active_background: rgba(58, 58, 58, 0.45),
            // SubtleFillColorSecondary: #0FFFFFFF.
            tab_hover_background: rgba(255, 255, 255, 0.0588),
            // CardStrokeColorDefault (dark): #19000000 — black, not white.
            tab_border: rgba(0, 0, 0, 0.098),
            // --color-border #5f6368, softened to a rim.
            divider: rgba(95, 99, 104, 0.55),
            // --scrollbar-thumb #5f6368.
            scrollbar_thumb: rgba(95, 99, 104, 1.0),
            // --scrollbar-thumb-hover #80868b.
            scrollbar_thumb_hover: rgba(128, 134, 139, 1.0),
            scrollbar_track: rgba(23, 24, 27, 0.90),
            // --color-border-strong #80868b.
            border_strong: rgba(128, 134, 139, 1.0),
            drive_bar_track: rgba(255, 255, 255, 0.15),
            // --kb-float-surface-dark rgb(40 40 44 / 58%).
            flyout_background: rgba(40, 40, 44, 0.58),
            flyout_border: rgba(0, 0, 0, 0.2),
            // Same surface as the module panel.
            toolbar_background: rgba(32, 33, 36, 1.0),
            // WinUI: #733A3A3A.
            layer_fill: rgba(58, 58, 58, 0.45),
            // ThemedIconAltColor (dark): #66161616 — black at 40%.
            icon_alt: rgba(22, 22, 22, 0.4),
            // Picker chrome — the SAME values as the light palette on purpose:
            // the web writes them as literals, not as theme variables.
            picker_chequer_a: rgba(255, 255, 255, 1.0),
            picker_chequer_b: rgba(187, 187, 187, 1.0),
            picker_handle: rgba(255, 255, 255, 1.0),
            picker_handle_shadow: rgba(0, 0, 0, 0.5),
        }
    }

    /// Detects the current system app theme.
    pub fn detect() -> Self {
        if system_uses_light_theme() {
            Self::light()
        } else {
            Self::dark()
        }
    }

}

fn read_dwm_dword(value_name: windows::core::PCWSTR) -> Option<u32> {
    use windows::core::w;
    use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};

    let mut value: u32 = 0;
    let mut size = std::mem::size_of::<u32>() as u32;
    unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!(r"Software\Microsoft\Windows\DWM"),
            value_name,
            RRF_RT_REG_DWORD,
            None,
            Some(&mut value as *mut _ as *mut _),
            Some(&mut size),
        )
    }
    .is_ok()
    .then_some(value)
}

/// Title-bar tint matching the DWM behavior: when "accent color on title
/// bars" is enabled, blend the accent into the theme background.
fn titlebar_tint(mode: ThemeMode, active: bool) -> D2D1_COLOR_F {
    use windows::core::w;

    let transparent = D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 0.0 };
    // TabBar.xaml: TitlebarArea Background="Transparent" — the Mica
    // backdrop shows through the title band; accent tint only applies in
    // light mode when "accent color on title bars" is enabled. When the
    // window is INACTIVE, the Mica backdrop desaturates on its own (DWM) and
    // Windows no longer applies the accent — so we fall back to transparent,
    // or to `AccentColorInactive` if title-bar accent color is enabled.
    if mode == ThemeMode::Dark {
        return transparent;
    }
    let prevalence = read_dwm_dword(w!("ColorPrevalence")).unwrap_or(0) != 0;
    if !prevalence {
        // Transparent: the Mica backdrop shows as-is (and desaturates on its
        // own when inactive).
        return transparent;
    }
    // AccentColor / AccentColorInactive are stored as ABGR.
    let key = if active { w!("AccentColor") } else { w!("AccentColorInactive") };
    let default = if active { 0x00D47800 } else { 0x009E9E9E };
    let abgr = read_dwm_dword(key).unwrap_or(default);
    let (r, g, b) = (
        (abgr & 0xFF) as f32 / 255.0,
        ((abgr >> 8) & 0xFF) as f32 / 255.0,
        ((abgr >> 16) & 0xFF) as f32 / 255.0,
    );
    // TRANSLUCENT accent (not pre-blended opaque): the caption buttons
    // ─ □ ✕ are composited by DWM BEHIND the client (sheet of glass); an
    // opaque band would cover them in light theme. 35%: verified on screen
    // against the compiled original, at the same window position (the Mica
    // backdrop depends on position — comparing elsewhere would skew the
    // measurement), comparison from 2026-07-23.
    D2D1_COLOR_F { r, g, b, a: 0.35 }
}

/// Reads `AppsUseLightTheme` from the registry (1 = light).
pub fn system_uses_light_theme() -> bool {
    use windows::core::w;
    use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};

    let mut value: u32 = 1;
    let mut size = std::mem::size_of::<u32>() as u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize"),
            w!("AppsUseLightTheme"),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut value as *mut _ as *mut _),
            Some(&mut size),
        )
    };
    status.is_ok() && value != 0
}
