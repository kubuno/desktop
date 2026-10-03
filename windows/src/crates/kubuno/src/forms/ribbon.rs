//! The typed handles of the ribbon family (`vskubuno/docs/RIBBON.md` §3): one per element, each a
//! [`Control`] with its builder methods — the code-first way to build or change a ribbon
//! (`RibbonButton::new().label("Coller").large_icon("ClipboardPaste").large().on_click(|_, _| …)`),
//! and the type of the fields `#[kubuno::view]` generates for its `x:Name`d elements
//! (`self.bold.set_checked(true)`).

use std::ops::Deref;

use kubuno_views::binding::Value;
use kubuno_views::events::{CheckedChangedEventArgs, EmptyEventArgs, MouseEventArgs, NumericValueChangedEventArgs, TextChangedEventArgs};

use super::{AsControl, Control, ControlEvent, Form, SenderParam};

macro_rules! ribbon_types {
    ($($(#[$doc:meta])* $name:ident;)*) => {$(
        $(#[$doc])*
        #[derive(Clone, PartialEq)]
        pub struct $name(Control);

        impl $name {
            /// The element this handle stands for.
            pub const ELEMENT: &'static str = stringify!($name);

            /// A new element, not yet in a ribbon (add it with `parent.controls().add(&element)`).
            pub fn new() -> Self {
                Self(Control::new(Self::ELEMENT))
            }

            /// Its name (`x:Name`).
            pub fn name(self, name: &str) -> Self {
                self.0.set_name(name);
                self
            }

            /// Its label (a tab's or a group's header).
            pub fn label(self, text: impl Into<String>) -> Self {
                self.0.set_text(text);
                self
            }

            /// Its small icon: a name of the Kubuno icon set, an image file ([`crate::IconSource::file`])
            /// or a project resource ([`crate::IconSource::resource`]).
            pub fn small_icon(self, icon: impl Into<crate::IconSource>) -> Self {
                self.0.set_property("SmallIcon", Value::Str(icon.into().as_str().to_string()));
                self
            }

            /// Its large icon (32 pixels): like [`Self::small_icon`].
            pub fn large_icon(self, icon: impl Into<crate::IconSource>) -> Self {
                self.0.set_property("LargeIcon", Value::Str(icon.into().as_str().to_string()));
                self
            }

            /// The `<Command>` it runs (its `x:Name`).
            pub fn command(self, command: &str) -> Self {
                self.0.set_property("Command", Value::Str(command.to_string()));
                self
            }

            /// Its KeyTip.
            pub fn key_tip(self, tip: &str) -> Self {
                self.0.set_property("KeyTip", Value::Str(tip.to_string()));
                self
            }

            /// Any property of the element.
            pub fn property(self, name: &str, value: impl Into<Value>) -> Self {
                self.0.set_property(name, value);
                self
            }

            /// Adds `child` inside it (a group in a tab, a button in a group…).
            #[allow(clippy::should_implement_trait)]
            pub fn add(self, child: &impl AsControl) -> Self {
                self.0.controls().add(child);
                self
            }

            /// Adds several children.
            pub fn items<I: AsControl>(self, children: impl IntoIterator<Item = I>) -> Self {
                for c in children {
                    self.0.controls().add(&c);
                }
                self
            }

            /// Its label, as it is now.
            pub fn get_label(&self) -> String {
                self.0.get_text()
            }

            /// Changes its label.
            pub fn set_label(&self, text: impl Into<String>) {
                self.0.set_text(text);
            }

            /// `Click`, with this element as the sender.
            pub fn click(&self) -> ControlEvent<'_, Self, MouseEventArgs> {
                self.0.on("OnClick")
            }

            /// Runs `handler` on each `Click` (builder form of `click().subscribe`).
            pub fn on_click(self, handler: impl FnMut(&Self, &mut MouseEventArgs) + 'static) -> Self {
                self.click().subscribe(handler);
                self
            }

            /// This handle as a plain [`Control`].
            pub fn as_control(&self) -> &Control {
                &self.0
            }

            /// A handle of this type to `control`, when it is one.
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
                    tracing::warn!(handler, "handler `{handler}` takes a `&{}` sender but the event was raised by a <{}>: not called", Self::ELEMENT, control.element());
                }
                typed
            }
        }
    )*};
}

ribbon_types! {
    /// The Office ribbon (`<Ribbon>`).
    Ribbon;
    /// A tab of a ribbon.
    RibbonTab;
    /// Tabs shown in a context, under a coloured header.
    RibbonContextualTabGroup;
    /// A group of commands of a tab.
    RibbonGroup;
    /// Buttons joined in one row.
    RibbonControlGroup;
    /// A row or a column of commands.
    RibbonBox;
    /// The quick access toolbar.
    RibbonQuickAccessToolbar;
    /// The Backstage (the « Fichier » tab).
    RibbonBackstage;
    /// A tab of the Backstage.
    BackstageTab;
    /// A command of the Backstage.
    BackstageButton;
    /// A rule of the Backstage.
    BackstageSeparator;
    /// A command button.
    RibbonButton;
    /// A button that stays pressed.
    RibbonToggleButton;
    /// A toggle of a set.
    RibbonRadioButton;
    /// A button opening a menu.
    RibbonMenuButton;
    /// An action and a menu.
    RibbonSplitButton;
    /// A split button whose menu is a colour palette.
    RibbonColorPicker;
    /// An entry of a menu.
    RibbonMenuItem;
    /// A menu entry with a sub-menu.
    RibbonSplitMenuItem;
    /// A check box.
    RibbonCheckBox;
    /// A drop-down list, editable or not.
    RibbonComboBox;
    /// A text field.
    RibbonTextBox;
    /// A number with arrows.
    RibbonNumericField;
    /// A gallery of choices.
    RibbonGallery;
    /// A category of a gallery.
    RibbonGalleryCategory;
    /// A choice of a gallery.
    RibbonGalleryItem;
    /// A static text.
    RibbonLabel;
    /// A separator.
    RibbonSeparator;
    /// A command of the view (component tray).
    Command;
}

