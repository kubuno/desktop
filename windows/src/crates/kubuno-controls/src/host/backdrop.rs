//! The backdrop of a floating panel window ([`super::form::FloatingPanel`]): what is behind the
//! window, blurred by the compositor, clipped to the panel's own corner radius, with the
//! renderer's (transparent) swap chain on top — the web's frosted floating panels (the app
//! launcher, the account panel) as real windows that may extend past their owner.
//!
//! Ported from the shell's `flyout_window.rs`, where every dead end below was hit first:
//!
//! * `DWMWA_SYSTEMBACKDROP_TYPE` (`DWMSBT_TRANSIENTWINDOW`) only blurs while its window is ACTIVE,
//!   and DWM rounds it at its own fixed radius.
//! * `DwmExtendFrameIntoClientArea(-1)` paints the extended frame as an OPAQUE sheet behind the
//!   content, hiding the blur.
//! * `SetWindowCompositionAttribute` (legacy acrylic) fills the window RECTANGLE; a window region
//!   does not clip a `WS_EX_NOREDIRECTIONBITMAP` window.
//!
//! Windows.UI.Composition has none of these limits: a host backdrop brush is blurred by the
//! compositor, and a geometric clip built from a rounded rectangle cuts the corners at any radius.
//! The window keeps a margin around the panel ([`super::form::FloatingPanel::shadow_margin`]) for
//! the drop shadow the page's renderer paints there.

use windows::core::{Interface, Result};
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Dxgi::IDXGISwapChain1;
use windows::UI::Composition::{
    CompositionRoundedRectangleGeometry, CompositionStretch, Compositor, ContainerVisual, Desktop::DesktopWindowTarget,
    SpriteVisual,
};
use windows::Win32::System::WinRT::Composition::{ICompositorDesktopInterop, ICompositorInterop};

/// The composition tree of one floating panel window (see the module doc).
pub(crate) struct PanelBackdrop {
    _target: DesktopWindowTarget,
    /// Laid out in LOGICAL units; its scale maps them to the window's physical pixels (the target
    /// applies no DPI scaling of its own — measured: content sized in logical units landed at
    /// exactly `size / scale` physical pixels).
    root: ContainerVisual,
    /// The blurred, rounded panel, inset by the shadow margin. The clip lives here rather than on
    /// the root, so the shadow drawn in the margin is not cut away with the corners.
    panel: ContainerVisual,
    blur: SpriteVisual,
    /// The swap chain, over the whole window: it carries the shadow in the margin as well as the
    /// page.
    content: SpriteVisual,
    geometry: CompositionRoundedRectangleGeometry,
    radius: f32,
    margin: f32,
    scale: f32,
}

impl PanelBackdrop {
    /// Builds the tree for `hwnd` (created with `WS_EX_NOREDIRECTIONBITMAP`) over `swapchain` (a
    /// renderer made with `Renderer::new_detached`). Fails when the system gives no backdrop feed:
    /// the caller then paints the panel opaquely.
    pub(crate) fn new(hwnd: HWND, swapchain: &IDXGISwapChain1, radius: f32, margin: f32, scale: f32) -> Result<Self> {
        ensure_dispatcher_queue();
        if !enable_host_backdrop(hwnd) {
            // No feed, no blur: a host backdrop brush would paint black.
            return Err(windows::core::Error::from(windows::Win32::Foundation::E_NOTIMPL));
        }
        let compositor = Compositor::new()?;
        // `isTopmost = false`: the composition layer sits between the window and its children,
        // which is where a backdrop belongs.
        let interop: ICompositorDesktopInterop = compositor.cast()?;
        // SAFETY: `hwnd` is the caller's live window, created on this thread.
        let target = unsafe { interop.CreateDesktopWindowTarget(hwnd, false)? };

        let root = compositor.CreateContainerVisual()?;
        root.SetScale(windows_numerics::Vector3 { X: scale, Y: scale, Z: 1.0 })?;
        target.SetRoot(&root)?;

        let panel = compositor.CreateContainerVisual()?;
        root.Children()?.InsertAtTop(&panel)?;
        // A host backdrop brush is ALREADY blurred by the system compositor (feeding it through a
        // Gaussian effect graph rendered nothing at all). The page paints its own translucent
        // ground over it: the tint is the page's `BackColor`.
        let blur = compositor.CreateSpriteVisual()?;
        blur.SetBrush(&compositor.CreateHostBackdropBrush()?)?;
        panel.Children()?.InsertAtTop(&blur)?;

        let surface_interop: ICompositorInterop = compositor.cast()?;
        // SAFETY: the swap chain belongs to the caller's renderer, which outlives this tree.
        let surface = unsafe { surface_interop.CreateCompositionSurfaceForSwapChain(swapchain)? };
        let brush = compositor.CreateSurfaceBrushWithSurface(&surface)?;
        brush.SetStretch(CompositionStretch::Fill)?;
        let content = compositor.CreateSpriteVisual()?;
        content.SetBrush(&brush)?;
        root.Children()?.InsertAtTop(&content)?;

        let geometry = compositor.CreateRoundedRectangleGeometry()?;
        let clip = compositor.CreateGeometricClipWithGeometry(&geometry)?;
        panel.SetClip(&clip)?;

        Ok(Self { _target: target, root, panel, blur, content, geometry, radius, margin, scale })
    }

