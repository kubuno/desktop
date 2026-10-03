//! The colour panel of the "Background colours" button (appearance settings).
//!
//! This is no longer a control of its own: it is
//! [`kubuno_desktop_ui::color::ColorPicker`], the design system's colour panel — hue
//! ring, saturation/value area (square, triangle or circle), harmonies, model
//! tabs with their channel sliders and the twelve fixed chips — driven by the
//! flyout's own gesture loop.
//!
//! What lives here is only the **adaptation layer**:
//!
//! * a `Copy` state the flyout can hold and compare ([`ColorPickerPanel`]),
//!   because the primitive's widget owns a `Vec` of chips and a `String` title
//!   and is therefore not `Copy`. The widget is BUILT from that state whenever
//!   geometry or paint is needed, so there is exactly one source of truth for
//!   the value;
//! * the conversion between Drive's `D2D1_COLOR_F` (channels 0..1) and the
//!   primitive's [`Color`] (RGB 0..255 with an `opacity` on 0..=100), written
//!   **once**, in [`from_d2d`], and covered by the tests at the bottom;
//! * the `#AARRGGBB` serialisation `app_theme_background_color` is persisted
//!   in, which the primitive does not carry (its own hex field is the web's
//!   six-digit `#rrggbb`).
//!
//! All the colour arithmetic, the hit-testing geometry and the activation of
//! discrete parts come from the primitive. The flyout is message driven (press,
//! move, release) and has no per-frame loop, so the primitive's frame-based
//! `pointer`/`keyboard` entry points are not used; the same building blocks
//! they are made of (`part_at`, `sv_at`, `hue_from_point`, `set_channel`,
//! `activate`) are called directly instead.

use kubuno_desktop_ui::color::{
    hue_from_point, js_round, Color, ColorMode, ColorPicker, Hsv, PickerLayout, PickerPart, Rgb, Scheme, SvShape,
};
use kubuno_desktop_ui::{Widget, WidgetState};
use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;

use crate::ui::{Painter, Rect};

/// The panel's width on the web — the primitive's own metric.
pub const PICKER_WIDTH: f32 = kubuno_desktop_ui::color::m::PICKER_W;

/// The picker's state, copied from `app_theme_background_color` on open.
///
/// It carries what the primitive's widget carries — the HSV triple, the
/// carried opacity and the three view choices (model tab, area shape, harmony
/// scheme) — so a fully desaturated colour keeps the hue it was left on and the
/// choices survive the widget being rebuilt for each gesture.
#[derive(Clone, Copy, PartialEq)]
pub struct ColorPickerPanel {
    /// Hue 0..360, saturation/value 0..1.
    pub hsv: Hsv,
    /// 0..=100. The web picker has no alpha control: the value is carried
    /// through untouched (see [`ColorPickerPanel::touch`] for the one exception).
    pub opacity: f64,
    pub mode: ColorMode,
    pub shape: SvShape,
    pub scheme: Scheme,
    /// The zone currently being dragged (the area, the ring and the channel
    /// sliders follow the mouse).
    pub drag: Option<PickerZone>,
}

/// A zone of the panel a gesture can act on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PickerZone {
    /// A continuous part (area, hue ring, channel slider): armed on
    /// button-down, it then follows the mouse.
    Drag(PickerPart),
    /// A discrete part (chip, shape, scheme, harmony swatch, model tab):
    /// resolved on click through the primitive's `activate`.
    Click(PickerPart),
    /// The close button or a footer button: ends the flyout.
    Close,
    /// Anywhere else inside the panel: the flyout stays open, nothing moves.
    Panel,
}

/// A Direct2D colour as the design system's colour model.
///
/// The only place the two scales meet: Drive keeps every channel on 0..1, the
/// primitive keeps RGB on 0..255 and alpha as an `opacity` on 0..=100 (the
/// name and the range the web's gradient model uses). The reverse direction is
/// the primitive's own [`Color::to_d2d`] and is never rewritten.
fn from_d2d(c: D2D1_COLOR_F) -> Color {
    Color::new(
        Rgb::new(f64::from(c.r) * 255.0, f64::from(c.g) * 255.0, f64::from(c.b) * 255.0),
        (f64::from(c.a) * 100.0).clamp(0.0, 100.0),
    )
}

