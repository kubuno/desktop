//! Surfaces without a window: the theme and text formats a canvas-less [`super::Graphics`]
//! answers with, and [`RecordingCanvas`], a canvas that records the primitives it is asked to
//! draw — for the tests of anything that paints (widgets, owner-draw handlers, custom controls).

use std::cell::RefCell;

use kubuno_drive_desktop_app_controls::{create_text_formats_styled, Canvas, Rect, TextFormats, TextStyle, Theme};
use kubuno_desktop_controls::{ControlCanvas, Visuals};
use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;
use windows::Win32::Graphics::Direct2D::ID2D1Bitmap1;
use windows::Win32::Graphics::DirectWrite::{DWriteCreateFactory, IDWriteFactory, IDWriteTextFormat, DWRITE_FACTORY_TYPE_SHARED, DWRITE_TEXT_ALIGNMENT};

struct Headless {
    theme: Theme,
    formats: TextFormats,
    visuals: Option<Visuals>,
}

fn build() -> Headless {
    // SAFETY: creating the shared DirectWrite factory has no precondition.
    let dwrite: Option<IDWriteFactory> = unsafe { DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED).ok() };
    let dwrite = dwrite.expect("DirectWrite is part of every supported Windows");
    let formats = create_text_formats_styled(&dwrite, None, &TextStyle::default()).expect("the default text formats can be created");
    let visuals = Visuals::read(&dwrite, 96.0).ok();
    Headless { theme: Theme::light(), formats, visuals }
}

thread_local! {
    /// Built once per thread and kept for the thread's life (tests, headless graphics).
    static HEADLESS: &'static Headless = Box::leak(Box::new(build()));
}

fn headless() -> &'static Headless {
    HEADLESS.with(|h| *h)
}

/// The light theme, for a surface-less `Graphics`.
pub fn headless_theme() -> &'static Theme {
    &headless().theme
}

/// The default text formats, for a surface-less `Graphics`.
pub fn headless_formats() -> &'static TextFormats {
    &headless().formats
}

/// A canvas that draws nothing and records every primitive it is asked for, as text
/// (`fill_rounded(0,0,10,10 r=2)`), with the light theme and real text formats (so measuring works).
#[derive(Default)]
pub struct RecordingCanvas {
    calls: RefCell<Vec<String>>,
}

impl RecordingCanvas {
    pub fn new() -> Self {
        Self::default()
    }

    /// The calls recorded so far.
    pub fn calls(&self) -> Vec<String> {
        self.calls.borrow().clone()
    }

    /// Forgets the calls recorded so far.
    pub fn clear(&self) {
        self.calls.borrow_mut().clear();
    }

    /// How many recorded calls start with `prefix`.
    pub fn count(&self, prefix: &str) -> usize {
        self.calls.borrow().iter().filter(|c| c.starts_with(prefix)).count()
    }

    fn log(&self, s: String) {
        self.calls.borrow_mut().push(s);
    }
}

fn r(rect: &Rect) -> String {
    format!("{},{},{},{}", rect.left, rect.top, rect.right, rect.bottom)
}

