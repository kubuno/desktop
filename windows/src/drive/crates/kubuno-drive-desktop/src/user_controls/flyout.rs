//! Port of `Files.App/UserControls/Menus/MenuFlyoutItemWithThemedIcon.xaml`
//! (and the WinUI MenuFlyout it lives in): custom Fluent flyout drawn as an
//! in-window D2D overlay, with optional submenu.
//!
//! The menus themselves are ports of `ContentPageContextFlyoutFactory`
//! (item / empty-space menus), `TabBar.xaml`'s TabFlyout and the sidebar's
//! context flyout.

use kubuno_drive_desktop_app_controls::themes::shape;
use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;
use windows::Win32::Graphics::DirectWrite::DWRITE_TEXT_ALIGNMENT_TRAILING;

use crate::ui::{Painter, Rect, GLYPH_CHEVRON_RIGHT};

/// The `ICommandManager` commands our context menus dispatch. One id per
/// command, so a menu's item ORDER never has to match its dispatch code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuCommand {
    None,
    /// Checks/unchecks tag #i (settings) on the pane's item.
    ToggleFileTag(usize),
    // Empty-space menu (no selection).
    LayoutDetails,
    LayoutList,
    LayoutCards,
    LayoutGrid,
    LayoutColumns,
    SortByName,
    SortByDateModified,
    SortByType,
    SortBySize,
    /// Recycle Bin only (`SortAction.cs`: `SortByOriginalFolderAction`,
    /// only runnable in `ContentPageTypes.RecycleBin`).
    SortByOriginalPath,
    /// Recycle Bin only (`SortAction.cs`: `SortByDateDeletedAction`).
    SortByDateDeleted,
    SortAscending,
    SortDescending,
    GroupByNone,
    GroupByName,
    GroupByDateModified,
    GroupByType,
    GroupBySize,
    GroupAscending,
    GroupDescending,
    RefreshItems,
    CreateFolder,
    CreateFile,
    /// Entry `i` of the ShellNew templates (`AddItemService.GetEntries()`), in
    /// the order of `list_shell_new_entries()`.
    CreateShellNew(usize),
    OpenTerminal,
    // Item menu.
    OpenItem,
    OpenItemWithApplicationPicker,
    OpenInNewTab,
    OpenInNewWindow,
    CutItem,
    CopyItem,
    PasteItem,
    CopyItemPath,
    Rename,
    ShareItem,
    DeleteItem,
    OpenProperties,
    // Recycle Bin (`ShowInRecycleBin`).
    RestoreRecycleBin,
    EmptyRecycleBin,
    RestoreAllRecycleBin,
    PinFolderToSidebar,
    UnpinFolderFromSidebar,
    OpenFileLocation,
    OpenInNewPaneVertical,
    OpenInNewPaneHorizontal,
    CloseActivePane,
    RunAsAdmin,
    RunAsAnotherUser,
    PasteItemAsShortcut,
    CreateFolderWithSelection,
    CreateShortcut,
    /// `CreateShortcutFromDialogAction`: the "Nouveau" submenu opens the
    /// shortcut-creation dialog (distinct from `CreateShortcut`).
    CreateShortcutFromDialog,
    CompressIntoArchive,
    CompressIntoZip,
    CompressIntoSevenZip,
    DecompressArchive,
    DecompressArchiveHere,
    DecompressArchiveToChildFolder,
    FlattenFolder,
    EditInNotepad,
    // Content/ImageManipulation + Content/Background (images).
    RotateLeft,
    RotateRight,
    SetAsWallpaperBackground,
    SetAsLockscreenBackground,
    SetAsSlideshowBackground,
    SetAsAppBackground,
    // Drive operations.
    FormatDrive,
    EjectDrive,
    OpenStorageSense,
    /// Entry `i` of the user's SendTo folder.
    SendTo(usize),
    /// The `ItemOverflow` entry itself: it only toggles its submenu.
    ShowMoreOptions,
    /// Entry `i` of the shell's own `IContextMenu`, once loaded into the
    /// overflow submenu.
    ShellCommand(usize),
}

impl MenuCommand {
    /// Commands that destroy data. The web `MenuDropdown` marks such rows
    /// `danger: true`; here the menu FACTORIES live in other modules, so the
    /// flag is derived from the command id — a factory can still force it with
    /// `FlyoutItem::danger()`.
    pub fn is_destructive(self) -> bool {
        matches!(
            self,
            MenuCommand::DeleteItem | MenuCommand::EmptyRecycleBin | MenuCommand::FormatDrive
        )
    }
}

