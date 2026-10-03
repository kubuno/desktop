//! Code-behind of the user control `StatusPresenter` (`status_presenter.kbcontrol`): what a page says
//! when it has no content yet — loading, nothing here, or that failed.
//!
//! Each page used to say these with a line of grey text, each with its own wording, colour and
//! place. `kubuno-ui` ships a `Spinner`, an `EmptyState` and a `Callout`; this control puts the
//! three shapes in one place: the shell's **editorial** decisions (a failure is a danger callout
//! rather than red text, an empty section gets a medallion and a sentence, loading turns).

use kubuno::views::prelude::*;

/// Which of the three the presenter shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StatusMode {
    /// Nothing: the page has content.
    #[default]
    None,
    Loading,
    Empty,
    Error,
}

impl StatusMode {
    pub fn name(self) -> &'static str {
        match self {
            StatusMode::None => "None",
            StatusMode::Loading => "Loading",
            StatusMode::Empty => "Empty",
            StatusMode::Error => "Error",
        }
    }

    pub fn parse(text: &str) -> Self {
        match text {
            "Loading" => StatusMode::Loading,
            "Empty" => StatusMode::Empty,
            "Error" => StatusMode::Error,
            _ => StatusMode::None,
        }
    }

    /// What a page with `has_content` shows, while it is `loading` and after `error` (empty: none):
    /// a failure first (it does not always mean there is nothing else), then the spinner over an
    /// empty page, then the empty state.
    pub fn of(has_content: bool, loading: bool, error: &str) -> Self {
        if !error.is_empty() {
            StatusMode::Error
        } else if has_content {
            StatusMode::None
        } else if loading {
            StatusMode::Loading
        } else {
            StatusMode::Empty
        }
    }
}

/// Loading / empty / error (see the module doc).
#[derive(UserControl, Default)]
#[user_control(view = "status_presenter.kbcontrol")]
#[category("Kubuno")]
pub struct StatusPresenter {
    base: UserControlCore,
    /// `None`, `Loading`, `Empty` or `Error`.
    #[property(bindable, on_change = "mode_changed")]
    #[category("Behavior")]
    pub mode: String,
    /// The empty state's glyph (a Kubuno icon name).
    #[property(bindable)]
    #[category("Appearance")]
    pub empty_icon: String,
    /// The short statement (« Aucun groupe »).
    #[property(bindable)]
    #[category("Appearance")]
    pub empty_title: String,
    /// The sentence that says what would appear here (empty: the title alone).
    #[property(bindable)]
    #[category("Appearance")]
    pub empty_text: String,
    /// The failure's message.
    #[property(bindable)]
    #[category("Appearance")]
    pub error_text: String,
    #[property(bindable)]
    #[browsable(false)]
    pub is_loading: bool,
    #[property(bindable)]
    #[browsable(false)]
    pub is_empty: bool,
    #[property(bindable)]
    #[browsable(false)]
    pub is_error: bool,
}

impl StatusPresenter {
    /// Shows `mode`, with the empty state's words and the failure's message.
    pub fn show(&mut self, mode: StatusMode, icon: &str, title: &str, text: &str, error: &str) {
        self.empty_icon = icon.to_string();
        self.empty_title = title.to_string();
        self.empty_text = text.to_string();
        self.error_text = error.to_string();
        self.mode = mode.name().to_string();
        self.mode_changed();
    }

    fn mode_changed(&mut self) {
        let mode = StatusMode::parse(&self.mode);
        self.is_loading = mode == StatusMode::Loading;
        self.is_empty = mode == StatusMode::Empty;
        self.is_error = mode == StatusMode::Error;
    }
}

#[kubuno::views::event_handlers]
impl StatusPresenter {
    fn status_presenter_load(&mut self) {
        if self.design_mode() && self.mode.is_empty() {
            self.show(StatusMode::Empty, "Inbox", "Rien pour l'instant", "Ce qui apparaîtra ici est décrit par cette phrase.", "");
        }
        self.mode_changed();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failure_wins_then_the_spinner_then_the_empty_state() {
        assert_eq!(StatusMode::of(true, true, "boom"), StatusMode::Error);
        assert_eq!(StatusMode::of(true, true, ""), StatusMode::None);
        assert_eq!(StatusMode::of(false, true, ""), StatusMode::Loading);
        assert_eq!(StatusMode::of(false, false, ""), StatusMode::Empty);
        for m in [StatusMode::None, StatusMode::Loading, StatusMode::Empty, StatusMode::Error] {
            assert_eq!(StatusMode::parse(m.name()), m);
        }
    }
}
