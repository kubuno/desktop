//! The splash window: a layered, top-most, never-activated popup on its own thread, painted with
//! Direct2D into a DIB at its monitor's DPI and handed to the compositor with
//! `UpdateLayeredWindow` (per-pixel alpha for the rounded card and its shadow, a constant alpha
//! for the fades).

use std::cell::RefCell;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Instant;

use accesskit::{ActionHandler, ActionRequest, ActivationHandler, Live, Node, NodeId, Role, TreeId, TreeInfo, TreeUpdate};
use windows::core::{w, Result, BOOL, HSTRING};
use windows::Win32::Foundation::{COLORREF, ERROR_SUCCESS, FILETIME, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
use windows::Win32::Graphics::Direct2D::Common::{D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_PIXEL_FORMAT, D2D_RECT_F, D2D_SIZE_U};
use windows::Win32::Graphics::Direct2D::{
    D2D1CreateFactory, ID2D1Bitmap, ID2D1DCRenderTarget, ID2D1Factory, ID2D1RenderTarget, D2D1_BITMAP_INTERPOLATION_MODE_NEAREST_NEIGHBOR,
    D2D1_ANTIALIAS_MODE_ALIASED, D2D1_BITMAP_PROPERTIES, D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_FEATURE_LEVEL_DEFAULT, D2D1_RENDER_TARGET_PROPERTIES,
    D2D1_RENDER_TARGET_TYPE_SOFTWARE, D2D1_RENDER_TARGET_USAGE_NONE, D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE,
};
use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, GetMonitorInfoW, MonitorFromPoint, ReleaseDC, SelectObject, AC_SRC_ALPHA, AC_SRC_OVER,
    BITMAPINFO, BITMAPINFOHEADER, BI_RGB, BLENDFUNCTION, DIB_RGB_COLORS, HBITMAP, HDC, HGDIOBJ, HMONITOR, MONITORINFO, MONITOR_DEFAULTTOPRIMARY,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
use windows::Win32::System::SystemInformation::GetSystemTimePreciseAsFileTime;
use windows::Win32::System::Threading::{GetCurrentProcess, GetCurrentProcessId, GetProcessTimes, GetStartupInfoW, STARTF_USESTDHANDLES, STARTUPINFOW};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, SetThreadDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, EnumWindows, GetCursorPos, GetMessageW, GetWindowLongPtrW, GetWindowRect,
    GetWindowThreadProcessId, IsWindow, IsWindowVisible, KillTimer, LoadCursorW, PostMessageW, PostQuitMessage, RegisterClassExW, SetTimer, SetWindowPos,
    ShowWindow, SystemParametersInfoW, TranslateMessage, GWL_EXSTYLE, HWND_TOPMOST, IDC_APPSTARTING, MA_NOACTIVATE, MSG, SPI_GETCLIENTAREAANIMATION,
    SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SW_SHOWNOACTIVATE, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, ULW_ALPHA, WM_DESTROY, WM_GETOBJECT, WM_LBUTTONDOWN,
    WM_MOUSEACTIVATE, WM_RBUTTONDOWN, WM_TIMER, WNDCLASSEXW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};

use super::art::{self, Parts, HEIGHT, WIDTH};
use super::{Inner, SplashContent, Timeline, WATCH_ANY, WM_SPLASH_WAKE};

/// The transparent margin around the card, for its shadow (DIP).
const MARGIN: f32 = 36.0;
/// How far the card rises while it fades in (DIP).
const RISE: f32 = 12.0;
const TIMER_ID: usize = 1;
/// The frame interval while animating (ms).
const FRAME_MS: u32 = 15;
/// How often the window it waits for is looked for (ms).
const WATCH_MS: u64 = 50;

/// Starts the splash thread.
pub(super) fn spawn(inner: Arc<Inner>) -> std::io::Result<()> {
    std::thread::Builder::new()
        .name("kubuno-splash".into())
        .spawn(move || {
            if let Err(e) = run(&inner) {
                tracing::warn!("the splash screen could not be shown: {e}");
            }
            STATE.with(|s| s.borrow_mut().take());
            inner.hwnd.store(0, Ordering::Release);
            inner.done.store(true, Ordering::Release);
        })
        .map(|_| ())
}

/// Wakes the splash thread (a status or a request changed).
pub(super) fn wake(hwnd: isize) {
    if hwnd != 0 {
        // SAFETY: posting to a window handle, possibly gone (the call then fails harmlessly).
        unsafe {
            let _ = PostMessageW(Some(HWND(hwnd as *mut core::ffi::c_void)), WM_SPLASH_WAKE, WPARAM(0), LPARAM(0));
        }
    }
}

