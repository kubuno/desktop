//! Port of `Windows/WindowsBulkOperations.cs`,
//! `WindowsBulkOperationsEventArgs.cs` and the two
//! `WindowsBulkOperationsSink` partials.
//!
//! The C# sink is a manually laid out COM vtable; here it is an
//! `#[implement(IFileOperationProgressSink)]` type whose 16 callbacks are
//! relayed as [`BulkOperationsEvent`] values on a tokio broadcast channel
//! (replacing the .NET events).

use tokio::sync::broadcast;
use windows::core::{implement, Ref, HSTRING, PCWSTR};
use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_LOCAL_SERVER};
use windows::Win32::UI::Shell::{
    FileOperation, IFileOperation, IFileOperationProgressSink, IFileOperationProgressSink_Impl,
    IShellItem, FILEOPERATION_FLAGS, FOF_ALLOWUNDO, FOF_NOCONFIRMMKDIR,
};

use crate::windows_storage::{display_name_of, WindowsFolder, WindowsStorable};

/// Default flags used by the C# constructor.
pub const DEFAULT_OPERATION_FLAGS: FILEOPERATION_FLAGS =
    FILEOPERATION_FLAGS(FOF_ALLOWUNDO.0 | FOF_NOCONFIRMMKDIR.0);

/// Event payload relayed from the `IFileOperationProgressSink` callbacks.
///
/// Where the C# `WindowsBulkOperationsEventArgs` carries live
/// `WindowsStorable` objects, the broadcast payload carries file-system paths
/// (`SIGDN_FILESYSPATH`), because shell items are apartment-bound and the
/// channel crosses threads.
#[derive(Debug, Clone)]
pub enum BulkOperationsEvent {
    /// `StartOperations` → `OperationsStarted`.
    Started,
    /// `FinishOperations` → `OperationsFinished`.
    Finished { result: windows::core::HRESULT },
    /// `PreRenameItem` → `ItemRenaming`.
    Renaming { flags: u32, source: Option<String>, new_name: String },
    /// `PostRenameItem` → `ItemRenamed`.
    Renamed {
        flags: u32,
        source: Option<String>,
        new_name: String,
        newly_created: Option<String>,
        result: windows::core::HRESULT,
    },
    /// `PreMoveItem` → `ItemMoving`.
    Moving { flags: u32, source: Option<String>, destination: Option<String>, new_name: String },
    /// `PostMoveItem` → `ItemMoved`.
    Moved {
        flags: u32,
        source: Option<String>,
        destination: Option<String>,
        new_name: String,
        newly_created: Option<String>,
        result: windows::core::HRESULT,
    },
    /// `PreCopyItem` → `ItemCopying`.
    Copying { flags: u32, source: Option<String>, destination: Option<String>, new_name: String },
    /// `PostCopyItem` → `ItemCopied`.
    Copied {
        flags: u32,
        source: Option<String>,
        destination: Option<String>,
        new_name: String,
        newly_created: Option<String>,
        result: windows::core::HRESULT,
    },
    /// `PreDeleteItem` → `ItemDeleting`.
    Deleting { flags: u32, source: Option<String> },
    /// `PostDeleteItem` → `ItemDeleted`.
    Deleted {
        flags: u32,
        source: Option<String>,
        newly_created: Option<String>,
        result: windows::core::HRESULT,
    },
    /// `PreNewItem` → `ItemCreating`.
    Creating { flags: u32, destination: Option<String>, new_name: String },
    /// `PostNewItem` → `ItemCreated`.
    Created {
        flags: u32,
        destination: Option<String>,
        new_name: String,
        template_name: String,
        file_attributes: u32,
        newly_created: Option<String>,
        result: windows::core::HRESULT,
    },
    /// `UpdateProgress` → `ProgressUpdated` (percentage like the C# sink).
    Progress { percent: i32, work_total: u32, work_so_far: u32 },
}

