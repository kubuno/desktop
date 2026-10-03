//! `ZoomSlider` — the zoom control at the right of the status bar: « − », a slider, « + » and the
//! percentage, which resets the zoom to 100 % (the web's `DocStatusBar` zoom cluster: same order,
//! same `Zoom arrière` / `Zoom avant` / `Rétablir à 100 %` tooltips, ±10 points per button).
//!
//! The slider is **non-linear**, as in Word: its middle is 100 %, its left half runs linearly from
//! 10 % to 100 % and its right half from 100 % to 500 % — the zoom range of the page canvas — so
//! the common zooms around 100 % get most of the travel. (The web's slider is linear over 50–200 %;
//! the desktop page zooms 10–500 %, which a linear slider of this length could not place well.)
//!
//! It is a lib control of Documents rather than a `kubuno-desktop-views` element: only the Office editors
//! show a zoom slider, and the mapping is theirs. It paints through the design system's own
//! `Slider` (`kubuno_desktop_ui::range::Slider`), so the rail and thumb are the shared ones.

use kubuno_desktop::prelude::*;
use kubuno_desktop::ui::graphics::Color;
use kubuno_desktop::ui::range::Slider;
use kubuno_desktop::ui::{Canvas, Rect, Size, Widget, WidgetState};
use kubuno_desktop::views::component::{AccessiblePart, Component, Control, ControlCore, EventCx, PaintEventCx};
use kubuno_desktop::views::events::{ChangeSource, Event};

/// The zoom range, in percent (`state::ZOOM_MIN` … `ZOOM_MAX`).
pub const MIN_PERCENT: f32 = 10.0;
pub const MAX_PERCENT: f32 = 500.0;
/// What « − » and « + » change, in points.
pub const STEP: f32 = 10.0;
/// The parts' widths (DIP): the buttons (`px-1.5` around a 14 DIP glyph), the slider (`w-28` and
/// its `px-1`), the percentage (`w-14`).
const BUTTON_W: f32 = 26.0;
const SLIDER_W: f32 = 120.0;
const PERCENT_W: f32 = 56.0;
/// The slider's resolution (positions 0…`TRACK`).
const TRACK: i32 = 1000;

/// Where `percent` sits on the slider, 0…1 (100 % in the middle).
pub fn position_of(percent: f32) -> f32 {
    let p = percent.clamp(MIN_PERCENT, MAX_PERCENT);
    if p <= 100.0 {
        (p - MIN_PERCENT) / (100.0 - MIN_PERCENT) * 0.5
    } else {
        0.5 + (p - 100.0) / (MAX_PERCENT - 100.0) * 0.5
    }
}

/// The percentage at slider position `t` (0…1), rounded to a whole percent.
pub fn percent_at(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    let p = if t <= 0.5 { MIN_PERCENT + t / 0.5 * (100.0 - MIN_PERCENT) } else { 100.0 + (t - 0.5) / 0.5 * (MAX_PERCENT - 100.0) };
    p.round()
}

/// One « − » (`dir` = -1) or « + » (1) from `percent`: the next multiple of ten that way, clamped.
pub fn stepped(percent: f32, dir: f32) -> f32 {
    let base = (percent / STEP).round() * STEP;
    let next = if (base - percent).abs() > 0.5 && (base - percent).signum() == dir.signum() { base } else { base + dir * STEP };
    next.clamp(MIN_PERCENT, MAX_PERCENT)
}

/// The parts of the control.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Part {
    Minus,
    Track,
    Plus,
    Percent,
}

/// The status bar's zoom control (see the module doc).
#[derive(kubuno_desktop::views::component::Component)]
#[kubuno(extends = Control, overrides(Control))]
#[category("Documents")]
#[toolbox(icon = "zoom-in")]
#[default_event("ValueChanged")]
pub struct ZoomSlider {
    base: ControlCore,
    /// The zoom, in percent (10–500).
    #[property(bindable)]
    #[category("Behavior")]
    #[default_value(100.0)]
    pub value: f32,
    /// Occurs when the user changes the zoom (a button, the slider), `e.new` in percent.
    #[event]
    #[category("Action")]
    pub value_changed: Event<NumericValueChangedEventArgs>,
    /// The part pressed (a button acts when released over it; the slider drags).
    pressed: Option<Part>,
    /// The part under the pointer.
    hot: Option<Part>,
}

impl Default for ZoomSlider {
    fn default() -> Self {
        Self { base: ControlCore::default(), value: 100.0, value_changed: Event::default(), pressed: None, hot: None }
    }
}