/// `HKCU\Software\Kubuno\Desktop`, `SplashScreen` (DWORD) = 0.
pub(super) fn registry_switch_off() -> bool {
    let mut value: u32 = 1;
    let mut size = std::mem::size_of::<u32>() as u32;
    // SAFETY: reads a DWORD into a local of that size.
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!("Software\\Kubuno\\Desktop"),
            w!("SplashScreen"),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut value as *mut u32 as *mut core::ffi::c_void),
            Some(&mut size),
        )
    };
    status == ERROR_SUCCESS && value == 0
}

/// How long ago the process started, in milliseconds.
pub(super) fn process_age_ms() -> Option<u64> {
    let ticks = |f: FILETIME| (u64::from(f.dwHighDateTime) << 32) | u64::from(f.dwLowDateTime);
    let (mut created, mut exited, mut kernel, mut user) = (FILETIME::default(), FILETIME::default(), FILETIME::default(), FILETIME::default());
    // SAFETY: queries of this process's own times into locals.
    unsafe {
        GetProcessTimes(GetCurrentProcess(), &mut created, &mut exited, &mut kernel, &mut user).ok()?;
        let now = GetSystemTimePreciseAsFileTime();
        Some(ticks(now).saturating_sub(ticks(created)) / 10_000)
    }
}

thread_local! {
    /// The splash window of this thread.
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

/// Everything the splash window owns.
struct State {
    inner: Arc<Inner>,
    hwnd: HWND,
    clock: Instant,
    timeline: Timeline,
    /// The content painted last.
    content: SplashContent,
    /// The progress as displayed (eased toward the content's).
    shown_progress: Option<f32>,
    surface: Surface,
    rt: ID2D1DCRenderTarget,
    /// The card under the status line and the progress bar.
    strip: Strip,
    scale: f32,
    origin: POINT,
    size: SIZE,
    /// The last opacity and rise handed to the compositor.
    last: Option<(u8, i32)>,
    next_watch: u64,
    animations: bool,
    access: accesskit_windows::Adapter,
    first_paint_logged: bool,
    /// The timer interval in force (ms): a frame interval while moving, slower while still.
    interval: u32,
}

/// A 32-bit top-down DIB selected into a memory DC: what Direct2D paints into and
/// `UpdateLayeredWindow` reads from.
struct Surface {
    dc: HDC,
    bitmap: HBITMAP,
    old: HGDIOBJ,
    /// The DIB's pixels (premultiplied BGRA, top-down rows), alive as long as `bitmap`.
    bits: *mut core::ffi::c_void,
    width: i32,
    height: i32,
}

impl Surface {
    fn new(width: i32, height: i32) -> Result<Self> {
        // SAFETY: GDI objects created here and released by `Drop`.
        unsafe {
            let screen = GetDC(None);
            let dc = CreateCompatibleDC(Some(screen));
            ReleaseDC(None, screen);
            if dc.is_invalid() {
                return Err(windows::core::Error::from_thread());
            }
            let info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: width,
                    biHeight: -height,
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bits = std::ptr::null_mut();
            let bitmap = match CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut bits, None, 0) {
                Ok(b) => b,
                Err(e) => {
                    let _ = DeleteDC(dc);
                    return Err(e);
                }
            };
            let old = SelectObject(dc, HGDIOBJ(bitmap.0));
            Ok(Self { dc, bitmap, old, bits, width, height })
        }
    }
}

impl Drop for Surface {
    fn drop(&mut self) {
        // SAFETY: releases what `new` created, in reverse order.
        unsafe {
            SelectObject(self.dc, self.old);
            let _ = DeleteObject(HGDIOBJ(self.bitmap.0));
            let _ = DeleteDC(self.dc);
        }
    }
}

/// A software Direct2D target drawing into `surface` at `dpi` (software: no Direct3D device to
/// create, which is what makes the first frame fast).
fn dc_target(surface: &Surface, dpi: f32) -> Result<ID2D1DCRenderTarget> {
    // SAFETY: Direct2D objects created here, bound to the live DIB of `surface`.
    unsafe {
        let factory: ID2D1Factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
        let props = D2D1_RENDER_TARGET_PROPERTIES {
            r#type: D2D1_RENDER_TARGET_TYPE_SOFTWARE,
            pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED },
            dpiX: dpi,
            dpiY: dpi,
            usage: D2D1_RENDER_TARGET_USAGE_NONE,
            minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
        };
        let rt = factory.CreateDCRenderTarget(&props)?;
        rt.BindDC(surface.dc, &RECT { left: 0, top: 0, right: surface.width, bottom: surface.height })?;
        rt.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
        Ok(rt)
    }
}

