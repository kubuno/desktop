//! The built-in control classes (`vskubuno/docs/EVENTS.md`, EVT-7a): one class of the control
//! hierarchy ([`crate::component`]) per element of the default registry, named like its
//! element, each in its family's level:
//!
//! | Level | Classes |
//! |---|---|
//! | `ButtonBase` | [`Button`], [`IconButton`], [`CheckBox`], [`RadioButton`], [`Switch`] |
//! | `TextBoxBase` | [`TextField`], [`TextArea`], [`MaskedField`], [`SearchField`] |
//! | `ListControl` | [`ListBox`], [`CheckedListBox`], [`ComboBox`], [`Dropdown`] |
//! | `LabelBase` | [`Label`], [`LinkLabel`], [`Badge`] |
//! | `RangeBase` | [`Slider`], [`ProgressBar`], [`NumericField`] |
//! | `ScrollableControl` | [`ScrollArea`] |
//! | `ContainerBase` | [`Panel`], [`GroupBox`], [`Card`], [`Stack`], [`Tabs`], [`Splitter`], [`Accordion`] |
//! | `Control` | [`Icon`], [`Separator`], [`Spinner`], [`Callout`], [`EmptyState`], [`Toolbar`], [`Breadcrumb`], [`Stepper`], [`ListView`], [`TreeView`], [`DataTable`], [`MonthCalendar`], [`DatePicker`], [`ColorField`], [`GradientField`] |
//! | `Component` (non-visual) | the structural elements, in [`items`]: `Item`, `Column`, `TabItem`, `Option`, `Step`, `AccordionSection`, `BreadcrumbItem`, `ToolbarItem` |
//!
//! They serve three purposes:
//!
//! - **the objects behind a view's elements**: every element of a compiled `.kbview` owns an
//!   instance of its class (`crate::design::DesignSlot`), through whose `on_…` methods its events
//!   are delivered;
//! - **the bases of custom controls**: `#[derive(Component)] #[kubuno(extends = Button)] struct
//!   RoundButton { base: Button }` inherits everything a `<Button>` does;
//! - **`Sender` types** (EVT-4): a handler bound to a `<Button>` declares `sender: &Sender<Button>`
//!   and reads the button's properties of the frame through it (`sender.text()`); the resolved
//!   properties are [`ElementProps`](crate::events::ElementProps).
//!
//! In a view, the element's node keeps rendering it (`kubuno_desktop_ui`'s widgets, rebuilt every frame
//! from the bound properties) — except `<Button>`, whose node syncs its properties into its
//! [`Button`] and paints through the class's `on_paint`. Built in Rust and hosted by
//! [`crate::component::ControlHost`], [`Button`] and [`Label`] paint themselves; the other classes
//! paint nothing yet outside a view (EVT-7b gives each its standalone look).

use std::cell::RefCell;
use std::rc::Rc;

use kubuno_desktop_ui::buttons::{Button as UiButton, Size as ButtonSize, Variant};
use kubuno_desktop_ui::display::{Label as UiLabel, Role};
use kubuno_desktop_ui::{Canvas, Size, Widget};

use crate::component::{
    ButtonBaseCore, Component, ContainerBaseCore, Control, ControlCore, Keys, LabelBaseCore, ListControlCore, PaintEventCx, RangeBaseCore, ScrollableControlCore, TextBoxBaseCore,
};

// ── Button ──────────────────────────────────────────────────────────────────────────────

/// `<Button>`: a push button (WinForms `Button`). Its look is `kubuno_desktop_ui`'s button:
/// [`Button::variant`], [`Button::size`], an optional leading [`Button::icon`], a loading state.
#[derive(Component, Default)]
#[kubuno(extends = ButtonBase, overrides(Control))]
pub struct Button {
    base: ButtonBaseCore,
    pub variant: Variant,
    pub size: ButtonSize,
    /// A leading icon (a Lucide glyph name, see `Canvas::vector_icon`).
    pub icon: Option<&'static str>,
    /// The content is replaced by a spinning ring and the button is inert.
    pub loading: bool,
    /// Where the loading ring is in its turn (`0.0..1.0`).
    pub loading_phase: f32,
    /// `IconSize`: the icon's own size, in DIP.
    pub icon_size: Option<f32>,
    /// `IconSpacing`: the gap between the icon and the text, in DIP.
    pub icon_spacing: Option<f32>,
    /// The icon is laid out like an image (`ImageAlign`, `TextImageRelation`).
    pub icon_aligned: bool,
}

