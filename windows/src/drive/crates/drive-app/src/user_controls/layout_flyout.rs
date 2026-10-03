//! Port of the "Disposition" `Flyout` from `Toolbar.xaml` (`LayoutOptionsButton`).
//!
//! This is NOT a menu: it's a real panel (`<Flyout>`, not
//! `<MenuFlyout>`), with a `StackPanel Spacing="12"` that stacks
//!
//! 1. the "Disposition" title;
//! 2. five 76×72 `RadioButton`s (`Local.RadioToggleButtonStyle`), 28px
//!    ThemedIcon above the 12 pt label;
//! 3. a full-width rule (`Margin="-20,0"`, hence the panel's 20 margin);
//! 4. the "Taille" title, a stepped `Slider` and the row of size icons,
//!    of which only some ticks are marked;
//! 5. a second rule;
//! 6. the "Éléments masqués" and "Extensions de fichiers" `ToggleSwitch`es.
//!
//! Each layout has its own slider (`Maximum` 5, 4 or 12) and its own
//! saved size — see `ViewMode::size_max`.

use crate::view_models::shell_view_model::ViewMode;
use crate::ui::{Painter, Rect};

/// `Flyout` padding: the rule overflows it by `-20,0`.
const PAD: f32 = 20.0;
/// `StackPanel Spacing="12"`.
const SPACING: f32 = 12.0;
/// `Local.RadioToggleButtonStyle`: Width 76, Height 72.
const CARD_W: f32 = 76.0;
const CARD_H: f32 = 72.0;
/// `StackPanel Orientation="Horizontal" Spacing="8"`.
const CARD_GAP: f32 = 8.0;
const HEADER_H: f32 = 20.0;
/// The WinUI Slider and its row of ticks.
const SLIDER_H: f32 = 32.0;
/// The row of 28px icons under the slider (`Spacing="4"` separates it from the Slider).
const SIZE_ICONS_H: f32 = 28.0;
const SIZE_GAP: f32 = 4.0;
/// A "label + ToggleSwitch" row (`Grid RowSpacing="4"`).
const TOGGLE_H: f32 = 28.0;
const TOGGLE_GAP: f32 = 4.0;
/// `ToggleSwitch MinWidth="68"`: the track, then its On/Off text.
const SWITCH_W: f32 = 40.0;
const SWITCH_H: f32 = 20.0;

/// Panel width: the five cards plus the margins.
pub const LAYOUT_PANEL_WIDTH: f32 = 5.0 * CARD_W + 4.0 * CARD_GAP + 2.0 * PAD;

/// What the panel displays, copied from the state on open.
#[derive(Clone, Copy, PartialEq)]
pub struct LayoutPanel {
    pub mode: ViewMode,
    /// Size of the CURRENT layout (each one has its own).
    pub size: u8,
    pub show_hidden: bool,
    pub show_extensions: bool,
    pub hot: Option<LayoutHit>,
    /// True while dragging the slider: the drag follows the mouse.
    pub dragging: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LayoutHit {
    /// One of the five cards (index in `ViewMode::ALL`).
    Card(usize),
    /// The size slider's rail.
    Slider,
    ToggleHidden,
    ToggleExtensions,
    Panel,
}

impl LayoutPanel {
    /// Total height: title, cards, rule, size (title + slider + icons),
    /// rule, two switches.
    pub fn height(&self) -> f32 {
        PAD
            + HEADER_H
            + SPACING
            + CARD_H
            + SPACING
            + 1.0
            + SPACING
            + HEADER_H
            + SPACING
            + SLIDER_H
            + SIZE_GAP
            + SIZE_ICONS_H
            + SPACING
            + 1.0
            + SPACING
            + TOGGLE_H
            + TOGGLE_GAP
            + TOGGLE_H
            + PAD
    }

    fn cards_top(&self, y: f32) -> f32 {
        y + PAD + HEADER_H + SPACING
    }

    pub fn card_rect(&self, x: f32, y: f32, i: usize) -> Rect {
        let left = x + PAD + (CARD_W + CARD_GAP) * i as f32;
        let top = self.cards_top(y);
        Rect::new(left, top, left + CARD_W, top + CARD_H)
    }

    fn slider_top(&self, y: f32) -> f32 {
        self.cards_top(y) + CARD_H + SPACING + 1.0 + SPACING + HEADER_H + SPACING
    }

    /// The rail: `Slider Padding="4,0,4,0"` within the content width.
    pub fn slider_rect(&self, x: f32, y: f32) -> Rect {
        let top = self.slider_top(y);
        Rect::new(x + PAD + 4.0, top, x + LAYOUT_PANEL_WIDTH - PAD - 4.0, top + SLIDER_H)
    }