/// Paints the whole window into its DIB — shadow, card and (`dynamic`) the status line and the
/// progress bar — in one pass, straight into the DIB: the first frame is the one that counts.
fn paint_window(rt: &ID2D1DCRenderTarget, content: &SplashContent, scale: f32, margin: i32, dynamic: Option<(Option<f32>, f32)>) -> Result<()> {
    let m = margin as f32 / scale;
    // SAFETY: drawing on the live DC target, between its BeginDraw and EndDraw.
    unsafe {
        rt.BeginDraw();
        rt.Clear(Some(&D2D1_COLOR_F::default()));
        let target: &ID2D1RenderTarget = rt;
        let mut painted = art::paint_shadow(target, (m, m), 1.0).and_then(|_| art::paint(target, (m, m), 1.0, content, Parts::Static));
        if let (Ok(()), Some((progress, phase))) = (&painted, dynamic) {
            painted = art::paint(target, (m, m), 1.0, content, Parts::Dynamic { progress, phase });
        }
        rt.EndDraw(None, None)?;
        painted
    }
}

/// The part of the card under the status line and the progress bar, as painted without them: a
/// frame restores it, then paints them again — never the whole artwork.
struct Strip {
    bitmap: ID2D1Bitmap,
    /// Where it goes, in the window's DIP.
    rect: D2D_RECT_F,
}

/// The strip around the status line and progress bar (`art::Layout`, with room for the spark at
/// the head of the bar), in card DIP.
const STRIP: (f32, f32, f32, f32) = (34.0, 372.0, 480.0, 424.0);

/// One frame after the first: the strip restored, the status line and the bar painted over it.
fn paint_status(rt: &ID2D1DCRenderTarget, strip: &Strip, content: &SplashContent, scale: f32, progress: Option<f32>, phase: f32) -> Result<()> {
    let m = (MARGIN * scale).round() / scale;
    // SAFETY: drawing on the live DC target, between its BeginDraw and EndDraw; the clip pushed is
    // popped.
    unsafe {
        rt.BeginDraw();
        rt.PushAxisAlignedClip(&strip.rect, D2D1_ANTIALIAS_MODE_ALIASED);
        rt.DrawBitmap(&strip.bitmap, Some(&strip.rect), 1.0, D2D1_BITMAP_INTERPOLATION_MODE_NEAREST_NEIGHBOR, None);
        let target: &ID2D1RenderTarget = rt;
        let painted = art::paint(target, (m, m), 1.0, content, Parts::Dynamic { progress, phase });
        rt.PopAxisAlignedClip();
        rt.EndDraw(None, None)?;
        painted
    }
}

/// Copies the strip out of the DIB just painted (shadow and card only).
fn capture_strip(rt: &ID2D1DCRenderTarget, surface: &Surface, scale: f32, margin: i32, dpi: f32) -> Result<Strip> {
    let px = |v: f32| margin + (v * scale) as i32;
    let (x0, y0) = (px(STRIP.0).max(0), px(STRIP.1).max(0));
    let (x1, y1) = ((px(STRIP.2) + 1).min(surface.width), (px(STRIP.3) + 1).min(surface.height));
    let props = D2D1_BITMAP_PROPERTIES { pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED }, dpiX: dpi, dpiY: dpi };
    let pitch = surface.width as u32 * 4;
    // SAFETY: reads rows `y0..y1`, columns `x0..x1` of the live DIB (GDI and Direct2D are done with
    // it: EndDraw returned), through a pointer inside it and its own row pitch.
    unsafe {
        let start = (surface.bits as *const u8).add((y0 as usize * surface.width as usize + x0 as usize) * 4);
        let bitmap = rt.CreateBitmap(D2D_SIZE_U { width: (x1 - x0) as u32, height: (y1 - y0) as u32 }, Some(start as *const core::ffi::c_void), pitch, &props)?;
        let d = |v: i32| v as f32 / scale;
        Ok(Strip { bitmap, rect: D2D_RECT_F { left: d(x0), top: d(y0), right: d(x1), bottom: d(y1) } })
    }
}

/// A splash rendered off screen: premultiplied BGRA pixels, top-down rows, the card `margin` px
/// inside the image (the shadow around it).
#[derive(Debug, Clone)]
pub struct Still {
    pub width: u32,
    pub height: u32,
    pub margin: u32,
    pub pixels: Vec<u8>,
}