impl Button {
    /// A primary, medium button labelled `text`.
    pub fn new(text: &str) -> Self {
        Self { base: ButtonBaseCore::with_text(text), ..Self::default() }
    }

    /// Builder: the variant.
    pub fn with_variant(mut self, variant: Variant) -> Self {
        self.variant = variant;
        self
    }

    /// Builder: the size.
    pub fn with_size(mut self, size: ButtonSize) -> Self {
        self.size = size;
        self
    }

    /// The `kubuno_desktop_ui` button this class paints (rebuilt from its properties).
    pub fn widget(&self) -> UiButton {
        let mut b = UiButton::new(self.text()).variant(self.variant).size(self.size);
        if let Some(icon) = self.icon {
            b = b.icon(icon);
        }
        let mut b = b.loading(self.loading).loading_phase(self.loading_phase);
        // `ButtonBase`: TextAlign, ImageAlign, TextImageRelation and the mnemonic's underline.
        b.text_align = self.base.text_align;
        b.image_align = self.base.image_align;
        b.text_image_relation = self.base.text_image_relation;
        b.mnemonic = self.base.mnemonic;
        b.icon_size = self.icon_size;
        b.gap = self.icon_spacing;
        b.icon_aligned = self.icon_aligned;
        b
    }
}

impl Control for Button {
    /// Paints the Kubuno button (with its `Image`), then raises `Paint`.
    fn on_paint(&mut self, e: &mut PaintEventCx<'_>) {
        let mut widget = self.widget();
        let image = self.base.image.as_deref().filter(|p| !p.is_empty()).map(kubuno_desktop_ui::graphics::Image::from_file);
        let size = image.as_ref().and_then(|i| e.graphics.image_size(i));
        widget.image_size = size.map(|s| (s.width, s.height));
        widget.paint(e.graphics, e.clip_rectangle, e.state);
        if let (Some(image), Some(_), Some(rect)) = (&image, size, widget.image_rect(e.graphics, e.clip_rectangle)) {
            e.graphics.draw_image_with(image, rect, None, if e.state.disabled { 0.5 } else { 1.0 });
        }
        e.raise(self, "OnPaint");
    }

    /// The button's measured size (its label, icon and padding).
    fn get_preferred_size(&self, canvas: &dyn Canvas, _proposed: Size) -> Size {
        self.widget().measure(canvas)
    }
}

// ── Label ───────────────────────────────────────────────────────────────────────────────

/// `<Label>`: static text (WinForms `Label`), in one of the type roles.
#[derive(Component, Default)]
#[kubuno(extends = LabelBase, overrides(Control))]
pub struct Label {
    base: LabelBaseCore,
    pub role: Role,
}

impl Label {
    /// A body-text label.
    pub fn new(text: &str) -> Self {
        Self { base: LabelBaseCore::with_text(text), role: Role::default() }
    }

    /// The `kubuno_desktop_ui` label this class paints.
    pub fn widget(&self) -> UiLabel {
        UiLabel::new(self.text()).role(self.role).align(self.base.text_align)
    }
}

impl Control for Label {
    /// Paints the text, then raises `Paint`.
    fn on_paint(&mut self, e: &mut PaintEventCx<'_>) {
        self.widget().paint(e.graphics, e.clip_rectangle, e.state);
        e.raise(self, "OnPaint");
    }

    fn get_preferred_size(&self, canvas: &dyn Canvas, _proposed: Size) -> Size {
        self.widget().measure(canvas)
    }
}

// ── Text fields: arrows and Home/End are input keys (WinForms `TextBoxBase.IsInputKey`) ──