impl Canvas for RecordingCanvas {
    fn theme(&self) -> &Theme {
        headless_theme()
    }
    fn formats(&self) -> &TextFormats {
        headless_formats()
    }
    fn scale(&self) -> f32 {
        1.0
    }
    fn fill_rounded(&self, rect: &Rect, radius: f32, _color: &D2D1_COLOR_F) {
        self.log(format!("fill_rounded({} r={radius})", r(rect)));
    }
    fn fill_top_rounded(&self, rect: &Rect, radius: f32, _color: &D2D1_COLOR_F) {
        self.log(format!("fill_top_rounded({} r={radius})", r(rect)));
    }
    fn fill_triangle(&self, _a: (f32, f32), _b: (f32, f32), _c: (f32, f32), _color: &D2D1_COLOR_F) {
        self.log("fill_triangle".to_string());
    }
    fn stroke_arc(&self, centre: (f32, f32), radius: f32, _start: f32, _sweep: f32, width: f32, _color: &D2D1_COLOR_F) {
        self.log(format!("stroke_arc({},{} r={radius} w={width})", centre.0, centre.1));
    }
    fn stroke_rounded(&self, rect: &Rect, radius: f32, _color: &D2D1_COLOR_F) {
        self.log(format!("stroke_rounded({} r={radius})", r(rect)));
    }
    fn stroke_rounded_w(&self, rect: &Rect, radius: f32, _color: &D2D1_COLOR_F, width: f32) {
        self.log(format!("stroke_rounded_w({} r={radius} w={width})", r(rect)));
    }
    fn text(&self, text: &str, rect: &Rect, _f: &IDWriteTextFormat, _c: &D2D1_COLOR_F, _centered: bool) {
        self.log(format!("text({text:?} {})", r(rect)));
    }
    fn text_aligned(&self, text: &str, rect: &Rect, _f: &IDWriteTextFormat, _c: &D2D1_COLOR_F, _a: DWRITE_TEXT_ALIGNMENT) {
        self.log(format!("text({text:?} {})", r(rect)));
    }
    fn text_ellipsis(&self, text: &str, rect: &Rect, _f: &IDWriteTextFormat, _c: &D2D1_COLOR_F) {
        self.log(format!("text({text:?} {})", r(rect)));
    }
    fn text_ellipsis_center(&self, text: &str, rect: &Rect, _f: &IDWriteTextFormat, _c: &D2D1_COLOR_F) {
        self.log(format!("text({text:?} {})", r(rect)));
    }
    fn image(&self, _b: &ID2D1Bitmap1, rect: &Rect, _size: f32) {
        self.log(format!("image({})", r(rect)));
    }
    fn image_alpha(&self, _b: &ID2D1Bitmap1, rect: &Rect, _size: f32, _alpha: f32) {
        self.log(format!("image({})", r(rect)));
    }
    fn vector_icon(&self, name: &'static str, rect: &Rect, _size: f32, _color: &D2D1_COLOR_F) {
        self.log(format!("icon({name} {})", r(rect)));
    }
    fn vector_icon_layered(&self, name: &'static str, rect: &Rect, _size: f32, _fg: &D2D1_COLOR_F, _accent: &D2D1_COLOR_F) {
        self.log(format!("icon({name} {})", r(rect)));
    }
    fn measure(&self, text: &str, format: &IDWriteTextFormat) -> f32 {
        // SAFETY: a plain COM getter on a live format.
        let size = unsafe { format.GetFontSize() };
        text.chars().count() as f32 * size * 0.55
    }
    fn draw_card_shadow(&self, rect: &Rect, _radius: f32) {
        self.log(format!("shadow({})", r(rect)));
    }
    fn draw_shadow(&self, rect: &Rect, _radius: f32, _layers: &[kubuno_drive_desktop_app_controls::themes::shape::ShadowLayer], _colour: (f32, f32, f32)) {
        self.log(format!("shadow({})", r(rect)));
    }
    fn erase_rounded(&self, rect: &Rect, _radius: f32) {
        self.log(format!("erase({})", r(rect)));
    }
    fn push_clip(&self, rect: &Rect) {
        self.log(format!("push_clip({})", r(rect)));
    }
    fn push_clip_rounded(&self, rect: &Rect, _radius: f32) {
        self.log(format!("push_clip({})", r(rect)));
    }
    fn pop_clip_rounded(&self) {
        self.log("pop_clip".to_string());
    }
    fn pop_clip(&self) {
        self.log("pop_clip".to_string());
    }
}

impl ControlCanvas for RecordingCanvas {
    fn visuals(&self) -> &Visuals {
        headless().visuals.as_ref().expect("the system visuals can be read")
    }
}
