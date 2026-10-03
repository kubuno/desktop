//! Port of `Files.App/Actions/` + `Data/Commands/Manager/CommandManager.cs`.
//!
//! Each action is a unit struct implementing [`Action`] (the counterpart
//! of `IAction`: label, description, glyph, hotkey, executability);
//! toggles additionally implement [`ToggleAction`] (`IToggleAction.IsOn`).
//! The Rust file mirrors the C# file of the same name — `display/layout_action`
//! ↔ `Actions/Display/LayoutAction.cs`, etc. `Actions/Git/` is excluded
//! (user decision).
//!
//! [`commands`] plays the role of `CommandManager`: the immutable list of
//! all actions, consulted by the command palette (`Description`), the menus
//! (`Label`) and the keyboard (`HotKey`).

use crate::main_window::MainWindow;

pub mod content;
pub mod display;
pub mod file_system;
pub mod global;
pub mod navigation;
pub mod open;
pub mod show;
pub mod sidebar;
pub mod start;

/// Port of `HotKey` (`Data/Commands/HotKey.cs`): virtual key + modifiers.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct HotKey {
    /// Win32 virtual key code (`VK_*`; letters are their ASCII value).
    pub key: u32,
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

impl HotKey {
    pub const fn ctrl(key: u32) -> Self {
        Self { key, ctrl: true, shift: false, alt: false }
    }
    pub const fn ctrl_shift(key: u32) -> Self {
        Self { key, ctrl: true, shift: true, alt: false }
    }
    pub const fn ctrl_alt(key: u32) -> Self {
        Self { key, ctrl: true, shift: false, alt: true }
    }

    /// The hotkey text as displayed by the original (`HotKey.LocalizedLabel`)
    /// — key names come from the keyboard layout.
    pub fn text(&self) -> String {
        crate::main_window::hotkey_text_vk(self.key, self.ctrl, self.shift, self.alt)
    }
}

/// Port of `IAction`.
pub trait Action {
    /// .resw key of the label (menus, bars). `Strings.X` in the original.
    fn label(&self) -> &'static str;
    /// .resw key of the description (name in the command palette).
    fn description(&self) -> &'static str;
    /// ThemedIcon style (`RichGlyph.themedIconStyle`), if any. Read by the
    /// command palette's glyph column, which is not ported yet.
    #[allow(dead_code)]
    fn glyph(&self) -> Option<&'static str> {
        None
    }
    fn hotkey(&self) -> Option<HotKey> {
        None
    }
    /// `SecondHotKey` (EditPath has Ctrl+L **and** Alt+D, Refresh Ctrl+R and F5).
    fn second_hotkey(&self) -> Option<HotKey> {
        None
    }
    /// `IsExecutable`: does the action make sense in the current context?
    fn is_executable(&self, _w: &MainWindow) -> bool {
        true
    }
    /// `ExecuteAsync`. The `&mut MainWindow` replaces the injected services.
    fn execute(&self, w: &mut MainWindow, parameter: Option<&str>);
}

/// Port of `IToggleAction`.
pub trait ToggleAction: Action {
    fn is_on(&self, w: &MainWindow) -> bool;
}

