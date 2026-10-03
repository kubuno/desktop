//! The typed control handles: one per Kubuno control, each a [`Control`] of that element with its
//! own builder methods, properties and events.

use std::ops::Deref;

use kubuno_views::binding::Value;
use kubuno_views::events::{CheckedChangedEventArgs, EventArgs, MouseEventArgs, NumericValueChangedEventArgs, SelectionChangedEventArgs, TextChangedEventArgs};

use super::{Anchor, AsControl, Control, ControlEvent, DialogResult, DockStyle, Form, SenderParam};

macro_rules! control_types {
    ($($(#[$doc:meta])* $name:ident;)*) => {$(
        $(#[$doc])*
        #[derive(Clone, PartialEq)]
        pub struct $name(Control);

        impl $name {
            /// The element this handle stands for.
            pub const ELEMENT: &'static str = stringify!($name);

            /// A new control, not yet in a form (add it with `form.controls().add(&control)`).
            pub fn new() -> Self {
                Self(Control::new(Self::ELEMENT))
            }

            /// Its name (`x:Name`); a control added without one gets `button1`, `label2`…
            pub fn name(self, name: &str) -> Self {
                self.0.set_name(name);
                self
            }

            /// Its text (`Text`).
            pub fn text(self, text: impl Into<String>) -> Self {
                self.0.set_text(text);
                self
            }

            /// Where it is in its container, in DIP.
            pub fn location(self, x: f32, y: f32) -> Self {
                self.0.set_location(x, y);
                self
            }

            /// Its size, in DIP.
            pub fn size(self, width: f32, height: f32) -> Self {
                self.0.set_size(width, height);
                self
            }

            /// Location and size.
            pub fn bounds(self, x: f32, y: f32, width: f32, height: f32) -> Self {
                self.0.set_bounds(x, y, width, height);
                self
            }

            /// The edges of its container it follows when the container is resized.
            pub fn anchor(self, anchor: Anchor) -> Self {
                self.0.set_anchor(anchor);
                self
            }

            /// The edge of its container it is docked to.
            pub fn dock(self, dock: DockStyle) -> Self {
                self.0.set_dock(dock);
                self
            }

            /// Whether it takes input.
            pub fn enabled(self, enabled: bool) -> Self {
                self.0.set_enabled(enabled);
                self
            }

            /// Whether it is shown.
            pub fn visible(self, visible: bool) -> Self {
                self.0.set_visible(visible);
                self
            }

            /// Its tooltip.
            pub fn tool_tip(self, text: impl Into<String>) -> Self {
                self.0.set_tool_tip(text);
                self
            }

            /// Its position in the Tab order.
            pub fn tab_index(self, index: u32) -> Self {
                self.0.set_property("TabIndex", Value::F32(index as f32));
                self
            }

            /// Any property of the element (`.property("Variant", "Primary")`).
            pub fn property(self, name: &str, value: impl Into<Value>) -> Self {
                self.0.set_property(name, value);
                self
            }

            /// Event `event` (its `.kbview` name, `"OnMouseDown"`) with this type as the sender.
            pub fn on<A: EventArgs + kubuno_views::events::ArgsChain>(&self, event: &'static str) -> ControlEvent<'_, Self, A> {
                self.0.on(event)
            }

            /// `Click`, with this control as the sender.
            pub fn click(&self) -> ControlEvent<'_, Self, MouseEventArgs> {
                self.0.on("OnClick")
            }

            /// `TextChanged`, with this control as the sender.
            pub fn text_changed(&self) -> ControlEvent<'_, Self, TextChangedEventArgs> {
                self.0.on("OnTextChanged")
            }

            /// This handle as a plain [`Control`].
            pub fn as_control(&self) -> &Control {
                &self.0
            }

            /// A handle of this type to `control`, when it is one (`None` for another element).
            pub fn from_control(control: &Control) -> Option<Self> {
                control.is(Self::ELEMENT).then(|| Self(control.clone()))
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl Deref for $name {
            type Target = Control;
            fn deref(&self) -> &Control {
                &self.0
            }
        }

        impl AsControl for $name {
            fn as_control(&self) -> &Control {
                &self.0
            }
        }

        impl From<$name> for Control {
            fn from(c: $name) -> Control {
                c.0
            }
        }

        impl std::fmt::Debug for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                self.0.fmt(f)
            }
        }

        impl SenderParam for $name {
            fn from_sender(control: &Control, _form: &Form, handler: &str) -> Option<Self> {
                let typed = Self::from_control(control);
                if typed.is_none() {
                    tracing::warn!(
                        handler,
                        "handler `{handler}` takes a `&{}` sender but the event was raised by a <{}> (`{}`): not called",
                        Self::ELEMENT,
                        control.element(),
                        control.get_name()
                    );
                }
                typed
            }
        }
    )*};
}

control_types! {
    /// An accordion of collapsible sections.
    Accordion;
    /// A small status label.
    Badge;
    /// A breadcrumb trail.
    Breadcrumb;
    /// A push button.
    Button;
    /// A highlighted message.
    Callout;
    /// A card container.
    Card;
    /// A check box.
    CheckBox;
    /// A list with a check box per item.
    CheckedListBox;
    /// A colour picker field.
    ColorField;
    /// A gradient picker field.
    GradientField;
    /// An editable drop-down list.
    ComboBox;
    /// A table bound to rows.
    DataTable;
    /// A date field with a calendar.
    DatePicker;
    /// A drop-down list.
    Dropdown;
    /// A placeholder for an empty list or page.
    EmptyState;
    /// A titled group of controls.
    GroupBox;
    /// A Kubuno vector icon.
    Icon;
    /// A button showing an icon.
    IconButton;
    /// A text label.
    Label;
    /// A hyperlink.
    LinkLabel;
    /// A list of items.
    ListBox;
    /// A list of items with columns or tiles.
    ListView;
    /// A text field with an input mask.
    MaskedField;
    /// A month calendar.
    MonthCalendar;
    /// A number field with up/down buttons.
    NumericField;
    /// A surface drawn by its `Paint` handler.
    PaintBox;
    /// A container where each control has its position (a form's surface).
    Panel;
    /// A progress bar.
    ProgressBar;
    /// A radio button.
    RadioButton;
    /// A scrollable container.
    ScrollArea;
    /// A search box.
    SearchField;
    /// A separator line.
    Separator;
    /// A slider.
    Slider;
    /// A busy indicator.
    Spinner;
    /// Two panes with a movable splitter.
    Splitter;
    /// A container laying its controls out in a row or a column.
    Stack;
    /// The steps of a process.
    Stepper;
    /// An on/off switch.
    Switch;
    /// Tab pages.
    Tabs;
    /// A multi-line text field.
    TextArea;
    /// A single-line text field.
    TextField;
    /// A tool bar.
    Toolbar;
    /// A tree of items.
    TreeView;
    /// A work area surrounded by dockable panels (`<DockArea>`).
    DockArea;
    /// The frame of an editor: top bar, status bar and body (`<WorkspaceShell>`).
    WorkspaceShell;
    /// A person's picture, or their initials on a colour (`<Avatar>`).
    Avatar;
    /// An image (`<PictureBox>`).
    PictureBox;
    /// A floating panel next to a control (`<Popover>`).
    Popover;
    /// A list whose items are views (`<Repeater>`).
    Repeater;
    /// A navigation pane (`<Sidebar>`).
    Sidebar;
    /// A status bar (`<StatusBar>`).
    StatusBar;
    /// A grid of rows and columns (`<TableLayoutPanel>`).
    TableLayoutPanel;
}

impl Button {
    /// Clicking the button closes its modal form with `result` (`DialogResult` of a WinForms button).
    pub fn dialog_result(self, result: DialogResult) -> Self {
        self.0.set_dialog_result(result);
        self
    }

    /// Its look: `"Primary"`, `"Secondary"`, `"Ghost"`, `"Danger"`…
    pub fn variant(self, variant: &str) -> Self {
        self.0.set_property("Variant", variant);
        self
    }
}

/// The icon of the controls that show one, with how it is drawn (`vskubuno/docs/ICONS.md`).
macro_rules! with_icon {
    ($($name:ident => $prop:literal),*) => {$(
        impl $name {
            /// Its icon: a name of the Kubuno icon set (`"Save"`), an image file
            /// ([`super::IconSource::file`]) or a project resource ([`super::IconSource::resource`]).
            pub fn icon(self, icon: impl Into<super::IconSource>) -> Self {
                self.set_icon(icon);
                self
            }

            /// Changes its icon.
            pub fn set_icon(&self, icon: impl Into<super::IconSource>) {
                self.0.set_property($prop, icon.into());
            }

            /// The size of its icon, in DIP (`IconSize`).
            pub fn icon_size(self, size: f32) -> Self {
                self.0.set_property("IconSize", size.to_string());
                self
            }

            /// The colour of its icon: a theme colour (`"Primary"`) or `#rrggbb` (`IconColor`).
            pub fn icon_color(self, color: &str) -> Self {
                self.0.set_property("IconColor", color);
                self
            }

            /// How a non-square image fills its icon's box (`IconScaling`).
            pub fn icon_scaling(self, scaling: super::IconScaling) -> Self {
                self.0.set_property("IconScaling", scaling.name());
                self
            }
        }
    )*};
}

with_icon!(Button => "Icon", IconButton => "Icon", EmptyState => "Icon", Icon => "Name");

impl Button {
    /// Where its icon goes relative to its text (`TextImageRelation`: `"ImageAboveText"`,
    /// `"ImageBeforeText"`…).
    pub fn text_image_relation(self, relation: &str) -> Self {
        self.0.set_property("TextImageRelation", relation);
        self
    }

    /// The space between its icon and its text, in DIP (`IconSpacing`).
    pub fn icon_spacing(self, spacing: f32) -> Self {
        self.0.set_property("IconSpacing", spacing);
        self
    }
}

macro_rules! checkable {
    ($($name:ident => $prop:literal),*) => {$(
        impl $name {
            /// Whether it is checked.
            pub fn checked(self, checked: bool) -> Self {
                self.0.set_property($prop, checked);
                self
            }

            pub fn is_checked(&self) -> bool {
                self.0.flag($prop, false)
            }

            pub fn set_checked(&self, checked: bool) {
                self.0.set_property($prop, checked);
            }

            /// `CheckedChanged`.
            pub fn checked_changed(&self) -> ControlEvent<'_, Self, CheckedChangedEventArgs> {
                self.0.on("OnCheckedChanged")
            }
        }
    )*};
}
checkable!(CheckBox => "Checked", Switch => "On");