/// One entry of a custom Fluent flyout (port of `MenuFlyoutItemWithThemedIcon`).
#[derive(Clone)]
pub struct FlyoutItem {
    /// A Segoe Fluent Icons codepoint (`RichGlyph.Glyph`).
    pub glyph: &'static str,
    /// A ThemedIcon style name (`RichGlyph.ThemedIconStyle`), preferred over
    /// `glyph` when both are set — that is the original's own precedence.
    pub icon: Option<&'static str>,
    /// `ContextMenuFlyoutItemViewModel.BitmapIcon`: a ready-made image, which
    /// is how the shell hands over the icons of its own menu entries.
    pub bitmap: Option<std::sync::Arc<kubuno_drive_desktop_app_storage::ShellBitmap>>,
    pub label: String,
    /// KeyboardAcceleratorTextOverride, right-aligned like the WinUI item.
    pub accel: Option<String>,
    pub enabled: bool,
    pub has_submenu: bool,
    /// `ContextMenuFlyoutItemViewModel.Items`: the submenu this item opens.
    pub children: Vec<FlyoutItem>,
    /// Width the submenu sizes itself to (measured when the menu is opened).
    pub children_width: f32,
    /// `ContextMenuFlyoutItemType.Separator`.
    pub separator: bool,
    /// `IsToggle`: a ToggleMenuFlyoutItem, whose icon column holds the
    /// checkmark instead of an icon.
    pub is_toggle: bool,
    /// `IsChecked`.
    pub checked: bool,
    /// `ComboBoxItem` rendering: the checked item carries the accent pill of its
    /// `SelectionIndicator` (and a subtle background), not a checkmark.
    pub pill: bool,
    pub command: MenuCommand,
}

// NOTE — the web `MenuItem` also carries an explicit `danger: boolean`. It is
// NOT mirrored as a field here: `FlyoutItem` is still built with EXHAUSTIVE
// struct literals in `main_window/{pointer_input,toolbar_clicks}.rs`, which a
// new field would break. `Default` is provided below so those five literals can
// switch to `..FlyoutItem::default()`, after which the field can be added; until
// then `is_danger()` derives the flag from the command, which covers every
// destructive entry the menus actually offer.

impl FlyoutItem {
    /// A plain command with no icon and no accelerator.
    pub fn new(label: String, enabled: bool) -> Self {
        Self {
            glyph: "",
            icon: None,
            bitmap: None,
            label,
            accel: None,
            enabled,
            has_submenu: false,
            children: Vec::new(),
            children_width: SUBMENU_WIDTH,
            separator: false,
            is_toggle: false,
            checked: false,
            pill: false,
            command: MenuCommand::None,
        }
    }

    /// `ContextMenuFlyoutItemType.Separator`.
    pub fn separator() -> Self {
        Self { separator: true, ..Self::new(String::new(), false) }
    }

    pub fn with_glyph(mut self, glyph: &'static str) -> Self {
        self.glyph = glyph;
        self
    }

    /// A ThemedIcon style (`assets/themed-icons.txt`).
    pub fn with_icon(mut self, icon: &'static str) -> Self {
        self.icon = Some(icon);
        self
    }

    /// The shell's own icon for this row.
    pub fn with_bitmap(
        mut self,
        bitmap: Option<std::sync::Arc<kubuno_drive_desktop_app_storage::ShellBitmap>>,
    ) -> Self {
        self.bitmap = bitmap;
        self
    }

    pub fn with_accel(mut self, accel: String) -> Self {
        self.accel = Some(accel);
        self
    }

    pub fn with_command(mut self, command: MenuCommand) -> Self {
        self.command = command;
        self
    }

    pub fn with_children(mut self, children: Vec<FlyoutItem>) -> Self {
        self.has_submenu = !children.is_empty();
        self.children = children;
        self
    }

    /// `IsToggle = true`: the item renders as a ToggleMenuFlyoutItem.
    pub fn checked(mut self, checked: bool) -> Self {
        self.is_toggle = true;
        self.checked = checked;
        self
    }

    /// Whether this row must be painted in the danger colour — the web's
    /// `danger: true`. Derived from the command it runs (see the note on the
    /// struct as to why it is not a field).
    fn is_danger(&self) -> bool {
        self.command.is_destructive()
    }
}

/// Lets a factory build a row from a few fields and `..FlyoutItem::default()`
/// instead of spelling out all fourteen — the pattern new fields need in order
/// to be additive.
impl Default for FlyoutItem {
    fn default() -> Self {
        Self::new(String::new(), true)
    }
}

