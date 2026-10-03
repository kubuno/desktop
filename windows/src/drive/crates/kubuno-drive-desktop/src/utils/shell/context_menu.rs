//! ContextMenu (mirrors ContextMenu.cs)
//!
//! Native shell context menu for an item — the same menu the original app
//! surfaces through `IContextMenu` (Utils/Shell/ContextMenu.cs).
//!
//! [`ShellMenu`] **enumerates** the `IContextMenu` so its entries can be drawn
//! inside our own flyout. That is what the original does for its `ItemOverflow`
//! entry (« Afficher plus d'options ») : the item starts out holding a disabled
//! « Chargement … » row, and `AddShellMenuItemsAsync` swaps in the shell items
//! once they have been queried.
//!
//! [`ShellWorker`] owns the `IContextMenu` on a dedicated STA thread — the
//! analogue of `ThreadWithMessageQueue.cs`, kept tightly coupled to `ShellMenu`.

use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Gdi::HBITMAP;
use windows::Win32::UI::Shell::{BHID_SFUIObject, IContextMenu, CMINVOKECOMMANDINFO, CMF_NORMAL};
use windows::Win32::UI::WindowsAndMessaging::{
    CreatePopupMenu, DestroyMenu, GetMenuItemCount, GetMenuItemInfoW, GetMenuStringW,
    HMENU, MENUITEMINFOW, MFS_DISABLED, MFS_GRAYED, MFT_SEPARATOR, MF_BYPOSITION,
    MIIM_BITMAP, MIIM_FTYPE, MIIM_ID, MIIM_STATE, MIIM_SUBMENU, SW_SHOWNORMAL,
};

use kubuno_drive_desktop_app_storage::windows_storage::WindowsStorable;

use super::context_menu_item::ShellEntry;

const CMD_FIRST: u32 = 1;
const CMD_LAST: u32 = 0x7FFF;

/// Copies an `HBITMAP` into BGRA pixels. Menu bitmaps are 32bpp top-down DIBs
/// with a real alpha channel; anything else (a 1bpp mask, say) is dropped.
fn bitmap_pixels(hbitmap: HBITMAP) -> Option<kubuno_drive_desktop_app_storage::ShellBitmap> {
    use windows::Win32::Graphics::Gdi::{
        CreateCompatibleDC, DeleteDC, GetDIBits, GetObjectW, BITMAP, BITMAPINFO, BITMAPINFOHEADER,
        BI_RGB, DIB_RGB_COLORS,
    };

    unsafe {
        let mut bm = BITMAP::default();
        let written = GetObjectW(
            hbitmap.into(),
            std::mem::size_of::<BITMAP>() as i32,
            Some(&mut bm as *mut _ as *mut _),
        );
        if written == 0 || bm.bmWidth <= 0 || bm.bmHeight <= 0 || bm.bmBitsPixel != 32 {
            return None;
        }
        let (w, h) = (bm.bmWidth as u32, bm.bmHeight as u32);

        let mut info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: bm.bmWidth,
                // Negative height = top-down, which is the row order we want.
                biHeight: -bm.bmHeight,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bgra = vec![0u8; (w * h * 4) as usize];
        let dc = CreateCompatibleDC(None);
        let rows = GetDIBits(
            dc,
            hbitmap,
            0,
            h,
            Some(bgra.as_mut_ptr() as *mut _),
            &mut info,
            DIB_RGB_COLORS,
        );
        let _ = DeleteDC(dc);
        if rows == 0 {
            return None;
        }
        // An all-zero alpha channel means the extension gave us an opaque
        // bitmap without alpha; treat it as fully opaque rather than invisible.
        if bgra.iter().skip(3).step_by(4).all(|&a| a == 0) {
            for a in bgra.iter_mut().skip(3).step_by(4) {
                *a = 255;
            }
        } else {
            // D2D wants premultiplied alpha.
            for px in bgra.as_chunks_mut::<4>().0 {
                let a = px[3] as u32;
                for c in &mut px[..3] {
                    *c = ((*c as u32 * a) / 255) as u8;
                }
            }
        }
        Some(kubuno_drive_desktop_app_storage::ShellBitmap { width: w, height: h, bgra })
    }
}

/// A live `IContextMenu` whose entries we host in our own flyout. It must stay
/// alive between the query and the invoke — the shell verbs are only valid on
/// the very object that produced the ids.
pub struct ShellMenu {
    menu: IContextMenu,
    hmenu: HMENU,
    pub entries: Vec<ShellEntry>,
}

