//! The Windows clipboard, multi-format: what the core's payloads become on it and back
//! (`kubuno_office_docs_core::edit::clipboard`).
//!
//! Out: our private format (`Kubuno.Documents.Blocks`), `HTML Format` (`CF_HTML`, UTF-8) and
//! `CF_UNICODETEXT` — in that order, since consumers take the first format they understand.
//! In: the same three, plus an image — the registered `PNG` format, else `CF_DIB` (made into a
//! `.bmp` WIC can decode), else one image file copied in the Explorer (`CF_HDROP`).
//!
//! The traps (from the first, stubbed version of this module): `OpenClipboard` must always be
//! closed ([`Guard`]); after `SetClipboardData` succeeds the system owns the handle (freeing it is
//! a double free), after it fails we still own it (not freeing it leaks); `GlobalSize` rounds up,
//! so payloads come back with trailing NULs; the clipboard may be held by another process for a few
//! milliseconds, so opening retries briefly — never an unbounded wait on the UI thread.

use std::sync::OnceLock;

use windows::core::PCWSTR;
use windows::Win32::Foundation::{GlobalFree, HANDLE, HGLOBAL, HWND};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::UI::Shell::{DragQueryFileW, HDROP};

const CF_UNICODETEXT: u32 = 13;
const CF_DIB: u32 = 8;
const CF_HDROP: u32 = 15;
const OPEN_RETRIES: u32 = 5;
const OPEN_RETRY_DELAY_MS: u64 = 20;

struct Guard;

impl Guard {
    fn open(owner: HWND) -> Option<Self> {
        for attempt in 0..OPEN_RETRIES {
            if unsafe { OpenClipboard(Some(owner)) }.is_ok() {
                return Some(Self);
            }
            if attempt + 1 < OPEN_RETRIES {
                std::thread::sleep(std::time::Duration::from_millis(OPEN_RETRY_DELAY_MS));
            }
        }
        kubuno_desktop::tracing::warn!("[documents] clipboard busy: OpenClipboard failed {OPEN_RETRIES} times");
        None
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseClipboard();
        }
    }
}

fn registered(name: &str, cell: &'static OnceLock<u32>) -> u32 {
    *cell.get_or_init(|| {
        let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
        unsafe { RegisterClipboardFormatW(PCWSTR(wide.as_ptr())) }
    })
}

fn private_format() -> u32 {
    static CELL: OnceLock<u32> = OnceLock::new();
    registered(kubuno_office_docs_core::edit::clipboard::PRIVATE_FORMAT, &CELL)
}

fn html_format() -> u32 {
    static CELL: OnceLock<u32> = OnceLock::new();
    registered(kubuno_office_docs_core::edit::clipboard::HTML_FORMAT, &CELL)
}

fn png_format() -> u32 {
    static CELL: OnceLock<u32> = OnceLock::new();
    registered("PNG", &CELL)
}

fn alloc(bytes: &[u8]) -> Option<HGLOBAL> {
    let h = unsafe { GlobalAlloc(GMEM_MOVEABLE, bytes.len().max(1)) }.ok()?;
    let p = unsafe { GlobalLock(h) };
    if p.is_null() {
        unsafe {
            let _ = GlobalFree(Some(h));
        }
        return None;
    }
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), p.cast::<u8>(), bytes.len());
        let _ = GlobalUnlock(h);
    }
    Some(h)
}

fn put(format: u32, bytes: &[u8]) -> Result<(), String> {
    if format == 0 {
        return Err("clipboard: format could not be registered".into());
    }
    let h = alloc(bytes).ok_or_else(|| "clipboard: out of memory".to_string())?;
    match unsafe { SetClipboardData(format, Some(HANDLE(h.0))) } {
        Ok(_) => Ok(()),
        Err(e) => {
            unsafe {
                let _ = GlobalFree(Some(h));
            }
            Err(format!("clipboard: SetClipboardData failed: {e}"))
        }
    }
}

fn get(format: u32) -> Option<Vec<u8>> {
    if format == 0 || unsafe { IsClipboardFormatAvailable(format) }.is_err() {
        return None;
    }
    let handle = unsafe { GetClipboardData(format) }.ok()?;
    let h = HGLOBAL(handle.0);
    let size = unsafe { GlobalSize(h) };
    if size == 0 {
        return None;
    }
    let p = unsafe { GlobalLock(h) };
    if p.is_null() {
        return None;
    }
    let mut out = vec![0u8; size];
    unsafe {
        std::ptr::copy_nonoverlapping(p.cast::<u8>(), out.as_mut_ptr(), size);
        let _ = GlobalUnlock(h);
    }
    Some(out)
}