    fn toggles_top(&self, y: f32) -> f32 {
        self.slider_top(y) + SLIDER_H + SIZE_GAP + SIZE_ICONS_H + SPACING + 1.0 + SPACING
    }

    pub fn toggle_rect(&self, x: f32, y: f32, second: bool) -> Rect {
        let top = self.toggles_top(y) + if second { TOGGLE_H + TOGGLE_GAP } else { 0.0 };
        Rect::new(x + PAD, top, x + LAYOUT_PANEL_WIDTH - PAD, top + TOGGLE_H)
    }

    /// The thumb's position, from 0 (first tick) to 1 (last).
    fn slider_fraction(&self) -> f32 {
        let max = self.mode.size_max().max(2) as f32;
        (self.size.clamp(1, self.mode.size_max()) as f32 - 1.0) / (max - 1.0)
    }

    /// The size designated by the abscissa `px` on the rail (stepped: `SnapsTo="Ticks"`).
    pub fn size_at(&self, x: f32, y: f32, px: f32) -> u8 {
        let rail = self.slider_rect(x, y);
        let t = ((px - rail.left) / (rail.right - rail.left).max(1.0)).clamp(0.0, 1.0);
        let max = self.mode.size_max();
        (1.0 + t * (max - 1) as f32).round().clamp(1.0, max as f32) as u8
    }

