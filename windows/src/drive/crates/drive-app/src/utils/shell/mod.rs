//! Port of `Files.App/Utils/Shell/` — the native shell context menu.
//!
//! NOTE: `imageres_icon` below does NOT belong to `ContextMenu` — it's
//! `Helpers/UI/UIHelpers.GetSidebarIconResource`. A strict mirror would
//! place it in `utils/helpers/ui/ui_helpers.rs`. It stays here to
//! preserve `crate::utils::shell::imageres_icon` (imported by
//! `utils/thumbnails.rs`).

pub mod context_menu;
pub mod context_menu_item;

pub use context_menu::*;

/// Extracts an icon from `imageres.dll` by INDEX — the C#'s
/// `UIHelpers.GetSidebarIconResource` (sidebar Drives/Network sections:
/// `Constants.ImageRes.ThisPC`, `Network`…).
pub fn imageres_icon(index: i32, size_px: i32) -> Option<drive_app_storage::ShellBitmap> {
    use windows::core::w;
    use windows::Win32::Graphics::Gdi::{
        CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, SelectObject, BITMAPINFO,
        BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS,
    };
    use windows::Win32::UI::Shell::SHDefExtractIconW;
    use windows::Win32::UI::WindowsAndMessaging::{DestroyIcon, DrawIconEx, DI_NORMAL, HICON};

    unsafe {
        let mut hicon = HICON::default();
        // `ExtractSelectedIconsFromDLL` passes `-index`: a negative
        // nIconIndex designates the RESOURCE ID (a positive ordinal would
        // give a completely different icon).
        if SHDefExtractIconW(
            w!(r"C:\Windows\System32\imageres.dll"),
            -index,
            0,
            Some(&mut hicon),
            None,
            size_px as u32,
        )
        .is_err()
            || hicon.is_invalid()
        {
            return None;
        }

        // HICON → 32bpp top-down DIB, alpha preserved by DrawIconEx.
        let dc = CreateCompatibleDC(None);
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: size_px,
                biHeight: -size_px,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits: *mut std::ffi::c_void = std::ptr::null_mut();
        let Ok(dib) = CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut bits, None, 0)
        else {
            let _ = DeleteDC(dc);
            let _ = DestroyIcon(hicon);
            return None;
        };
        let old = SelectObject(dc, dib.into());
        let ok = DrawIconEx(dc, 0, 0, hicon, size_px, size_px, 0, None, DI_NORMAL).is_ok();
        let pixels = if ok {
            let len = (size_px * size_px * 4) as usize;
            Some(std::slice::from_raw_parts(bits as *const u8, len).to_vec())
        } else {
            None
        };
        SelectObject(dc, old);
        let _ = DeleteObject(dib.into());
        let _ = DeleteDC(dc);
        let _ = DestroyIcon(hicon);

        pixels.map(|bgra| drive_app_storage::ShellBitmap {
            width: size_px as u32,
            height: size_px as u32,
            bgra,
        })
    }
}
