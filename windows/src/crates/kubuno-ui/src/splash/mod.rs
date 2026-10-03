//! The splash screen of a Kubuno desktop application: a large, artwork-led window shown the
//! moment the program starts, while it initialises, then faded out into the main window.
//!
//! ```no_run
//! use kubuno_ui::splash::{Artwork, SplashScreen};
//!
//! let splash = SplashScreen::new()
//!     .artwork(Artwork::Drive)
//!     .product("Kubuno Drive")
//!     .version(env!("CARGO_PKG_VERSION"))
//!     .license(env!("CARGO_PKG_LICENSE"))
//!     .show();
//! splash.step("Chargement des réglages…", 0.3);
//! // … heavy initialisation …
//! splash.step("Ouverture de la fenêtre…", 0.8);
//! // The splash fades out once the main window is shown (and it has been up for at least
//! // `min_duration`); `splash.close()` or `splash.close_when_window(hwnd)` say it explicitly.
//! ```
//!
//! # How it works
//!
//! * It lives on **its own thread**, with its own window and message loop: it appears within a
//!   few tens of milliseconds of the launch and keeps animating while the application's thread is
//!   busy initialising. The application only posts to it ([`Splash`] is `Send + Sync`).
//! * It is a **layered window** (per-pixel alpha): the card has antialiased rounded corners and a
//!   soft shadow, and fades in and out as a whole. It is painted with Direct2D into a DIB at the
//!   **launch monitor's DPI** (sharp at 100–200 %), on the monitor the application was started
//!   on (the one the shell hands over, else the one under the pointer), centred in its work area.
//! * It is **top-most but never activated**: it takes no focus from other applications, has no
//!   task bar button, and a click on it dismisses it.
//! * It closes **when the application shows a window**: by default the first visible top-level
//!   window of the process (a main window, or an error dialog that must not hide behind it), or
//!   the one named by [`Splash::close_when_window`] — never before [`SplashScreen::min_duration`]
//!   (1.5 s by default), and in any case after [`SplashScreen::max_duration`].
//! * **Accessibility**: UI Automation sees a window named after the product, its status line as
//!   a polite live region (Narrator reads each new step) and a progress indicator.
//! * It honours Windows' « animation effects » switch (no fade, no shimmer when it is off).
//!
//! # Turning it off
//!
//! [`is_disabled_by_user`]: the `--no-splash` command-line flag, the `KUBUNO_NO_SPLASH`
//! environment variable, or the per-user switch `HKCU\Software\Kubuno\Desktop`,
//! `SplashScreen` (DWORD) = 0 — then [`SplashScreen::show`] returns an inert [`Splash`].
//!
//! # The look
//!
//! The artwork ([`art`]) is procedural — gradients, layered shapes, light effects and a hero
//! rendition of the module's mark, all vector, nothing but code — one per application in the
//! colours of its web logo ([`Artwork`]). A splash screen has its OWN fixed look, like a box
//! cover: it is dark in the light theme and in the dark theme alike (a deep, saturated ground in
//! the module's colour family, white type), so it reads the same on every desktop and never
//! flashes white in a dark session.

pub mod art;
mod window;

use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub use art::{Artwork, Layout, Palette, Parts};
pub use window::{render_still, Still};

/// The window message the application's thread posts to wake the splash thread.
pub(crate) const WM_SPLASH_WAKE: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 0x51;

/// What the splash shows: the fixed parts (set by the builder) and the live ones (status,
/// progress) the application updates while it starts.
#[derive(Debug, Clone, PartialEq)]
pub struct SplashContent {
    pub artwork: Artwork,
    /// The full product name, « Kubuno Drive » (UI Automation's window name).
    pub product: String,
    /// The version line, « Version 0.1.0 » ([`format_version`]).
    pub version: String,
    /// A short line under the product name; empty for none.
    pub tagline: String,
    /// The live status line: the startup step in progress.
    pub status: String,
    /// The progress of the startup, `0.0..=1.0`; `None` while unknown (a shimmer runs instead).
    pub progress: Option<f32>,
    /// The legal line, « © Kubuno contributors · AGPL-3.0-or-later ».
    pub legal: String,
    /// A short credits line under it.
    pub credits: String,
}

