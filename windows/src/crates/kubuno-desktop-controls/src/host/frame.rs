//! The corners of a top-level window (`FormOptions::corner_radius`, the views' `CornerRadius` and
//! `CornerPreference`): who rounds them, and the window the host draws itself when it is not DWM.
//!
//! **Two paths** ([`plan`]):
//!
//! * **DWM** — on Windows 11 (build 22000+), a radius equal to one of DWM's presets (8 DIP:
//!   `DWMWCP_ROUND`, 4 DIP: `DWMWCP_ROUNDSMALL`) or 0 (`DWMWCP_DONOTROUND`) is handed to DWM
//!   (`DWMWA_WINDOW_CORNER_PREFERENCE`): its own shadow, its own 1 px border, the system materials
//!   (Mica, Acrylic) clipped to the curve, and DWM squares a maximised or snapped window itself.
//!   This is the default window's path (8 DIP), and the fastest.
//! * **Host** — any other radius, and every radius on Windows 10 (where DWM rounds nothing), is
//!   drawn by the host: the window is created `WS_EX_NOREDIRECTIONBITMAP` (its pixels come from its
//!   DirectComposition swap chain only, transparent where nothing is painted), DWM's non-client
//!   rendering is turned off (no square shadow or border behind the curve), and the window is the
//!   visible frame plus [`SHADOW_MARGIN`] on every side, where the host paints the window's soft
//!   shadow ([`FRAME_SHADOW`]). The frame's ground is filled and the page clipped to the rounded
//!   rectangle, and the 1 px border follows the curve. The same technique as a floating panel's
//!   (`super::backdrop`, the shell's flyouts), without the blur.
//!
//! **Square when Windows squares.** A maximised, snapped (`IsWindowArranged`) or full-screen
//! window has square corners and no shadow margin on either path ([`squared`]): DWM does it on its
//! path, the host on its own (the margin goes, so the frame fills the snapped area exactly).
//!
//! **What the host path cannot do**, and where the corners then come from (the nearest DWM preset
//! on Windows 11, square on Windows 10 — said in the docs, never silent):
//!
//! * a system material (Mica, Mica Alt, Acrylic) where the system draws one ([`system_backdrops`],
//!   Windows 11 22H2+): only DWM can clip its material to a curve;
//! * Windows' own title bar (`Chrome::System`): DWM draws that frame;
//! * a translucent window (`Opacity` below 100 at creation, `TransparencyKey`): a layered window;
//! * an embedded window (the designer's surface): its parent draws the frame.
//!
//! **Hit testing.** On the host path the shadow margin is the resize band, as Windows' invisible
//! borders are (a window region rounds the window's own corners off, so the clicks there go
//! through to what is behind); the band follows the curve ([`shape_hit`]), nothing outside the
//! curve belongs to the caption buttons, and a window that cannot be resized lets nothing of its
//! margin act.
//!
//! Two switches, for testing (`KUBUNO_*` environment variables, read once): `KUBUNO_CUSTOM_CORNERS=1`
//! forces the host path for every radius (what Windows 10 gets, reproduced on Windows 11), and
//! `KUBUNO_CORNER_RADIUS=<dip>` overrides the radius of every top-level window of the process.

use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::UI::WindowsAndMessaging::*;

/// The room around a host-rounded window's frame for its shadow, in DIP — the width of Windows'
/// own resize border (`SM_CXSIZEFRAME + SM_CXPADDEDBORDER`, 8 DIP), which takes the clicks there
/// as Windows' invisible borders do. Not wider: a window region cuts what a DirectComposition
/// window draws (measured: a 16 DIP shadow came out cut at the region's edge), so everything the
/// window paints must lie inside the area that takes the clicks, and the shadow is shaped for it
/// ([`FRAME_SHADOW`]).
pub const SHADOW_MARGIN: f32 = 8.0;

/// The shadow of a host-rounded window: the web window's (`--kb-shadow-window`, `0 6px 18px` at
/// 24 %) tightened to fit [`SHADOW_MARGIN`] — `blur / 2 + dy` = 8 DIP below the frame, 4 above —
/// plus a contact shadow along the edge, close to Windows 11's own.
pub const FRAME_SHADOW: [kubuno_drive_desktop_app_controls::themes::shape::ShadowLayer; 2] = [
    kubuno_drive_desktop_app_controls::themes::shape::ShadowLayer { dy: 2.0, blur: 12.0, spread: 0.0, opacity: 0.22 },
    kubuno_drive_desktop_app_controls::themes::shape::ShadowLayer { dy: 0.5, blur: 2.0, spread: 0.0, opacity: 0.12 },
];