/// Renders the splash showing `content` as its window would at `scale` (1.0 = 100 %), status
/// line and progress bar included — for previews, documentation and tests.
pub fn render_still(content: &SplashContent, scale: f32) -> Result<Still> {
    let scale = scale.clamp(0.5, 4.0);
    let margin = (MARGIN * scale).round() as i32;
    let size = SIZE { cx: (WIDTH * scale).round() as i32 + 2 * margin, cy: (HEIGHT * scale).round() as i32 + 2 * margin };
    let surface = Surface::new(size.cx, size.cy)?;
    let rt = dc_target(&surface, 96.0 * scale)?;
    paint_window(&rt, content, scale, margin, Some((content.progress, 0.45)))?;
    // SAFETY: reads the DIB the DC target filled (GDI and Direct2D are done with it once EndDraw
    // returned).
    unsafe {
        let len = (size.cx * size.cy * 4) as usize;
        let pixels = std::slice::from_raw_parts(surface.bits as *const u8, len).to_vec();
        Ok(Still { width: size.cx as u32, height: size.cy as u32, margin: margin as u32, pixels })
    }
}

/// The monitor the application was launched on: the one the shell hands over in the start-up
/// information (`STARTF_HASSHELLDATA`: `hStdOutput` is then a monitor), else the one under the
/// pointer — where the user just clicked the tile or the icon.
fn launch_monitor() -> HMONITOR {
    const STARTF_HASSHELLDATA: u32 = 0x400;
    // SAFETY: plain queries into locals.
    unsafe {
        let mut si = STARTUPINFOW { cb: std::mem::size_of::<STARTUPINFOW>() as u32, ..Default::default() };
        GetStartupInfoW(&mut si);
        if si.dwFlags.0 & STARTF_HASSHELLDATA != 0 && si.dwFlags.0 & STARTF_USESTDHANDLES.0 == 0 {
            let monitor = HMONITOR(si.hStdOutput.0);
            let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
            if GetMonitorInfoW(monitor, &mut mi).as_bool() {
                return monitor;
            }
        }
        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        MonitorFromPoint(pt, MONITOR_DEFAULTTOPRIMARY)
    }
}

/// Where a window `w × h` px goes in `work`: centred, a little above the middle (the eye's centre).
pub(crate) fn placement(work: RECT, w: i32, h: i32) -> POINT {
    let (ww, wh) = (work.right - work.left, work.bottom - work.top);
    // Larger than the work area: pinned to its top-left corner.
    let x = if w >= ww { work.left } else { work.left + (ww - w) / 2 };
    let y = if h >= wh { work.top } else { work.top + ((wh - h) as f32 * 0.46) as i32 };
    POINT { x, y }
}

/// Whether Windows' « animation effects » are on.
fn animations_enabled() -> bool {
    let mut on = BOOL(1);
    // SAFETY: reads a BOOL into a local.
    unsafe {
        let _ = SystemParametersInfoW(SPI_GETCLIENTAREAANIMATION, 0, Some(&mut on as *mut BOOL as *mut core::ffi::c_void), SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0));
    }
    on.as_bool()
}

