//! Port of `Files.App/Views/Settings/AppearancePage.xaml` and its
//! `AppearanceViewModel`: theme / backdrop combos, the 20 background-color
//! swatches (AppThemeResourceFactory), background image, font, and the
//! ShowTabActions / AddressBar / Toolbar / StatusBar cards.

use drive_app_controls::themes::shape::{height, radius};
use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;

use crate::ui::{Hot, Layout, Painter, Rect, UiState};
use crate::views::settings::controls as sc;

/// `AppThemeResourceFactory.AppThemeResources`, verbatim: (name key, color).
pub const APP_THEME_RESOURCES: [(&str, &str); 20] = [
    ("Default", "#00000000"),
    ("YellowGold", "#32FFB900"),
    ("OrangeBright", "#32F7630C"),
    ("BrickRed", "#32D13438"),
    ("ModRed", "#32FF4343"),
    ("Red", "#32EA005E"),
    ("RoseBright", "#32EA005E"),
    ("Blue", "#320078D7"),
    ("IrisPastel", "#328764B8"),
    ("VioletRedLight", "#32B146C2"),
    ("CoolBlueBright", "#320099BC"),
    ("Seafoam", "#3200B7C3"),
    ("MintLight", "#3200B294"),
    ("Gray", "#327A7574"),
    ("Green", "#32107C10"),
    ("Overcast", "#32767676"),
    ("Storm", "#324C4A48"),
    ("BlueGray", "#3269797E"),
    ("GrayDark", "#324A5459"),
    ("Camouflage", "#327E735F"),
];

/// Expander indices in `UiState::appearance_expanded`.
pub const EXP_BG_COLOR: usize = 0;
pub const EXP_BG_IMAGE: usize = 1;
pub const EXP_ADDRESS_BAR: usize = 2;
pub const EXP_TOOLBAR: usize = 3;

/// One interactive row of the page, in XAML order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApRow {
    Theme,
    Backdrop,
    BgColorHeader,
    BgImageHeader,
    ImageOpacity,
    ImageFit,
    ImageVAlign,
    ImageHAlign,
    Font,
    ShowTabActions,
    AddressBarHeader,
    StatusCenter,
    ToolbarHeader,
    CustomizeToolbar,
    ShowStatusBar,
}

pub struct AppearanceLayout {
    pub rows: Vec<(Rect, ApRow)>,
    /// The 20 color swatches (only when the BackgroundColor expander is open).
    pub swatches: Vec<Rect>,
    pub swatch_panel: Option<Rect>,
    pub extent: f32,
}

/// WinUI SettingsCard MinHeight (calibrated on the running original app).
const ROW_H: f32 = 64.0;
const ROW_GAP: f32 = 4.0; // StackPanel Spacing="4"
/// Swatch template: Width=120, rows 16 + 50 + label.
const SWATCH_W: f32 = 120.0;
const SWATCH_H: f32 = 94.0;
const SWATCH_MARGIN: f32 = 4.0;

