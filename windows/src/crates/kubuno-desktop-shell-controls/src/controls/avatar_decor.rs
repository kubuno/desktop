//! `AvatarDecor` — what the web's account panel draws around the active account's 96 avatar: a 3-DIP
//! white outline (`outline: 3px solid #fff`, which covers its `box-shadow` ring), and
//! the camera button at its bottom right (`absolute bottom-1 right-0`, a 32 white disc with a border and
//! a shadow, a 15 glyph), which raises `CameraClicked`. Placed over the `<Avatar>` it decorates, 3 DIP
//! larger on every side; only the camera takes the pointer.

use kubuno_desktop::controls::host::access::AccessRole;
use kubuno_desktop::ui::metrics::pill;
use kubuno_desktop::ui::{Canvas, Rect, Size};
use kubuno_desktop::views::component::{AccessiblePart, Control, ControlCore, EventCx, PaintEventCx};
use kubuno_desktop::views::events::{EmptyEventArgs, Event, MouseEventArgs};

use crate::ShellControlsResources;

/// The outline and the ring around the avatar.
pub const RING: f32 = 3.0;
const CAMERA: f32 = 32.0;
const GLYPH: f32 = 15.0;

/// The avatar's ring and its camera (see the module doc).
#[derive(kubuno_desktop::views::component::Component, Default)]
#[kubuno(extends = Control, overrides(Control))]
#[category("Kubuno")]
#[toolbox(icon = "camera")]
#[default_event("CameraClicked")]
pub struct AvatarDecor {
    base: ControlCore,
    /// Shows the camera button.
    #[property(bindable)]
    #[category("Appearance")]
    pub show_camera: bool,
    /// An upload is under way: the camera takes no click.
    #[property(bindable)]
    #[category("Behavior")]
    pub busy: bool,
    /// Occurs when the camera button is clicked.
    #[event]
    #[category("Action")]
    pub camera_clicked: Event<EmptyEventArgs>,
    hot: bool,
    pressed: bool,
    size: f32,
}

impl AvatarDecor {
    /// The camera's disc, in local coordinates of a decor `size` wide (the avatar `size - 2 * RING`).
    pub fn camera_rect(size: f32) -> Rect {
        let avatar = size - 2.0 * RING;
        let right = RING + avatar;
        let bottom = RING + avatar - 4.0;
        Rect::new(right - CAMERA, bottom - CAMERA, right, bottom)
    }

    fn on_camera(&self, x: f32, y: f32) -> bool {
        self.show_camera && Self::camera_rect(if self.size > 0.0 { self.size } else { 102.0 }).contains(x, y)
    }
}

impl Control for AvatarDecor {
    fn get_preferred_size(&self, _canvas: &dyn Canvas, _proposed: Size) -> Size {
        Size { width: 102.0, height: 102.0 }
    }

    fn accessible_parts(&self) -> Vec<AccessiblePart> {
        if !self.show_camera {
            return Vec::new();
        }
        vec![AccessiblePart { name: ShellControlsResources::account_change_photo().to_string(), role: AccessRole::Button, bounds: Self::camera_rect(self.size.max(102.0)) }]
    }

    fn on_paint(&mut self, e: &mut PaintEventCx<'_>) {
        let r = e.clip_rectangle;
        self.size = (r.right - r.left).min(r.bottom - r.top);
        let c: &dyn Canvas = e.graphics;
        let t = c.theme().clone();
        let s = self.size;
        // The ring, then the outline inside it (each 3 wide, centred on its own band).
        // The outline covers the ring (CSS paints `outline` over `box-shadow`, both 3 outside the
        // border): what shows is a 3 white band around the avatar.
        let mut white = t.accent;
        (white.r, white.g, white.b, white.a) = (1.0, 1.0, 1.0, 1.0);
        c.stroke_rounded_w(&Rect::new(r.left + 1.5, r.top + 1.5, r.left + s - 1.5, r.top + s - 1.5), pill(s - 3.0), &white, 3.0);
        if self.show_camera {
            let cam = Self::camera_rect(s);
            let cam = Rect::new(r.left + cam.left, r.top + cam.top, r.left + cam.right, r.top + cam.bottom);
            c.draw_card_shadow(&cam, pill(CAMERA));
            // `bg-white hover:bg-surface-1`, the theme's layer in the dark theme.
            let ground = if self.hot && !self.busy { t.row_hover } else { kubuno_desktop::Application::theme().layer_background };
            c.fill_rounded(&cam, pill(CAMERA), &kubuno_desktop::Application::theme().layer_background);
            if self.hot && !self.busy {
                c.fill_rounded(&cam, pill(CAMERA), &ground);
            }
            c.stroke_rounded(&cam, pill(CAMERA), &t.card_stroke);
            let mut ink = t.text_secondary;
            if self.busy {
                ink.a *= 0.5;
            }
            let glyph = Rect::new((cam.left + cam.right - GLYPH) / 2.0, (cam.top + cam.bottom - GLYPH) / 2.0, (cam.left + cam.right + GLYPH) / 2.0, (cam.top + cam.bottom + GLYPH) / 2.0);
            c.vector_icon("Camera", &glyph, GLYPH, &ink);
        }
        e.raise(self, "OnPaint");
    }

    fn on_mouse_move(&mut self, e: &mut EventCx<'_, MouseEventArgs>) {
        let hot = self.on_camera(e.args().x, e.args().y);
        if hot != self.hot {
            self.hot = hot;
            self.invalidate();
        }
        e.raise(&*self, "OnMouseMove");
    }

    fn on_mouse_down(&mut self, e: &mut EventCx<'_, MouseEventArgs>) {
        self.pressed = self.on_camera(e.args().x, e.args().y);
        e.raise(&*self, "OnMouseDown");
    }

    fn on_mouse_up(&mut self, e: &mut EventCx<'_, MouseEventArgs>) {
        if std::mem::take(&mut self.pressed) && self.on_camera(e.args().x, e.args().y) && !self.busy {
            self.raise_camera_clicked(EmptyEventArgs);
        }
        e.raise(&*self, "OnMouseUp");
    }

    fn on_mouse_leave(&mut self, e: &mut EventCx<'_, EmptyEventArgs>) {
        if std::mem::take(&mut self.hot) {
            self.invalidate();
        }
        e.raise(&*self, "OnMouseLeave");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_camera_sits_at_the_avatar_bottom_right() {
        let cam = AvatarDecor::camera_rect(102.0);
        // The avatar spans 3..99: the disc's right edge on it, its bottom 4 above it (`bottom-1`).
        assert_eq!((cam.right, cam.bottom, cam.right - cam.left), (99.0, 95.0, 32.0));
        let d = AvatarDecor { show_camera: true, size: 102.0, ..AvatarDecor::default() };
        assert!(d.on_camera(90.0, 90.0) && !d.on_camera(50.0, 50.0));
    }
}
