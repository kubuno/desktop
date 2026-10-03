//! Caption geometry shared by the chromes the host strips the system caption for: the resize
//! edges, the title bar a page declares under `Chrome::Custom` ([`TitleBar`], [`custom_hit_test`]),
//! and the caption buttons a `Form` shows. The Kubuno band itself is laid out and painted by
//! [`crate::window_chrome`] (hit-tested by `super::chrome::hit_test`).

use drive_app_controls::Rect;

/// Which caption slot the pointer is over, if any. Emitted by
/// [`TitleBar::button_at`] and consumed by the WndProc.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hot {
    Min,
    Max,
    Close,
}

impl Hot {
    /// The Win32 `HT*` code this slot maps to for `WM_NCHITTEST`. The system
    /// buttons keep their native codes so Windows 11's snap-layouts flyout
    /// pops on hover of the maximise button — a wholly custom code would kill
    /// that affordance.
    pub const fn ht_code(self) -> u32 {
        use windows::Win32::UI::WindowsAndMessaging::*;
        match self {
            Self::Min => HTMINBUTTON,
            Self::Max => HTMAXBUTTON,
            Self::Close => HTCLOSE,
        }
    }
}

use super::form::CaptionButtonState;

thread_local! {
    /// Which caption buttons show (`Form.ControlBox`/`MinimizeBox`/`MaximizeBox`, see
    /// [`super::form::caption_buttons`]): `(min, max, close)`.
    static BUTTONS: std::cell::Cell<(CaptionButtonState, CaptionButtonState, bool)> =
        const { std::cell::Cell::new((CaptionButtonState::Shown, CaptionButtonState::Shown, true)) };
}

/// Sets which caption buttons show (the host does, from the window's `Form` properties).
pub fn set_buttons(buttons: (CaptionButtonState, CaptionButtonState, bool)) {
    BUTTONS.with(|b| b.set(buttons));
}

/// The per-window caption buttons, for `super::window_tls` (taken: the defaults stay).
pub(crate) fn take_window_state() -> (CaptionButtonState, CaptionButtonState, bool) {
    BUTTONS.with(|b| b.replace((CaptionButtonState::Shown, CaptionButtonState::Shown, true)))
}

// ── Shared frame geometry (Kubuno and Custom chrome) ─────────────────────

/// The resize edge or corner `(x, y)` lies on, as a Win32 `HT*` code, or
/// `None` when it is inside the window.
///
/// `border` is the resize band's thickness in DIP. `WM_NCCALCSIZE` keeps the
/// left/right/bottom borders in the non-client area, so in client coordinates
/// they fall just outside `[0, size]`; the same `< border` / `> size - border`
/// test therefore catches both the true border and a few forgiving pixels
/// inside. The caller passes `None` while the window is maximised: a
/// maximised window has no resize border.
pub fn resize_edge(size: (f32, f32), x: f32, y: f32, border: Option<f32>) -> Option<u32> {
    use windows::Win32::UI::WindowsAndMessaging::*;
    let border = border?;
    let left = x < border;
    let right = x > size.0 - border;
    let top = y < border;
    let bottom = y > size.1 - border;
    match (top, bottom, left, right) {
        (true, _, true, _) => Some(HTTOPLEFT),
        (true, _, _, true) => Some(HTTOPRIGHT),
        (_, true, true, _) => Some(HTBOTTOMLEFT),
        (_, true, _, true) => Some(HTBOTTOMRIGHT),
        (true, _, _, _) => Some(HTTOP),
        (_, true, _, _) => Some(HTBOTTOM),
        (_, _, true, _) => Some(HTLEFT),
        (_, _, _, true) => Some(HTRIGHT),
        _ => None,
    }
}

/// The title bar a page paints itself under `Chrome::Custom`, declared to the
/// host every frame through [`super::set_title_bar`] so `WM_NCHITTEST` can
/// route the pointer the way the page drew it. Everything is in client DIP.
#[derive(Clone, Default)]
pub struct TitleBar {
    /// The drag band's height from the top of the client area. `0` = no
    /// caption at all (nothing drags the window).
    pub height:  f32,
    /// The page's minimise button, reported as `HTMINBUTTON`.
    pub min:     Option<Rect>,
    /// The page's maximise/restore button, reported as `HTMAXBUTTON` so
    /// Windows 11 offers its snap-layouts flyout on hover.
    pub max:     Option<Rect>,
    /// The page's close button, reported as `HTCLOSE`.
    pub close:   Option<Rect>,
    /// Interactive controls inside the band (a search field, a menu button,
    /// an avatar): they stay client area instead of dragging the window.
    pub no_drag: Vec<Rect>,
}

impl TitleBar {
    /// Which declared caption button `(x, y)` is over, if any.
    pub fn button_at(&self, x: f32, y: f32) -> Option<Hot> {
        let inside = |r: &Option<Rect>| r.as_ref().is_some_and(|r| r.contains(x, y));
        if inside(&self.close) {
            Some(Hot::Close)
        } else if inside(&self.max) {
            Some(Hot::Max)
        } else if inside(&self.min) {
            Some(Hot::Min)
        } else {
            None
        }
    }
}