impl ZoomSlider {
    /// The parts' rectangles in `bounds` (right-aligned, so a wider control keeps them at its end).
    fn parts(bounds: Rect) -> [(Part, Rect); 4] {
        let total = 2.0 * BUTTON_W + SLIDER_W + PERCENT_W;
        let x = (bounds.right - total).max(bounds.left);
        let r = |l: f32, w: f32| Rect::new(l, bounds.top, l + w, bounds.bottom);
        [
            (Part::Minus, r(x, BUTTON_W)),
            (Part::Track, r(x + BUTTON_W, SLIDER_W)),
            (Part::Plus, r(x + BUTTON_W + SLIDER_W, BUTTON_W)),
            (Part::Percent, r(x + 2.0 * BUTTON_W + SLIDER_W, PERCENT_W)),
        ]
    }

    fn part_at(&self, x: f32, y: f32) -> Option<Part> {
        let local = Rect::new(0.0, 0.0, self.size().width, self.size().height);
        Self::parts(local).into_iter().find(|(_, r)| r.contains(x, y)).map(|(p, _)| p)
    }

    /// The slider widget for the current value (its rail inset by the slider's `px-1`).
    fn slider(&self) -> Slider {
        let mut s = Slider::new();
        s.set_minimum(0);
        s.set_maximum(TRACK);
        s.set_value_clamped((position_of(self.value) * TRACK as f32).round() as i32);
        s
    }

    fn track_rect(bounds: Rect) -> Rect {
        let r = Self::parts(bounds)[1].1;
        Rect::new(r.left + 4.0, r.top, r.right - 4.0, r.bottom)
    }

    /// Sets the zoom from the user and raises `ValueChanged`.
    fn change(&mut self, percent: f32) {
        let new = percent.clamp(MIN_PERCENT, MAX_PERCENT);
        if (new - self.value).abs() < 0.01 {
            return;
        }
        let old = self.value;
        self.value = new;
        self.raise_value_changed(NumericValueChangedEventArgs::new(old, new, ChangeSource::User));
        self.invalidate();
    }

    fn drag(&mut self, x: f32, y: f32) {
        let local = Rect::new(0.0, 0.0, self.size().width, self.size().height);
        let mut s = self.slider();
        s.drag_to(Self::track_rect(local), x, y);
        self.change(percent_at(s.value() as f32 / TRACK as f32));
    }

    /// The percentage as shown (`100 %`).
    fn percent_text(&self) -> String {
        format!("{} %", self.value.round() as i32)
    }
}

impl Control for ZoomSlider {
    fn get_preferred_size(&self, _canvas: &dyn Canvas, _proposed: Size) -> Size {
        Size { width: 2.0 * BUTTON_W + SLIDER_W + PERCENT_W, height: 22.0 }
    }

    fn on_paint(&mut self, e: &mut PaintEventCx<'_>) {
        let b = e.clip_rectangle;
        let c: &dyn Canvas = e.graphics;
        let t = c.theme();
        let dark = t.mode == kubuno_desktop::ui::ThemeMode::Dark;
        // `hover:bg-black/5`, and its dark-theme counterpart.
        let hover = if dark { Color::rgba_f(1.0, 1.0, 1.0, 0.08) } else { Color::rgba_f(0.0, 0.0, 0.0, 0.05) }.to_d2d();
        let ink = t.text_secondary;
        for (part, r) in Self::parts(b) {
            if part != Part::Track && (self.hot == Some(part) || self.pressed == Some(part)) && !self.design_mode() {
                c.fill_rounded(&r, 0.0, &hover);
            }
            match part {
                Part::Minus | Part::Plus => {
                    let name = if part == Part::Minus { "Minus" } else { "Plus" };
                    let (cx, cy) = ((r.left + r.right) / 2.0, (r.top + r.bottom) / 2.0);
                    c.vector_icon(name, &Rect::new(cx - 7.0, cy - 7.0, cx + 7.0, cy + 7.0), 14.0, &ink);
                }
                Part::Track => {
                    let state = WidgetState::REST.hot(self.hot == Some(Part::Track)).pressed(self.pressed == Some(Part::Track));
                    self.slider().paint(c, Self::track_rect(b), state);
                }
                Part::Percent => c.text(&self.percent_text(), &r, &c.formats().caption, &ink, true),
            }
        }
        e.raise(self, "OnPaint");
    }