/// Where a point lands inside an open flyout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlyoutHit {
    Item(usize),
    SubItem(usize),
    /// A 3rd-level item (e.g. Year/Month/Day under "Grouper par › Date de
    /// modification"), indexed in `subsub_items()`.
    SubSubItem(usize),
    /// A button of the CommandBarFlyout's PrimaryCommands band.
    Primary(usize),
    Panel,
    Outside,
}

// ── Kubuno `MenuDropdown` metrics ────────────────────────────────────────────
// The menus follow the WEB design system now, not the WinUI template. Values
// mirror `core/frontend/src/ui/MenuDropdown.tsx` one for one: the panel is a
// 5 px-padded CSS grid (`icon | label | shortcut`, `columnGap: 8`) and each row
// a `padding: 5px 12px 5px 10px` button with a `20px` line box.

pub const FLYOUT_WIDTH: f32 = 310.0;
/// `minWidth` of a cascaded submenu panel (`SUB_W` in `MenuDropdown`).
pub const SUBMENU_WIDTH: f32 = 220.0;
/// A `MenuDropdown` action row: 5 px padding + a 20 px line box + 5 px.
pub const FLYOUT_ITEM_HEIGHT: f32 = shape::height::MENU_ITEM;
/// `margin: '5px 6px'` around a 1 px rule.
pub const FLYOUT_SEPARATOR_HEIGHT: f32 = SEPARATOR_MARGIN * 2.0 + 1.0;
/// `--kb-float-radius`: the radius every floating surface shares.
pub const FLYOUT_CORNER_RADIUS: f32 = shape::radius::FLOAT;
/// The panel's own `padding: 5` — also the gutter the highlight pill is inset
/// by, which is why it doubles as the pill's side margin.
const FLYOUT_PADDING: f32 = 5.0;
/// A row's highlight pill radius (`borderRadius: 6`).
const PILL_RADIUS: f32 = shape::radius::MENU_ITEM;
/// Vertical margin of a separator; its side margin is `FLYOUT_PADDING + 6`.
const SEPARATOR_MARGIN: f32 = 5.0;
const SEPARATOR_INSET: f32 = FLYOUT_PADDING + 6.0;
/// Disabled rows: `disabled:opacity-40`.
const DISABLED_OPACITY: f32 = 0.4;
/// The shortcut column dims slightly on the accent pill (`opacity: 0.85`).
const HOVER_SHORTCUT_OPACITY: f32 = 0.85;
/// A `CommandBarFlyout`'s PrimaryCommands band. Measured on the original (at
/// 175% DPI, a 674 px popup): the bar is its own 48 DIP panel sitting flush on
/// top of the menu, with a 1 px divider between them; inside it the
/// AppBarButtons are 40×40, laid out LEFT to right (they do not stretch to the
/// panel width — a menu wider than the buttons leaves empty space on the right).
const PRIMARY_BUTTON: f32 = 40.0;
/// `CommandBarFlyoutCommandBarPadding` — 4 DIP above and below the buttons.
const PRIMARY_PADDING: f32 = 4.0;
const PRIMARY_BAND: f32 = PRIMARY_PADDING + PRIMARY_BUTTON + PRIMARY_PADDING;
/// Panel padding (5) + the row's own `padding-left` (10): the icon column.
pub const ICON_LEFT: f32 = FLYOUT_PADDING + 10.0;
/// `width: 20` on the icon `<span>`. The 16 DIP glyph is left-aligned in it,
/// as the flex box does.
const ICON_BOX: f32 = 20.0;
/// Icon column + `columnGap: 8`: where the label column starts (43, which the
/// WinUI template happened to share).
pub const LABEL_LEFT: f32 = ICON_LEFT + ICON_BOX + 8.0;
/// Gap between the label and the shortcut columns: `columnGap: 8` plus the
/// shortcut cell's own `paddingLeft: 16`.
pub const ACCEL_GAP: f32 = 8.0 + 16.0;
/// Panel padding (5) + the row's `padding-right` (12): where the shortcut /
/// chevron column ends.
pub const CONTENT_RIGHT: f32 = FLYOUT_PADDING + 12.0;

