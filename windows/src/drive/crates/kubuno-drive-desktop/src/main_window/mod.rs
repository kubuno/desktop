//! Port of `Files.App/MainWindow.xaml.cs` **and** `Views/MainPage.xaml.cs`:
//! the frameless Win32 window (Mica, custom title bar), its wndproc, the
//! keyboard/mouse routing and the command dispatch (≈ `Actions/`).

use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::graphics::Renderer;
use crate::services::storage::IconCache;
use crate::data::items::HomeModel;
use crate::view_models::shell_view_model::Location;
use crate::styles::theme::Theme;
use crate::ui::{Hot, UiState};

/// Timer driving the tab reorder animation (42 is the background-ops poll).
const TAB_ANIM_TIMER: usize = 43;
/// Drives the flyout unfold (`PopupThemeTransition`).
const MENU_ANIM_TIMER: usize = 44;
/// Fades out the `ScrollBar` indicator after scrolling.
const SCROLLBAR_TIMER: usize = 45;
/// Drives the Minimal pane's slide (`TranslateX`, 350 ms).
const SIDEBAR_ANIM_TIMER: usize = 46;
/// Opens the tooltip after hovering (`ToolTipService`, ~750 ms delay).
const TOOLTIP_TIMER: usize = 47;
/// Tooltip opening delay — the web's `Tooltip` default (`delay = 400`), not
/// WinUI's 750 ms `ToolTipService.InitialShowDelay`.
const TOOLTIP_DELAY_MS: u32 = 400;

/// Localized name of a key, the way `HotKey.LocalizedLabel` builds it in the
/// original: the modifier and key names come from the keyboard layout, so a
/// French install reads "Ctrl+Maj+T", not "Ctrl+Shift+T".
fn key_name(vk: u32) -> String {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        GetKeyNameTextW, MapVirtualKeyW, MAPVK_VK_TO_VSC,
    };

    use windows::Win32::UI::Input::KeyboardAndMouse::{
        VK_DELETE, VK_DOWN, VK_END, VK_HOME, VK_INSERT, VK_LEFT, VK_NEXT, VK_PRIOR, VK_RIGHT,
        VK_UP,
    };

    let scan = unsafe { MapVirtualKeyW(vk, MAPVK_VK_TO_VSC) };
    // The navigation block shares its scan codes with the numeric keypad: with
    // no extended-key flag, VK_DELETE names itself ". (pavé num.)".
    let extended = [
        VK_DELETE, VK_INSERT, VK_HOME, VK_END, VK_PRIOR, VK_NEXT, VK_LEFT, VK_RIGHT, VK_UP, VK_DOWN,
    ]
    .iter()
    .any(|k| k.0 as u32 == vk);
    let lparam = ((scan << 16) | if extended { 1 << 24 } else { 0 }) as i32;
    let mut buf = [0u16; 64];
    let len = unsafe { GetKeyNameTextW(lparam, &mut buf) };
    if len == 0 {
        return String::new();
    }
    // The layout returns them upper-cased ("CTRL", "MAJ"); the original shows
    // them title-cased ("Ctrl+Maj+T").
    let name = String::from_utf16_lossy(&buf[..len as usize]);
    let mut chars = name.chars();
    let cased: String = match chars.next() {
        Some(first) => first.to_uppercase().chain(chars.flat_map(char::to_lowercase)).collect(),
        None => String::new(),
    };
    // A single-character OEM key that isn't A-Z displays lower-case
    // (« Ctrl+ù »), letters stay upper-case (« Ctrl+Maj+C »).
    if cased.chars().count() == 1 {
        let c = cased.chars().next().unwrap();
        if !c.is_ascii_alphabetic() {
            return c.to_lowercase().collect();
        }
    }
    // GetKeyNameText renders French names WITHOUT accents; the original
    // shows them accented (« Entrée », « Échap »…).
    match cased.as_str() {
        "Entree" => "Entrée".into(),
        "Echap" => "Échap".into(),
        "Suppr." => "Suppr".into(),
        "Fleche haut" => "Flèche haut".into(),
        "Fleche bas" => "Flèche bas".into(),
        "Fleche gauche" => "Flèche gauche".into(),
        "Fleche droite" => "Flèche droite".into(),
        _ => cased,
    }
}

