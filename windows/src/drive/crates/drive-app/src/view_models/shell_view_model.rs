//! Port of `Files.App/ViewModels/ShellViewModel.cs`: the state of a tab —
//! location, history, listing, sort (`OrderFiles`), filter, layout, Columns
//! panes. Also hosts `FolderLayoutModes` (C#: `Data/Enums`) and the sizes
//! from `LayoutSizeKindHelper` (C#: `Helpers/Layout`).

use std::path::{Path, PathBuf};

use crate::data::items::{load_directory, DirEntryItem};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Location {
    Home,
    Dir(PathBuf),
    Settings,
    /// The Recycle Bin (`Shell:RecycleBinFolder`, `ContentPageTypes.RecycleBin`).
    RecycleBin,
    /// Results of a recursive search (`IsSearchResultPage`,
    /// `ContentPageTypes.SearchResults`): `root` = starting folder.
    SearchResults { query: String, root: String },
}

/// Layout of the file area — port of `FolderLayoutModes` (minus `Adaptive`,
/// which the bar only shows when `LayoutAdaptive.IsExecutable`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ViewMode {
    Details,
    List,
    Cards,
    Columns,
    Grid,
}

impl ViewMode {
    pub const ALL: [ViewMode; 5] =
        [ViewMode::Details, ViewMode::List, ViewMode::Cards, ViewMode::Grid, ViewMode::Columns];

    /// The number of ticks on the size slider: `Maximum` of the Slider in
    /// Toolbar.xaml (Details/List/Columns 5, Cards 4, Grid 12).
    pub fn size_max(self) -> u8 {
        match self {
            ViewMode::Details | ViewMode::List | ViewMode::Columns => 5,
            ViewMode::Cards => 4,
            ViewMode::Grid => 12,
        }
    }

    /// The ticks marked with an icon under the slider, and their icon. The
    /// grid only has four, placed on ticks 1, 2, 8 and 12 — these are the
    /// named values of `GridViewSizeKind` (Small, Medium, Large, ExtraLarge).
    pub fn size_ticks(self) -> &'static [(u8, &'static str)] {
        match self {
            ViewMode::Cards => &[
                (1, "SizeSmall28"),
                (2, "SizeMedium28"),
                (3, "SizeLarge28"),
                (4, "SizeExtraLarge28"),
            ],
            ViewMode::Grid => &[
                (1, "SizeSmall28"),
                (2, "SizeMedium28"),
                (8, "SizeLarge28"),
                (12, "SizeExtraLarge28"),
            ],
            _ => &[
                (1, "SizeCompact28"),
                (2, "SizeSmall28"),
                (3, "SizeMedium28"),
                (4, "SizeLarge28"),
                (5, "SizeExtraLarge28"),
            ],
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            ViewMode::Details => "LayoutDetails28",
            ViewMode::List => "LayoutList28",
            ViewMode::Cards => "LayoutTiles28",
            ViewMode::Grid => "LayoutGrid28",
            ViewMode::Columns => "LayoutColumns28",
        }
    }

    /// The label's resource key (`Commands.Layout*.Label`).
    pub fn label_key(self) -> &'static str {
        match self {
            ViewMode::Details => "Details",
            ViewMode::List => "List",
            ViewMode::Cards => "Cards",
            ViewMode::Grid => "Grid",
            ViewMode::Columns => "Columns",
        }
    }
}

/// The saved size for the `mode` layout (`LayoutSettingsService`).
pub fn layout_size(mode: ViewMode) -> u8 {
    let s = crate::services::settings::get();
    match mode {
        ViewMode::Details => s.details_view_size,
        ViewMode::List => s.list_view_size,
        ViewMode::Cards => s.cards_view_size,
        ViewMode::Columns => s.columns_view_size,
        ViewMode::Grid => s.grid_view_size,
    }
    .clamp(1, mode.size_max())
}

/// Height of a Details row. Rescaled onto the Kubuno web list: at the
/// default density its rows are `px-4 py-2.5` = 40 DIP
/// (`shape::height::FILE_ROW`), and size 1 is the web's "compact" density.
/// The larger sizes keep the original 4 DIP progression above the default.
pub fn row_height_for(size: u8) -> f32 {
    use drive_app_controls::themes::shape::height::FILE_ROW;
    match size {
        1 => FILE_ROW - 8.0,
        2 => FILE_ROW,
        3 => FILE_ROW + 4.0,
        4 => FILE_ROW + 8.0,
        _ => FILE_ROW + 12.0,
    }
}