fn run(inner: &Arc<Inner>) -> Result<()> {
    // SAFETY: Win32 and Direct2D calls on this thread's own objects; every handle is owned by the
    // `State` below or released before returning.
    unsafe {
        let _ = SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        // The application's own start-up runs flat out beside it: the splash must not wait behind.
        let _ = windows::Win32::System::Threading::SetThreadPriority(
            windows::Win32::System::Threading::GetCurrentThread(),
            windows::Win32::System::Threading::THREAD_PRIORITY_ABOVE_NORMAL,
        );
        let clock = Instant::now();
        let monitor = launch_monitor();
        let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        let _ = GetMonitorInfoW(monitor, &mut mi);
        let (mut dpi_x, mut dpi_y) = (96u32, 96u32);
        if GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y).is_err() {
            dpi_x = 96;
        }
        let dpi = dpi_x.max(48) as f32;
        let scale = dpi / 96.0;
        let margin = (MARGIN * scale).round() as i32;
        let (card_w, card_h) = ((WIDTH * scale).round() as i32, (HEIGHT * scale).round() as i32);
        let size = SIZE { cx: card_w + 2 * margin, cy: card_h + 2 * margin };
        let origin = placement(mi.rcWork, size.cx, size.cy);

        let instance = GetModuleHandleW(None)?;
        let class = w!("KubunoSplashScreen");
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(wndproc),
            hInstance: instance.into(),
            hCursor: LoadCursorW(None, IDC_APPSTARTING)?,
            lpszClassName: class,
            ..Default::default()
        };
        // A second splash in the process finds the class registered: fine.
        let _ = RegisterClassExW(&wc);
        let content = inner.lock().clone();
        let hwnd = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            class,
            &HSTRING::from(content.product.as_str()),
            WS_POPUP,
            origin.x,
            origin.y,
            size.cx,
            size.cy,
            None,
            None,
            Some(instance.into()),
            None,
        )?;

        // The first frame: shadow and card straight into the DIB, the strip under the status line
        // kept, then the status line and the bar on top.
        let surface = Surface::new(size.cx, size.cy)?;
        let rt = dc_target(&surface, dpi)?;
        paint_window(&rt, &content, scale, margin, None)?;
        let strip = capture_strip(&rt, &surface, scale, margin, dpi)?;

        let access = accesskit_windows::Adapter::new(hwnd, false, NoActions);
        let animations = animations_enabled();
        let (fade_in, fade_out) = if animations { (240, 320) } else { (0, 0) };
        let mut state = State {
            inner: inner.clone(),
            hwnd,
            clock,
            timeline: Timeline::new(inner.min_ms, inner.max_ms, fade_in, fade_out),
            shown_progress: content.progress,
            content,
            surface,
            rt,
            strip,
            scale,
            origin,
            size,
            last: None,
            next_watch: 0,
            animations,
            access,
            first_paint_logged: false,
            interval: FRAME_MS,
        };
        state.render()?;
        state.present(true, 0.0, 1.0);
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
        let now = state.now();
        state.timeline.shown(now);
        if let Some(age) = process_age_ms() {
            inner.first_paint_ms.store(age, Ordering::Release);
        }
        inner.hwnd.store(hwnd.0 as isize, Ordering::Release);
        STATE.with(|s| *s.borrow_mut() = Some(state));
        SetTimer(Some(hwnd), TIMER_ID, FRAME_MS, None);

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    Ok(())
}

impl State {
    fn now(&self) -> u64 {
        self.clock.elapsed().as_millis() as u64
    }

    /// Paints the status line and the progress bar into the DIB, over the card's strip restored
    /// under them (the rest of the DIB keeps the artwork painted once).
    fn render(&mut self) -> Result<()> {
        let phase = if self.animations { (self.now() % 1800) as f32 / 1800.0 } else { 0.35 };
        paint_status(&self.rt, &self.strip, &self.content, self.scale, self.shown_progress, phase)
    }

    /// Hands the DIB (when `pixels` changed) and the opacity and position to the compositor.
    fn present(&mut self, pixels: bool, entrance: f32, opacity: f32) {
        let alpha = (opacity.clamp(0.0, 1.0) * 255.0).round() as u8;
        let rise = ((1.0 - entrance) * RISE * self.scale).round() as i32;
        if !pixels && self.last == Some((alpha, rise)) {
            return;
        }
        self.last = Some((alpha, rise));
        let blend = BLENDFUNCTION { BlendOp: AC_SRC_OVER as u8, BlendFlags: 0, SourceConstantAlpha: alpha, AlphaFormat: AC_SRC_ALPHA as u8 };
        let dst = POINT { x: self.origin.x, y: self.origin.y + rise };
        let src = POINT { x: 0, y: 0 };
        // SAFETY: the layered window of this thread and its DIB.
        unsafe {
            let _ = windows::Win32::UI::WindowsAndMessaging::UpdateLayeredWindow(
                self.hwnd,
                None,
                Some(&dst),
                Some(&self.size),
                pixels.then_some(self.surface.dc),
                pixels.then_some(&src as *const POINT),
                COLORREF(0),
                Some(&blend),
                ULW_ALPHA,
            );
        }
    }