/// Two radii closer than this are the same radius (half a DIP).
const SAME_RADIUS: f32 = 0.5;

/// `DWM_WINDOW_CORNER_PREFERENCE` values.
pub const DWMWCP_DONOTROUND: i32 = 1;
pub const DWMWCP_ROUND: i32 = 2;
pub const DWMWCP_ROUNDSMALL: i32 = 3;

/// Who rounds a window's corners ([`plan`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CornerPath {
    /// DWM, with this `DWM_WINDOW_CORNER_PREFERENCE` (a no-op before Windows 11).
    Dwm(i32),
    /// The host, at this radius in DIP (see the module doc).
    Host(f32),
}

impl CornerPath {
    /// Drawn by the host.
    pub fn is_host(self) -> bool {
        matches!(self, Self::Host(_))
    }
}

/// What decides the path beyond the radius.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CornerContext {
    /// DWM rounds windows (Windows 11, build 22000 or later).
    pub dwm_rounds: bool,
    /// The host can draw this window's frame itself (a Kubuno or custom title bar, no system
    /// material, opaque, top-level, not a floating panel: see the module doc).
    pub host_can_draw: bool,
    /// `KUBUNO_CUSTOM_CORNERS=1`: the host path for every radius it can draw.
    pub force_host: bool,
}

/// Who rounds a window asking for `radius` DIP, and how (see the module doc).
pub fn plan(radius: f32, ctx: CornerContext) -> CornerPath {
    let radius = if radius.is_finite() { radius.max(0.0) } else { 0.0 };
    if radius < SAME_RADIUS {
        return CornerPath::Dwm(DWMWCP_DONOTROUND);
    }
    let preset = dwm_preset(radius);
    if ctx.host_can_draw && (ctx.force_host || !ctx.dwm_rounds || preset.is_none()) {
        return CornerPath::Host(radius);
    }
    // DWM: exactly when the radius is a preset; otherwise the nearest one (Windows 11), or
    // nothing at all (Windows 10, where the value is ignored).
    CornerPath::Dwm(preset.unwrap_or_else(|| nearest_preset(radius)))
}

/// The DWM preset drawing exactly `radius` DIP, if any.
pub fn dwm_preset(radius: f32) -> Option<i32> {
    if (radius - crate::host::form::DEFAULT_CORNER_RADIUS).abs() < SAME_RADIUS {
        Some(DWMWCP_ROUND)
    } else if (radius - crate::host::form::SMALL_CORNER_RADIUS).abs() < SAME_RADIUS {
        Some(DWMWCP_ROUNDSMALL)
    } else {
        None
    }
}

/// The DWM preset closest to `radius` DIP (0, 4 or 8).
pub fn nearest_preset(radius: f32) -> i32 {
    if radius < 2.0 {
        DWMWCP_DONOTROUND
    } else if radius < 6.0 {
        DWMWCP_ROUNDSMALL
    } else {
        DWMWCP_ROUND
    }
}

/// A window whose frame the host draws ([`CornerPath::Host`]), as it stands.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub(crate) struct HostFrame {
    /// The radius asked for, in DIP (followed live: `CornerRadius` bound or changed).
    pub radius: f32,
    /// Windows squares the window now ([`squared`]): no margin, no curve, no border.
    pub squared: bool,
    /// The hit region last given to the window (`(w, h, margin, radius, band)`, physical px), so
    /// it is only replaced when it changes.
    pub region: Option<(i32, i32, i32, i32, i32)>,
}

impl HostFrame {
    /// The room around the frame for its shadow now, in DIP.
    pub fn margin(&self) -> f32 {
        if self.squared { 0.0 } else { SHADOW_MARGIN }
    }

    /// The radius the frame shows now, in DIP.
    pub fn shown_radius(&self) -> f32 {
        effective_radius(self.radius, self.squared)
    }
}

/// The radius a window shows: the one asked for, or 0 while Windows squares it (`squared`).
pub fn effective_radius(radius: f32, squared: bool) -> f32 {
    if squared { 0.0 } else { radius.max(0.0) }
}