/// Same for List and Columns, whose rows carry a name only and so stay one
/// step below Details (the default lands on the 36 DIP nav-row height).
pub fn list_row_height_for(size: u8) -> f32 {
    match size {
        1 => 28.0,
        2 => 36.0,
        3 => 40.0,
        4 => 44.0,
        _ => 48.0,
    }
}

/// `LayoutSizeKindHelper.GetGridViewItemWidth`: 80 → 300 in steps of 20.
pub fn grid_item_width_for(size: u8) -> f32 {
    60.0 + 20.0 * size.clamp(1, 12) as f32
}

/// The name row below the icon box in grid layout (`Margin="4,0,4,8"`, at
/// most two lines).
pub const GRID_NAME_ROW: f32 = 44.0;

/// The DISPLAY size of the icon.
///
/// Careful: `LayoutSizeKindHelper.GetIconSize` gives the size of the
/// *thumbnail requested from the shell* (96/128/256 in grid), not the size
/// it's actually painted at. In grid layout, `GridLayoutPage.xaml` places
/// the image in a square `ItemWidthGridView` box with `Margin="12"` and
/// `Stretch="Uniform"`: the icon therefore ends up `width - 24`. The other
/// layouts, on the other hand, do paint the icon at the requested size.
/// Where a row's NAME column starts, measured from the row's left edge: the
/// icon gutter (8) + the icon + a gap (8). Both the column header and the cell
/// read it from here, so a larger icon size pushes the name instead of being
/// overlapped by it. Never narrower than the 36 the default density gives.
pub fn name_left_for(mode: ViewMode, size: u8) -> f32 {
    (8.0 + icon_size_for(mode, size) + 8.0).max(36.0)
}

pub fn icon_size_for(mode: ViewMode, size: u8) -> f32 {
    match mode {
        // Row icons follow the web list: its `FolderGlyph` is 20 px at the
        // default density, 16 when compact.
        ViewMode::Details | ViewMode::List | ViewMode::Columns => match size {
            1 => 16.0,
            2 => 20.0,
            3 => 24.0,
            4 => 28.0,
            _ => 32.0,
        },
        // `CardsViewIconSize` = `GetIconSize(CardsView)`.
        ViewMode::Cards => match size {
            1 | 2 => 64.0,
            3 => 80.0,
            _ => 96.0,
        },
        ViewMode::Grid => grid_item_width_for(size) - 24.0,
    }
}

/// The Cards layout's tile: icon box + details box
/// (`CardsViewIconBox*` / `CardsViewDetailsBox*`), placed side by side for
/// the small size and stacked for the others (`CardsViewOrientation`).
pub fn card_size_for(size: u8) -> (f32, f32) {
    let ((ibw, ibh), (dbw, dbh)) = card_boxes(size);
    if cards_horizontal(size) {
        (ibw + dbw, ibh.max(dbh))
    } else {
        (ibw.max(dbw), ibh + dbh)
    }
}

/// `CardsViewOrientation`: horizontal for the small card, vertical otherwise.
pub fn cards_horizontal(size: u8) -> bool {
    size <= 1
}

/// (icon box, details box).
pub fn card_boxes(size: u8) -> ((f32, f32), (f32, f32)) {
    match size {
        1 => ((104.0, 104.0), (196.0, 104.0)),
        2 => ((240.0, 96.0), (240.0, 144.0)),
        3 => ((280.0, 128.0), (280.0, 144.0)),
        _ => ((320.0, 160.0), (320.0, 128.0)),
    }
}

/// Sortable columns of the details view (port of `SortOption`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SortColumn {
    Name,
    DateModified,
    Type,
    Size,
    /// Recycle Bin only (`SortOption.OriginalFolder`).
    OriginalPath,
    /// Recycle Bin only (`SortOption.DateDeleted`).
    DateDeleted,
}

/// The 4 columns of the Details view: (.resw key, sort column). The
/// Recycle Bin replaces "Date modified / Type" with "Original path / Date
/// deleted" (`ColumnsViewModel`: OriginalPath + DateDeleted visible).
pub fn detail_columns(recycle: bool) -> [(&'static str, SortColumn); 4] {
    if recycle {
        [
            ("Name", SortColumn::Name),
            ("OriginalPath", SortColumn::OriginalPath),
            ("DateDeleted", SortColumn::DateDeleted),
            ("Size", SortColumn::Size),
        ]
    } else {
        [
            ("Name", SortColumn::Name),
            ("DateModifiedLowerCase", SortColumn::DateModified),
            ("Type", SortColumn::Type),
            ("Size", SortColumn::Size),
        ]
    }
}

/// The sort's folders/files grouping (`SortFilesFirst` +
/// `SortDirectoriesAlongsideFiles`): the original's radio trio.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum SortGrouping {
    #[default]
    FoldersFirst,
    FilesFirst,
    Together,
}

