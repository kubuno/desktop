//! In-place cell editing of a [`DataTable`] — WinForms `DataGridView` with its default
//! `EditMode = EditOnKeystrokeOrF2`.
//!
//! Off unless [`DataTable::editable`] is set (a hand-built table keeps its look and its keys), and
//! per column with [`flags::READ_ONLY`]. The table keeps the **current cell** (the keyboard
//! [`DataTable::cursor`] row and [`DataTable::current_column`]) and, while a cell is edited, a
//! [`CellEditor`]: a real [`TextField`] laid over the cell, so the caret, the selection, the
//! clipboard, the undo history, the context menu and the IME composition are the text field's own.
//!
//! The table never writes a value anywhere: it says what the user asked for as a [`CellAction`]
//! and the caller (a view node, a page) decides — raises its cancelable events, writes the data
//! back, then calls [`DataTable::begin_edit`] / [`DataTable::end_edit`] / [`DataTable::cancel_edit`].
//!
//! | input (the table holds the focus) | not editing | editing |
//! |---|---|---|
//! | F2 | [`CellAction::BeginEdit`] (the cell's text, caret at the end) | — |
//! | a character typed | [`CellAction::BeginEdit`] with that text | typed into the editor |
//! | double-click on a cell | [`CellAction::BeginEdit`] (whole text selected) | the editor's own |
//! | ← → | the current cell moves one column | the caret moves |
//! | ↑ ↓ PgUp PgDn Home End | the current row moves ([`DataTable::on_key`]) | ↑ ↓: [`CellAction::Commit`] then move |
//! | Tab / Shift+Tab | next / previous cell, wrapping to the next / previous row; past the last / first cell [`CellAction::LeaveGrid`] | [`CellAction::Commit`] then move the same way |
//! | Enter | [`crate::tables::TableEvent::RowActivated`] (unchanged) | [`CellAction::Commit`] then down |
//! | Escape | [`CellAction::CancelRowEdit`] | [`CellAction::CancelEdit`] |
//! | a click elsewhere, the focus leaving | — | [`CellAction::Commit`] (the caller commits, then moves) |

use kubuno_drive_desktop_app_controls::{Canvas, Rect};
use kubuno_desktop_controls::host::{self, vk, InputEvent, Modifiers};
use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;

use super::{flags, grid, has_flag, DataTable, Mode};
use crate::metrics::radius;
use crate::text::{EditInput, TextField};
use crate::widget::{Widget, WidgetState};

/// The side of the round error glyph drawn in a cell in error, DIP (WinForms' 16 px error icon,
/// the same size as the view runtime's ErrorProvider glyph).
pub const ERROR_GLYPH: f32 = 16.0;

/// Where the current cell goes after a commit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellMove {
    /// Stays on the edited cell (a click elsewhere, the focus leaving: the caller moves).
    Stay,
    /// One row down (Enter, ↓), staying on the last row.
    Down,
    /// One row up (↑).
    Up,
    /// The next cell (Tab): one column right, or the first column of the next row.
    Next,
    /// The previous cell (Shift+Tab).
    Previous,
}

/// What the user asked the table for — see the module doc for which input gives which.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CellAction {
    /// Begin editing `row` (an item index) / `column` (a **visible** column). `initial`: the text
    /// the editor starts with (a typed character), `None` for the cell's own text; `select_all`:
    /// the whole text is selected (a double-click) rather than the caret put at the end. The
    /// caller raises its cancelable `CellBeginEdit`, then calls [`DataTable::begin_edit`].
    BeginEdit { row: usize, column: usize, initial: Option<String>, select_all: bool },
    /// The editor's `text` is proposed for `row` / `column`: the caller validates and writes it,
    /// then calls [`DataTable::end_edit`] and [`DataTable::move_current_cell`] with `then`
    /// (a refused value keeps the editor open: the caller does neither).
    Commit { row: usize, column: usize, text: String, then: CellMove },
    /// Escape in the editor: the caller calls [`DataTable::cancel_edit`] (the cell shows its
    /// value again).
    CancelEdit { row: usize, column: usize },
    /// Escape on a cell that is not being edited: the caller cancels the row's pending edit
    /// (WinForms' second Escape, `BindingSource.CancelEdit`).
    CancelRowEdit { row: usize },
    /// Tab past the last cell (`forward`) or Shift+Tab before the first: the focus leaves the
    /// table (WinForms' `StandardTab = false` behaviour at the grid's ends).
    LeaveGrid { forward: bool },
}

