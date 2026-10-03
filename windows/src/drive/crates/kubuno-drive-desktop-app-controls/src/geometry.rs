//! Rectangle in DIP — the geometric primitive shared by every control
//! (counterpart of the `Rect`/`Point` types from WinUI in `Files.App.Controls`).

use windows::Win32::Graphics::Direct2D::Common::D2D_RECT_F;

#[derive(Default, Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

impl Rect {
    pub fn new(left: f32, top: f32, right: f32, bottom: f32) -> Self {
        Self { left, top, right, bottom }
    }

    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.left && x < self.right && y >= self.top && y < self.bottom
    }

    /// Same rect, moved horizontally (tab reorder animation).
    pub fn shift_x(&self, dx: f32) -> Self {
        Self::new(self.left + dx, self.top, self.right + dx, self.bottom)
    }

    pub fn d2d(&self) -> D2D_RECT_F {
        D2D_RECT_F {
            left: self.left,
            top: self.top,
            right: self.right,
            bottom: self.bottom,
        }
    }

    pub fn inflate(&self, dx: f32, dy: f32) -> Rect {
        Rect::new(self.left - dx, self.top - dy, self.right + dx, self.bottom + dy)
    }
}