impl SplashContent {
    /// The content of `artwork` with its defaults.
    pub fn new(artwork: Artwork) -> Self {
        Self {
            artwork,
            product: artwork.product().to_string(),
            version: String::new(),
            tagline: artwork.tagline().to_string(),
            status: format!("Démarrage de {}…", artwork.product()),
            progress: None,
            legal: legal_line("AGPL-3.0-or-later"),
            credits: DEFAULT_CREDITS.to_string(),
        }
    }

    /// The product name in its two typographic parts: the family (« Kubuno », set light) and the
    /// application (« Drive », set large). A name that does not start with « Kubuno » is all
    /// application.
    pub fn title_parts(&self) -> (String, String) {
        split_product(&self.product)
    }
}

/// The credits line every application shows unless it sets its own.
pub const DEFAULT_CREDITS: &str = "Logiciel libre et auto-hébergé, conçu par la communauté Kubuno.";

/// « © Kubuno contributors · <licence> ».
pub fn legal_line(license: &str) -> String {
    let license = license.trim();
    if license.is_empty() {
        "© Kubuno contributors".to_string()
    } else {
        format!("© Kubuno contributors · {license}")
    }
}

/// The version line: « Version 0.1.0-alpha », and the build identifier after it when there is
/// one (`X.Y.Z-<commits>.g<hash>`, the identifier `build_deb.sh` derives from git), without
/// repeating the version it starts with.
pub fn format_version(version: &str, build: Option<&str>) -> String {
    let version = version.trim();
    let base = if version.is_empty() { String::new() } else { format!("Version {version}") };
    let Some(build) = build.map(str::trim).filter(|b| !b.is_empty() && *b != version) else {
        return base;
    };
    // `0.1.0-42.gabc1234` under version `0.1.0`: show `42.gabc1234` only.
    let core = version.split(['-', '+']).next().unwrap_or(version);
    let short = build
        .strip_prefix(version)
        .or_else(|| build.strip_prefix(core))
        .map(|rest| rest.trim_start_matches(['-', '+', '.']))
        .filter(|rest| !rest.is_empty())
        .unwrap_or(build);
    if base.is_empty() {
        format!("Build {short}")
    } else {
        format!("{base} · build {short}")
    }
}

/// « Kubuno Drive » → (« Kubuno », « Drive »); « Notes » → (« », « Notes »).
pub fn split_product(product: &str) -> (String, String) {
    let product = product.trim();
    match product.split_once(' ') {
        Some((family, rest)) if family.eq_ignore_ascii_case("kubuno") && !rest.trim().is_empty() => (family.to_string(), rest.trim().to_string()),
        _ => (String::new(), product.to_string()),
    }
}

/// Whether the user turned the splash screen off: the `--no-splash` flag, the `KUBUNO_NO_SPLASH`
/// environment variable (any value but `0`), or `HKCU\Software\Kubuno\Desktop`, `SplashScreen`
/// (DWORD) = 0.
pub fn is_disabled_by_user() -> bool {
    if std::env::args_os().skip(1).any(|a| a == "--no-splash") {
        return true;
    }
    if std::env::var_os("KUBUNO_NO_SPLASH").is_some_and(|v| !v.is_empty() && v != "0") {
        return true;
    }
    window::registry_switch_off()
}

/// The startup timeline of a splash: when it appeared, when it may and must go, and its opacity
/// at any moment (milliseconds on one clock). Pure, so the close rules are testable.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Timeline {
    pub min_ms: u64,
    pub max_ms: u64,
    pub fade_in_ms: u64,
    pub fade_out_ms: u64,
    shown_at: Option<u64>,
    close_requested_at: Option<u64>,
    out_start: Option<u64>,
}

impl Timeline {
    pub fn new(min_ms: u64, max_ms: u64, fade_in_ms: u64, fade_out_ms: u64) -> Self {
        Self { min_ms, max_ms: max_ms.max(min_ms), fade_in_ms, fade_out_ms, shown_at: None, close_requested_at: None, out_start: None }
    }

    /// The window's first frame reached the screen.
    pub fn shown(&mut self, now: u64) {
        self.shown_at.get_or_insert(now);
    }

    /// The application is ready (its window is up): the splash goes once it has been shown for
    /// `min_ms`.
    pub fn request_close(&mut self, now: u64) {
        self.close_requested_at.get_or_insert(now);
    }

    /// A click: it goes now, whatever the minimum.
    pub fn dismiss(&mut self, now: u64) {
        self.out_start = Some(self.out_start.map_or(now, |s| s.min(now)));
    }

