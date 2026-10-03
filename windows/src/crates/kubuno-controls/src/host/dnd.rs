//! Drag and drop — WinForms' `AllowDrop` / `DoDragDrop` over OLE.
//!
//! **As a target.** A page that has a drop target this frame calls [`accept_drops`]; the host then
//! registers the window with OLE (`RegisterDragDrop`, once). When something is dragged over the
//! window — files from the Explorer, text from a browser, data from another Kubuno window or from
//! this one — OLE calls the window's drop target, which records the drag ([`DragFrame`]: its
//! phase, data, allowed effects, pointer and keys) and **renders a frame at once**, so the page's
//! `DragEnter`/`DragOver`/`DragDrop`/`DragLeave` handlers run and answer with [`set_effect`] before
//! OLE is answered — the cursor shows what a drop would do while the pointer is still moving.
//!
//! **As a source.** [`do_drag_drop`] asks for a drag of a [`DataObject`]: it starts right after the
//! frame that asked (never inside a paint: OLE runs a modal loop, during which this window keeps
//! rendering as a target), and the effect the target chose is handed to the request's callback.
//! Text, file lists (`CF_HDROP`: files can be dropped onto the Explorer) and custom formats (by name,
//! as registered clipboard formats) are offered.
//!
//! The per-frame state and the phase machine ([`Tracker`]) are plain Rust and unit-tested; the OLE
//! objects live in [`ole`].

use std::cell::{Cell, RefCell};
use std::ops::{BitAnd, BitOr, BitOrAssign};
use std::path::PathBuf;
use std::rc::Rc;

use super::input::Modifiers;

/// The operations a drag allows or a drop target accepts (WinForms `DragDropEffects`, a bit set).
///
/// ```
/// use kubuno_controls::host::dnd::DragDropEffects;
///
/// let allowed = DragDropEffects::COPY | DragDropEffects::MOVE;
/// assert!(allowed.contains(DragDropEffects::MOVE));
/// assert!(!allowed.contains(DragDropEffects::LINK));
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct DragDropEffects(pub u8);

impl DragDropEffects {
    pub const NONE: Self = Self(0);
    pub const COPY: Self = Self(1);
    pub const MOVE: Self = Self(2);
    pub const LINK: Self = Self(4);
    pub const SCROLL: Self = Self(8);
    pub const ALL: Self = Self(1 | 2 | 4 | 8);

    /// Whether every bit of `other` is set.
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// Whether no bit is set.
    pub const fn is_none(self) -> bool {
        self.0 == 0
    }

    /// The OLE `DROPEFFECT` bits.
    pub const fn to_ole(self) -> u32 {
        let mut v = 0u32;
        if self.0 & 1 != 0 {
            v |= 1;
        }
        if self.0 & 2 != 0 {
            v |= 2;
        }
        if self.0 & 4 != 0 {
            v |= 4;
        }
        if self.0 & 8 != 0 {
            v |= 0x8000_0000;
        }
        v
    }

    /// From OLE `DROPEFFECT` bits.
    pub const fn from_ole(v: u32) -> Self {
        let mut e = 0u8;
        if v & 1 != 0 {
            e |= 1;
        }
        if v & 2 != 0 {
            e |= 2;
        }
        if v & 4 != 0 {
            e |= 4;
        }
        if v & 0x8000_0000 != 0 {
            e |= 8;
        }
        Self(e)
    }

    /// The one effect a drop performs, among `self`, for the keys held — Windows' convention:
    /// Ctrl copies, Shift moves, Ctrl+Shift links, otherwise move when allowed, else copy, else link.
    pub fn pick(self, mods: Modifiers) -> Self {
        let choice = match (mods.ctrl, mods.shift) {
            (true, true) => Self::LINK,
            (true, false) => Self::COPY,
            (false, true) => Self::MOVE,
            (false, false) if self.contains(Self::MOVE) => Self::MOVE,
            (false, false) if self.contains(Self::COPY) => Self::COPY,
            (false, false) => Self::LINK,
        };
        if self.contains(choice) {
            choice
        } else {
            [Self::MOVE, Self::COPY, Self::LINK].into_iter().find(|e| self.contains(*e)).unwrap_or(Self::NONE)
        }
    }
}