fn trim_nuls(mut v: Vec<u8>) -> Vec<u8> {
    while v.last() == Some(&0) {
        v.pop();
    }
    v
}

/// Puts a copy on the clipboard: the private payload, the `CF_HTML` string and the text (CRLF).
pub fn write(owner: HWND, private: &[u8], cf_html: &str, text: &str) -> Result<(), String> {
    let _guard = Guard::open(owner).ok_or_else(|| "clipboard: busy".to_string())?;
    unsafe { EmptyClipboard() }.map_err(|e| format!("clipboard: EmptyClipboard failed: {e}"))?;
    put(private_format(), private)?;
    put(html_format(), cf_html.as_bytes())?;
    put(CF_UNICODETEXT, &kubuno_office_docs_core::edit::clipboard::utf16_le_nul(text))
}

/// Everything readable on the clipboard, read in one opening.
#[derive(Debug, Default)]
pub struct Contents {
    pub private: Option<Vec<u8>>,
    pub html: Option<String>,
    pub text: Option<String>,
    /// Image bytes WIC can decode (PNG, a `.bmp` built from `CF_DIB`, or a copied image file).
    pub image: Option<Vec<u8>>,
}

/// A `.bmp` file from a packed DIB (`BITMAPFILEHEADER` + the DIB).
fn bmp_from_dib(dib: &[u8]) -> Option<Vec<u8>> {
    if dib.len() < 40 {
        return None;
    }
    let header_size = u32::from_le_bytes([dib[0], dib[1], dib[2], dib[3]]) as usize;
    let bit_count = u16::from_le_bytes([dib[14], dib[15]]);
    let compression = u32::from_le_bytes([dib[16], dib[17], dib[18], dib[19]]);
    let colors_used = u32::from_le_bytes([dib[32], dib[33], dib[34], dib[35]]) as usize;
    let palette = if bit_count <= 8 { (if colors_used > 0 { colors_used } else { 1 << bit_count }) * 4 } else { 0 };
    // BI_BITFIELDS with a 40-byte header: three masks follow it.
    let masks = if compression == 3 && header_size == 40 { 12 } else { 0 };
    let off_bits = 14 + header_size + masks + palette;
    let total = 14 + dib.len();
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&(total as u32).to_le_bytes());
    out.extend_from_slice(&[0, 0, 0, 0]);
    out.extend_from_slice(&(off_bits as u32).to_le_bytes());
    out.extend_from_slice(dib);
    Some(out)
}

fn copied_image_file() -> Option<Vec<u8>> {
    if unsafe { IsClipboardFormatAvailable(CF_HDROP) }.is_err() {
        return None;
    }
    let handle = unsafe { GetClipboardData(CF_HDROP) }.ok()?;
    let drop = HDROP(handle.0);
    let count = unsafe { DragQueryFileW(drop, u32::MAX, None) };
    if count != 1 {
        return None;
    }
    let len = unsafe { DragQueryFileW(drop, 0, None) } as usize;
    let mut buf = vec![0u16; len + 1];
    let got = unsafe { DragQueryFileW(drop, 0, Some(&mut buf)) } as usize;
    let path = String::from_utf16_lossy(&buf[..got]);
    let lower = path.to_ascii_lowercase();
    if [".png", ".jpg", ".jpeg", ".gif", ".bmp", ".webp", ".tif", ".tiff"].iter().any(|e| lower.ends_with(e)) {
        std::fs::read(&path).ok()
    } else {
        None
    }
}

/// Reads the clipboard.
pub fn read(owner: HWND) -> Contents {
    let Some(_guard) = Guard::open(owner) else { return Contents::default() };
    let private = get(private_format()).map(trim_nuls);
    let html = get(html_format()).map(trim_nuls).map(|b| String::from_utf8_lossy(&b).into_owned());
    let text = get(CF_UNICODETEXT).map(|b| kubuno_office_docs_core::edit::clipboard::utf16_le_to_string(&b));
    let image = get(png_format()).or_else(|| get(CF_DIB).and_then(|d| bmp_from_dib(&d))).or_else(copied_image_file);
    Contents { private, html, text, image }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_dib_becomes_a_bmp_with_the_right_offset() {
        // A 1×1 32-bit DIB: 40-byte header, no palette, 4 bytes of pixels.
        let mut dib = vec![0u8; 44];
        dib[0] = 40;
        dib[4] = 1;
        dib[8] = 1;
        dib[12] = 1;
        dib[14] = 32;
        let bmp = bmp_from_dib(&dib).expect("a bmp");
        assert_eq!(&bmp[..2], b"BM");
        assert_eq!(u32::from_le_bytes([bmp[10], bmp[11], bmp[12], bmp[13]]), 14 + 40);
        assert_eq!(bmp.len(), 14 + 44);
    }
}