    /// Starts the fade-out when its time has come.
    pub fn update(&mut self, now: u64) {
        if self.out_start.is_some() {
            return;
        }
        let Some(shown) = self.shown_at else { return };
        if let Some(requested) = self.close_requested_at {
            let start = requested.max(shown.saturating_add(self.min_ms));
            if now >= start {
                self.out_start = Some(start);
                return;
            }
        }
        // Never stuck on screen: an application that shows nothing is past its splash anyway.
        let limit = shown.saturating_add(self.max_ms);
        if now >= limit {
            self.out_start = Some(limit);
        }
    }

    pub fn closing(&self) -> bool {
        self.out_start.is_some()
    }

    /// The opacity at `now`, `0..=1`: eased in, held, eased out.
    pub fn opacity(&self, now: u64) -> f32 {
        let Some(shown) = self.shown_at else { return 0.0 };
        let fade_in = ease_out_cubic(ratio(now.saturating_sub(shown), self.fade_in_ms));
        match self.out_start {
            Some(start) if now >= start => fade_in.min(1.0 - ease_in_out(ratio(now - start, self.fade_out_ms))),
            _ => fade_in,
        }
    }

    /// How far the entrance has gone, `0..=1` (the card rises into place as it fades in).
    pub fn entrance(&self, now: u64) -> f32 {
        self.shown_at.map_or(0.0, |shown| ease_out_cubic(ratio(now.saturating_sub(shown), self.fade_in_ms)))
    }

    /// The fade-out is over: the window can go.
    pub fn finished(&self, now: u64) -> bool {
        self.out_start.is_some_and(|start| now >= start.saturating_add(self.fade_out_ms))
    }

    /// Whether the window is still moving (fading or rising) at `now`.
    pub fn animating(&self, now: u64) -> bool {
        let entering = self.shown_at.is_some_and(|s| now < s.saturating_add(self.fade_in_ms));
        entering || self.out_start.is_some()
    }
}

fn ratio(elapsed: u64, total: u64) -> f32 {
    if total == 0 {
        1.0
    } else {
        (elapsed as f32 / total as f32).clamp(0.0, 1.0)
    }
}

fn ease_out_cubic(t: f32) -> f32 {
    1.0 - (1.0 - t).powi(3)
}

fn ease_in_out(t: f32) -> f32 {
    if t < 0.5 {
        4.0 * t * t * t
    } else {
        1.0 - (-2.0 * t + 2.0).powi(3) / 2.0
    }
}

/// The builder of a splash screen (Windows Forms has none; this is the shape of the usual
/// `SplashScreen` helpers): set what it shows, then [`show`](Self::show) it.
#[derive(Debug, Clone)]
pub struct SplashScreen {
    content: SplashContent,
    version: String,
    build: Option<String>,
    min: Duration,
    max: Duration,
    auto_close: bool,
    enabled: bool,
}

impl Default for SplashScreen {
    fn default() -> Self {
        Self::new()
    }
}

impl SplashScreen {
    /// A splash screen with the Kubuno artwork and its defaults.
    pub fn new() -> Self {
        Self {
            content: SplashContent::new(Artwork::Kubuno),
            version: String::new(),
            build: None,
            min: Duration::from_millis(1500),
            max: Duration::from_secs(30),
            auto_close: true,
            enabled: true,
        }
    }

    /// The artwork, and with it the default product name and tagline (set them after).
    pub fn artwork(mut self, artwork: Artwork) -> Self {
        let status_default = self.content.status == format!("Démarrage de {}…", self.content.product);
        if self.content.product == self.content.artwork.product() {
            self.content.product = artwork.product().to_string();
        }
        if self.content.tagline == self.content.artwork.tagline() {
            self.content.tagline = artwork.tagline().to_string();
        }
        self.content.artwork = artwork;
        if status_default {
            self.content.status = format!("Démarrage de {}…", self.content.product);
        }
        self
    }

    /// The product name, « Kubuno Drive ».
    pub fn product(mut self, name: impl Into<String>) -> Self {
        let status_default = self.content.status == format!("Démarrage de {}…", self.content.product);
        self.content.product = name.into();
        if status_default {
            self.content.status = format!("Démarrage de {}…", self.content.product);
        }
        self
    }

    /// The version, usually `env!("CARGO_PKG_VERSION")`.
    pub fn version(mut self, version: impl Into<String>) -> Self {
        self.version = version.into();
        self.content.version = format_version(&self.version, self.build.as_deref());
        self
    }