    fn tool_tip_at(&self, x: f32, y: f32) -> Option<String> {
        use crate::Resources as R;
        Some(match self.part_at(x, y)? {
            Part::Minus => R::zoom_out().to_string(),
            Part::Plus => R::zoom_in().to_string(),
            Part::Percent => R::zoom_reset().to_string(),
            Part::Track => format!("{} {}", R::zoom_label(), self.percent_text()),
        })
    }

    fn accessible_parts(&self) -> Vec<AccessiblePart> {
        use crate::Resources as R;
        use kubuno_desktop::controls::host::access::AccessRole;
        let local = Rect::new(0.0, 0.0, self.size().width, self.size().height);
        Self::parts(local)
            .into_iter()
            .filter(|(p, _)| *p != Part::Track)
            .map(|(p, r)| {
                let name = match p {
                    Part::Minus => R::zoom_out().to_string(),
                    Part::Plus => R::zoom_in().to_string(),
                    _ => R::zoom_reset().to_string(),
                };
                AccessiblePart { name, role: AccessRole::Button, bounds: r }
            })
            .collect()
    }

    fn on_mouse_down(&mut self, e: &mut EventCx<'_, MouseEventArgs>) {
        if e.args().button == MouseButton::Left {
            let (x, y) = (e.args().x, e.args().y);
            self.pressed = self.part_at(x, y);
            if self.pressed == Some(Part::Track) {
                self.drag(x, y);
            }
            self.invalidate();
        }
        e.raise(&*self, "OnMouseDown");
    }

    fn on_mouse_move(&mut self, e: &mut EventCx<'_, MouseEventArgs>) {
        let (x, y) = (e.args().x, e.args().y);
        let hot = self.part_at(x, y);
        if hot != self.hot {
            self.hot = hot;
            self.invalidate();
        }
        if self.pressed == Some(Part::Track) {
            if e.args().button == MouseButton::Left {
                self.drag(x, y);
            } else {
                self.pressed = None;
            }
        }
        e.raise(&*self, "OnMouseMove");
    }

    fn on_mouse_up(&mut self, e: &mut EventCx<'_, MouseEventArgs>) {
        let released = self.part_at(e.args().x, e.args().y);
        match self.pressed.take() {
            Some(Part::Minus) if released == Some(Part::Minus) => self.change(stepped(self.value, -1.0)),
            Some(Part::Plus) if released == Some(Part::Plus) => self.change(stepped(self.value, 1.0)),
            Some(Part::Percent) if released == Some(Part::Percent) => self.change(100.0),
            _ => {}
        }
        self.invalidate();
        e.raise(&*self, "OnMouseUp");
    }

    fn on_mouse_leave(&mut self, e: &mut EventCx<'_, kubuno_desktop::views::events::EmptyEventArgs>) {
        if self.hot.take().is_some() {
            self.invalidate();
        }
        e.raise(&*self, "OnMouseLeave");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_middle_of_the_slider_is_one_hundred_percent() {
        assert_eq!(position_of(100.0), 0.5);
        assert_eq!(percent_at(0.5), 100.0);
        assert_eq!(percent_at(0.0), MIN_PERCENT);
        assert_eq!(percent_at(1.0), MAX_PERCENT);
        assert_eq!(position_of(5000.0), 1.0, "clamped");
    }

    #[test]
    fn positions_and_percentages_are_inverses() {
        for p in [10.0, 25.0, 50.0, 75.0, 100.0, 150.0, 200.0, 333.0, 500.0] {
            assert_eq!(percent_at(position_of(p)), p);
        }
        // The left half is ten times finer than the right one.
        assert!(position_of(110.0) - position_of(100.0) < position_of(100.0) - position_of(90.0));
    }

    #[test]
    fn the_buttons_step_to_the_next_ten() {
        assert_eq!(stepped(100.0, 1.0), 110.0);
        assert_eq!(stepped(100.0, -1.0), 90.0);
        assert_eq!(stepped(104.0, 1.0), 110.0);
        assert_eq!(stepped(104.0, -1.0), 100.0);
        assert_eq!(stepped(10.0, -1.0), 10.0);
        assert_eq!(stepped(500.0, 1.0), 500.0);
    }

    #[test]
    fn the_parts_sit_at_the_right_end_in_the_webs_order() {
        let parts = ZoomSlider::parts(Rect::new(0.0, 0.0, 400.0, 22.0));
        assert_eq!(parts[3].1.right, 400.0);
        assert_eq!(parts.map(|(p, _)| p), [Part::Minus, Part::Track, Part::Plus, Part::Percent]);
    }
}