/// One cell being edited: where, what it showed when the edit began, and the text field.
#[derive(Clone)]
pub struct CellEditor {
    /// The item index of the edited row.
    pub row: usize,
    /// The **visible** column edited.
    pub column: usize,
    /// The cell's text when the edit began.
    pub original: String,
    field: TextField,
    /// The press that began the edit (a double-click) is still held: the field does not see it
    /// (it is not a click in the new field) until the button is released.
    ignore_press: bool,
}

impl CellEditor {
    /// The text in the editor now.
    pub fn text(&self) -> &str {
        self.field.text()
    }

    /// The editor's text field (caret, selection, undo…).
    pub fn field(&self) -> &TextField {
        &self.field
    }

    /// The editor's text field, to drive it from code (`set_text`, `select`).
    pub fn field_mut(&mut self) -> &mut TextField {
        &mut self.field
    }
}

/// A cell showing an error glyph (WinForms `DataGridViewCell.ErrorText`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CellError {
    /// The item index of the row.
    pub row: usize,
    /// The **model** column (an index into the columns, hidden ones included).
    pub column: usize,
    pub message: String,
}

fn char_len(s: &str) -> i32 {
    i32::try_from(s.chars().count()).unwrap_or(i32::MAX)
}

impl DataTable {
    // ── Current cell ─────────────────────────────────────────────────────

    /// Whether the **visible** column `column` can be edited: the table is
    /// [`DataTable::editable`] and the column exists and does not carry [`flags::READ_ONLY`].
    pub fn column_editable(&self, column: usize) -> bool {
        self.editable && self.visible_columns().get(column).is_some_and(|c| !has_flag(c, flags::READ_ONLY))
    }

    /// The current cell — `(item row, visible column)`, WinForms `CurrentCell` — once the
    /// keyboard cursor stands on a row.
    pub fn current_cell(&self) -> Option<(usize, usize)> {
        let row = self.cursor.filter(|&r| r < self.list.item_count())?;
        let n = self.visible_columns().len();
        (n > 0).then(|| (row, self.current_column.min(n - 1)))
    }

    /// Makes `(row, column)` the current cell: the cursor moves there (turning the page and
    /// scrolling the row into view), the selection is left to the caller.
    pub fn set_current_cell(&mut self, bounds: Rect, row: usize, column: usize) {
        let n = self.visible_columns().len();
        self.current_column = column.min(n.saturating_sub(1));
        if row < self.list.item_count() {
            self.move_cursor(bounds, row, false);
        }
    }

    /// Where the current cell goes for `then`, from `(row, column)`: `None` past the table's
    /// ends for [`CellMove::Next`] / [`CellMove::Previous`] (the focus leaves), the same cell
    /// at the ends for the vertical moves.
    pub fn cell_after(&self, row: usize, column: usize, then: CellMove) -> Option<(usize, usize)> {
        let rows = self.list.item_count();
        let cols = self.visible_columns().len();
        if rows == 0 || cols == 0 {
            return None;
        }
        let (row, column) = (row.min(rows - 1), column.min(cols - 1));
        Some(match then {
            CellMove::Stay => (row, column),
            CellMove::Down => ((row + 1).min(rows - 1), column),
            CellMove::Up => (row.saturating_sub(1), column),
            CellMove::Next if column + 1 < cols => (row, column + 1),
            CellMove::Next if row + 1 < rows => (row + 1, 0),
            CellMove::Next => return None,
            CellMove::Previous if column > 0 => (row, column - 1),
            CellMove::Previous if row > 0 => (row - 1, cols - 1),
            CellMove::Previous => return None,
        })
    }