macro_rules! ranged {
    ($($name:ident),*) => {$(
        impl $name {
            /// Its value.
            pub fn value(self, value: f32) -> Self {
                self.0.set_property("Value", value);
                self
            }

            /// The smallest value.
            pub fn minimum(self, minimum: f32) -> Self {
                self.0.set_property("Minimum", minimum);
                self
            }

            /// The largest value.
            pub fn maximum(self, maximum: f32) -> Self {
                self.0.set_property("Maximum", maximum);
                self
            }

            pub fn get_value(&self) -> f32 {
                self.0.number("Value").unwrap_or(0.0)
            }

            pub fn set_value(&self, value: f32) {
                self.0.set_property("Value", value);
            }

            pub fn get_minimum(&self) -> f32 {
                self.0.number("Minimum").unwrap_or(0.0)
            }

            pub fn set_minimum(&self, minimum: f32) {
                self.0.set_property("Minimum", minimum);
            }

            pub fn get_maximum(&self) -> f32 {
                self.0.number("Maximum").unwrap_or(100.0)
            }

            pub fn set_maximum(&self, maximum: f32) {
                self.0.set_property("Maximum", maximum);
            }

            /// `ValueChanged`.
            pub fn value_changed(&self) -> ControlEvent<'_, Self, NumericValueChangedEventArgs> {
                self.0.on("OnValueChanged")
            }
        }
    )*};
}
ranged!(Slider, NumericField, ProgressBar);

