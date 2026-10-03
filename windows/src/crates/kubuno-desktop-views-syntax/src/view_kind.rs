//! The two kinds of view file (`vskubuno/docs/VIEWS-SPEC.md` §1.1, "File kinds"): a form, window or
//! dialog is a `.kbview`; a user control - a view whose root is `<UserControl>`, or whose code-behind
//! declares it as one - is a `.kbcontrol`. Both are the same XML format; only the role differs.
//!
//! The file rules live here (both targets apply them); deciding the kind of a given view from its
//! code-behind, the `view-file-kind` diagnostic and its rename quick fix are the language server's
//! (`kubuno-desktop-views-ls`' `view_kind`, which re-exports these items).

use std::path::Path;

/// The extension of a form, window or dialog view (no dot).
pub const VIEW_EXTENSION: &str = "kbview";

/// The extension of a user control view (no dot).
pub const CONTROL_EXTENSION: &str = "kbcontrol";

/// The diagnostic code of an extension that disagrees with the view's kind.
pub const DIAGNOSTIC_CODE: &str = "view-file-kind";

/// The root element that makes a view a user control whatever its code-behind says.
pub const USER_CONTROL_ROOT: &str = "UserControl";

/// Whether `path` is a view file of either kind.
pub fn is_view_file(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case(VIEW_EXTENSION) || e.eq_ignore_ascii_case(CONTROL_EXTENSION))
}

/// What a view file holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewKind {
    /// A form, a window, a dialog, a flyout… (`.kbview`).
    Form,
    /// A user control (`.kbcontrol`).
    UserControl,
}

impl ViewKind {
    /// The extension a file of this kind takes (no dot).
    pub fn extension(self) -> &'static str {
        match self {
            ViewKind::Form => VIEW_EXTENSION,
            ViewKind::UserControl => CONTROL_EXTENSION,
        }
    }
}

/// The kind the extension of `path` announces, `None` for any other file.
pub fn kind_of_extension(path: &Path) -> Option<ViewKind> {
    let ext = path.extension()?.to_str()?;
    if ext.eq_ignore_ascii_case(VIEW_EXTENSION) {
        Some(ViewKind::Form)
    } else if ext.eq_ignore_ascii_case(CONTROL_EXTENSION) {
        Some(ViewKind::UserControl)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extensions_and_kinds() {
        assert!(is_view_file(Path::new("views/main_view.kbview")));
        assert!(is_view_file(Path::new("Row.KbControl")));
        assert!(!is_view_file(Path::new("strings.kbres")));
        assert!(!is_view_file(Path::new("kbview")));
        assert_eq!(kind_of_extension(Path::new("x.KBVIEW")), Some(ViewKind::Form));
        assert_eq!(kind_of_extension(Path::new("x.rs")), None);
        assert_eq!(ViewKind::UserControl.extension(), CONTROL_EXTENSION);
    }
}