/// Like [`hotkey_text`], but from a virtual-key code and modifier
/// booleans — the form used by `actions::HotKey`.
/// True if `path` is a drive root (« X:\ » or « X: »).
pub(crate) fn is_drive_root(path: &str) -> bool {
    let p = path.trim_end_matches('\\');
    p.len() == 2 && p.as_bytes()[1] == b':' && p.as_bytes()[0].is_ascii_alphabetic()
}

/// The displayed name of a destination folder (for the drag caption).
/// A drive root (`C:\`) keeps its full label, otherwise the leaf name.
fn folder_display_name(path: &std::path::Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}

/// The "volume root" of a path, to decide move vs copy: the drive letter
/// (`C:`) or, for a UNC path, `\\server\share`. Two paths on the same root
/// move; on different roots, we copy.
fn volume_root(path: &std::path::Path) -> String {
    let s = path.to_string_lossy();
    if let Some(rest) = s.strip_prefix(r"\\") {
        let mut it = rest.splitn(3, '\\');
        let server = it.next().unwrap_or("");
        let share = it.next().unwrap_or("");
        format!(r"\\{server}\{share}").to_lowercase()
    } else if s.len() >= 2 && s.as_bytes()[1] == b':' {
        s[..2].to_lowercase()
    } else {
        String::new()
    }
}

fn same_drive(a: &std::path::Path, b: &std::path::Path) -> bool {
    let (ra, rb) = (volume_root(a), volume_root(b));
    !ra.is_empty() && ra == rb
}

/// The target of an item drop, resolved from the point under the pointer. A
/// folder (copy/move), the « Épinglé » header (pin) or a tag (assign it) —
/// the counterpart of the original's `HandleItemDroppedAsync`.
pub(crate) enum DropTarget {
    /// A folder/drive: copy or move the items into it.
    Folder { rect: crate::ui::Rect, dest: std::path::PathBuf, name: String },
    /// The « Épinglé » header: pin the dragged folders to quick access.
    Pin { rect: crate::ui::Rect },
    /// A tag: assign it to the dragged files.
    Tag { rect: crate::ui::Rect, uid: String, name: String },
}

pub(crate) fn hotkey_text_vk(key: u32, ctrl: bool, shift: bool, alt: bool) -> String {
    let mut parts = Vec::new();
    if ctrl {
        parts.push(key_name(0x11));
    }
    if shift {
        parts.push(key_name(0x10));
    }
    if alt {
        parts.push(key_name(0x12));
    }
    parts.push(key_name(key));
    parts.retain(|p| !p.is_empty());
    parts.join("+")
}

/// "Ctrl+Maj+T" from the modifier names and the key.
fn hotkey_text(modifiers: &[&str], key: &str) -> String {
    let vk = |name: &str| -> u32 {
        match name {
            "Control" => 0x11,
            "Shift" => 0x10,
            "Alt" => 0x12,
            k => k.as_bytes()[0] as u32, // 'A'..'Z' map onto their VK codes
        }
    };
    let mut parts: Vec<String> = modifiers.iter().map(|m| key_name(vk(m))).collect();
    parts.push(key_name(vk(key)));
    parts.retain(|p| !p.is_empty());
    parts.join("+")
}