/// How far the resize grip (drawn 2 DIP off the bottom-right corner, `window_chrome::grip_rect`)
/// moves in from a corner rounded at `radius` DIP, so that it stays inside the curve: the curve
/// passes `radius × (1 − 1/√2)` in from the corner along the diagonal.
pub fn grip_inset(radius: f32) -> f32 {
    (radius.max(0.0) * (1.0 - std::f32::consts::FRAC_1_SQRT_2) - 2.0).max(0.0)
}

/// The bounds to place the resize grip in for a window `bounds` rounded at `radius`.
pub fn grip_bounds(bounds: kubuno_drive_desktop_app_controls::Rect, radius: f32) -> kubuno_drive_desktop_app_controls::Rect {
    let inset = grip_inset(radius);
    kubuno_drive_desktop_app_controls::Rect::new(bounds.left, bounds.top, bounds.right - inset, bounds.bottom - inset)
}

/// Where a point lies against a rounded frame ([`shape_hit`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShapeHit {
    /// Inside the frame, clear of its resize band: the window's own hit test decides.
    Inside,
    /// On the resize band, outside or inside the curve: this `HT*` code.
    Resize(u32),
    /// Outside the frame and its band (the shadow margin): nothing of the window.
    Outside,
}

/// Where `(x, y)` lies against a frame of `size` rounded at `radius` (all in the frame's DIP, the
/// frame's top-left at the origin), with a resize band of `border` on both sides of its outline
/// (`None`: the window cannot be resized). The band follows the curve at the corners: a point
/// in a corner's band resizes from that corner, on either side of the curve.
pub fn shape_hit(size: (f32, f32), radius: f32, x: f32, y: f32, border: Option<f32>) -> ShapeHit {
    let (w, h) = size;
    let r = radius.max(0.0).min(w / 2.0).min(h / 2.0).max(0.0);
    // The signed distance to the rounded rectangle's outline (negative inside).
    let qx = (x - w / 2.0).abs() - (w / 2.0 - r);
    let qy = (y - h / 2.0).abs() - (h / 2.0 - r);
    let outside = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt();
    let distance = outside + qx.max(qy).min(0.0) - r;
    let in_corner = qx > 0.0 && qy > 0.0;
    let (left, top) = (x < w / 2.0, y < h / 2.0);
    let corner = || match (top, left) {
        (true, true) => HTTOPLEFT,
        (true, false) => HTTOPRIGHT,
        (false, true) => HTBOTTOMLEFT,
        (false, false) => HTBOTTOMRIGHT,
    };
    match border {
        Some(b) if distance > -b && distance <= b => {
            if in_corner {
                return ShapeHit::Resize(corner());
            }
            if distance > 0.0 {
                // Outside a straight edge (inside, the window's own band test answers, with its
                // own corner zones).
                let code = if x < 0.0 {
                    if y < r { HTTOPLEFT } else if y > h - r { HTBOTTOMLEFT } else { HTLEFT }
                } else if x > w {
                    if y < r { HTTOPRIGHT } else if y > h - r { HTBOTTOMRIGHT } else { HTRIGHT }
                } else if y < 0.0 {
                    HTTOP
                } else {
                    HTBOTTOM
                };
                return ShapeHit::Resize(code);
            }
            ShapeHit::Inside
        }
        _ if distance > 0.0 => ShapeHit::Outside,
        _ => ShapeHit::Inside,
    }
}

// ── The platform ─────────────────────────────────────────────────────────────

/// The Windows build number (`RtlGetVersion`, which no compatibility manifest lies to), 0 when it
/// cannot be read.
pub fn windows_build() -> u32 {
    use std::sync::OnceLock;
    static BUILD: OnceLock<u32> = OnceLock::new();
    *BUILD.get_or_init(|| {
        #[repr(C)]
        struct OsVersionInfoW {
            size: u32,
            major: u32,
            minor: u32,
            build: u32,
            platform: u32,
            csd: [u16; 128],
        }
        use windows::core::{s, w};
        use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
        // SAFETY: `RtlGetVersion` is looked up by name in ntdll (always loaded) and called with a
        // correctly sized and initialised `RTL_OSVERSIONINFOW`.
        unsafe {
            let Ok(ntdll) = GetModuleHandleW(w!("ntdll.dll")) else { return 0 };
            let Some(proc) = GetProcAddress(ntdll, s!("RtlGetVersion")) else { return 0 };
            let get: extern "system" fn(*mut OsVersionInfoW) -> i32 = std::mem::transmute(proc);
            let mut info = OsVersionInfoW { size: std::mem::size_of::<OsVersionInfoW>() as u32, major: 0, minor: 0, build: 0, platform: 0, csd: [0; 128] };
            if get(&mut info) == 0 { info.build } else { 0 }
        }
    })
}