/// Handles bulk file operations in Windows (copy, move, delete, create,
/// rename), supporting progress tracking and event notifications.
pub struct WindowsBulkOperations {
    file_operation: IFileOperation,
    progress_sink: IFileOperationProgressSink,
    progress_sink_cookie: u32,
    events: broadcast::Sender<BulkOperationsEvent>,
}

// SAFETY: mirrors the C# class, which is used across threads; see the module
// docs of `windows_storage` for the COM apartment caveats.
unsafe impl Send for WindowsBulkOperations {}
unsafe impl Sync for WindowsBulkOperations {}

impl WindowsBulkOperations {
    /// Port of the C# constructor. `owner_hwnd` owns any file-operation
    /// dialogs; `flags` defaults to [`DEFAULT_OPERATION_FLAGS`].
    pub fn new(owner_hwnd: Option<HWND>, flags: FILEOPERATION_FLAGS) -> windows::core::Result<Self> {
        let (events, _) = broadcast::channel(256);

        // SAFETY: standard COM activation + sink registration; the sink cookie
        // is revoked in `Drop`.
        unsafe {
            let file_operation: IFileOperation =
                CoCreateInstance(&FileOperation, None, CLSCTX_LOCAL_SERVER)?;

            if let Some(hwnd) = owner_hwnd {
                file_operation.SetOwnerWindow(hwnd)?;
            }

            file_operation.SetOperationFlags(flags)?;

            let progress_sink: IFileOperationProgressSink =
                BulkOperationsSink { events: events.clone() }.into();
            let progress_sink_cookie = file_operation.Advise(&progress_sink)?;

            Ok(Self { file_operation, progress_sink, progress_sink_cookie, events })
        }
    }

    /// Creates an instance with the default flags
    /// (`FOF_ALLOWUNDO | FOF_NOCONFIRMMKDIR`).
    pub fn with_defaults() -> windows::core::Result<Self> {
        Self::new(None, DEFAULT_OPERATION_FLAGS)
    }

    /// Subscribes to the operation events (replaces the C# `event` members).
    pub fn subscribe(&self) -> broadcast::Receiver<BulkOperationsEvent> {
        self.events.subscribe()
    }

    /// Port of `QueueCopyOperation`.
    pub fn queue_copy_operation(
        &self,
        target_item: &WindowsStorable,
        destination_folder: &WindowsFolder,
        copy_name: Option<&str>,
    ) -> windows::core::Result<()> {
        // SAFETY: all shell items are live; the optional name outlives the call.
        unsafe {
            self.file_operation.CopyItem(
                target_item.shell_item(),
                destination_folder.shell_item(),
                copy_name.map(HSTRING::from).as_ref().map_or(PCWSTR::null(), |s| PCWSTR(s.as_ptr())),
                &self.progress_sink,
            )
        }
    }

    /// Port of `QueueDeleteOperation`.
    pub fn queue_delete_operation(&self, target_item: &WindowsStorable) -> windows::core::Result<()> {
        // SAFETY: the shell item is live.
        unsafe { self.file_operation.DeleteItem(target_item.shell_item(), &self.progress_sink) }
    }

    /// Port of `QueueMoveOperation` (which, like the C# code, does not pass a
    /// per-item sink).
    pub fn queue_move_operation(
        &self,
        target_item: &WindowsStorable,
        destination_folder: &WindowsFolder,
        new_name: Option<&str>,
    ) -> windows::core::Result<()> {
        // SAFETY: all shell items are live; the optional name outlives the call.
        unsafe {
            self.file_operation.MoveItem(
                target_item.shell_item(),
                destination_folder.shell_item(),
                new_name.map(HSTRING::from).as_ref().map_or(PCWSTR::null(), |s| PCWSTR(s.as_ptr())),
                None::<&IFileOperationProgressSink>,
            )
        }
    }

