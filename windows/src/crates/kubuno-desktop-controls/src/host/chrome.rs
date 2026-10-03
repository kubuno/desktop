//! The host's side of the Kubuno chrome ([`crate::window_chrome`]) that a page talks to: where the
//! title band's regions are this frame (so the page can put its own controls in the band), the
//! areas of the band that do or do not drag the window, and the window events the chrome raises
//! (a double-click on the band, a caption button of the window's own, the help button, the start
//! and end of a move or resize, a DPI change).
//!
//! All of it is per window (swapped by `super::window_tls` with the rest of the frame state).

use std::cell::RefCell;

use kubuno_drive_desktop_app_controls::Rect;

use crate::window_chrome::{self, ChromeLayout, ChromeStyle, Part, SlotWidths, SystemButtons};

/// Something the window's chrome did, for the page to raise as the `Form` event of the same name.
#[derive(Debug, Clone, PartialEq)]
pub enum WindowEvent {
    /// The band was double-clicked (the window maximises or restores unless it cannot).
    TitleBarDoubleClick,
    /// One of the window's own caption buttons ([`ChromeStyle::commands`]) was clicked: its id.
    CaptionButtonClick(String),
    /// The help button (`HelpButton`) was clicked.
    HelpButtonClicked,
    /// The user started moving or resizing the window (`ResizeBegin`).
    ResizeBegin,
    /// …and stopped (`ResizeEnd`).
    ResizeEnd,
    /// The window moved to a display of another scale (`DpiChanged`), in DPI.
    DpiChanged { old: u32, new: u32 },
    /// Another window drawn inside this one became active, or one closed (`MdiChildActivate`;
    /// raised by the `kubuno-desktop` facade, which draws MDI documents).
    MdiChildActivate,
}

/// What the host knows about the band this frame (set before the page paints).
#[derive(Clone)]
pub(crate) struct ChromeContext {
    pub style: ChromeStyle,
    pub bounds: Rect,
    pub has_icon: bool,
    pub buttons: SystemButtons,
}

/// The per-window chrome state.
#[derive(Default)]
pub(crate) struct ChromeTls {
    context: Option<ChromeContext>,
    slots: SlotWidths,
    events: Vec<WindowEvent>,
    /// The areas the page declared as dragging the window this frame (`TitleBar.Drag`).
    drag: Vec<Rect>,
}

thread_local! {
    static STATE: RefCell<ChromeTls> = RefCell::new(ChromeTls::default());
}

pub(super) fn take_window_state() -> ChromeTls {
    STATE.with(|s| s.try_borrow_mut().map(|mut v| std::mem::take(&mut *v)).unwrap_or_default())
}

pub(super) fn put_window_state(state: ChromeTls) {
    STATE.with(|s| {
        if let Ok(mut slot) = s.try_borrow_mut() {
            *slot = state;
        }
    });
}

/// The host, before the page paints: the band of this frame (`None`: no Kubuno band).
pub(super) fn begin_frame(context: Option<ChromeContext>) {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.context = context;
        s.slots = SlotWidths::default();
        s.drag.clear();
    });
}

/// The slot widths the page declared this frame.
pub(super) fn declared_slots() -> SlotWidths {
    STATE.with(|s| s.borrow().slots)
}

/// Declares an area that drags the window this frame.
pub(super) fn push_drag(rect: Rect) {
    STATE.with(|s| s.borrow_mut().drag.push(rect));
}

/// The drag areas declared this frame.
pub(super) fn drag_areas() -> Vec<Rect> {
    STATE.with(|s| s.borrow().drag.clone())
}

/// Queues a window event for the next frame.
pub(super) fn push_event(event: WindowEvent) {
    STATE.with(|s| s.borrow_mut().events.push(event));
}

/// The window events since the last frame (taken: each is delivered once).
pub fn take_window_events() -> Vec<WindowEvent> {
    STATE.with(|s| s.try_borrow_mut().map(|mut s| std::mem::take(&mut s.events)).unwrap_or_default())
}

/// The title band of this window this frame, with its three regions sized for `slots` (the width
/// of the page's controls in each), in client DIP — `None` when the window has no Kubuno band
/// (`Chrome::System`/`Custom`, `FormBorderStyle::None`). Calling it declares the widths: the host
/// writes the title between the regions and hit-tests the band accordingly. A page placing
/// interactive controls in the band also declares them with [`super::add_title_bar_hole`], or
/// they would drag the window.
pub fn title_bar_layout(slots: SlotWidths) -> Option<ChromeLayout> {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.slots = slots;
        let c = s.context.clone()?;
        Some(window_chrome::layout(&c.style, c.bounds, c.has_icon, c.buttons, slots))
    })
}

/// The band's height this frame, `0` without a Kubuno band — also when the page extends under it.
pub fn title_bar_height() -> f32 {
    STATE.with(|s| s.borrow().context.as_ref().map_or(0.0, |c| c.style.band_height()))
}

/// The part a caption hit-test code stands for.
pub(super) fn part_of_ht(ht: u32, layout: &ChromeLayout, x: f32, y: f32) -> Option<Part> {
    use windows::Win32::UI::WindowsAndMessaging::*;
    match ht {
        HTMINBUTTON => Some(Part::Minimize),
        HTMAXBUTTON => Some(Part::Maximize),
        HTCLOSE => Some(Part::Close),
        HTHELP => Some(Part::Help),
        HTOBJECT => layout.hit(x, y).filter(|p| matches!(p, Part::Command(_))),
        _ => None,
    }
}

