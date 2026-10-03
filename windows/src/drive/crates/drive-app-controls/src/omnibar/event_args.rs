//! Omnibar event arguments (mirror of
//! `Files.App.Controls/Omnibar/EventArgs.cs`).
//! The C# `record class`es are transcribed as data structs. Opaque payloads
//! on the WinUI side (`Item`/`SelectedItem` of type `object`) have no
//! portable counterpart and are omitted; only the scalar fields are ported.

use super::omnibar_mode::OmnibarMode;
use super::omnibar_text_change_reason::OmnibarTextChangeReason;

/// `OmnibarQuerySubmittedEventArgs(Mode, Item, Text)`. `Item` (object?) omitted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OmnibarQuerySubmittedEventArgs {
    pub mode: OmnibarMode,
    pub text: String,
}

/// `OmnibarSuggestionChosenEventArgs(Mode, SelectedItem)`. `SelectedItem`
/// (object) omitted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OmnibarSuggestionChosenEventArgs {
    pub mode: OmnibarMode,
}

/// `OmnibarTextChangedEventArgs(Mode, Reason)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OmnibarTextChangedEventArgs {
    pub mode: OmnibarMode,
    pub reason: OmnibarTextChangeReason,
}

/// `OmnibarModeChangedEventArgs(OldMode?, NewMode)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OmnibarModeChangedEventArgs {
    pub old_mode: Option<OmnibarMode>,
    pub new_mode: OmnibarMode,
}

/// `OmnibarIsFocusedChangedEventArgs(IsFocused)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OmnibarIsFocusedChangedEventArgs {
    pub is_focused: bool,
}