    /// One turn of the timer: requests, the watched window, the content, the animation.
    fn tick(&mut self) {
        let now = self.now();
        let inner = self.inner.clone();
        if inner.dismiss.swap(false, Ordering::AcqRel) {
            self.timeline.dismiss(now);
        }
        if inner.close.load(Ordering::Acquire) {
            self.timeline.request_close(now);
        }
        if now >= self.next_watch {
            self.next_watch = now + WATCH_MS;
            if watched_window_shown(inner.watch.load(Ordering::Acquire), self.hwnd) {
                self.timeline.request_close(now);
            }
        }
        self.timeline.update(now);

        let content = inner.lock().clone();
        let mut dirty = false;
        if content != self.content {
            let status_changed = content.status != self.content.status;
            self.content = content;
            dirty = true;
            if status_changed {
                self.publish_access();
            }
        }
        // The bar eases toward its value, and fills up as the splash goes.
        let target = if self.timeline.closing() { Some(1.0) } else { self.content.progress };
        self.shown_progress = match (target, self.shown_progress) {
            (None, _) => {
                dirty |= self.animations;
                None
            }
            (Some(t), None) => {
                dirty = true;
                Some(if self.animations { 0.0 } else { t })
            }
            (Some(t), Some(shown)) => {
                let next = if self.animations { shown + (t - shown) * 0.16 } else { t };
                let next = if (t - next).abs() < 0.002 { t } else { next };
                dirty |= next != shown;
                Some(next)
            }
        };
        if dirty {
            if let Err(e) = self.render() {
                tracing::warn!("splash screen frame: {e}");
            }
        }
        self.present(dirty, self.timeline.entrance(now), self.timeline.opacity(now));
        // Still (nothing fading, rising or shimmering): the timer only has to watch.
        let interval = if dirty || self.timeline.animating(now) { FRAME_MS } else { WATCH_MS as u32 };
        if interval != self.interval {
            self.interval = interval;
            // SAFETY: re-arms this thread's own timer.
            unsafe { SetTimer(Some(self.hwnd), TIMER_ID, interval, None) };
        }

        if self.timeline.finished(now) {
            self.finish(now);
        }
    }

    fn finish(&mut self, now: u64) {
        if !self.first_paint_logged {
            self.first_paint_logged = true;
            let first = self.inner.first_paint_ms.load(Ordering::Acquire);
            let asked = self.inner.shown_after_ms.load(Ordering::Acquire);
            tracing::info!(
                "splash screen « {} »: first frame {} ms after the process started (shown at {} ms), {} ms on screen",
                self.content.product,
                if first == u64::MAX { "?".to_string() } else { first.to_string() },
                if asked == u64::MAX { "?".to_string() } else { asked.to_string() },
                now
            );
        }
        // SAFETY: this thread's own window and timer.
        unsafe {
            let _ = KillTimer(Some(self.hwnd), TIMER_ID);
            let _ = DestroyWindow(self.hwnd);
        }
    }

    /// Pushes the status and progress to UI Automation (a polite live region: Narrator reads it).
    fn publish_access(&mut self) {
        let tree = access_tree(&self.content, self.scale);
        if let Some(events) = self.access.update_if_active(|| tree) {
            events.raise();
        }
    }
}

/// What UI Automation sees: the window named after the product, its status line (a live region)
/// and its progress.
fn access_tree(content: &SplashContent, scale: f32) -> TreeUpdate {
    const ROOT: NodeId = NodeId(0);
    const STATUS: NodeId = NodeId(1);
    const PROGRESS: NodeId = NodeId(2);
    let l = art::Layout::standard();
    let m = MARGIN * scale;
    let bounds = |r: crate::Rect| accesskit::Rect {
        x0: f64::from(m + r.left * scale),
        y0: f64::from(m + r.top * scale),
        x1: f64::from(m + r.right * scale),
        y1: f64::from(m + r.bottom * scale),
    };
    let mut root = Node::new(Role::Window);
    root.set_label(content.product.clone());
    if !content.version.is_empty() {
        root.set_description(content.version.clone());
    }
    root.set_children(vec![STATUS, PROGRESS]);
    let mut status = Node::new(Role::Label);
    status.set_label(content.status.clone());
    status.set_live(Live::Polite);
    status.set_bounds(bounds(l.status));
    let mut progress = Node::new(Role::ProgressIndicator);
    progress.set_label("Progression du démarrage");
    progress.set_bounds(bounds(l.progress));
    if let Some(p) = content.progress {
        progress.set_min_numeric_value(0.0);
        progress.set_max_numeric_value(100.0);
        progress.set_numeric_value(f64::from((p * 100.0).round()));
    }
    TreeUpdate { nodes: vec![(ROOT, root), (STATUS, status), (PROGRESS, progress)], tree: Some(TreeInfo::new(ROOT)), tree_id: TreeId::ROOT, focus: ROOT }
}

/// The splash has nothing to act on.
struct NoActions;

impl ActionHandler for NoActions {
    fn do_action(&mut self, _request: ActionRequest) {}
}

/// Answers UI Automation's first question with the current tree.
struct Activation(TreeUpdate);