impl RibbonButton {
    /// A large button (icon over label).
    pub fn large(self) -> Self {
        self.0.set_property("Size", Value::Str("Large".into()));
        self
    }
}

impl RibbonTab {
    /// Its header.
    pub fn header(self, text: impl Into<String>) -> Self {
        self.0.set_property("Header", Value::Str(text.into()));
        self
    }
}

impl RibbonGroup {
    /// Its header.
    pub fn header(self, text: impl Into<String>) -> Self {
        self.0.set_property("Header", Value::Str(text.into()));
        self
    }

    /// `DialogLauncherClick`.
    pub fn dialog_launcher_click(&self) -> ControlEvent<'_, Self, MouseEventArgs> {
        self.0.on("OnDialogLauncherClick")
    }
}

impl RibbonContextualTabGroup {
    /// Shows or hides its tabs (a table got selected…).
    pub fn set_visible(&self, visible: bool) {
        self.0.set_visible(visible);
    }
}

impl Ribbon {
    /// The active tab (its `x:Name`).
    pub fn selected_tab(&self) -> String {
        self.0.string("SelectedTab")
    }

    /// Makes the tab `name` active.
    pub fn set_selected_tab(&self, name: &str) {
        self.0.set_property("SelectedTab", Value::Str(name.to_string()));
    }

    /// Minimizes or restores the ribbon.
    pub fn set_minimized(&self, on: bool) {
        self.0.set_property("IsMinimized", Value::Bool(on));
    }

    /// `SelectedTabChanged`.
    pub fn selected_tab_changed(&self) -> ControlEvent<'_, Self, TextChangedEventArgs> {
        self.0.on("OnSelectedTabChanged")
    }

    /// Merges `fragment` into this ribbon (`vskubuno/docs/RIBBON.md` §8): its tabs and groups name the
    /// ribbon's own by `x:Name`. The fragment stays while the returned handle lives (`keep()` it to
    /// leave it for the session).
    pub fn merge(&self, fragment: kubuno_ui::ribbon::merge::RibbonExtension) -> kubuno_ui::ribbon::merge::MergeHandle {
        kubuno_ui::ribbon::merge::register(self.0.get_name(), fragment)
    }
}

macro_rules! ribbon_checkable {
    ($($name:ident),*) => {$(
        impl $name {
            /// Whether it is checked.
            pub fn checked(self, on: bool) -> Self {
                self.0.set_property("Checked", on);
                self
            }

            pub fn is_checked(&self) -> bool {
                self.0.flag("Checked", false)
            }

            pub fn set_checked(&self, on: bool) {
                self.0.set_property("Checked", on);
            }

            /// `CheckedChanged`.
            pub fn checked_changed(&self) -> ControlEvent<'_, Self, CheckedChangedEventArgs> {
                self.0.on("OnCheckedChanged")
            }
        }
    )*};
}
ribbon_checkable!(RibbonToggleButton, RibbonRadioButton, RibbonCheckBox, RibbonMenuItem, Command);

impl Command {
    /// Whether it can run (the elements running it are greyed out otherwise).
    pub fn set_enabled(&self, on: bool) {
        self.0.set_enabled(on);
    }

    /// `Execute`: an element running it was clicked, or its shortcut pressed.
    pub fn executed(&self) -> ControlEvent<'_, Self, EmptyEventArgs> {
        self.0.on("OnExecute")
    }

    /// Runs `handler` on each `Execute`.
    pub fn on_execute(self, handler: impl FnMut(&Self, &mut EmptyEventArgs) + 'static) -> Self {
        self.executed().subscribe(handler);
        self
    }
}

impl RibbonComboBox {
    /// The selected (or typed) value.
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

impl RibbonGallery {
    /// The selected item's value.
    pub fn get_selected_value(&self) -> String {
        self.0.string("SelectedValue")
    }

    pub fn set_selected_value(&self, value: impl Into<String>) {
        self.0.set_property("SelectedValue", Value::Str(value.into()));
    }

    /// `ItemClick` (`e.new` is the item's value).
    pub fn item_click(&self) -> ControlEvent<'_, Self, TextChangedEventArgs> {
        self.0.on("OnItemClick")
    }
}

impl RibbonColorPicker {
    /// The colour (`#RRGGBB`, empty for Automatic).
    pub fn get_selected_color(&self) -> String {
        self.0.string("SelectedColor")
    }

    pub fn set_selected_color(&self, color: &str) {
        self.0.set_property("SelectedColor", Value::Str(color.to_string()));
    }

    /// `SelectedColorChanged`.
    pub fn selected_color_changed(&self) -> ControlEvent<'_, Self, TextChangedEventArgs> {
        self.0.on("OnSelectedColorChanged")
    }
}

impl RibbonNumericField {
    pub fn get_value(&self) -> f32 {
        self.0.number("Value").unwrap_or(0.0)
    }

    pub fn set_value(&self, value: f32) {
        self.0.set_property("Value", value);
    }

    /// `ValueChanged`.
    pub fn value_changed(&self) -> ControlEvent<'_, Self, NumericValueChangedEventArgs> {
        self.0.on("OnValueChanged")
    }
}

impl RibbonTextBox {
    /// `TextChanged` (committed).
    pub fn text_changed(&self) -> ControlEvent<'_, Self, TextChangedEventArgs> {
        self.0.on("OnTextChanged")
    }
}