/// What an open flyout controls (dispatches item clicks).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlyoutKind {
    TabActions,
    ViewMode,
    NewMenu,
    SortMenu,
    SelectionMenu,
    /// Status bar, git network actions button: Pull / Push / Sync.
    GitActions,
    /// Status bar, branch selector: list of branches (checkmark on the
    /// head) + "Créer une branche".
    GitBranches,
    /// The dropdown menu of a ComboBox (Settings pages): the selection is
    /// rendered as a `ComboBoxItem` (accent pill) and the choice is picked up by
    /// `MainWindow::dropdown`'s modal loop.
    Combo,
    /// The `ColorPicker` of the "Couleurs du fond" button (AppearancePage).
    ColorPicker,
    /// TabFlyout of TabBar.xaml, opened by right-clicking tab `i`.
    TabContext(usize),
    /// `BreadcrumbBar`: the ellipsis (…) flyout — the collapsed head
    /// segments, dispatched by index in `breadcrumb_overflow`.
    BreadcrumbOverflow,
    /// `GetItemContextCommandsWithoutShellItems`: right-click on a file row,
    /// a Quick-Access card or a drive card.
    ItemContext,
    /// The same factory with no selection: right-click on the file area's
    /// empty space.
    EmptySpace,
    /// The sidebar item's own context flyout.
    SidebarContext,
}

/// An open Fluent-style flyout, drawn as an in-window overlay.
pub struct Flyout {
    pub kind: FlyoutKind,
    pub x: f32,
    pub y: f32,
    /// A MenuFlyout sizes to its content; each menu carries its own width.
    pub width: f32,
    pub items: Vec<FlyoutItem>,
    /// `CommandBarFlyout.PrimaryCommands`: the icon-only band across the top
    /// (the items the factory marks `IsPrimary`).
    pub primary: Vec<FlyoutItem>,
    /// Index of the item whose submenu is open.
    pub submenu: Option<usize>,
    /// Index, WITHIN the open submenu, of the item whose 3rd level is open
    /// (e.g. "Date de modification" under "Grouper par", which unfolds
    /// Year/Month/Day).
    pub subsubmenu: Option<usize>,
    /// (is_in_submenu, index) hover state.
    pub hot: Option<(bool, usize)>,
    /// Hovered item of the 3rd level, if any.
    pub hot_sub2: Option<usize>,
    /// Hovered button of the primary band.
    pub hot_primary: Option<usize>,
    /// The path the ItemContext / SidebarContext menu acts on.
    pub path: Option<String>,
    /// When the panel started opening, and when its submenu did. WinUI plays a
    /// `PopupThemeTransition` on every flyout popup; Files does not override it
    /// (there is no transition anywhere in its XAML), so both the menu and its
    /// submenus unfold with the same motion.
    pub opened: std::time::Instant,
    pub submenu_opened: Option<std::time::Instant>,
    pub subsubmenu_opened: Option<std::time::Instant>,
    /// The submenu's actual top-left corner, in client DIP, once clamped to
    /// the monitor (`sync_flyout` fills it in). The hover test uses it.
    pub sub_pos: Option<(f32, f32)>,
    /// Same for the 3rd-level panel.
    pub subsub_pos: Option<(f32, f32)>,
    /// The "Disposition" flyout is not a menu but a panel: when it
    /// is present, it's THAT which gets drawn, and `items` stays empty.
    pub layout: Option<super::layout_flyout::LayoutPanel>,
    /// The `ColorPicker` of the "Couleurs du fond" button — same principle.
    pub picker: Option<super::color_picker::ColorPickerPanel>,
}

/// How long a menu takes to unfold. The Fluent "point-to-point" motion for a
/// popup this size.
pub const FLYOUT_ANIM_MS: f32 = 170.0;

