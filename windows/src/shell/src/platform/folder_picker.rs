//! The system folder picker.
//!
//! The Tauri build reached this through the dialog plugin; here it is the
//! shell's own `IFileOpenDialog` with `FOS_PICKFOLDERS`, which is what that
//! plugin called underneath.

use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::{
    CoCreateInstance, CLSCTX_INPROC_SERVER, IBindCtx,
};
use windows::Win32::UI::Shell::{
    FileOpenDialog, IFileOpenDialog, IShellItem, SHCreateItemFromParsingName,
    FOS_FORCEFILESYSTEM, FOS_PATHMUSTEXIST, FOS_PICKFOLDERS, SIGDN_FILESYSPATH,
};

/// Asks the user for a folder, starting at `current` when it exists. Returns
/// `None` when the dialog is dismissed — a cancel is not an error.
pub fn pick(owner: HWND, title: &str, current: &str) -> Option<String> {
    unsafe {
        let dialog: IFileOpenDialog =
            CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;
        // A folder picker is the file dialog in "pick folders" mode, restricted
        // to real file-system paths — a shell library would not be syncable.
        dialog
            .SetOptions(FOS_PICKFOLDERS | FOS_FORCEFILESYSTEM | FOS_PATHMUSTEXIST)
            .ok()?;
        let _ = dialog.SetTitle(&windows::core::HSTRING::from(title));

        if !current.trim().is_empty() {
            let path = windows::core::HSTRING::from(current);
            let bind: Option<&IBindCtx> = None;
            if let Ok(item) = SHCreateItemFromParsingName::<_, _, IShellItem>(&path, bind) {
                let _ = dialog.SetFolder(&item);
            }
        }

        // A dismissed dialog returns an error code; that is a normal outcome.
        dialog.Show(Some(owner)).ok()?;
        let item = dialog.GetResult().ok()?;
        let wide = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        let picked = wide.to_string().ok()?;
        windows::Win32::System::Com::CoTaskMemFree(Some(wide.as_ptr() as *const _));
        Some(picked)
    }
}