impl BitOr for DragDropEffects {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

impl BitOrAssign for DragDropEffects {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

impl BitAnd for DragDropEffects {
    type Output = Self;
    fn bitand(self, rhs: Self) -> Self {
        Self(self.0 & rhs.0)
    }
}

/// The data being dragged (WinForms `DataObject`): text, a file list and any custom formats, by
/// name (a custom format travels as a registered clipboard format of that name).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DataObject {
    /// Plain text (`CF_UNICODETEXT`, WinForms `DataFormats.UnicodeText`).
    pub text: Option<String>,
    /// Files (`CF_HDROP`, `DataFormats.FileDrop`).
    pub files: Vec<PathBuf>,
    /// Any other format, by name, as raw bytes.
    pub custom: Vec<(String, Vec<u8>)>,
}

impl DataObject {
    pub fn new() -> Self {
        Self::default()
    }

    /// Text data (`new DataObject(DataFormats.UnicodeText, text)`).
    pub fn from_text(text: impl Into<String>) -> Self {
        Self { text: Some(text.into()), ..Self::default() }
    }

    /// A file list (`DataFormats.FileDrop`).
    pub fn from_files(files: impl IntoIterator<Item = PathBuf>) -> Self {
        Self { files: files.into_iter().collect(), ..Self::default() }
    }

    /// Adds (or replaces) the custom format `name` (`SetData(name, bytes)`).
    pub fn with_custom(mut self, name: &str, bytes: Vec<u8>) -> Self {
        self.set_custom(name, bytes);
        self
    }

    pub fn set_custom(&mut self, name: &str, bytes: Vec<u8>) {
        match self.custom.iter_mut().find(|(n, _)| n == name) {
            Some(slot) => slot.1 = bytes,
            None => self.custom.push((name.to_string(), bytes)),
        }
    }

    /// The bytes of the custom format `name`, if present.
    pub fn get(&self, name: &str) -> Option<&[u8]> {
        self.custom.iter().find(|(n, _)| n == name).map(|(_, b)| b.as_slice())
    }

    /// Whether the format `name` is present (`GetDataPresent`): `"Text"`/`"UnicodeText"`,
    /// `"FileDrop"`, or a custom name.
    pub fn has_format(&self, name: &str) -> bool {
        match name {
            "Text" | "UnicodeText" | "StringFormat" => self.text.is_some(),
            "FileDrop" => !self.files.is_empty(),
            other => self.get(other).is_some(),
        }
    }

    /// The formats present, WinForms names first (`GetFormats`).
    pub fn formats(&self) -> Vec<String> {
        let mut out = Vec::new();
        if self.text.is_some() {
            out.push("UnicodeText".to_string());
        }
        if !self.files.is_empty() {
            out.push("FileDrop".to_string());
        }
        out.extend(self.custom.iter().map(|(n, _)| n.clone()));
        out
    }

    /// Whether the object carries nothing.
    pub fn is_empty(&self) -> bool {
        self.text.is_none() && self.files.is_empty() && self.custom.is_empty()
    }
}

/// Where a drag over the window is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DragPhase {
    /// It just came in (`DragEnter`).
    Enter,
    /// It is moving over the window (`DragOver`).
    Over,
    /// It left, or was cancelled (`DragLeave`).
    Leave,
    /// It was dropped (`DragDrop`).
    Drop,
}

/// The drag over the window, as the frame being rendered sees it ([`current`]).
#[derive(Debug, Clone)]
pub struct DragFrame {
    pub phase: DragPhase,
    pub data: Rc<DataObject>,
    /// What the source allows.
    pub allowed: DragDropEffects,
    /// The pointer, in client DIP.
    pub x: f32,
    pub y: f32,
    pub mods: Modifiers,
    /// The mouse buttons held (left, right, middle): WinForms' `KeyState` bits.
    pub buttons: (bool, bool, bool),
    /// The drag was started by this process ([`do_drag_drop`]).
    pub internal: bool,
}

/// The phase machine of a drag over the window: what OLE says (enter, over, leave, drop) becomes
/// the frame's [`DragFrame`]; what the page answers ([`set_effect`]) goes back.
#[derive(Default)]
pub struct Tracker {
    frame: Option<DragFrame>,
    effect: DragDropEffects,
    /// What the last rendered frame answered (what OLE is told).
    answered: DragDropEffects,
}