/// The `CommandManager`: all actions, indexed by their `CommandCodes`
/// (the C# class name, without the `Action` suffix).
pub fn commands() -> &'static [(&'static str, &'static dyn Action)] {
    &[
        // === Display ===
        ("LayoutDetails", &display::layout_action::LayoutDetails),
        ("LayoutList", &display::layout_action::LayoutList),
        ("LayoutCards", &display::layout_action::LayoutCards),
        ("LayoutGrid", &display::layout_action::LayoutGrid),
        ("LayoutColumns", &display::layout_action::LayoutColumns),
        ("LayoutIncreaseSize", &display::layout_action::LayoutIncreaseSize),
        ("LayoutDecreaseSize", &display::layout_action::LayoutDecreaseSize),
        ("SortByName", &display::sort_action::SortByName),
        ("SortByDateModified", &display::sort_action::SortByDateModified),
        ("SortBySize", &display::sort_action::SortBySize),
        ("SortByType", &display::sort_action::SortByType),
        // Recycle bin only (`SortAction.cs`: GetIsExecutable => RecycleBin).
        ("SortByOriginalFolder", &display::sort_action::SortByOriginalFolder),
        ("SortByDateDeleted", &display::sort_action::SortByDateDeleted),
        ("SortAscending", &display::sort_action::SortAscending),
        ("SortFoldersFirst", &display::sort_files_first_action::SortFoldersFirst),
        ("SortFilesFirst", &display::sort_files_first_action::SortFilesFirst),
        ("SortFilesAndFoldersTogether", &display::sort_files_first_action::SortFilesAndFoldersTogether),
        ("GroupByNone", &display::group_action::GroupByNone),
        ("GroupByName", &display::group_action::GroupByName),
        ("GroupByDateModified", &display::group_action::GroupByDateModified),
        ("GroupByType", &display::group_action::GroupByType),
        ("GroupBySize", &display::group_action::GroupBySize),
        ("GroupByDateModifiedYear", &display::group_action::GroupByDateModifiedYear),
        ("GroupByDateModifiedMonth", &display::group_action::GroupByDateModifiedMonth),
        ("GroupByDateModifiedDay", &display::group_action::GroupByDateModifiedDay),
        ("GroupAscending", &display::group_action::GroupAscending),
        ("GroupDescending", &display::group_action::GroupDescending),
        ("SortDescending", &display::sort_action::SortDescending),
        // === FileSystem ===
        ("AddItem", &file_system::AddItem),
        ("CopyItem", &file_system::CopyItem),
        ("CutItem", &file_system::CutItem),
        ("PasteItem", &file_system::PasteItem),
        ("DeleteItem", &file_system::DeleteItem),
        ("Rename", &file_system::Rename),
        ("CreateFolder", &file_system::CreateFolder),
        ("RefreshItems", &file_system::RefreshItems),
        ("OpenItem", &file_system::OpenItem),
        ("OpenFileLocation", &file_system::OpenFileLocation),
        ("CopyItemPath", &file_system::CopyItemPath),
        ("CopyPath", &file_system::CopyPath),
        ("CopyPathWithQuotes", &file_system::CopyPathWithQuotes),
        ("CopyItemPathWithQuotes", &file_system::CopyItemPathWithQuotes),
        ("FormatDrive", &file_system::FormatDrive),
        ("CreateShortcut", &file_system::CreateShortcut),
        ("CreateShortcutFromDialog", &file_system::CreateShortcutFromDialog),
        ("CreateFile", &file_system::CreateFile),
        ("PasteItemAsShortcut", &file_system::PasteItemAsShortcut),
        ("PasteItemToSelection", &file_system::PasteItemToSelection),
        ("DeleteItemPermanently", &file_system::DeleteItemPermanently),
        ("EmptyRecycleBin", &file_system::EmptyRecycleBin),
        ("RestoreAllRecycleBin", &file_system::RestoreAllRecycleBin),
        ("RestoreRecycleBin", &file_system::RestoreRecycleBin),
        ("CreateFolderWithSelection", &file_system::CreateFolderWithSelection),
        ("FlattenFolder", &file_system::FlattenFolder),
        // === Content ===
        ("RotateLeft", &content::image_manipulation::RotateLeft),
        ("RotateRight", &content::image_manipulation::RotateRight),
        ("SetAsWallpaperBackground", &content::background::SetAsWallpaperBackground),
        ("SetAsLockscreenBackground", &content::background::SetAsLockscreenBackground),
        ("SetAsSlideshowBackground", &content::background::SetAsSlideshowBackground),
        ("SetAsAppBackground", &content::background::SetAsAppBackground),
        ("CompressIntoArchive", &content::archives::CompressIntoArchive),
        ("CompressIntoZip", &content::archives::CompressIntoZip),
        ("CompressIntoSevenZip", &content::archives::CompressIntoSevenZip),
        ("DecompressArchive", &content::archives::DecompressArchive),
        ("DecompressArchiveHere", &content::archives::DecompressArchiveHere),
        ("DecompressArchiveToChildFolder", &content::archives::DecompressArchiveToChildFolder),
        ("DecompressArchiveHereSmart", &content::archives::DecompressArchiveHereSmart),
        ("RunAsAdmin", &content::run::RunAsAdmin),
        ("RunAsAnotherUser", &content::run::RunAsAnotherUser),
        ("RunWithPowershell", &content::run::RunWithPowershell),
        ("LaunchPreviewPopup", &content::preview_popup::LaunchPreviewPopup),
        ("RemoveTags", &content::tags::RemoveTags),
        ("ShareItem", &content::share::ShareItem),
        ("SelectAll", &content::selection::SelectAll),
        ("InvertSelection", &content::selection::InvertSelection),
        ("ClearSelection", &content::selection::ClearSelection),
        ("ToggleSelect", &content::selection::ToggleSelect),
        // === Sidebar ===
        ("PinFolderToSidebar", &sidebar::PinFolderToSidebar),
        ("UnpinFolderFromSidebar", &sidebar::UnpinFolderFromSidebar),
        // === Navigation ===
        ("NewTab", &navigation::NewTab),
        ("NewWindow", &navigation::NewWindow),
        ("CloseSelectedTab", &navigation::CloseSelectedTab),
        ("NextTab", &navigation::NextTab),
        ("PreviousTab", &navigation::PreviousTab),
        ("NavigateBack", &navigation::NavigateBack),
        ("NavigateForward", &navigation::NavigateForward),
        ("NavigateUp", &navigation::NavigateUp),
        ("NavigateHome", &navigation::NavigateHome),
        ("OpenInNewTab", &navigation::OpenInNewTab),
        ("OpenInNewWindow", &navigation::OpenInNewWindow),
        ("SplitPaneVertically", &navigation::SplitPaneVertically),
        ("SplitPaneHorizontally", &navigation::SplitPaneHorizontally),
        ("CloseActivePane", &navigation::CloseActivePane),
        ("ToggleDualPane", &navigation::ToggleDualPane),
        ("CloseAllTabs", &navigation::CloseAllTabs),
        ("CloseOtherTabsSelected", &navigation::CloseOtherTabsSelected),
        ("CloseTabsToTheLeftSelected", &navigation::CloseTabsToTheLeftSelected),
        ("CloseTabsToTheRightSelected", &navigation::CloseTabsToTheRightSelected),
        ("DuplicateSelectedTab", &navigation::DuplicateSelectedTab),
        ("ReopenClosedTab", &navigation::ReopenClosedTab),
        ("FocusOtherPane", &navigation::FocusOtherPane),
        ("ArrangePanesHorizontally", &navigation::ArrangePanesHorizontally),
        ("ArrangePanesVertically", &navigation::ArrangePanesVertically),
        ("OpenInNewPane", &navigation::OpenInNewPane),
        ("OpenInOtherPane", &navigation::OpenInOtherPane),
        ("OpenCurrentFolderInOtherPane", &navigation::OpenCurrentFolderInOtherPane),
        // === Global ===
        ("EditPath", &global::EditPath),
        ("Undo", &global::Undo),
        ("Redo", &global::Redo),
        ("Search", &global::Search),
        ("ToggleFullScreen", &global::ToggleFullScreen),
        ("EnterCompactOverlay", &global::EnterCompactOverlay),
        ("ExitCompactOverlay", &global::ExitCompactOverlay),
        ("ToggleCompactOverlay", &global::ToggleCompactOverlay),
        ("SetLightTheme", &global::SetLightTheme),
        ("SetDarkTheme", &global::SetDarkTheme),
        ("SetDefaultTheme", &global::SetDefaultTheme),
        ("ToggleAppTheme", &global::ToggleAppTheme),
        ("OpenHelp", &global::OpenHelp),
        // === Open ===
        ("OpenSettings", &open::OpenSettings),
        ("OpenStorageSense", &open::OpenStorageSense),
        ("OpenTerminal", &open::OpenTerminal),
        ("OpenTerminalAsAdmin", &open::OpenTerminalAsAdmin),
        ("OpenCommandPalette", &open::OpenCommandPalette),
        ("OpenProperties", &open::OpenProperties),
        ("OpenClassicProperties", &open::OpenClassicProperties),
        ("EditInNotepad", &open::EditInNotepad),
        ("OpenItemWithApplicationPicker", &open::OpenItemWithApplicationPicker),
        ("OpenLogFile", &open::OpenLogFile),
        ("OpenLogFileLocation", &open::OpenLogFileLocation),
        ("OpenSettingsFile", &open::OpenSettingsFile),
        ("OpenInIDE", &open::OpenInIDE),
        ("OpenRepoInIDE", &open::OpenRepoInIDE),
        // === Show ===
        ("ToggleShowHiddenItems", &show::toggle_show_hidden_items_action::ToggleShowHiddenItems),
        ("ToggleShowFileExtensions", &show::toggle_show_file_extensions_action::ToggleShowFileExtensions),
        ("ToggleInfoPane", &show::toggle_info_pane_action::ToggleInfoPane),
        ("ToggleShelfPane", &show::toggle_shelf_pane_action::ToggleShelfPane),
        ("ToggleSidebar", &show::toggle_sidebar_action::ToggleSidebar),
        ("ToggleDetailsPane", &show::toggle_details_pane_action::ToggleDetailsPane),
        ("TogglePreviewPane", &show::toggle_preview_pane_action::TogglePreviewPane),
        ("ToggleToolbar", &show::toggle_toolbar_action::ToggleToolbar),
        ("ToggleFilterHeader", &show::toggle_filter_header_action::ToggleFilterHeader),
        ("ToggleDotFilesSetting", &show::toggle_dot_files_setting_action::ToggleDotFilesSetting),
    ]
}

/// The action bearing this `CommandCodes` (`CommandManager[CommandCodes]`).
pub fn by_name(name: &str) -> Option<&'static dyn Action> {
    commands().iter().find(|(n, _)| *n == name).map(|(_, a)| *a)
}

/// The action bound to a keyboard shortcut, if any (`CommandManager[HotKey]`).
pub fn by_hotkey(key: u32, ctrl: bool, shift: bool, alt: bool) -> Option<&'static dyn Action> {
    let pressed = HotKey { key, ctrl, shift, alt };
    commands()
        .iter()
        .find(|(_, a)| a.hotkey() == Some(pressed) || a.second_hotkey() == Some(pressed))
        .map(|(_, a)| *a)
}