/// DWM rounds windows (`DWMWA_WINDOW_CORNER_PREFERENCE`, Windows 11 build 22000+), unless the host
/// path is forced (`KUBUNO_CUSTOM_CORNERS`).
pub fn dwm_rounds() -> bool {
    windows_build() >= 22000
}

/// The system draws backdrop materials (`DWMWA_SYSTEMBACKDROP_TYPE`: Mica, Mica Alt, Acrylic —
/// Windows 11 22H2, build 22621+). Before, a `Backdrop` is a no-op and does not keep the host
/// from rounding the window itself.
pub fn system_backdrops() -> bool {
    windows_build() >= 22621
}

/// `KUBUNO_CUSTOM_CORNERS=1`: the host draws every rounded corner it can (Windows 10's path).
pub fn force_host() -> bool {
    use std::sync::OnceLock;
    static FORCE: OnceLock<bool> = OnceLock::new();
    *FORCE.get_or_init(|| std::env::var("KUBUNO_CUSTOM_CORNERS").is_ok_and(|v| matches!(v.trim(), "1" | "true" | "yes")))
}

/// `KUBUNO_CORNER_RADIUS=<dip>`: the radius of every top-level window of the process, for testing.
pub fn radius_override() -> Option<f32> {
    use std::sync::OnceLock;
    static RADIUS: OnceLock<Option<f32>> = OnceLock::new();
    *RADIUS.get_or_init(|| std::env::var("KUBUNO_CORNER_RADIUS").ok().and_then(|v| crate::host::form::parse_corner_radius(&v)))
}

/// Windows squares the window's corners now: maximised, snapped (`IsWindowArranged`, or its rect
/// away from its restore rect), or covering its whole monitor (full screen).
pub fn squared(hwnd: HWND) -> bool {
    use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST};
    // SAFETY: read-only queries on a live window of this thread, with correctly sized structures.
    unsafe {
        if IsZoomed(hwnd).as_bool() {
            return true;
        }
        if IsIconic(hwnd).as_bool() {
            return false;
        }
        let mut rc = RECT::default();
        if GetWindowRect(hwnd, &mut rc).is_err() {
            return false;
        }
        let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        if !GetMonitorInfoW(monitor, &mut info).as_bool() {
            return false;
        }
        let m = info.rcMonitor;
        if rc.left <= m.left && rc.top <= m.top && rc.right >= m.right && rc.bottom >= m.bottom {
            return true;
        }
        arranged(hwnd, rc, info.rcWork, info.rcMonitor)
    }
}

/// The window is snapped (Aero Snap, a snap layout): `IsWindowArranged` where it exists; before,
/// a normal window whose rectangle is not its restore rectangle.
fn arranged(hwnd: HWND, rc: RECT, work: RECT, monitor: RECT) -> bool {
    use std::sync::OnceLock;
    type IsArranged = extern "system" fn(HWND) -> i32;
    static PROC: OnceLock<Option<IsArranged>> = OnceLock::new();
    let proc = *PROC.get_or_init(|| {
        use windows::core::{s, w};
        use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
        // SAFETY: an export of user32 (loaded by every window process), with its documented
        // signature `BOOL IsWindowArranged(HWND)`.
        unsafe {
            let user32 = GetModuleHandleW(w!("user32.dll")).ok()?;
            GetProcAddress(user32, s!("IsWindowArranged")).map(|p| std::mem::transmute::<_, IsArranged>(p))
        }
    });
    if let Some(is_arranged) = proc {
        return is_arranged(hwnd) != 0;
    }
    // SAFETY: plain placement query with a correctly sized structure.
    unsafe {
        let mut placement = WINDOWPLACEMENT { length: std::mem::size_of::<WINDOWPLACEMENT>() as u32, ..Default::default() };
        if GetWindowPlacement(hwnd, &mut placement).is_err() || placement.showCmd != SW_SHOWNORMAL.0 as u32 {
            return false;
        }
        // The restore rectangle is in workspace coordinates (the work area's origin).
        let n = placement.rcNormalPosition;
        let (dx, dy) = (work.left - monitor.left, work.top - monitor.top);
        let normal = RECT { left: n.left + dx, top: n.top + dy, right: n.right + dx, bottom: n.bottom + dy };
        normal != rc
    }
}