impl Tracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// A drag came in.
    #[allow(clippy::too_many_arguments)]
    pub fn enter(&mut self, data: Rc<DataObject>, allowed: DragDropEffects, x: f32, y: f32, mods: Modifiers, buttons: (bool, bool, bool), internal: bool) {
        self.effect = DragDropEffects::NONE;
        self.frame = Some(DragFrame { phase: DragPhase::Enter, data, allowed, x, y, mods, buttons, internal });
    }

    /// It moved (or keys changed). A move before any enter is ignored.
    pub fn over(&mut self, x: f32, y: f32, mods: Modifiers, buttons: (bool, bool, bool)) {
        if let Some(f) = self.frame.as_mut() {
            // A frame that has not rendered its Enter yet keeps it: the page must see the enter.
            if f.phase != DragPhase::Enter {
                f.phase = DragPhase::Over;
            }
            f.x = x;
            f.y = y;
            f.mods = mods;
            f.buttons = buttons;
        }
    }

    /// It left or was cancelled.
    pub fn leave(&mut self) {
        if let Some(f) = self.frame.as_mut() {
            f.phase = DragPhase::Leave;
        }
    }

    /// It was dropped at `(x, y)`.
    pub fn drop_at(&mut self, x: f32, y: f32, mods: Modifiers) {
        if let Some(f) = self.frame.as_mut() {
            f.phase = DragPhase::Drop;
            f.x = x;
            f.y = y;
            f.mods = mods;
        }
    }

    /// What a frame sees.
    pub fn current(&self) -> Option<DragFrame> {
        self.frame.clone()
    }

    /// The page's answer for the pointer's position (kept within what the source allows).
    pub fn set_effect(&mut self, effect: DragDropEffects) {
        let allowed = self.frame.as_ref().map_or(DragDropEffects::NONE, |f| f.allowed);
        self.effect = effect & allowed;
    }

    pub fn effect(&self) -> DragDropEffects {
        self.effect
    }

    /// The answer of the last frame that rendered the drag.
    pub fn answered(&self) -> DragDropEffects {
        self.answered
    }

    /// After a frame rendered the drag: an enter becomes a drag moving over; a leave or a drop
    /// ends it. Returns the effect to answer OLE with.
    pub fn end_frame(&mut self) -> DragDropEffects {
        let effect = self.effect;
        if self.frame.is_some() {
            self.answered = effect;
        }
        match self.frame.as_ref().map(|f| f.phase) {
            Some(DragPhase::Enter) => {
                if let Some(f) = self.frame.as_mut() {
                    f.phase = DragPhase::Over;
                }
            }
            Some(DragPhase::Leave) | Some(DragPhase::Drop) => {
                self.frame = None;
                self.effect = DragDropEffects::NONE;
            }
            _ => {}
        }
        effect
    }
}

/// A drag this window asked to start ([`do_drag_drop`]).
pub(crate) struct StartRequest {
    pub data: DataObject,
    pub allowed: DragDropEffects,
    pub done: Box<dyn FnOnce(DragDropEffects)>,
}

thread_local! {
    static TRACKER: RefCell<Tracker> = RefCell::new(Tracker::new());
    /// The page has a drop target this frame.
    static ACCEPT: Cell<bool> = const { Cell::new(false) };
    /// A drag to start after this frame.
    static START: RefCell<Option<StartRequest>> = const { RefCell::new(None) };
    /// The data of the drag this thread is running as a source (so the drop target of the same
    /// process reads it whole, custom formats included, without a round trip through OLE).
    static INTERNAL: RefCell<Option<Rc<DataObject>>> = const { RefCell::new(None) };
}

/// The per-window drag state (the drop target's tracker, this frame's drop target, a drag to
/// start), swapped by `super::window_tls` between the windows of a thread.
#[derive(Default)]
pub(crate) struct WindowState {
    tracker: Tracker,
    accept: bool,
    start: Option<StartRequest>,
}

pub(crate) fn take_window_state() -> WindowState {
    WindowState {
        tracker: TRACKER.with(|t| t.try_borrow_mut().map(|mut v| std::mem::take(&mut *v)).unwrap_or_default()),
        accept: ACCEPT.with(|a| a.replace(false)),
        start: START.with(|s| s.try_borrow_mut().ok().and_then(|mut v| v.take())),
    }
}

pub(crate) fn put_window_state(state: WindowState) {
    TRACKER.with(|t| {
        if let Ok(mut v) = t.try_borrow_mut() {
            *v = state.tracker;
        }
    });
    ACCEPT.with(|a| a.set(state.accept));
    START.with(|s| {
        if let Ok(mut v) = s.try_borrow_mut() {
            *v = state.start;
        }
    });
}

/// The drag over the window in the frame being rendered, if any.
pub fn current() -> Option<DragFrame> {
    TRACKER.with(|t| t.borrow().current())
}

/// The page's answer to the drag at the pointer: what a drop there would do (`DragEventArgs.Effect`,
/// masked by what the source allows). `NONE` refuses.
pub fn set_effect(effect: DragDropEffects) {
    TRACKER.with(|t| t.borrow_mut().set_effect(effect));
}

/// Whether the page has a drop target this frame (call it every frame; a frame that does not
/// refuses drags). The window is registered with OLE the first time.
pub fn accept_drops(accept: bool) {
    ACCEPT.with(|a| a.set(a.get() || accept));
}