    /// Port of `QueueCreateOperation`.
    pub fn queue_create_operation(
        &self,
        destination_folder: &WindowsFolder,
        file_attributes: u32,
        name: &str,
        template_name: Option<&str>,
    ) -> windows::core::Result<()> {
        // SAFETY: the shell item is live; the strings outlive the call.
        unsafe {
            self.file_operation.NewItem(
                destination_folder.shell_item(),
                file_attributes,
                &HSTRING::from(name),
                template_name
                    .map(HSTRING::from)
                    .as_ref()
                    .map_or(PCWSTR::null(), |s| PCWSTR(s.as_ptr())),
                &self.progress_sink,
            )
        }
    }

    /// Port of `QueueRenameOperation`.
    pub fn queue_rename_operation(
        &self,
        target_item: &WindowsStorable,
        new_name: &str,
    ) -> windows::core::Result<()> {
        // SAFETY: the shell item is live; the string outlives the call.
        unsafe {
            self.file_operation.RenameItem(
                target_item.shell_item(),
                &HSTRING::from(new_name),
                &self.progress_sink,
            )
        }
    }

    /// Port of `PerformAllOperations`.
    pub fn perform_all_operations(&self) -> windows::core::Result<()> {
        // SAFETY: plain COM call.
        unsafe { self.file_operation.PerformOperations() }
    }
}

impl Drop for WindowsBulkOperations {
    /// Port of `Dispose` (the interface releases happen automatically).
    fn drop(&mut self) {
        // SAFETY: revokes the cookie returned by `Advise` in `new`.
        unsafe {
            let _ = self.file_operation.Unadvise(self.progress_sink_cookie);
        }
    }
}

/// Resolves a callback shell item to its file-system path.
fn item_path(item: Ref<'_, IShellItem>) -> Option<String> {
    let item = item.as_ref()?;
    let path = display_name_of(item, windows::Win32::UI::Shell::SIGDN_FILESYSPATH);
    if path.is_empty() {
        None
    } else {
        Some(path)
    }
}

/// Reads a callback `PCWSTR` (may be null).
fn pcwstr_to_string(text: &PCWSTR) -> String {
    if text.is_null() {
        String::new()
    } else {
        // SAFETY: the shell passes valid NUL-terminated strings.
        unsafe { text.to_string().unwrap_or_default() }
    }
}

/// Port of `WindowsBulkOperationsSink` — replaces the manual vtable with
/// `#[implement]`.
#[implement(IFileOperationProgressSink)]
struct BulkOperationsSink {
    events: broadcast::Sender<BulkOperationsEvent>,
}

impl BulkOperationsSink {
    fn send(&self, event: BulkOperationsEvent) {
        // A send error only means there is no live receiver.
        let _ = self.events.send(event);
    }
}

impl IFileOperationProgressSink_Impl for BulkOperationsSink_Impl {
    fn StartOperations(&self) -> windows::core::Result<()> {
        self.send(BulkOperationsEvent::Started);
        Ok(())
    }

    fn FinishOperations(&self, hrresult: windows::core::HRESULT) -> windows::core::Result<()> {
        self.send(BulkOperationsEvent::Finished { result: hrresult });
        Ok(())
    }

    fn PreRenameItem(
        &self,
        dwflags: u32,
        psiitem: Ref<'_, IShellItem>,
        psznewname: &PCWSTR,
    ) -> windows::core::Result<()> {
        self.send(BulkOperationsEvent::Renaming {
            flags: dwflags,
            source: item_path(psiitem),
            new_name: pcwstr_to_string(psznewname),
        });
        Ok(())
    }

    fn PostRenameItem(
        &self,
        dwflags: u32,
        psiitem: Ref<'_, IShellItem>,
        psznewname: &PCWSTR,
        hrrename: windows::core::HRESULT,
        psinewlycreated: Ref<'_, IShellItem>,
    ) -> windows::core::Result<()> {
        self.send(BulkOperationsEvent::Renamed {
            flags: dwflags,
            source: item_path(psiitem),
            new_name: pcwstr_to_string(psznewname),
            newly_created: item_path(psinewlycreated),
            result: hrrename,
        });
        Ok(())
    }