impl ActivationHandler for Activation {
    fn request_initial_tree(&mut self) -> Option<TreeUpdate> {
        Some(self.0.clone())
    }
}

/// Whether what the splash waits for is on screen: window `watch`, or ([`WATCH_ANY`]) any
/// window of the process but the splash.
fn watched_window_shown(watch: isize, me: HWND) -> bool {
    match watch {
        0 => false,
        WATCH_ANY => any_window_shown(me),
        h => {
            let hwnd = HWND(h as *mut core::ffi::c_void);
            // SAFETY: queries of a window handle, possibly gone (they then answer false).
            unsafe { IsWindow(Some(hwnd)).as_bool() && shown(hwnd) }
        }
    }
}

/// Visible and not cloaked (a window the compositor holds back until its first frame is not
/// on screen yet).
unsafe fn shown(hwnd: HWND) -> bool {
    if !IsWindowVisible(hwnd).as_bool() {
        return false;
    }
    let mut cloaked: u32 = 0;
    let ok = DwmGetWindowAttribute(hwnd, DWMWA_CLOAKED, &mut cloaked as *mut u32 as *mut core::ffi::c_void, std::mem::size_of::<u32>() as u32).is_ok();
    !(ok && cloaked != 0)
}

struct Search {
    me: HWND,
    pid: u32,
    found: bool,
}

/// Whether the process shows a real window (not a tool window, not a speck) other than `me`.
fn any_window_shown(me: HWND) -> bool {
    // SAFETY: enumerates the top-level windows with a callback that only reads them.
    unsafe {
        let mut search = Search { me, pid: GetCurrentProcessId(), found: false };
        let _ = EnumWindows(Some(enum_proc), LPARAM(&mut search as *mut Search as isize));
        search.found
    }
}

unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
    // SAFETY: `lparam` is the `Search` `any_window_shown` passes, alive for the enumeration.
    let search = unsafe { &mut *(lparam.0 as *mut Search) };
    if hwnd == search.me {
        return BOOL(1);
    }
    let mut pid = 0u32;
    // SAFETY: plain queries of a window handle.
    unsafe {
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid != search.pid || !shown(hwnd) {
            return BOOL(1);
        }
        let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
        if ex & WS_EX_TOOLWINDOW.0 != 0 {
            return BOOL(1);
        }
        let mut r = RECT::default();
        if GetWindowRect(hwnd, &mut r).is_ok() && r.right - r.left >= 120 && r.bottom - r.top >= 80 {
            search.found = true;
            return BOOL(0);
        }
    }
    BOOL(1)
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_TIMER | WM_SPLASH_WAKE => {
            STATE.with(|s| {
                if let Ok(mut s) = s.try_borrow_mut() {
                    if let Some(state) = s.as_mut() {
                        state.tick();
                    }
                }
            });
            LRESULT(0)
        }
        WM_LBUTTONDOWN | WM_RBUTTONDOWN => {
            STATE.with(|s| {
                if let Ok(mut s) = s.try_borrow_mut() {
                    if let Some(state) = s.as_mut() {
                        let now = state.now();
                        state.timeline.dismiss(now);
                    }
                }
            });
            LRESULT(0)
        }
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        WM_GETOBJECT => {
            // The answer is handed back once the state is no longer borrowed: returning the
            // provider to UI Automation may send a nested WM_GETOBJECT.
            let answer = STATE.with(|s| {
                let mut s = s.try_borrow_mut().ok()?;
                let state = s.as_mut()?;
                let mut activation = Activation(access_tree(&state.content, state.scale));
                state.access.handle_wm_getobject(wparam, lparam, &mut activation)
            });
            match answer {
                Some(a) => a.into(),
                // SAFETY: the default handling of a message of this window.
                None => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
            }
        }
        WM_DESTROY => {
            // SAFETY: ends this thread's message loop.
            unsafe { PostQuitMessage(0) };
            LRESULT(0)
        }
        // SAFETY: the default handling of a message of this window.
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_splash_is_centred_a_little_above_the_middle() {
        let work = RECT { left: 0, top: 0, right: 1920, bottom: 1040 };
        let p = placement(work, 872, 572);
        assert_eq!(p.x, (1920 - 872) / 2);
        assert!(p.y < (1040 - 572) / 2 && p.y > 0);
        // On a second monitor to the left.
        let p = placement(RECT { left: -2560, top: 0, right: 0, bottom: 1400 }, 872, 572);
        assert!(p.x > -2560 && p.x + 872 < 0);
        // Larger than the work area: pinned to its top-left corner.
        let p = placement(RECT { left: 0, top: 0, right: 800, bottom: 600 }, 1400, 900);
        assert_eq!((p.x, p.y), (0, 0));
    }

    #[test]
    fn the_status_is_a_polite_live_region() {
        let mut content = SplashContent::new(super::super::Artwork::Drive);
        content.status = "Chargement des réglages…".into();
        content.progress = Some(0.4);
        let tree = access_tree(&content, 1.75);
        assert_eq!(tree.nodes.len(), 3);
        let (_, root) = &tree.nodes[0];
        assert_eq!(root.label(), Some("Kubuno Drive"));
        let (_, status) = &tree.nodes[1];
        assert_eq!(status.label(), Some("Chargement des réglages…"));
        assert_eq!(status.live(), Some(Live::Polite));
        let (_, progress) = &tree.nodes[2];
        assert_eq!(progress.numeric_value(), Some(40.0));
    }

    #[test]
    fn every_artwork_renders_a_card_with_rounded_corners_and_a_shadow() {
        for art in super::super::Artwork::ALL {
            let mut content = SplashContent::new(art);
            content.version = "Version 0.1.0".into();
            content.progress = Some(0.5);
            for scale in [1.0, 1.75] {
                let still = render_still(&content, scale).expect("render");
                let px = |x: u32, y: u32| {
                    let i = ((y * still.width + x) * 4) as usize;
                    [still.pixels[i], still.pixels[i + 1], still.pixels[i + 2], still.pixels[i + 3]]
                };
                let m = still.margin;
                assert_eq!(px(0, 0)[3], 0, "{art:?}: the window's corner is transparent");
                assert!(px(m + 1, m + 1)[3] < 100, "{art:?}: the card's corner is rounded off (only the shadow there)");
                assert_eq!(px(still.width / 2, still.height / 2)[3], 255, "{art:?}: the card is opaque");
                // The shadow under the card's bottom edge.
                let below = px(still.width / 2, still.height - m + (6.0 * scale) as u32);
                assert!(below[3] > 0 && below[3] < 200, "{art:?}: a soft shadow, got {below:?}");
                // Artwork, not a flat fill: the hero side differs from the type side.
                let left = px(m + 20, still.height / 2);
                let right = px(still.width - m - (190.0 * scale) as u32, m + (232.0 * scale) as u32);
                assert_ne!(left, right, "{art:?}");
            }
        }
    }

    #[test]
    fn a_status_frame_over_the_strip_matches_a_full_paint() {
        let scale = 1.25;
        let margin = (MARGIN * scale).round() as i32;
        let size = SIZE { cx: (WIDTH * scale).round() as i32 + 2 * margin, cy: (HEIGHT * scale).round() as i32 + 2 * margin };
        let mut content = SplashContent::new(super::super::Artwork::Documents);
        content.progress = Some(0.3);
        let dpi = 96.0 * scale;
        let pixels = |s: &Surface| unsafe { std::slice::from_raw_parts(s.bits as *const u8, (s.width * s.height * 4) as usize).to_vec() };

        let surface = Surface::new(size.cx, size.cy).expect("surface");
        let rt = dc_target(&surface, dpi).expect("target");
        paint_window(&rt, &content, scale, margin, None).expect("first frame");
        let strip = capture_strip(&rt, &surface, scale, margin, dpi).expect("strip");
        // Another step first, then back: nothing of it may remain.
        let mut other = content.clone();
        other.status = "Une étape bien plus longue que la première, pour couvrir toute la ligne…".into();
        paint_status(&rt, &strip, &other, scale, Some(0.9), 0.2).expect("frame");
        paint_status(&rt, &strip, &content, scale, Some(0.3), 0.45).expect("frame");
        let framed = pixels(&surface);

        let reference = Surface::new(size.cx, size.cy).expect("surface");
        let rt2 = dc_target(&reference, dpi).expect("target");
        paint_window(&rt2, &content, scale, margin, Some((Some(0.3), 0.45))).expect("full paint");
        let full = pixels(&reference);

        let differing = framed.chunks(4).zip(full.chunks(4)).filter(|(a, b)| a.iter().zip(b.iter()).any(|(x, y)| x.abs_diff(*y) > 3)).count();
        assert!(differing * 2000 < framed.len() / 4, "{differing} pixels differ from a full paint");
    }

    #[test]
    fn the_process_age_is_known() {
        let age = process_age_ms().expect("process times");
        assert!(age < 24 * 3600 * 1000);
    }
}