    /// The build identifier (`X.Y.Z-<commits>.g<hash>`), shown after the version.
    pub fn build(mut self, build: impl Into<String>) -> Self {
        self.build = Some(build.into());
        self.content.version = format_version(&self.version, self.build.as_deref());
        self
    }

    /// The licence on the legal line, usually `env!("CARGO_PKG_LICENSE")`.
    pub fn license(mut self, license: &str) -> Self {
        self.content.legal = legal_line(license);
        self
    }

    /// The line under the product name (empty: none).
    pub fn tagline(mut self, tagline: impl Into<String>) -> Self {
        self.content.tagline = tagline.into();
        self
    }

    /// The credits line under the legal line.
    pub fn credits(mut self, credits: impl Into<String>) -> Self {
        self.content.credits = credits.into();
        self
    }

    /// The first status line (« Démarrage de Kubuno Drive… » by default).
    pub fn status(mut self, status: impl Into<String>) -> Self {
        self.content.status = status.into();
        self
    }

    /// The shortest time the splash stays on screen once shown (1.5 s by default), so a fast
    /// start does not flash it.
    pub fn min_duration(mut self, min: Duration) -> Self {
        self.min = min;
        self
    }

    /// The longest (30 s by default): past it, it goes even if no window was shown.
    pub fn max_duration(mut self, max: Duration) -> Self {
        self.max = max;
        self
    }

    /// Whether it closes by itself when the process shows its first window (on by default). Off,
    /// only [`Splash::close`], [`Splash::close_when_window`] or a click close it.
    pub fn auto_close(mut self, on: bool) -> Self {
        self.auto_close = on;
        self
    }

    /// Whether to show it at all — the application's own condition (started hidden at logon,
    /// handing over to a running instance…). The user's switch ([`is_disabled_by_user`]) applies
    /// on top of it.
    pub fn enabled(mut self, on: bool) -> Self {
        self.enabled = on;
        self
    }

    /// What it will show.
    pub fn content(&self) -> &SplashContent {
        &self.content
    }

    /// Shows it — on its own thread, at once — and returns the handle the application drives it
    /// with. Disabled (by the application or the user), the handle is inert.
    pub fn show(self) -> Splash {
        if !self.enabled || is_disabled_by_user() {
            return Splash::disabled();
        }
        let started = window::process_age_ms();
        let inner = Arc::new(Inner {
            content: Mutex::new(self.content),
            hwnd: AtomicIsize::new(0),
            watch: AtomicIsize::new(if self.auto_close { WATCH_ANY } else { 0 }),
            close: AtomicBool::new(false),
            dismiss: AtomicBool::new(false),
            first_paint_ms: AtomicU64::new(u64::MAX),
            shown_after_ms: AtomicU64::new(started.unwrap_or(u64::MAX)),
            done: AtomicBool::new(false),
            min_ms: self.min.as_millis().min(u128::from(u64::MAX)) as u64,
            max_ms: self.max.as_millis().min(u128::from(u64::MAX)) as u64,
        });
        if window::spawn(inner.clone()).is_err() {
            return Splash::disabled();
        }
        Splash { inner: Some(inner) }
    }
}

/// `Inner::watch`: close on the first visible window of the process.
pub(crate) const WATCH_ANY: isize = -1;

/// The state the application's thread and the splash thread share.
pub(crate) struct Inner {
    pub content: Mutex<SplashContent>,
    /// The splash window, once created (0 before, and after it is gone).
    pub hwnd: AtomicIsize,
    /// What closes it: 0 nothing, [`WATCH_ANY`] the first visible window of the process, else
    /// that window.
    pub watch: AtomicIsize,
    /// The application asked it to go (after the minimum time).
    pub close: AtomicBool,
    /// It goes now.
    pub dismiss: AtomicBool,
    /// Milliseconds from the process start to the first frame on screen (`u64::MAX`: not yet).
    pub first_paint_ms: AtomicU64,
    /// The process age when `show` was called (`u64::MAX`: unknown).
    pub shown_after_ms: AtomicU64,
    /// The window is gone.
    pub done: AtomicBool,
    pub min_ms: u64,
    pub max_ms: u64,
}

impl Inner {
    fn wake(&self) {
        window::wake(self.hwnd.load(Ordering::Acquire));
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, SplashContent> {
        match self.content.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        }
    }
}

