//! `OmnibarTextChangeReason` (mirror of
//! `Files.App.Controls/Omnibar/OmnibarTextChangeReason.cs`).

/// Reason for an Omnibar text change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OmnibarTextChangeReason {
    UserInput,
    SuggestionChosen,
    ProgrammaticChange,
    None,
}