/// The Win32 `HT*` code of a part.
pub(super) fn ht_of(part: Part) -> u32 {
    use windows::Win32::UI::WindowsAndMessaging::*;
    match part {
        Part::Minimize => HTMINBUTTON,
        Part::Maximize => HTMAXBUTTON,
        Part::Close => HTCLOSE,
        Part::Help => HTHELP,
        Part::Command(_) => HTOBJECT,
    }
}

/// `WM_NCHITTEST` for the Kubuno chrome, as a pure function (client DIP): the caption buttons,
/// then the resize borders (`border` is `None` when the window cannot be resized or is maximised),
/// then the grip, then the page's own drag areas (`TitleBar.Drag`), then its interactive controls
/// in the band (holes), then the icon (the window menu) and the band (`HTCAPTION`).
#[allow(clippy::too_many_arguments)] // One pure function of everything a hit test reads, for the tests.
pub fn hit_test(layout: Option<&ChromeLayout>, bar: &super::TitleBar, drag: &[Rect], size: (f32, f32), x: f32, y: f32, border: Option<f32>, grip: Option<Rect>) -> u32 {
    use windows::Win32::UI::WindowsAndMessaging::*;
    if let Some(part) = layout.and_then(|l| l.hit(x, y)) {
        return ht_of(part);
    }
    if let Some(edge) = super::caption::resize_edge(size, x, y, border) {
        return edge;
    }
    if grip.is_some_and(|g| g.contains(x, y)) {
        return HTBOTTOMRIGHT;
    }
    if drag.iter().any(|r| r.contains(x, y)) {
        return HTCAPTION;
    }
    if bar.no_drag.iter().any(|r| r.contains(x, y)) {
        return HTCLIENT;
    }
    if let Some(l) = layout {
        if l.icon.is_some_and(|r| r.contains(x, y)) {
            return HTSYSMENU;
        }
        if l.in_band(x, y) {
            return HTCAPTION;
        }
    }
    HTCLIENT
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::TitleBar;
    use windows::Win32::UI::WindowsAndMessaging::*;

    #[test]
    fn hit_test_order() {
        let style = ChromeStyle { help_button: true, commands: vec![crate::window_chrome::CaptionCommand::new("pin", "Pin")], ..ChromeStyle::default() };
        let bounds = Rect::new(0.0, 0.0, 800.0, 600.0);
        let l = window_chrome::layout(&style, bounds, true, SystemButtons::default(), SlotWidths { left: 0.0, center: 0.0, right: 100.0 });
        let bar = TitleBar { no_drag: vec![l.right], ..TitleBar::default() };
        let drag = [Rect::new(300.0, 50.0, 400.0, 80.0)];
        let b = Some(6.0);
        let close = l.rect_of(Part::Close).expect("close");
        assert_eq!(hit_test(Some(&l), &bar, &drag, (800.0, 600.0), close.left + 5.0, 22.0, b, None), HTCLOSE);
        let help = l.rect_of(Part::Help).expect("help");
        assert_eq!(hit_test(Some(&l), &bar, &drag, (800.0, 600.0), help.left + 5.0, 22.0, b, None), HTHELP);
        let cmd = l.rect_of(Part::Command(0)).expect("cmd");
        assert_eq!(hit_test(Some(&l), &bar, &drag, (800.0, 600.0), cmd.left + 5.0, 22.0, b, None), HTOBJECT);
        assert_eq!(part_of_ht(HTOBJECT, &l, cmd.left + 5.0, 22.0), Some(Part::Command(0)));
        assert_eq!(hit_test(Some(&l), &bar, &drag, (800.0, 600.0), l.right.left + 5.0, 22.0, b, None), HTCLIENT);
        assert_eq!(hit_test(Some(&l), &bar, &drag, (800.0, 600.0), 200.0, 22.0, b, None), HTCAPTION);
        assert_eq!(hit_test(Some(&l), &bar, &drag, (800.0, 600.0), 20.0, 22.0, b, None), HTSYSMENU);
        assert_eq!(hit_test(Some(&l), &bar, &drag, (800.0, 600.0), 350.0, 60.0, b, None), HTCAPTION, "a page drag area");
        assert_eq!(hit_test(Some(&l), &bar, &drag, (800.0, 600.0), 400.0, 2.0, b, None), HTTOP);
        assert_eq!(hit_test(Some(&l), &bar, &drag, (800.0, 600.0), 400.0, 300.0, b, None), HTCLIENT);
        let grip = window_chrome::grip_rect(Rect::new(0.0, 0.0, 800.0, 600.0));
        assert_eq!(hit_test(Some(&l), &bar, &drag, (800.0, 600.0), 790.0, 590.0, None, Some(grip)), HTBOTTOMRIGHT);
        // Borderless: only the page's drag areas drag.
        assert_eq!(hit_test(None, &bar, &drag, (800.0, 600.0), 350.0, 60.0, None, None), HTCAPTION);
        assert_eq!(hit_test(None, &bar, &drag, (800.0, 600.0), 200.0, 22.0, None, None), HTCLIENT);
    }
}
