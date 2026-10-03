//! The shared button, a replica of the web's `@ui/Button`
//! (`core/frontend/src/ui/Button.tsx`).
//!
//! Every number here was MEASURED on the running web app through CDP, not read
//! off the Tailwind classes: the project scales its radius ramp, so `rounded-md`
//! resolves to **4** and not the 6 the class name suggests. Reading the source
//! alone is how the desktop ended up with pill-shaped buttons the web never has.
//!
//! Two rules come straight from the web component and are enforced here rather
//! than left to callers:
//!
//! * **The radius is not overridable.** `Button.tsx` says so in a comment and
//!   its stylesheet order makes it true — a caller appending `rounded-full`
//!   still renders at 4. So [`RADIUS`] is a constant and no variant changes it.
//! * **Buttons are never emboldened.** Colour and fill carry the hierarchy; the
//!   label is always regular weight at 14/20.
//!
//! Horizontal padding IS overridable, because the web overrides it (the waffle's
//! edit buttons use `px-5` and `px-6`).

use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;

use crate::themes::Theme;
use crate::{Canvas, Rect};

/// `rounded-md`, which this design system resolves to 4. Never overridable —
/// see the module docs.
pub const RADIUS: f32 = 4.0;

/// The web's `VARIANT` map.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Variant {
    /// Filled accent: the action that carries a page.
    Primary,
    /// White with a border.
    Secondary,
    /// No fill until hovered.
    Ghost,
    /// The accent WITHOUT a fill — a dialog's confirming action next to a
    /// `Ghost` cancel.
    Text,
    /// Filled danger.
    Danger,
    /// Destructive, without a fill. Its own variant on the web too, because two
    /// colour utilities on one element are settled by stylesheet order and the
    /// override silently lost.
    TextDanger,
}

/// The web's `SIZE` map: `h-8/h-9/h-11` over `px-3/px-4/px-5`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Size {
    Sm,
    Md,
    Lg,
}

impl Size {
    /// From the shape tokens, so a height is never stated twice.
    pub fn height(self) -> f32 {
        use crate::themes::shape::height as h;
        match self {
            Size::Sm => h::BUTTON_SM,
            Size::Md => h::BUTTON_MD,
            Size::Lg => h::BUTTON_LG,
        }
    }

    pub fn pad_x(self) -> f32 {
        match self {
            Size::Sm => 12.0,
            Size::Md => 16.0,
            Size::Lg => 20.0,
        }
    }

    /// `gap-1.5` on the small size, `gap-2` on the others.
    pub fn gap(self) -> f32 {
        match self {
            Size::Sm => 6.0,
            Size::Md | Size::Lg => 8.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Rest,
    Hover,
    Active,
    Disabled,
}

/// One button. Built with [`Button::new`] and adjusted with the builders, so a
/// caller states only what differs from the web's defaults (`primary`, `md`).
#[derive(Debug, Clone, Copy)]
pub struct Button<'a> {
    pub label:   &'a str,
    /// A leading icon, by name, drawn before the label. `'static` because an
    /// icon name is a compile-time constant, as the canvas requires.
    pub icon:    Option<&'static str>,
    pub variant: Variant,
    pub size:    Size,
    /// Overrides the size's horizontal padding — the one metric the web lets a
    /// caller change.
    pub pad_x:   Option<f32>,
}

impl<'a> Button<'a> {
    pub fn new(label: &'a str) -> Self {
        Self { label, icon: None, variant: Variant::Primary, size: Size::Md, pad_x: None }
    }

    pub fn variant(mut self, v: Variant) -> Self {
        self.variant = v;
        self
    }

    pub fn size(mut self, s: Size) -> Self {
        self.size = s;
        self
    }

    pub fn icon(mut self, name: &'static str) -> Self {
        self.icon = Some(name);
        self
    }

    pub fn pad_x(mut self, pad: f32) -> Self {
        self.pad_x = Some(pad);
        self
    }

    fn padding(&self) -> f32 {
        self.pad_x.unwrap_or_else(|| self.size.pad_x())
    }

