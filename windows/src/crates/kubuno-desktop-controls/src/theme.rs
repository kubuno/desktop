//! The **themed part** renderer — WinForms as Windows actually paints it today.
//!
//! ## Why this exists
//!
//! [`crate::system`] reads the classic visual facts: `GetSysColor`,
//! `GetSystemMetricsForDpi`, `lfMessageFont`. Those are the right source for a
//! machine with **visual styles off**, and the `DrawEdge` bevels built on them
//! are exactly what such a machine shows. They are *not* what the reference
//! sheets in `C:\kubuno-build\winforms-ref\shots\` show, because the toolkit
//! rendered them with visual styles **on** — and a themed control is painted by
//! `uxtheme.dll` from the msstyles file, not from the `COLOR_*` table.
//!
//! The gap is not a shade or two, and it cannot be closed by picking a better
//! system colour, because the colours simply are not there: pixel-sampling the
//! sheets gives a `Fixed3D` edit frame of `#ABADB3`, a button face of `#FDFDFD`
//! over a `#D0D0D0` top edge and a `#BABABA` bottom one, a `GroupBox` frame of
//! `#DCDCDC` — and **none** of those is any `GetSysColor` index on the same
//! machine. They live in the theme, so the only way to be identical *by
//! construction* rather than by approximation is to ask the theme.
//!
//! So this module renders the genuine part through `DrawThemeBackground` and
//! hands the pixels back as a Direct2D bitmap. The classic path stays exactly
//! where it was: it is the fallback, and on a themed-off machine it remains the
//! only thing that is right.
//!
//! ## The three traps, and what this file does about them
//!
//! **1. Alpha.** `DrawThemeBackground` draws through GDI, which has no notion of
//! an alpha channel: it writes RGB and leaves the fourth byte at whatever the
//! DIB happened to hold. A freshly created DIB section is zeroed, so a naive
//! upload produces a bitmap that is *entirely transparent* — the classic
//! symptom, and the reason this technique is usually reported as "not working".
//! Some parts are worse than transparent: the ones that antialias against their
//! ground write partial coverage into RGB and garbage into alpha, giving fringed
//! edges.
//!
//! The strategy here is **pre-fill + force-opaque**:
//!
//! * the DIB is filled with the caller-supplied `background` (opaque) *before*
//!   the part is drawn, so anything the part leaves untouched — the corners a
//!   rounded button does not cover, the ground a partially transparent part
//!   blends into — is already the exact colour that surrounds the control on
//!   screen;
//! * the part is drawn over it;
//! * every pixel's alpha byte is then forced to `0xFF`, so the resulting bitmap
//!   is opaque and blends as a plain blit.
//!
//! The documented alternative — `DrawThemeParentBackground` — is **not
//! available here and could not be**: it takes an `HWND` and asks *that
//! window's parent* to paint its background by sending it `WM_ERASEBKGND` /
//! `WM_PRINTCLIENT`. The controls in this crate are drawn, not windowed: there
//! is no child HWND whose parent could be asked. The caller-supplied background
//! is the honest substitute, and it is strictly more precise — it is the colour
//! the control's own painter is about to put around the part, not a guess made
//! by a window we do not have.
//!
//! `IsThemeBackgroundPartiallyTransparent` is still consulted, and it earns its
//! keep in the cache rather than in the drawing: a part that answers *no* covers
//! its whole rectangle opaquely, so the pre-fill cannot show through and the
//! background is **dropped from the cache key**. One `EDIT` frame is then shared
//! by every field on the page whatever ground they sit on, while a rounded
//! `BUTTON` face — which does answer *yes* — is correctly cached per ground.
//!
//! **2. Cost.** A GDI round trip (memory DC, DIB section, `DrawThemeBackground`,
//! `GdiFlush`, a D2D upload) is on the order of tens of microseconds. Doing it
//! per control per frame would cost milliseconds on a page of sixty controls —
//! visible, and for nothing, since the answer never changes. Everything is
//! therefore cached on `(class, part, state, width_px, height_px, dpi,
//! background)` and the steady state is a hash lookup plus a `DrawBitmap`.
//!
//! **3. Staleness.** Three things invalidate the cache, and all three are silent
//! failures if missed: a **DPI change** (the theme renders different geometry at
//! 144 DPI than at 96), `WM_THEMECHANGED` (the user switched theme or toggled
//! high contrast), and a **device rebuild** (a `ID2D1Bitmap1` belongs to the
//! device that created it; keeping one across a lost device is a crash waiting
//! for a resize). Each has an explicit entry point the host calls.
//!
//! ## Picking the part id is a measurement, not a lookup
//!
//! The obvious id is not always the right one, and the wrong one does not fail —
//! it draws something plausible. The `EDIT` class is the worked example: on
//! Windows 11 `EP_EDITBORDER_NOSCROLL` (6), the part the *window manager* uses
//! for a themed non-client edit border, renders the modern rounded frame
//! (`#ECECEC` over `#FEFEFE`) — which is **not** what the reference sheet shows.
//! `EP_EDITTEXT` (1) renders `#ABADB3` over `#FFFFFF`, which is exactly the
//! sheet, and is the element .NET's own `VisualStyleElement.TextBox.TextEdit`
//! names. Both are "the theme"; only one is the toolkit.
//!
//! So every part a family adopts is chosen by rendering it and sampling it
//! against the sheet — never by reading its name. The tests at the bottom of
//! this file pin the pixels for the two parts already adopted, so a Windows
//! update that moves them is a failing test rather than a slow drift.
//!
//! ## What a caller sees
//!
//! Nothing, normally: families call
//! [`crate::control::ControlCanvas::draw_theme_part`], which returns `false`
//! when theming is unavailable, and write
//!
//! ```ignore
//! if !c.draw_theme_part(theme::class::EDIT, EP_EDITBORDER_NOSCROLL, state, bounds, bg) {
//!     // the existing classic painting, unchanged
//! }
//! ```
//!
//! so the themed and the classic look are two branches of one paint, never two
//! copies of a control.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;