pub struct MainWindow {
    pub(crate) hwnd: HWND,
    renderer: Option<Renderer>,
    theme: Theme,
    pub(crate) state: UiState,
    pub(crate) model: HomeModel,
    dpi: f32,
    mouse_tracking: bool,
    backdrop_available: bool,
    icons: IconCache,
    /// Tab drag: live reorder within the strip, then detach outside it
    /// (port of the TabView's drag events).
    tab_drag: Option<TabDrag>,
    /// Handle currently being dragged (`SidebarResizer` / `InfoPaneSizer`), with
    /// the pane's width when grabbed (`preManipulationSidebarWidth`).
    pane_drag: Option<(Hot, f32, f32)>,
    /// INTERNAL drag of selected items (the port has no OLE drag-drop):
    /// (captured paths, starting x/y in DIP, threshold crossed). This is the
    /// counterpart of `Item_DragStarting`: on release, the drop point decides
    /// the target (list folder, sidebar, breadcrumb, tab, other pane, Shelf)
    /// and the operation (move/copy), cf. `Item_Drop`.
    item_drag: Option<(Vec<std::path::PathBuf>, f32, f32, bool)>,
    /// Head segments collapsed under the breadcrumb ellipsis, in order —
    /// the navigation target of each item in the `BreadcrumbOverflow` flyout.
    breadcrumb_overflow: Vec<std::path::PathBuf>,
    /// Undo/redo stack for file operations (`StorageHistory`).
    history: crate::utils::storage_history::StorageHistory,
    /// The Shelf pane's view model (`ShelfViewModel.Items`).
    shelf: crate::view_models::shelf_view_model::ShelfViewModel,
    /// Last pointer position in DIP: the tooltip's anchor (`ToolTip`
    /// opens at the pointer, `Placement=Mouse`).
    mouse_dip: (f32, f32),
    /// The open tooltip (`ToolTipService.ToolTip`): its text, once the
    /// hover delay has elapsed. `None` = none (the `ToolTip` is closed).
    tooltip_shown: Option<String>,
    /// Choice recorded by the `Combo` flyout during `dropdown`'s modal
    /// loop (item index, set by `on_flyout_click`).
    combo_pick: Option<usize>,
    /// The (DIP) rect of the ComboBox button that opens the next `dropdown`:
    /// the menu aligns below it, left edge to the button's left edge.
    combo_anchor: Option<crate::ui::Rect>,
    /// Saved placement while in compact overlay (port of EnterCompactOverlay).
    compact_overlay: Option<WINDOWPLACEMENT>,
    /// Saved placement while in full screen (port of ToggleFullScreen).
    fullscreen: Option<WINDOWPLACEMENT>,
    /// Live directory watchers, one per open Dir location.
    watchers: Vec<crate::utils::folder_watcher::DirWatcher>,
    watcher_seq: usize,
    /// Directories with pending change notifications (debounced refresh).
    pending_refresh: std::collections::HashSet<String>,
    /// StatusCenter: labels of in-flight background operations.
    ops_monitor: std::sync::Arc<crate::utils::storage::OpsMonitor>,
    ops_seq: usize,
    /// Generation of the current search (`FolderSearch`: each new search
    /// cancels the previous one; filters out late WM_APP batches).
    search_generation: u64,
    /// Cached background image (AppThemeBackgroundImageSource → bitmap).
    bg_image: Option<(String, Option<windows::Win32::Graphics::Direct2D::ID2D1Bitmap1>)>,
    /// Locations of closed tabs, most recent last (ReopenClosedTab).
    closed_tabs: Vec<Location>,
    /// Acrylic popup windows hosting the open flyout: [0] main panel,
    /// [1] submenu. Lazily created, kept hidden between uses.
    menu_popups: Vec<crate::user_controls::flyout_window::FlyoutWindow>,
    /// True while we hold the mouse capture for an open flyout.
    menu_capture: bool,
    /// Off-thread owner of the `IContextMenu` that fills the « Afficher plus
    /// d'options » submenu. Created on first use.
    shell_worker: Option<crate::utils::shell::ShellWorker>,
    /// Tab ghost (popup) during a detached drag. Created on first
    /// detachment, hidden between uses.
    tab_ghost: Option<crate::user_controls::tab_ghost::TabGhostWindow>,
    /// Last insertion-preview refresh received (safety net).
    tab_preview_refresh: Option<std::time::Instant>,
    /// « Calculer la taille des dossiers » (port of UserSizeProvider).
    size_provider: crate::services::size_provider::SizeProvider,
}