    pub fn hit(&self, x: f32, y: f32, px: f32, py: f32) -> LayoutHit {
        for i in 0..ViewMode::ALL.len() {
            if self.card_rect(x, y, i).contains(px, py) {
                return LayoutHit::Card(i);
            }
        }
        // The rail is thin: we widen its hit zone to the whole band, as
        // the WinUI Slider does (whose Thumb overflows the rail).
        if self.slider_rect(x, y).inflate(0.0, 6.0).contains(px, py) {
            return LayoutHit::Slider;
        }
        if self.toggle_rect(x, y, false).contains(px, py) {
            return LayoutHit::ToggleHidden;
        }
        if self.toggle_rect(x, y, true).contains(px, py) {
            return LayoutHit::ToggleExtensions;
        }
        LayoutHit::Panel
    }
}

impl Painter<'_> {
    /// Draws the panel from the (0,0) corner of its popup.
    pub(crate) fn draw_layout_panel(&self, panel: &LayoutPanel, dpi: f32) {
        self.set_scale(dpi / 96.0);
        let tr = drive_localization::tr;
        let t = self.theme;
        let f = &self.renderer.formats;
        let (x, y) = (0.0, 0.0);
        let width = LAYOUT_PANEL_WIDTH;

        // 1. Title.
        let header = Rect::new(x + PAD, y + PAD, x + width - PAD, y + PAD + HEADER_H);
        self.text(tr("Layout"), &header, &f.body_strong, &t.text_primary, false);

        // 2. The five cards.
        for (i, mode) in ViewMode::ALL.iter().enumerate() {
            let rect = panel.card_rect(x, y, i);
            let selected = *mode == panel.mode;
            let hot = panel.hot == Some(LayoutHit::Card(i));
            // A checked RadioButton takes the accented background; otherwise the
            // subtle background of a ToggleButton.
            let (fill, fg) = if selected {
                (t.accent, t.accent_foreground)
            } else if hot {
                (t.control_fill_hover, t.text_primary)
            } else {
                (t.card_background, t.text_primary)
            };
            self.fill_rounded(&rect, 4.0, &fill);
            if !selected {
                self.stroke_rounded(&rect, 4.0, &t.card_stroke);
            }
            let cx = (rect.left + rect.right) / 2.0;
            let icon = Rect::new(cx - 14.0, rect.top + 10.0, cx + 14.0, rect.top + 38.0);
            self.vector_icon(mode.icon(), &icon, 28.0, &fg);
            let label = Rect::new(rect.left + 2.0, rect.top + 42.0, rect.right - 2.0, rect.bottom - 4.0);
            self.text_aligned(
                tr(mode.label_key()),
                &label,
                &f.caption,
                &fg,
                windows::Win32::Graphics::DirectWrite::DWRITE_TEXT_ALIGNMENT_CENTER,
            );
        }

        // 3. Full-width rule (Margin="-20,0" cancels the padding).
        let mut cursor = panel.cards_top(y) + CARD_H + SPACING;
        self.divider_line(x, x + width, cursor);

        // 4. Size.
        cursor += 1.0 + SPACING;
        let size_header = Rect::new(x + PAD, cursor, x + width - PAD, cursor + HEADER_H);
        self.text(tr("Size"), &size_header, &f.body_strong, &t.text_primary, false);

        // `@ui/RangeSlider`: a 6 DIP `rounded-full` rail on black-10%, filled
        // with the accent up to the value.
        let rail = panel.slider_rect(x, y);
        let cy = (rail.top + rail.bottom) / 2.0;
        let track = Rect::new(rail.left, cy - 3.0, rail.right, cy + 3.0);
        self.fill_rounded(&track, 3.0, &t.drive_bar_track);
        let thumb_x = rail.left + (rail.right - rail.left) * panel.slider_fraction();
        self.fill_rounded(&Rect::new(rail.left, cy - 3.0, thumb_x, cy + 3.0), 3.0, &t.accent);
        // The ticks (`TickPlacement="BottomRight"`), one per possible value.
        let max = panel.mode.size_max();
        for i in 0..max {
            let tx = rail.left + (rail.right - rail.left) * (i as f32 / (max - 1).max(1) as f32);
            self.fill_rounded(
                &Rect::new(tx - 0.5, cy + 6.0, tx + 0.5, cy + 11.0),
                0.0,
                &t.text_secondary,
            );
        }
        // The thumb is an ACCENT disc ringed by 2 DIP of white — that ring is
        // what detaches it from the filled part of the rail
        // (`box-shadow: 0 0 0 2px #fff`). It grows slightly while grabbed.
        let hot_slider = panel.hot == Some(LayoutHit::Slider) || panel.dragging;
        let r = if hot_slider { 7.0 } else { 6.0 };
        let ring = r + 2.0;
        self.fill_rounded(
            &Rect::new(thumb_x - ring, cy - ring, thumb_x + ring, cy + ring),
            ring,
            &t.layer_background,
        );
        self.fill_rounded(
            &Rect::new(thumb_x - r, cy - r, thumb_x + r, cy + r),
            r,
            &t.accent,
        );

        // The row of icons: placed on the named ticks, filled at the
        // current size (`IsFilled="{x:Bind ViewModel.IsLayoutSize…}"`).
        let icons_top = rail.bottom + SIZE_GAP;
        for (tick, name) in panel.mode.size_ticks() {
            let frac = (*tick as f32 - 1.0) / (max - 1).max(1) as f32;
            let tx = rail.left + (rail.right - rail.left) * frac;
            // The end icons stay within the panel.
            let cx = tx.clamp(x + PAD + 14.0, x + width - PAD - 14.0);
            let rect = Rect::new(cx - 14.0, icons_top, cx + 14.0, icons_top + SIZE_ICONS_H);
            let color =
                if *tick == panel.size { t.text_primary } else { t.text_secondary };
            self.vector_icon(name, &rect, 28.0, &color);
        }

        // 5. Second rule.
        cursor = icons_top + SIZE_ICONS_H + SPACING;
        self.divider_line(x, x + width, cursor);

        // 6. The two switches.
        // The labels are those of the commands: `ToggleShowHiddenItemsAction`
        // → `Strings.HiddenItems`, `ToggleShowFileExtensionsAction` →
        // `Strings.ShowFileExtensions`.
        for (second, (label, on)) in [
            (false, (tr("HiddenItems"), panel.show_hidden)),
            (true, (tr("ShowFileExtensions"), panel.show_extensions)),
        ] {
            let row = panel.toggle_rect(x, y, second);
            self.text(label, &row, &f.body, &t.text_primary, false);
            self.draw_toggle_switch(&row, on);
        }
    }

    fn divider_line(&self, left: f32, right: f32, y: f32) {
        let y = self.px(y);
        self.fill_rounded(
            &Rect::new(left, y, right, y + 1.0 / self.scale()),
            0.0,
            &self.theme.divider,
        );
    }

    /// A `ToggleSwitch`: the track, the thumb, then the On/Off text on the right.
    fn draw_toggle_switch(&self, row: &Rect, on: bool) {
        let t = self.theme;
        let f = &self.renderer.formats;
        let (on_text, off_text) = drive_localization::toggle_on_off();
        let text = if on { on_text } else { off_text };
        let text_w = 74.0;
        let track_left = row.right - text_w - SWITCH_W;
        let cy = (row.top + row.bottom) / 2.0;
        let track = self.draw_toggle((track_left, cy - SWITCH_H / 2.0), on, true);
        let label = Rect::new(track.right + 8.0, row.top, row.right, row.bottom);
        self.text(text, &label, &f.body, &t.text_primary, false);
    }
}