use kubuno_drive_desktop_app_controls::geometry::Rect;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_PIXEL_FORMAT, D2D_SIZE_U,
};
use windows::Win32::Graphics::Direct2D::{
    ID2D1Bitmap1, ID2D1DeviceContext, D2D1_BITMAP_OPTIONS_NONE, D2D1_BITMAP_PROPERTIES1,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GdiFlush, SelectObject,
    BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HBITMAP, HDC, HGDIOBJ,
};
use windows::Win32::UI::Controls::{
    CloseThemeData, DrawThemeBackground, IsAppThemed, IsThemeActive,
    IsThemeBackgroundPartiallyTransparent, HTHEME,
};
use windows::Win32::UI::HiDpi::OpenThemeDataForDpi;

// ─────────────────────────────────────────────────────────────────────────────
// The window classes
// ─────────────────────────────────────────────────────────────────────────────

/// The theme class names, as the families spell them at their call sites.
///
/// Constants rather than bare literals so a typo is a compile error instead of a
/// silently unthemed control: `OpenThemeData` answers `NULL` for an unknown
/// class, which looks exactly like "visual styles are off" and would send the
/// family quietly down the classic branch.
pub mod class {
    pub const BUTTON: &str = "BUTTON";
    pub const EDIT: &str = "EDIT";
    pub const COMBOBOX: &str = "COMBOBOX";
    pub const LISTVIEW: &str = "LISTVIEW";
    pub const TREEVIEW: &str = "TREEVIEW";
    pub const HEADER: &str = "HEADER";
    pub const SCROLLBAR: &str = "SCROLLBAR";
    pub const SPIN: &str = "SPIN";
    pub const TRACKBAR: &str = "TRACKBAR";
    pub const TAB: &str = "TAB";
    pub const PROGRESS: &str = "PROGRESS";
    pub const TOOLBAR: &str = "TOOLBAR";
    pub const MENU: &str = "MENU";
    pub const STATUS: &str = "STATUS";
    pub const DATEPICKER: &str = "DATEPICKER";
    pub const MONTHCAL: &str = "MONTHCAL";
}

/// The part ids the families draw with, as plain `i32`.
///
/// Re-exported from the Windows SDK's own constants rather than written as
/// numbers — a part id is exactly the kind of magic number that is copied wrong
/// once and then looks merely "a bit off" forever. Keeping them here also keeps
/// `windows::Win32::UI::Controls` out of the family files, which paint through
/// the `Canvas` and should not be naming Win32 types.
///
/// Only the parts actually adopted are listed; the next wave adds its own as it
/// measures them (see the module docs on why a part id is a measurement).
pub mod part {
    /// `EDIT` — the text field's frame and fill. **Not**
    /// `EP_EDITBORDER_NOSCROLL`: see the module docs.
    pub const EP_EDITTEXT: i32 = windows::Win32::UI::Controls::EP_EDITTEXT.0;
    /// `BUTTON` — a push button, including a check/radio in `Appearance::Button`.
    pub const BP_PUSHBUTTON: i32 = windows::Win32::UI::Controls::BP_PUSHBUTTON.0;
}

/// The state ids that go with [`part`].
pub mod state {
    use windows::Win32::UI::Controls as sdk;

    /// `EP_EDITTEXT` — an editable field.
    pub const ETS_NORMAL: i32 = sdk::ETS_NORMAL.0;
    /// `EP_EDITTEXT` — a field whose control is disabled.
    pub const ETS_DISABLED: i32 = sdk::ETS_DISABLED.0;
    /// `EP_EDITTEXT` — a field whose control is read-only.
    pub const ETS_READONLY: i32 = sdk::ETS_READONLY.0;

    /// `BP_PUSHBUTTON` — at rest.
    pub const PBS_NORMAL: i32 = sdk::PBS_NORMAL.0;
    /// `BP_PUSHBUTTON` — the pointer is over it.
    pub const PBS_HOT: i32 = sdk::PBS_HOT.0;
    /// `BP_PUSHBUTTON` — the pointer is down on it, or a toggle is checked.
    pub const PBS_PRESSED: i32 = sdk::PBS_PRESSED.0;
    /// `BP_PUSHBUTTON` — the control is disabled.
    pub const PBS_DISABLED: i32 = sdk::PBS_DISABLED.0;
    /// `BP_PUSHBUTTON` — the form's `AcceptButton`.
    pub const PBS_DEFAULTED: i32 = sdk::PBS_DEFAULTED.0;
}