impl ColorPickerPanel {
    pub fn from_color(c: D2D1_COLOR_F) -> Self {
        let colour = from_d2d(c);
        Self {
            hsv: colour.to_hsv(),
            opacity: colour.opacity,
            mode: ColorMode::Rgb,
            shape: SvShape::Square,
            scheme: Scheme::Comp,
            drag: None,
        }
    }

    /// The primitive's widget, built from this state.
    ///
    /// The recent grid stays empty (Drive has no colour history to feed it, and
    /// the primitive drops the whole block when the list is empty). The screen
    /// eyedropper is off: it needs a per-frame poll this flyout has no loop for.
    fn widget(&self) -> ColorPicker {
        let mut w = ColorPicker::new(Color::default());
        w.hsv = self.hsv;
        w.opacity = self.opacity;
        w.mode = self.mode;
        w.shape = self.shape;
        w.scheme = self.scheme;
        w.eyedropper = false;
        w
    }

    /// Copies back what a gesture may have changed on the widget.
    fn adopt(&mut self, w: &ColorPicker) {
        let before = self.hsv;
        self.hsv = w.hsv;
        self.mode = w.mode;
        self.shape = w.shape;
        self.scheme = w.scheme;
        if self.hsv != before {
            self.touch();
        }
    }

    /// Drive paints the colour as a translucent tint over the window, and its
    /// default is fully transparent (`#00000000`). The web picker has no alpha
    /// control, so without this the first pick would stay invisible: when the
    /// carried opacity is zero, choosing a colour makes it opaque. Any other
    /// opacity is kept as it is.
    fn touch(&mut self) {
        if self.opacity <= 0.0 {
            self.opacity = 100.0;
        }
    }

    /// The colour the panel names, in the design system's model.
    ///
    /// Drive persists that colour as a string and never as a `D2D1_COLOR_F`,
    /// so the Direct2D form is only ever reached through [`Color::to_d2d`] —
    /// inside the primitive's own paint, and in the tests below.
    fn colour(&self) -> Color {
        self.widget().color()
    }

    /// `#AARRGGBB`, the persisted format of `app_theme_background_color`.
    ///
    /// The primitive's own `hex_text()` is the web's six-digit string without
    /// alpha, so the alpha byte is appended here — through the same
    /// `Math.round` the rest of the family quantises with.
    pub fn hex(&self) -> String {
        let colour = self.colour();
        let (r, g, b) = colour.rgb.channels();
        let a = js_round(colour.opacity.clamp(0.0, 100.0) / 100.0 * 255.0) as u8;
        format!("#{a:02X}{r:02X}{g:02X}{b:02X}")
    }

    pub fn height(&self) -> f32 {
        self.widget().height_for_width(PICKER_WIDTH)
    }

    /// The panel's rectangle at origin `(x, y)`.
    fn bounds(widget: &ColorPicker, x: f32, y: f32) -> Rect {
        Rect::new(x, y, x + PICKER_WIDTH, y + widget.height_for_width(PICKER_WIDTH))
    }

    pub fn hit(&self, x: f32, y: f32, px: f32, py: f32) -> Option<PickerZone> {
        let widget = self.widget();
        let bounds = Self::bounds(&widget, x, y);
        if !bounds.contains(px, py) {
            return None;
        }
        // The primitive owns the geometry; everything it does not claim is
        // still inside the panel, which keeps the flyout open.
        Some(match widget.part_at(bounds, px, py) {
            Some(part @ (PickerPart::Area | PickerPart::Ring | PickerPart::Channel(_))) => PickerZone::Drag(part),
            Some(
                part @ (PickerPart::Swatch(_)
                | PickerPart::Recent(_)
                | PickerPart::Shape(_)
                | PickerPart::Scheme(_)
                | PickerPart::Harmony(_)
                | PickerPart::Mode(_)),
            ) => PickerZone::Click(part),
            // The footer's buttons end the flyout the way its close button
            // does: the colour is already applied live, as it is dragged.
            Some(PickerPart::Close | PickerPart::Cancel | PickerPart::Confirm) => PickerZone::Close,
            // The hex field and the numeric boxes are read-only here (the
            // flyout has no keyboard focus to route a text edit to), and the
            // eyedropper and module tools are not offered.
            Some(PickerPart::Hex | PickerPart::ChannelBox(_) | PickerPart::Eyedropper | PickerPart::Tool(_))
            | None => PickerZone::Panel,
        })
    }