    fn PreMoveItem(
        &self,
        dwflags: u32,
        psiitem: Ref<'_, IShellItem>,
        psidestinationfolder: Ref<'_, IShellItem>,
        psznewname: &PCWSTR,
    ) -> windows::core::Result<()> {
        self.send(BulkOperationsEvent::Moving {
            flags: dwflags,
            source: item_path(psiitem),
            destination: item_path(psidestinationfolder),
            new_name: pcwstr_to_string(psznewname),
        });
        Ok(())
    }

    fn PostMoveItem(
        &self,
        dwflags: u32,
        psiitem: Ref<'_, IShellItem>,
        psidestinationfolder: Ref<'_, IShellItem>,
        psznewname: &PCWSTR,
        hrmove: windows::core::HRESULT,
        psinewlycreated: Ref<'_, IShellItem>,
    ) -> windows::core::Result<()> {
        self.send(BulkOperationsEvent::Moved {
            flags: dwflags,
            source: item_path(psiitem),
            destination: item_path(psidestinationfolder),
            new_name: pcwstr_to_string(psznewname),
            newly_created: item_path(psinewlycreated),
            result: hrmove,
        });
        Ok(())
    }

    fn PreCopyItem(
        &self,
        dwflags: u32,
        psiitem: Ref<'_, IShellItem>,
        psidestinationfolder: Ref<'_, IShellItem>,
        psznewname: &PCWSTR,
    ) -> windows::core::Result<()> {
        self.send(BulkOperationsEvent::Copying {
            flags: dwflags,
            source: item_path(psiitem),
            destination: item_path(psidestinationfolder),
            new_name: pcwstr_to_string(psznewname),
        });
        Ok(())
    }

    fn PostCopyItem(
        &self,
        dwflags: u32,
        psiitem: Ref<'_, IShellItem>,
        psidestinationfolder: Ref<'_, IShellItem>,
        psznewname: &PCWSTR,
        hrcopy: windows::core::HRESULT,
        psinewlycreated: Ref<'_, IShellItem>,
    ) -> windows::core::Result<()> {
        self.send(BulkOperationsEvent::Copied {
            flags: dwflags,
            source: item_path(psiitem),
            destination: item_path(psidestinationfolder),
            new_name: pcwstr_to_string(psznewname),
            newly_created: item_path(psinewlycreated),
            result: hrcopy,
        });
        Ok(())
    }

    fn PreDeleteItem(&self, dwflags: u32, psiitem: Ref<'_, IShellItem>) -> windows::core::Result<()> {
        self.send(BulkOperationsEvent::Deleting { flags: dwflags, source: item_path(psiitem) });
        Ok(())
    }

    fn PostDeleteItem(
        &self,
        dwflags: u32,
        psiitem: Ref<'_, IShellItem>,
        hrdelete: windows::core::HRESULT,
        psinewlycreated: Ref<'_, IShellItem>,
    ) -> windows::core::Result<()> {
        self.send(BulkOperationsEvent::Deleted {
            flags: dwflags,
            source: item_path(psiitem),
            newly_created: item_path(psinewlycreated),
            result: hrdelete,
        });
        Ok(())
    }

