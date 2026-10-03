//! Port of `Files.App/Services/Windows/WindowsWallpaperService.cs`.
//!
//! The DESKTOP background goes through `IDesktopWallpaper` (COM shell), applied to
//! each monitor; the slideshow via `SetSlideshow` on an `IShellItemArray`
//! built from the PIDLs; the LOCK screen via the WinRT
//! `LockScreen.SetImageFileAsync`.

use windows::core::{Result, HSTRING, PCWSTR};
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};
use windows::Win32::UI::Shell::{
    DesktopWallpaper, IDesktopWallpaper, ILCreateFromPathW, SHCreateShellItemArrayFromIDLists,
    DESKTOP_WALLPAPER_POSITION,
};

/// `SetDesktopWallpaper`: the image on ALL monitors.
pub fn set_desktop_wallpaper(path: &str) -> Result<()> {
    unsafe {
        let wallpaper: IDesktopWallpaper = CoCreateInstance(&DesktopWallpaper, None, CLSCTX_INPROC_SERVER)?;
        let count = wallpaper.GetMonitorDevicePathCount()?;
        let pszpath = HSTRING::from(path);
        for i in 0..count {
            let monitor = wallpaper.GetMonitorDevicePathAt(i)?;
            // SetWallpaper(monitorId, path) — monitorId is an allocated PWSTR
            // passed through as-is.
            let _ = wallpaper.SetWallpaper(PCWSTR(monitor.0), &pszpath);
            windows::Win32::System::Com::CoTaskMemFree(Some(monitor.0 as *const _));
        }
    }
    Ok(())
}

/// `SetDesktopSlideshow`: the selection as a desktop slideshow (FILL position).
pub fn set_desktop_slideshow(paths: &[String]) -> Result<()> {
    unsafe {
        let wallpaper: IDesktopWallpaper = CoCreateInstance(&DesktopWallpaper, None, CLSCTX_INPROC_SERVER)?;
        let pidls: Vec<*const windows::Win32::UI::Shell::Common::ITEMIDLIST> = paths
            .iter()
            .map(|p| ILCreateFromPathW(&HSTRING::from(p.as_str())) as *const _)
            .collect();
        let array = SHCreateShellItemArrayFromIDLists(&pidls)?;
        for pidl in &pidls {
            windows::Win32::System::Com::CoTaskMemFree(Some(*pidl as *const _));
        }
        wallpaper.SetSlideshow(&array)?;
        // DWPOS_FILL = 4.
        wallpaper.SetPosition(DESKTOP_WALLPAPER_POSITION(4))?;
    }
    Ok(())
}

/// `SetLockScreenWallpaper`: the lock screen image (WinRT).
pub fn set_lock_screen_wallpaper(path: &str) -> Result<()> {
    use windows::Storage::StorageFile;
    use windows::System::UserProfile::LockScreen;
    let file = StorageFile::GetFileFromPathAsync(&HSTRING::from(path))?.join()?;
    LockScreen::SetImageFileAsync(&file)?.join()?;
    Ok(())
}
