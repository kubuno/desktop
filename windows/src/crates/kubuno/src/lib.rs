//! # `kubuno` — Kubuno desktop applications, written like Windows Forms
//!
//! The one dependency of a Kubuno desktop application. It re-exports the layers underneath
//! ([`views`], [`controls`], [`ui`], [`events`], and [`data`] with the `data` feature) and adds the
//! programming model of Windows Forms on top of them (`vskubuno/docs/PROGRAMMING-MODEL.md`):
//!
//! | Windows Forms | Kubuno |
//! |---|---|
//! | `Application.Run(new Form1())` | `kubuno::Application::run(MainView::new())` |
//! | `Form1.Designer.cs` (generated, never edited) | `#[kubuno::view("main_view.kbview")]` (generated at compile time, no file) |
//! | `InitializeComponent()` | `self.initialize_component()` |
//! | `this.button1.Enabled = false` | `self.hello.set_enabled(false)` |
//! | `private void button1_Click(object sender, EventArgs e)` | `fn hello_click(&mut self, sender: &Button, e: &MouseEventArgs)` |
//! | `new Button { Text = "OK", Location = …, Anchor = … }` | `Button::new().text("OK").location(10.0, 10.0).anchor(Anchor::TOP \| Anchor::RIGHT)` |
//! | `button1.Click += (s, e) => …` | `ok.click().subscribe(\|sender, e\| …)` |
//! | `this.Controls.Add(ok)` | `self.controls().add(&ok)` |
//! | `form.Show()` / `form.ShowDialog(this)` | `form.show()` / `form.show_dialog(self)` → [`DialogResult`] |
//! | `MessageBox.Show("…")` | `kubuno::MessageBox::show("…")` |
//!
//! A form designed in the Visual Studio designer is a `.kbview` file and a struct:
//!
//! ```no_run
//! use kubuno::prelude::*;
//!
//! #[kubuno::view(xml = r#"
//!   <Panel DesignWidth="400" DesignHeight="120" Title="Hello" OnLoad="main_view_load">
//!     <TextField x:Name="status" X="16" Y="16" Width="260" Height="36" Anchor="Top, Left, Right"/>
//!     <Button x:Name="hello" Text="Say hello" OnClick="hello_click" X="288" Y="16" Width="96" Height="36" Anchor="Top, Right"/>
//!   </Panel>"#)]
//! pub struct MainView {
//!     clicks: u32,
//! }
//!
//! impl MainView {
//!     pub fn new() -> Self {
//!         let mut view = Self::default();
//!         view.initialize_component();
//!         view
//!     }
//!
//!     fn main_view_load(&mut self, sender: &Form, e: &EventArgs) {
//!         self.status.set_text("Ready.");
//!     }
//!
//!     fn hello_click(&mut self, sender: &Button, e: &MouseEventArgs) {
//!         self.clicks += 1;
//!         self.status.set_text(format!("Hello! ({} clicks)", self.clicks));
//!     }
//! }
//!
//! fn main() -> kubuno::Result {
//!     kubuno::Application::run(MainView::new())
//! }
//! ```
//!
//! A form can also be built entirely in code, and both mix freely:
//!
//! ```no_run
//! use kubuno::prelude::*;
//!
//! fn main() -> kubuno::Result {
//!     let form = Form::new().text("Code first").client_size(360.0, 140.0);
//!     let name = TextField::new().location(16.0, 16.0).size(328.0, 36.0).anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT);
//!     let ok = Button::new().text("OK").location(264.0, 84.0).size(80.0, 36.0).anchor(Anchor::BOTTOM | Anchor::RIGHT);
//!     let field = name.clone();
//!     ok.click().subscribe(move |_sender, _e| {
//!         MessageBox::show(&format!("Hello, {}!", field.get_text()));
//!     });
//!     form.controls().add(&name);
//!     form.controls().add(&ok);
//!     kubuno::Application::run(form)
//! }
//! ```


pub mod application;
pub mod forms;
mod message_box;
pub mod popup;
pub mod printing;
pub mod storage;
mod view;

#[doc(hidden)]
pub mod __private;

pub use application::{Application, MdiLayout};
pub use forms::{
    Anchor, Button, CheckBox, ComboBox, Control, ControlCollection, DialogResult, DockStyle, Dropdown, Form, GroupBox, Label, ListBox, NumericField, Panel, ProgressBar,
    RadioButton, Slider, Switch, TextArea, TextField,
};
/// The icon of a control, typed (`Button::new().icon("Save")`, `.icon(IconSource::file("save.svg"))`).
pub use forms::{IconScaling, IconSource};
pub use message_box::{MessageBox, MessageBoxButtons, MessageBoxIcon};
pub use view::View;
/// The splash screen: `kubuno::SplashScreen::new().artwork(..).product(..).version(..).show()`,
/// then `splash.set_status(..)`, `splash.set_progress(..)`, `splash.close_when(&main_form)`.
pub mod splash;
pub use splash::{Artwork, Splash, SplashScreen};