/// One theme class, i.e. one `HTHEME`.
///
/// An enum rather than a string key because the handle table is a fixed-size
/// array indexed by the discriminant: opening a theme is a file-backed lookup,
/// and hashing a class name on every part draw would put a string hash in the
/// hot path for no benefit — the set is closed and known at compile time.
///
/// The sixteen members are exactly the classes the ten control families need
/// between them; nothing is listed speculatively.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ThemeClass {
    /// `BUTTON` — push buttons, check boxes, radio buttons, group boxes.
    Button,
    /// `EDIT` — the text-field frame and its background.
    Edit,
    /// `COMBOBOX` — the drop-down button and the combo's own frame.
    ComboBox,
    /// `LISTVIEW` — the list's frame, its items, and the group headers.
    ListView,
    /// `TREEVIEW` — the tree's frame, its items and the expand/collapse glyph.
    TreeView,
    /// `HEADER` — a `ListView`'s column header band.
    Header,
    /// `SCROLLBAR` — arrows, thumb, gripper and track.
    ScrollBar,
    /// `SPIN` — the up/down buttons of a `NumericUpDown`.
    Spin,
    /// `TRACKBAR` — the slider's track, thumb and ticks.
    TrackBar,
    /// `TAB` — the tab items and the body they sit on.
    Tab,
    /// `PROGRESS` — the bar's frame, its trough and its chunk.
    Progress,
    /// `TOOLBAR` — `ToolStrip` buttons and separators.
    Toolbar,
    /// `MENU` — the menu bar, drop-downs, check marks and separators.
    Menu,
    /// `STATUS` — the `StatusStrip` band, its panes and its grip.
    Status,
    /// `DATEPICKER` — the `DateTimePicker` frame and its drop-down button.
    DatePicker,
    /// `MONTHCAL` — the `MonthCalendar` grid, its header and its selection.
    MonthCal,
}

/// How many handles the table holds — one per [`ThemeClass`].
const CLASS_COUNT: usize = 16;

impl ThemeClass {
    /// Every class, in discriminant order. Used by [`ThemeRenderer::drop`] and
    /// by the tests; a family never iterates.
    pub const ALL: [ThemeClass; CLASS_COUNT] = [
        Self::Button,
        Self::Edit,
        Self::ComboBox,
        Self::ListView,
        Self::TreeView,
        Self::Header,
        Self::ScrollBar,
        Self::Spin,
        Self::TrackBar,
        Self::Tab,
        Self::Progress,
        Self::Toolbar,
        Self::Menu,
        Self::Status,
        Self::DatePicker,
        Self::MonthCal,
    ];

    /// The name Windows knows the class by — the same string as the matching
    /// [`class`] constant.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Button => class::BUTTON,
            Self::Edit => class::EDIT,
            Self::ComboBox => class::COMBOBOX,
            Self::ListView => class::LISTVIEW,
            Self::TreeView => class::TREEVIEW,
            Self::Header => class::HEADER,
            Self::ScrollBar => class::SCROLLBAR,
            Self::Spin => class::SPIN,
            Self::TrackBar => class::TRACKBAR,
            Self::Tab => class::TAB,
            Self::Progress => class::PROGRESS,
            Self::Toolbar => class::TOOLBAR,
            Self::Menu => class::MENU,
            Self::Status => class::STATUS,
            Self::DatePicker => class::DATEPICKER,
            Self::MonthCal => class::MONTHCAL,
        }
    }

    /// The same name as a NUL-terminated wide literal, which is what
    /// `OpenThemeData` takes.
    ///
    /// Built with `w!` rather than converted from [`ThemeClass::name`] at run
    /// time: a `HSTRING` per open would allocate, and these are compile-time
    /// constants in the binary's data segment.
    const fn wide(self) -> PCWSTR {
        match self {
            Self::Button => w!("BUTTON"),
            Self::Edit => w!("EDIT"),
            Self::ComboBox => w!("COMBOBOX"),
            Self::ListView => w!("LISTVIEW"),
            Self::TreeView => w!("TREEVIEW"),
            Self::Header => w!("HEADER"),
            Self::ScrollBar => w!("SCROLLBAR"),
            Self::Spin => w!("SPIN"),
            Self::TrackBar => w!("TRACKBAR"),
            Self::Tab => w!("TAB"),
            Self::Progress => w!("PROGRESS"),
            Self::Toolbar => w!("TOOLBAR"),
            Self::Menu => w!("MENU"),
            Self::Status => w!("STATUS"),
            Self::DatePicker => w!("DATEPICKER"),
            Self::MonthCal => w!("MONTHCAL"),
        }
    }

    /// Index into the handle table.
    const fn index(self) -> usize {
        match self {
            Self::Button => 0,
            Self::Edit => 1,
            Self::ComboBox => 2,
            Self::ListView => 3,
            Self::TreeView => 4,
            Self::Header => 5,
            Self::ScrollBar => 6,
            Self::Spin => 7,
            Self::TrackBar => 8,
            Self::Tab => 9,
            Self::Progress => 10,
            Self::Toolbar => 11,
            Self::Menu => 12,
            Self::Status => 13,
            Self::DatePicker => 14,
            Self::MonthCal => 15,
        }
    }

    /// The class a name refers to, or `None`.
    ///
    /// This is what turns the `&str` a family passes to
    /// [`crate::control::ControlCanvas::draw_theme_part`] back into a table
    /// index. Comparing sixteen short static strings is cheaper than hashing
    /// one, and — unlike a hash map — it cannot be built wrong.
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.name() == name)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// The cache
// ─────────────────────────────────────────────────────────────────────────────

/// What a cached part is keyed on.
///
/// The size is in **device pixels**, not DIP: two controls of the same DIP width
/// on displays at different scales are genuinely different renderings, and a
/// theme is free to change its geometry (not merely its scale) between them.
///
/// `background` is `None` for a part `IsThemeBackgroundPartiallyTransparent`
/// says covers its rectangle — see the module docs. Dropping it there is not an
/// optimisation detail: without it, a single `EDIT` frame would be re-rendered
/// once per distinct field ground on the page, for pixels that cannot differ.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct PartKey {
    class: usize,
    part: i32,
    state: i32,
    width_px: u32,
    height_px: u32,
    /// Rounded to the whole DPI; a display never reports a fractional one.
    dpi: u32,
    background: Option<u32>,
}

