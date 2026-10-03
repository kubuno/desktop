//! `StatusDot` — a small filled circle in a theme colour: the connection state at the foot of
//! the rail, the sync state and the server line of the launcher. The design system has no
//! filled-dot primitive (a `Badge` dot is tied to its pill), so this custom control paints the
//! one shape the former pages drew by hand: a pill as wide as it is tall.

use kubuno_desktop::ui::{Canvas, Rect, Size};
use kubuno_desktop::views::component::{Control, ControlCore, PaintEventCx};
use kubuno_desktop::views::style::ColorValue;

/// A filled circle of `Diameter` DIP, centred in its box, in `Color` (a theme token such as
/// `Success`, `Warning`, `Danger`, `Primary`, `TextTertiary`).
#[derive(kubuno_desktop::views::component::Component)]
#[kubuno(extends = Control, overrides(Control))]
#[category("Kubuno")]
#[toolbox(icon = "circle")]
pub struct StatusDot {
    base: ControlCore,
    /// The dot's colour: a theme token, or a free colour.
    #[property(bindable)]
    #[category("Appearance")]
    pub color: Option<ColorValue>,
    /// The dot's diameter, in DIP.
    #[property]
    #[category("Layout")]
    #[default_value(8.0)]
    pub diameter: f32,
    /// Rings the dot in the accent colour, 2 DIP outside it (the current choice of a palette).
    #[property(bindable)]
    #[category("Appearance")]
    pub ring: bool,
}

impl Default for StatusDot {
    fn default() -> Self {
        Self { base: ControlCore::default(), color: None, diameter: 8.0, ring: false }
    }
}

/// The circle's box: `diameter` wide and tall, centred in `bounds` (never larger than it).
pub fn dot_rect(bounds: Rect, diameter: f32) -> Rect {
    let d = diameter.max(0.0).min(bounds.right - bounds.left).min(bounds.bottom - bounds.top);
    let cx = (bounds.left + bounds.right) / 2.0;
    let cy = (bounds.top + bounds.bottom) / 2.0;
    Rect::new(cx - d / 2.0, cy - d / 2.0, cx + d / 2.0, cy + d / 2.0)
}

impl Control for StatusDot {
    fn get_preferred_size(&self, _canvas: &dyn Canvas, _proposed: Size) -> Size {
        Size { width: self.diameter, height: self.diameter }
    }

    fn on_paint(&mut self, e: &mut PaintEventCx<'_>) {
        let bounds = e.clip_rectangle;
        let canvas: &dyn Canvas = e.graphics;
        let color = match &self.color {
            Some(c) => c.resolve_with(canvas.theme(), kubuno_desktop::views::style::high_contrast()),
            None => canvas.theme().text_tertiary,
        };
        let dot = dot_rect(bounds, self.diameter);
        let d = dot.right - dot.left;
        if d > 0.0 {
            canvas.fill_rounded(&dot, d / 2.0, &color);
        }
        if self.ring && d > 0.0 {
            let ring = Rect::new(dot.left - 2.0, dot.top - 2.0, dot.right + 2.0, dot.bottom + 2.0);
            canvas.stroke_rounded(&ring, (d + 4.0) / 2.0, &canvas.theme().accent);
        }
        e.raise(self, "OnPaint");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_dot_is_centred_and_fits_its_box() {
        let r = dot_rect(Rect::new(0.0, 0.0, 20.0, 20.0), 8.0);
        assert_eq!((r.left, r.top, r.right, r.bottom), (6.0, 6.0, 14.0, 14.0));
        let r = dot_rect(Rect::new(0.0, 0.0, 6.0, 30.0), 12.0);
        assert_eq!(r.right - r.left, 6.0, "never wider than its box");
    }
}