macro_rules! text_input {
    ($($name:ident),*) => {$(
        impl $name {
            /// The hint shown while it is empty.
            pub fn placeholder(self, text: impl Into<String>) -> Self {
                self.0.set_property("Placeholder", Value::Str(text.into()));
                self
            }

            pub fn set_placeholder(&self, text: impl Into<String>) {
                self.0.set_property("Placeholder", Value::Str(text.into()));
            }

            /// Whether the text can be selected but not changed.
            pub fn read_only(self, read_only: bool) -> Self {
                self.0.set_property("ReadOnly", read_only);
                self
            }

            pub fn is_read_only(&self) -> bool {
                self.0.flag("ReadOnly", false)
            }

            pub fn set_read_only(&self, read_only: bool) {
                self.0.set_property("ReadOnly", read_only);
            }
        }
    )*};
}
text_input!(TextField, TextArea, SearchField, MaskedField);

macro_rules! indexed {
    ($($name:ident),*) => {$(
        impl $name {
            /// The selected item's index.
            pub fn selected_index(self, index: usize) -> Self {
                self.0.set_property("SelectedIndex", index);
                self
            }

            /// The selected item's index, `None` when nothing is selected.
            pub fn get_selected_index(&self) -> Option<usize> {
                self.0.number("SelectedIndex").filter(|i| *i >= 0.0).map(|i| i as usize)
            }

            pub fn set_selected_index(&self, index: Option<usize>) {
                self.0.set_property("SelectedIndex", index.map_or(-1.0, |i| i as f32));
            }

            /// `SelectionChanged`.
            pub fn selection_changed(&self) -> ControlEvent<'_, Self, SelectionChangedEventArgs> {
                self.0.on("OnSelectionChanged")
            }
        }
    )*};
}
indexed!(ListBox, Tabs);