    /// Moves the current cell by `then` (after a commit, or Tab without an edit). Returns the
    /// new current cell, `None` when the move leaves the table.
    pub fn move_current_cell(&mut self, bounds: Rect, then: CellMove) -> Option<(usize, usize)> {
        let (row, column) = self.current_cell()?;
        let (r, c) = self.cell_after(row, column, then)?;
        self.set_current_cell(bounds, r, c);
        Some((r, c))
    }

    // ── Editing ──────────────────────────────────────────────────────────

    /// Whether a cell is being edited.
    pub fn is_editing(&self) -> bool {
        self.editor.is_some()
    }

    /// The cell being edited, if any.
    pub fn editor(&self) -> Option<&CellEditor> {
        self.editor.as_ref()
    }

    /// The cell being edited, to drive its text field from code.
    pub fn editor_mut(&mut self) -> Option<&mut CellEditor> {
        self.editor.as_mut()
    }

    /// Begins editing `row` / `column` (see [`CellAction::BeginEdit`]); the cell becomes the
    /// current cell. `false` (nothing happens) when the cell cannot be edited or does not exist.
    /// An edit already in progress is dropped: commit it first.
    pub fn begin_edit(&mut self, row: usize, column: usize, initial: Option<&str>, select_all: bool) -> bool {
        if !self.column_editable(column) || row >= self.list.item_count() {
            return false;
        }
        let original = self.cell_text(row, column);
        let text = initial.map_or_else(|| original.clone(), str::to_string);
        let mut field = TextField::new();
        field.reset_text(&text);
        if let Some(col) = self.visible_columns().get(column) {
            field.text_align = col.text_align;
        }
        let selection = if select_all { (0, char_len(&text)) } else { (char_len(&text), 0) };
        field.select(selection.0, selection.1);
        // It appears focused: no select-all of a focus gain, the caret stays where it was put.
        field.assume_focused();
        self.cursor = Some(row);
        self.current_column = column;
        self.editor = Some(CellEditor { row, column, original, field, ignore_press: true });
        true
    }

    /// Ends the edit after its value was accepted and returns the editor (its text is what was
    /// committed). The cell then shows whatever the caller put in the item.
    pub fn end_edit(&mut self) -> Option<CellEditor> {
        self.editor.take()
    }

    /// Cancels the edit: the typed text is dropped and the cell shows its value again.
    pub fn cancel_edit(&mut self) -> Option<CellEditor> {
        self.editor.take()
    }

    /// The box the editor is laid over — the cell's whole column span on its row, inset by
    /// 2 DIP so the text field's border shows inside the row. `None` without an editor or when
    /// the row is not laid out as a table row on this page.
    pub fn editor_rect(&self, bounds: Rect) -> Option<Rect> {
        let e = self.editor.as_ref()?;
        self.cell_box(bounds, e.row, e.column).map(|r| r.inflate(-2.0, -2.0))
    }

    /// The whole box of a cell (padding included): `row`'s band across **visible** column
    /// `column`. `None` off the current page or outside the table layout.
    pub fn cell_box(&self, bounds: Rect, row: usize, column: usize) -> Option<Rect> {
        let l = self.layout_of(bounds);
        if l.mode != Mode::Table || !self.page_range().contains(&row) {
            return None;
        }
        let xs = self.column_x_offsets(l.frame);
        let (a, b) = (*xs.get(column)?, *xs.get(column + 1)?);
        let r = self.row_rect_in(&l, row);
        Some(Rect::new(a, r.top, b, r.bottom))
    }

    /// The **visible** column under `x` on a body row, for a click that picks a cell.
    pub fn cell_column_at(&self, bounds: Rect, x: f32) -> Option<usize> {
        let l = self.layout_of(bounds);
        let xs = self.column_x_offsets(l.frame);
        (0..xs.len().saturating_sub(1)).find(|&i| x >= xs[i] && x < xs[i + 1])
    }