    fn PreNewItem(
        &self,
        dwflags: u32,
        psidestinationfolder: Ref<'_, IShellItem>,
        psznewname: &PCWSTR,
    ) -> windows::core::Result<()> {
        self.send(BulkOperationsEvent::Creating {
            flags: dwflags,
            destination: item_path(psidestinationfolder),
            new_name: pcwstr_to_string(psznewname),
        });
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn PostNewItem(
        &self,
        dwflags: u32,
        psidestinationfolder: Ref<'_, IShellItem>,
        psznewname: &PCWSTR,
        psztemplatename: &PCWSTR,
        dwfileattributes: u32,
        hrnew: windows::core::HRESULT,
        psinewitem: Ref<'_, IShellItem>,
    ) -> windows::core::Result<()> {
        self.send(BulkOperationsEvent::Created {
            flags: dwflags,
            destination: item_path(psidestinationfolder),
            new_name: pcwstr_to_string(psznewname),
            template_name: pcwstr_to_string(psztemplatename),
            file_attributes: dwfileattributes,
            newly_created: item_path(psinewitem),
            result: hrnew,
        });
        Ok(())
    }

    fn UpdateProgress(&self, iworktotal: u32, iworksofar: u32) -> windows::core::Result<()> {
        let percent = if iworktotal == 0 {
            0
        } else {
            (iworksofar as f64 * 100.0 / iworktotal as f64) as i32
        };
        self.send(BulkOperationsEvent::Progress {
            percent,
            work_total: iworktotal,
            work_so_far: iworksofar,
        });
        Ok(())
    }

    fn ResetTimer(&self) -> windows::core::Result<()> {
        // The C# sink returns E_NOTIMPL; the shell ignores the result.
        Err(windows::core::Error::from_hresult(windows::Win32::Foundation::E_NOTIMPL))
    }

    fn PauseTimer(&self) -> windows::core::Result<()> {
        Err(windows::core::Error::from_hresult(windows::Win32::Foundation::E_NOTIMPL))
    }

    fn ResumeTimer(&self) -> windows::core::Result<()> {
        Err(windows::core::Error::from_hresult(windows::Win32::Foundation::E_NOTIMPL))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests_support::init_com;
    use crate::windows_storage::{WindowsStorable, WindowsStorableItem};

    #[test]
    fn copy_and_delete_file_with_events() {
        init_com();

        let dir = std::env::temp_dir().join(format!("files-bulk-test-{}", std::process::id()));
        let source_dir = dir.join("src");
        let dest_dir = dir.join("dst");
        std::fs::create_dir_all(&source_dir).unwrap();
        std::fs::create_dir_all(&dest_dir).unwrap();
        let source_file = source_dir.join("file.txt");
        std::fs::write(&source_file, b"payload").unwrap();

        let Some(WindowsStorableItem::File(file)) =
            WindowsStorable::try_parse(source_file.to_str().unwrap())
        else {
            panic!("source file should parse");
        };
        let Some(WindowsStorableItem::Folder(destination)) =
            WindowsStorable::try_parse(dest_dir.to_str().unwrap())
        else {
            panic!("destination should parse");
        };

        // FOF_NO_UI: never show dialogs from a test.
        let flags = FILEOPERATION_FLAGS(
            DEFAULT_OPERATION_FLAGS.0 | windows::Win32::UI::Shell::FOF_NO_UI.0,
        );
        let operations = WindowsBulkOperations::new(None, flags).expect("IFileOperation");
        let mut events = operations.subscribe();

        operations.queue_copy_operation(&file, &destination, None).expect("queue copy");
        operations.perform_all_operations().expect("perform");

        assert!(dest_dir.join("file.txt").exists(), "file should have been copied");

        let mut saw_started = false;
        let mut saw_copied = false;
        let mut saw_finished = false;
        while let Ok(event) = events.try_recv() {
            match event {
                BulkOperationsEvent::Started => saw_started = true,
                BulkOperationsEvent::Copied { result, .. } => {
                    assert!(result.is_ok());
                    saw_copied = true;
                }
                BulkOperationsEvent::Finished { result } => {
                    assert!(result.is_ok());
                    saw_finished = true;
                }
                _ => {}
            }
        }
        assert!(saw_started && saw_copied && saw_finished, "sink events should be relayed");

        drop(operations);
        std::fs::remove_dir_all(&dir).ok();
    }
}