/// A part that has been rendered and uploaded — the bitmap, plus where it goes.
///
/// `dest` is carried with the bitmap rather than recomputed by the caller
/// because the two must agree exactly: the bitmap is `size_px` device pixels and
/// `dest` is the DIP rectangle those pixels land on 1:1. Letting a caller snap
/// the rectangle itself is how a themed border ends up resampled and blurry.
#[derive(Clone)]
pub struct CachedPart {
    /// The rendered pixels, opaque, ready to blit.
    pub bitmap: ID2D1Bitmap1,
    /// The **pixel-snapped** DIP rectangle the bitmap must be drawn into.
    pub dest: Rect,
    /// The size actually rendered, in device pixels.
    pub size_px: (u32, u32),
}

/// What the cache has been doing — for the probe and for a `debug_assert` that
/// a page is not thrashing it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ThemeCacheStats {
    /// Distinct parts currently held.
    pub entries: usize,
    /// Draws served from the cache.
    pub hits: u64,
    /// Draws that had to go through GDI.
    pub misses: u64,
}

// ─────────────────────────────────────────────────────────────────────────────
// The renderer
// ─────────────────────────────────────────────────────────────────────────────

/// Renders themed parts and caches them as Direct2D bitmaps.
///
/// Lives as long as the **window**, not as long as a frame: it holds theme
/// handles and GPU bitmaps, both of which would be pointless to rebuild sixty
/// times a second. The host owns one and lends it to each frame's `Painter`,
/// exactly as it already does with [`crate::system::Visuals`] — and for the same
/// reason, so the two have one lifetime story rather than two.
///
/// Deliberately **not** a field of `Visuals`: `Visuals` is a plain `Clone +
/// Debug` value read from the system with no device in sight, and hanging COM
/// handles off it would make cloning it duplicate GPU resources and printing it
/// meaningless. It is also not owned by the `Painter`, which is created and
/// dropped per frame — that would defeat the cache entirely, which is the whole
/// point of the file.
///
/// Everything is interior-mutable (`RefCell`/`Cell`) because painting takes
/// `&self` all the way down: a control paints through `&dyn ControlCanvas`, and
/// widening that to `&mut` for a cache write would ripple through ten families.
pub struct ThemeRenderer {
    /// One `HTHEME` per class, opened on first use. `None` means either "not
    /// asked for yet" or "asked for and refused"; [`ThemeRenderer::refused`]
    /// separates the two so a missing class is not re-opened every frame.
    handles: RefCell<[Option<HTHEME>; CLASS_COUNT]>,
    /// Classes whose `OpenThemeData` already failed, so the failure costs one
    /// call rather than one per frame.
    refused: RefCell<[bool; CLASS_COUNT]>,
    /// `IsThemeBackgroundPartiallyTransparent`, memoised per part: it is a
    /// property of the msstyles file, so it is constant for the life of the
    /// theme, and it is asked on the miss path of every uncached part.
    transparent: RefCell<HashMap<(usize, i32, i32), bool>>,
    /// The rendered parts.
    cache: RefCell<HashMap<PartKey, ID2D1Bitmap1>>,
    /// The DPI everything above was built at. Carried as renderer state rather
    /// than passed per call: the handles, the cached geometry and this number
    /// must agree, and three arguments that must agree are three chances to
    /// disagree.
    dpi: Cell<f32>,
    /// `IsAppThemed() && IsThemeActive()`, re-read on `WM_THEMECHANGED`.
    active: Cell<bool>,
    hits: Cell<u64>,
    misses: Cell<u64>,
}

impl Default for ThemeRenderer {
    fn default() -> Self {
        Self::new()
    }
}

impl ThemeRenderer {
    /// Opens nothing yet — the first [`ThemeRenderer::draw_part`] for a class
    /// opens that class, and a page that mounts no `TreeView` never pays for
    /// the `TREEVIEW` theme.
    pub fn new() -> Self {
        Self {
            handles: RefCell::new([None; CLASS_COUNT]),
            refused: RefCell::new([false; CLASS_COUNT]),
            transparent: RefCell::new(HashMap::new()),
            cache: RefCell::new(HashMap::new()),
            dpi: Cell::new(96.0),
            active: Cell::new(read_theme_active()),
            hits: Cell::new(0),
            misses: Cell::new(0),
        }
    }

    /// Whether visual styles are on for **this process**.
    ///
    /// Two questions, both of which must answer yes:
    ///
    /// * `IsThemeActive` — the *user* has a visual style applied at all (false
    ///   under the classic theme, and under some remote-session policies);
    /// * `IsAppThemed` — *this executable* is themed, which on Win32 means it
    ///   carries the ComCtl32 v6 dependency in its manifest. An unmanifested
    ///   binary gets `NULL` from `OpenThemeData` however themed the desktop is,
    ///   and the symptom — every control silently classic — looks exactly like a
    ///   themed-off machine. Asking both tells the two apart.
    ///
    /// …plus [`CLASSIC_ENV`], so the classic branch can be exercised on a themed
    /// machine.
    ///
    /// A caller uses this to decide nothing: the classic branch is chosen by
    /// [`crate::control::ControlCanvas::draw_theme_part`] returning `false`,
    /// which is also true for a class or part the theme does not define. This is
    /// for diagnostics and for the tests, which must not assert a themed pixel
    /// on a machine that cannot produce one.
    pub fn is_theme_active(&self) -> bool {
        self.active.get()
    }