impl SortGrouping {
    pub fn of(files_first: bool, alongside: bool) -> Self {
        if alongside {
            Self::Together
        } else if files_first {
            Self::FilesFirst
        } else {
            Self::FoldersFirst
        }
    }
}

/// The comparator from `ShellViewModel.OrderFiles`: folders/files grouping
/// first, then the requested column — and the direction only reverses
/// WITHIN each group.
pub fn sort_entries(
    entries: &mut [DirEntryItem],
    column: SortColumn,
    ascending: bool,
    grouping: SortGrouping,
) {
    entries.sort_by(|a, b| {
        let group = match grouping {
            SortGrouping::FoldersFirst => b.is_dir.cmp(&a.is_dir),
            SortGrouping::FilesFirst => a.is_dir.cmp(&b.is_dir),
            SortGrouping::Together => std::cmp::Ordering::Equal,
        };
        let ordering = group.then_with(|| match column {
            SortColumn::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            SortColumn::DateModified | SortColumn::DateDeleted => a.modified.cmp(&b.modified),
            SortColumn::Type => a.type_text().cmp(&b.type_text()),
            SortColumn::Size => a.size.cmp(&b.size),
            SortColumn::OriginalPath => a
                .original_path
                .as_deref()
                .unwrap_or("")
                .to_lowercase()
                .cmp(&b.original_path.as_deref().unwrap_or("").to_lowercase()),
        });
        if ascending || !ordering.is_ne() {
            ordering
        } else {
            match (a.is_dir, b.is_dir) {
                (true, false) | (false, true) if grouping != SortGrouping::Together => ordering,
                _ => ordering.reverse(),
            }
        }
    });
}

/// A pane of `ColumnsLayoutPage` (a 200 DIP `BladeItem`, cf. the XAML
/// `Style`): a `ColumnLayoutPage` listing a folder.
pub struct ColumnPane {
    pub path: std::path::PathBuf,
    pub entries: Vec<DirEntryItem>,
    /// The selected row — it's the one that opened the next pane.
    pub selected: Option<usize>,
    pub scroll: f32,
}

/// `BladeItem`: `Width="200"`, 1px right border (`DividerStrokeColorDefault`).
pub const COLUMN_PANE_WIDTH: f32 = 200.0;

impl ColumnPane {
    pub fn load(
        path: std::path::PathBuf,
        sort: SortColumn,
        ascending: bool,
        grouping: SortGrouping,
    ) -> Self {
        let mut entries = load_directory(&path.to_string_lossy()).unwrap_or_default();
        sort_entries(&mut entries, sort, ascending, grouping);
        Self { path, entries, selected: None, scroll: 0.0 }
    }
}

pub struct Tab {
    pub location: Location,
    pub entries: Vec<DirEntryItem>,
    /// Unfiltered listing; `entries` is the filtered+sorted view.
    all_entries: Vec<DirEntryItem>,
    pub filter: String,
    pub load_error: Option<String>,
    pub history: Vec<Location>,
    pub history_index: usize,
    pub scroll: f32,
    /// The multi-selection (`ShellViewModel.SelectedItems`): the indices in
    /// `entries`, sorted.
    pub selected: std::collections::BTreeSet<usize>,
    /// The starting point of a Shift+click selection.
    pub selection_anchor: Option<usize>,
    /// The ListView's keyboard FOCUS item: the selection's head, the one
    /// that the arrow keys move and that Ctrl+Space toggles.
    pub focused: Option<usize>,
    pub sort_column: SortColumn,
    pub sort_ascending: bool,
    /// The folders/files radio trio (`SortFilesFirst`,
    /// `SortDirectoriesAlongsideFiles`).
    pub sort_files_first: bool,
    pub sort_directories_alongside_files: bool,
    /// The grouping (`DirectoryGroupOption` / `DirectoryGroupDirection` /
    /// `DirectoryGroupByDateUnit`).
    pub group_option: GroupOption,
    pub group_ascending: bool,
    pub group_by_date_unit: crate::services::settings::GroupByDateUnit,
    pub view_mode: ViewMode,
    /// The Columns layout's panes (`ColumnHost.Items`). Empty in any other
    /// layout.
    pub columns: Vec<ColumnPane>,
    /// The folder's git state (`StatusBarViewModel.GitBranchDisplayName` +
    /// ahead/behind). `git_branch` non-null ⇒ git repo: the git UI shows.
    pub git_branch: Option<String>,
    pub git_ahead: usize,
    pub git_behind: usize,
}

