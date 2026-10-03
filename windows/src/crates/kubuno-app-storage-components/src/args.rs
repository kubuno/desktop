//! The args of the storage components' events, and how they are raised.

use kubuno_views::component::ComponentCore;
use kubuno_views::events::{CancelEventArgs, ElementRef, Event, EventArgs};

/// `SettingChanged` args (WinForms `PropertyChangedEventArgs` of `ApplicationSettingsBase`): which setting changed,
/// and whether the change came from outside this window (another instance, an administrator, a reload).
#[derive(EventArgs, Debug, Clone, Default, PartialEq, Eq)]
pub struct SettingChangedEventArgs {
    /// The setting's name (`Theme`).
    pub setting_name: String,
    /// Written by another process (or reloaded), not by this window.
    pub external: bool,
}

/// `SettingsSaving` args (WinForms `SettingsSaving`): `cancel` keeps the changes pending.
#[derive(EventArgs, Debug, Clone, Default, PartialEq, Eq)]
#[args(extends = CancelEventArgs, cancel)]
pub struct SettingsSavingEventArgs {
    pub cancel: bool,
}

/// `ValueChanged` args of a `RegistryKey`: a value was written through a binding, or the key changed outside.
#[derive(EventArgs, Debug, Clone, Default, PartialEq, Eq)]
pub struct RegistryValueChangedEventArgs {
    /// The value's name; empty when the key changed outside (any value may have).
    pub value_name: String,
    pub external: bool,
}

/// Raises an event of a component: its Rust subscribers first, then the `.kbview` handler its element names —
/// synchronously inside a binding or the runtime's sync (a cancelable event can be cancelled by its XML handler),
/// else queued for the runtime. Returns the args as the handlers left them.
pub(crate) fn emit<A: EventArgs + Clone>(core: &mut ComponentCore, class: &'static str, event: &Event<A>, attr: &'static str, mut args: A) -> A {
    let name = core.site.as_ref().map(|s| s.name.clone()).unwrap_or_default();
    let sender = ElementRef { name: Some(&name), element: class, id: "", bounds: Default::default(), focus_id: None, attributes: &[] };
    event.raise(&sender, &mut args);
    if core.site.is_some() && !(!name.is_empty() && kubuno_views::scope::raise_now(&name, attr, &mut args)) {
        core.queue_event(attr, Box::new(args.clone()));
    }
    args
}
