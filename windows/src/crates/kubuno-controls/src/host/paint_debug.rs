//! The paint debug overlay: what WPF's "show paint rectangles" / "show layout bounds" and a frame
//! counter show, drawn by the host over the page after each frame.
//!
//! - **Invalidated regions** flash (magenta, fading over 600 ms): the rectangles a page reports with
//!   [`note_invalidated`] — a control that invalidated itself, a custom control whose paint buffer
//!   was repainted.
//! - **Layout bounds** of every element the page reports with [`note_layout`] (cyan outline), its
//!   padding (green band inside) and margin (orange band outside).
//! - **Frame time**: the time the last frame's paint took and the frames per second, top right.
//!
//! Toggled by the environment variable `KUBUNO_PAINT_DEBUG` at start (`1`/`all`/`on`, or a comma
//! list of `invalidate`, `layout`, `fps`; `0`/`off` or unset: off), live by the registered window
//! message `"Kubuno.PaintDebug"` (`wParam` = [`PaintDebugFlags`] bits, what Visual Studio's
//! *Debug › Kubuno › Paint debug* posts to every Kubuno window), or from code with [`set_flags`].

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::sync::OnceLock;

use drive_app_controls::{Canvas, Rect};
use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;

use crate::enums::Padding;

/// Which parts of the overlay show (bits, as the window message carries them).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct PaintDebugFlags(pub u32);

impl PaintDebugFlags {
    pub const OFF: Self = Self(0);
    /// Flash the invalidated regions.
    pub const INVALIDATE: Self = Self(1);
    /// Outline the layout bounds, padding and margin.
    pub const LAYOUT: Self = Self(2);
    /// Show the frame time and frames per second.
    pub const FPS: Self = Self(4);
    pub const ALL: Self = Self(7);

    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0 && other.0 != 0
    }

    pub const fn is_off(self) -> bool {
        self.0 & 7 == 0
    }

    /// Reads `KUBUNO_PAINT_DEBUG`'s value (see the module doc); unknown words are ignored.
    pub fn parse(value: &str) -> Self {
        let mut f = 0;
        for word in value.split([',', ';', ' ', '|']).map(str::trim).filter(|w| !w.is_empty()) {
            match word.to_ascii_lowercase().as_str() {
                "1" | "on" | "all" | "true" | "yes" => f |= 7,
                "invalidate" | "invalidated" | "paint" | "flash" => f |= 1,
                "layout" | "bounds" => f |= 2,
                "fps" | "time" | "frame" => f |= 4,
                _ => {}
            }
        }
        Self(f)
    }
}

/// The name of the registered window message that toggles the overlay.
pub const MESSAGE_NAME: &str = "Kubuno.PaintDebug";

/// The id of [`MESSAGE_NAME`] (registered once per process).
pub fn message() -> u32 {
    static ID: OnceLock<u32> = OnceLock::new();
    *ID.get_or_init(|| {
        let wide: Vec<u16> = MESSAGE_NAME.encode_utf16().chain(std::iter::once(0)).collect();
        // SAFETY: a NUL-terminated wide string.
        unsafe { windows::Win32::UI::WindowsAndMessaging::RegisterWindowMessageW(windows::core::PCWSTR(wide.as_ptr())) }
    })
}

/// How long an invalidated region keeps flashing.
const FLASH_MS: u64 = 600;
/// How many frame times are kept for the counter.
const HISTORY: usize = 120;

struct LayoutNote {
    bounds: Rect,
    padding: Option<Padding>,
    margin: Option<Padding>,
}

#[derive(Default)]
struct State {
    flashes: Vec<(Rect, u64)>,
    layout: Vec<LayoutNote>,
    /// (frame start ms, paint duration µs), newest last.
    frames: VecDeque<(u64, u64)>,
}

thread_local! {
    static FLAGS: Cell<Option<PaintDebugFlags>> = const { Cell::new(None) };
    static STATE: RefCell<State> = RefCell::new(State::default());
}

/// The overlay's parts in force (the environment's at first).
pub fn flags() -> PaintDebugFlags {
    FLAGS.with(|f| match f.get() {
        Some(v) => v,
        None => {
            let v = std::env::var("KUBUNO_PAINT_DEBUG").map(|s| PaintDebugFlags::parse(&s)).unwrap_or(PaintDebugFlags::OFF);
            f.set(Some(v));
            v
        }
    })
}

