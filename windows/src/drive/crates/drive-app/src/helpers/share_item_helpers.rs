//! ShareItemHelpers (mirror of ShareItemHelpers.cs)
//!
//! Port of `ShareItemHelpers.ShareItemsAsync`: the Windows share sheet.
//!
//! Exactly the original's route — `DataTransferManager` is a WinRT type with no
//! window of its own, so a desktop app reaches it through
//! `IDataTransferManagerInterop`: `GetForWindow` to get the manager bound to our
//! HWND, a `DataRequested` handler that fills the `DataPackage`, then
//! `ShowShareUIForWindow`.

use windows::core::{Interface, Ref, Result, HSTRING};
use windows::ApplicationModel::DataTransfer::{DataRequestedEventArgs, DataTransferManager};
use windows::Foundation::TypedEventHandler;
use windows::Storage::{IStorageItem, StorageFile, StorageFolder};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Shell::IDataTransferManagerInterop;
use windows_collections::IIterable;

/// `ShareItemHelpers.IsItemShareable`: folders are not shareable (unless they
/// are archives, which are files to us anyway).
pub fn is_shareable(path: &str) -> bool {
    !std::path::Path::new(path).is_dir()
}

pub fn share_items(hwnd: HWND, paths: &[String]) -> Result<()> {
    let interop: IDataTransferManagerInterop =
        windows::core::factory::<DataTransferManager, IDataTransferManagerInterop>()?;
    let manager: DataTransferManager = unsafe { interop.GetForWindow(hwnd)? };

    let owned: Vec<String> = paths.to_vec();
    manager.DataRequested(&TypedEventHandler::<
        DataTransferManager,
        DataRequestedEventArgs,
    >::new(move |_, args: Ref<DataRequestedEventArgs>| {
        let Some(args) = args.as_ref() else { return Ok(()) };
        let request = args.Request()?;
        let deferral = request.GetDeferral()?;
        let data = request.Data()?;

        let mut items: Vec<IStorageItem> = Vec::new();
        for path in &owned {
            let h = HSTRING::from(path.as_str());
            let item: Result<IStorageItem> = if std::path::Path::new(path).is_dir() {
                StorageFolder::GetFolderFromPathAsync(&h)?.join()?.cast()
            } else {
                StorageFile::GetFileFromPathAsync(&h)?.join()?.cast()
            };
            if let Ok(item) = item {
                items.push(item);
            }
        }
        if items.is_empty() {
            deferral.Complete()?;
            return Ok(());
        }

        let title = items[0].Name().unwrap_or_default();
        let props = data.Properties()?;
        props.SetTitle(&title)?;
        // `IIterable<T>` builds from the interface's Default repr, i.e. Option<T>.
        let iterable: IIterable<IStorageItem> =
            items.into_iter().map(Some).collect::<Vec<_>>().into();
        data.SetStorageItems(&iterable, false)?;
        deferral.Complete()?;
        Ok(())
    }))?;

    unsafe { interop.ShowShareUIForWindow(hwnd) }
}