impl Tab {
    pub fn new_home() -> Self {
        Self {
            location: Location::Home,
            entries: Vec::new(),
            all_entries: Vec::new(),
            filter: String::new(),
            load_error: None,
            history: vec![Location::Home],
            history_index: 0,
            scroll: 0.0,
            selected: std::collections::BTreeSet::new(),
            selection_anchor: None,
            focused: None,
            sort_column: SortColumn::Name,
            sort_ascending: true,
            sort_files_first: false,
            sort_directories_alongside_files: false,
            group_option: GroupOption::None,
            group_ascending: true,
            group_by_date_unit: Default::default(),
            view_mode: ViewMode::Details,
            columns: Vec::new(),
            git_branch: None,
            git_ahead: 0,
            git_behind: 0,
        }
    }

    /// Recomputes the current git branch + ahead/behind
    /// (`DirectoryPropertiesViewModel.UpdateGitInfo`). Cleared outside a repo
    /// or outside a folder (Home, search).
    fn update_git_info(&mut self) {
        let info = match &self.location {
            Location::Dir(path) => crate::utils::git::head_info(&path.to_string_lossy()),
            _ => None,
        };
        match info {
            Some(i) => {
                self.git_branch = Some(i.branch);
                self.git_ahead = i.ahead;
                self.git_behind = i.behind;
            }
            None => {
                self.git_branch = None;
                self.git_ahead = 0;
                self.git_behind = 0;
            }
        }
    }

    /// The sort's current grouping.
    pub fn sort_grouping(&self) -> SortGrouping {
        SortGrouping::of(self.sort_files_first, self.sort_directories_alongside_files)
    }

    /// The first selected item (the original's `SelectedItem`).
    pub fn selected_entry(&self) -> Option<&DirEntryItem> {
        // In Columns view, the item described by the info pane is the
        // selection of the rightmost pane (`ActiveColumnShellPage`), not
        // the main list's.
        if self.view_mode == ViewMode::Columns {
            return self.column_selection();
        }
        self.selected.first().and_then(|&i| self.entries.get(i))
    }

    /// The paths of the whole selection, in display order.
    pub fn selected_paths(&self) -> Vec<String> {
        self.selected
            .iter()
            .filter_map(|&i| self.entries.get(i).map(|e| e.path.clone()))
            .collect()
    }

    /// Simple click: the selection collapses to `i`, which becomes the anchor.
    pub fn select_single(&mut self, i: usize) {
        self.selected.clear();
        self.selected.insert(i);
        self.selection_anchor = Some(i);
        self.focused = Some(i);
    }

    /// Ctrl+click: toggles `i` without touching the rest.
    pub fn toggle_select(&mut self, i: usize) {
        if !self.selected.remove(&i) {
            self.selected.insert(i);
        }
        self.selection_anchor = Some(i);
        self.focused = Some(i);
    }

    /// Shift+click: selects the contiguous block between the anchor and `i`.
    pub fn select_range(&mut self, i: usize) {
        let anchor = self.selection_anchor.unwrap_or(i);
        self.selected.clear();
        for k in anchor.min(i)..=anchor.max(i) {
            self.selected.insert(k);
        }
        self.focused = Some(i);
    }

    /// `SelectAllAction` / `InvertSelectionAction` / `ClearSelectionAction`.
    pub fn select_all(&mut self) {
        self.selected = (0..self.entries.len()).collect();
    }

    pub fn invert_selection(&mut self) {
        self.selected = (0..self.entries.len())
            .filter(|i| !self.selected.contains(i))
            .collect();
    }

    pub fn clear_selection(&mut self) {
        self.selected.clear();
        self.selection_anchor = None;
        self.focused = None;
    }