/// Turns the overlay's parts on or off (the window repaints at its next frame).
pub fn set_flags(flags: PaintDebugFlags) {
    FLAGS.with(|f| f.set(Some(flags)));
    if flags.is_off() {
        STATE.with(|s| *s.borrow_mut() = State::default());
    }
}

/// Whether the layout bounds are shown (a page asks before collecting them).
pub fn layout_enabled() -> bool {
    flags().contains(PaintDebugFlags::LAYOUT)
}

/// Whether invalidated regions flash.
pub fn invalidate_enabled() -> bool {
    flags().contains(PaintDebugFlags::INVALIDATE)
}

/// Reports a region (client DIP) the page invalidated: it flashes.
pub fn note_invalidated(rect: Rect) {
    if !invalidate_enabled() {
        return;
    }
    let now = super::now_ms();
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        if s.flashes.len() < 512 {
            s.flashes.push((rect, now));
        }
    });
}

/// Reports an element's bounds (client DIP) with its padding and margin, for this frame.
pub fn note_layout(bounds: Rect, padding: Option<Padding>, margin: Option<Padding>) {
    if !layout_enabled() {
        return;
    }
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        if s.layout.len() < 4096 {
            s.layout.push(LayoutNote { bounds, padding, margin });
        }
    });
}

/// How many layout bounds were reported for the current frame (what the overlay will outline).
pub fn layout_note_count() -> usize {
    STATE.with(|s| s.borrow().layout.len())
}

/// A frame starts: the layout notes are the new frame's.
pub(crate) fn begin_frame() {
    STATE.with(|s| s.borrow_mut().layout.clear());
}

/// Records the paint time of a frame that started at `start_ms` (µs).
pub(crate) fn record_frame(start_ms: u64, paint_us: u64) {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.frames.push_back((start_ms, paint_us));
        while s.frames.len() > HISTORY {
            s.frames.pop_front();
        }
    });
}

/// The last frame's paint time (ms) and the frames painted during the last second.
pub fn frame_stats(now_ms: u64) -> (f32, usize) {
    STATE.with(|s| {
        let s = s.borrow();
        let last = s.frames.back().map_or(0.0, |(_, us)| *us as f32 / 1000.0);
        let fps = s.frames.iter().filter(|(t, _)| now_ms.saturating_sub(*t) < 1000).count();
        (last, fps)
    })
}

fn rgba(r: f32, g: f32, b: f32, a: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F { r, g, b, a }
}

/// Paints the overlay over a `size` client area; returns whether it animates (the host asks for
/// another frame soon).
pub(crate) fn paint(c: &dyn Canvas, size: (f32, f32), now: u64) -> bool {
    let flags = flags();
    if flags.is_off() {
        return false;
    }
    let mut animating = false;
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        if flags.contains(PaintDebugFlags::LAYOUT) {
            for n in &s.layout {
                let b = n.bounds;
                if let Some(p) = n.margin {
                    let outer = Rect::new(b.left - p.left, b.top - p.top, b.right + p.right, b.bottom + p.bottom);
                    band(c, outer, b, &rgba(1.0, 0.6, 0.0, 0.18));
                }
                if let Some(p) = n.padding {
                    let inner = Rect::new(b.left + p.left, b.top + p.top, (b.right - p.right).max(b.left + p.left), (b.bottom - p.bottom).max(b.top + p.top));
                    band(c, b, inner, &rgba(0.1, 0.8, 0.2, 0.2));
                }
                c.stroke_rounded(&b, 0.0, &rgba(0.0, 0.75, 0.9, 0.85));
            }
        }
        if flags.contains(PaintDebugFlags::INVALIDATE) {
            s.flashes.retain(|(_, t)| now.saturating_sub(*t) < FLASH_MS);
            for (r, t) in &s.flashes {
                let k = 1.0 - now.saturating_sub(*t) as f32 / FLASH_MS as f32;
                c.fill_rounded(r, 0.0, &rgba(1.0, 0.0, 0.8, 0.30 * k));
                c.stroke_rounded_w(r, 0.0, &rgba(1.0, 0.0, 0.8, 0.9 * k), 2.0);
            }
            animating |= !s.flashes.is_empty();
        }
    });
    if flags.contains(PaintDebugFlags::FPS) {
        let (ms, fps) = frame_stats(now);
        let text = format!("{ms:.1} ms · {fps} fps");
        let f = &c.formats().caption_strong;
        let w = c.measure(&text, f) + 16.0;
        let r = frame_box(size, w);
        c.fill_rounded(&r, 6.0, &rgba(0.1, 0.1, 0.12, 0.8));
        c.text(&text, &r, f, &rgba(1.0, 1.0, 1.0, 1.0), true);
        // The counter keeps counting only while frames come; it does not force any.
    }
    animating
}