    /// Applies a point (mouse) to the zone: the area, the ring and the sliders
    /// follow it, discrete parts are activated.
    pub fn apply(&mut self, zone: PickerZone, x: f32, y: f32, px: f32, py: f32) {
        let mut widget = self.widget();
        let g = widget.layout(Self::bounds(&widget, x, y));
        match zone {
            PickerZone::Drag(PickerPart::Area) => {
                // A drag is allowed to leave the area; the primitive clamps.
                let (s, v) = widget.sv_at(g.sv, px, py);
                widget.set_hsv(widget.hsv.h, s, v);
            }
            PickerZone::Drag(PickerPart::Ring) => {
                let h = hue_from_point(g.wheel, px, py);
                widget.set_hsv(h, widget.hsv.s, widget.hsv.v);
            }
            PickerZone::Drag(PickerPart::Channel(i)) => {
                if let (Some(row), Some(ch)) = (g.channels.get(i).copied(), widget.channels().get(i).cloned()) {
                    widget.set_channel(i, PickerLayout::channel_value_at(row, px, ch.max));
                }
            }
            // The shared activation: chips, shapes, schemes, harmonies, tabs.
            PickerZone::Click(part) => {
                widget.activate(part);
            }
            PickerZone::Drag(_) | PickerZone::Close | PickerZone::Panel => return,
        }
        self.adopt(&widget);
    }
}