    /// (Re)builds the pane stack: `ColumnsLayoutPage` opens a single one, on
    /// the current folder, and the following ones are born from selections.
    pub fn rebuild_columns(&mut self) {
        self.columns.clear();
        if self.view_mode != ViewMode::Columns {
            return;
        }
        if let Location::Dir(path) = &self.location {
            self.columns.push(ColumnPane::load(
                path.clone(),
                self.sort_column,
                self.sort_ascending,
                self.sort_grouping(),
            ));
        }
    }

    /// Click on row `row` of pane `col`.
    ///
    /// `DismissOtherBlades(index)`: everything to the right of the clicked
    /// pane disappears; if the item is a folder, a new pane opens to its
    /// right. The current path follows the selection
    /// (`SetSelectedPathOrNavigate`), but WITHOUT rebuilding the stack —
    /// otherwise we'd lose the column to the left.
    pub fn select_column_row(&mut self, col: usize, row: usize) {
        let Some(pane) = self.columns.get_mut(col) else { return };
        let Some(entry) = pane.entries.get(row) else { return };
        let (path, is_dir) = (std::path::PathBuf::from(&entry.path), entry.is_dir);
        pane.selected = Some(row);
        // `SetSelectedPathOrNavigate`: if the pane immediately to the right
        // ALREADY shows this folder, keep it (and the following ones)
        // instead of reloading everything; otherwise close the panes to the
        // right and open a new one.
        let reuse_next = is_dir
            && self.columns.get(col + 1).is_some_and(|p| p.path == path);
        if reuse_next {
            self.columns.truncate(col + 2);
        } else {
            self.columns.truncate(col + 1);
        }
        if is_dir {
            if !reuse_next {
                self.columns.push(ColumnPane::load(
                    path.clone(),
                    self.sort_column,
                    self.sort_ascending,
                    self.sort_grouping(),
                ));
            }
            // The breadcrumb follows the opened pane (new or reused).
            self.location = Location::Dir(path.clone());
            self.history.truncate(self.history_index + 1);
            self.history.push(Location::Dir(path));
            self.history_index = self.history.len() - 1;
        }
    }

    /// The selected item in the rightmost pane that has one — it's the one
    /// the info pane describes.
    pub fn column_selection(&self) -> Option<&DirEntryItem> {
        self.columns
            .iter()
            .rev()
            .find_map(|p| p.selected.and_then(|i| p.entries.get(i)))
    }

    /// Toggles direction when the column is already active, otherwise sorts
    /// ascending by the new column (Explorer behavior).
    pub fn sort_by(&mut self, column: SortColumn) {
        if self.sort_column == column {
            self.sort_ascending = !self.sort_ascending;
        } else {
            self.sort_column = column;
            self.sort_ascending = true;
        }
        self.apply_sort();
        // The Columns layout's panes carry their own listing: a sort change
        // rebuilds them.
        self.rebuild_columns();
    }

    /// Sets the sort column and direction outright — what the context menu's
    /// SortBy* / SortAscending / SortDescending commands do (they are toggles
    /// bound to a value, not a click on a column header).
    pub fn set_sort(&mut self, column: SortColumn, ascending: bool) {
        self.sort_column = column;
        self.sort_ascending = ascending;
        self.apply_sort();
        self.rebuild_columns();
    }

    /// Live name filter (port of `FilesAndFoldersFilter`).
    pub fn set_filter(&mut self, filter: &str) {
        self.filter = filter.to_string();
        self.apply_sort();
    }

    fn apply_sort(&mut self) {
        self.selected.clear();
        self.selection_anchor = None;
        self.focused = None;
        let needle = self.filter.to_lowercase();
        self.entries = self
            .all_entries
            .iter()
            .filter(|e| needle.is_empty() || e.name.to_lowercase().contains(&needle))
            .cloned()
            .collect();
        let grouping = self.sort_grouping();
        sort_entries(&mut self.entries, self.sort_column, self.sort_ascending, grouping);
        // The grouping (`GroupedCollection`): a STABLE sort by group key on
        // top of the elements' sort — each group keeps its internal order.
        if self.group_option != GroupOption::None {
            let option = self.group_option;
            let ascending = self.group_ascending;
            let unit = self.group_by_date_unit;
            self.entries.sort_by(|a, b| {
                let ka = option.key(a, unit);
                let kb = option.key(b, unit);
                let ord = ka.cmp(&kb);
                if ascending { ord } else { ord.reverse() }
            });
        }
    }