    /// The DPI parts are rendered at. Rendering at the window's real DPI (rather
    /// than rendering at 96 and letting Direct2D scale) is the entire reason the
    /// result is crisp: `OpenThemeDataForDpi` gives the theme's own 144-DPI
    /// artwork, not a stretched 96-DPI one.
    ///
    /// Changing it invalidates **everything**: the handles were opened for the
    /// old DPI and the bitmaps were rendered at the old pixel size.
    pub fn set_dpi(&self, dpi: f32) {
        let dpi = dpi.max(1.0);
        if (self.dpi.get() - dpi).abs() < 0.5 {
            return;
        }
        self.dpi.set(dpi);
        self.close_handles();
        self.cache.borrow_mut().clear();
        self.transparent.borrow_mut().clear();
    }

    /// `WM_THEMECHANGED` — the user switched visual style, turned high contrast
    /// on, or logged a policy change in. Every handle is stale (the msstyles
    /// file behind it is gone) and every pixel was drawn from the old one.
    ///
    /// `IsAppThemed`/`IsThemeActive` are re-read here too: the message is
    /// precisely when they change, and a cached `true` would keep the library
    /// calling `DrawThemeBackground` after the theme has been switched off.
    pub fn on_theme_changed(&self) {
        self.active.set(read_theme_active());
        self.close_handles();
        self.cache.borrow_mut().clear();
        self.transparent.borrow_mut().clear();
    }

    /// The Direct2D device was rebuilt (a resize that recreated the swap chain,
    /// a device-removed reset).
    ///
    /// An `ID2D1Bitmap1` belongs to the device that created it; drawing one from
    /// a dead device is undefined and shows up as a crash inside `EndDraw`, far
    /// from the cause. The theme handles survive — only the GPU side is dropped.
    pub fn on_device_lost(&self) {
        self.cache.borrow_mut().clear();
    }

    /// Cache counters — see [`ThemeCacheStats`].
    pub fn stats(&self) -> ThemeCacheStats {
        ThemeCacheStats {
            entries: self.cache.borrow().len(),
            hits: self.hits.get(),
            misses: self.misses.get(),
        }
    }

    /// Renders (or recalls) one themed part sized for `rect_dip`.
    ///
    /// `background` is the colour that surrounds the part on screen — the ground
    /// the control's own painter has put, or is about to put, behind it. It is
    /// what the DIB is pre-filled with, so a part with rounded corners or a soft
    /// edge blends into the right colour instead of into black or into nothing;
    /// see the module docs for why this replaces `DrawThemeParentBackground`.
    ///
    /// Returns `None` — meaning "paint this the classic way" — when visual
    /// styles are off, when the class or part is not in the theme, when the
    /// rectangle is degenerate, or when any GDI/Direct2D step fails. A caller
    /// never has to distinguish those: all of them mean the same thing.
    ///
    /// The DPI is **not** a parameter (see [`ThemeRenderer::set_dpi`]).
    pub fn draw_part(
        &self,
        ctx: &ID2D1DeviceContext,
        class: ThemeClass,
        part: i32,
        state: i32,
        rect_dip: Rect,
        background: D2D1_COLOR_F,
    ) -> Option<CachedPart> {
        if !self.active.get() {
            return None;
        }
        let dpi = self.dpi.get();
        let scale = (dpi / 96.0).max(0.01);

        // Snap to the DEVICE pixel grid before deciding the bitmap's size, so
        // the rendered pixels land 1:1 on screen. A part rendered at 21 px and
        // drawn into a 20.6 px box is resampled, and a one-pixel theme border
        // resampled is a two-pixel grey smear — the exact artefact this whole
        // file exists to avoid.
        let left_px = (rect_dip.left * scale).round();
        let top_px = (rect_dip.top * scale).round();
        let right_px = (rect_dip.right * scale).round();
        let bottom_px = (rect_dip.bottom * scale).round();
        let width_px = (right_px - left_px) as i32;
        let height_px = (bottom_px - top_px) as i32;
        if width_px <= 0 || height_px <= 0 {
            return None;
        }
        // A part large enough to be a mistake (a docked control given the whole
        // window, say) is refused rather than turned into a multi-megabyte
        // cache entry that will never be hit twice.
        if width_px > MAX_PART_PX || height_px > MAX_PART_PX {
            return None;
        }

        let theme = self.handle(class)?;
        let opaque = self.covers_its_rect(theme, class, part, state);
        let key = PartKey {
            class: class.index(),
            part,
            state,
            width_px: width_px as u32,
            height_px: height_px as u32,
            dpi: dpi.round() as u32,
            background: if opaque { None } else { Some(bgra(background)) },
        };

        let dest = Rect::new(
            left_px / scale,
            top_px / scale,
            right_px / scale,
            bottom_px / scale,
        );

        if let Some(bitmap) = self.cache.borrow().get(&key) {
            self.hits.set(self.hits.get() + 1);
            return Some(CachedPart {
                bitmap: bitmap.clone(),
                dest,
                size_px: (key.width_px, key.height_px),
            });
        }

        let bitmap = render_part(
            ctx,
            theme,
            part,
            state,
            width_px,
            height_px,
            dpi,
            bgra(background),
        )?;
        self.misses.set(self.misses.get() + 1);
        self.cache.borrow_mut().insert(key, bitmap.clone());
        Some(CachedPart { bitmap, dest, size_px: (key.width_px, key.height_px) })
    }

    /// The handle for `class`, opening it on first use.
    ///
    /// A refusal is remembered: `OpenThemeData` on a class the current theme
    /// does not define is a file lookup that fails, and repeating it once per
    /// control per frame would be a real cost paid to learn the same "no".
    fn handle(&self, class: ThemeClass) -> Option<HTHEME> {
        let i = class.index();
        if let Some(h) = self.handles.borrow()[i] {
            return Some(h);
        }
        if self.refused.borrow()[i] {
            return None;
        }
        // `OpenThemeDataForDpi` rather than `OpenThemeData`: the second answers
        // for the *process's* DPI awareness context, so on a per-monitor-v2
        // process (which the host is) it hands back the 96-DPI artwork whatever
        // display the window is on — and a border drawn from it is a blurry
        // half-pixel at 150 %.
        let h = unsafe { OpenThemeDataForDpi(None, class.wide(), self.dpi.get().round() as u32) };
        if h.is_invalid() {
            self.refused.borrow_mut()[i] = true;
            return None;
        }
        self.handles.borrow_mut()[i] = Some(h);
        Some(h)
    }