impl Painter<'_> {
    /// Paints the full panel at origin (x, y) in DIP.
    pub(crate) fn draw_color_picker(&self, panel: &ColorPickerPanel, x: f32, y: f32) {
        let widget = panel.widget();
        let bounds = ColorPickerPanel::bounds(&widget, x, y);
        widget.paint(self, bounds, WidgetState::REST);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d2d(r: f32, g: f32, b: f32, a: f32) -> D2D1_COLOR_F {
        D2D1_COLOR_F { r, g, b, a }
    }

    /// The one conversion this file owns, both ways.
    #[test]
    fn d2d_round_trip() {
        let source = d2d(1.0, 0.5, 0.0, 0.5);
        let colour = from_d2d(source);
        assert_eq!(colour.rgb.channels(), (255, 128, 0));
        assert!((colour.opacity - 50.0).abs() < 1e-9);
        let back = colour.to_d2d();
        assert!((back.r - 1.0).abs() < 1e-6);
        assert!((back.a - 0.5).abs() < 1e-6);
    }

    /// A colour survives the trip through HSV, which is how the panel stores
    /// it — the primitive's arithmetic, exercised through this adapter.
    #[test]
    fn color_survives_hsv() {
        let source = d2d(0.2, 0.6, 0.9, 0.4);
        let panel = ColorPickerPanel::from_color(source);
        assert_eq!(panel.colour().rgb.channels(), from_d2d(source).rgb.channels());
        assert!((panel.colour().opacity - 40.0).abs() < 1e-4);
    }

    #[test]
    fn hex_is_argb() {
        let panel = ColorPickerPanel::from_color(d2d(1.0, 0.0, 0.0, 1.0));
        assert_eq!(panel.hex(), "#FFFF0000");
        let clear = ColorPickerPanel::from_color(d2d(0.0, 0.0, 0.0, 0.0));
        assert_eq!(clear.hex(), "#00000000");
    }

    /// A grey keeps the hue it was given rather than snapping to red — the
    /// web's own behaviour, inherited from the primitive's HSV state.
    #[test]
    fn grey_keeps_its_hue() {
        let mut panel = ColorPickerPanel::from_color(d2d(1.0, 0.0, 0.0, 1.0));
        panel.hsv.h = 210.0;
        panel.hsv.s = 0.0;
        let c = panel.colour().to_d2d();
        assert!((panel.hsv.h - 210.0).abs() < 1e-9);
        assert!((c.r - c.g).abs() < 1e-6 && (c.g - c.b).abs() < 1e-6);
    }

    /// Hit-testing and the geometry the drag uses agree: a click in the middle
    /// of the saturation/value area lands on it, and applying that point moves
    /// the colour there.
    #[test]
    fn area_hit_and_drag_agree() {
        let mut panel = ColorPickerPanel::from_color(d2d(1.0, 0.0, 0.0, 1.0));
        let widget = panel.widget();
        let sv = widget.layout(ColorPickerPanel::bounds(&widget, 0.0, 0.0)).sv;
        let (cx, cy) = ((sv.left + sv.right) / 2.0, (sv.top + sv.bottom) / 2.0);
        let zone = panel.hit(0.0, 0.0, cx, cy);
        assert_eq!(zone, Some(PickerZone::Drag(PickerPart::Area)));
        panel.apply(PickerZone::Drag(PickerPart::Area), 0.0, 0.0, cx, cy);
        assert!((panel.hsv.s - 0.5).abs() < 0.05);
        assert!((panel.hsv.v - 0.5).abs() < 0.05);
    }

    /// The hue ring answers where it is drawn, and dragging to the handle's
    /// position for a given hue lands on that hue.
    #[test]
    fn ring_hit_and_drag_agree() {
        let mut panel = ColorPickerPanel::from_color(d2d(1.0, 0.0, 0.0, 1.0));
        let widget = panel.widget();
        let wheel = widget.layout(ColorPickerPanel::bounds(&widget, 0.0, 0.0)).wheel;
        let (cx, cy) = ColorPicker::ring_point(wheel, 180.0);
        assert_eq!(panel.hit(0.0, 0.0, cx, cy), Some(PickerZone::Drag(PickerPart::Ring)));
        panel.apply(PickerZone::Drag(PickerPart::Ring), 0.0, 0.0, cx, cy);
        assert!((panel.hsv.h - 180.0).abs() < 2.0);
    }

    /// A model tab is a discrete part: clicking it switches the model the
    /// panel keeps, and the choice survives the widget being rebuilt.
    #[test]
    fn tab_click_switches_mode() {
        let mut panel = ColorPickerPanel::from_color(d2d(1.0, 0.0, 0.0, 1.0));
        let widget = panel.widget();
        let tab = widget.layout(ColorPickerPanel::bounds(&widget, 0.0, 0.0)).tabs[1];
        let (cx, cy) = ((tab.left + tab.right) / 2.0, (tab.top + tab.bottom) / 2.0);
        let zone = panel.hit(0.0, 0.0, cx, cy);
        assert_eq!(zone, Some(PickerZone::Click(PickerPart::Mode(ColorMode::ALL[1]))));
        panel.apply(zone.unwrap(), 0.0, 0.0, cx, cy);
        assert_eq!(panel.mode, ColorMode::ALL[1]);
        assert_eq!(panel.widget().mode, ColorMode::ALL[1]);
    }

    /// The default tint is fully transparent: picking a colour must make it
    /// visible, while an opacity the user already has is kept.
    #[test]
    fn pick_makes_transparent_default_visible() {
        let mut clear = ColorPickerPanel::from_color(d2d(0.0, 0.0, 0.0, 0.0));
        clear.apply(PickerZone::Click(PickerPart::Swatch(0)), 0.0, 0.0, 0.0, 0.0);
        assert!(clear.opacity > 99.0);
        let mut half = ColorPickerPanel::from_color(d2d(0.0, 0.0, 0.0, 0.5));
        half.apply(PickerZone::Click(PickerPart::Swatch(0)), 0.0, 0.0, 0.0, 0.0);
        assert!((half.opacity - 50.0).abs() < 1e-6);
    }

    /// Outside the panel there is no zone at all — what tells the flyout to
    /// close.
    #[test]
    fn outside_is_none() {
        let panel = ColorPickerPanel::from_color(d2d(0.0, 0.0, 0.0, 1.0));
        assert_eq!(panel.hit(0.0, 0.0, -10.0, -10.0), None);
        assert_eq!(panel.hit(0.0, 0.0, PICKER_WIDTH + 1.0, 10.0), None);
    }
}
