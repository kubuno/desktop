//! `StorageGauge` — the header's storage pill (the web's `FilesStorageGaugeHeader`): a drive
//! glyph, the design system's quota bar and « used / quota », one clickable block that lights up
//! under the pointer. No primitive holds those three things, so this custom control paints them
//! with the primitives' own pieces (`ProgressBar`, the canvas' glyphs and text).

use kubuno_desktop::ui::metrics::{pill, space};
use kubuno_desktop::ui::range::{ProgressBar, ProgressSize};
use kubuno_desktop::ui::{Canvas, Rect, Size, Widget, WidgetState};
use kubuno_desktop::views::component::{Component as _, Control, ControlCore, EventCx, HasControlCore, PaintEventCx};
use kubuno_desktop::views::events::EmptyEventArgs;

/// The gauge's track: `w-20`, and the resolution its fraction is kept at.
const TRACK: f32 = 80.0;
const STEPS: i32 = 1000;
/// The drive glyph.
const GLYPH: f32 = 16.0;

/// The storage pill of the header (see the module doc). `Click` opens the storage page.
#[derive(kubuno_desktop::views::component::Component, Default)]
#[kubuno(extends = Control, overrides(Control))]
#[category("Kubuno")]
#[toolbox(icon = "hard-drive")]
#[default_event("Click")]
pub struct StorageGauge {
    base: ControlCore,
    /// Bytes in use.
    #[property(bindable)]
    #[category("Data")]
    pub used_bytes: u64,
    /// Bytes granted; 0 for no quota (the gauge then shows an empty bar).
    #[property(bindable)]
    #[category("Data")]
    pub quota_bytes: u64,
}

impl StorageGauge {
    /// Shows `used` of `quota` bytes.
    pub fn set_usage(&mut self, used: u64, quota: u64) {
        if (self.used_bytes, self.quota_bytes) != (used, quota) {
            self.used_bytes = used;
            self.quota_bytes = quota;
            self.invalidate();
        }
    }

    /// The text written after the bar.
    pub fn label(&self) -> String {
        format!("{} / {}", crate::model::view_model::format_size(self.used_bytes), crate::model::view_model::format_size(self.quota_bytes))
    }
}

/// The gauge's bar: the design system's quota bar, `sm` (6 DIP) and in its `Auto` variant,
/// which turns amber then red at the web's own thresholds.
pub fn gauge(used: u64, quota: u64) -> ProgressBar {
    let mut bar = ProgressBar::new().with_size(ProgressSize::Sm);
    let fraction = if quota == 0 { 0.0 } else { (used as f64 / quota as f64).min(1.0) };
    bar.set_maximum(STEPS);
    bar.set_value((fraction * f64::from(STEPS)).round() as i32);
    bar
}

/// Where the three parts go in the pill `r`: the glyph, the bar's band, the text.
pub fn parts(r: Rect) -> (Rect, Rect, Rect) {
    let icon = Rect::new(r.left + space::MD, r.top, r.left + space::MD + GLYPH, r.bottom);
    let track = Rect::new(icon.right + space::SM, r.top, icon.right + space::SM + TRACK, r.bottom);
    let text = Rect::new(track.right + space::SM, r.top, r.right - space::SM, r.bottom);
    (icon, track, text)
}

impl Control for StorageGauge {
    fn get_preferred_size(&self, _canvas: &dyn Canvas, _proposed: Size) -> Size {
        // Wide enough for « 1023.9 Mo / 15.00 Go » without an ellipsis.
        Size { width: 240.0, height: 36.0 }
    }

    fn on_paint(&mut self, e: &mut PaintEventCx<'_>) {
        let r = e.clip_rectangle;
        // The designer shows a gauge a third full (no account there).
        if self.quota_bytes == 0 && self.design_mode() {
            self.used_bytes = 3_500_000_000;
            self.quota_bytes = 16_000_000_000;
        }
        let c: &dyn Canvas = e.graphics;
        let t = c.theme().clone();
        if self.control_core().hot {
            c.fill_rounded(&r, pill(r.bottom - r.top), &t.control_fill_hover);
        }
        let (icon, track, text) = parts(r);
        c.vector_icon("HardDrive", &icon, GLYPH, &t.text_tertiary);
        // The bar centres its own 6 DIP track in the band it is given.
        gauge(self.used_bytes, self.quota_bytes).paint(c, track, WidgetState::REST);
        c.text_ellipsis(&self.label(), &text, &c.formats().caption, &t.text_secondary);
        e.raise(self, "OnPaint");
    }

    fn on_mouse_enter(&mut self, e: &mut EventCx<'_, EmptyEventArgs>) {
        self.invalidate();
        e.raise(&*self, "OnMouseEnter");
    }

    fn on_mouse_leave(&mut self, e: &mut EventCx<'_, EmptyEventArgs>) {
        self.invalidate();
        e.raise(&*self, "OnMouseLeave");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The bar follows the web's quota thresholds — amber from 75 %, red from 90 % — and an
    /// unset quota draws an empty bar.
    #[test]
    fn the_gauge_follows_the_quota_thresholds() {
        use kubuno_desktop::ui::range::ProgressVariant;
        assert_eq!(gauge(50, 100).resolved_variant(), ProgressVariant::Primary);
        assert_eq!(gauge(80, 100).resolved_variant(), ProgressVariant::Warning);
        assert_eq!(gauge(95, 100).resolved_variant(), ProgressVariant::Danger);
        assert_eq!(gauge(300, 100).fraction(), 1.0, "an overrun quota is a full bar");
        assert_eq!(gauge(10, 0).fraction(), 0.0);
        assert!((gauge(3_500_000_000, 16_000_000_000).fraction() - 0.219).abs() < 0.001);
    }

    #[test]
    fn the_parts_sit_in_the_pill() {
        let (icon, track, text) = parts(Rect::new(0.0, 0.0, 240.0, 36.0));
        assert!(icon.right < track.left && track.right < text.left && text.right <= 240.0);
    }
}