/// Starts a drag of `data` allowing `allowed` (`Control.DoDragDrop`), right after the frame that
/// asked; `done` receives the effect the target chose (`NONE` when cancelled or refused). A second
/// request in the same frame replaces the first (whose `done` gets `NONE`).
pub fn do_drag_drop(data: DataObject, allowed: DragDropEffects, done: impl FnOnce(DragDropEffects) + 'static) {
    let old = START.with(|s| s.borrow_mut().replace(StartRequest { data, allowed, done: Box::new(done) }));
    if let Some(old) = old {
        (old.done)(DragDropEffects::NONE);
    }
}

/// Whether a drag started by this thread is running (between [`do_drag_drop`] taking effect and
/// its end).
pub fn is_dragging() -> bool {
    INTERNAL.with(|i| i.borrow().is_some())
}

pub(crate) fn take_accept() -> bool {
    ACCEPT.with(|a| a.replace(false))
}

pub(crate) fn take_start() -> Option<StartRequest> {
    START.with(|s| s.borrow_mut().take())
}

pub(crate) fn has_pending_start() -> bool {
    START.with(|s| s.borrow().is_some())
}

pub(crate) fn with_tracker<R>(f: impl FnOnce(&mut Tracker) -> R) -> R {
    TRACKER.with(|t| f(&mut t.borrow_mut()))
}

pub(crate) fn set_internal(data: Option<Rc<DataObject>>) {
    INTERNAL.with(|i| *i.borrow_mut() = data);
}

pub(crate) fn internal() -> Option<Rc<DataObject>> {
    INTERNAL.with(|i| i.borrow().clone())
}

/// The OLE objects: the window's drop target, a drag's data object and drop source, and the
/// reading of a foreign data object.
pub(crate) mod ole {
    use std::path::PathBuf;