    /// The group boundaries over `entries`: (start index, label, group
    /// size). Empty with no grouping.
    pub fn group_boundaries(&self) -> Vec<(usize, String, usize)> {
        let mut out = Vec::new();
        if self.group_option == GroupOption::None {
            return out;
        }
        let mut current: Option<(usize, String)> = None;
        for (i, e) in self.entries.iter().enumerate() {
            let label = self.group_option.key(e, self.group_by_date_unit).1;
            match &current {
                Some((start, l)) if *l == label => {
                    let _ = start;
                }
                _ => {
                    if let Some((start, l)) = current.take() {
                        out.push((start, l, i - start));
                    }
                    current = Some((i, label));
                }
            }
        }
        if let Some((start, l)) = current {
            out.push((start, l, self.entries.len() - start));
        }
        out
    }

    pub fn title(&self) -> String {
        match &self.location {
            Location::Home => drive_localization::tr("Home").to_string(),
            Location::Settings => drive_localization::tr("Settings").to_string(),
            Location::RecycleBin => drive_localization::tr("RecycleBin").to_string(),
            // The tab title follows the search FOLDER (`SearchPathParam`).
            Location::SearchResults { root, .. } => std::path::Path::new(root)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| root.clone()),
            Location::Dir(path) => path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.to_string_lossy().trim_end_matches('\\').to_string()),
        }
    }

    /// Path segments for the breadcrumb bar: (display name, full path).
    pub fn breadcrumbs(&self) -> Vec<(String, PathBuf)> {
        if self.location == Location::RecycleBin {
            // A single segment, not navigable (empty path).
            return vec![(drive_localization::tr("RecycleBin").to_string(), PathBuf::new())];
        }
        if let Location::SearchResults { root, query } = &self.location {
            // Single-segment override: "Search results in {1} for {0}"
            // (`SearchPagePathBoxOverrideText`), not navigable.
            let name = std::path::Path::new(root)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| root.clone());
            let label = drive_localization::tr("SearchPagePathBoxOverrideText")
                .replacen("{0}", query, 1)
                .replacen("{1}", &name, 1);
            return vec![(label, PathBuf::new())];
        }
        let Location::Dir(path) = &self.location else {
            return Vec::new();
        };
        let mut segments = Vec::new();
        let mut current = PathBuf::new();
        for component in path.components() {
            use std::path::Component;
            match component {
                Component::Prefix(prefix) => {
                    current.push(component.as_os_str());
                    segments.push((prefix.as_os_str().to_string_lossy().into_owned(), PathBuf::new()));
                }
                Component::RootDir => {
                    current.push(component.as_os_str());
                    if let Some(last) = segments.last_mut() {
                        last.1 = current.clone();
                    }
                }
                Component::Normal(name) => {
                    current.push(name);
                    segments.push((name.to_string_lossy().into_owned(), current.clone()));
                }
                _ => {}
            }
        }
        segments
    }

    pub fn navigate(&mut self, location: Location) {
        self.history.truncate(self.history_index + 1);
        self.history.push(location.clone());
        self.history_index = self.history.len() - 1;
        self.set_location(location);
    }

    pub fn can_go_back(&self) -> bool {
        self.history_index > 0
    }

    pub fn can_go_forward(&self) -> bool {
        self.history_index + 1 < self.history.len()
    }

    pub fn can_go_up(&self) -> bool {
        matches!(&self.location, Location::Dir(_))
    }

    pub fn go_back(&mut self) {
        if self.can_go_back() {
            self.history_index -= 1;
            self.set_location(self.history[self.history_index].clone());
        }
    }

    pub fn go_forward(&mut self) {
        if self.can_go_forward() {
            self.history_index += 1;
            self.set_location(self.history[self.history_index].clone());
        }
    }

    /// Jumps to an absolute history index (Back/Forward history flyouts).
    pub fn go_to_history(&mut self, index: usize) {
        if index < self.history.len() && index != self.history_index {
            self.history_index = index;
            self.set_location(self.history[index].clone());
        }
    }

    pub fn go_up(&mut self) {
        if let Location::Dir(path) = &self.location {
            match path.parent().filter(|p| !p.as_os_str().is_empty()) {
                Some(parent) => self.navigate(Location::Dir(parent.to_path_buf())),
                None => self.navigate(Location::Home),
            }
        }
    }

    pub fn refresh(&mut self) {
        self.load();
        self.update_git_info();
    }

    fn set_location(&mut self, location: Location) {
        self.location = location;
        self.scroll = 0.0;
        self.selected.clear();
        self.selection_anchor = None;
        self.focused = None;
        // `BaseShellPage.OnNavigatedTo` reloads the folder's preferences
        // BEFORE listing: a folder's layout and sort are its own (cf.
        // `LayoutPreferencesManager`).
        if let Location::Dir(path) = &self.location {
            let prefs = crate::helpers::layout_preferences::get(&path.to_string_lossy());
            self.view_mode = prefs.layout_mode;
            self.sort_column = prefs.sort_column;
            self.sort_ascending = prefs.sort_ascending;
            self.sort_files_first = prefs.sort_files_first;
            self.sort_directories_alongside_files = prefs.sort_directories_alongside_files;
            self.group_option = prefs.group_option;
            self.group_ascending = prefs.group_ascending;
            self.group_by_date_unit = prefs.group_by_date_unit;
        }
        self.load();
        self.rebuild_columns();
        self.update_git_info();
    }

    /// The tab's current preferences, as they get saved.
    pub fn prefs(&self) -> crate::helpers::layout_preferences::LayoutPreferences {
        crate::helpers::layout_preferences::LayoutPreferences {
            layout_mode: self.view_mode,
            sort_column: self.sort_column,
            sort_ascending: self.sort_ascending,
            sort_files_first: self.sort_files_first,
            sort_directories_alongside_files: self.sort_directories_alongside_files,
            group_option: self.group_option,
            group_ascending: self.group_ascending,
            group_by_date_unit: self.group_by_date_unit,
        }
    }

    /// Appends a batch of search results (progressive `SearchTick`) and
    /// re-sorts/re-filters. No effect outside a search-results tab.
    pub fn append_search_results(&mut self, mut batch: Vec<crate::data::items::DirEntryItem>) {
        if !matches!(self.location, Location::SearchResults { .. }) {
            return;
        }
        self.all_entries.append(&mut batch);
        self.apply_sort();
    }

    /// Writes the current folder's preferences (`SetLayoutPreferencesForPath`).
    pub fn save_prefs(&self) {
        if let Location::Dir(path) = &self.location {
            crate::helpers::layout_preferences::set(&path.to_string_lossy(), self.prefs());
        }
    }

    fn load(&mut self) {
        self.entries.clear();
        self.all_entries.clear();
        self.load_error = None;
        if self.location == Location::RecycleBin {
            // The Recycle Bin via the Shell — `date_deleted` occupies `modified`
            // (the "Date deleted" column and its sort).
            self.all_entries = crate::services::storage::storage_trash_bin_service::list()
                .into_iter()
                .map(|item| crate::data::items::DirEntryItem {
                    name: item.name,
                    path: item.path,
                    is_dir: item.is_dir,
                    size: item.size,
                    size_known: true,
                    modified: item.date_deleted,
                    original_path: Some(item.original_path),
                })
                .collect();
            self.apply_sort();
            return;
        }
        if let Location::Dir(path) = &self.location {
            match load_directory(&path.to_string_lossy()) {
                Ok(entries) => {
                    self.all_entries = entries;
                    self.apply_sort();
                }
                Err(e) => {
                    self.load_error = Some(match e.kind() {
                        std::io::ErrorKind::PermissionDenied => {
                            "Accès refusé à ce dossier.".to_string()
                        }
                        _ => format!("Impossible d'ouvrir ce dossier : {e}"),
                    });
                }
            }
        }
    }
}

