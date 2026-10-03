//! Port of `Windows/Helpers/WindowsStorableHelpers.Icon.cs`.
//!
//! Deviation from the C# implementation: instead of re-encoding the thumbnail
//! through GDI+ into PNG bytes, the raw 32-bpp BGRA pixels (top-down row
//! order) are returned together with the dimensions. The UI consumes them
//! directly as a Direct2D bitmap.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use windows::core::{Error, Interface, HSTRING};
use windows::Win32::Foundation::{E_FAIL, SIZE};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, DeleteDC, DeleteObject, GetDIBits, GetObjectW, BITMAP, BITMAPINFO,
    BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HBITMAP,
};
use windows::Win32::UI::Shell::{IShellItemImageFactory, SHDefExtractIconW, SIIGBF};
use windows::Win32::UI::WindowsAndMessaging::{DestroyIcon, GetIconInfo, HICON, ICONINFO};

use crate::windows_storage::WindowsStorable;

/// `E_PENDING` ("the data necessary to complete this operation is not yet
/// available") — not exposed by the `windows` crate.
const E_PENDING: windows::core::HRESULT = windows::core::HRESULT(0x8000000A_u32 as i32);

/// Raw 32-bpp BGRA bitmap (top-down row order) with **premultiplied** alpha,
/// ready to be wrapped in a `D2D1_ALPHA_MODE_PREMULTIPLIED` Direct2D bitmap
/// by the UI layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellBitmap {
    pub width: u32,
    pub height: u32,
    /// `width * height * 4` bytes, BGRA premultiplied, first row is the top
    /// row.
    pub bgra: Vec<u8>,
}

/// Port of the `DllIconCache` field.
type DllIconCache = Mutex<HashMap<(String, i32, u32), ShellBitmap>>;

fn dll_icon_cache() -> &'static DllIconCache {
    static CACHE: OnceLock<DllIconCache> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Normalizes a BGRA buffer to premultiplied alpha, in place.
///
/// Direct2D consumes these pixels as `D2D1_ALPHA_MODE_PREMULTIPLIED`, but the
/// shell is inconsistent about what it hands out:
/// - `IShellItemImageFactory::GetImage` documents PARGB32, yet with
///   `SIIGBF_ICONONLY` it frequently returns the icon's straight-alpha ARGB
///   pixels unchanged (observed on Windows 11 for drives and file icons);
/// - `HICON` color bitmaps are straight-alpha by definition;
/// - legacy providers return 32-bpp bitmaps whose alpha channel is all zero.
///
/// Feeding straight alpha to a premultiplied-alpha D2D bitmap makes every
/// anti-aliased edge pixel blend far too bright (e.g. a coverage-32 edge pixel
/// with color 151 contributes 151 instead of 151*32/255 = 19), which shows up
/// as light fringes / jagged edges.
///
/// Detection: premultiplied pixels satisfy `channel <= alpha` for every
/// channel. If any pixel violates that, the buffer is straight alpha and is
/// premultiplied here. If the alpha channel is zero everywhere while colors
/// are not, the bitmap has no alpha information at all and is made opaque
/// (premultiplying would erase it).
fn premultiply_alpha_in_place(bgra: &mut [u8]) {
    let mut any_alpha = false;
    let mut any_color = false;
    let mut straight = false;
    for px in bgra.as_chunks::<4>().0 {
        let a = px[3];
        any_alpha |= a != 0;
        any_color |= px[0] != 0 || px[1] != 0 || px[2] != 0;
        straight |= px[0] > a || px[1] > a || px[2] > a;
    }

    if !any_alpha {
        if any_color {
            // 32-bpp bitmap without an alpha channel: treat as opaque.
            for px in bgra.as_chunks_mut::<4>().0 {
                px[3] = 255;
            }
        }
        return;
    }

    if straight {
        for px in bgra.as_chunks_mut::<4>().0 {
            let a = px[3] as u32;
            for c in &mut px[..3] {
                *c = ((*c as u32 * a + 127) / 255) as u8;
            }
        }
    }
}