    /// The glyph size the web uses inside a button: `size-4` on `sm`, else 18.
    fn icon_size(&self) -> f32 {
        match self.size {
            Size::Sm => 16.0,
            Size::Md | Size::Lg => 18.0,
        }
    }
}

/// The fill, label colour and whether a border is stroked, for one state.
struct Paint {
    fill:   Option<D2D1_COLOR_F>,
    label:  D2D1_COLOR_F,
    border: bool,
}

fn dim(c: D2D1_COLOR_F, alpha: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F { a: c.a * alpha, ..c }
}

fn paint_for(b: &Button, t: &Theme, state: State) -> Paint {
    let hovered = matches!(state, State::Hover);
    let pressed = matches!(state, State::Active);
    let p = match b.variant {
        Variant::Primary => Paint {
            // `hover:bg-primary-hover active:bg-primary-hover` — one colour for
            // both, as the web writes it.
            fill:   Some(if hovered || pressed { t.accent_hover } else { t.accent }),
            label:  t.accent_foreground,
            border: false,
        },
        Variant::Secondary => Paint {
            fill:   Some(if pressed {
                t.surface_2
            } else if hovered {
                t.card_background
            } else {
                t.layer_background
            }),
            label:  t.text_primary,
            border: true,
        },
        Variant::Ghost => Paint {
            fill:   if pressed {
                Some(t.surface_3)
            } else if hovered {
                Some(t.surface_2)
            } else {
                None
            },
            label:  t.text_secondary,
            border: false,
        },
        Variant::Text => Paint {
            fill:   (hovered || pressed).then_some(t.accent_light),
            label:  t.accent,
            border: false,
        },
        Variant::Danger => Paint {
            // `hover:opacity-90 active:opacity-80` on the web: the fill itself
            // fades rather than changing hue.
            fill:   Some(if pressed {
                dim(t.danger, 0.8)
            } else if hovered {
                dim(t.danger, 0.9)
            } else {
                t.danger
            }),
            label:  t.accent_foreground,
            border: false,
        },
        Variant::TextDanger => Paint {
            fill:   (hovered || pressed).then_some(t.danger_light),
            label:  t.danger,
            border: false,
        },
    };
    if matches!(state, State::Disabled) {
        // `disabled:opacity-50` over the whole button.
        return Paint {
            fill: p.fill.map(|f| dim(f, 0.5)),
            label: dim(p.label, 0.5),
            border: p.border,
        };
    }
    p
}

/// The button's intrinsic width: padding, the icon and its gap, then the label —
/// the same content-driven sizing the web has. Measured, so it tracks the real
/// font: the web's « Annuler » comes to 88.9 and its « OK » to 67.3.
pub fn width(c: &dyn Canvas, b: &Button) -> f32 {
    let f = c.formats();
    let text = if b.label.is_empty() { 0.0 } else { c.measure(b.label, &f.body) };
    let icon = match b.icon {
        Some(_) if b.label.is_empty() => b.icon_size(),
        Some(_) => b.icon_size() + b.size.gap(),
        None => 0.0,
    };
    b.padding() * 2.0 + icon + text
}

/// Lays a button out at its intrinsic width, with its top-left at `(x, y)`.
pub fn rect_at(c: &dyn Canvas, b: &Button, x: f32, y: f32) -> Rect {
    Rect::new(x, y, x + width(c, b), y + b.size.height())
}

/// Lays a button out at its intrinsic width, with its top-RIGHT at `(right, y)`.
pub fn rect_ending_at(c: &dyn Canvas, b: &Button, right: f32, y: f32) -> Rect {
    Rect::new(right - width(c, b), y, right, y + b.size.height())
}

/// Draws the button inside `rect`. The caller owns the rectangle so a row of
/// buttons can be aligned or stretched; [`rect_at`] gives the intrinsic one.
pub fn draw(c: &dyn Canvas, rect: &Rect, b: &Button, state: State) {
    let t = c.theme();
    let f = c.formats();
    let p = paint_for(b, t, state);

    if let Some(fill) = p.fill {
        c.fill_rounded(rect, RADIUS, &fill);
    }
    if p.border {
        c.stroke_rounded(rect, RADIUS, &t.card_stroke);
    }

    // `inline-flex items-center justify-center`: the icon and the label are
    // centred TOGETHER, not each in its own half.
    let pad = b.padding();
    match b.icon {
        Some(name) if b.label.is_empty() => {
            c.vector_icon(name, rect, b.icon_size(), &p.label);
        }
        Some(name) => {
            let size = b.icon_size();
            let text = c.measure(b.label, &f.body);
            let total = size + b.size.gap() + text;
            let start = (rect.left + rect.right) / 2.0 - total / 2.0;
            let icon = Rect::new(start, rect.top, start + size, rect.bottom);
            c.vector_icon(name, &icon, size, &p.label);
            let label = Rect::new(icon.right + b.size.gap(), rect.top, rect.right - pad, rect.bottom);
            c.text(b.label, &label, &f.body, &p.label, false);
        }
        None => {
            c.text(b.label, rect, &f.body, &p.label, true);
        }
    }
}

/// A round icon button — the header's 36px controls and the waffle's 40px
/// pencil. These ARE circles on the web (`rounded-full`), unlike the text
/// buttons; the two shapes are easy to mix up, so they live in one place.
pub struct IconButton {
    pub icon:  &'static str,
    /// The circle's diameter: 36 in the header, 40 for the waffle pencil.
    pub size:  f32,
    /// The glyph inside it: 18 in the header, 16 for the pencil.
    pub glyph: f32,
    /// Whether it carries a resting fill (the pencil sits on surface-2, the
    /// header's controls on nothing).
    pub filled: bool,
}

impl IconButton {
    /// The header's control: 36px circle, 18px glyph, no resting fill.
    pub fn header(icon: &'static str) -> Self {
        Self { icon, size: 36.0, glyph: 18.0, filled: false }
    }