macro_rules! valued {
    ($($name:ident),*) => {$(
        impl $name {
            /// The selected value.
            pub fn selected_value(self, value: impl Into<String>) -> Self {
                self.0.set_property("SelectedValue", Value::Str(value.into()));
                self
            }

            pub fn get_selected_value(&self) -> String {
                self.0.string("SelectedValue")
            }

            pub fn set_selected_value(&self, value: impl Into<String>) {
                self.0.set_property("SelectedValue", Value::Str(value.into()));
            }

            /// `SelectedValueChanged`.
            pub fn selected_value_changed(&self) -> ControlEvent<'_, Self, TextChangedEventArgs> {
                self.0.on("OnSelectedValueChanged")
            }
        }
    )*};
}
valued!(ComboBox, Dropdown);

macro_rules! with_items {
    ($($name:ident),*) => {$(
        impl $name {
            /// Its items (`<Item Text="…"/>` children).
            pub fn items<S: AsRef<str>>(self, items: impl IntoIterator<Item = S>) -> Self {
                for text in items {
                    let item = Control::new("Item");
                    item.set_text(text.as_ref());
                    self.0.controls().add(&item);
                }
                self
            }

            /// Adds an item.
            pub fn add_item(&self, text: &str) {
                let item = Control::new("Item");
                item.set_text(text);
                self.0.controls().add(&item);
            }
        }
    )*};
}
with_items!(ListBox, ComboBox, Dropdown, CheckedListBox);

impl DockArea {
    /// The current layout as JSON (the web's `DockLayout` shape): what the user arranged.
    pub fn save_layout(&self) -> String {
        self.0.string("Layout")
    }

    /// Restores a layout saved by [`Self::save_layout`]; an empty string restores the default
    /// arrangement. A text that is not a layout is ignored (the panels stay as they are).
    pub fn load_layout(&self, json: &str) {
        if json.trim().is_empty() || kubuno_ui::dock::DockLayout::from_json(json).is_some() {
            self.0.set_property("Layout", Value::Str(json.to_string()));
        }
    }

    /// Back to the arrangement the view declares (`controller.reset`).
    pub fn reset(&self) {
        self.0.set_property("Layout", Value::Str(String::new()));
    }

    /// Opens panel `name` (its `x:Name`) and brings it to the front: re-docked on the right if it
    /// was closed, surfaced (and unrolled) if it is already on screen (`controller.open`).
    pub fn open(&self, name: &str) {
        // Cleared first so that opening the panel that is already the active one still applies.
        self.0.set_property("ActivePanel", Value::Str(String::new()));
        self.0.set_property("ActivePanel", Value::Str(name.to_string()));
    }

    /// Closes panel `name`; the user can reopen it (`controller.close`).
    pub fn close(&self, name: &str) {
        let Some(layout) = kubuno_ui::dock::DockLayout::from_json(&self.save_layout()) else { return };
        let closed = kubuno_ui::dock::close_panel(&layout, name);
        self.0.set_property("Layout", Value::Str(closed.to_json()));
    }

    /// The panel shown in front (its `x:Name`), as last activated.
    pub fn active_panel(&self) -> String {
        self.0.string("ActivePanel")
    }

    /// `PanelActivated`: the user brought a panel to the front (`e.new` is its name).
    pub fn panel_activated(&self) -> ControlEvent<'_, Self, TextChangedEventArgs> {
        self.0.on("OnPanelActivated")
    }

    /// `PanelClosed`: the user closed a panel (`e.new` is its name).
    pub fn panel_closed(&self) -> ControlEvent<'_, Self, TextChangedEventArgs> {
        self.0.on("OnPanelClosed")
    }

    /// `LayoutChanged`: the panels were moved, resized, closed or reopened (`e.new` is the layout).
    pub fn layout_changed(&self) -> ControlEvent<'_, Self, TextChangedEventArgs> {
        self.0.on("OnLayoutChanged")
    }
}