/// Tab drag in progress. As long as the cursor stays within the strip, the
/// tab reorders live (`index` tracks it). Past the strip, the tab is
/// DETACHED (`detached` carries it with its original index for Escape) and
/// a ghost popup follows the cursor — the counterpart of the original
/// TabView's OLE drag (`TabView_TabDroppedOutside` / `TabView_TabStripDrop`).
pub(crate) struct TabDrag {
    /// Current index in `state.tabs` (live insertion), meaningless while
    /// detached.
    index: usize,
    press_x: f32,
    /// Grab point within the tab (the cursor stays on it).
    grab: f32,
    /// 4-DIP threshold crossed (otherwise release counts as a plain click).
    moved: bool,
    /// Drag start time — the original's accident guard:
    /// "dragTime < 1s && dragDistance < 100px" cancels the tear-out.
    start: std::time::Instant,
    origin_screen: (i32, i32),
    /// Tab removed from the strip + its original index (restored by Escape).
    detached: Option<(crate::view_models::shell_view_model::TabGroup, usize)>,
    /// Port window whose strip is being hovered: it shows the insertion
    /// preview (`WM_APP_TAB_PREVIEW`) as long as the hover lasts.
    preview_target: Option<HWND>,
}

/// Insertion preview at `target`: wparam 1 = show (lparam = screen x),
/// wparam 0 = clear. Cross-process PostMessage, with no pointed-to data.
fn post_tab_preview(target: HWND, x_screen: Option<i32>) {
    unsafe {
        let _ = PostMessageW(
            Some(target),
            WM_APP_TAB_PREVIEW,
            WPARAM(x_screen.is_some() as usize),
            LPARAM(x_screen.unwrap_or(0) as isize),
        );
    }
}

/// `dwData` for the WM_COPYDATA "drop this tab into your strip" message (‘TABD’).
const TAB_DROP_COPYDATA: usize = 0x54414244;

/// The main window of an OTHER port process whose TAB STRIP contains the
/// screen point `pt` (otherwise None).
fn port_tab_strip_under_point(
    pt: windows::Win32::Foundation::POINT,
    not_this: HWND,
) -> Option<HWND> {
    use windows::Win32::Graphics::Gdi::ScreenToClient;
    use windows::Win32::UI::HiDpi::GetDpiForWindow;
    use windows::Win32::UI::WindowsAndMessaging::{
        GetAncestor, GetClassNameW, WindowFromPoint, GA_ROOT,
    };
    unsafe {
        let hit = WindowFromPoint(pt);
        let root = GetAncestor(hit, GA_ROOT);
        if root.is_invalid() || root == not_this {
            return None;
        }
        let mut buf = [0u16; 64];
        let len = GetClassNameW(root, &mut buf);
        if String::from_utf16_lossy(&buf[..len.max(0) as usize]) != "DriveMainWindow" {
            return None;
        }
        let mut client = pt;
        if !ScreenToClient(root, &mut client).as_bool() {
            return None;
        }
        let scale = GetDpiForWindow(root) as f32 / 96.0;
        ((client.y as f32) < crate::ui::TAB_BAR_HEIGHT * scale).then_some(root)
    }
}

/// Sends the tab to `target` (path + screen x, WM_COPYDATA). true if the other
/// window took it — the counterpart of the original's `TabDropHandledIdentifier`.
fn send_tab_to_window(target: HWND, group: &crate::view_models::shell_view_model::TabGroup, x_screen: i32) -> bool {
    use windows::Win32::System::DataExchange::COPYDATASTRUCT;
    use windows::Win32::UI::WindowsAndMessaging::{
        SendMessageTimeoutW, SMTO_ABORTIFHUNG, WM_COPYDATA,
    };
    let path = match &group.active().location {
        Location::Dir(p) => p.to_string_lossy().into_owned(),
        _ => String::new(),
    };
    let payload = format!("{x_screen}|{path}");
    let wide: Vec<u16> = payload.encode_utf16().collect();
    let cds = COPYDATASTRUCT {
        dwData: TAB_DROP_COPYDATA,
        cbData: (wide.len() * 2) as u32,
        lpData: wide.as_ptr() as *mut _,
    };
    let mut result = 0usize;
    unsafe {
        let _ = SendMessageTimeoutW(
            target,
            WM_COPYDATA,
            WPARAM(0),
            LPARAM(&cds as *const _ as isize),
            SMTO_ABORTIFHUNG,
            1000,
            Some(&mut result),
        );
    }
    result == 1
}