    // ── Keys ─────────────────────────────────────────────────────────────

    /// A key-down on the rows while NO cell is edited (see the module doc): F2 begins an edit,
    /// ← → move the current column, Tab / Shift+Tab walk the cells, Escape asks for the row's
    /// edit to be cancelled. `(used, action)`: an unused key is the caller's (then
    /// [`DataTable::on_key`] moves the rows). Nothing is used when the table is not editable.
    pub fn cell_key(&mut self, bounds: Rect, key: u16, mods: Modifiers) -> (bool, Option<CellAction>) {
        if !self.editable || self.editor.is_some() {
            return (false, None);
        }
        let Some((row, column)) = self.current_cell().or_else(|| {
            let first = self.page_range().start;
            (first < self.list.item_count() && !self.visible_columns().is_empty()).then_some((first, 0))
        }) else {
            return (false, None);
        };
        let plain = mods.matches(Modifiers::NONE);
        match key {
            vk::F2 if plain => {
                let action = self.column_editable(column).then_some(CellAction::BeginEdit { row, column, initial: None, select_all: false });
                (true, action)
            }
            vk::LEFT | vk::RIGHT if plain => {
                let n = self.visible_columns().len();
                let c = if key == vk::LEFT { column.saturating_sub(1) } else { (column + 1).min(n.saturating_sub(1)) };
                self.set_current_cell(bounds, row, c);
                (true, None)
            }
            vk::TAB if plain || mods.matches(Modifiers::SHIFT) => {
                let then = if mods.shift { CellMove::Previous } else { CellMove::Next };
                if self.current_cell().is_none() {
                    self.set_current_cell(bounds, row, column);
                }
                match self.move_current_cell(bounds, then) {
                    Some(_) => (true, None),
                    None => (true, Some(CellAction::LeaveGrid { forward: !mods.shift })),
                }
            }
            vk::ESCAPE if plain => (true, Some(CellAction::CancelRowEdit { row })),
            _ => (false, None),
        }
    }

    /// Text typed while no cell is edited: begins editing the current cell with it
    /// (`EditOnKeystroke`). `None` when the table or the column is read-only.
    pub fn cell_text_input(&mut self, text: &str) -> Option<CellAction> {
        if !self.editable || self.editor.is_some() || text.chars().all(char::is_control) {
            return None;
        }
        let (row, column) = self.current_cell().or_else(|| {
            let first = self.page_range().start;
            (first < self.list.item_count() && !self.visible_columns().is_empty()).then_some((first, 0))
        })?;
        self.column_editable(column).then(|| CellAction::BeginEdit { row, column, initial: Some(text.to_string()), select_all: false })
    }

    /// A key-down while a cell is edited, for the keys the TABLE handles (the others are the
    /// text field's): Enter / ↓ commit and go down, ↑ up, Tab / Shift+Tab to the next /
    /// previous cell, Escape cancels. `None` for any other key.
    pub fn editor_key(&self, key: u16, mods: Modifiers) -> Option<CellAction> {
        let e = self.editor.as_ref()?;
        let plain = mods.matches(Modifiers::NONE);
        let then = match key {
            vk::ENTER | vk::DOWN if plain => CellMove::Down,
            vk::UP if plain => CellMove::Up,
            vk::TAB if plain => CellMove::Next,
            vk::TAB if mods.matches(Modifiers::SHIFT) => CellMove::Previous,
            vk::ESCAPE if plain => return Some(CellAction::CancelEdit { row: e.row, column: e.column }),
            _ => return None,
        };
        Some(CellAction::Commit { row: e.row, column: e.column, text: e.text().to_string(), then })
    }

    /// Whether [`DataTable::editor_key`] handles `key` with `mods`.
    fn is_editor_key(&self, key: u16, mods: Modifiers) -> bool {
        self.editor.is_some() && self.editor_key(key, mods).is_some()
    }