/// `#[kubuno::view("main_view.kbview")]` on a struct: the struct becomes the form of that view —
/// see [`View`] for what it generates.
pub use kubuno_views_macros::view;

/// The views layer: the `.kbview` syntax tree, registry, bindings, runtime and control hierarchy.
pub use kubuno_views as views;
/// The native host (the window, its message loop, input, painting) and the WinForms control replicas.
pub use kubuno_controls as controls;
/// Kubuno's design system: widgets, themes, `Graphics`.
pub use kubuno_ui as ui;
/// The typed event system: the args types, `Event<A>`, `UiHandle`, `UiDispatcher`, `Timer`…
pub use kubuno_views::events;
/// The printing stack's component classes (`kubuno::printing` has the handles a form uses).
pub use kubuno_print as print;
/// Resources (`.kbres` files, vskubuno docs/RESOURCES.md): `kubuno::resources!("resources.kbres")` generates the
/// strongly typed class; `kubuno::resources::set_culture("fr")` switches the UI culture live.
pub use kubuno_resources as resources;
#[doc(inline)]
pub use kubuno_resources::resources;

/// The typed settings class of a `.kbsettings` file (vskubuno docs/STORAGE-COMPONENTS.md), Windows Forms'
/// `Properties.Settings`: `kubuno::settings!("settings.kbsettings")` generates `pub struct Settings` with one
/// accessor per setting (`Settings::theme()`, `Settings::set_theme("Dark")`), `save`, `reload`, `on_changed`,
/// and `store()` (the shared `kubuno::storage::engine::Settings`). Optionally a visibility and a type name:
/// `kubuno::settings!(pub(crate) WindowSettings, "window.kbsettings")`.
#[macro_export]
macro_rules! settings {
    ($($input:tt)*) => {
        $crate::__settings_impl! { krate = $crate::storage::engine; $($input)* }
    };
}

#[doc(hidden)]
pub use kubuno_resources_macros::settings as __settings_impl;
/// Data access (connections, commands, table adapters, binding sources) — the `data` feature.
#[cfg(feature = "data")]
pub use kubuno_data as data;
/// Logging (`kubuno::tracing::info!(…)`): under the debugger its lines go to Visual Studio's Output
/// window, otherwise to `%LOCALAPPDATA%\Kubuno\logs\<exe>.log`.
pub use tracing;

/// A value of a binding (`{Binding …}`) or of a control property.
pub use kubuno_views::binding::{ObjectValue, Row, Rows, Value};
/// A custom control property of any Rust type (`#[property] messages: Shared<Vec<Message>>`).
pub use kubuno_views::component::Shared;

/// `Result<T, kubuno::Error>`; `fn main() -> kubuno::Result` returns it.
pub type Result<T = ()> = std::result::Result<T, Error>;

/// What can go wrong running an application: the window could not be created.
#[derive(Debug)]
pub struct Error {
    message: String,
}