/// Posted by the shell worker when a query (or an invoke) has completed.
const WM_APP_SHELL_MENU: u32 = 0x8000 + 3; // WM_APP + 3
/// Posted by a git worker thread when a network/checkout operation completes.
const WM_APP_GIT_DONE: u32 = 0x8000 + 4; // WM_APP + 4
/// Tab insertion preview (cross-window hover, see `post_tab_preview`).
const WM_APP_TAB_PREVIEW: u32 = 0x8000 + 5; // WM_APP + 5
/// Preview safety net: if the source stops refreshing (crash, missed drop),
/// the gap closes itself.
const TAB_PREVIEW_TIMER: usize = 48;


// ── Sub-modules (each file = an `impl MainWindow` block, mirroring a
//    file from the original). See the correspondence table in
//    docs/ARCHITECTURE (window split like MainWindow/MainPage/…).
mod lifecycle;
mod render;
mod message;
mod pointer_input;
mod toolbar_clicks;
mod sidebar_clicks;
mod tabs;
mod navigation;
mod context_menus;
mod menu_commands;
mod popup;
mod file_operations;
mod dialog_actions;
mod layout_view;
mod panes;
mod settings;
mod appearance_settings;
mod git;


extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        if msg == WM_NCCREATE {
            let create = &*(lparam.0 as *const CREATESTRUCTW);
            let window = create.lpCreateParams as *mut MainWindow;
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, window as isize);
            (*window).init(hwnd);
            return DefWindowProcW(hwnd, msg, wparam, lparam);
        }

        let window = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut MainWindow;
        if window.is_null() {
            return DefWindowProcW(hwnd, msg, wparam, lparam);
        }

        // We NO LONGER delegate to DwmDefWindowProc: it reserved (hit-test)
        // and drew the system caption buttons, bigger than WinUI's. We draw
        // and handle them ourselves (draw_caption_buttons + Hot::Caption* in
        // on_click), so their clicks must reach our nc_hit_test (HTCLIENT)
        // instead of being swallowed by DWM. (Trade-off: no more Win11
        // "snap layouts" hover on Maximize.)
        match (*window).handle_message(msg, wparam, lparam) {
            Some(result) => result,
            None => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

/// Image picker for the Appearance background image (SelectImageCommand).
/// Folder picker (FOS_PICKFOLDERS) — GeneralPage "Ajouter une page > Parcourir".
fn pick_folder(hwnd: HWND) -> Option<String> {
    use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};
    use windows::Win32::UI::Shell::{
        FileOpenDialog, IFileOpenDialog, FOS_PICKFOLDERS, SIGDN_FILESYSPATH,
    };
    unsafe {
        let dialog: IFileOpenDialog =
            CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;
        let options = dialog.GetOptions().unwrap_or_default();
        dialog.SetOptions(options | FOS_PICKFOLDERS).ok()?;
        dialog.Show(Some(hwnd)).ok()?;
        let item = dialog.GetResult().ok()?;
        let name = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        let path = name.to_string().ok();
        windows::Win32::System::Com::CoTaskMemFree(Some(name.0 as _));
        path
    }
}