    /// Whether the part paints its whole rectangle — memoised, see [`PartKey`].
    fn covers_its_rect(&self, theme: HTHEME, class: ThemeClass, part: i32, state: i32) -> bool {
        let k = (class.index(), part, state);
        if let Some(&v) = self.transparent.borrow().get(&k) {
            return v;
        }
        let opaque =
            !unsafe { IsThemeBackgroundPartiallyTransparent(theme, part, state) }.as_bool();
        self.transparent.borrow_mut().insert(k, opaque);
        opaque
    }

    fn close_handles(&self) {
        let mut handles = self.handles.borrow_mut();
        for slot in handles.iter_mut() {
            if let Some(h) = slot.take() {
                // A failed close leaks a handle and nothing else; there is no
                // recovery, and the process is about to ask for a fresh one.
                let _ = unsafe { CloseThemeData(h) };
            }
        }
        *self.refused.borrow_mut() = [false; CLASS_COUNT];
    }
}

impl Drop for ThemeRenderer {
    /// Theme handles are a per-process resource: leaking them keeps the
    /// msstyles file mapped for the life of the process, which is exactly the
    /// kind of leak that never shows up in a test and shows up in a long
    /// session.
    fn drop(&mut self) {
        self.close_handles();
    }
}

/// The largest part this renderer will rasterise, per side, in device pixels.
///
/// Not a limit of the technique — a guard. Every entry is a GPU bitmap held for
/// the life of the window, so one accidental full-window part at 4 K would cost
/// tens of megabytes for a single cache hit that never comes.
const MAX_PART_PX: i32 = 2048;

/// Setting this to anything makes the library behave as if visual styles were
/// off, so the classic fallback can be seen — and screenshotted — on a machine
/// that *is* themed.
///
/// The fallback is not a degraded mode: it is the whole rendering on a themed-off
/// machine, and it is the only one there. Without a switch it could only be
/// exercised by changing the desktop theme of the machine running the tests,
/// which is neither scriptable nor something to do to a developer's session — so
/// the branch would go untested by default, which is exactly how a fallback rots.
pub const CLASSIC_ENV: &str = "KUBUNO_CONTROLS_CLASSIC";

/// `IsAppThemed() && IsThemeActive()` — see [`ThemeRenderer::is_theme_active`].
fn read_theme_active() -> bool {
    if std::env::var_os(CLASSIC_ENV).is_some() {
        return false;
    }
    unsafe { IsThemeActive().as_bool() && IsAppThemed().as_bool() }
}

/// A Direct2D colour as one `0xAARRGGBB` word in **BGRA byte order**, which is
/// what a 32-bit DIB and `DXGI_FORMAT_B8G8R8A8_UNORM` both store.
///
/// Alpha is forced opaque: a themed part is composited against a ground, and a
/// translucent ground would let the window's own clear colour through — a
/// different question from the one the caller asked.
fn bgra(c: D2D1_COLOR_F) -> u32 {
    let ch = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u32;
    0xFF00_0000 | (ch(c.r) << 16) | (ch(c.g) << 8) | ch(c.b)
}

// ─────────────────────────────────────────────────────────────────────────────
// The GDI round trip
// ─────────────────────────────────────────────────────────────────────────────

/// A memory DC with a 32-bit top-down DIB section selected into it.
///
/// **Top-down** (`biHeight` negative) matters: a bottom-up DIB — the default —
/// stores its first row last, so uploading its bits to Direct2D unchanged gives
/// a vertically mirrored part. On a symmetric part that is invisible; on a
/// button with a darker bottom edge it silently moves the edge to the top.
///
/// RAII because the failure paths are many and each of the three resources
/// leaks independently: the DIB, the memory DC, and the object the DIB replaced
/// in it.
struct MemDib {
    dc: HDC,
    bitmap: HBITMAP,
    previous: HGDIOBJ,
    /// The DIB's own pixel storage, `width * height` words in BGRA order. Valid
    /// until the bitmap is deleted, i.e. until this guard drops.
    bits: *mut u32,
    len: usize,
}

impl MemDib {
    fn new(width: i32, height: i32) -> Option<Self> {
        // `CreateCompatibleDC(None)` is compatible with the application's
        // current screen, which is all a themed part needs — it never touches
        // the window's own DC, and using one would tie part rendering to a live
        // HWND for no gain.
        let dc = unsafe { CreateCompatibleDC(None) };
        if dc.is_invalid() {
            return None;
        }
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                // Negative: top-down. See the type's docs.
                biHeight: -height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
        let bitmap =
            match unsafe { CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut bits, None, 0) } {
                Ok(b) if !b.is_invalid() && !bits.is_null() => b,
                _ => {
                    let _ = unsafe { DeleteDC(dc) };
                    return None;
                }
            };
        let previous = unsafe { SelectObject(dc, bitmap.into()) };
        Some(Self {
            dc,
            bitmap,
            previous,
            bits: bits.cast::<u32>(),
            len: (width as usize) * (height as usize),
        })
    }

    /// The pixels, as words. Safe to hand out as a slice: the allocation is the
    /// DIB's, it is `len` words long by construction, and it outlives every
    /// borrow because only `Drop` frees it.
    ///
    /// `&mut self`, even though the pointer would not need it: the DIB's bits
    /// are the guard's own storage, so handing out a `&mut [u32]` from a `&self`
    /// would let two callers write the same pixels at once. Rust cannot see
    /// through the raw pointer to say so, which is exactly when the signature
    /// has to.
    fn pixels(&mut self) -> &mut [u32] {
        unsafe { std::slice::from_raw_parts_mut(self.bits, self.len) }
    }
}