/// Where the frame-time box goes in a `size` client area: bottom-right, `w` wide. The top-right corner belongs to the
/// caption buttons (minimize/maximize/close) that Kubuno apps draw inside the client area, so the box never goes there.
fn frame_box(size: (f32, f32), w: f32) -> Rect {
    Rect::new(size.0 - w - 8.0, size.1 - 30.0, size.0 - 8.0, size.1 - 8.0)
}

/// Fills the band between `outer` and `inner` (four strips).
fn band(c: &dyn Canvas, outer: Rect, inner: Rect, color: &D2D1_COLOR_F) {
    let strips = [
        Rect::new(outer.left, outer.top, outer.right, inner.top),
        Rect::new(outer.left, inner.bottom, outer.right, outer.bottom),
        Rect::new(outer.left, inner.top, inner.left, inner.bottom),
        Rect::new(inner.right, inner.top, outer.right, inner.bottom),
    ];
    for s in strips {
        if s.right > s.left && s.bottom > s.top {
            c.fill_rounded(&s, 0.0, color);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_frame_box_stays_clear_of_the_caption_buttons() {
        let r = frame_box((800.0, 600.0), 100.0);
        assert!(r.top > 600.0 / 2.0, "bottom half, far below any caption bar");
        assert!(r.right <= 800.0 && r.bottom <= 600.0 && r.left >= 0.0);
    }

    #[test]
    fn the_environment_value_is_parsed_leniently() {
        assert_eq!(PaintDebugFlags::parse("1"), PaintDebugFlags::ALL);
        assert_eq!(PaintDebugFlags::parse("all"), PaintDebugFlags::ALL);
        assert_eq!(PaintDebugFlags::parse("layout, FPS"), PaintDebugFlags(6));
        assert_eq!(PaintDebugFlags::parse("invalidate"), PaintDebugFlags::INVALIDATE);
        assert!(PaintDebugFlags::parse("0").is_off() && PaintDebugFlags::parse("off").is_off() && PaintDebugFlags::parse("").is_off());
        assert!(!PaintDebugFlags::OFF.contains(PaintDebugFlags::OFF));
    }

    #[test]
    fn notes_are_kept_only_while_their_part_is_on() {
        set_flags(PaintDebugFlags::OFF);
        note_invalidated(Rect::new(0.0, 0.0, 1.0, 1.0));
        note_layout(Rect::new(0.0, 0.0, 1.0, 1.0), None, None);
        STATE.with(|s| assert!(s.borrow().flashes.is_empty() && s.borrow().layout.is_empty()));
        set_flags(PaintDebugFlags::ALL);
        note_invalidated(Rect::new(0.0, 0.0, 1.0, 1.0));
        note_layout(Rect::new(0.0, 0.0, 1.0, 1.0), Some(Padding::all(2.0)), None);
        STATE.with(|s| assert_eq!((s.borrow().flashes.len(), s.borrow().layout.len()), (1, 1)));
        begin_frame();
        STATE.with(|s| assert!(s.borrow().layout.is_empty(), "layout notes are per frame"));
        record_frame(1000, 16_000);
        record_frame(1500, 8_000);
        assert_eq!(frame_stats(1600), (8.0, 2));
        assert_eq!(frame_stats(2400).1, 1, "only the last second counts");
        set_flags(PaintDebugFlags::OFF);
        STATE.with(|s| assert!(s.borrow().flashes.is_empty(), "turning it off forgets"));
    }
}