    /// One frame of the editor, before painting: the table's own keys ([`DataTable::editor_key`])
    /// are taken from the host's queue first, then the text field runs (pointer, typing, caret,
    /// clipboard, IME) with `input` — whose `focused` says whether the TABLE holds the focus.
    /// Returns the action of the first table key pressed this frame, else a
    /// [`CellAction::Commit`] with [`CellMove::Stay`] when the table lost the focus.
    pub fn update_editor(&mut self, c: &dyn Canvas, bounds: Rect, input: &EditInput) -> Option<CellAction> {
        let rect = self.editor_rect(bounds)?;
        let mut first: Option<(u16, Modifiers)> = None;
        if input.focused {
            let taken = host::consume(|e| match e {
                InputEvent::Key { vk: k, down: true, mods, .. } => self.is_editor_key(*k, *mods),
                _ => false,
            });
            first = taken.into_iter().find_map(|e| match e {
                InputEvent::Key { vk: k, mods, .. } => Some((k, mods)),
                _ => None,
            });
        }
        let editor = self.editor.as_mut()?;
        let mut frame_input = *input;
        editor.ignore_press &= input.mouse_down || input.right_down;
        if editor.ignore_press {
            // The press that began the edit is not a click in the new field.
            frame_input.mouse_down = false;
            frame_input.right_down = false;
            frame_input.click_count = 0;
        }
        let outcome = editor.field.update(c, rect, &frame_input);
        if let Some((k, mods)) = first {
            return self.editor_key(k, mods);
        }
        if outcome.submitted {
            return self.editor_key(vk::ENTER, Modifiers::NONE);
        }
        if !input.focused {
            let e = self.editor.as_ref()?;
            return Some(CellAction::Commit { row: e.row, column: e.column, text: e.text().to_string(), then: CellMove::Stay });
        }
        None
    }

    // ── Paint ────────────────────────────────────────────────────────────

    /// The error glyph of a cell: a round danger-coloured badge with a white exclamation mark,
    /// vertically centred at the right end of the cell box `cell`.
    pub fn error_glyph_rect(cell: Rect) -> Rect {
        let cy = (cell.top + cell.bottom) / 2.0;
        let right = cell.right - grid::SELECT_PAD_X / 2.0;
        Rect::new(right - ERROR_GLYPH, cy - ERROR_GLYPH / 2.0, right, cy + ERROR_GLYPH / 2.0)
    }