impl WorkspaceShell {
    /// `Back`: the back arrow was clicked.
    pub fn back(&self) -> ControlEvent<'_, Self, kubuno_views::events::EmptyEventArgs> {
        self.0.on("OnBack")
    }

    /// `Delete`: the delete button was clicked (confirm with a `MessageBox` before deleting).
    pub fn delete(&self) -> ControlEvent<'_, Self, kubuno_views::events::EmptyEventArgs> {
        self.0.on("OnDelete")
    }

    /// `Search`: the search button was clicked.
    pub fn search(&self) -> ControlEvent<'_, Self, kubuno_views::events::EmptyEventArgs> {
        self.0.on("OnSearch")
    }
}

impl Repeater {
    /// Its rows (`ItemsSource`): one item each, shown by its template.
    pub fn items_source(self, rows: impl Into<kubuno_views::binding::Rows>) -> Self {
        self.0.set_property("ItemsSource", Value::List(rows.into()));
        self
    }

    /// Replaces its rows. Keep a [`kubuno_views::binding::Rows`] and hand the same one back after
    /// changing it: an unchanged list costs nothing per frame.
    pub fn set_items(&self, rows: impl Into<kubuno_views::binding::Rows>) {
        self.0.set_property("ItemsSource", Value::List(rows.into()));
    }

    /// Its rows, as last set.
    pub fn get_items(&self) -> kubuno_views::binding::Rows {
        match self.0.get_property("ItemsSource") {
            Some(Value::List(rows)) => rows,
            _ => kubuno_views::binding::Rows::new(),
        }
    }

    /// The selected item's index (`SelectionMode="Single"`), `None` for none.
    pub fn selected_index(&self) -> Option<usize> {
        self.0.number("SelectedIndex").filter(|i| *i >= 0.0).map(|i| i.round() as usize)
    }

    pub fn set_selected_index(&self, index: Option<usize>) {
        self.0.set_property("SelectedIndex", Value::F32(index.map_or(-1.0, |i| i as f32)));
    }

    /// The item being painted or handled (inside a handler of its template, or of `ItemClick`):
    /// its index, key and row.
    pub fn current_item() -> Option<kubuno_views::binding::ItemContext> {
        kubuno_views::binding::current_item()
    }

    /// `ItemClick`: an item was clicked (`e.index`).
    pub fn item_click(&self) -> ControlEvent<'_, Self, kubuno_views::events::ItemEventArgs> {
        self.0.on("OnItemClick")
    }

    /// `SelectionChanged`.
    pub fn selection_changed(&self) -> ControlEvent<'_, Self, SelectionChangedEventArgs> {
        self.0.on("OnSelectionChanged")
    }
}

impl Sidebar {
    /// The key of the active row (`SelectedItem`).
    pub fn selected_item(&self) -> String {
        self.0.string("SelectedItem")
    }

    /// Makes the row of key `key` active.
    pub fn set_selected_item(&self, key: &str) {
        self.0.set_property("SelectedItem", Value::Str(key.to_string()));
    }

    /// The icon rail (`DisplayMode="Compact"`) or the full pane.
    pub fn set_compact(&self, compact: bool) {
        self.0.set_property("DisplayMode", Value::Str(if compact { "Compact" } else { "Expanded" }.to_string()));
    }

    /// Rows from a list (`ItemsSource`: fields `Text`, `Icon`, `Key`, `Level`, `Kind`).
    pub fn set_items(&self, rows: impl Into<kubuno_views::binding::Rows>) {
        self.0.set_property("ItemsSource", Value::List(rows.into()));
    }

    /// `ItemInvoked`: a row was chosen (`e.new` is its key).
    pub fn item_invoked(&self) -> ControlEvent<'_, Self, TextChangedEventArgs> {
        self.0.on("OnItemInvoked")
    }

    /// `SelectionChanged`: the active row changed (`e.new` is its key).
    pub fn selection_changed(&self) -> ControlEvent<'_, Self, TextChangedEventArgs> {
        self.0.on("OnSelectionChanged")
    }
}