/// JSON open dialog — AdvancedPage "Importer les paramètres".
fn pick_open_json(hwnd: HWND) -> Option<String> {
    use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};
    use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
    use windows::Win32::UI::Shell::{FileOpenDialog, IFileOpenDialog, SIGDN_FILESYSPATH};
    unsafe {
        let dialog: IFileOpenDialog =
            CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;
        let filters = [COMDLG_FILTERSPEC { pszName: w!("JSON"), pszSpec: w!("*.json") }];
        dialog.SetFileTypes(&filters).ok()?;
        dialog.Show(Some(hwnd)).ok()?;
        let item = dialog.GetResult().ok()?;
        let name = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        let path = name.to_string().ok();
        windows::Win32::System::Com::CoTaskMemFree(Some(name.0 as _));
        path
    }
}

/// JSON save dialog — AdvancedPage "Exporter les paramètres".
fn pick_save_json(hwnd: HWND) -> Option<String> {
    use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};
    use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
    use windows::Win32::UI::Shell::{FileSaveDialog, IFileSaveDialog, SIGDN_FILESYSPATH};
    unsafe {
        let dialog: IFileSaveDialog =
            CoCreateInstance(&FileSaveDialog, None, CLSCTX_INPROC_SERVER).ok()?;
        let filters = [COMDLG_FILTERSPEC { pszName: w!("JSON"), pszSpec: w!("*.json") }];
        dialog.SetFileTypes(&filters).ok()?;
        dialog.SetFileName(w!("settings.json")).ok()?;
        dialog.SetDefaultExtension(w!("json")).ok()?;
        dialog.Show(Some(hwnd)).ok()?;
        let item = dialog.GetResult().ok()?;
        let name = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        let path = name.to_string().ok();
        windows::Win32::System::Com::CoTaskMemFree(Some(name.0 as _));
        path
    }
}

/// Windows version string like `AboutViewModel.GetWindowsVersion()`
/// ("major.minor.build.revision"), read from the registry.
fn windows_version() -> String {
    use windows::Win32::System::Registry::{RegGetValueW, HKEY_LOCAL_MACHINE, RRF_RT_REG_DWORD, RRF_RT_REG_SZ};
    unsafe {
        let key = w!("SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion");
        let dword = |name: windows::core::PCWSTR| -> u32 {
            let mut value = 0u32;
            let mut size = std::mem::size_of::<u32>() as u32;
            let _ = RegGetValueW(
                HKEY_LOCAL_MACHINE,
                key,
                name,
                RRF_RT_REG_DWORD,
                None,
                Some(&mut value as *mut _ as *mut _),
                Some(&mut size),
            );
            value
        };
        let major = dword(w!("CurrentMajorVersionNumber"));
        let minor = dword(w!("CurrentMinorVersionNumber"));
        let ubr = dword(w!("UBR"));
        let mut build_buf = [0u16; 32];
        let mut size = (build_buf.len() * 2) as u32;
        let _ = RegGetValueW(
            HKEY_LOCAL_MACHINE,
            key,
            w!("CurrentBuildNumber"),
            RRF_RT_REG_SZ,
            None,
            Some(build_buf.as_mut_ptr() as *mut _),
            Some(&mut size),
        );
        let build = String::from_utf16_lossy(&build_buf)
            .trim_end_matches('\0')
            .to_string();
        format!("{major}.{minor}.{build}.{ubr}")
    }
}

fn pick_image_file(hwnd: HWND) -> Option<String> {
    use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};
    use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
    use windows::Win32::UI::Shell::{FileOpenDialog, IFileOpenDialog, SIGDN_FILESYSPATH};
    unsafe {
        let dialog: IFileOpenDialog =
            CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;
        let filters = [COMDLG_FILTERSPEC {
            pszName: w!("Images"),
            pszSpec: w!("*.png;*.jpg;*.jpeg;*.bmp;*.gif;*.tif;*.tiff;*.webp"),
        }];
        dialog.SetFileTypes(&filters).ok()?;
        dialog.Show(Some(hwnd)).ok()?;
        let item = dialog.GetResult().ok()?;
        let name = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        let path = name.to_string().ok();
        windows::Win32::System::Com::CoTaskMemFree(Some(name.0 as _));
        path
    }
}

pub fn run_message_loop() -> i32 {
    unsafe {
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        msg.wParam.0 as i32
    }
}