    use windows::core::{implement, Ref, BOOL, HRESULT, PCWSTR};
    use windows::Win32::Foundation::{DRAGDROP_S_CANCEL, DRAGDROP_S_DROP, DRAGDROP_S_USEDEFAULTCURSORS, DV_E_FORMATETC, E_NOTIMPL, HGLOBAL, HWND, POINT, POINTL, S_OK};
    use windows::Win32::Graphics::Gdi::ScreenToClient;
    use windows::Win32::System::Com::{IDataObject, IDataObject_Impl, IEnumFORMATETC, DATADIR_GET, DVASPECT_CONTENT, FORMATETC, STGMEDIUM, STGMEDIUM_0, TYMED_HGLOBAL};
    use windows::Win32::System::DataExchange::{GetClipboardFormatNameW, RegisterClipboardFormatW};
    use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE};
    use windows::Win32::System::Ole::{
        IDropSource, IDropSource_Impl, IDropTarget, IDropTarget_Impl, ReleaseStgMedium, CF_HDROP, CF_UNICODETEXT, DROPEFFECT, DROPEFFECT_NONE,
    };
    use windows::Win32::System::SystemServices::{MK_CONTROL, MK_LBUTTON, MK_MBUTTON, MK_RBUTTON, MK_SHIFT, MODIFIERKEYS_FLAGS};
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VK_MENU};
    use windows::Win32::UI::Shell::{DragQueryFileW, SHCreateStdEnumFmtEtc, DROPFILES, HDROP};

    use super::{DataObject, DragDropEffects};
    use crate::host::input::Modifiers;

    /// The biggest custom format read from a foreign drag (the Explorer offers many).
    const MAX_FOREIGN_FORMAT: usize = 1 << 20;

    pub fn modifiers(keys: MODIFIERKEYS_FLAGS) -> (Modifiers, (bool, bool, bool)) {
        // SAFETY: `GetKeyState` has no precondition.
        let alt = unsafe { GetKeyState(i32::from(VK_MENU.0)) } < 0;
        let mods = Modifiers { ctrl: keys.0 & MK_CONTROL.0 != 0, shift: keys.0 & MK_SHIFT.0 != 0, alt, meta: false };
        (mods, (keys.0 & MK_LBUTTON.0 != 0, keys.0 & MK_RBUTTON.0 != 0, keys.0 & MK_MBUTTON.0 != 0))
    }

    fn format(cf: u16) -> FORMATETC {
        FORMATETC { cfFormat: cf, ptd: std::ptr::null_mut(), dwAspect: DVASPECT_CONTENT.0, lindex: -1, tymed: TYMED_HGLOBAL.0 as u32 }
    }

    fn register(name: &str) -> u16 {
        let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
        // SAFETY: a NUL-terminated wide string.
        unsafe { RegisterClipboardFormatW(PCWSTR(wide.as_ptr())) as u16 }
    }

    /// The bytes of an `HGLOBAL`.
    unsafe fn global_bytes(h: HGLOBAL, max: usize) -> Option<Vec<u8>> {
        let size = GlobalSize(h);
        if size == 0 || size > max {
            return None;
        }
        let p = GlobalLock(h) as *const u8;
        if p.is_null() {
            return None;
        }
        let v = std::slice::from_raw_parts(p, size).to_vec();
        let _ = GlobalUnlock(h);
        Some(v)
    }

    fn global_from(bytes: &[u8]) -> Option<HGLOBAL> {
        // SAFETY: a fresh movable block, filled while locked.
        unsafe {
            let h = GlobalAlloc(GMEM_MOVEABLE, bytes.len().max(1)).ok()?;
            let p = GlobalLock(h) as *mut u8;
            if p.is_null() {
                return None;
            }
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), p, bytes.len());
            let _ = GlobalUnlock(h);
            Some(h)
        }
    }

    fn utf16_z(s: &str) -> Vec<u8> {
        s.encode_utf16().chain(std::iter::once(0)).flat_map(u16::to_le_bytes).collect()
    }

    /// `DROPFILES` followed by the double-NUL-terminated wide paths.
    fn hdrop_bytes(files: &[PathBuf]) -> Vec<u8> {
        let header = DROPFILES { pFiles: std::mem::size_of::<DROPFILES>() as u32, pt: POINT::default(), fNC: BOOL(0), fWide: BOOL(1) };
        // SAFETY: `DROPFILES` is plain old data.
        let head = unsafe { std::slice::from_raw_parts(&header as *const DROPFILES as *const u8, std::mem::size_of::<DROPFILES>()) };
        let mut out = head.to_vec();
        for f in files {
            out.extend(utf16_z(&f.to_string_lossy()));
        }
        out.extend([0u8, 0u8]);
        out
    }

    /// Reads a foreign data object: text, files and the registered formats.
    pub fn read(obj: &IDataObject) -> DataObject {
        let mut out = DataObject::default();
        // SAFETY: OLE calls on a live data object; every medium obtained is released.
        unsafe {
            if let Ok(mut m) = obj.GetData(&format(CF_HDROP.0)) {
                let hdrop = HDROP(m.u.hGlobal.0);
                let count = DragQueryFileW(hdrop, u32::MAX, None);
                for i in 0..count {
                    let len = DragQueryFileW(hdrop, i, None) as usize;
                    let mut buf = vec![0u16; len + 1];
                    let got = DragQueryFileW(hdrop, i, Some(&mut buf)) as usize;
                    out.files.push(PathBuf::from(String::from_utf16_lossy(&buf[..got.min(len)])));
                }
                ReleaseStgMedium(&mut m);
            }
            if let Ok(mut m) = obj.GetData(&format(CF_UNICODETEXT.0)) {
                if let Some(bytes) = global_bytes(m.u.hGlobal, 64 << 20) {
                    let units: Vec<u16> = bytes.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).take_while(|u| *u != 0).collect();
                    out.text = Some(String::from_utf16_lossy(&units));
                }
                ReleaseStgMedium(&mut m);
            }
            if let Ok(e) = obj.EnumFormatEtc(DATADIR_GET.0 as u32) {
                let mut f = [FORMATETC::default()];
                let mut fetched = 0u32;
                while e.Next(&mut f, Some(&mut fetched)).is_ok() && fetched == 1 {
                    let cf = f[0].cfFormat;
                    if cf >= 0xC000 && f[0].tymed & TYMED_HGLOBAL.0 as u32 != 0 {
                        let mut name = [0u16; 256];
                        let n = GetClipboardFormatNameW(u32::from(cf), &mut name);
                        if n > 0 {
                            let name = String::from_utf16_lossy(&name[..n as usize]);
                            if let Ok(mut m) = obj.GetData(&format(cf)) {
                                if let Some(bytes) = global_bytes(m.u.hGlobal, MAX_FOREIGN_FORMAT) {
                                    out.custom.push((name, bytes));
                                }
                                ReleaseStgMedium(&mut m);
                            }
                        }
                    }
                    if !f[0].ptd.is_null() {
                        windows::Win32::System::Com::CoTaskMemFree(Some(f[0].ptd as *const _));
                    }
                }
            }
        }
        out
    }

    /// What the window's drop target does with each OLE call: record it and render a frame.
    pub trait Sink {
        fn enter(&self, data: DataObject, allowed: DragDropEffects, pt: (i32, i32), keys: MODIFIERKEYS_FLAGS) -> DragDropEffects;
        fn over(&self, pt: (i32, i32), keys: MODIFIERKEYS_FLAGS) -> DragDropEffects;
        fn leave(&self);
        fn dropped(&self, pt: (i32, i32), keys: MODIFIERKEYS_FLAGS) -> DragDropEffects;
    }

    /// Screen pixels to the window's client pixels.
    pub fn to_client(hwnd: HWND, pt: &POINTL) -> (i32, i32) {
        let mut p = POINT { x: pt.x, y: pt.y };
        // SAFETY: plain coordinate conversion on a window handle.
        unsafe {
            let _ = ScreenToClient(hwnd, &mut p);
        }
        (p.x, p.y)
    }

    #[implement(IDropTarget)]
    pub struct DropTarget {
        pub sink: Box<dyn Sink>,
    }

    impl IDropTarget_Impl for DropTarget_Impl {
        fn DragEnter(&self, data: Ref<IDataObject>, keys: MODIFIERKEYS_FLAGS, pt: &POINTL, effect: *mut DROPEFFECT) -> windows::core::Result<()> {
            let allowed = if effect.is_null() { DragDropEffects::ALL } else { DragDropEffects::from_ole(unsafe { (*effect).0 }) };
            let object = match super::internal() {
                Some(own) => (*own).clone(),
                None => data.ok().map(read).unwrap_or_default(),
            };
            let answer = self.sink.enter(object, allowed, (pt.x, pt.y), keys);
            if !effect.is_null() {
                // SAFETY: OLE passes a writable effect.
                unsafe { *effect = DROPEFFECT(answer.to_ole()) };
            }
            Ok(())
        }

        fn DragOver(&self, keys: MODIFIERKEYS_FLAGS, pt: &POINTL, effect: *mut DROPEFFECT) -> windows::core::Result<()> {
            let answer = self.sink.over((pt.x, pt.y), keys);
            if !effect.is_null() {
                // SAFETY: as above.
                unsafe { *effect = DROPEFFECT(answer.to_ole()) };
            }
            Ok(())
        }

        fn DragLeave(&self) -> windows::core::Result<()> {
            self.sink.leave();
            Ok(())
        }

        fn Drop(&self, _data: Ref<IDataObject>, keys: MODIFIERKEYS_FLAGS, pt: &POINTL, effect: *mut DROPEFFECT) -> windows::core::Result<()> {
            let answer = self.sink.dropped((pt.x, pt.y), keys);
            if !effect.is_null() {
                // SAFETY: as above.
                unsafe { *effect = DROPEFFECT(answer.to_ole()) };
            }
            Ok(())
        }
    }

    /// The source side of a drag started here: Escape cancels, releasing the button drops.
    #[implement(IDropSource)]
    pub struct DropSource;

    impl IDropSource_Impl for DropSource_Impl {
        fn QueryContinueDrag(&self, escape: BOOL, keys: MODIFIERKEYS_FLAGS) -> HRESULT {
            if escape.as_bool() {
                return DRAGDROP_S_CANCEL;
            }
            if keys.0 & (MK_LBUTTON.0 | MK_RBUTTON.0) == 0 {
                return DRAGDROP_S_DROP;
            }
            S_OK
        }

        fn GiveFeedback(&self, _effect: DROPEFFECT) -> HRESULT {
            DRAGDROP_S_USEDEFAULTCURSORS
        }
    }

    /// The data object of a drag started here.
    #[implement(IDataObject)]
    pub struct DataObj {
        entries: Vec<(FORMATETC, Vec<u8>)>,
    }

    impl DataObj {
        pub fn new(data: &DataObject) -> Self {
            let mut entries = Vec::new();
            if let Some(t) = &data.text {
                entries.push((format(CF_UNICODETEXT.0), utf16_z(t)));
            }
            if !data.files.is_empty() {
                entries.push((format(CF_HDROP.0), hdrop_bytes(&data.files)));
            }
            for (name, bytes) in &data.custom {
                let cf = register(name);
                if cf != 0 {
                    entries.push((format(cf), bytes.clone()));
                }
            }
            Self { entries }
        }

        fn find(&self, f: *const FORMATETC) -> Option<&Vec<u8>> {
            if f.is_null() {
                return None;
            }
            // SAFETY: OLE passes a valid FORMATETC.
            let f = unsafe { &*f };
            if f.tymed & TYMED_HGLOBAL.0 as u32 == 0 {
                return None;
            }
            self.entries.iter().find(|(e, _)| e.cfFormat == f.cfFormat).map(|(_, b)| b)
        }
    }

    impl IDataObject_Impl for DataObj_Impl {
        fn GetData(&self, f: *const FORMATETC) -> windows::core::Result<STGMEDIUM> {
            let bytes = self.find(f).ok_or_else(|| windows::core::Error::from(DV_E_FORMATETC))?;
            let h = global_from(bytes).ok_or_else(|| windows::core::Error::from(windows::Win32::Foundation::E_OUTOFMEMORY))?;
            Ok(STGMEDIUM { tymed: TYMED_HGLOBAL.0 as u32, u: STGMEDIUM_0 { hGlobal: h }, pUnkForRelease: std::mem::ManuallyDrop::new(None) })
        }

        fn GetDataHere(&self, _f: *const FORMATETC, _m: *mut STGMEDIUM) -> windows::core::Result<()> {
            Err(E_NOTIMPL.into())
        }

        fn QueryGetData(&self, f: *const FORMATETC) -> HRESULT {
            if self.find(f).is_some() {
                S_OK
            } else {
                DV_E_FORMATETC
            }
        }

        fn GetCanonicalFormatEtc(&self, _in: *const FORMATETC, out: *mut FORMATETC) -> HRESULT {
            if !out.is_null() {
                // SAFETY: a writable FORMATETC from OLE.
                unsafe { (*out).ptd = std::ptr::null_mut() };
            }
            windows::Win32::Foundation::DATA_S_SAMEFORMATETC
        }

        fn SetData(&self, _f: *const FORMATETC, _m: *const STGMEDIUM, _release: BOOL) -> windows::core::Result<()> {
            Err(E_NOTIMPL.into())
        }

        fn EnumFormatEtc(&self, direction: u32) -> windows::core::Result<IEnumFORMATETC> {
            if direction != DATADIR_GET.0 as u32 {
                return Err(E_NOTIMPL.into());
            }
            let formats: Vec<FORMATETC> = self.entries.iter().map(|(f, _)| *f).collect();
            // SAFETY: a slice of plain FORMATETCs, copied by the shell.
            unsafe { SHCreateStdEnumFmtEtc(&formats) }
        }

        fn DAdvise(&self, _f: *const FORMATETC, _advf: u32, _sink: Ref<windows::Win32::System::Com::IAdviseSink>) -> windows::core::Result<u32> {
            Err(windows::Win32::Foundation::OLE_E_ADVISENOTSUPPORTED.into())
        }

        fn DUnadvise(&self, _c: u32) -> windows::core::Result<()> {
            Err(windows::Win32::Foundation::OLE_E_ADVISENOTSUPPORTED.into())
        }

        fn EnumDAdvise(&self) -> windows::core::Result<windows::Win32::System::Com::IEnumSTATDATA> {
            Err(windows::Win32::Foundation::OLE_E_ADVISENOTSUPPORTED.into())
        }
    }

    /// Runs OLE's drag loop for `data` (blocks until the drop or the cancel) and returns the effect.
    pub fn run_drag(data: &DataObject, allowed: DragDropEffects) -> DragDropEffects {
        // OLE on this thread (a window with no drop target has not initialised it yet; a second
        // initialisation of the UI thread only counts one more).
        // SAFETY: plain OLE initialisation of the calling (UI, STA) thread.
        if let Err(e) = unsafe { windows::Win32::System::Ole::OleInitialize(None) } {
            tracing::warn!("drag and drop unavailable: OleInitialize failed ({e})");
            return DragDropEffects::NONE;
        }
        let obj: IDataObject = DataObj::new(data).into();
        let source: IDropSource = DropSource.into();
        let mut effect = DROPEFFECT_NONE;
        // SAFETY: both objects live across the call; OLE is initialised on this thread by the host.
        let hr = unsafe { windows::Win32::System::Ole::DoDragDrop(&obj, &source, DROPEFFECT(allowed.to_ole()), &mut effect) };
        if hr == DRAGDROP_S_DROP {
            DragDropEffects::from_ole(effect.0) & allowed
        } else {
            DragDropEffects::NONE
        }
    }

    /// Makes `data` readable back as `IDataObject` (tests of the round trip).
    #[cfg(test)]
    pub fn to_ole(data: &DataObject) -> IDataObject {
        DataObj::new(data).into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(t: &Tracker) -> Option<(DragPhase, f32)> {
        t.current().map(|f| (f.phase, f.x))
    }

    #[test]
    fn enter_over_drop_then_the_drag_ends() {
        let mut t = Tracker::new();
        assert!(frame(&t).is_none());
        t.over(1.0, 1.0, Modifiers::NONE, (true, false, false));
        assert!(frame(&t).is_none(), "a move before an enter is ignored");
        t.enter(Rc::new(DataObject::from_text("hi")), DragDropEffects::COPY | DragDropEffects::MOVE, 5.0, 5.0, Modifiers::NONE, (true, false, false), false);
        t.over(6.0, 5.0, Modifiers::NONE, (true, false, false));
        assert_eq!(frame(&t), Some((DragPhase::Enter, 6.0)), "the enter is kept until a frame saw it");
        t.set_effect(DragDropEffects::COPY | DragDropEffects::LINK);
        assert_eq!(t.end_frame(), DragDropEffects::COPY, "masked by what the source allows");
        t.over(7.0, 5.0, Modifiers::CTRL, (true, false, false));
        assert_eq!(frame(&t), Some((DragPhase::Over, 7.0)));
        assert_eq!(t.end_frame(), DragDropEffects::COPY, "the answer stands until changed");
        t.drop_at(8.0, 6.0, Modifiers::NONE);
        assert_eq!(frame(&t), Some((DragPhase::Drop, 8.0)));
        t.set_effect(DragDropEffects::MOVE);
        assert_eq!(t.end_frame(), DragDropEffects::MOVE);
        assert!(frame(&t).is_none(), "a drop ends the drag");
    }

    #[test]
    fn leave_ends_the_drag_without_an_effect() {
        let mut t = Tracker::new();
        t.enter(Rc::new(DataObject::default()), DragDropEffects::ALL, 0.0, 0.0, Modifiers::NONE, (true, false, false), true);
        t.end_frame();
        t.set_effect(DragDropEffects::COPY);
        t.leave();
        assert_eq!(frame(&t).map(|f| f.0), Some(DragPhase::Leave));
        t.end_frame();
        assert!(frame(&t).is_none());
        assert_eq!(t.effect(), DragDropEffects::NONE);
    }

    #[test]
    fn effects_map_to_ole_and_follow_the_keys() {
        let all = DragDropEffects::ALL;
        assert_eq!(DragDropEffects::from_ole(all.to_ole()), all);
        assert_eq!(DragDropEffects::SCROLL.to_ole(), 0x8000_0000);
        let cm = DragDropEffects::COPY | DragDropEffects::MOVE;
        assert_eq!(cm.pick(Modifiers::NONE), DragDropEffects::MOVE);
        assert_eq!(cm.pick(Modifiers::CTRL), DragDropEffects::COPY);
        assert_eq!(DragDropEffects::COPY.pick(Modifiers::SHIFT), DragDropEffects::COPY, "moving not allowed: copy");
        assert_eq!(DragDropEffects::NONE.pick(Modifiers::NONE), DragDropEffects::NONE);
    }

    #[test]
    fn data_objects_name_their_formats_like_winforms() {
        let d = DataObject::from_text("t").with_custom("kubuno/item", vec![1, 2]).with_custom("kubuno/item", vec![3]);
        assert!(d.has_format("UnicodeText") && d.has_format("Text") && !d.has_format("FileDrop"));
        assert_eq!(d.get("kubuno/item"), Some(&[3u8][..]), "set replaces");
        assert_eq!(d.formats(), vec!["UnicodeText".to_string(), "kubuno/item".to_string()]);
        assert!(DataObject::default().is_empty());
    }

    #[test]
    fn a_data_object_round_trips_through_ole() {
        // COM on this thread for the shell's format enumerator.
        // SAFETY: plain apartment initialisation of the test thread.
        let _ = unsafe { windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED) };
        let d = DataObject {
            text: Some("héllo".into()),
            files: vec![PathBuf::from("C:\\a b\\c.txt"), PathBuf::from("D:\\x.png")],
            custom: vec![("Kubuno.Test.Format".into(), vec![9, 8, 7])],
        };
        let back = ole::read(&ole::to_ole(&d));
        assert_eq!(back.text.as_deref(), Some("héllo"));
        assert_eq!(back.files, d.files);
        assert_eq!(back.get("Kubuno.Test.Format"), Some(&[9u8, 8, 7][..]));
    }

    #[test]
    fn a_new_start_request_replaces_the_pending_one() {
        let first = Rc::new(Cell::new(None));
        let f2 = first.clone();
        do_drag_drop(DataObject::from_text("a"), DragDropEffects::COPY, move |e| f2.set(Some(e)));
        do_drag_drop(DataObject::from_text("b"), DragDropEffects::MOVE, |_| {});
        assert_eq!(first.get(), Some(DragDropEffects::NONE), "the replaced request is told it did not happen");
        let req = take_start().expect("pending");
        assert_eq!((req.data.text.as_deref(), req.allowed), (Some("b"), DragDropEffects::MOVE));
        assert!(take_start().is_none());
    }
}