impl StatusBar {
    /// `ItemClicked`: a clickable cell was clicked (`e.index`).
    pub fn item_clicked(&self) -> ControlEvent<'_, Self, kubuno_views::events::ItemEventArgs> {
        self.0.on("OnItemClicked")
    }
}

impl Avatar {
    /// The person's name (initials and colour).
    pub fn set_display_name(&self, name: &str) {
        self.0.set_property("DisplayName", Value::Str(name.to_string()));
    }

    /// The picture, from a file (relative to the view).
    pub fn set_image(&self, path: &str) {
        self.0.set_property("Image", Value::Str(path.to_string()));
    }

    /// The picture, from the bytes of an image file (PNG, JPEG…); empty for none.
    pub fn set_image_data(&self, bytes: Vec<u8>) {
        self.0.set_property("ImageData", Value::Object(kubuno_views::binding::ObjectValue::new(bytes)));
    }

    /// The presence dot: `"None"`, `"Online"`, `"Away"`, `"Busy"` or `"Offline"`.
    pub fn set_presence(&self, presence: &str) {
        self.0.set_property("Presence", Value::Str(presence.to_string()));
    }
}

impl PictureBox {
    /// The image, from a file (relative to the view).
    pub fn set_image(&self, path: &str) {
        self.0.set_property("Image", Value::Str(path.to_string()));
    }

    /// The image, from the bytes of an image file (PNG, JPEG…).
    pub fn set_image_data(&self, bytes: Vec<u8>) {
        self.0.set_property("ImageData", Value::Object(kubuno_views::binding::ObjectValue::new(bytes)));
    }

    /// How the image fits: `"Normal"`, `"Stretch"`, `"Zoom"`, `"Center"` or `"Cover"`.
    pub fn set_size_mode(&self, mode: &str) {
        self.0.set_property("SizeMode", Value::Str(mode.to_string()));
    }
}

impl Popover {
    /// Opens it next to its target (WinForms-like `Show()`).
    pub fn show(&self) {
        self.0.set_property("IsOpen", Value::Bool(true));
    }

    /// Closes it.
    pub fn hide(&self) {
        self.0.set_property("IsOpen", Value::Bool(false));
    }

    /// Whether it is open (false once the user dismissed it).
    pub fn is_open(&self) -> bool {
        self.0.flag("IsOpen", false)
    }

    /// `Opened`.
    pub fn opened(&self) -> ControlEvent<'_, Self, kubuno_views::events::EmptyEventArgs> {
        self.0.on("OnOpened")
    }

    /// `Closed`: it closed (the user clicked outside it, pressed Escape, or code hid it).
    pub fn closed(&self) -> ControlEvent<'_, Self, kubuno_views::events::EmptyEventArgs> {
        self.0.on("OnClosed")
    }
}

/// A custom control of a view, typed with its class: what `#[kubuno::view]` gives a
/// `#[control]` field (`#[control] thread: Custom<MessageThread>`), the Rust twin of the
/// `private MessageThread thread;` of a `Form1.Designer.cs`. It is a [`Control`] (deref) whose
/// class instance is reached with [`Custom::with`] — `self.thread.with(|t| t.append(message))`.
/// Works for the controls of the project and those of a library it depends on.
pub struct Custom<T> {
    control: Control,
    _class: std::marker::PhantomData<fn() -> T>,
}

impl<T> Custom<T> {
    /// A handle of this type to `control` (no check of its class: [`Custom::with`] answers `None`
    /// for another class).
    pub fn from_control(control: &Control) -> Self {
        Self { control: control.clone(), _class: std::marker::PhantomData }
    }

    /// This handle as a plain [`Control`].
    pub fn as_control(&self) -> &Control {
        &self.control
    }
}

