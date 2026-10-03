//! The event arguments the shell's user controls raise to the window.

use kubuno::views::events::EventArgs;

/// A command of a page the window carries out (`sync_now`, `open_folder`, `browse`, `submit`…).
#[derive(EventArgs, Debug, Clone, Default, PartialEq)]
pub struct CommandEventArgs {
    pub command: String,
}

impl CommandEventArgs {
    pub fn new(command: &str) -> Self {
        Self { command: command.to_string() }
    }
}

/// A setting the user changed on the settings page: its name and its new value, as text.
#[derive(EventArgs, Debug, Clone, Default, PartialEq)]
pub struct SettingEventArgs {
    /// `theme`, `interval`, `notifications`, `autostart`, `offline`, `proxy`.
    pub name: String,
    pub value: String,
}

/// A command about one item of a list (an account, a label): what, which, and an optional value.
#[derive(EventArgs, Debug, Clone, Default, PartialEq)]
pub struct ItemCommandEventArgs {
    /// `select`, `move`, `disconnect`, `add` (accounts); `create`, `delete`, `colour` (labels).
    pub command: String,
    /// The item's id (an instance id, a label id); empty for a command about the list.
    pub id: String,
    /// The command's value (a label's name or colour).
    pub value: String,
}

impl ItemCommandEventArgs {
    pub fn new(command: &str, id: &str, value: &str) -> Self {
        Self { command: command.to_string(), id: id.to_string(), value: value.to_string() }
    }
}

/// What a section of the administration console asks the window for: the section, the command
/// (`load`: another page or a search; `open-web`: the section in the browser; `set-module`…), the item
/// it is about, and a value.
#[derive(EventArgs, Debug, Clone, Default, PartialEq)]
pub struct AdminCommandEventArgs {
    pub section: String,
    pub command: String,
    pub id: String,
    pub value: String,
}
