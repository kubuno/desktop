//! `StackedBar` — a composition bar: a rounded track with proportional segments laid end to end (the
//! storage's accounts / other content / free space, the accounts' ok / near / full). A `ProgressBar`
//! carries one fill; this one carries several, on the same track token and thickness, so the two read
//! as one family.

use kubuno_desktop::ui::metrics::pill;
use kubuno_desktop::ui::{Canvas, Rect, Size};
use kubuno_desktop::views::component::{Component as _, Control, ControlCore, PaintEventCx};

/// The track's thickness (the `ProgressBar`'s `Md`).
pub const BAR_H: f32 = 8.0;

/// One segment: its colour (a theme token name) and its amount.
#[derive(Debug, Clone, PartialEq)]
pub struct Segment {
    pub color: String,
    pub value: f64,
}

/// The segments written « Primary=812;TextTertiary=12 » (what does not parse is left out).
pub fn parse_segments(text: &str) -> Vec<Segment> {
    text.split(';')
        .filter_map(|part| {
            let (color, value) = part.split_once('=')?;
            Some(Segment { color: color.trim().to_string(), value: value.trim().parse().ok()? })
        })
        .collect()
}

/// The segments as a property value.
pub fn segments_text(segments: &[(&str, f64)]) -> String {
    segments.iter().map(|(c, v)| format!("{c}={v}")).collect::<Vec<_>>().join(";")
}

/// Where each segment goes on `track` for a `total` (the rest of the track stays empty).
pub fn layout(segments: &[Segment], total: f64, track: Rect) -> Vec<(usize, Rect)> {
    let total = total.max(1.0);
    let width = track.right - track.left;
    let mut x = track.left;
    let mut out = Vec::new();
    for (i, s) in segments.iter().enumerate() {
        let w = width * (s.value.max(0.0) / total) as f32;
        if w > 0.5 {
            let right = (x + w).min(track.right);
            out.push((i, Rect::new(x, track.top, right, track.bottom)));
            x = right;
        }
    }
    out
}

/// A composition bar (see the module doc).
#[derive(kubuno_desktop::views::component::Component, Default)]
#[kubuno(extends = Control, overrides(Control))]
#[category("Kubuno")]
#[toolbox(icon = "chart-bar-stacked")]
pub struct StackedBar {
    base: ControlCore,
    /// The segments, « Primary=812;TextTertiary=12 » (colours are theme tokens: Primary, Success,
    /// Warning, Danger, TextTertiary).
    #[property(bindable)]
    #[category("Data")]
    pub segments: String,
    /// What the whole track stands for.
    #[property(bindable)]
    #[category("Data")]
    pub total: f32,
}

impl Control for StackedBar {
    fn get_preferred_size(&self, _canvas: &dyn Canvas, _proposed: Size) -> Size {
        Size { width: 600.0, height: BAR_H }
    }

    fn on_paint(&mut self, e: &mut PaintEventCx<'_>) {
        if self.segments.is_empty() && self.design_mode() {
            self.segments = "Primary=812;TextTertiary=60".into();
            self.total = 4000.0;
        }
        let b = e.clip_rectangle;
        let c: &dyn Canvas = e.graphics;
        let t = c.theme().clone();
        let cy = (b.top + b.bottom) / 2.0;
        let track = Rect::new(b.left, cy - BAR_H / 2.0, b.right, cy + BAR_H / 2.0);
        c.fill_rounded(&track, pill(BAR_H), &t.surface_2);
        let segments = parse_segments(&self.segments);
        for (i, r) in layout(&segments, f64::from(self.total), track) {
            let colour = match segments[i].color.as_str() {
                "Success" => t.success,
                "Warning" => t.warning,
                "Danger" => t.danger,
                "TextTertiary" => t.text_tertiary,
                _ => t.accent,
            };
            c.fill_rounded(&r, pill(BAR_H), &colour);
        }
        e.raise(self, "OnPaint");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segments_follow_one_another_in_proportion() {
        let s = parse_segments("Primary=50;TextTertiary=25;nonsense");
        assert_eq!(s.len(), 2);
        let l = layout(&s, 100.0, Rect::new(0.0, 0.0, 200.0, 8.0));
        assert_eq!(l[0].1.right, 100.0);
        assert_eq!(l[1].1.left, 100.0);
        assert_eq!(l[1].1.right, 150.0);
        assert_eq!(segments_text(&[("Success", 3.0)]), "Success=3");
    }
}