/// Turns DWM's frame off for a host-rounded window (no square shadow or border behind its curve)
/// or back on. Before the window shows.
pub fn set_dwm_frame(hwnd: HWND, on: bool) {
    use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMNCRP_DISABLED, DWMNCRP_USEWINDOWSTYLE, DWMWA_NCRENDERING_POLICY};
    let policy = if on { DWMNCRP_USEWINDOWSTYLE } else { DWMNCRP_DISABLED };
    // SAFETY: a 4-byte enum value alive for the call, on the host's own window.
    unsafe {
        let _ = DwmSetWindowAttribute(hwnd, DWMWA_NCRENDERING_POLICY, &policy.0 as *const i32 as *const _, 4);
    }
}

/// The window region of a host-rounded window `w × h` physical pixels: its frame (one `margin`
/// in), rounded at `radius`, grown by `band` (all in physical pixels) — what takes the clicks and
/// what shows (the region also cuts the drawing); the window rectangle's corners beyond it let
/// the clicks through. A window without a margin (squared) gets its whole rectangle, never no
/// region at all: a window without DWM's frame (`set_dwm_frame`) that has no region of its own
/// is given one by the system — the classic theme's, with rounded top corners (measured: a
/// snapped window showed them).
pub fn set_hit_region(hwnd: HWND, w: i32, h: i32, margin: i32, radius: i32, band: i32) {
    use windows::Win32::Graphics::Gdi::{CreateRoundRectRgn, SetWindowRgn};
    // SAFETY: the region is created here and handed over to the window (`SetWindowRgn` owns it on
    // success, and the system deletes a region it failed to take only when it is a valid one, so
    // a failure leaks at most one region handle).
    unsafe {
        let inset = (margin - band).max(0);
        let r = (radius + band).max(0);
        // `CreateRoundRectRgn` takes the corner ellipse's width and height; its right and bottom
        // edges are exclusive, hence the + 1.
        let region = CreateRoundRectRgn(inset, inset, w - inset + 1, h - inset + 1, 2 * r, 2 * r);
        let _ = SetWindowRgn(hwnd, Some(region), true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WIN11: CornerContext = CornerContext { dwm_rounds: true, host_can_draw: true, force_host: false };
    const WIN10: CornerContext = CornerContext { dwm_rounds: false, host_can_draw: true, force_host: false };

    #[test]
    fn a_preset_radius_goes_to_dwm_on_windows_11() {
        assert_eq!(plan(8.0, WIN11), CornerPath::Dwm(DWMWCP_ROUND));
        assert_eq!(plan(8.2, WIN11), CornerPath::Dwm(DWMWCP_ROUND), "half a DIP is the same radius");
        assert_eq!(plan(4.0, WIN11), CornerPath::Dwm(DWMWCP_ROUNDSMALL));
        assert_eq!(plan(0.0, WIN11), CornerPath::Dwm(DWMWCP_DONOTROUND));
        assert_eq!(plan(-3.0, WIN11), CornerPath::Dwm(DWMWCP_DONOTROUND), "a negative radius is square");
        assert_eq!(plan(f32::NAN, WIN11), CornerPath::Dwm(DWMWCP_DONOTROUND));
    }

    #[test]
    fn any_other_radius_is_drawn_by_the_host() {
        assert_eq!(plan(16.0, WIN11), CornerPath::Host(16.0));
        assert_eq!(plan(24.0, WIN11), CornerPath::Host(24.0));
        assert_eq!(plan(6.0, WIN11), CornerPath::Host(6.0));
    }

    #[test]
    fn windows_10_draws_every_rounded_corner_itself() {
        assert_eq!(plan(8.0, WIN10), CornerPath::Host(8.0));
        assert_eq!(plan(4.0, WIN10), CornerPath::Host(4.0));
        assert_eq!(plan(0.0, WIN10), CornerPath::Dwm(DWMWCP_DONOTROUND), "square needs nothing");
    }

    #[test]
    fn the_forced_host_path_reproduces_windows_10() {
        let forced = CornerContext { force_host: true, ..WIN11 };
        assert_eq!(plan(8.0, forced), CornerPath::Host(8.0));
        assert_eq!(plan(0.0, forced), CornerPath::Dwm(DWMWCP_DONOTROUND));
    }

    #[test]
    fn a_window_the_host_cannot_draw_takes_the_nearest_preset() {
        let material = CornerContext { host_can_draw: false, ..WIN11 };
        assert_eq!(plan(16.0, material), CornerPath::Dwm(DWMWCP_ROUND));
        assert_eq!(plan(5.0, material), CornerPath::Dwm(DWMWCP_ROUNDSMALL));
        assert_eq!(plan(1.0, material), CornerPath::Dwm(DWMWCP_DONOTROUND));
        assert_eq!(plan(8.0, material), CornerPath::Dwm(DWMWCP_ROUND));
        let old = CornerContext { host_can_draw: false, ..WIN10 };
        assert!(!plan(16.0, old).is_host(), "Windows 10 without the host path: DWM's value, ignored there");
    }

    #[test]
    fn the_grip_stays_inside_the_curve() {
        assert_eq!(grip_inset(0.0), 0.0);
        assert_eq!(grip_inset(4.0), 0.0, "Windows 11's small corners leave the grip where it is");
        assert!(grip_inset(8.0) < 0.5);
        let i = grip_inset(24.0);
        assert!((i - (24.0 * (1.0 - std::f32::consts::FRAC_1_SQRT_2) - 2.0)).abs() < 1e-4 && i > 4.0);
        let b = grip_bounds(kubuno_drive_desktop_app_controls::Rect::new(0.0, 0.0, 400.0, 300.0), 24.0);
        assert_eq!((b.right, b.bottom), (400.0 - i, 300.0 - i));
    }

    #[test]
    fn windows_squares_a_maximised_or_snapped_window() {
        assert_eq!(effective_radius(16.0, true), 0.0);
        assert_eq!(effective_radius(16.0, false), 16.0);
    }

    #[test]
    fn the_resize_band_follows_the_curve() {
        let size = (400.0, 300.0);
        let b = Some(8.0);
        // Well inside: the window decides.
        assert_eq!(shape_hit(size, 24.0, 200.0, 150.0, b), ShapeHit::Inside);
        // Just outside the curve of the top-left corner (cut away, transparent): it resizes from
        // the corner, it is not the caption.
        assert_eq!(shape_hit(size, 24.0, 3.0, 3.0, b), ShapeHit::Resize(HTTOPLEFT));
        // Just inside the curve, near it: the corner too.
        assert_eq!(shape_hit(size, 24.0, 9.0, 9.0, b), ShapeHit::Resize(HTTOPLEFT));
        // The top-right corner, where a close button sits.
        assert_eq!(shape_hit(size, 24.0, 398.0, 2.0, b), ShapeHit::Resize(HTTOPRIGHT));
        // Outside the straight edges, within the band.
        assert_eq!(shape_hit(size, 24.0, -4.0, 150.0, b), ShapeHit::Resize(HTLEFT));
        assert_eq!(shape_hit(size, 24.0, 200.0, 305.0, b), ShapeHit::Resize(HTBOTTOM));
        assert_eq!(shape_hit(size, 24.0, 405.0, 150.0, b), ShapeHit::Resize(HTRIGHT));
        // Inside a straight edge: the window's own band test answers.
        assert_eq!(shape_hit(size, 24.0, 3.0, 150.0, b), ShapeHit::Inside);
        // Beyond the band: the shadow margin, nothing.
        assert_eq!(shape_hit(size, 24.0, -12.0, 150.0, b), ShapeHit::Outside);
        // Not resizable: outside the curve is nothing, inside is the window's.
        assert_eq!(shape_hit(size, 24.0, 2.0, 2.0, None), ShapeHit::Outside);
        assert_eq!(shape_hit(size, 24.0, 9.0, 9.0, None), ShapeHit::Inside);
        // Square: the corner pixel is the window's.
        assert_eq!(shape_hit(size, 0.0, 0.5, 0.5, None), ShapeHit::Inside);
    }
}