/// A tab of the main window: one or two panes, each with independent
/// navigation (port of `ShellPanesPage` + `MultiPanesContext`).
pub struct TabGroup {
    pub panes: Vec<Tab>,
    pub active_pane: usize,
    /// true = panes side by side (Vertical split), false = stacked.
    pub split_vertical: bool,
    /// Fraction of the space occupied by the FIRST pane (`GridSplitter`
    /// star-width). 0.5 = equal split; clamped to keep ≥ 100 DIP each.
    pub split_ratio: f32,
}

impl TabGroup {
    pub fn new_home() -> Self {
        Self {
            panes: vec![Tab::new_home()],
            active_pane: 0,
            split_vertical: true,
            split_ratio: 0.5,
        }
    }

    pub fn active(&self) -> &Tab {
        &self.panes[self.active_pane]
    }

    pub fn active_mut(&mut self) -> &mut Tab {
        &mut self.panes[self.active_pane]
    }

    pub fn other(&self) -> Option<&Tab> {
        (self.panes.len() == 2).then(|| &self.panes[1 - self.active_pane])
    }

    /// The tab-bar title follows the active pane.
    pub fn title(&self) -> String {
        self.active().title()
    }

    /// Port of `SplitPaneVertically/Horizontally`: duplicates the current
    /// location into a second pane (no-op reorients when already split).
    pub fn split(&mut self, vertical: bool) {
        self.split_vertical = vertical;
        if self.panes.len() == 1 {
            let mut pane = Tab::new_home();
            if let Location::Dir(path) = &self.active().location {
                pane.navigate(Location::Dir(path.clone()));
            }
            self.panes.push(pane);
        }
    }