impl ShellMenu {
    /// Queries the shell menu of `path`. Entries carrying a submenu are left
    /// out: our flyout only nests one level, and the overflow already is that
    /// level.
    pub fn open(path: &str) -> Option<Self> {
        let item = WindowsStorable::try_parse(path)?;
        unsafe {
            let menu = item
                .storable()
                .shell_item()
                .BindToHandler::<_, IContextMenu>(None, &BHID_SFUIObject)
                .ok()?;
            let hmenu = CreatePopupMenu().ok()?;
            if menu.QueryContextMenu(hmenu, 0, CMD_FIRST, CMD_LAST, CMF_NORMAL).ok().is_err() {
                let _ = DestroyMenu(hmenu);
                return None;
            }

            let count = GetMenuItemCount(Some(hmenu));
            let mut entries = Vec::new();
            for i in 0..count.max(0) {
                let mut info = MENUITEMINFOW {
                    cbSize: std::mem::size_of::<MENUITEMINFOW>() as u32,
                    fMask: MIIM_ID | MIIM_STATE | MIIM_FTYPE | MIIM_SUBMENU | MIIM_BITMAP,
                    ..Default::default()
                };
                if GetMenuItemInfoW(hmenu, i as u32, true, &mut info).is_err() {
                    continue;
                }
                if !info.hSubMenu.is_invalid() {
                    continue;
                }
                let separator = info.fType.0 & MFT_SEPARATOR.0 != 0;
                if separator {
                    // Never open on a separator, and never repeat one.
                    if entries.last().is_none_or(|e: &ShellEntry| e.separator) {
                        continue;
                    }
                    entries.push(ShellEntry {
                        label: String::new(),
                        separator: true,
                        enabled: false,
                        bitmap: None,
                        id: 0,
                    });
                    continue;
                }

                let mut buf = [0u16; 512];
                let len = GetMenuStringW(hmenu, i as u32, Some(&mut buf), MF_BYPOSITION);
                if len <= 0 {
                    continue;
                }
                // The shell writes accelerators as "&Ouvrir\tEntrée": drop the
                // mnemonic ampersands and the tab column, like WinUI does.
                let raw = String::from_utf16_lossy(&buf[..len as usize]);
                let label = raw.split('\t').next().unwrap_or(&raw).replace('&', "");
                if label.is_empty() {
                    continue;
                }
                let state = info.fState.0;
                let bitmap = (!info.hbmpItem.is_invalid())
                    .then(|| bitmap_pixels(info.hbmpItem))
                    .flatten()
                    .map(std::sync::Arc::new);
                entries.push(ShellEntry {
                    label,
                    separator: false,
                    enabled: state & (MFS_DISABLED.0 | MFS_GRAYED.0) == 0,
                    bitmap,
                    id: info.wID.saturating_sub(CMD_FIRST),
                });
            }
            // A trailing separator would draw a stray line at the bottom.
            while entries.last().is_some_and(|e| e.separator) {
                entries.pop();
            }
            Some(Self { menu, hmenu, entries })
        }
    }

    /// Runs entry `index`. Returns false when the row is a separator/disabled.
    pub fn invoke(&self, hwnd: HWND, index: usize) -> bool {
        let Some(entry) = self.entries.get(index) else {
            return false;
        };
        if entry.separator || !entry.enabled {
            return false;
        }
        unsafe {
            let invoke = CMINVOKECOMMANDINFO {
                cbSize: std::mem::size_of::<CMINVOKECOMMANDINFO>() as u32,
                hwnd,
                lpVerb: windows::core::PCSTR(entry.id as usize as *const u8),
                nShow: SW_SHOWNORMAL.0,
                ..Default::default()
            };
            self.menu.InvokeCommand(&invoke).is_ok()
        }
    }
}

impl Drop for ShellMenu {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyMenu(self.hmenu);
        }
    }
}

/// The worker that owns the `IContextMenu`.
///
/// `QueryContextMenu` wakes every shell extension installed on the machine and
/// routinely takes a few hundred milliseconds — on the UI thread that would
/// freeze the menu mid-unfold. The original runs it as a `Task`
/// (`AddShellMenuItemsAsync`); we give it its own STA thread, which also keeps
/// the COM object on a single apartment, as `IContextMenu` requires: the verbs
/// are only valid on the very object that produced the ids, so invoking has to
/// go back through the same thread.
pub struct ShellWorker {
    tx: std::sync::mpsc::Sender<Request>,
    /// Entries of the last `open`, once the worker is done.
    pub result: std::sync::Arc<std::sync::Mutex<Option<Vec<ShellEntry>>>>,
    /// Set when an `invoke` actually ran a verb (the view must refresh).
    pub invoked: std::sync::Arc<std::sync::Mutex<bool>>,
}

enum Request {
    Open(String),
    Invoke(usize),
}

impl ShellWorker {
    /// `notify` is posted to `hwnd` whenever a request completes.
    pub fn new(hwnd: HWND, notify: u32) -> Self {
        use windows::Win32::Foundation::{LPARAM, WPARAM};
        use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
        use windows::Win32::UI::WindowsAndMessaging::PostMessageW;

        let (tx, rx) = std::sync::mpsc::channel::<Request>();
        let result = std::sync::Arc::new(std::sync::Mutex::new(None));
        let invoked = std::sync::Arc::new(std::sync::Mutex::new(false));
        let (result_w, invoked_w) = (result.clone(), invoked.clone());
        let hwnd_raw = hwnd.0 as isize;

        std::thread::spawn(move || unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            let hwnd = HWND(hwnd_raw as *mut _);
            let mut menu: Option<ShellMenu> = None;
            while let Ok(request) = rx.recv() {
                match request {
                    Request::Open(path) => {
                        let opened = ShellMenu::open(&path);
                        *result_w.lock().unwrap() = opened.as_ref().map(|m| m.entries.clone());
                        menu = opened;
                    }
                    Request::Invoke(i) => {
                        let ran = menu.as_ref().is_some_and(|m| m.invoke(hwnd, i));
                        *invoked_w.lock().unwrap() = ran;
                    }
                }
                let _ = PostMessageW(Some(hwnd), notify, WPARAM(0), LPARAM(0));
            }
        });
        Self { tx, result, invoked }
    }

    pub fn open(&self, path: String) {
        *self.result.lock().unwrap() = None;
        let _ = self.tx.send(Request::Open(path));
    }

    pub fn invoke(&self, index: usize) {
        let _ = self.tx.send(Request::Invoke(index));
    }
}