impl Drop for MemDib {
    fn drop(&mut self) {
        unsafe {
            // The DIB must be deselected before it can be deleted — GDI refuses
            // to delete an object that is still selected, and the leak is
            // silent.
            SelectObject(self.dc, self.previous);
            let _ = DeleteObject(self.bitmap.into());
            let _ = DeleteDC(self.dc);
        }
    }
}

/// Renders one part into a DIB and uploads it as a Direct2D bitmap.
///
/// The alpha handling is the point; see the module docs. In order: pre-fill with
/// the opaque background, let the theme draw over it, flush GDI, force every
/// alpha byte opaque, upload.
#[allow(clippy::too_many_arguments)]
fn render_part(
    ctx: &ID2D1DeviceContext,
    theme: HTHEME,
    part: i32,
    state: i32,
    width_px: i32,
    height_px: i32,
    dpi: f32,
    background: u32,
) -> Option<ID2D1Bitmap1> {
    let mut dib = MemDib::new(width_px, height_px)?;

    // (1) Pre-fill. Whatever the part does not cover is already the colour that
    // surrounds it on screen, so a rounded corner blends into the form's face
    // rather than into the zeroed black a fresh DIB holds.
    dib.pixels().fill(background);

    // (2) The part itself, at the DIB's origin.
    let rect = RECT { left: 0, top: 0, right: width_px, bottom: height_px };
    if unsafe { DrawThemeBackground(theme, dib.dc, part, state, &rect, None) }.is_err() {
        return None;
    }

    // (3) GDI batches drawing calls per thread and flushes them lazily. Reading
    // the DIB's bits without flushing can therefore read them BEFORE the theme
    // has drawn — intermittently, under load, which is the worst way to find a
    // bug. One call removes the whole class.
    let _ = unsafe { GdiFlush() };

    // (4) Force opaque. `DrawThemeBackground` writes RGB through GDI, which has
    // no alpha channel: every pixel it touched now carries whatever alpha the
    // theme's own bitmaps happened to contain — often zero. Without this the
    // upload is a fully transparent (or edge-fringed) bitmap, which is the
    // single most common way this technique is reported as "not working".
    for p in dib.pixels().iter_mut() {
        *p |= 0xFF00_0000;
    }

    // (5) Upload. Premultiplied is the safe alpha mode to ask a device context
    // for, and with every alpha at 255 premultiplied and straight are the same
    // bytes — so nothing is being reinterpreted here, only labelled.
    let props = D2D1_BITMAP_PROPERTIES1 {
        pixelFormat: D2D1_PIXEL_FORMAT {
            format: DXGI_FORMAT_B8G8R8A8_UNORM,
            alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
        },
        dpiX: dpi,
        dpiY: dpi,
        bitmapOptions: D2D1_BITMAP_OPTIONS_NONE,
        colorContext: std::mem::ManuallyDrop::new(None),
    };
    let size = D2D_SIZE_U { width: width_px as u32, height: height_px as u32 };
    unsafe {
        ctx.CreateBitmap(
            size,
            Some(dib.bits.cast::<core::ffi::c_void>()),
            (width_px as u32) * 4,
            &props,
        )
    }
    .ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::UI::Controls::{
        BP_PUSHBUTTON, EP_EDITTEXT, ETS_DISABLED, ETS_NORMAL, PBS_DISABLED, PBS_NORMAL,
    };

    /// Renders a part into a DIB and returns its pixels — the same GDI path
    /// `render_part` uses, stopping short of the Direct2D upload so the
    /// assertions need no device, no window and no swap chain.
    ///
    /// `None` when theming is unavailable, which is a **skip**, not a failure:
    /// on a themed-off machine the classic path is the correct rendering and
    /// there is no themed pixel to assert.
    fn sample(class: ThemeClass, part: i32, state: i32, w: i32, h: i32) -> Option<Vec<u32>> {
        if !read_theme_active() {
            return None;
        }
        let theme = unsafe { OpenThemeDataForDpi(None, class.wide(), 96) };
        if theme.is_invalid() {
            return None;
        }
        let mut dib = MemDib::new(w, h)?;
        // The reference sheets were captured on `COLOR_BTNFACE` (#F0F0F0), which
        // is the ground a form puts behind both of these controls — so the
        // pre-fill here is the same ground the sheet has, and a sampled pixel is
        // comparable with it directly.
        dib.pixels().fill(0xFFF0_F0F0);
        let rect = RECT { left: 0, top: 0, right: w, bottom: h };
        let drawn = unsafe { DrawThemeBackground(theme, dib.dc, part, state, &rect, None) }.is_ok();
        let _ = unsafe { GdiFlush() };
        let out = drawn.then(|| dib.pixels().to_vec());
        let _ = unsafe { CloseThemeData(theme) };
        out
    }

    /// `#RRGGBB` for a BGRA word, so a failure prints the colour a human can
    /// compare with the reference sheet rather than a decimal.
    fn hex(px: u32) -> String {
        format!("#{:02X}{:02X}{:02X}", (px >> 16) & 0xFF, (px >> 8) & 0xFF, px & 0xFF)
    }

    #[test]
    fn the_class_names_round_trip() {
        for c in ThemeClass::ALL {
            assert_eq!(ThemeClass::from_name(c.name()), Some(c), "{}", c.name());
        }
        assert_eq!(ThemeClass::from_name("NOPE"), None);
        // The table index must be unique, or two classes share a handle.
        let mut seen = [false; CLASS_COUNT];
        for c in ThemeClass::ALL {
            assert!(!seen[c.index()], "duplicate index for {}", c.name());
            seen[c.index()] = true;
        }
    }

    /// A `COLORREF` is `0x00BBGGRR` but a DIB word is `0xAARRGGBB`; getting the
    /// two the same way round is the difference between a blue selection and an
    /// orange one.
    #[test]
    fn a_colour_becomes_an_opaque_bgra_word() {
        let red = D2D1_COLOR_F { r: 1.0, g: 0.0, b: 0.0, a: 1.0 };
        assert_eq!(bgra(red), 0xFFFF_0000);
        let face = D2D1_COLOR_F { r: 240.0 / 255.0, g: 240.0 / 255.0, b: 240.0 / 255.0, a: 1.0 };
        assert_eq!(bgra(face), 0xFFF0_F0F0);
        // A translucent request still yields an opaque ground.
        let ghost = D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 0.0 };
        assert_eq!(bgra(ghost) >> 24, 0xFF);
    }

    /// The finding that forced this file: the `Fixed3D` edit frame the reference
    /// sheet shows is `#ABADB3`, which is no `GetSysColor` index — and the theme
    /// produces it exactly, byte for byte.
    ///
    /// Sampled at `(0, 4)`: the left edge, four rows down, i.e. clear of the
    /// corner where a theme may round or blend. `(1, 4)` is the frame's inner
    /// pad, which the sheet shows as `Window` white even on a field whose client
    /// area is `Control` grey — the reason `text.rs` repaints that one ring.
    #[test]
    fn the_edit_frame_is_the_reference_border() {
        let Some(px) = sample(ThemeClass::Edit, EP_EDITTEXT.0, ETS_NORMAL.0, 120, 23) else {
            eprintln!("[theme] visual styles unavailable — themed border not asserted");
            return;
        };
        assert_eq!(hex(px[4 * 120]), "#ABADB3", "left edge of EP_EDITTEXT");
        assert_eq!(hex(px[4 * 120 + 1]), "#FFFFFF", "inner pad of EP_EDITTEXT");
    }

    /// The frame is the SAME line in every state — measured, not assumed.
    ///
    /// It is what lets `text.rs` pass the control's real state (`ETS_DISABLED`
    /// for a dead field) without the border moving, and it matches the sheet,
    /// whose read-only and disabled fields are framed in the same `#ABADB3` as
    /// the editable one. The fill behind it does move, which is exactly why the
    /// control repaints the pad rather than trusting it.
    #[test]
    fn the_edit_frame_is_the_same_line_in_every_state() {
        let (Some(normal), Some(disabled)) = (
            sample(ThemeClass::Edit, EP_EDITTEXT.0, ETS_NORMAL.0, 120, 23),
            sample(ThemeClass::Edit, EP_EDITTEXT.0, ETS_DISABLED.0, 120, 23),
        ) else {
            eprintln!("[theme] visual styles unavailable — themed states not asserted");
            return;
        };
        assert_eq!(hex(disabled[4 * 120]), "#ABADB3", "ETS_DISABLED frame");
        assert_eq!(hex(normal[4 * 120]), hex(disabled[4 * 120]), "the frame is state-invariant");
        assert_ne!(normal[4 * 120 + 1], disabled[4 * 120 + 1], "the FILL is not");
    }

    /// The other half of the finding: the button face is `#FDFDFD` over a
    /// `#D0D0D0` top edge and a `#BABABA` bottom one, and not one of the three
    /// is a system colour.
    ///
    /// It also pins the geometry `buttons.rs` depends on: the part keeps a
    /// **one-pixel transparent margin** all round — index 0 is still the
    /// pre-fill — so the visible border sits at index 1 and a caption must be
    /// inset by two device pixels, not one.
    #[test]
    fn the_push_button_is_the_reference_face_inside_a_one_pixel_margin() {
        let (w, h) = (120usize, 33usize);
        let Some(px) = sample(ThemeClass::Button, BP_PUSHBUTTON.0, PBS_NORMAL.0, w as i32, h as i32)
        else {
            eprintln!("[theme] visual styles unavailable — themed face not asserted");
            return;
        };
        let mid = h / 2;
        assert_eq!(hex(px[mid * w]), "#F0F0F0", "the outermost ring is the part's own margin");
        assert_eq!(hex(px[mid * w + 1]), "#D0D0D0", "the border");
        assert_eq!(hex(px[mid * w + w / 2]), "#FDFDFD", "the face");
        assert_eq!(hex(px[w + w / 2]), "#D0D0D0", "the top edge");
        assert_eq!(hex(px[(h - 2) * w + w / 2]), "#BABABA", "the bottom edge, darker than the top");
    }

    /// A disabled push button is a different face, not a greyed caption over the
    /// same one — proof the state id reaches the theme.
    #[test]
    fn a_disabled_push_button_has_its_own_face() {
        let (w, h) = (120usize, 33usize);
        let Some(px) =
            sample(ThemeClass::Button, BP_PUSHBUTTON.0, PBS_DISABLED.0, w as i32, h as i32)
        else {
            eprintln!("[theme] visual styles unavailable — themed face not asserted");
            return;
        };
        let mid = h / 2;
        assert_eq!(hex(px[mid * w + 1]), "#E9E9E9", "disabled border");
        assert_eq!(hex(px[mid * w + w / 2]), "#F9F9F9", "disabled face");
    }
}