macro_rules! text_classes {
    ($($(#[$doc:meta])* $name:ident { multiline: $multi:expr };)*) => {$(
        $(#[$doc])*
        #[derive(Component)]
        #[kubuno(extends = TextBoxBase, overrides(Control))]
        pub struct $name {
            base: TextBoxBaseCore,
        }

        impl Default for $name {
            fn default() -> Self {
                Self { base: TextBoxBaseCore { multiline: $multi, ..TextBoxBaseCore::default() } }
            }
        }

        impl $name {
            pub fn new() -> Self {
                Self::default()
            }
        }

        impl Control for $name {
            fn is_input_key(&self, key: Keys) -> bool {
                self.base.is_input_key(key)
            }
        }
    )*};
}

text_classes! {
    /// `<TextField>`: a one-line text field (WinForms `TextBox`).
    TextField { multiline: false };
    /// `<TextArea>`: a multi-line text field (Enter and the vertical arrows are input keys).
    TextArea { multiline: true };
    /// `<MaskedField>`: a text field with an input mask (WinForms `MaskedTextBox`).
    MaskedField { multiline: false };
    /// `<SearchField>`: a text field with a search glyph and a clear button.
    SearchField { multiline: false };
}

// ── The classes whose look lives in their view node ────────────────────────────────────

macro_rules! classes {
    ($($(#[$doc:meta])* $name:ident: $level:ident($core:ty) $({ $($init:tt)* })?;)*) => {$(
        $(#[$doc])*
        #[derive(Component)]
        #[kubuno(extends = $level)]
        pub struct $name {
            base: $core,
        }

        impl Default for $name {
            fn default() -> Self {
                #[allow(unused_mut)]
                let mut base = <$core>::default();
                $( classes!(@init base, $($init)*); )?
                Self { base }
            }
        }

        impl $name {
            pub fn new() -> Self {
                Self::default()
            }
        }
    )*};
    (@init $base:ident, selectable) => {
        $base.control.styles.set($crate::component::ControlStyles::SELECTABLE, true);
        $base.control.tab_stop = true;
    };
    (@init $base:ident, standard_double_click) => {
        $base.control.styles.set($crate::component::ControlStyles::STANDARD_DOUBLE_CLICK, true);
    };
}

classes! {
    /// `<IconButton>`: a square button showing one glyph.
    IconButton: ButtonBase(ButtonBaseCore);
    /// `<DropDownButton>`: a button that opens a menu (WinForms `ToolStripDropDownButton`).
    DropDownButton: ButtonBase(ButtonBaseCore);
    /// `<SplitButton>`: a button and an arrow that opens a menu (WinForms `ToolStripSplitButton`).
    SplitButton: ButtonBase(ButtonBaseCore);
    /// `<MenuBar>`: the menu bar of a window (WinForms `MenuStrip`).
    MenuBar: Control(ControlCore);
    /// `<CheckBox>` (WinForms `CheckBox`).
    CheckBox: ButtonBase(ButtonBaseCore);
    /// `<RadioButton>` (WinForms `RadioButton`).
    RadioButton: ButtonBase(ButtonBaseCore);
    /// `<Switch>`: an on/off toggle.
    Switch: ButtonBase(ButtonBaseCore);
    /// `<ListBox>` (WinForms `ListBox`).
    ListBox: ListControl(ListControlCore);
    /// `<CheckedListBox>` (WinForms `CheckedListBox`).
    CheckedListBox: ListControl(ListControlCore);
    /// `<ComboBox>`: an editable drop-down list (WinForms `ComboBox`).
    ComboBox: ListControl(ListControlCore);
    /// `<Dropdown>`: a drop-down list of options.
    Dropdown: ListControl(ListControlCore);
    /// `<LinkLabel>`: a label that acts as a hyperlink (WinForms `LinkLabel`, selectable).
    LinkLabel: LabelBase(LabelBaseCore) { selectable };
    /// `<Badge>`: a small status chip.
    Badge: LabelBase(LabelBaseCore);
    /// `<Slider>` (WinForms `TrackBar`).
    Slider: RangeBase(RangeBaseCore);
    /// `<ProgressBar>` (WinForms `ProgressBar`).
    ProgressBar: RangeBase(RangeBaseCore);
    /// `<NumericField>` (WinForms `NumericUpDown`).
    NumericField: RangeBase(RangeBaseCore);
    /// `<ScrollArea>`: a scrolling viewport.
    ScrollArea: ScrollableControl(ScrollableControlCore);
    /// `<Panel>`: a Dock/Anchor container (WinForms `Panel`).
    Panel: ContainerBase(ContainerBaseCore);
    /// `<GroupBox>`: a captioned frame (WinForms `GroupBox`).
    GroupBox: ContainerBase(ContainerBaseCore);
    /// `<FloatingWindow>`: a window drawn inside the view (the web `FloatingWindow`).
    FloatingWindow: ContainerBase(ContainerBaseCore);
    /// `<Card>`: a surface with a header.
    Card: ContainerBase(ContainerBaseCore);
    /// `<Stack>`: a flow container.
    Stack: ContainerBase(ContainerBaseCore);
    /// `<Tabs>`: tab pages (WinForms `TabControl`).
    Tabs: ContainerBase(ContainerBaseCore) { selectable };
    /// `<Splitter>`: two panes and a movable bar (WinForms `SplitContainer`).
    Splitter: ContainerBase(ContainerBaseCore);
    /// `<Accordion>`: collapsible sections.
    Accordion: ContainerBase(ContainerBaseCore);
    /// `<DockArea>`: a work area and its dockable panels.
    DockArea: ContainerBase(ContainerBaseCore);
    /// `<WorkspaceShell>`: the frame of an editor (top bar, status bar, body).
    WorkspaceShell: ContainerBase(ContainerBaseCore);
    /// `<Popover>`: a floating panel anchored to a control (light-dismiss).
    Popover: ContainerBase(ContainerBaseCore);
    /// `<TableLayoutPanel>`: a grid of rows and columns (WinForms `TableLayoutPanel`).
    TableLayoutPanel: ContainerBase(ContainerBaseCore);
    /// `<Repeater>`: a list whose items are views (WPF `ItemsControl`).
    Repeater: Control(ControlCore);
    /// `<Sidebar>`: a navigation pane (WinUI `NavigationView`).
    Sidebar: Control(ControlCore);
    /// `<StatusBar>`: a status bar (WinForms `StatusStrip`).
    StatusBar: Control(ControlCore);
    /// `<Avatar>`: a person's picture or initials.
    Avatar: Control(ControlCore);
    /// `<PictureBox>`: an image (WinForms `PictureBox`).
    PictureBox: Control(ControlCore);
    /// `<SplashArtwork>`: the artwork of a splash screen (`kubuno_desktop_ui::splash`).
    SplashArtwork: Control(ControlCore);
    /// `<Icon>`: one glyph.
    Icon: Control(ControlCore);
    /// `<Separator>`: a rule.
    Separator: Control(ControlCore);
    /// `<Spinner>`: an activity indicator.
    Spinner: Control(ControlCore);
    /// `<Callout>`: an inline message banner.
    Callout: Control(ControlCore);
    /// `<EmptyState>`: the placeholder of an empty list or page.
    EmptyState: Control(ControlCore);
    /// `<Toolbar>`: a row of commands (WinForms `ToolStrip`).
    Toolbar: Control(ControlCore);
    /// `<Breadcrumb>`: a path of segments.
    Breadcrumb: Control(ControlCore);
    /// `<Stepper>`: the steps of a wizard.
    Stepper: Control(ControlCore);
    /// `<ListView>` (WinForms `ListView`).
    ListView: Control(ControlCore);
    /// `<TreeView>` (WinForms `TreeView`).
    TreeView: Control(ControlCore);
    /// `<DataTable>`: a data grid (WinForms `DataGridView`).
    DataTable: Control(ControlCore);
    /// `<MonthCalendar>` (WinForms `MonthCalendar`).
    MonthCalendar: Control(ControlCore);
    /// `<DatePicker>` (WinForms `DateTimePicker`).
    DatePicker: Control(ControlCore);
    /// `<ColorField>`: a colour swatch that opens a picker.
    ColorField: Control(ControlCore);
    /// `<GradientField>`: a gradient swatch that opens a gradient picker.
    GradientField: Control(ControlCore);
    /// `<PaintBox>`: a surface its `Paint` handler draws on (Delphi's `TPaintBox`, WinForms' `Panel`/`PictureBox` with a `Paint` handler).
    PaintBox: Control(ControlCore);
}

/// The ribbon family's classes (`vskubuno/docs/RIBBON.md` §3).
#[path = "ribbon_classes.rs"]
pub mod ribbon;

/// The structural elements (`<TabItem>`, `<Item>`, `<Column>`…) as non-visual components
/// (WinForms `ToolStripItem`, `ColumnHeader`, `TabPage`'s role here): their parent control
/// raises their events. Not in the prelude — `Option` would shadow `std::option::Option`.
pub mod items {
    use crate::component::{Component, ComponentCore};

    macro_rules! items {
        ($($(#[$doc:meta])* $name:ident;)*) => {$(
            $(#[$doc])*
            #[derive(Component, Default)]
            #[kubuno(extends = Component)]
            pub struct $name {
                base: ComponentCore,
            }
        )*};
    }

    items! {
        /// `<Item>`: a row of a list, a node of a tree.
        Item;
        /// `<Column>`: a column of a `<ListView>` or `<DataTable>`.
        Column;
        /// `<TabItem>`: a page of a `<Tabs>`.
        TabItem;
        /// `<Option>`: an option of a `<Dropdown>` / `<ComboBox>`.
        Option;
        /// `<Step>`: a step of a `<Stepper>`.
        Step;
        /// `<AccordionSection>`: a section of an `<Accordion>`.
        AccordionSection;
        /// `<BreadcrumbItem>`: a segment of a `<Breadcrumb>`.
        BreadcrumbItem;
        /// `<ToolbarItem>`: a command of a `<Toolbar>`.
        ToolbarItem;
        /// `<MenuItem>`: a command of a menu (a `<ContextMenu>`, a `<MenuBar>`, a drop-down button).
        MenuItem;
        /// `<MenuSeparator>`: a line between the commands of a menu.
        MenuSeparator;
        /// `<MenuHeader>`: a section title in a menu.
        MenuHeader;
        /// `<DockPanel>`: a panel of a `<DockArea>`.
        DockPanel;
        /// `<SidebarItem>`: a row of a `<Sidebar>`.
        SidebarItem;
        /// `<SidebarSection>`: a section header of a `<Sidebar>`.
        SidebarSection;
        /// `<StatusLabel>`: a cell of a `<StatusBar>`.
        StatusLabel;
    }
}

/// The built-in non-visual components (WinForms' component tray, EVT-7b): elements a view
/// declares that paint nothing. Not in the prelude — `Timer` is also the Rust timer of
/// [`crate::events::Timer`].
pub mod components {
    use crate::component::{Component, ComponentCore};

    /// `<Timer>`: raises `Tick` every `Interval` milliseconds while `Enabled` (WinForms `Timer`).
    #[derive(Component, Default)]
    #[kubuno(extends = Component)]
    pub struct Timer {
        base: ComponentCore,
    }

    /// `<ToolTip>`: how the view's tooltips appear (WinForms `ToolTip`, the extender of every
    /// control's `ToolTip` property).
    #[derive(Component, Default)]
    #[kubuno(extends = Component)]
    pub struct ToolTip {
        base: ComponentCore,
    }

    /// `<ContextMenu>`: the menu a right click on a control opens (WinForms `ContextMenuStrip`).
    #[derive(Component, Default)]
    #[kubuno(extends = Component)]
    pub struct ContextMenu {
        base: ComponentCore,
    }
}

// ── The class table ─────────────────────────────────────────────────────────────────────

/// A class the registry and the view runtime can name and instantiate.
#[derive(Clone, Copy)]
pub struct ClassRef {
    /// The class (and element) name.
    pub name: &'static str,
    /// The class followed by its ancestors, `"Component"` last.
    pub chain: &'static [&'static str],
    /// A fresh instance with its defaults (`None` for an application class without `Default`,
    /// or one only known from its source: EVT-7b).
    pub create: fn() -> Option<Rc<RefCell<dyn Component>>>,
}

fn create<C: Component + Default>() -> Option<Rc<RefCell<dyn Component>>> {
    Some(Rc::new(RefCell::new(C::default())))
}

macro_rules! class_table {
    ($($name:literal => $ty:ty),* $(,)?) => {
        /// Every built-in class, controls then structural elements.
        pub const CLASSES: &[ClassRef] = &[
            $(ClassRef {
                name: $name,
                chain: <$ty as crate::component::Lineage>::CHAIN,
                create: create::<$ty>,
            }),*
        ];
    };
}

class_table! {
    "Accordion" => Accordion, "Badge" => Badge, "Breadcrumb" => Breadcrumb, "Button" => Button, "Callout" => Callout, "Card" => Card,
    "CheckBox" => CheckBox, "CheckedListBox" => CheckedListBox, "ColorField" => ColorField, "GradientField" => GradientField, "ComboBox" => ComboBox, "DataTable" => DataTable,
    "DatePicker" => DatePicker, "Dropdown" => Dropdown, "EmptyState" => EmptyState, "FloatingWindow" => FloatingWindow, "GroupBox" => GroupBox, "Icon" => Icon,
    "IconButton" => IconButton, "Label" => Label, "LinkLabel" => LinkLabel, "ListBox" => ListBox, "ListView" => ListView,
    "MaskedField" => MaskedField, "MonthCalendar" => MonthCalendar, "NumericField" => NumericField, "Panel" => Panel,
    "ProgressBar" => ProgressBar, "RadioButton" => RadioButton, "ScrollArea" => ScrollArea, "SearchField" => SearchField,
    "Separator" => Separator, "Slider" => Slider, "Spinner" => Spinner, "Splitter" => Splitter, "Stack" => Stack, "Stepper" => Stepper,
    "Switch" => Switch, "Tabs" => Tabs, "PaintBox" => PaintBox, "TextArea" => TextArea, "TextField" => TextField, "Toolbar" => Toolbar, "TreeView" => TreeView,
    "Item" => items::Item, "Column" => items::Column, "TabItem" => items::TabItem, "Option" => items::Option, "Step" => items::Step,
    "AccordionSection" => items::AccordionSection, "BreadcrumbItem" => items::BreadcrumbItem, "ToolbarItem" => items::ToolbarItem,
    "MenuItem" => items::MenuItem, "MenuSeparator" => items::MenuSeparator, "MenuHeader" => items::MenuHeader, "MenuBar" => MenuBar, "DropDownButton" => DropDownButton, "SplitButton" => SplitButton, "DockPanel" => items::DockPanel, "DockArea" => DockArea, "WorkspaceShell" => WorkspaceShell,
    "UserControl" => crate::component::UserControlCore, "Timer" => components::Timer, "ToolTip" => components::ToolTip,
    "ContextMenu" => components::ContextMenu,
    "Popover" => Popover, "TableLayoutPanel" => TableLayoutPanel, "Repeater" => Repeater, "Sidebar" => Sidebar, "StatusBar" => StatusBar,
    "Avatar" => Avatar, "PictureBox" => PictureBox, "SplashArtwork" => SplashArtwork, "SidebarItem" => items::SidebarItem, "SidebarSection" => items::SidebarSection,
    "StatusLabel" => items::StatusLabel,
    "Ribbon" => ribbon::Ribbon, "RibbonTab" => ribbon::RibbonTab, "RibbonContextualTabGroup" => ribbon::RibbonContextualTabGroup, "RibbonGroup" => ribbon::RibbonGroup, "RibbonControlGroup" => ribbon::RibbonControlGroup, "RibbonBox" => ribbon::RibbonBox, "RibbonQuickAccessToolbar" => ribbon::RibbonQuickAccessToolbar, "RibbonBackstage" => ribbon::RibbonBackstage, "BackstageTab" => ribbon::BackstageTab, "BackstageButton" => ribbon::BackstageButton, "BackstageSeparator" => ribbon::BackstageSeparator, "RibbonButton" => ribbon::RibbonButton, "RibbonToggleButton" => ribbon::RibbonToggleButton, "RibbonRadioButton" => ribbon::RibbonRadioButton, "RibbonMenuButton" => ribbon::RibbonMenuButton, "RibbonSplitButton" => ribbon::RibbonSplitButton, "RibbonColorPicker" => ribbon::RibbonColorPicker, "RibbonMenuItem" => ribbon::RibbonMenuItem, "RibbonSplitMenuItem" => ribbon::RibbonSplitMenuItem, "RibbonCheckBox" => ribbon::RibbonCheckBox, "RibbonComboBox" => ribbon::RibbonComboBox, "RibbonTextBox" => ribbon::RibbonTextBox, "RibbonNumericField" => ribbon::RibbonNumericField, "RibbonGallery" => ribbon::RibbonGallery, "RibbonGalleryCategory" => ribbon::RibbonGalleryCategory, "RibbonGalleryItem" => ribbon::RibbonGalleryItem, "RibbonLabel" => ribbon::RibbonLabel, "RibbonSeparator" => ribbon::RibbonSeparator, "Command" => ribbon::Command,
}

/// The element names of the control classes (the structural elements excluded).
pub const ALL: &[&str] = &[
    "Accordion", "Badge", "Breadcrumb", "Button", "Callout", "Card", "CheckBox", "CheckedListBox", "ColorField", "ComboBox", "DataTable", "DatePicker", "Dropdown",
    "EmptyState", "FloatingWindow", "GradientField", "GroupBox", "Icon", "IconButton", "Label", "LinkLabel", "ListBox", "ListView", "MaskedField", "MonthCalendar", "NumericField", "PaintBox", "Panel", "ProgressBar",
    "RadioButton", "ScrollArea", "SearchField", "Separator", "Slider", "Spinner", "Splitter", "Stack", "Stepper", "Switch", "Tabs", "TextArea", "TextField", "Toolbar",
    "TreeView", "UserControl", "DockArea", "WorkspaceShell", "Ribbon", "Popover", "TableLayoutPanel", "Repeater", "Sidebar", "StatusBar", "Avatar", "PictureBox", "SplashArtwork",
    "MenuBar", "DropDownButton", "SplitButton",
];

/// The class of element `name`: a built-in class, else an application's class (EVT-7b,
/// [`crate::registry::project`]).
pub fn class_of(name: &str) -> Option<&'static ClassRef> {
    CLASSES.iter().find(|c| c.name == name).or_else(|| crate::registry::project::snapshot().classes.iter().find(|c| c.name == name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::component::{ButtonBase, ControlStyles, HasControlCore, Lineage};
    use crate::events::ElementType;

    /// Every control of the registry has its class here, and nothing else does.
    #[test]
    fn one_type_per_non_structural_control() {
        let mut expected: Vec<&str> =
            crate::registry::builtins().iter().map(|c| c.name).filter(|n| !crate::registry::is_gated(n) && !crate::registry::is_non_visual(n)).collect();
        expected.sort_unstable();
        let mut actual = ALL.to_vec();
        actual.sort_unstable();
        assert_eq!(actual, expected);
        assert_eq!(<Button as ElementType>::ELEMENT, "Button");
    }

    /// Every element of the registry (controls and structural elements) has a class, whose
    /// chain starts with its name; the controls are `Control`s, the structural ones are not.
    #[test]
    fn every_registry_element_has_a_class() {
        for meta in crate::registry::builtins() {
            let class = class_of(meta.name).unwrap_or_else(|| panic!("no class for <{}>", meta.name));
            assert_eq!(class.chain.first(), Some(&meta.name));
            assert_eq!(class.chain.last(), Some(&"Component"));
            let instance = (class.create)().expect("a built-in class has a default instance");
            let instance = instance.borrow();
            assert_eq!(instance.class_name(), meta.name);
            let control = (!crate::registry::is_gated(meta.name) || meta.is_a("RibbonControl")) && !crate::registry::is_non_visual(meta.name);
            assert_eq!(instance.as_control().is_some(), control, "{}", meta.name);
        }
        assert_eq!(CLASSES.len(), crate::registry::builtins().len());
    }

    #[test]
    fn classes_sit_in_their_family_level() {
        assert_eq!(<Button as Lineage>::CHAIN, ["Button", "ButtonBase", "Control", "Component"]);
        assert_eq!(<TextArea as Lineage>::CHAIN, ["TextArea", "TextBoxBase", "Control", "Component"]);
        assert_eq!(<Panel as Lineage>::CHAIN, ["Panel", "ContainerBase", "ScrollableControl", "Control", "Component"]);
        assert_eq!(<Slider as Lineage>::CHAIN, ["Slider", "RangeBase", "Control", "Component"]);
        assert_eq!(<items::TabItem as Lineage>::CHAIN, ["TabItem", "Component"]);
        assert_eq!(class_of("Option").map(|c| c.chain), Some(&["Option", "Component"][..]));
    }

    #[test]
    fn classes_carry_the_winforms_styles() {
        let b = Button::new("Ok");
        assert!(b.get_style(ControlStyles::STANDARD_CLICK) && !b.get_style(ControlStyles::STANDARD_DOUBLE_CLICK), "a button's second click is a Click");
        assert!(b.can_select());
        let l = Label::new("Name");
        assert!(!l.can_select() && !l.control_core().tab_stop, "a label is not selectable");
        assert!(LinkLabel::new().can_select());
        assert!(!Panel::new().can_select() && Tabs::new().can_select());
        let t = TextArea::new();
        assert!(t.is_input_key(Keys::plain(crate::events::Key(kubuno_desktop_controls::host::vk::DOWN))));
        assert!(!TextField::new().is_input_key(Keys::plain(crate::events::Key(kubuno_desktop_controls::host::vk::DOWN))));
        let mut clicks = Button::new("x");
        clicks.perform_click();
    }
}