    /// The waffle's pencil: 40px circle on surface-2, 16px glyph.
    pub fn tinted(icon: &'static str, size: f32, glyph: f32) -> Self {
        Self { icon, size, glyph, filled: true }
    }
}

pub fn draw_icon_button(c: &dyn Canvas, rect: &Rect, b: &IconButton, state: State) {
    let t = c.theme();
    let radius = crate::themes::shape::pill(rect.bottom - rect.top);
    let fill = match state {
        State::Active => Some(t.surface_3),
        State::Hover => Some(t.surface_3),
        _ if b.filled => Some(t.surface_2),
        _ => None,
    };
    if let Some(fill) = fill {
        c.fill_rounded(rect, radius, &fill);
    }
    let colour = if matches!(state, State::Disabled) {
        dim(t.text_secondary, 0.5)
    } else {
        t.text_secondary
    };
    c.vector_icon(b.icon, rect, b.glyph, &colour);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The web's measured geometry, pinned. `rounded-md` really is 4.
    #[test]
    fn the_sizes_match_the_measured_web_button() {
        assert_eq!(RADIUS, 4.0);
        assert_eq!(Size::Sm.height(), 32.0);
        assert_eq!(Size::Md.height(), 36.0);
        assert_eq!(Size::Lg.height(), 44.0);
        assert_eq!(Size::Sm.pad_x(), 12.0);
        assert_eq!(Size::Md.pad_x(), 16.0);
        assert_eq!(Size::Lg.pad_x(), 20.0);
        assert_eq!(Size::Sm.gap(), 6.0);
        assert_eq!(Size::Md.gap(), 8.0);
    }

    /// Padding is the only overridable metric, and it reaches `width`.
    #[test]
    fn padding_is_overridable_but_the_radius_is_not() {
        let b = Button::new("OK").pad_x(24.0);
        assert_eq!(b.padding(), 24.0);
        assert_eq!(Button::new("OK").padding(), 16.0);
        // No builder, field or variant can change the radius: it is a const.
        assert_eq!(RADIUS, 4.0);
    }

    #[test]
    fn the_builders_default_to_the_webs_defaults() {
        let b = Button::new("Envoyer");
        assert_eq!(b.variant, Variant::Primary);
        assert_eq!(b.size, Size::Md);
        assert!(b.icon.is_none());
        let g = Button::new("Annuler").variant(Variant::Ghost).pad_x(20.0);
        assert_eq!(g.variant, Variant::Ghost);
        assert_eq!(g.padding(), 20.0);
    }
}