/// `WM_NCHITTEST` for `Chrome::Custom`, as a pure function of the declared
/// [`TitleBar`], the client size and the point (all client DIP).
///
/// Order: the resize borders and corners first (`border` is `None` while
/// maximised), then the declared caption buttons, then the drag band minus
/// its `no_drag` holes (`HTCAPTION`), and everything else is `HTCLIENT`.
pub fn custom_hit_test(bar: &TitleBar, size: (f32, f32), x: f32, y: f32, border: Option<f32>) -> u32 {
    use windows::Win32::UI::WindowsAndMessaging::{HTCAPTION, HTCLIENT};
    if let Some(edge) = resize_edge(size, x, y, border) {
        return edge;
    }
    if let Some(button) = bar.button_at(x, y) {
        return button.ht_code();
    }
    if y >= 0.0 && y < bar.height && !bar.no_drag.iter().any(|r| r.contains(x, y)) {
        return HTCAPTION;
    }
    HTCLIENT
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::UI::WindowsAndMessaging::*;

    const SIZE: (f32, f32) = (800.0, 600.0);

    fn bar() -> TitleBar {
        TitleBar {
            height: 48.0,
            min: Some(Rect::new(662.0, 0.0, 708.0, 48.0)),
            max: Some(Rect::new(708.0, 0.0, 754.0, 48.0)),
            close: Some(Rect::new(754.0, 0.0, 800.0, 48.0)),
            no_drag: vec![Rect::new(100.0, 8.0, 300.0, 40.0)],
        }
    }

    #[test]
    fn resize_edges_and_corners() {
        let b = Some(6.0);
        assert_eq!(resize_edge(SIZE, 2.0, 2.0, b), Some(HTTOPLEFT));
        assert_eq!(resize_edge(SIZE, 798.0, 2.0, b), Some(HTTOPRIGHT));
        assert_eq!(resize_edge(SIZE, 2.0, 598.0, b), Some(HTBOTTOMLEFT));
        assert_eq!(resize_edge(SIZE, 798.0, 598.0, b), Some(HTBOTTOMRIGHT));
        assert_eq!(resize_edge(SIZE, 400.0, 2.0, b), Some(HTTOP));
        assert_eq!(resize_edge(SIZE, 400.0, 598.0, b), Some(HTBOTTOM));
        assert_eq!(resize_edge(SIZE, 2.0, 300.0, b), Some(HTLEFT));
        assert_eq!(resize_edge(SIZE, 798.0, 300.0, b), Some(HTRIGHT));
        // Outside the client area (the real non-client border) still resizes.
        assert_eq!(resize_edge(SIZE, -3.0, 300.0, b), Some(HTLEFT));
        assert_eq!(resize_edge(SIZE, 400.0, 300.0, b), None);
        // Maximised: no border at all.
        assert_eq!(resize_edge(SIZE, 2.0, 2.0, None), None);
    }

    #[test]
    fn custom_borders_win_over_buttons() {
        // The top-right corner resizes even though the close button is there.
        assert_eq!(custom_hit_test(&bar(), SIZE, 798.0, 2.0, Some(6.0)), HTTOPRIGHT);
        // Maximised, the same point is the close button.
        assert_eq!(custom_hit_test(&bar(), SIZE, 798.0, 2.0, None), HTCLOSE);
    }

    #[test]
    fn custom_buttons_map_to_system_codes() {
        let b = Some(6.0);
        assert_eq!(custom_hit_test(&bar(), SIZE, 680.0, 24.0, b), HTMINBUTTON);
        assert_eq!(custom_hit_test(&bar(), SIZE, 730.0, 24.0, b), HTMAXBUTTON);
        assert_eq!(custom_hit_test(&bar(), SIZE, 770.0, 24.0, b), HTCLOSE);
        assert_eq!(bar().button_at(730.0, 24.0), Some(Hot::Max));
        assert_eq!(bar().button_at(500.0, 24.0), None);
    }

    #[test]
    fn custom_drag_band_and_holes() {
        let b = Some(6.0);
        assert_eq!(custom_hit_test(&bar(), SIZE, 50.0, 24.0, b), HTCAPTION);
        assert_eq!(custom_hit_test(&bar(), SIZE, 500.0, 24.0, b), HTCAPTION);
        // A no-drag hole (a search field) stays client area.
        assert_eq!(custom_hit_test(&bar(), SIZE, 200.0, 24.0, b), HTCLIENT);
        // Below the band: the page.
        assert_eq!(custom_hit_test(&bar(), SIZE, 400.0, 48.0, b), HTCLIENT);
        assert_eq!(custom_hit_test(&bar(), SIZE, 400.0, 300.0, b), HTCLIENT);
    }

    #[test]
    fn custom_without_declaration_is_all_client() {
        let empty = TitleBar::default();
        assert_eq!(custom_hit_test(&empty, SIZE, 400.0, 10.0, Some(6.0)), HTCLIENT);
        assert_eq!(custom_hit_test(&empty, SIZE, 400.0, 2.0, Some(6.0)), HTTOP);
    }

}