/// Copies the pixels of `hbitmap` into a top-down 32-bpp BGRA buffer.
///
/// Replaces the manual row flip + GDI+ encoding of the C# version: asking
/// `GetDIBits` for a negative-height DIB yields top-down rows directly.
///
/// # Safety
/// `hbitmap` must be a valid GDI bitmap handle. Ownership is not taken; the
/// caller remains responsible for deleting it.
unsafe fn hbitmap_to_bgra(hbitmap: HBITMAP) -> windows::core::Result<ShellBitmap> {
    let mut bmp = BITMAP::default();
    if GetObjectW(
        hbitmap.into(),
        std::mem::size_of::<BITMAP>() as i32,
        Some(&mut bmp as *mut _ as *mut _),
    ) == 0
    {
        return Err(Error::from_hresult(E_FAIL));
    }

    let width = bmp.bmWidth;
    let height = bmp.bmHeight;
    if width <= 0 || height <= 0 {
        return Err(Error::from_hresult(E_FAIL));
    }

    let mut info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
        biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: width,
        // Negative height requests a top-down DIB (performs the vertical flip).
        biHeight: -height,
        biPlanes: 1,
        biBitCount: 32,
        biCompression: BI_RGB.0,
        ..Default::default()
        },
        ..Default::default()
    };

    let mut pixels = vec![0u8; width as usize * height as usize * 4];

    let hdc = CreateCompatibleDC(None);
    let lines = GetDIBits(
        hdc,
        hbitmap,
        0,
        height as u32,
        Some(pixels.as_mut_ptr() as *mut _),
        &mut info,
        DIB_RGB_COLORS,
    );
    let _ = DeleteDC(hdc);

    if lines == 0 {
        return Err(Error::from_hresult(E_FAIL));
    }

    // GetDIBits preserves the source alpha bytes verbatim for 32-bpp DIBs,
    // so whether the buffer is premultiplied depends entirely on the
    // provider; normalize before handing it to Direct2D.
    premultiply_alpha_in_place(&mut pixels);

    Ok(ShellBitmap { width: width as u32, height: height as u32, bgra: pixels })
}

/// Converts an `HICON` into BGRA pixels via its color bitmap.
///
/// # Safety
/// `hicon` must be a valid icon handle; ownership is not taken.
unsafe fn hicon_to_bgra(hicon: HICON) -> windows::core::Result<ShellBitmap> {
    let mut icon_info = ICONINFO::default();
    GetIconInfo(hicon, &mut icon_info)?;

    let result = if icon_info.hbmColor.is_invalid() {
        Err(Error::from_hresult(E_FAIL))
    } else {
        hbitmap_to_bgra(icon_info.hbmColor)
    };

    if !icon_info.hbmColor.is_invalid() {
        let _ = DeleteObject(icon_info.hbmColor.into());
    }
    if !icon_info.hbmMask.is_invalid() {
        let _ = DeleteObject(icon_info.hbmMask.into());
    }

    result
}

impl WindowsStorable {
    /// Port of `TryGetThumbnail` — `IShellItemImageFactory::GetImage`.
    ///
    /// Prefer running this on an STA thread (see [`crate::sta_thread`]); some
    /// thumbnail providers require it.
    pub fn try_get_thumbnail(&self, size: i32, options: SIIGBF) -> windows::core::Result<ShellBitmap> {
        // SAFETY: the HBITMAP returned by GetImage is owned here and deleted
        // on every exit path.
        unsafe {
            let factory: IShellItemImageFactory = self.shell_item().cast()?;

            // GetImage transiently returns E_PENDING ("data not yet
            // available") when the shell is already extracting an image on
            // another thread; retry briefly instead of failing the icon.
            let mut attempts = 0;
            let hbitmap = loop {
                match factory.GetImage(SIZE { cx: size, cy: size }, options) {
                    Ok(hbitmap) => break hbitmap,
                    Err(e) if e.code() == E_PENDING && attempts < 10 => {
                        attempts += 1;
                        std::thread::sleep(std::time::Duration::from_millis(20));
                    }
                    Err(e) => return Err(e),
                }
            };

            let bitmap = hbitmap_to_bgra(hbitmap);
            let _ = DeleteObject(hbitmap.into());
            bitmap
        }
    }
}

/// Port of `GetThumbnailAsync` — resolves `path`, then extracts the thumbnail
/// on a dedicated STA thread.
pub async fn get_thumbnail_async(path: String, size: i32, options: SIIGBF) -> Option<ShellBitmap> {
    crate::sta_thread::run(move || {
        let item = WindowsStorable::try_parse(&path)?;
        item.storable().try_get_thumbnail(size, options).ok()
    })
    .await
    .flatten()
}