    /// Port of `CloseActivePane` — keeps the other pane.
    pub fn close_active_pane(&mut self) {
        if self.panes.len() == 2 {
            self.panes.remove(self.active_pane);
            self.active_pane = 0;
        }
    }

    /// Port of `ToggleDualPaneAction`: closes the second pane if it exists,
    /// otherwise opens one (current arrangement).
    pub fn toggle_dual_pane(&mut self) {
        if self.panes.len() == 2 {
            self.close_active_pane();
        } else {
            self.split(self.split_vertical);
        }
    }
}

/// Invokes a shell verb on an item — what the original's commands do through
/// `IStorable.TryInvokeContextMenuVerbAsync` (properties, pintohome…).
pub fn shell_verb(path: &str, verb: &str) {
    if let Some(item) = drive_app_storage::windows_storage::WindowsStorable::try_parse(path) {
        let _ = item.storable().try_invoke_context_menu_verb(verb);
    }
}

/// Opens a file (or shortcut) with its default handler.
pub fn shell_open(path: &str) {
    use windows::core::HSTRING;
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    unsafe {
        ShellExecuteW(
            None,
            &HSTRING::from("open"),
            &HSTRING::from(path),
            None,
            None,
            SW_SHOWNORMAL,
        );
    }
}

/// Whether a path can be browsed in-app (a real directory).
pub fn browsable(path: &str) -> bool {
    Path::new(path).is_dir()
}

/// Port of `GroupOption` (`Data/Enums/GroupOption.cs`), reduced to the ported
/// options (DateCreated, FileTag, SyncStatus… will come with their columns).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum GroupOption {
    #[default]
    None,
    Name,
    DateModified,
    Type,
    Size,
}

impl GroupOption {
    /// The group key of an item (`GroupingHelper.GetItemGroupKeySelector`):
    /// (group sort rank, header label).
    pub fn key(
        self,
        e: &DirEntryItem,
        unit: crate::services::settings::GroupByDateUnit,
    ) -> (i64, String) {
        let tr = drive_localization::tr;
        match self {
            GroupOption::None => (0, String::new()),
            // The first letter, uppercased.
            GroupOption::Name => {
                let c = e.name.chars().next().unwrap_or(' ').to_uppercase().next().unwrap_or(' ');
                (c as i64, c.to_string())
            }
            GroupOption::DateModified => match e.modified {
                Some(t) => {
                    let (rank, label) =
                        crate::services::date_time_formatter::time_span_label_unit(t, unit);
                    // NEGATIVE rank: most recent first, the original's
                    // ascending grouping order.
                    (-rank, label)
                }
                None => (0, String::new()),
            },
            // Folder → its type; file → its lowercase extension.
            GroupOption::Type => {
                if e.is_dir {
                    (0, e.type_text())
                } else {
                    let ext = std::path::Path::new(&e.name)
                        .extension()
                        .map(|x| x.to_string_lossy().to_lowercase())
                        .unwrap_or_else(|| " ".into());
                    (1, ext)
                }
            }
            // The bands from `GroupingHelper.sizeGroups`; folders keep their
            // own group, like the original (no computed size).
            GroupOption::Size => {
                if e.is_dir {
                    (-1, tr("Folders").to_string())
                } else {
                    let bands: [(u64, &str); 5] = [
                        (5_000_000_000, "ItemSizeText_Huge"),
                        (1_000_000_000, "ItemSizeText_VeryLarge"),
                        (128_000_000, "ItemSizeText_Large"),
                        (1_000_000, "ItemSizeText_Medium"),
                        (16_000, "ItemSizeText_Small"),
                    ];
                    for (i, (size, key)) in bands.iter().enumerate() {
                        if e.size > *size {
                            return ((bands.len() - i) as i64, tr(key).to_string());
                        }
                    }
                    (0, tr("ItemSizeText_Tiny").to_string())
                }
            }
        }
    }
}