/// Fluent's decelerate easing (the `1 - (1-t)³` shape of cubic-bezier
/// 0.1,0.9,0.2,1): fast out of the gate, settling gently.
pub fn flyout_ease(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

/// Progress of an unfold started at `since`, 1.0 once it is over.
pub fn flyout_progress(since: std::time::Instant) -> f32 {
    let ms = since.elapsed().as_secs_f32() * 1000.0;
    flyout_ease(ms / FLYOUT_ANIM_MS)
}

/// Total height of `items` laid out one under the other.
fn items_height(items: &[FlyoutItem]) -> f32 {
    items.iter().map(row_height).sum()
}

fn row_height(item: &FlyoutItem) -> f32 {
    if item.separator {
        FLYOUT_SEPARATOR_HEIGHT
    } else {
        FLYOUT_ITEM_HEIGHT
    }
}

/// The row rect of item `i` inside a panel whose content starts at `top`.
fn row_rect(items: &[FlyoutItem], i: usize, left: f32, right: f32, top: f32) -> Rect {
    let y = top + items_height(&items[..i]);
    Rect::new(left, y, right, y + row_height(&items[i]))
}

impl Flyout {
    /// Opens (or closes) the submenu of item `index`, restarting its unfold.
    /// Re-opening the same one does not restart it.
    pub fn set_submenu(&mut self, index: Option<usize>) {
        if self.submenu == index {
            return;
        }
        self.submenu = index;
        self.submenu_opened = index.map(|_| std::time::Instant::now());
        self.sub_pos = None;
        // Changing submenu necessarily closes the 3rd level.
        self.subsubmenu = None;
        self.subsubmenu_opened = None;
        self.subsub_pos = None;
        self.hot_sub2 = None;
    }

    /// Opens (or closes) the 3rd-level submenu of the sub-item at `index`
    /// (indexed within the currently-open submenu).
    pub fn set_subsubmenu(&mut self, index: Option<usize>) {
        if self.subsubmenu == index {
            return;
        }
        self.subsubmenu = index;
        self.subsubmenu_opened = index.map(|_| std::time::Instant::now());
        self.subsub_pos = None;
    }

    /// True while the panel or one of its submenus is still unfolding.
    pub fn animating(&self) -> bool {
        flyout_progress(self.opened) < 1.0
            || self.submenu_opened.is_some_and(|t| flyout_progress(t) < 1.0)
            || self.subsubmenu_opened.is_some_and(|t| flyout_progress(t) < 1.0)
    }

    /// Height the PrimaryCommands band adds above the menu items (0 when the
    /// menu has none — a plain MenuFlyout rather than a CommandBarFlyout).
    pub fn primary_height(&self) -> f32 {
        if self.primary.is_empty() {
            0.0
        } else {
            PRIMARY_BAND
        }
    }

    pub fn panel_rect(&self) -> Rect {
        if let Some(panel) = &self.layout {
            return Rect::new(
                self.x,
                self.y,
                self.x + super::layout_flyout::LAYOUT_PANEL_WIDTH,
                self.y + panel.height(),
            );
        }
        if let Some(picker) = &self.picker {
            return Rect::new(
                self.x,
                self.y,
                self.x + super::color_picker::PICKER_WIDTH,
                self.y + picker.height(),
            );
        }
        Rect::new(
            self.x,
            self.y,
            self.x + self.width,
            self.y + self.primary_height() + items_height(&self.items) + 2.0 * FLYOUT_PADDING,
        )
    }

    /// One AppBarButton of the primary band: 40×40, packed from the left.
    pub fn primary_rect(&self, i: usize) -> Option<Rect> {
        if i >= self.primary.len() {
            return None;
        }
        let left = self.x + PRIMARY_PADDING + PRIMARY_BUTTON * i as f32;
        let top = self.y + PRIMARY_PADDING;
        Some(Rect::new(left, top, left + PRIMARY_BUTTON, top + PRIMARY_BUTTON))
    }

    /// A row's own box: it spans the panel's FULL width, the 5 px side gutter
    /// being applied to the highlight pill inside it (`pill_rect`). Hit-testing
    /// therefore covers the gutter too, as the web grid row does.
    pub fn item_rect(&self, i: usize) -> Rect {
        row_rect(
            &self.items,
            i,
            self.x,
            self.x + self.width,
            self.y + FLYOUT_PADDING + self.primary_height(),
        )
    }

    /// The entries of the open submenu, if any.
    pub fn sub_items(&self) -> Option<&[FlyoutItem]> {
        let parent = self.submenu?;
        let children = &self.items.get(parent)?.children;
        (!children.is_empty()).then_some(children.as_slice())
    }

    pub fn sub_panel_rect(&self) -> Option<Rect> {
        let parent = self.submenu?;
        let entries = self.sub_items()?;
        let width = self.items[parent].children_width;
        let anchor = self.item_rect(parent);
        // `sub_pos` is the ACTUAL position, the one `sync_flyout` obtained
        // after clamping to the screen; without it hover would target the
        // panel's theoretical spot, not the one where it's visible.
        // `MenuDropdown`: `left = r.right - 2`, measured on the ROW BUTTON —
        // which here is the row minus the panel's padding. The top offset keeps
        // the submenu's first row level with its parent row.
        let (left, top) = self
            .sub_pos
            .unwrap_or((anchor.right - FLYOUT_PADDING - 2.0, anchor.top - FLYOUT_PADDING));
        Some(Rect::new(
            left,
            top,
            left + width,
            top + items_height(entries) + 2.0 * FLYOUT_PADDING,
        ))
    }

    pub fn sub_item_rect(&self, i: usize) -> Option<Rect> {
        let entries = self.sub_items()?;
        let panel = self.sub_panel_rect()?;
        Some(row_rect(entries, i, panel.left, panel.right, panel.top + FLYOUT_PADDING))
    }

    /// The entries of the open 3rd-level submenu, if any.
    pub fn subsub_items(&self) -> Option<&[FlyoutItem]> {
        let entries = self.sub_items()?;
        let children = &entries.get(self.subsubmenu?)?.children;
        (!children.is_empty()).then_some(children.as_slice())
    }

    pub fn subsub_panel_rect(&self) -> Option<Rect> {
        let entries = self.subsub_items()?;
        let parent = self.subsubmenu?;
        let width = self.sub_items()?.get(parent)?.children_width;
        let anchor = self.sub_item_rect(parent)?;
        let (left, top) = self
            .subsub_pos
            .unwrap_or((anchor.right - FLYOUT_PADDING - 2.0, anchor.top - FLYOUT_PADDING));
        Some(Rect::new(
            left,
            top,
            left + width,
            top + items_height(entries) + 2.0 * FLYOUT_PADDING,
        ))
    }

    pub fn subsub_item_rect(&self, i: usize) -> Option<Rect> {
        let entries = self.subsub_items()?;
        let panel = self.subsub_panel_rect()?;
        Some(row_rect(entries, i, panel.left, panel.right, panel.top + FLYOUT_PADDING))
    }

    pub fn hit(&self, x: f32, y: f32) -> FlyoutHit {
        // The 3rd level is the topmost: test it first.
        if let Some(entries) = self.subsub_items() {
            for (i, entry) in entries.iter().enumerate() {
                if !entry.separator && self.subsub_item_rect(i).is_some_and(|r| r.contains(x, y))
                {
                    return FlyoutHit::SubSubItem(i);
                }
            }
            if self.subsub_panel_rect().is_some_and(|r| r.contains(x, y)) {
                return FlyoutHit::Panel;
            }
        }
        if let Some(entries) = self.sub_items() {
            for (i, entry) in entries.iter().enumerate() {
                if !entry.separator && self.sub_item_rect(i).is_some_and(|r| r.contains(x, y)) {
                    return FlyoutHit::SubItem(i);
                }
            }
            if self.sub_panel_rect().is_some_and(|r| r.contains(x, y)) {
                return FlyoutHit::Panel;
            }
        }
        for i in 0..self.primary.len() {
            if self.primary_rect(i).is_some_and(|r| r.contains(x, y)) {
                return FlyoutHit::Primary(i);
            }
        }
        for i in 0..self.items.len() {
            if !self.items[i].separator && self.item_rect(i).contains(x, y) {
                return FlyoutHit::Item(i);
            }
        }
        if self.panel_rect().contains(x, y) {
            FlyoutHit::Panel
        } else {
            FlyoutHit::Outside
        }
    }

    /// The command the item at `hit` runs, if it is a leaf command.
    pub fn command_at(&self, hit: FlyoutHit) -> Option<MenuCommand> {
        let item = match hit {
            FlyoutHit::Item(i) => self.items.get(i)?,
            FlyoutHit::SubItem(i) => self.sub_items()?.get(i)?,
            FlyoutHit::SubSubItem(i) => self.subsub_items()?.get(i)?,
            FlyoutHit::Primary(i) => self.primary.get(i)?,
            _ => return None,
        };
        (item.enabled && !item.has_submenu && item.command != MenuCommand::None)
            .then_some(item.command)
    }
}

/// The row's highlight pill. The panel's own 5 px padding IS the gutter the
/// pill is inset by (`MenuDropdown`: "Side padding forms the gutter the
/// highlight pill is inset by"), and the pill fills the row's full height.
fn pill_rect(rect: &Rect) -> Rect {
    Rect::new(rect.left + FLYOUT_PADDING, rect.top, rect.right - FLYOUT_PADDING, rect.bottom)
}

/// The same colour at a fraction of its opacity. The web fades disabled rows
/// and the hovered shortcut with `opacity`, for which there is no token: the
/// menu is painted on a transparent target, so alpha composites over the
/// acrylic exactly as CSS opacity does over the frosted panel.
fn faded(color: &D2D1_COLOR_F, opacity: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F { a: color.a * opacity, ..*color }
}

impl Painter<'_> {
    /// An in-window rounded panel (omnibar suggestions, StatusCenter).
    ///
    /// Its surface is OPAQUE (`--color-surface-0`), NOT the frosted
    /// `flyout_background`: that colour is only half-opaque because the real
    /// context menus live in their own popup window, where the compositor
    /// blurs what shows through. There is no blur behind an in-window panel,
    /// so the same colour would simply let the toolbar and the file list read
    /// straight through it.
    pub(crate) fn draw_flyout_panel(&self, panel: &Rect) {
        let t = self.theme;
        self.draw_layered_shadow(
            panel,
            FLYOUT_CORNER_RADIUS,
            &kubuno_drive_desktop_app_controls::themes::shape::SHADOW_MENU,
            kubuno_drive_desktop_app_controls::themes::shape::SHADOW_GREY,
        );
        self.fill_rounded(panel, FLYOUT_CORNER_RADIUS, &t.layer_background);
        self.stroke_rounded(panel, FLYOUT_CORNER_RADIUS, &t.card_stroke);
    }

    fn draw_flyout_item(&self, rect: &Rect, item: &FlyoutItem, hot: bool) {
        let t = self.theme;
        let f = &self.renderer.formats;

        // Separator: a 1 px rule with `margin: '5px 6px'` — it stops SHORT of
        // the panel edge instead of bleeding to it, so it lines up with the
        // highlight pills above and below.
        if item.separator {
            let y = self.px((rect.top + rect.bottom) / 2.0);
            let line = Rect::new(
                rect.left + SEPARATOR_INSET,
                y,
                rect.right - SEPARATOR_INSET,
                y + 1.0 / self.scale(),
            );
            self.fill_rounded(&line, 0.0, &t.divider);
            return;
        }

        // THE Kubuno hover: a FULL accent pill (`background: c.hover`), not the
        // subtle grey fill of the Fluent template — which is why the label, the
        // icon and the shortcut all flip to `accent_foreground` below.
        let hot = hot && item.enabled;
        let pill = pill_rect(rect);
        if hot {
            self.fill_rounded(&pill, PILL_RADIUS, &t.accent);
        }

        let danger = item.is_danger();
        let label_color = if hot {
            t.accent_foreground
        } else if danger {
            t.danger
        } else {
            t.text_primary
        };
        // At rest the icon carries the ACCENT, not the label colour (the web's
        // `color: c.accent` on the icon cell); a destructive row tints it danger.
        let icon_color = if hot {
            t.accent_foreground
        } else if danger {
            t.danger
        } else {
            t.accent
        };
        let (fg, icon_fg) = if item.enabled {
            (label_color, icon_color)
        } else {
            (faded(&label_color, DISABLED_OPACITY), faded(&icon_color, DISABLED_OPACITY))
        };

        let cy = (rect.top + rect.bottom) / 2.0;
        let icon = Rect::new(rect.left + ICON_LEFT, cy - 8.0, rect.left + ICON_LEFT + 16.0, cy + 8.0);
        // The `SelectionIndicator` bar of a checked ComboBox / radio row, on the
        // pill's leading edge; it has to invert when the accent pill is under it.
        let indicator = Rect::new(pill.left + 2.0, cy - 8.0, pill.left + 5.0, cy + 8.0);
        let indicator_color = if hot { &t.accent_foreground } else { &t.accent };
        if item.pill {
            // `ComboBoxItem`: the selected item has a subtle background — skipped
            // under the accent pill, which already fills the row — plus the bar.
            if item.checked {
                if !hot {
                    self.fill_rounded(&pill, PILL_RADIUS, &t.control_fill_hover);
                }
                self.fill_rounded(&indicator, 1.5, indicator_color);
            }
        } else if item.is_toggle {
            if let Some(name) = item.icon {
                // Toggle WITH icon (layouts, sizes — the original's
                // `RadioMenuFlyoutItem`): the icon stays in its column, the selection is
                // marked by the indicator + background (instead of HIDING the icon).
                if item.checked {
                    if !hot {
                        self.fill_rounded(&pill, PILL_RADIUS, &t.control_fill_hover);
                    }
                    self.fill_rounded(&indicator, 1.5, indicator_color);
                }
                self.vector_icon_layered(name, &icon, 16.0, &icon_fg, &icon_fg);
            } else if item.checked {
                // Without an icon: the icon column carries the checkmark (E73E) —
                // the web's `{item.checked ? '✓' : icon}` cell.
                self.text("\u{E73E}", &icon, &f.icon_small, &icon_fg, true);
            }
        } else if let Some(bitmap) = &item.bitmap {
            self.shell_bitmap(bitmap, &icon);
        } else if let Some(name) = item.icon {
            // A single-colour line icon: both layers take the same colour, since
            // the web paints the whole glyph in the accent (or danger, or the
            // pill's foreground) rather than layering two tones.
            self.vector_icon_layered(name, &icon, 16.0, &icon_fg, &icon_fg);
        } else if !item.glyph.is_empty() {
            self.text(item.glyph, &icon, &f.icon_small, &icon_fg, true);
        }

        // The label only yields room to the accelerator column when this item
        // actually has one — otherwise a long label would be clipped for
        // nothing (the flyout is already sized to fit both). The column is
        // MEASURED: "Alt+Ctrl+Entrée" overflows a fixed width.
        let accel_w = item.accel.as_deref().map(|a| self.measure(a, &f.caption) + CONTENT_RIGHT);
        let reserved = match (accel_w, item.has_submenu) {
            (Some(w), _) => w + ACCEL_GAP,
            (None, true) => CONTENT_RIGHT + 12.0,
            (None, false) => CONTENT_RIGHT,
        };
        let label = Rect::new(rect.left + LABEL_LEFT, rect.top, rect.right - reserved, rect.bottom);
        self.text(&item.label, &label, &f.body, &fg, false);
        if let (Some(accel), Some(w)) = (&item.accel, accel_w) {
            let accel_rect =
                Rect::new(rect.right - w, rect.top, rect.right - CONTENT_RIGHT, rect.bottom);
            // On the accent pill the shortcut keeps a slight `opacity: 0.85`, so
            // it stays secondary to the label it belongs to.
            let color = if hot {
                faded(&t.accent_foreground, HOVER_SHORTCUT_OPACITY)
            } else {
                t.text_secondary
            };
            let color = if item.enabled { color } else { faded(&color, DISABLED_OPACITY) };
            self.text_aligned(accel, &accel_rect, &f.caption, &color, DWRITE_TEXT_ALIGNMENT_TRAILING);
        }
        if item.has_submenu {
            let chevron = Rect::new(
                rect.right - CONTENT_RIGHT - 12.0,
                rect.top,
                rect.right - CONTENT_RIGHT,
                rect.bottom,
            );
            // The caret shares the shortcut column, hence its colours.
            let color = if hot { t.accent_foreground } else { t.text_secondary };
            let color = if item.enabled { color } else { faded(&color, DISABLED_OPACITY) };
            self.text(GLYPH_CHEVRON_RIGHT, &chevron, &f.icon_small, &color, true);
        }
    }

    /// One AppBarButton of the PrimaryCommands band: icon only, no label, with
    /// the same hover pill as a menu item.
    fn draw_primary_button(&self, rect: &Rect, item: &FlyoutItem, hot: bool) {
        let t = self.theme;
        let hot = hot && item.enabled;
        if hot {
            self.fill_rounded(&rect.inflate(-2.0, -2.0), PILL_RADIUS, &t.accent);
        }
        // Same colour rules as a menu row: accent at rest, danger for a
        // destructive command, the pill's foreground under the accent fill.
        let color = if hot {
            t.accent_foreground
        } else if item.is_danger() {
            t.danger
        } else {
            t.accent
        };
        let fg = if item.enabled { color } else { faded(&color, DISABLED_OPACITY) };
        let cx = (rect.left + rect.right) / 2.0;
        let cy = (rect.top + rect.bottom) / 2.0;
        let icon = Rect::new(cx - 8.0, cy - 8.0, cx + 8.0, cy + 8.0);
        match item.icon {
            Some(name) => self.vector_icon_layered(name, &icon, 16.0, &fg, &fg),
            None => self.text(item.glyph, &icon, &self.renderer.formats.icon_small, &fg, true),
        }
    }

    /// Draws one menu panel from the popup's top-left (0,0): the
    /// CommandBarFlyout's PrimaryCommands band (when it has one), then the
    /// SecondaryCommands rows. The panel background, border and shadow belong to
    /// the popup window itself (acrylic + DWM), so nothing of them is drawn here.
    pub(crate) fn draw_menu(
        &self,
        items: &[FlyoutItem],
        primary: &[FlyoutItem],
        hot: Option<usize>,
        hot_primary: Option<usize>,
        width: f32,
        dpi: f32,
    ) {
        self.set_scale(dpi / 96.0);

        let mut top = FLYOUT_PADDING;
        if !primary.is_empty() {
            for (i, item) in primary.iter().enumerate() {
                let left = PRIMARY_PADDING + PRIMARY_BUTTON * i as f32;
                let rect = Rect::new(
                    left,
                    PRIMARY_PADDING,
                    left + PRIMARY_BUTTON,
                    PRIMARY_PADDING + PRIMARY_BUTTON,
                );
                self.draw_primary_button(&rect, item, hot_primary == Some(i));
            }
            // The command bar is its own panel: a 1 px divider runs edge to edge
            // where it meets the menu below.
            let y = self.px(PRIMARY_BAND);
            let line = Rect::new(0.0, y, width, y + 1.0 / self.scale());
            self.fill_rounded(&line, 0.0, &self.theme.divider);
            top = PRIMARY_BAND + FLYOUT_PADDING;
        }

        for i in 0..items.len() {
            let rect = row_rect(items, i, 0.0, width, top);
            self.draw_flyout_item(&rect, &items[i], hot == Some(i));
        }
    }
}
