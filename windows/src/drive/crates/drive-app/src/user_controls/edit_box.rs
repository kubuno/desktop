//! Port of the inline TextBox editors of
//! `Files.App/Views/Layouts/DetailsLayoutPage.xaml` (inline rename) and
//! `Files.App/UserControls/NavigationToolbar.xaml` (Omnibar path/search
//! input): single-line edit state (caret, selection) and its D2D drawing.


/// `EditState.entry` value meaning "this editor is the search/filter box".
pub const EDIT_SEARCH: usize = usize::MAX;
/// `EditState.entry` value meaning "this editor is the path input"
/// (Omnibar path mode — EditPath action, Ctrl+L / Alt+D).
pub const EDIT_PATH: usize = usize::MAX - 1;
/// `EditState.entry` value meaning "this editor is the command palette"
/// (Omnibar command-palette mode — OpenCommandPalette, Ctrl+Maj+P).
pub const EDIT_PALETTE: usize = usize::MAX - 2;

/// The text field of a `ContentDialog` (CreateArchiveDialog…).
pub const EDIT_DIALOG: usize = usize::MAX - 3;

/// Inline single-line text editor state (rename, filter…).
pub struct EditState {
    /// Index of the entry being renamed in the active pane.
    pub entry: usize,
    pub text: String,
    /// Caret position in bytes (always on a char boundary).
    pub caret: usize,
    /// Selection anchor; equal to `caret` when nothing is selected.
    pub anchor: usize,
}

impl EditState {
    pub fn selection(&self) -> (usize, usize) {
        (self.caret.min(self.anchor), self.caret.max(self.anchor))
    }

    pub fn replace_selection(&mut self, s: &str) {
        let (a, b) = self.selection();
        self.text.replace_range(a..b, s);
        self.caret = a + s.len();
        self.anchor = self.caret;
    }

    pub fn move_caret(&mut self, forward: bool, extend: bool) {
        if forward {
            if let Some(c) = self.text[self.caret..].chars().next() {
                self.caret += c.len_utf8();
            }
        } else if let Some(c) = self.text[..self.caret].chars().next_back() {
            self.caret -= c.len_utf8();
        }
        if !extend {
            self.anchor = self.caret;
        }
    }
}