pub fn compute(content: &Rect, scroll: f32, expanded: [bool; 4]) -> AppearanceLayout {
    let page_left = content.left + 300.0;
    let right = content.right - 24.0;
    let mut rows: Vec<(Rect, ApRow)> = Vec::new();
    let mut swatches = Vec::new();
    let mut swatch_panel = None;
    let mut y = content.top + 56.0 - scroll;

    let push = |rows: &mut Vec<(Rect, ApRow)>, y: &mut f32, kind: ApRow| {
        rows.push((Rect::new(page_left, *y, right, *y + ROW_H), kind));
        *y += ROW_H + ROW_GAP;
    };

    push(&mut rows, &mut y, ApRow::Theme);
    push(&mut rows, &mut y, ApRow::Backdrop);
    push(&mut rows, &mut y, ApRow::BgColorHeader);
    if expanded[EXP_BG_COLOR] {
        // GridView Padding="8", items 120 wide, attached under the header.
        let panel_top = y - ROW_GAP;
        let avail = right - page_left - 16.0;
        let per_row = ((avail + SWATCH_MARGIN) / (SWATCH_W + SWATCH_MARGIN * 2.0))
            .floor()
            .max(1.0) as usize;
        let mut sx = page_left + 8.0 + SWATCH_MARGIN;
        let mut sy = panel_top + 8.0 + SWATCH_MARGIN;
        for i in 0..APP_THEME_RESOURCES.len() {
            if i > 0 && i % per_row == 0 {
                sy += SWATCH_H + SWATCH_MARGIN * 2.0;
                sx = page_left + 8.0 + SWATCH_MARGIN;
            }
            swatches.push(Rect::new(sx, sy, sx + SWATCH_W, sy + SWATCH_H));
            sx += SWATCH_W + SWATCH_MARGIN * 2.0;
        }
        let bottom = swatches.last().map_or(sy, |r| r.bottom) + 8.0 + SWATCH_MARGIN;
        swatch_panel = Some(Rect::new(page_left, panel_top, right, bottom));
        y = bottom + ROW_GAP;
    }
    push(&mut rows, &mut y, ApRow::BgImageHeader);
    if expanded[EXP_BG_IMAGE] {
        push(&mut rows, &mut y, ApRow::ImageOpacity);
        push(&mut rows, &mut y, ApRow::ImageFit);
        push(&mut rows, &mut y, ApRow::ImageVAlign);
        push(&mut rows, &mut y, ApRow::ImageHAlign);
    }
    push(&mut rows, &mut y, ApRow::Font);
    push(&mut rows, &mut y, ApRow::ShowTabActions);
    push(&mut rows, &mut y, ApRow::AddressBarHeader);
    if expanded[EXP_ADDRESS_BAR] {
        push(&mut rows, &mut y, ApRow::StatusCenter);
    }
    push(&mut rows, &mut y, ApRow::ToolbarHeader);
    if expanded[EXP_TOOLBAR] {
        push(&mut rows, &mut y, ApRow::CustomizeToolbar);
    }
    push(&mut rows, &mut y, ApRow::ShowStatusBar);

    AppearanceLayout {
        rows,
        swatches,
        swatch_panel,
        extent: (y + scroll) - content.top + 16.0,
    }
}

/// True if the row is an item INSIDE an expander (drawn inside a group card).
fn is_expander_item(kind: ApRow) -> bool {
    matches!(
        kind,
        ApRow::ImageOpacity
            | ApRow::ImageFit
            | ApRow::ImageVAlign
            | ApRow::ImageHAlign
            | ApRow::StatusCenter
            | ApRow::CustomizeToolbar
    )
}

/// The "Parcourir ▾" split button inside the BackgroundImage header.
/// `@ui/Button size="sm"`: h-8.
pub fn browse_button_rect(header: &Rect) -> Rect {
    let cy = (header.top + header.bottom) / 2.0;
    let h = height::BUTTON_SM;
    Rect::new(header.right - 176.0, cy - h / 2.0, header.right - 44.0, cy + h / 2.0)
}

/// The slider track of the Opacity row (Width=140, right-aligned).
pub fn opacity_slider_rect(row: &Rect) -> Rect {
    Rect::new(row.right - 156.0, row.top, row.right - 16.0, row.bottom)
}

/// The ToggleSwitch zone in a header row that has both a toggle and a chevron.
pub fn header_toggle_rect(header: &Rect) -> Rect {
    Rect::new(header.right - 100.0, header.top, header.right - 44.0, header.bottom)
}