    /// Sizes the tree to the window's `w × h` physical pixels.
    pub(crate) fn resize(&self, w: u32, h: u32) {
        let window = windows_numerics::Vector2 { X: w as f32 / self.scale, Y: h as f32 / self.scale };
        let _ = self.root.SetSize(window);
        let _ = self.content.SetSize(window);
        let panel = windows_numerics::Vector2 {
            X: (window.X - self.margin * 2.0).max(0.0),
            Y: (window.Y - self.margin * 2.0).max(0.0),
        };
        let _ = self.panel.SetOffset(windows_numerics::Vector3 { X: self.margin, Y: self.margin, Z: 0.0 });
        let _ = self.panel.SetSize(panel);
        let _ = self.blur.SetSize(panel);
        let _ = self.geometry.SetSize(panel);
        let _ = self.geometry.SetCornerRadius(windows_numerics::Vector2 { X: self.radius, Y: self.radius });
    }

    /// Follows a DPI change (the tree is laid out in logical units).
    pub(crate) fn set_scale(&mut self, scale: f32, w: u32, h: u32) {
        self.scale = scale;
        let _ = self.root.SetScale(windows_numerics::Vector3 { X: scale, Y: scale, Z: 1.0 });
        self.resize(w, h);
    }
}

/// One dispatcher queue per thread is required before a `Compositor` exists; kept for the thread's
/// life (the windows outlive any single call).
fn ensure_dispatcher_queue() {
    use windows::Win32::System::WinRT::{CreateDispatcherQueueController, DispatcherQueueOptions, DQTAT_COM_NONE, DQTYPE_THREAD_CURRENT};
    thread_local! {
        static QUEUE: std::cell::RefCell<Option<windows::System::DispatcherQueueController>> = const { std::cell::RefCell::new(None) };
    }
    QUEUE.with(|q| {
        let mut q = q.borrow_mut();
        if q.is_some() {
            return;
        }
        let options = DispatcherQueueOptions {
            dwSize: std::mem::size_of::<DispatcherQueueOptions>() as u32,
            threadType: DQTYPE_THREAD_CURRENT,
            apartmentType: DQTAT_COM_NONE,
        };
        // SAFETY: plain WinRT call with a filled-in options struct.
        *q = unsafe { CreateDispatcherQueueController(options) }.ok();
    });
}

#[repr(C)]
struct AccentPolicy {
    accent_state: u32,
    accent_flags: u32,
    gradient_color: u32,
    animation_id: u32,
}

#[repr(C)]
struct WindowCompositionAttribData {
    attrib: u32,
    data: *mut std::ffi::c_void,
    size: usize,
}

/// Opens the host-backdrop feed for `hwnd`: without it `CreateHostBackdropBrush` has nothing to
/// sample and paints BLACK — the compositor only receives what is behind a window once the window
/// asks DWM for it (`ACCENT_ENABLE_HOSTBACKDROP`, through the undocumented but long-stable
/// `SetWindowCompositionAttribute`). False when the entry point is missing.
fn enable_host_backdrop(hwnd: HWND) -> bool {
    use windows::core::{s, PCSTR};
    use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryA};
    const WCA_ACCENT_POLICY: u32 = 19;
    const ACCENT_ENABLE_HOSTBACKDROP: u32 = 5;
    // SAFETY: the procedure is looked up by name in user32 and called with the documented layout
    // of its two structures, both alive for the call.
    unsafe {
        let Ok(user32) = LoadLibraryA(s!("user32.dll")) else { return false };
        let Some(proc) = GetProcAddress(user32, PCSTR(c"SetWindowCompositionAttribute".as_ptr() as *const u8)) else {
            return false;
        };
        let set_attr: extern "system" fn(HWND, *mut WindowCompositionAttribData) -> i32 = std::mem::transmute(proc);
        let mut policy = AccentPolicy { accent_state: ACCENT_ENABLE_HOSTBACKDROP, accent_flags: 0, gradient_color: 0, animation_id: 0 };
        let mut data = WindowCompositionAttribData {
            attrib: WCA_ACCENT_POLICY,
            data: &mut policy as *mut _ as *mut _,
            size: std::mem::size_of::<AccentPolicy>(),
        };
        set_attr(hwnd, &mut data) != 0
    }
}