/// Code-first: a custom control or user control of the application created in code, then added to
/// a form like any control — `form.controls().add(&Custom::<AddressEditor>::new().location(16.0, 16.0))`.
impl<T: kubuno_views::registry::Registered + 'static> Custom<T> {
    /// A new control of class `T`, not yet in a form (WinForms `new AddressEditor()`).
    pub fn new() -> Self {
        Self::from_control(&Control::new(T::registration().name))
    }

    /// A new control of class `T` whose declared properties (`#[property]` fields) take the values
    /// `instance` has — Windows Forms' object initializer, `new AddressEditor { Street = "…" }`:
    /// `Custom::from_instance(AddressEditor { street: "…".into(), ..Default::default() })`. Only the
    /// values that differ from `T::default()` are written; the control is created from them when
    /// its form shows (the instance itself is not kept).
    pub fn from_instance(instance: T) -> Self
    where
        T: kubuno_views::component::Component + Default,
    {
        let control = Self::new();
        let pristine = T::default();
        for p in T::registration().properties {
            let Some(value) = instance.kubuno_get_property(p.name) else { continue };
            if pristine.kubuno_get_property(p.name).as_ref() != Some(&value) {
                control.control.set_property(p.name, value);
            }
        }
        control
    }

    /// A new control of class `T` whose declared properties are those `init` gives a default
    /// instance — the object initializer without the struct update syntax (the class's `base`
    /// field is private to its module): `Custom::<AddressEditor>::init(|e| e.street = "…".into())`.
    pub fn init(init: impl FnOnce(&mut T)) -> Self
    where
        T: kubuno_views::component::Component + Default,
    {
        let mut instance = T::default();
        init(&mut instance);
        Self::from_instance(instance)
    }

    /// Its name (`x:Name`); a control added without one gets a generated name.
    pub fn name(self, name: &str) -> Self {
        self.control.set_name(name);
        self
    }

    /// Where it is in its container, in DIP.
    pub fn location(self, x: f32, y: f32) -> Self {
        self.control.set_location(x, y);
        self
    }

    /// Its size, in DIP.
    pub fn size(self, width: f32, height: f32) -> Self {
        self.control.set_size(width, height);
        self
    }

    /// Location and size.
    pub fn bounds(self, x: f32, y: f32, width: f32, height: f32) -> Self {
        self.control.set_bounds(x, y, width, height);
        self
    }

    /// The edges of its container it follows when the container is resized.
    pub fn anchor(self, anchor: Anchor) -> Self {
        self.control.set_anchor(anchor);
        self
    }

    /// The edge of its container it is docked to.
    pub fn dock(self, dock: DockStyle) -> Self {
        self.control.set_dock(dock);
        self
    }

    /// One of its properties, by its XML name (`.property("Street", "12 rue de la Paix")`).
    pub fn property(self, name: &str, value: impl Into<Value>) -> Self {
        self.control.set_property(name, value);
        self
    }

    /// One of its events, by its XML name (`"OnAddressValidated"`), with this handle as the sender:
    /// `editor.on::<AddressValidatedEventArgs>("OnAddressValidated").subscribe(|sender, e| …)`.
    pub fn on<A: EventArgs + kubuno_views::events::ArgsChain>(&self, event: &'static str) -> ControlEvent<'_, Self, A> {
        self.control.on(event)
    }
}

impl<T: kubuno_views::component::Component> Custom<T> {
    /// Runs `f` on the control's instance (see [`Control::with`]).
    pub fn with<R>(&self, f: impl FnOnce(&mut T) -> R) -> Option<R> {
        self.control.with::<T, R>(f)
    }

    /// Runs `f` on the control's instance, reading only.
    pub fn with_ref<R>(&self, f: impl FnOnce(&T) -> R) -> Option<R> {
        self.control.with_ref::<T, R>(f)
    }
}

impl<T> Clone for Custom<T> {
    fn clone(&self) -> Self {
        Self::from_control(&self.control)
    }
}

impl<T> Default for Custom<T> {
    /// Not linked yet: `initialize_component` links it to its element.
    fn default() -> Self {
        Self::from_control(&Control::default())
    }
}

impl<T> PartialEq for Custom<T> {
    fn eq(&self, other: &Self) -> bool {
        self.control == other.control
    }
}

impl<T> Deref for Custom<T> {
    type Target = Control;
    fn deref(&self) -> &Control {
        &self.control
    }
}

impl<T> AsControl for Custom<T> {
    fn as_control(&self) -> &Control {
        &self.control
    }
}

impl<T> std::fmt::Debug for Custom<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.control.fmt(f)
    }
}

impl<T: kubuno_views::registry::Registered + 'static> SenderParam for Custom<T> {
    fn from_sender(control: &Control, _form: &Form, handler: &str) -> Option<Self> {
        let class = T::registration().name;
        if control.is(class) {
            return Some(Self::from_control(control));
        }
        tracing::warn!(handler, "handler `{handler}` takes a `&Custom<{class}>` sender but the event was raised by a <{}> (`{}`): not called", control.element(), control.get_name());
        None
    }
}