impl Painter<'_> {
    pub(crate) fn draw_settings_appearance(&self, layout: &Layout, state: &UiState) {
        let Some(ap) = &layout.appearance else { return };
        let t = self.theme;
        let f = &self.renderer.formats;
        let s = crate::services::settings::get();
        let tr = drive_localization::tr;

        // Group cards: an expander's header + items share one card.
        let mut i = 0usize;
        while i < ap.rows.len() {
            let (rect, kind) = ap.rows[i];
            // Find the span of this visual group.
            let mut bottom = rect.bottom;
            let mut last = i;
            if kind == ApRow::BgColorHeader {
                if let Some(panel) = &ap.swatch_panel {
                    bottom = panel.bottom;
                }
            } else {
                while last + 1 < ap.rows.len() && is_expander_item(ap.rows[last + 1].1) {
                    last += 1;
                    bottom = ap.rows[last].0.bottom;
                }
            }
            // `@ui/Card`: `rounded-xl border border-border bg-surface-0`.
            let group = Rect::new(rect.left, rect.top, rect.right, bottom);
            self.fill_rounded(&group, radius::XL, &t.layer_background);
            self.stroke_rounded(&group, radius::XL, &t.card_stroke);
            // Separator under the header and between the expander items.
            for j in (i + 1)..=last {
                let r = ap.rows[j].0;
                self.hairline(r.left, r.top - 2.0, r.right);
            }
            if kind == ApRow::BgColorHeader && bottom > rect.bottom {
                self.hairline(rect.left, rect.bottom + 2.0, rect.right);
            }
            i = last + 1;
        }

        // Row contents.
        for (idx, (rect, kind)) in ap.rows.iter().enumerate() {
            let hot = state.hot == Some(Hot::SettingRow(idx));
            if hot {
                // A row inside a card rounds like a small control; a
                // standalone one is the card itself.
                let r = if is_expander_item(*kind) { radius::SM } else { radius::XL };
                self.fill_rounded(rect, r, &t.control_fill_hover);
            }
            let label_left = rect.left + 48.0;
            let icon_rect = Rect::new(rect.left + 12.0, rect.top, rect.left + 36.0, rect.bottom);
            let label_rect = Rect::new(label_left, rect.top, rect.right - 220.0, rect.bottom);
            match kind {
                ApRow::Theme => {
                    self.text("\u{E790}", &icon_rect, &f.icon, &t.text_secondary, true);
                    let combo = self.combo(rect, s.theme.label());
                    let label = Rect::new(label_left, rect.top, combo.left - 12.0, rect.bottom);
                    self.text(tr("SettingsAppearanceTheme"), &label, &f.body, &t.text_primary, false);
                }
                ApRow::Backdrop => {
                    self.text("\u{EF1F}", &icon_rect, &f.icon, &t.text_secondary, true);
                    let combo = self.combo(rect, s.backdrop.label());
                    let label = Rect::new(label_left, rect.top, combo.left - 12.0, rect.bottom);
                    self.text(tr("BackdropMaterial"), &label, &f.body, &t.text_primary, false);
                }
                ApRow::BgColorHeader => {
                    self.text("\u{E771}", &icon_rect, &f.icon, &t.text_secondary, true);
                    self.text(tr("BackgroundColor"), &label_rect, &f.body, &t.text_primary, false);
                    // Current-color button (checkerboard + color + caret):
                    // an `@ui/Dropdown` trigger whose "label" is the swatch.
                    let cy = (rect.top + rect.bottom) / 2.0;
                    let h = height::BUTTON_SM;
                    let btn = Rect::new(rect.right - 108.0, cy - h / 2.0, rect.right - 44.0, cy + h / 2.0);
                    self.stroke_rounded(&btn, radius::SM, &t.card_stroke);
                    let sw = Rect::new(btn.left + 4.0, btn.top + 4.0, btn.left + 28.0, btn.bottom - 4.0);
                    self.checkerboard(&sw);
                    if let Some(c) = crate::services::settings::parse_color(&s.app_theme_background_color) {
                        self.fill_rounded(&sw, radius::SM, &c);
                    }
                    let chev = Rect::new(btn.left + 32.0, btn.top, btn.right, btn.bottom);
                    self.text("\u{E70D}", &chev, &f.icon_small, &t.text_secondary, true);
                    self.expander_chevron(rect, state.appearance_expanded[EXP_BG_COLOR]);
                }
                ApRow::BgImageHeader => {
                    self.text("\u{E91B}", &icon_rect, &f.icon, &t.text_secondary, true);
                    if s.app_theme_background_image_source.is_empty() {
                        self.text(tr("BackgroundImage"), &label_rect, &f.body, &t.text_primary, false);
                    } else {
                        // Header + Description (the image path), like the XAML.
                        let top = Rect::new(label_left, rect.top + 10.0, label_rect.right, rect.top + 34.0);
                        let sub = Rect::new(label_left, rect.top + 32.0, label_rect.right, rect.bottom - 8.0);
                        self.text(tr("BackgroundImage"), &top, &f.body, &t.text_primary, false);
                        self.text(&s.app_theme_background_image_source, &sub, &f.caption, &t.text_secondary, false);
                    }
                    // SplitButton "Parcourir | ▾": one `@ui/Button
                    // variant="secondary"` box split by a hairline.
                    // `@ui/Button variant="secondary"`: `bg-white border
                    // border-border text-text-primary`, `rounded-md`, never bold.
                    let btn = browse_button_rect(rect);
                    self.fill_rounded(&btn, radius::SM, &t.layer_background);
                    self.stroke_rounded(&btn, radius::SM, &t.card_stroke);
                    let text_zone = Rect::new(btn.left, btn.top, btn.right - 28.0, btn.bottom);
                    self.text_centered(tr("Browse"), &text_zone, &f.body, &t.text_primary);
                    self.vline(btn.right - 28.0, btn.top + 5.0, btn.bottom - 5.0);
                    let chev = Rect::new(btn.right - 28.0, btn.top, btn.right, btn.bottom);
                    self.text("\u{E70D}", &chev, &f.icon_small, &t.text_secondary, true);
                    self.expander_chevron(rect, state.appearance_expanded[EXP_BG_IMAGE]);
                }
                ApRow::ImageOpacity => {
                    self.text(tr("Opacity"), &label_rect, &f.body, &t.text_primary, false);
                    let track = opacity_slider_rect(rect);
                    let cy = (rect.top + rect.bottom) / 2.0;
                    let base = Rect::new(track.left, cy - 2.0, track.right, cy + 2.0);
                    self.fill_rounded(&base, 2.0, &t.control_fill_pressed);
                    let v = ((s.app_theme_background_image_opacity - 0.1) / 0.9).clamp(0.0, 1.0);
                    let fill_w = (track.right - track.left - 16.0) * v;
                    let filled = Rect::new(track.left, cy - 2.0, track.left + fill_w + 8.0, cy + 2.0);
                    self.fill_rounded(&filled, 2.0, &t.accent);
                    let knob = Rect::new(track.left + fill_w, cy - 8.0, track.left + fill_w + 16.0, cy + 8.0);
                    self.fill_rounded(&knob, 8.0, &t.card_background);
                    self.stroke_rounded(&knob, 8.0, &t.card_stroke);
                    let dot = knob.inflate(-4.0, -4.0);
                    self.fill_rounded(&dot, 4.0, &t.accent);
                }
                ApRow::ImageFit => {
                    let combo = self.combo(rect, tr(s.app_theme_background_image_fit.tr_key()));
                    let label = Rect::new(label_left, rect.top, combo.left - 12.0, rect.bottom);
                    self.text(tr("ImageFit"), &label, &f.body, &t.text_primary, false);
                }
                ApRow::ImageVAlign => {
                    let combo = self.combo(rect, tr(s.app_theme_background_image_vertical_alignment.tr_key()));
                    let label = Rect::new(label_left, rect.top, combo.left - 12.0, rect.bottom);
                    self.text(tr("VerticalAlignment"), &label, &f.body, &t.text_primary, false);
                }
                ApRow::ImageHAlign => {
                    let combo = self.combo(rect, tr(s.app_theme_background_image_horizontal_alignment.tr_key()));
                    let label = Rect::new(label_left, rect.top, combo.left - 12.0, rect.bottom);
                    self.text(tr("HorizontalAlignment"), &label, &f.body, &t.text_primary, false);
                }
                ApRow::Font => {
                    self.text("\u{E8D2}", &icon_rect, &f.icon, &t.text_secondary, true);
                    let value = if s.app_theme_font_family == crate::services::settings::STANDARD_FONT {
                        tr("Default").to_string()
                    } else {
                        s.app_theme_font_family.clone()
                    };
                    let combo = self.combo(rect, &value);
                    let label = Rect::new(label_left, rect.top, combo.left - 12.0, rect.bottom);
                    self.text(tr("Font"), &label, &f.body, &t.text_primary, false);
                }
                ApRow::ShowTabActions => {
                    self.vector_icon("TabActions", &icon_rect, 16.0, &t.text_secondary);
                    self.text(tr("ShowTabActions"), &label_rect, &f.body, &t.text_primary, false);
                    self.toggle_switch(rect, s.show_tab_actions);
                }
                ApRow::AddressBarHeader => {
                    self.vector_icon("WebAsset", &icon_rect, 16.0, &t.text_secondary);
                    self.text(tr("AddressBar"), &label_rect, &f.body, &t.text_primary, false);
                    self.expander_chevron(rect, state.appearance_expanded[EXP_ADDRESS_BAR]);
                }
                ApRow::StatusCenter => {
                    let combo = self.combo(rect, tr(s.status_center_visibility.tr_key()));
                    let label = Rect::new(label_left, rect.top, combo.left - 12.0, rect.bottom);
                    self.text(tr("ShowStatusCenterButton"), &label, &f.body, &t.text_primary, false);
                }
                ApRow::ToolbarHeader => {
                    self.vector_icon("Toolbar", &icon_rect, 16.0, &t.text_secondary);
                    self.text(tr("Toolbar"), &label_rect, &f.body, &t.text_primary, false);
                    let toggle = header_toggle_rect(rect);
                    self.toggle_switch_at(&toggle, s.show_toolbar);
                    self.expander_chevron(rect, state.appearance_expanded[EXP_TOOLBAR]);
                }
                ApRow::CustomizeToolbar => {
                    self.text(tr("CustomizeToolbarDescription"), &label_rect, &f.body, &t.text_primary, false);
                    let action = Rect::new(rect.right - 40.0, rect.top, rect.right - 12.0, rect.bottom);
                    self.text("\u{E8A7}", &action, &f.icon_small, &t.text_secondary, true);
                }
                ApRow::ShowStatusBar => {
                    self.vector_icon("StatusBar", &icon_rect, 16.0, &t.text_secondary);
                    self.text(tr("ShowStatusBar"), &label_rect, &f.body, &t.text_primary, false);
                    self.toggle_switch(rect, s.show_status_bar);
                }
            }
        }

        // The 20 background-color swatches (AppThemeResourcesItemTemplate).
        if let Some(_panel) = &ap.swatch_panel {
            for (i, item) in ap.swatches.iter().enumerate() {
                let (name_key, color_hex) = APP_THEME_RESOURCES[i];
                let selected = s.app_theme_background_color == color_hex
                    && APP_THEME_RESOURCES[..i].iter().all(|(_, c)| *c != color_hex);
                let hovered = state.hot == Some(Hot::ThemeSwatch(i));

                // Background color band (rows 0-1, 66 high). The tile is a
                // selectable `@ui/Card`, so the band takes the card radius on
                // its top corners only.
                let band = Rect::new(item.left, item.top, item.right, item.top + 66.0);
                if let Some(c) = crate::services::settings::parse_color(color_hex) {
                    self.fill_top_rounded(&band, radius::XL, &c);
                }
                // Mini tab bar (row 0): 32×12 tab + hairlines.
                let layer = t.layer_fill;
                let tab = Rect::new(item.left + 4.0, item.top + 4.0, item.left + 36.0, item.top + 16.0);
                self.fill_rounded(&tab, radius::SM, &layer);
                self.stroke_rounded(&tab, radius::SM, &t.card_stroke);
                self.hairline(item.left, item.top + 16.0, item.right);
                // File area (row 1): Layer fill over the color.
                let area = Rect::new(item.left, item.top + 16.0, item.right, item.top + 66.0);
                self.fill_rounded(&area, 0.0, &layer);
                self.hairline(item.left, item.top + 66.0, item.right);
                // Name (Padding=4, centered).
                let label = Rect::new(item.left + 4.0, item.top + 66.0, item.right - 4.0, item.bottom);
                self.text_centered(drive_localization::tr(name_key), &label, &f.caption, &t.text_primary);

                // Item border, at the `@ui/Card` radius. Selection = the web
                // swatch's `boxShadow: 0 0 0 2px var(--color-primary)`
                // (`@ui/ColorSwatchPicker`), ONE accent ring, not two
                // concentric lines. Hover raises the border like a checkbox
                // does (`checkboxCanvas`: `--color-border-strong`), for which
                // the desktop `Theme` has no token yet — `text_tertiary`
                // (#80868b) is the closest it carries.
                if selected {
                    self.stroke_rounded_w(item, radius::XL, &t.accent, 2.0);
                } else if hovered {
                    self.stroke_rounded(item, radius::XL, &t.text_tertiary);
                } else {
                    self.stroke_rounded(item, radius::XL, &t.card_stroke);
                }
            }
        }
    }

    /// ComboBox at the right edge of a settings row. Geometry and paint are
    /// the generic page's — ONE `@ui/Dropdown` for the whole settings section
    /// (this page used to carry its own fixed-200-wide copy). Returns the rect
    /// so the caller can clip its label before it.
    fn combo(&self, row: &Rect, value: &str) -> Rect {
        let combo = sc::combo_rect(self, row, false, value);
        self.draw_combo(&combo, value, false);
        combo
    }

    /// ToggleSwitch at the right edge of a settings row — the `kubuno-ui`
    /// `Switch`, through the settings pages' one switch painter.
    pub(crate) fn toggle_switch(&self, row: &Rect, on: bool) {
        let cy = (row.top + row.bottom) / 2.0;
        let track = self.settings_switch(row.right - 58.0, cy, on, true);
        self.toggle_state_text(&track, row, on, false);
    }

    fn toggle_switch_at(&self, zone: &Rect, on: bool) {
        let cy = (zone.top + zone.bottom) / 2.0;
        let track = self.settings_switch(zone.left, cy, on, true);
        self.toggle_state_text(&track, zone, on, false);
    }

    /// Expander chevron (up when open) at the very right of a header row —
    /// the generic pages' painter, so both settings sections show the same one.
    fn expander_chevron(&self, row: &Rect, expanded: bool) {
        self.settings_expander_chevron(row, expanded);
    }

    /// 6px two-tone checkerboard under the transparent-color preview.
    /// `@ui/GradientPicker`: `repeating-conic-gradient(#bbb 0 25%, #fff 0 50%)`.
    pub(crate) fn checkerboard(&self, rect: &Rect) {
        let light = D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 1.0 };
        let dark = D2D1_COLOR_F { r: 0.733, g: 0.733, b: 0.733, a: 1.0 };
        self.fill_rounded(rect, radius::SM, &light);
        let cell = 6.0;
        let mut row = 0;
        let mut y = rect.top;
        while y < rect.bottom {
            let mut col = 0;
            let mut x = rect.left;
            while x < rect.right {
                if (row + col) % 2 == 1 {
                    let r = Rect::new(x, y, (x + cell).min(rect.right), (y + cell).min(rect.bottom));
                    self.fill_rounded(&r, 0.0, &dark);
                }
                x += cell;
                col += 1;
            }
            y += cell;
            row += 1;
        }
    }

    fn text_centered(&self, s: &str, rect: &Rect, format: &windows::Win32::Graphics::DirectWrite::IDWriteTextFormat, color: &D2D1_COLOR_F) {
        self.text(s, rect, format, color, true);
    }

    /// 1-DIP horizontal separator (DividerStrokeColorDefault ≈ card stroke).
    pub(crate) fn hairline(&self, left: f32, y: f32, right: f32) {
        let r = Rect::new(left, y, right, y + 1.0);
        self.fill_rounded(&r, 0.0, &self.theme.card_stroke);
    }

    /// 1-DIP vertical separator (SplitButton / omnibar mode divider).
    pub(crate) fn vline(&self, x: f32, top: f32, bottom: f32) {
        let r = Rect::new(x, top, x + 1.0, bottom);
        self.fill_rounded(&r, 0.0, &self.theme.card_stroke);
    }
}