/// A window a splash can wait for ([`Splash::close_when`]): its handle once it exists.
pub trait SplashTarget {
    /// The window's handle (`HWND` as an integer), `None` while it is not created yet.
    fn splash_target_hwnd(&self) -> Option<isize>;
}

impl SplashTarget for windows::Win32::Foundation::HWND {
    fn splash_target_hwnd(&self) -> Option<isize> {
        (!self.is_invalid()).then_some(self.0 as isize)
    }
}

/// The handle of a shown splash screen. Cheap to clone, `Send + Sync`: any thread may update it.
/// Dropping it does not close the splash (it closes by itself, see the module documentation).
#[derive(Clone, Default)]
pub struct Splash {
    inner: Option<Arc<Inner>>,
}

impl std::fmt::Debug for Splash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Splash").field("shown", &self.is_shown()).finish()
    }
}

impl Splash {
    /// A handle that does nothing (the splash is turned off).
    pub fn disabled() -> Self {
        Self { inner: None }
    }

    /// Whether a splash was shown and is still on screen.
    pub fn is_shown(&self) -> bool {
        self.inner.as_ref().is_some_and(|i| !i.done.load(Ordering::Acquire))
    }

    /// The status line: the startup step in progress (« Chargement des polices… »).
    pub fn set_status(&self, status: impl Into<String>) {
        if let Some(inner) = &self.inner {
            inner.lock().status = status.into();
            inner.wake();
        }
    }

    /// The progress of the startup, `0.0..=1.0` (it only moves forward); a value outside, or
    /// NaN, puts back the indeterminate shimmer.
    pub fn set_progress(&self, progress: f32) {
        if let Some(inner) = &self.inner {
            {
                let mut c = inner.lock();
                c.progress = if (0.0..=1.0).contains(&progress) { Some(c.progress.map_or(progress, |p| p.max(progress))) } else { None };
            }
            inner.wake();
        }
    }

    /// [`set_status`](Self::set_status) and [`set_progress`](Self::set_progress) at once.
    pub fn step(&self, status: impl Into<String>, progress: f32) {
        if let Some(inner) = &self.inner {
            {
                let mut c = inner.lock();
                c.status = status.into();
                if (0.0..=1.0).contains(&progress) {
                    c.progress = Some(c.progress.map_or(progress, |p| p.max(progress)));
                }
            }
            inner.wake();
        }
    }

    /// The application is ready: the splash fades out once it has been shown for its minimum
    /// time.
    pub fn close(&self) {
        if let Some(inner) = &self.inner {
            inner.close.store(true, Ordering::Release);
            inner.wake();
        }
    }

    /// Closes it (after its minimum time) once window `hwnd` is visible on screen.
    pub fn close_when_window(&self, hwnd: isize) {
        if let Some(inner) = &self.inner {
            if hwnd != 0 {
                inner.watch.store(hwnd, Ordering::Release);
                inner.wake();
            }
        }
    }

    /// Closes it once `target` is shown — Windows Forms' habit of closing the splash when the main
    /// form appears. A target with no window yet (a form not opened) is waited for as the first
    /// window the process shows.
    pub fn close_when<T: SplashTarget + ?Sized>(&self, target: &T) {
        match target.splash_target_hwnd() {
            Some(hwnd) => self.close_when_window(hwnd),
            None => {
                if let Some(inner) = &self.inner {
                    inner.watch.store(WATCH_ANY, Ordering::Release);
                    inner.wake();
                }
            }
        }
    }

    /// Fades it out now, whatever its minimum time (what a click on it does).
    pub fn dismiss(&self) {
        if let Some(inner) = &self.inner {
            inner.dismiss.store(true, Ordering::Release);
            inner.wake();
        }
    }

    /// How long after the process started its first frame reached the screen.
    pub fn time_to_first_paint(&self) -> Option<Duration> {
        let ms = self.inner.as_ref()?.first_paint_ms.load(Ordering::Acquire);
        (ms != u64::MAX).then(|| Duration::from_millis(ms))
    }

    /// Waits up to `timeout` for the splash to be gone; `true` when it is (or never was).
    pub fn wait_closed(&self, timeout: Duration) -> bool {
        let Some(inner) = &self.inner else { return true };
        let deadline = std::time::Instant::now() + timeout;
        while !inner.done.load(Ordering::Acquire) {
            if std::time::Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        true
    }
}

#[cfg(test)]
mod tests;