/// Port of `TryExtractImageFromDll` — `SHDefExtractIconW`, with the same
/// `(path, index, size)` cache as the C# version.
pub fn try_extract_image_from_dll(
    path: &str,
    index: i32,
    size: u32,
) -> windows::core::Result<ShellBitmap> {
    let key = (path.to_string(), index, size);
    if let Some(cached) = dll_icon_cache().lock().unwrap().get(&key) {
        return Ok(cached.clone());
    }

    // SAFETY: the icon handle is owned here and destroyed on every exit path.
    let bitmap = unsafe {
        let mut hicon = HICON::default();
        let hr = SHDefExtractIconW(&HSTRING::from(path), -index, 0, Some(&mut hicon), None, size);
        if hr.is_err() {
            if !hicon.is_invalid() {
                let _ = DestroyIcon(hicon);
            }
            return Err(Error::from_hresult(hr));
        }
        if hicon.is_invalid() {
            return Err(Error::from_hresult(E_FAIL));
        }

        let bitmap = hicon_to_bgra(hicon);
        let _ = DestroyIcon(hicon);
        bitmap?
    };

    dll_icon_cache().lock().unwrap().insert(key, bitmap.clone());
    Ok(bitmap)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests_support::init_com;
    use crate::windows_storage::WindowsStorable;
    use windows::Win32::UI::Shell::{SIIGBF_BIGGERSIZEOK, SIIGBF_ICONONLY};

    /// Counts pixels per alpha class and premultiplied-invariant violations.
    fn alpha_stats(bitmap: &ShellBitmap) -> (usize, usize, usize, usize) {
        let (mut zero, mut opaque, mut mid, mut violations) = (0, 0, 0, 0);
        for px in bitmap.bgra.as_chunks::<4>().0 {
            match px[3] {
                0 => zero += 1,
                255 => opaque += 1,
                _ => mid += 1,
            }
            if px[0] > px[3] || px[1] > px[3] || px[2] > px[3] {
                violations += 1;
            }
        }
        (zero, opaque, mid, violations)
    }

    #[test]
    fn premultiply_converts_straight_alpha() {
        // Pixel with coverage 51 (20%): straight color 200 must become 40.
        let mut bgra = vec![200, 100, 0, 51, 255, 255, 255, 255];
        premultiply_alpha_in_place(&mut bgra);
        assert_eq!(bgra, vec![40, 20, 0, 51, 255, 255, 255, 255]);
    }

    #[test]
    fn premultiply_keeps_premultiplied_data_untouched() {
        // channel <= alpha everywhere: already premultiplied, must not be
        // premultiplied a second time (which would darken edges).
        let original = vec![40u8, 20, 0, 51, 128, 64, 32, 255, 0, 0, 0, 0];
        let mut bgra = original.clone();
        premultiply_alpha_in_place(&mut bgra);
        assert_eq!(bgra, original);
    }

    #[test]
    fn premultiply_makes_zero_alpha_bitmaps_opaque() {
        // 32-bpp bitmap without alpha information: colors set, alpha all 0.
        let mut bgra = vec![10u8, 20, 30, 0, 40, 50, 60, 0];
        premultiply_alpha_in_place(&mut bgra);
        assert_eq!(bgra, vec![10, 20, 30, 255, 40, 50, 60, 255]);
    }

    /// The full pipeline must yield anti-aliased (intermediate-alpha) and
    /// premultiplied pixels, otherwise Direct2D renders jagged/fringed edges.
    #[test]
    fn thumbnail_is_antialiased_and_premultiplied() {
        init_com();
        // A throwaway .txt exercises the generic text-file icon. (Do not
        // reuse the files of other tests: concurrent GetImage calls on the
        // same item can transiently return E_PENDING.)
        let txt = std::env::temp_dir().join("drive_app_storage_icon_probe.txt");
        std::fs::write(&txt, b"probe").expect("write probe txt");
        let txt_path = txt.to_string_lossy().into_owned();

        for path in ["C:\\", txt_path.as_str()] {
            let item = WindowsStorable::try_parse(path).expect("path should parse");
            let bitmap = item
                .storable()
                .try_get_thumbnail(96, SIIGBF_ICONONLY | SIIGBF_BIGGERSIZEOK)
                .expect("icon should extract");
            let (_, _, mid, violations) = alpha_stats(&bitmap);
            assert!(
                mid > 0,
                "{path}: no intermediate alpha; icon edges are not anti-aliased"
            );
            assert_eq!(
                violations, 0,
                "{path}: color channel exceeds alpha; buffer is straight alpha, \
                 not the premultiplied alpha Direct2D expects"
            );
        }
    }

    #[test]
    fn extract_shell32_icon() {
        init_com();
        let bitmap = try_extract_image_from_dll("C:\\Windows\\System32\\shell32.dll", 0, 32)
            .expect("shell32.dll icon 0 should extract");
        assert_eq!(bitmap.width, 32);
        assert_eq!(bitmap.height, 32);
        assert_eq!(bitmap.bgra.len(), 32 * 32 * 4);

        // Second call must hit the cache and return identical data.
        let cached = try_extract_image_from_dll("C:\\Windows\\System32\\shell32.dll", 0, 32).unwrap();
        assert_eq!(bitmap, cached);
    }

    #[test]
    fn thumbnail_of_known_file() {
        init_com();
        let item = WindowsStorable::try_parse("C:\\Windows\\System32\\ntdll.dll")
            .expect("ntdll should parse");
        let bitmap = item
            .storable()
            .try_get_thumbnail(32, SIIGBF_ICONONLY)
            .expect("icon thumbnail should extract");
        assert!(bitmap.width > 0 && bitmap.height > 0);
        assert_eq!(bitmap.bgra.len(), (bitmap.width * bitmap.height * 4) as usize);
    }
}
