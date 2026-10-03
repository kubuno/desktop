//! `BarChart` — a mini bar chart of a daily series (the dashboard's « Connexions par jour »): one
//! accent bar per day, its height the day's count over the series' largest, 3 DIP apart. The design
//! system has no chart primitive; this is the one the console needs.

use kubuno::ui::{Canvas, Rect, Size};
use kubuno::views::component::{Component as _, Control, ControlCore, PaintEventCx};

/// The gap between two bars.
const GAP: f32 = 3.0;
/// The bars' corner radius.
const RADIUS: f32 = 2.0;

/// A daily series as bars (see the module doc).
#[derive(kubuno::views::component::Component, Default)]
#[kubuno(extends = Control, overrides(Control))]
#[category("Kubuno")]
#[toolbox(icon = "chart-column")]
pub struct BarChart {
    base: ControlCore,
    /// The series, oldest first, as counts separated by commas (« 12,9,14 »): what a view binds.
    #[property(bindable, on_change = "series_changed")]
    #[category("Data")]
    pub series: String,
    values: Vec<i64>,
}

impl BarChart {
    fn series_changed(&mut self) {
        let values = parse_series(&self.series);
        self.set_values(values);
    }

    /// Shows `values` (oldest first).
    pub fn set_values(&mut self, values: Vec<i64>) {
        if self.values != values {
            self.values = values;
            self.invalidate();
        }
    }
}

/// A series written « 12,9,14 » (what is not a count is left out).
pub fn parse_series(text: &str) -> Vec<i64> {
    text.split(',').filter_map(|v| v.trim().parse().ok()).collect()
}

/// The series `values` as a property value.
pub fn series_text(values: &[i64]) -> String {
    values.iter().map(i64::to_string).collect::<Vec<_>>().join(",")
}

/// The bars of `values` in `plot`.
pub fn bars(values: &[i64], plot: Rect) -> Vec<Rect> {
    if values.is_empty() {
        return Vec::new();
    }
    let max = values.iter().copied().max().unwrap_or(1).max(1) as f32;
    let n = values.len() as f32;
    let width = ((plot.right - plot.left) - (n - 1.0) * GAP) / n;
    values
        .iter()
        .enumerate()
        .map(|(i, &v)| {
            let h = (v.max(0) as f32 / max) * (plot.bottom - plot.top);
            let left = plot.left + i as f32 * (width + GAP);
            Rect::new(left, plot.bottom - h, left + width, plot.bottom)
        })
        .collect()
}

impl Control for BarChart {
    fn get_preferred_size(&self, _canvas: &dyn Canvas, _proposed: Size) -> Size {
        Size { width: 340.0, height: 96.0 }
    }

    fn on_paint(&mut self, e: &mut PaintEventCx<'_>) {
        if self.values.is_empty() && self.design_mode() {
            self.values = vec![12, 9, 14, 15, 13, 3, 2, 12, 13, 15, 16, 14, 4, 2];
        }
        let c: &dyn Canvas = e.graphics;
        let accent = c.theme().accent;
        for bar in bars(&self.values, e.clip_rectangle) {
            c.fill_rounded(&bar, RADIUS, &accent);
        }
        e.raise(self, "OnPaint");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_largest_day_fills_the_plot() {
        let b = bars(&[2, 4, 0], Rect::new(0.0, 0.0, 66.0, 100.0));
        assert_eq!(b.len(), 3);
        assert_eq!(b[1].top, 0.0);
        assert_eq!(b[0].top, 50.0);
        assert_eq!(b[2].top, 100.0, "an empty day has no bar");
        assert!((b[1].left - (b[0].right + GAP)).abs() < 0.01);
        assert!(bars(&[], Rect::new(0.0, 0.0, 10.0, 10.0)).is_empty());
    }
}