    pub(super) fn paint_error_glyph(c: &dyn Canvas, cell: Rect) {
        let r = Self::error_glyph_rect(cell);
        let white = D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 1.0 };
        c.fill_rounded(&r, ERROR_GLYPH / 2.0, &c.theme().danger);
        let mid = (r.left + r.right) / 2.0;
        c.fill_rounded(&Rect::new(mid - 1.0, r.top + 3.5, mid + 1.0, r.top + 10.0), 0.0, &white);
        c.fill_rounded(&Rect::new(mid - 1.0, r.top + 11.5, mid + 1.0, r.top + 13.5), 0.0, &white);
    }

    /// The current cell's ring: a 2 DIP accent outline inside the cell, drawn while the rows
    /// hold a visible focus and no cell is edited (an editable table's replacement for the row
    /// ring).
    pub(super) fn paint_current_cell(&self, c: &dyn Canvas, bounds_of_body: Rect, cell: Rect) {
        let inset = grid::FOCUS_RING / 2.0;
        let (left, right) = (cell.left.max(bounds_of_body.left), cell.right.min(bounds_of_body.right));
        let r = Rect::new(left + inset, cell.top + inset, right - inset, cell.bottom - inset);
        c.stroke_rounded_w(&r, radius::SM, &c.theme().accent, grid::FOCUS_RING);
    }

    /// The editor over its cell (focused: caret and outline).
    pub(super) fn paint_editor(&self, c: &dyn Canvas, rect: Rect) {
        if let Some(e) = &self.editor {
            e.field.paint(c, rect, WidgetState::REST.focused(true).focus_visible(true));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tables::{column, flags, with_flag, Layout};
    use crate::views::ListViewItem;

    fn bounds() -> Rect {
        Rect::new(0.0, 0.0, 800.0, 600.0)
    }

    fn grid(rows: usize) -> DataTable {
        let mut t = DataTable::new();
        t.layout = Layout::Table;
        t.columns = vec![column("name", "Nom", 200), column("amount", "Montant", 120), with_flag(column("id", "Id", 60), flags::READ_ONLY)];
        for i in 0..rows {
            t.items.push(ListViewItem::new(format!("Row {i}")).with_sub(format!("{i}.5")).with_sub(i.to_string()));
        }
        t.editable = true;
        t
    }

    const NONE: Modifiers = Modifiers::NONE;

    #[test]
    fn a_read_only_table_uses_no_key_and_begins_nothing() {
        let mut t = grid(3);
        t.editable = false;
        assert_eq!(t.cell_key(bounds(), vk::F2, NONE), (false, None));
        assert_eq!(t.cell_text_input("a"), None);
        assert!(!t.begin_edit(0, 0, None, false));
        assert!(!t.column_editable(0));
    }

    #[test]
    fn f2_and_typing_begin_an_edit_of_the_current_cell() {
        let mut t = grid(3);
        // No current cell yet: the first cell of the page.
        assert_eq!(t.cell_key(bounds(), vk::F2, NONE), (true, Some(CellAction::BeginEdit { row: 0, column: 0, initial: None, select_all: false })));
        t.set_current_cell(bounds(), 1, 1);
        assert_eq!(t.cell_text_input("7"), Some(CellAction::BeginEdit { row: 1, column: 1, initial: Some("7".into()), select_all: false }));
        assert_eq!(t.cell_text_input("\r"), None, "control characters are keys, not text");
        // A read-only column: F2 is used (nothing else happens), typing begins nothing.
        t.set_current_cell(bounds(), 1, 2);
        assert_eq!(t.cell_key(bounds(), vk::F2, NONE), (true, None));
        assert_eq!(t.cell_text_input("x"), None);
        assert!(!t.begin_edit(1, 2, None, false));
    }

    #[test]
    fn begin_edit_starts_from_the_cell_text_or_the_typed_one() {
        let mut t = grid(3);
        assert!(t.begin_edit(2, 1, None, false));
        let e = t.editor().expect("editing");
        assert_eq!((e.row, e.column, e.text(), e.original.as_str()), (2, 1, "2.5", "2.5"));
        assert_eq!(e.field().selection_start(), 3, "F2: the caret at the end");
        assert_eq!(t.current_cell(), Some((2, 1)));
        assert!(t.begin_edit(0, 0, Some("Z"), false));
        assert_eq!(t.editor().map(|e| (e.text().to_string(), e.original.clone())), Some(("Z".into(), "Row 0".into())));
        assert!(t.begin_edit(0, 0, None, true));
        assert_eq!(t.editor().map(|e| e.field().selection_length()), Some(5), "a double-click selects the whole text");
    }

    #[test]
    fn enter_tab_arrows_commit_and_escape_cancels() {
        let mut t = grid(3);
        assert!(t.begin_edit(1, 0, Some("New"), false));
        let commit = |then| Some(CellAction::Commit { row: 1, column: 0, text: "New".into(), then });
        assert_eq!(t.editor_key(vk::ENTER, NONE), commit(CellMove::Down));
        assert_eq!(t.editor_key(vk::DOWN, NONE), commit(CellMove::Down));
        assert_eq!(t.editor_key(vk::UP, NONE), commit(CellMove::Up));
        assert_eq!(t.editor_key(vk::TAB, NONE), commit(CellMove::Next));
        assert_eq!(t.editor_key(vk::TAB, Modifiers::SHIFT), commit(CellMove::Previous));
        assert_eq!(t.editor_key(vk::ESCAPE, NONE), Some(CellAction::CancelEdit { row: 1, column: 0 }));
        assert_eq!(t.editor_key(vk::LEFT, NONE), None, "the caret keys are the text field's");
        assert_eq!(t.editor_key(vk::letter('A'), NONE), None);
        // While editing, the not-editing keys are not the table's.
        assert_eq!(t.cell_key(bounds(), vk::F2, NONE), (false, None));
        assert_eq!(t.cell_text_input("x"), None);
        assert!(t.cancel_edit().is_some());
        assert!(!t.is_editing());
        assert_eq!(t.editor_key(vk::ENTER, NONE), None);
    }

    #[test]
    fn the_current_cell_walks_like_a_datagridview() {
        let mut t = grid(2);
        t.set_current_cell(bounds(), 0, 0);
        assert_eq!(t.cell_key(bounds(), vk::RIGHT, NONE), (true, None));
        assert_eq!(t.current_cell(), Some((0, 1)));
        t.cell_key(bounds(), vk::RIGHT, NONE);
        t.cell_key(bounds(), vk::RIGHT, NONE);
        assert_eq!(t.current_cell(), Some((0, 2)), "stops at the last column");
        // Tab wraps to the next row, then leaves the grid after the last cell.
        assert_eq!(t.cell_key(bounds(), vk::TAB, NONE), (true, None));
        assert_eq!(t.current_cell(), Some((1, 0)));
        t.set_current_cell(bounds(), 1, 2);
        assert_eq!(t.cell_key(bounds(), vk::TAB, NONE), (true, Some(CellAction::LeaveGrid { forward: true })));
        assert_eq!(t.current_cell(), Some((1, 2)));
        t.set_current_cell(bounds(), 0, 0);
        assert_eq!(t.cell_key(bounds(), vk::TAB, Modifiers::SHIFT), (true, Some(CellAction::LeaveGrid { forward: false })));
        t.set_current_cell(bounds(), 1, 0);
        t.cell_key(bounds(), vk::TAB, Modifiers::SHIFT);
        assert_eq!(t.current_cell(), Some((0, 2)), "Shift+Tab wraps back to the previous row's last cell");
        // Escape without an edit: the row's edit is to be cancelled.
        assert_eq!(t.cell_key(bounds(), vk::ESCAPE, NONE), (true, Some(CellAction::CancelRowEdit { row: 0 })));
        // Up/Down are the rows' (DataTable::on_key).
        assert_eq!(t.cell_key(bounds(), vk::DOWN, NONE), (false, None));
    }

    #[test]
    fn a_commit_moves_down_and_stays_on_the_last_row() {
        let t = grid(2);
        assert_eq!(t.cell_after(0, 1, CellMove::Down), Some((1, 1)));
        assert_eq!(t.cell_after(1, 1, CellMove::Down), Some((1, 1)));
        assert_eq!(t.cell_after(0, 1, CellMove::Up), Some((0, 1)));
        assert_eq!(t.cell_after(1, 1, CellMove::Stay), Some((1, 1)));
        assert_eq!(grid(0).cell_after(0, 0, CellMove::Down), None);
    }

    #[test]
    fn the_editor_covers_its_cell_inside_the_row() {
        let mut t = grid(3);
        assert!(t.editor_rect(bounds()).is_none());
        assert!(t.begin_edit(1, 1, None, false));
        let cell = t.cell_box(bounds(), 1, 1).expect("laid out");
        let row = t.row_rect(bounds(), 1);
        assert_eq!((cell.top, cell.bottom), (row.top, row.bottom));
        let xs = t.column_x_offsets(t.layout_of(bounds()).frame);
        assert_eq!((cell.left, cell.right), (xs[1], xs[2]));
        let ed = t.editor_rect(bounds()).expect("editing");
        assert_eq!((ed.left, ed.top, ed.right, ed.bottom), (cell.left + 2.0, cell.top + 2.0, cell.right - 2.0, cell.bottom - 2.0));
        assert_eq!(t.cell_column_at(bounds(), (cell.left + cell.right) / 2.0), Some(1));
        let g = DataTable::error_glyph_rect(cell);
        assert!(g.right <= cell.right && g.left >= cell.left && (g.right - g.left) == ERROR_GLYPH);
    }
}
