//! The event args of the data components, and how they are raised ([`emit`]): to the Rust
//! subscribers at once, then to the handler the component's element names in the view
//! (`OnCurrentChanged="…"`) — synchronously when the view runtime or a binding called the
//! component, else queued for the runtime (`vskubuno/docs/DATA.md` §7, DATA-2).

use kubuno_views::component::{ComponentCore, Site};
use kubuno_views::events::{CancelEventArgs, ElementRef, Event, EventArgs};

use crate::value::DbValue;

/// The state of a connection (ADO.NET `ConnectionState`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum ConnectionState {
    #[default]
    Closed,
    Connecting,
    Open,
    /// The last attempt to connect failed (it is retried on the next operation).
    Broken,
}

impl ConnectionState {
    pub fn as_str(self) -> &'static str {
        match self {
            ConnectionState::Closed => "Closed",
            ConnectionState::Connecting => "Connecting",
            ConnectionState::Open => "Open",
            ConnectionState::Broken => "Broken",
        }
    }

    pub(crate) fn to_u8(self) -> u8 {
        self as u8
    }

    pub(crate) fn from_u8(v: u8) -> Self {
        match v {
            1 => ConnectionState::Connecting,
            2 => ConnectionState::Open,
            3 => ConnectionState::Broken,
            _ => ConnectionState::Closed,
        }
    }
}

/// `StateChange` args (ADO.NET `StateChangeEventArgs`).
#[derive(EventArgs, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StateChangeEventArgs {
    pub original_state: ConnectionState,
    pub current_state: ConnectionState,
}

/// How a binding source's list changed (WinForms `ListChangedType`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum ListChangedType {
    /// Much changed: reload everything (a fill, a filter, a sort, a save).
    #[default]
    Reset,
    ItemAdded,
    ItemDeleted,
    ItemChanged,
    ItemMoved,
}

/// `ListChanged` args (WinForms `ListChangedEventArgs`): indices are positions in the binding
/// source's view, `-1` when not applicable.
#[derive(EventArgs, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ListChangedEventArgs {
    pub list_changed_type: ListChangedType,
    pub new_index: i32,
    pub old_index: i32,
}

/// `AddingNew` args: `cancel = true` refuses the new row.
#[derive(EventArgs, Debug, Clone, Copy, Default, PartialEq, Eq)]
#[args(extends = CancelEventArgs, cancel)]
pub struct AddingNewEventArgs {
    pub cancel: bool,
}

/// `RowValidating` args: the row being committed (`values`, by column name); a handler adds field
/// errors with [`RowValidatingEventArgs::add_error`] (shown by an `ErrorProvider`) or sets `cancel`.
#[derive(EventArgs, Debug, Clone, Default, PartialEq)]
#[args(extends = CancelEventArgs, cancel)]
pub struct RowValidatingEventArgs {
    /// The row's position in the view.
    pub row_index: i32,
    pub values: Vec<(String, DbValue)>,
    /// `(column, message)`; an empty column is an error of the whole row.
    pub errors: Vec<(String, String)>,
    pub cancel: bool,
}

impl RowValidatingEventArgs {
    /// The value of `column` (case-insensitive).
    pub fn value(&self, column: &str) -> Option<&DbValue> {
        self.values.iter().find(|(n, _)| n.eq_ignore_ascii_case(column)).map(|(_, v)| v)
    }

    /// The value of `column` as text (`""` for NULL or a missing column).
    pub fn text(&self, column: &str) -> String {
        self.value(column).map(DbValue::to_display).unwrap_or_default()
    }

    /// Adds an error on `column` (or on the row when `column` is empty).
    pub fn add_error(&mut self, column: impl Into<String>, message: impl Into<String>) {
        self.errors.push((column.into(), message.into()));
    }
}

/// `DataError` args: an operation of the component failed (a fill, a save, a refused edit).
#[derive(EventArgs, Debug, Clone, Default, PartialEq, Eq)]
pub struct DataErrorEventArgs {
    pub message: String,
    /// The column concerned, empty when none.
    pub column: String,
}

/// Raises an event of a data component: first to its Rust subscribers (`event`, sender: the
/// component), then to the `.kbview` handler its element names — synchronously when the component
/// is being called through a binding or the view runtime (`kubuno_views::scope::raise_now`: a
/// cancelable event can be cancelled by its XML handler), else queued on the component for the
/// runtime to deliver (only for a component of a view, i.e. one with a site). Returns the args as
/// the handlers left them.
pub(crate) fn emit<A: EventArgs + Clone>(core: &mut ComponentCore, class: &'static str, event: &Event<A>, attr: &'static str, mut args: A) -> A {
    let name = core.site.as_ref().map(|s| s.name.clone()).unwrap_or_default();
    let sender = ElementRef { name: Some(&name), element: class, id: "", bounds: Default::default(), focus_id: None, attributes: &[] };
    event.raise(&sender, &mut args);
    if core.site.is_some() && !(!name.is_empty() && kubuno_views::scope::raise_now(&name, attr, &mut args)) {
        core.queue_event(attr, Box::new(args.clone()));
    }
    args
}

/// The name of a component (its site's, i.e. its `x:Name`), `""` when it has none.
pub(crate) fn name_of(core: &ComponentCore) -> &str {
    core.site.as_ref().map_or("", |s| s.name.as_str())
}

/// Names a component (its site), outside a view.
pub(crate) fn set_name_of(core: &mut ComponentCore, name: impl Into<String>) {
    core.site = Some(Site { name: name.into(), design_mode: false, container: None });
}