impl Error {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self { message: message.into() }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Error {}

/// A Rust value a `#[bind]` field of a view holds: converted to and from the binding [`Value`].
pub trait Bindable: Sized {
    fn to_value(&self) -> Value;
    /// `None` when `value` does not fit (the field keeps its value).
    fn from_value(value: &Value) -> Option<Self>;
}

impl Bindable for String {
    fn to_value(&self) -> Value {
        Value::Str(self.clone())
    }
    fn from_value(value: &Value) -> Option<Self> {
        match value {
            Value::Str(s) => Some(s.clone()),
            Value::F32(f) => Some(f.to_string()),
            Value::Bool(b) => Some(b.to_string()),
            Value::List(_) | Value::Object(_) => None,
        }
    }
}

impl Bindable for bool {
    fn to_value(&self) -> Value {
        Value::Bool(*self)
    }
    fn from_value(value: &Value) -> Option<Self> {
        match value {
            Value::Bool(b) => Some(*b),
            Value::Str(s) => s.trim().parse().ok(),
            _ => None,
        }
    }
}

macro_rules! bindable_number {
    ($($t:ty),*) => {$(
        impl Bindable for $t {
            fn to_value(&self) -> Value {
                Value::F32(*self as f32)
            }
            fn from_value(value: &Value) -> Option<Self> {
                match value {
                    Value::F32(f) => Some(*f as $t),
                    Value::Str(s) => s.trim().parse().ok(),
                    _ => None,
                }
            }
        }
    )*};
}
bindable_number!(f32, f64, i32, i64, u32, u64, usize);

impl Bindable for Vec<kubuno_views::binding::Row> {
    /// A new snapshot at every read: prefer a [`Rows`] field for a long list (see its doc).
    fn to_value(&self) -> Value {
        Value::from(self.clone())
    }
    fn from_value(value: &Value) -> Option<Self> {
        match value {
            Value::List(rows) => Some(rows.to_vec()),
            _ => None,
        }
    }
}

/// A list field (`#[bind] items: Rows`): handed to the bindings as the same snapshot until it
/// changes, so long lists cost nothing per frame.
impl Bindable for Rows {
    fn to_value(&self) -> Value {
        Value::List(self.clone())
    }
    fn from_value(value: &Value) -> Option<Self> {
        match value {
            Value::List(rows) => Some(rows.clone()),
            _ => None,
        }
    }
}

/// Any Rust value handed to a custom control's `Shared<T>` property.
impl<T: std::any::Any + Send + Sync> Bindable for Shared<T> {
    fn to_value(&self) -> Value {
        Value::from(self.clone())
    }
    fn from_value(value: &Value) -> Option<Self> {
        <Shared<T> as kubuno_views::component::PropertyValue>::from_value(value)
    }
}

/// What a form's code uses, in one import: `use kubuno::prelude::*;`.
pub mod prelude {
    pub use crate::forms::{
        Accordion, Anchor, AsControl, AsForm, Badge, Breadcrumb, Button, Callout, Card, CheckBox, CheckedListBox, ColorField, ComboBox, GradientField, Control, ControlCollection, Custom, DataTable,
        DatePicker, DialogResult, DockStyle, Dropdown, EmptyState, Form, FormBorderStyle, GroupBox, Icon, IconButton, Label, LinkLabel, ListBox, ListView, MaskedField,
        MonthCalendar, NumericField, PaintBox, Panel, ProgressBar, RadioButton, ScrollArea, SearchField, Separator, Slider, Spinner, Splitter, Stack, StartPosition, Stepper,
        Switch, Tabs, TextArea, TextField, Toolbar, TreeView, DockArea, WorkspaceShell, Avatar, PictureBox, Popover, Repeater, Sidebar, StatusBar, TableLayoutPanel,
    };
    pub use crate::forms::{IconScaling, IconSource};
    /// The ribbon family (`vskubuno/docs/RIBBON.md`).
    pub use crate::forms::{
        BackstageButton, BackstageSeparator, BackstageTab, Command, Ribbon, RibbonBackstage, RibbonBox, RibbonButton, RibbonCheckBox, RibbonColorPicker, RibbonComboBox,
        RibbonContextualTabGroup, RibbonControlGroup, RibbonGallery, RibbonGalleryCategory, RibbonGalleryItem, RibbonGroup, RibbonLabel, RibbonMenuButton, RibbonMenuItem,
        RibbonNumericField, RibbonQuickAccessToolbar, RibbonRadioButton, RibbonSeparator, RibbonSplitButton, RibbonSplitMenuItem, RibbonTab, RibbonTextBox, RibbonToggleButton,
    };
    pub use crate::printing::{PageSetupDialog, PrintDialog, PrintDocument, PrintEventArgs, PrintPageEventArgs, PrintPreviewControl, PrintPreviewDialog, QueryPageSettingsEventArgs};
    /// The args of the storage components' events (the components themselves are `kubuno::storage::…`: their
    /// names would clash with the typed class `settings!` generates).
    pub use crate::storage::{RegistryValueChangedEventArgs, SettingChangedEventArgs, SettingsSavingEventArgs};
    pub use crate::forms::{Backdrop, CaptionButtonStyle, CaptionCommand, CornerPreference, HeaderItems, SizeGripStyle, TitleAlignment, WindowKind};
    pub use crate::{Application, Bindable, MdiLayout, MessageBox, MessageBoxButtons, MessageBoxIcon, Row, Rows, Shared, Value, View};
    /// The root of the args types, under its Windows Forms name: `e: &EventArgs` takes any event.
    pub use kubuno_views::events::EmptyEventArgs as EventArgs;
    pub use kubuno_views::events::{
        CancelEventArgs, CheckedChangedEventArgs, DragDropEffects, DragEventArgs, FormClosedEventArgs, FormClosingEventArgs, HandledEventArgs, ItemActivateEventArgs,
        ItemEventArgs, KeyEventArgs, KeyPressEventArgs, MouseButton, MouseEventArgs, NumericValueChangedEventArgs, PaintEventArgs, ScrollEventArgs,
        SelectionChangedEventArgs, TextChangedEventArgs, ValueChangedEventArgs,
    };
    pub use kubuno_views::events::{CaptionButtonEventArgs, DpiChangedEventArgs};
    pub use kubuno_views::events::{delay, spawn_local, Timer, UiDispatcher, UiHandle};
}
