//! FileThumbnailHelper (mirrors FileThumbnailHelper.cs)
//!
//! Port of `Files.App/Utils/Storage/Helpers/FileThumbnailHelper.cs`: the
//! (synchronous and STA worker) extraction of shell thumbnails + the
//! Direct2D upload. NOTE: the C# counterpart lives under
//! `Utils/Storage/Helpers/`; a strict mirror would place it in
//! `utils/storage/helpers/file_thumbnail_helper.rs`. It stays here to
//! keep the `thumbnails` module cohesive. `upload` (D2D) and
//! `WM_APP_ICON_READY` are immediate-rendering infra with no strict C#
//! counterpart.

use kubuno_drive_desktop_app_storage::windows_storage::WindowsStorable;
use windows::Win32::Graphics::Direct2D::Common::{D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_PIXEL_FORMAT, D2D_SIZE_U};
use windows::Win32::Graphics::Direct2D::{
    ID2D1Bitmap1, ID2D1DeviceContext, D2D1_BITMAP_OPTIONS_NONE, D2D1_BITMAP_PROPERTIES1,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::UI::Shell::{SIIGBF_BIGGERSIZEOK, SIIGBF_ICONONLY};

use crate::services::storage::icon_cache_service::Key;

/// Posted to the UI thread when background thumbnails are ready.
// ⚠ 0x8000+3 was in COLLISION with WM_APP_SHELL_MENU (main_window.rs):
// its match arm, placed earlier, intercepted the message and the
// asynchronous thumbnail drain never ran. The WM_APP values are numbered:
// 1 DIR_CHANGED, 2 OPS_PROGRESS, 3 SHELL_MENU, 4 GIT_DONE, 5 TAB_PREVIEW.
pub const WM_APP_ICON_READY: u32 = 0x8000 + 6; // WM_APP + 6

/// Dedicated STA worker: extracts thumbnails and notifies the UI thread.
pub(crate) fn spawn_worker(
    results: crate::services::storage::icon_cache_service::ThumbResults,
    hwnd_raw: isize,
) -> std::sync::mpsc::Sender<Key> {
    use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
    use windows::Win32::UI::WindowsAndMessaging::PostMessageW;

    let (tx, rx) = std::sync::mpsc::channel::<Key>();
    std::thread::spawn(move || {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        }
        while let Ok(key) = rx.recv() {
            let (path, size_px, _) = &key;
            // Like `fetch_icon`: target a NATIVE shell frame (…48, 256).
            // At an intermediate size (e.g. 245), the shell fails on
            // folders — the original also requests Jumbo=256 for the
            // pane (FolderPreviewViewModel), hence its "open" folder.
            const STANDARD: [i32; 5] = [16, 24, 32, 48, 256];
            let fetch_px = STANDARD.iter().copied().find(|s| *s >= *size_px).unwrap_or(256);
            let shell_bitmap = WindowsStorable::try_parse(path).and_then(|item| {
                match item.storable().try_get_thumbnail(fetch_px, SIIGBF_BIGGERSIZEOK) {
                    Ok(b) => Some(b),
                    Err(e) => {
                        tracing::warn!("async thumbnail failed for {path} at {fetch_px}px: {e}");
                        None
                    }
                }
            });
            results.lock().unwrap().push((key, shell_bitmap));
            unsafe {
                let _ = PostMessageW(
                    Some(HWND(hwnd_raw as *mut _)),
                    WM_APP_ICON_READY,
                    WPARAM(0),
                    LPARAM(0),
                );
            }
        }
    });
    tx
}

pub(crate) fn fetch_icon(ctx: &ID2D1DeviceContext, path: &str, size_px: i32, thumbnail: bool) -> Option<ID2D1Bitmap1> {
    let item = WindowsStorable::try_parse(path)?;
    // Icons only exist at the shell image-list sizes (16/24/32/48 plus the
    // 256 jumbo frame); asking for anything else makes the shell resample
    // (jagged or blurry edges). Fetch the next native size UP and let
    // Direct2D downscale with high-quality cubic filtering instead. There is
    // deliberately no 96 step: 96 is not a native frame, so the shell would
    // synthesize it from 48 or 256 and the icon would be resampled twice
    // (shell then Direct2D). Above 48, go straight to the native 256 frame.
    const STANDARD: [i32; 5] = [16, 24, 32, 48, 256];
    let fetch_px = STANDARD
        .iter()
        .copied()
        .find(|s| *s >= size_px)
        .unwrap_or(256);
    // Thumbnails (grid tiles): let the shell return the content preview
    // (images, PDFs…) and fall back to the icon itself.
    let flags = if thumbnail {
        SIIGBF_BIGGERSIZEOK
    } else {
        SIIGBF_ICONONLY | SIIGBF_BIGGERSIZEOK
    };
    let shell_bitmap = match item.storable().try_get_thumbnail(fetch_px, flags) {
        Ok(b) => b,
        Err(e) => {
            tracing::warn!("icon fetch failed for {path} at {fetch_px}px: {e}");
            return None;
        }
    };
    upload(ctx, &shell_bitmap)
}

/// Uploads decoded BGRA pixels into a D2D bitmap.
pub(crate) fn upload(ctx: &ID2D1DeviceContext, shell_bitmap: &kubuno_drive_desktop_app_storage::ShellBitmap) -> Option<ID2D1Bitmap1> {
    let props = D2D1_BITMAP_PROPERTIES1 {
        pixelFormat: D2D1_PIXEL_FORMAT {
            format: DXGI_FORMAT_B8G8R8A8_UNORM,
            alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
        },
        dpiX: 96.0,
        dpiY: 96.0,
        bitmapOptions: D2D1_BITMAP_OPTIONS_NONE,
        colorContext: std::mem::ManuallyDrop::new(None),
    };
    unsafe {
        ctx.CreateBitmap(
            D2D_SIZE_U {
                width: shell_bitmap.width,
                height: shell_bitmap.height,
            },
            Some(shell_bitmap.bgra.as_ptr() as *const _),
            shell_bitmap.width * 4,
            &props,
        )
        .ok()
    }
}
