//! The system's file dialogs (`IFileOpenDialog`): Insertion › Image.

use std::path::PathBuf;

use windows::core::{w, PCWSTR};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED};
use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
use windows::Win32::UI::Shell::{FileOpenDialog, IFileOpenDialog, SIGDN_FILESYSPATH, FOS_FILEMUSTEXIST, FOS_FORCEFILESYSTEM};

/// Asks for one image file; `None` when cancelled.
pub fn open_image() -> Option<PathBuf> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let dialog: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;
        let filters = [
            COMDLG_FILTERSPEC { pszName: w!("Images"), pszSpec: w!("*.png;*.jpg;*.jpeg;*.gif;*.bmp;*.webp;*.tif;*.tiff") },
            COMDLG_FILTERSPEC { pszName: w!("*.*"), pszSpec: w!("*.*") },
        ];
        let _ = dialog.SetFileTypes(&filters);
        let opts = dialog.GetOptions().ok()?;
        let _ = dialog.SetOptions(opts | FOS_FILEMUSTEXIST | FOS_FORCEFILESYSTEM);
        let owner = crate::platform::ime::focus_window();
        dialog.Show(owner).ok()?;
        let item = dialog.GetResult().ok()?;
        let name = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        let path = PCWSTR(name.0).to_string().ok();
        windows::Win32::System::Com::CoTaskMemFree(Some(name.0 as *const _));
        path.map(PathBuf::from)
    }
}
