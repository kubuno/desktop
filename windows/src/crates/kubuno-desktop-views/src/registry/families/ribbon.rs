//! Component family `ribbon` — the Office ribbon as a designable family of controls
//! (`vskubuno/docs/RIBBON.md`): `<Ribbon>` and every element composing it (tabs, contextual tab
//! groups, groups, joined control groups and boxes, the quick access toolbar, the Backstage and
//! its entries, buttons, toggles, radio buttons, menu / split buttons, colour pickers, menu items,
//! check boxes, combo boxes, text boxes, numeric fields, galleries and their categories and items,
//! labels, separators), plus the non-visual `<Command>` their `Command` property runs.
//!
//! One node, [`RibbonNode`], builds the `kubuno_desktop_ui::ribbon` engine's model from the element tree
//! every frame (properties, bindings and command state resolved), runs it, and maps what the
//! engine reports back to the sub-elements: every sub-element is a real control (a class of the
//! `RibbonControl` level) whose rectangle the node declares as a virtual region
//! (`crate::virtual_regions`) — selected on the design surface, routed by the input router, the
//! sender of its own events.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

#[allow(unused_imports)] // Used by the `component!` invocations below.
use crate::registry::macros::component;
use crate::registry::{ComponentMeta, PropKind, PropertyMeta};

use crate::ast::{AstNode, Element};
use crate::binding::{PropSource, Value, ViewModel};
use crate::events::{ChangeSource, CheckedChangedEventArgs, EmptyEventArgs, NumericValueChangedEventArgs, TextChangedEventArgs};
use crate::node::{PaintCx, ViewEventKind, ViewNode};
use crate::props::{BuildCx, BuildError, Props};
use crate::virtual_regions::VirtualElement;

use kubuno_desktop_controls::host::{self, Modifiers};
use kubuno_desktop_ui::ribbon::{
    self as engine, Backstage, BackstageSection, GalleryDisplay, GallerySpec, Icon, ItemKind, ItemSize, RegionKind, Ribbon, RibbonDesign, RibbonEvent, RibbonGroup, RibbonItem, RibbonOption,
    RibbonTab, RibbonTheme, ScaleStep, ScalingPolicy,
};
use kubuno_desktop_ui::{Canvas, Rect, Size};

// ═════════════════════════════════════════════════════════════════════════════
// The two levels' properties
// ═════════════════════════════════════════════════════════════════════════════

const APPEARANCE: &str = "Appearance";
const BEHAVIOR: &str = "Behavior";
const LAYOUT: &str = "Layout";

/// What every element of a ribbon has (`RibbonControl`), and the container layout properties of
/// `Control` hidden: the ribbon lays its elements out.
pub const RIBBON_CONTROL_PROPERTIES: &[PropertyMeta] = &[
    PropertyMeta::new("Label", PropKind::String, "", "Text of the element (a tab's or a group's is its Header). Empty: the command's label.").category(APPEARANCE).localizable().bindable(),
    PropertyMeta::new("SmallIcon", PropKind::String, "", "Icon of the element when small: a name of the Kubuno icon set (Bold, ClipboardPaste…).").category(APPEARANCE).editor(crate::registry::families::ribbon::ICON_EDITOR),
    PropertyMeta::new("LargeIcon", PropKind::String, "", "Icon of the element when large (32 pixels). Empty: the small icon.").category(APPEARANCE).editor(crate::registry::families::ribbon::ICON_EDITOR),
    PropertyMeta::new("ScreenTipTitle", PropKind::String, "", "Title of the tooltip. Empty: the label.").category(APPEARANCE).localizable(),
    PropertyMeta::new("ScreenTipText", PropKind::String, "", "Description shown in the tooltip.").category(APPEARANCE).localizable(),
    PropertyMeta::new("KeyTip", PropKind::String, "", "Letters shown when Alt is pressed; typing them runs the element. Empty: assigned automatically.").category(BEHAVIOR),
    PropertyMeta::new("Command", PropKind::String, "", "The Command of the view the element runs: its label, icons, shortcut, state and Execute event.").category(BEHAVIOR).editor("reference:Command"),
    PropertyMeta::new("CommandParameter", PropKind::String, "", "Value handed to the command when the element runs it.").category(BEHAVIOR),
    PropertyMeta::new("ShowLabel", PropKind::Bool, "true", "Whether the label is shown next to the icon (it stays in the tooltip).").category(APPEARANCE),
    PropertyMeta::new("CanAddToQat", PropKind::Bool, "true", "Whether a right click offers to add the element to the quick access toolbar.").category(BEHAVIOR),
    PropertyMeta::new("X", PropKind::F32, "0", "Not used: the ribbon lays its elements out.", ).category(LAYOUT).hidden(),
    PropertyMeta::new("Y", PropKind::F32, "0", "Not used: the ribbon lays its elements out.").category(LAYOUT).hidden(),
    PropertyMeta::new("Dock", PropKind::Enum(&["None", "Top", "Bottom", "Left", "Right", "Fill"]), "None", "Not used: the ribbon lays its elements out.").category(LAYOUT).hidden(),
    PropertyMeta::new("Anchor", PropKind::String, "Top, Left", "Not used: the ribbon lays its elements out.").category(LAYOUT).hidden(),
    PropertyMeta::new("Margin", PropKind::String, "0, 0, 0, 0", "Not used: the ribbon lays its elements out.").category(LAYOUT).hidden(),
    PropertyMeta::new("TabIndex", PropKind::F32, "0", "Not used: the ribbon has its own keyboard (KeyTips).").category(BEHAVIOR).hidden(),
    // What `Control` has that means nothing for a ribbon element: the ribbon paints, sizes and
    // routes its elements itself (the tooltip is the ScreenTip).
    not_used("Width"),
    not_used("Height"),
    not_used("Padding"),
    not_used("MinimumSize"),
    not_used("MaximumSize"),
    not_used("AutoSize"),
    not_used("AutoSizeMode"),
    not_used("BackColor"),
    not_used("ForeColor"),
    not_used("Font"),
    not_used("Cursor"),
    not_used("RightToLeft"),
    not_used("BackgroundImage"),
    not_used("BackgroundImageLayout"),
    not_used("TabStop"),
    not_used("ContextMenu"),
    not_used("AllowDrop"),
    not_used("UseWaitCursor"),
    not_used("ToolTip"),
    not_used("CausesValidation"),
    not_used("TitleBar.Region"),
    not_used("TitleBar.Drag"),
    not_used("ActionBar.Region"),
    not_used("Stack.Fill"),
    not_used("TableLayoutPanel.Row"),
    not_used("TableLayoutPanel.Column"),
    not_used("TableLayoutPanel.RowSpan"),
    not_used("TableLayoutPanel.ColumnSpan"),
];

/// A `Control` property a ribbon element hides from the Properties window.
const fn not_used(name: &'static str) -> PropertyMeta {
    PropertyMeta::new(name, PropKind::String, "", "Not used by ribbon elements.").category(LAYOUT).hidden()
}

/// What stands in a group, a quick access toolbar or a menu (`RibbonItem`).
pub const RIBBON_ITEM_PROPERTIES: &[PropertyMeta] = &[
    PropertyMeta::new("Size", PropKind::Enum(&["Small", "Large"]), "Small", "Large: the icon over the label, a column of its own. Small: stacked three to a column.").category(APPEARANCE),
];

/// The Properties window editor of every icon property of the family (`SmallIcon`, `LargeIcon`,
/// `Icon`, `Command`'s icons): the one place to switch to the shared icon property kind.
pub const ICON_EDITOR: &str = "icon";

/// The layout properties a ribbon element must not set (the validator's warning).
pub const HIDDEN_LAYOUT: &[&str] = &["X", "Y", "Dock", "Anchor", "Margin", "TabIndex"];

/// A group's `SizeDefinition` values (`RIBBON.md` §5): `Auto`, `Custom` (its
/// `<RibbonGroup.SizeDefinitions>`) and the Win32 templates — the Properties window's drop-down.
/// Kept equal to `kubuno_desktop_ui::ribbon::scaling::Template::names()` by a test.
pub const SIZE_DEFINITIONS: &[&str] = &[
    "Auto", "Custom", "OneButton", "TwoButtons", "ThreeButtons", "ThreeButtons-OneBigAndTwoSmall", "ThreeButtonsAndOneCheckBox", "FourButtons", "FiveButtons", "SixButtons",
    "SevenButtons", "EightButtons", "NineButtons", "TenButtons", "ElevenButtons", "BigButtonsAndSmallButtonsOrInputs", "InRibbonGalleryAndBigButton",
    "InRibbonGalleryAndThreeButtons", "ButtonGroups", "ButtonGroupsAndInputs",
];

/// The elements that show a smart tag when selected in the designer (`<Ribbon>` itself too).
pub(crate) const SMART_TAGGED: &[&str] = &[
    "RibbonTab", "RibbonGroup", "RibbonButton", "RibbonToggleButton", "RibbonMenuButton", "RibbonSplitButton", "RibbonGallery", "RibbonComboBox",
];

/// The item elements a group, a box, a control group and the quick access toolbar take.
pub(crate) const ITEMS: &[&str] = &[
    "RibbonButton",
    "RibbonToggleButton",
    "RibbonRadioButton",
    "RibbonMenuButton",
    "RibbonSplitButton",
    "RibbonColorPicker",
    "RibbonCheckBox",
    "RibbonComboBox",
    "RibbonTextBox",
    "RibbonNumericField",
    "RibbonGallery",
    "RibbonLabel",
    "RibbonSeparator",
];

/// A group's children: the items, the joined control groups and the boxes.
pub(crate) const GROUP_CHILDREN: &[&str] = &[
    "RibbonButton",
    "RibbonToggleButton",
    "RibbonRadioButton",
    "RibbonMenuButton",
    "RibbonSplitButton",
    "RibbonColorPicker",
    "RibbonCheckBox",
    "RibbonComboBox",
    "RibbonTextBox",
    "RibbonNumericField",
    "RibbonGallery",
    "RibbonLabel",
    "RibbonSeparator",
    "RibbonControlGroup",
    "RibbonBox",
];

/// The content of a menu: menu items, separators, in-menu galleries, check boxes, labels.
pub(crate) const MENU: &[&str] = &["RibbonMenuItem", "RibbonSplitMenuItem", "RibbonSeparator", "RibbonGallery", "RibbonCheckBox", "RibbonLabel"];

pub(crate) fn nested(props: &Props<'_>, parents: &str) -> BuildError {
    let name = props.element().name().unwrap_or_default();
    BuildError::new(format!("`<{name}>` is only valid inside {parents}"), props.element().name_range())
}

// ═════════════════════════════════════════════════════════════════════════════
// The elements
// ═════════════════════════════════════════════════════════════════════════════

component! {
    mod_name: ribbon,
    name: "Ribbon",
    doc: "The Office ribbon: tabs of groups of commands, contextual tabs, a quick access toolbar and a Backstage (the « Fichier » tab). Dock it at the top of the view.",
    ctor: kubuno_desktop_ui::ribbon::Ribbon::new(Vec::new(), kubuno_desktop_ui::ribbon::RibbonTheme::default()),
    children: ChildrenModel::List(&["RibbonTab", "RibbonContextualTabGroup", "RibbonQuickAccessToolbar", "RibbonBackstage"]),
    default_event: "OnSelectedTabChanged",
    props: [
        PropertyMeta::new("Tone", PropKind::Enum(&["Documents", "Spreadsheet", "Presentation", "Projects", "Diagrams", "Data", "Maths", "Whiteboard", "Plain"]), "Documents", "The colour of the tab strip: the tone of an Office editor, or Plain for the workspace look.").category("Appearance"),
        PropertyMeta::new("OwnsCaption", PropKind::Bool, "true", "The window's title bar takes the tab strip's colour, so both read as one band.").category("Appearance"),
        PropertyMeta::new("SelectedTab", PropKind::String, "", "The active tab: its x:Name (else its header).").category("Behavior").bindable(),
        PropertyMeta::new("IsMinimized", PropKind::Bool, "false", "Only the tab strip shows; clicking a tab shows its groups over the page (Ctrl+F1).").category("Behavior").bindable(),
        PropertyMeta::new("QatPosition", PropKind::Enum(&["InTabStrip", "BelowRibbon"]), "InTabStrip", "Where the quick access toolbar sits.").category("Appearance"),
        PropertyMeta::new("DisplayMode", PropKind::Enum(&["Classic", "Simplified"]), "Classic", "Classic: tabs over a row of groups. Simplified: one row of commands, the rest in an overflow menu.").category("Appearance"),
        PropertyMeta::new("ShowKeyTips", PropKind::Bool, "true", "Alt shows the KeyTips of the tabs and commands.").category("Behavior"),
        PropertyMeta::new("QatSettingsKey", PropKind::String, "", "Saves the commands the user adds to the quick access toolbar under this name (per user, per application) and restores them at the next start. Empty: not saved.").category("Behavior"),
        PropertyMeta::new("PreviewWidth", PropKind::F32, "0", "In the designer only: lays the ribbon out at this width, to preview how its groups shrink (0: the view's width).").category("Design").design_time(),
    ],
    events: [
        EventMeta::new("OnSelectedTabChanged", "Occurs when another tab becomes active (e.new is its name).").category(crate::registry::EventCategory::PropertyChanged).args::<crate::events::TextChangedEventArgs>(),
        EventMeta::new("OnIsMinimizedChanged", "Occurs when the ribbon is minimized or restored.").category(crate::registry::EventCategory::PropertyChanged).args::<crate::events::CheckedChangedEventArgs>(),
        EventMeta::new("OnQatChanged", "Occurs when the user adds a command to the quick access toolbar or removes one (e.new: their names, comma-separated).").category(crate::registry::EventCategory::PropertyChanged).args::<crate::events::TextChangedEventArgs>(),
    ],
    smoke: |r| { r },
    build: |props, cx| { crate::registry::families::ribbon::build_ribbon(props, cx) },
}

component! {
    mod_name: ribbon_tab,
    name: "RibbonTab",
    doc: "A tab of a ribbon: its groups of commands. A <RibbonTab.ScalingPolicy> child (Scale steps: Group, Size, Ideal) orders how its groups shrink.",
    ctor: kubuno_desktop_ui::ribbon::RibbonTab::new("home", "Accueil", Vec::new()),
    children: ChildrenModel::List(&["RibbonGroup"]),
    props: [
        PropertyMeta::new("Header", PropKind::String, "", "Text of the tab.").category("Appearance").localizable().bindable(),
    ],
    events: [],
    smoke: |t| { t },
    build: |props, _cx| { Err(crate::registry::families::ribbon::nested(props, "`<Ribbon>`, `<RibbonContextualTabGroup>`")) },
}

component! {
    mod_name: ribbon_contextual_tab_group,
    name: "RibbonContextualTabGroup",
    doc: "Tabs shown only in a context (a table, an image is selected), under a coloured header. Bind its Visible property.",
    ctor: (),
    children: ChildrenModel::List(&["RibbonTab"]),
    props: [
        PropertyMeta::new("Header", PropKind::String, "", "Text of the coloured header above its tabs.").category("Appearance").localizable().bindable(),
        PropertyMeta::new("Color", PropKind::String, "#107C41", "Colour of the header and of its tabs' top rule.").category("Appearance").editor("color").type_converter("Color"),
    ],
    events: [],
    smoke: |t| { t },
    build: |props, _cx| { Err(crate::registry::families::ribbon::nested(props, "`<Ribbon>`")) },
}

component! {
    mod_name: ribbon_group,
    name: "RibbonGroup",
    doc: "A group of commands of a tab, its header below them. It folds into a button when the window is too narrow.",
    ctor: kubuno_desktop_ui::ribbon::RibbonGroup::new("g", "Groupe", Vec::new()),
    children: ChildrenModel::List(crate::registry::families::ribbon::GROUP_CHILDREN),
    default_event: "OnDialogLauncherClick",
    props: [
        PropertyMeta::new("Header", PropKind::String, "", "Text below the group.").category("Appearance").localizable().bindable(),
        PropertyMeta::new("Icon", PropKind::String, "", "Icon of the button the group folds into (else its first command's).").category("Appearance").editor(crate::registry::families::ribbon::ICON_EDITOR),
        PropertyMeta::new("SizeDefinition", PropKind::Enum(crate::registry::families::ribbon::SIZE_DEFINITIONS), "Auto", "How its commands look at each size: Auto, or a template (OneButton, ThreeButtons-OneBigAndTwoSmall, FourButtons…).").category("Layout"),
        PropertyMeta::new("ShowDialogLauncher", PropKind::Bool, "false", "Shows the small launcher at the bottom right of the group.").category("Appearance"),
        PropertyMeta::new("DialogLauncherCommand", PropKind::String, "", "The Command the launcher runs.").category("Behavior").editor("reference:Command"),
    ],
    events: [
        EventMeta::new("OnDialogLauncherClick", "Occurs when the dialog launcher of the group is clicked.").args::<crate::events::MouseEventArgs>(),
    ],
    smoke: |g| { g },
    build: |props, _cx| { Err(crate::registry::families::ribbon::nested(props, "`<RibbonTab>`")) },
}

component! {
    mod_name: ribbon_control_group,
    name: "RibbonControlGroup",
    doc: "Buttons joined in one row (bold, italic, underline…), one row of a group.",
    ctor: (),
    children: ChildrenModel::List(crate::registry::families::ribbon::ITEMS),
    props: [],
    events: [],
    smoke: |g| { g },
    build: |props, _cx| { Err(crate::registry::families::ribbon::nested(props, "`<RibbonGroup>`")) },
}

component! {
    mod_name: ribbon_box,
    name: "RibbonBox",
    doc: "A row (or a column) of commands inside a group: the font and size combo boxes on one line.",
    ctor: (),
    children: ChildrenModel::List(crate::registry::families::ribbon::ITEMS),
    props: [
        PropertyMeta::new("Orientation", PropKind::Enum(&["Horizontal", "Vertical"]), "Horizontal", "Horizontal: a row, one line of the group. Vertical: a column of its own.").category("Layout"),
    ],
    events: [],
    smoke: |b| { b },
    build: |props, _cx| { Err(crate::registry::families::ribbon::nested(props, "`<RibbonGroup>`")) },
}

component! {
    mod_name: ribbon_qat,
    name: "RibbonQuickAccessToolbar",
    doc: "The quick access toolbar: small commands always shown (save, undo, redo), after « Fichier » or below the ribbon.",
    ctor: (),
    children: ChildrenModel::List(crate::registry::families::ribbon::ITEMS),
    props: [
        PropertyMeta::new("Position", PropKind::Enum(&["InTabStrip", "BelowRibbon"]), "InTabStrip", "Where it sits (the Ribbon's QatPosition wins when set).").category("Appearance"),
    ],
    events: [],
    smoke: |q| { q },
    build: |props, _cx| { Err(crate::registry::families::ribbon::nested(props, "`<Ribbon>`")) },
}

component! {
    mod_name: ribbon_backstage,
    name: "RibbonBackstage",
    doc: "The Backstage: the « Fichier » tab and the view it opens over the page — tabs (each showing a view) and buttons.",
    ctor: kubuno_desktop_ui::ribbon::Backstage::new(Vec::new()),
    children: ChildrenModel::List(&["BackstageTab", "BackstageButton", "BackstageSeparator"]),
    props: [
        PropertyMeta::new("Header", PropKind::String, "Fichier", "Text of the tab that opens it.").category("Appearance").localizable().bindable(),
    ],
    events: [],
    smoke: |b| { b },
    build: |props, _cx| { Err(crate::registry::families::ribbon::nested(props, "`<Ribbon>`")) },
}

component! {
    mod_name: backstage_tab,
    name: "BackstageTab",
    doc: "A tab of the Backstage: a row of its rail showing a view (its one child) on the right.",
    ctor: kubuno_desktop_ui::ribbon::BackstageSection::view("info", "Informations", "Info"),
    children: ChildrenModel::SingleWidget,
    props: [
        PropertyMeta::new("Header", PropKind::String, "", "Text of the row.").category("Appearance").localizable().bindable(),
        PropertyMeta::new("Icon", PropKind::String, "", "Icon of the row.").category("Appearance").editor(crate::registry::families::ribbon::ICON_EDITOR),
    ],
    events: [],
    smoke: |t| { t },
    build: |props, _cx| { Err(crate::registry::families::ribbon::nested(props, "`<RibbonBackstage>`")) },
}

component! {
    mod_name: backstage_button,
    name: "BackstageButton",
    doc: "A command of the Backstage's rail (Imprimer, Fermer…).",
    ctor: kubuno_desktop_ui::ribbon::BackstageSection::action("close", "Fermer", "X"),
    children: ChildrenModel::None,
    props: [],
    events: [
        EventMeta::new("OnClick", "Occurs when the row is clicked.").args::<crate::events::MouseEventArgs>(),
    ],
    smoke: |b| { b },
    build: |props, _cx| { Err(crate::registry::families::ribbon::nested(props, "`<RibbonBackstage>`")) },
}

component! {
    mod_name: backstage_separator,
    name: "BackstageSeparator",
    doc: "A rule between two rows of the Backstage's rail.",
    ctor: (),
    children: ChildrenModel::None,
    props: [],
    events: [],
    smoke: |s| { s },
    build: |props, _cx| { Err(crate::registry::families::ribbon::nested(props, "`<RibbonBackstage>`")) },
}

component! {
    mod_name: ribbon_button,
    name: "RibbonButton",
    doc: "A command button of a ribbon: large (icon over label) or small (stacked three to a column).",
    ctor: kubuno_desktop_ui::ribbon::RibbonItem::button("b", "Bouton", "Copy"),
    children: ChildrenModel::None,
    props: [],
    events: [
        EventMeta::new("OnClick", "Occurs when the button is clicked (then its Command runs, unless the handler marks it handled).").args::<crate::events::MouseEventArgs>(),
    ],
    smoke: |b| { b },
    build: |props, _cx| { Err(crate::registry::families::ribbon::nested(props, "a `<RibbonGroup>`, a `<RibbonBox>`, a `<RibbonControlGroup>` or the `<RibbonQuickAccessToolbar>`")) },
}

component! {
    mod_name: ribbon_toggle_button,
    name: "RibbonToggleButton",
    doc: "A button that stays pressed (bold, italic): its Checked state.",
    ctor: kubuno_desktop_ui::ribbon::RibbonItem::toggle("t", "", "Bold", false),
    children: ChildrenModel::None,
    default_event: "OnCheckedChanged",
    props: [
        PropertyMeta::new("Checked", PropKind::Bool, "false", "Whether it is pressed.").category("Appearance").bindable(),
    ],
    events: [
        EventMeta::new("OnClick", "Occurs when the button is clicked.").args::<crate::events::MouseEventArgs>(),
        EventMeta::new("OnCheckedChanged", "Occurs when the button is pressed or released.").category(crate::registry::EventCategory::PropertyChanged).args::<crate::events::CheckedChangedEventArgs>(),
    ],
    smoke: |t| { t },
    build: |props, _cx| { Err(crate::registry::families::ribbon::nested(props, "a group")) },
}

component! {
    mod_name: ribbon_radio_button,
    name: "RibbonRadioButton",
    doc: "A toggle of a set (align left, centre, right): pressing it releases the others of its GroupName.",
    ctor: kubuno_desktop_ui::ribbon::RibbonItem::toggle("r", "", "AlignLeft", false),
    children: ChildrenModel::None,
    default_event: "OnCheckedChanged",
    props: [
        PropertyMeta::new("GroupName", PropKind::String, "", "The set it belongs to (the radio buttons of a ribbon with the same GroupName).").category("Behavior"),
        PropertyMeta::new("Checked", PropKind::Bool, "false", "Whether it is the pressed one.").category("Appearance").bindable(),
    ],
    events: [
        EventMeta::new("OnClick", "Occurs when the button is clicked.").args::<crate::events::MouseEventArgs>(),
        EventMeta::new("OnCheckedChanged", "Occurs when the button is pressed or released.").category(crate::registry::EventCategory::PropertyChanged).args::<crate::events::CheckedChangedEventArgs>(),
    ],
    smoke: |t| { t },
    build: |props, _cx| { Err(crate::registry::families::ribbon::nested(props, "a group")) },
}

component! {
    mod_name: ribbon_menu_button,
    name: "RibbonMenuButton",
    doc: "A button whose whole surface opens a menu (its RibbonMenuItem children).",
    ctor: kubuno_desktop_ui::ribbon::RibbonItem::menu("m", "Menu", "List", Vec::new()),
    children: ChildrenModel::List(crate::registry::families::ribbon::MENU),
    default_event: "OnDropDownOpening",
    props: [],
    events: [
        EventMeta::new("OnDropDownOpening", "Occurs when the menu opens.").category(crate::registry::EventCategory::Behavior),
    ],
    smoke: |m| { m },
    build: |props, _cx| { Err(crate::registry::families::ribbon::nested(props, "a group")) },
}

component! {
    mod_name: ribbon_split_button,
    name: "RibbonSplitButton",
    doc: "A button with its own action and a chevron opening a menu (its RibbonMenuItem children).",
    ctor: kubuno_desktop_ui::ribbon::RibbonItem::split("s", "Split", "Link", Vec::new()),
    children: ChildrenModel::List(crate::registry::families::ribbon::MENU),
    props: [],
    events: [
        EventMeta::new("OnClick", "Occurs when the main part of the button is clicked.").args::<crate::events::MouseEventArgs>(),
        EventMeta::new("OnDropDownOpening", "Occurs when the menu opens.").category(crate::registry::EventCategory::Behavior),
    ],
    smoke: |s| { s },
    build: |props, _cx| { Err(crate::registry::families::ribbon::nested(props, "a group")) },
}

component! {
    mod_name: ribbon_color_picker,
    name: "RibbonColorPicker",
    doc: "A split button whose menu is a colour palette (font colour, highlight): its colour is shown under its icon.",
    ctor: kubuno_desktop_ui::ribbon::RibbonItem::color_picker("c", "", "Baseline", None),
    children: ChildrenModel::List(crate::registry::families::ribbon::MENU),
    default_event: "OnSelectedColorChanged",
    props: [
        PropertyMeta::new("SelectedColor", PropKind::String, "", "The colour (#RRGGBB); empty for Automatic.").category("Appearance").bindable().editor("color").type_converter("Color"),
    ],
    events: [
        EventMeta::new("OnClick", "Occurs when the main part is clicked (apply the current colour).").args::<crate::events::MouseEventArgs>(),
        EventMeta::new("OnSelectedColorChanged", "Occurs when a colour is picked (e.new: #RRGGBB, empty for Automatic).").category(crate::registry::EventCategory::PropertyChanged).args::<crate::events::TextChangedEventArgs>(),
    ],
    smoke: |c| { c },
    build: |props, _cx| { Err(crate::registry::families::ribbon::nested(props, "a group")) },
}

component! {
    mod_name: ribbon_menu_item,
    name: "RibbonMenuItem",
    doc: "An entry of a ribbon menu; its own RibbonMenuItem children make a sub-menu.",
    ctor: kubuno_desktop_ui::ribbon::RibbonItem::entry("e", "Entrée"),
    children: ChildrenModel::List(crate::registry::families::ribbon::MENU),
    props: [
        PropertyMeta::new("Checked", PropKind::Bool, "false", "Shows a tick before the entry.").category("Appearance").bindable(),
    ],
    events: [
        EventMeta::new("OnClick", "Occurs when the entry is chosen.").args::<crate::events::MouseEventArgs>(),
    ],
    smoke: |e| { e },
    build: |props, _cx| { Err(crate::registry::families::ribbon::nested(props, "a menu")) },
}

component! {
    mod_name: ribbon_split_menu_item,
    name: "RibbonSplitMenuItem",
    doc: "An entry of a ribbon menu that runs its own command and opens a sub-menu (its children).",
    ctor: kubuno_desktop_ui::ribbon::RibbonItem::entry("e", "Entrée"),
    children: ChildrenModel::List(crate::registry::families::ribbon::MENU),
    props: [
        PropertyMeta::new("Checked", PropKind::Bool, "false", "Shows a tick before the entry.").category("Appearance").bindable(),
    ],
    events: [
        EventMeta::new("OnClick", "Occurs when the entry is chosen.").args::<crate::events::MouseEventArgs>(),
    ],
    smoke: |e| { e },
    build: |props, _cx| { Err(crate::registry::families::ribbon::nested(props, "a menu")) },
}

component! {
    mod_name: ribbon_check_box,
    name: "RibbonCheckBox",
    doc: "A check box of a ribbon (Règle, Quadrillage…).",
    ctor: kubuno_desktop_ui::ribbon::RibbonItem::check_box("k", "Règle", false),
    children: ChildrenModel::None,
    default_event: "OnCheckedChanged",
    props: [
        PropertyMeta::new("Checked", PropKind::Bool, "false", "Whether it is checked.").category("Appearance").bindable(),
    ],
    events: [
        EventMeta::new("OnCheckedChanged", "Occurs when it is checked or unchecked.").category(crate::registry::EventCategory::PropertyChanged).args::<crate::events::CheckedChangedEventArgs>(),
    ],
    smoke: |k| { k },
    build: |props, _cx| { Err(crate::registry::families::ribbon::nested(props, "a group or a menu")) },
}

component! {
    mod_name: ribbon_combo_box,
    name: "RibbonComboBox",
    doc: "A drop-down list of a ribbon (font, size); IsEditable lets the user type a value.",
    ctor: kubuno_desktop_ui::ribbon::RibbonItem::combo_box("f", Vec::new(), "", 120.0, true),
    children: ChildrenModel::List(&["Option"]),
    default_event: "OnSelectedValueChanged",
    props: [
        PropertyMeta::new("IsEditable", PropKind::Bool, "false", "The value can be typed as well as picked.").category("Behavior"),
        PropertyMeta::new("Width", PropKind::F32, "120", "Width of the field, in pixels.").category("Layout"),
        PropertyMeta::new("SelectedValue", PropKind::String, "", "The value picked or typed.").category("Data").bindable(),
        PropertyMeta::new("ItemsSource", PropKind::String, "", "The choices, from a list (field Text, or Value and Text), instead of the Option children.").category("Data").bindable().editor("list"),
    ],
    events: [
        EventMeta::new("OnSelectedValueChanged", "Occurs when a value is picked or typed (Enter).").category(crate::registry::EventCategory::PropertyChanged).args::<crate::events::TextChangedEventArgs>(),
    ],
    smoke: |c| { c },
    build: |props, _cx| { Err(crate::registry::families::ribbon::nested(props, "a group or a box")) },
}

component! {
    mod_name: ribbon_text_box,
    name: "RibbonTextBox",
    doc: "A text field of a ribbon.",
    ctor: kubuno_desktop_ui::ribbon::RibbonItem::text_box("t", "", 120.0),
    children: ChildrenModel::None,
    default_event: "OnTextChanged",
    props: [
        PropertyMeta::new("Width", PropKind::F32, "120", "Width of the field, in pixels.").category("Layout"),
        PropertyMeta::new("Text", PropKind::String, "", "The text.").category("Data").bindable(),
    ],
    events: [
        EventMeta::new("OnTextChanged", "Occurs when the text is committed (Enter, or leaving the field).").category(crate::registry::EventCategory::PropertyChanged).args::<crate::events::TextChangedEventArgs>(),
    ],
    smoke: |t| { t },
    build: |props, _cx| { Err(crate::registry::families::ribbon::nested(props, "a group or a box")) },
}

component! {
    mod_name: ribbon_numeric_field,
    name: "RibbonNumericField",
    doc: "A number with up and down arrows (a spacing, an indent).",
    ctor: kubuno_desktop_ui::ribbon::RibbonItem::numeric("n", 0.0, 0.0, 100.0, 1.0, 64.0),
    children: ChildrenModel::None,
    default_event: "OnValueChanged",
    props: [
        PropertyMeta::new("Value", PropKind::F32, "0", "The number.").category("Data").bindable(),
        PropertyMeta::new("Minimum", PropKind::F32, "0", "The smallest value.").category("Behavior"),
        PropertyMeta::new("Maximum", PropKind::F32, "100", "The largest value.").category("Behavior"),
        PropertyMeta::new("Increment", PropKind::F32, "1", "What an arrow adds or removes.").category("Behavior"),
        PropertyMeta::new("Width", PropKind::F32, "64", "Width of the field, in pixels.").category("Layout"),
    ],
    events: [
        EventMeta::new("OnValueChanged", "Occurs when the number changes (an arrow, or a value typed and committed).").category(crate::registry::EventCategory::PropertyChanged).args::<crate::events::NumericValueChangedEventArgs>(),
    ],
    smoke: |n| { n },
    build: |props, _cx| { Err(crate::registry::families::ribbon::nested(props, "a group or a box")) },
}

component! {
    mod_name: ribbon_gallery,
    name: "RibbonGallery",
    doc: "A gallery of choices (styles, colours, tables): a grid in the ribbon with a « more » button, a drop-down, or a grid in a menu.",
    ctor: kubuno_desktop_ui::ribbon::RibbonItem::gallery("g", Vec::new()),
    children: ChildrenModel::List(&["RibbonGalleryCategory", "RibbonGalleryItem"]),
    default_event: "OnItemClick",
    props: [
        PropertyMeta::new("Display", PropKind::Enum(&["InRibbon", "DropDown", "InMenu", "Inline"]), "InRibbon", "InRibbon: a grid in the group. DropDown: a button opening the grid. InMenu: a grid inside a menu. Inline: one row of labelled chips, each as wide as its text (the web editors' style strip).").category("Appearance"),
        PropertyMeta::new("Rows", PropKind::F32, "1", "Rows shown in the ribbon.").category("Layout"),
        PropertyMeta::new("Columns", PropKind::F32, "4", "Columns shown in the ribbon and in the drop-down.").category("Layout"),
        PropertyMeta::new("ItemWidth", PropKind::F32, "72", "Width of a cell, in pixels.").category("Layout"),
        PropertyMeta::new("ItemHeight", PropKind::F32, "56", "Height of a cell, in pixels.").category("Layout"),
        PropertyMeta::new("SelectedValue", PropKind::String, "", "The Value of the selected item.").category("Data").bindable(),
    ],
    events: [
        EventMeta::new("OnItemClick", "Occurs when an item is clicked (e.new is its Value).").args::<crate::events::TextChangedEventArgs>(),
    ],
    smoke: |g| { g },
    build: |props, _cx| { Err(crate::registry::families::ribbon::nested(props, "a group or a menu")) },
}

component! {
    mod_name: ribbon_gallery_category,
    name: "RibbonGalleryCategory",
    doc: "A category of a gallery: its items under a header in the drop-down.",
    ctor: (),
    children: ChildrenModel::List(&["RibbonGalleryItem"]),
    props: [
        PropertyMeta::new("Header", PropKind::String, "", "Text of the category's header.").category("Appearance").localizable().bindable(),
    ],
    events: [],
    smoke: |c| { c },
    build: |props, _cx| { Err(crate::registry::families::ribbon::nested(props, "`<RibbonGallery>`")) },
}

component! {
    mod_name: ribbon_gallery_item,
    name: "RibbonGalleryItem",
    doc: "A choice of a gallery: a value, a label, an icon or a colour.",
    ctor: kubuno_desktop_ui::ribbon::RibbonOption::new("normal", "Normal"),
    children: ChildrenModel::None,
    props: [
        PropertyMeta::new("Value", PropKind::String, "", "What the gallery's SelectedValue becomes when it is clicked (else its label).").category("Data"),
        PropertyMeta::new("Color", PropKind::String, "", "A colour previewed in the cell.").category("Appearance").editor("color").type_converter("Color"),
    ],
    events: [
        EventMeta::new("OnClick", "Occurs when the item is clicked.").args::<crate::events::MouseEventArgs>(),
    ],
    smoke: |i| { i },
    build: |props, _cx| { Err(crate::registry::families::ribbon::nested(props, "`<RibbonGallery>`")) },
}

component! {
    mod_name: ribbon_label,
    name: "RibbonLabel",
    doc: "A static text of a ribbon group or menu (a menu's section header).",
    ctor: kubuno_desktop_ui::ribbon::RibbonItem::text_label("l", "Texte"),
    children: ChildrenModel::None,
    props: [],
    events: [],
    smoke: |l| { l },
    build: |props, _cx| { Err(crate::registry::families::ribbon::nested(props, "a group or a menu")) },
}

component! {
    mod_name: ribbon_separator,
    name: "RibbonSeparator",
    doc: "A separator between the commands of a group or the entries of a menu.",
    ctor: kubuno_desktop_ui::ribbon::RibbonItem::separator("s"),
    children: ChildrenModel::None,
    props: [],
    events: [],
    smoke: |s| { s },
    build: |props, _cx| { Err(crate::registry::families::ribbon::nested(props, "a group or a menu")) },
}

component! {
    mod_name: command,
    name: "Command",
    doc: "A command of the view (component tray): its label, icons, shortcut, enabled and checked state, and its Execute event, shared by every ribbon element, button or menu item whose Command names it.",
    ctor: (),
    children: ChildrenModel::None,
    default_event: "OnExecute",
    props: [
        PropertyMeta::new("Label", PropKind::String, "", "Text of the elements that run it.").category("Appearance").localizable().bindable(),
        PropertyMeta::new("SmallIcon", PropKind::String, "", "Small icon of the elements that run it.").category("Appearance").editor(crate::registry::families::ribbon::ICON_EDITOR),
        PropertyMeta::new("LargeIcon", PropKind::String, "", "Large icon of the elements that run it.").category("Appearance").editor(crate::registry::families::ribbon::ICON_EDITOR),
        PropertyMeta::new("Shortcut", PropKind::String, "", "Keyboard shortcut running it anywhere in the view (Ctrl+B, Ctrl+Shift+S, F5…).").category("Behavior"),
        PropertyMeta::new("KeyTip", PropKind::String, "", "KeyTip of the elements that run it.").category("Behavior"),
        PropertyMeta::new("ScreenTipTitle", PropKind::String, "", "Title of their tooltip.").category("Appearance").localizable(),
        PropertyMeta::new("ScreenTipText", PropKind::String, "", "Description in their tooltip.").category("Appearance").localizable(),
        PropertyMeta::new("Enabled", PropKind::Bool, "true", "Whether it can run: the elements that run it are greyed out otherwise.").category("Behavior").bindable(),
        PropertyMeta::new("Checked", PropKind::Bool, "false", "Its checked state (bold on): the toggles that run it show it.").category("Behavior").bindable(),
        PropertyMeta::new("IsCheckable", PropKind::Bool, "false", "Running it switches Checked.").category("Behavior"),
    ],
    events: [
        EventMeta::new("OnExecute", "Occurs when the command runs: an element running it was clicked, or its shortcut pressed.").category(crate::registry::EventCategory::Action),
        EventMeta::new("OnQueryStatus", "Occurs before the elements running it are drawn: update Enabled and Checked here (bindings usually suffice).").category(crate::registry::EventCategory::Behavior),
    ],
    smoke: |c| { c },
    build: |_props, _cx| { Ok(Box::new(crate::node::custom::NonVisualNode) as Box<dyn ViewNode>) },
}

/// The French documentation of the family (`registry::docs_fr`), keyed `Element` or
/// `Element.Member` (`RibbonControl.Label` for a level's property).
pub fn french(key: &str) -> Option<&'static str> {
    Some(match key {
        "RibbonControl.Label" => "Texte de l'élément (l'en-tête pour un onglet ou un groupe). Vide : le libellé de la commande.",
        "RibbonControl.SmallIcon" => "Icône de l'élément en petit : un nom du jeu d'icônes Kubuno (Bold, ClipboardPaste…).",
        "RibbonControl.LargeIcon" => "Icône de l'élément en grand (32 pixels). Vide : la petite icône.",
        "RibbonControl.ScreenTipTitle" => "Titre de l'info-bulle. Vide : le libellé.",
        "RibbonControl.ScreenTipText" => "Description affichée dans l'info-bulle.",
        "RibbonControl.KeyTip" => "Lettres affichées quand on appuie sur Alt ; les taper exécute l'élément. Vide : attribuées automatiquement.",
        "RibbonControl.Command" => "La commande (Command) de la vue qu'exécute l'élément : son libellé, ses icônes, son raccourci, son état et son événement Execute.",
        "RibbonControl.CommandParameter" => "Valeur transmise à la commande quand l'élément l'exécute.",
        "RibbonControl.ShowLabel" => "Indique si le libellé est affiché à côté de l'icône (il reste dans l'info-bulle).",
        "RibbonControl.CanAddToQat" => "Indique si un clic droit propose d'ajouter l'élément à la barre d'outils Accès rapide.",
        "RibbonControl.X" | "RibbonControl.Y" | "RibbonControl.Dock" | "RibbonControl.Anchor" | "RibbonControl.Margin" => "Inutilisé : le ruban dispose lui-même ses éléments.",
        "RibbonControl.TabIndex" => "Inutilisé : le ruban a son propre clavier (les KeyTips).",
        "RibbonControl.Width" | "RibbonControl.Height" | "RibbonControl.Padding" | "RibbonControl.MinimumSize" | "RibbonControl.MaximumSize" | "RibbonControl.AutoSize"
        | "RibbonControl.AutoSizeMode" | "RibbonControl.BackColor" | "RibbonControl.ForeColor" | "RibbonControl.Font" | "RibbonControl.Cursor" | "RibbonControl.RightToLeft"
        | "RibbonControl.BackgroundImage" | "RibbonControl.BackgroundImageLayout" | "RibbonControl.TabStop" | "RibbonControl.ContextMenu" | "RibbonControl.AllowDrop"
        | "RibbonControl.UseWaitCursor" | "RibbonControl.ToolTip" | "RibbonControl.CausesValidation" | "RibbonControl.TitleBar.Region" | "RibbonControl.TitleBar.Drag"
        | "RibbonControl.ActionBar.Region" | "RibbonControl.Stack.Fill" | "RibbonControl.TableLayoutPanel.Row" | "RibbonControl.TableLayoutPanel.Column"
        | "RibbonControl.TableLayoutPanel.RowSpan" | "RibbonControl.TableLayoutPanel.ColumnSpan" => "Inutilisé par les éléments du ruban.",
        "View.TitleBarFollowsRibbon" => "Quand la vue a un ruban et pas de couleur de barre de titre à elle, la barre de titre prend la couleur de la bande d'onglets du ruban, pour ne former qu'une bande.",
        "RibbonItem.Size" => "Large : l'icône au-dessus du libellé, dans une colonne à part. Small : empilé par trois dans une colonne.",
        "Ribbon" => "Le ruban Office : des onglets de groupes de commandes, des onglets contextuels, une barre d'outils Accès rapide et un Backstage (l'onglet « Fichier »). Ancrez-le en haut de la vue.",
        "Ribbon.Tone" => "Couleur de la bande d'onglets : la teinte d'un éditeur Office, ou Plain pour l'aspect de l'espace de travail.",
        "Ribbon.OwnsCaption" => "La barre de titre de la fenêtre prend la couleur de la bande d'onglets, pour ne former qu'une bande.",
        "Ribbon.SelectedTab" => "L'onglet actif : son x:Name (sinon son en-tête).",
        "Ribbon.IsMinimized" => "Seule la bande d'onglets est affichée ; un clic sur un onglet montre ses groupes par-dessus la page (Ctrl+F1).",
        "Ribbon.QatPosition" => "Emplacement de la barre d'outils Accès rapide.",
        "Ribbon.DisplayMode" => "Classic : des onglets au-dessus d'une rangée de groupes. Simplified : une seule rangée de commandes, le reste dans un menu de débordement.",
        "Ribbon.ShowKeyTips" => "Alt affiche les KeyTips des onglets et des commandes.",
        "Ribbon.PreviewWidth" => "Dans le concepteur seulement : dispose le ruban à cette largeur, pour voir comment ses groupes se réduisent (0 : la largeur de la vue).",
        "Ribbon.QatSettingsKey" => "Enregistre sous ce nom (par utilisateur et par application) les commandes que l'utilisateur ajoute à la barre d'outils Accès rapide, et les rétablit au démarrage suivant. Vide : rien n'est enregistré.",
        "Ribbon.OnSelectedTabChanged" => "Se produit quand un autre onglet devient actif (e.new est son nom).",
        "Ribbon.OnIsMinimizedChanged" => "Se produit quand le ruban est réduit ou restauré.",
        "Ribbon.OnQatChanged" => "Se produit quand l'utilisateur ajoute une commande à la barre d'outils Accès rapide ou l'en retire (e.new : leurs noms, séparés par des virgules).",
        "RibbonTab" => "Un onglet du ruban : ses groupes de commandes. Un enfant <RibbonTab.ScalingPolicy> (étapes Scale : Group, Size, Ideal) ordonne la réduction de ses groupes.",
        "RibbonTab.Header" => "Texte de l'onglet.",
        "RibbonContextualTabGroup" => "Des onglets affichés seulement dans un contexte (un tableau, une image sélectionnés), sous un en-tête coloré. Liez sa propriété Visible.",
        "RibbonContextualTabGroup.Header" => "Texte de l'en-tête coloré au-dessus de ses onglets.",
        "RibbonContextualTabGroup.Color" => "Couleur de l'en-tête et du filet supérieur de ses onglets.",
        "RibbonGroup" => "Un groupe de commandes d'un onglet, son en-tête dessous. Il se replie en bouton quand la fenêtre est trop étroite.",
        "RibbonGroup.Header" => "Texte sous le groupe.",
        "RibbonGroup.Icon" => "Icône du bouton en lequel le groupe se replie (sinon celle de sa première commande).",
        "RibbonGroup.SizeDefinition" => "Aspect de ses commandes à chaque taille : Auto, ou un modèle (OneButton, ThreeButtons-OneBigAndTwoSmall, FourButtons…).",
        "RibbonGroup.ShowDialogLauncher" => "Affiche le petit lanceur en bas à droite du groupe.",
        "RibbonGroup.DialogLauncherCommand" => "La commande qu'exécute le lanceur.",
        "RibbonGroup.OnDialogLauncherClick" => "Se produit quand on clique sur le lanceur du groupe.",
        "RibbonControlGroup" => "Des boutons accolés sur une ligne (gras, italique, souligné…), une ligne d'un groupe.",
        "RibbonBox" => "Une ligne (ou une colonne) de commandes dans un groupe : les listes de police et de taille sur une ligne.",
        "RibbonBox.Orientation" => "Horizontal : une ligne du groupe. Vertical : une colonne à part.",
        "RibbonQuickAccessToolbar" => "La barre d'outils Accès rapide : de petites commandes toujours affichées (enregistrer, annuler, rétablir), après « Fichier » ou sous le ruban.",
        "RibbonQuickAccessToolbar.Position" => "Son emplacement (QatPosition du ruban l'emporte quand il est défini).",
        "RibbonBackstage" => "Le Backstage : l'onglet « Fichier » et la vue qu'il ouvre par-dessus la page — des onglets (chacun montrant une vue) et des boutons.",
        "RibbonBackstage.Header" => "Texte de l'onglet qui l'ouvre.",
        "BackstageTab" => "Un onglet du Backstage : une ligne de son bandeau qui montre une vue (son unique enfant) à droite.",
        "BackstageTab.Header" => "Texte de la ligne.",
        "BackstageTab.Icon" => "Icône de la ligne.",
        "BackstageButton" => "Une commande du bandeau du Backstage (Imprimer, Fermer…).",
        "BackstageButton.OnClick" => "Se produit quand on clique sur la ligne.",
        "BackstageSeparator" => "Un filet entre deux lignes du bandeau du Backstage.",
        "RibbonButton" => "Un bouton de commande du ruban : grand (icône au-dessus du libellé) ou petit (empilé par trois dans une colonne).",
        "RibbonButton.OnClick" => "Se produit quand on clique sur le bouton (puis sa commande s'exécute).",
        "RibbonToggleButton" => "Un bouton qui reste enfoncé (gras, italique) : son état Checked.",
        "RibbonToggleButton.Checked" | "RibbonRadioButton.Checked" => "Indique s'il est enfoncé.",
        "RibbonToggleButton.OnClick" | "RibbonRadioButton.OnClick" => "Se produit quand on clique sur le bouton.",
        "RibbonToggleButton.OnCheckedChanged" | "RibbonRadioButton.OnCheckedChanged" => "Se produit quand le bouton est enfoncé ou relâché.",
        "RibbonRadioButton" => "Une bascule d'un ensemble (aligner à gauche, centrer, à droite) : l'enfoncer relâche les autres de son GroupName.",
        "RibbonRadioButton.GroupName" => "L'ensemble auquel il appartient (les boutons radio du ruban de même GroupName).",
        "RibbonMenuButton" => "Un bouton dont toute la surface ouvre un menu (ses enfants RibbonMenuItem).",
        "RibbonMenuButton.OnDropDownOpening" | "RibbonSplitButton.OnDropDownOpening" => "Se produit quand le menu s'ouvre.",
        "RibbonSplitButton" => "Un bouton avec sa propre action et un chevron qui ouvre un menu (ses enfants RibbonMenuItem).",
        "RibbonSplitButton.OnClick" => "Se produit quand on clique sur la partie principale du bouton.",
        "RibbonColorPicker" => "Un bouton partagé dont le menu est une palette de couleurs (couleur de police, surlignage) : sa couleur est affichée sous son icône.",
        "RibbonColorPicker.SelectedColor" => "La couleur (#RRVVBB) ; vide pour Automatique.",
        "RibbonColorPicker.OnClick" => "Se produit quand on clique sur la partie principale (appliquer la couleur actuelle).",
        "RibbonColorPicker.OnSelectedColorChanged" => "Se produit quand une couleur est choisie (e.new : #RRVVBB, vide pour Automatique).",
        "RibbonMenuItem" => "Une entrée d'un menu du ruban ; ses propres enfants RibbonMenuItem forment un sous-menu.",
        "RibbonSplitMenuItem" => "Une entrée d'un menu du ruban qui exécute sa propre commande et ouvre un sous-menu (ses enfants).",
        "RibbonMenuItem.Checked" | "RibbonSplitMenuItem.Checked" => "Affiche une coche devant l'entrée.",
        "RibbonMenuItem.OnClick" | "RibbonSplitMenuItem.OnClick" => "Se produit quand l'entrée est choisie.",
        "RibbonCheckBox" => "Une case à cocher du ruban (Règle, Quadrillage…).",
        "RibbonCheckBox.Checked" => "Indique si elle est cochée.",
        "RibbonCheckBox.OnCheckedChanged" => "Se produit quand elle est cochée ou décochée.",
        "RibbonComboBox" => "Une liste déroulante du ruban (police, taille) ; IsEditable permet de taper une valeur.",
        "RibbonComboBox.IsEditable" => "La valeur peut être tapée aussi bien que choisie.",
        "RibbonComboBox.Width" | "RibbonTextBox.Width" | "RibbonNumericField.Width" => "Largeur du champ, en pixels.",
        "RibbonComboBox.SelectedValue" => "La valeur choisie ou tapée.",
        "RibbonComboBox.ItemsSource" => "Les choix, depuis une liste (champ Text, ou Value et Text), au lieu des enfants Option.",
        "RibbonComboBox.OnSelectedValueChanged" => "Se produit quand une valeur est choisie ou tapée (Entrée).",
        "RibbonTextBox" => "Un champ de texte du ruban.",
        "RibbonTextBox.Text" => "Le texte.",
        "RibbonTextBox.OnTextChanged" => "Se produit quand le texte est validé (Entrée, ou en quittant le champ).",
        "RibbonNumericField" => "Un nombre avec des flèches haut et bas (un espacement, un retrait).",
        "RibbonNumericField.Value" => "Le nombre.",
        "RibbonNumericField.Minimum" => "La plus petite valeur.",
        "RibbonNumericField.Maximum" => "La plus grande valeur.",
        "RibbonNumericField.Increment" => "Ce qu'une flèche ajoute ou retire.",
        "RibbonNumericField.OnValueChanged" => "Se produit quand le nombre change (une flèche, ou une valeur tapée et validée).",
        "RibbonGallery" => "Une galerie de choix (styles, couleurs, tableaux) : une grille dans le ruban avec un bouton « plus », une liste déroulante ou une grille dans un menu.",
        "RibbonGallery.Display" => "InRibbon : une grille dans le groupe. DropDown : un bouton qui ouvre la grille. InMenu : une grille dans un menu. Inline : une rangée de pastilles légendées, chacune aussi large que son texte (la bande de styles des éditeurs web).",
        "RibbonGallery.Rows" => "Lignes affichées dans le ruban.",
        "RibbonGallery.Columns" => "Colonnes affichées dans le ruban et dans la liste déroulante.",
        "RibbonGallery.ItemWidth" => "Largeur d'une cellule, en pixels.",
        "RibbonGallery.ItemHeight" => "Hauteur d'une cellule, en pixels.",
        "RibbonGallery.SelectedValue" => "La valeur (Value) de l'élément sélectionné.",
        "RibbonGallery.OnItemClick" => "Se produit quand on clique sur un élément (e.new est sa valeur).",
        "RibbonGalleryCategory" => "Une catégorie d'une galerie : ses éléments sous un en-tête dans la liste déroulante.",
        "RibbonGalleryCategory.Header" => "Texte de l'en-tête de la catégorie.",
        "RibbonGalleryItem" => "Un choix d'une galerie : une valeur, un libellé, une icône ou une couleur.",
        "RibbonGalleryItem.Value" => "Ce que devient SelectedValue de la galerie quand on clique dessus (sinon son libellé).",
        "RibbonGalleryItem.Color" => "Une couleur prévisualisée dans la cellule.",
        "RibbonGalleryItem.OnClick" => "Se produit quand on clique sur l'élément.",
        "RibbonLabel" => "Un texte fixe d'un groupe ou d'un menu du ruban (l'en-tête d'une section de menu).",
        "RibbonSeparator" => "Un séparateur entre les commandes d'un groupe ou les entrées d'un menu.",
        "Command" => "Une commande de la vue (barre des composants) : son libellé, ses icônes, son raccourci, son état activé et coché, et son événement Execute, partagés par chaque élément du ruban, bouton ou entrée de menu dont la propriété Command la nomme.",
        "Command.Label" => "Texte des éléments qui l'exécutent.",
        "Command.SmallIcon" => "Petite icône des éléments qui l'exécutent.",
        "Command.LargeIcon" => "Grande icône des éléments qui l'exécutent.",
        "Command.Shortcut" => "Raccourci clavier qui l'exécute partout dans la vue (Ctrl+B, Ctrl+Maj+S, F5…).",
        "Command.KeyTip" => "KeyTip des éléments qui l'exécutent.",
        "Command.ScreenTipTitle" => "Titre de leur info-bulle.",
        "Command.ScreenTipText" => "Description dans leur info-bulle.",
        "Command.Enabled" => "Indique si elle peut s'exécuter : les éléments qui l'exécutent sont grisés sinon.",
        "Command.Checked" => "Son état coché (gras activé) : les bascules qui l'exécutent l'affichent.",
        "Command.IsCheckable" => "L'exécuter inverse Checked.",
        "Command.OnExecute" => "Se produit quand la commande s'exécute : un élément qui l'exécute a été cliqué, ou son raccourci pressé.",
        "Command.OnQueryStatus" => "Se produit avant l'affichage des éléments qui l'exécutent : mettez à jour Enabled et Checked ici (les liaisons suffisent en général).",
        _ => return None,
    })
}

/// What the designer inserts when a ribbon element is dropped from the Toolbox: `None` for the
/// elements that need nothing more than their tag.
pub fn skeleton(component: &str) -> Option<String> {
    let button = r#"<RibbonButton Label="Bouton" SmallIcon="Star"/>"#;
    Some(match component {
        "Ribbon" => r#"<Ribbon Dock="Top"><RibbonTab Header="Accueil"><RibbonGroup Header="Groupe"><RibbonButton Label="Bouton" LargeIcon="Star" Size="Large"/></RibbonGroup></RibbonTab></Ribbon>"#.to_string(),
        "RibbonTab" => format!(r#"<RibbonTab Header="Onglet"><RibbonGroup Header="Groupe">{button}</RibbonGroup></RibbonTab>"#),
        "RibbonContextualTabGroup" => format!(r##"<RibbonContextualTabGroup Header="Outils" Color="#107C41"><RibbonTab Header="Onglet contextuel"><RibbonGroup Header="Groupe">{button}</RibbonGroup></RibbonTab></RibbonContextualTabGroup>"##),
        "RibbonGroup" => format!(r#"<RibbonGroup Header="Groupe">{button}</RibbonGroup>"#),
        "RibbonControlGroup" => r#"<RibbonControlGroup><RibbonToggleButton SmallIcon="Bold" Label="Gras" ShowLabel="false"/><RibbonToggleButton SmallIcon="Italic" Label="Italique" ShowLabel="false"/></RibbonControlGroup>"#.to_string(),
        "RibbonBox" => format!(r#"<RibbonBox>{button}</RibbonBox>"#),
        "RibbonQuickAccessToolbar" => r#"<RibbonQuickAccessToolbar><RibbonButton Label="Enregistrer" SmallIcon="Save"/></RibbonQuickAccessToolbar>"#.to_string(),
        "RibbonBackstage" => r#"<RibbonBackstage Header="Fichier"><BackstageTab Header="Informations" Icon="Info"><Label Text="Informations"/></BackstageTab><BackstageButton Label="Fermer" SmallIcon="X"/></RibbonBackstage>"#.to_string(),
        "BackstageTab" => r#"<BackstageTab Header="Onglet" Icon="FileText"><Label Text="Contenu"/></BackstageTab>"#.to_string(),
        "BackstageButton" => r#"<BackstageButton Label="Commande" SmallIcon="Star"/>"#.to_string(),
        "RibbonButton" => button.to_string(),
        "RibbonToggleButton" => r#"<RibbonToggleButton Label="Bascule" SmallIcon="Bold"/>"#.to_string(),
        "RibbonRadioButton" => r#"<RibbonRadioButton Label="Option" SmallIcon="AlignLeft" GroupName="groupe"/>"#.to_string(),
        "RibbonMenuButton" => r#"<RibbonMenuButton Label="Menu" SmallIcon="List"><RibbonMenuItem Label="Entrée 1"/><RibbonMenuItem Label="Entrée 2"/></RibbonMenuButton>"#.to_string(),
        "RibbonSplitButton" => r#"<RibbonSplitButton Label="Action" SmallIcon="Link"><RibbonMenuItem Label="Entrée 1"/><RibbonMenuItem Label="Entrée 2"/></RibbonSplitButton>"#.to_string(),
        "RibbonColorPicker" => r##"<RibbonColorPicker Label="Couleur" SmallIcon="Baseline" SelectedColor="#C00000" ShowLabel="false"/>"##.to_string(),
        "RibbonMenuItem" => r#"<RibbonMenuItem Label="Entrée"/>"#.to_string(),
        "RibbonSplitMenuItem" => r#"<RibbonSplitMenuItem Label="Entrée"><RibbonMenuItem Label="Sous-entrée"/></RibbonSplitMenuItem>"#.to_string(),
        "RibbonCheckBox" => r#"<RibbonCheckBox Label="Case à cocher"/>"#.to_string(),
        "RibbonComboBox" => r#"<RibbonComboBox Width="120" SelectedValue="Un"><Option Value="Un"/><Option Value="Deux"/></RibbonComboBox>"#.to_string(),
        "RibbonTextBox" => r#"<RibbonTextBox Width="120"/>"#.to_string(),
        "RibbonNumericField" => r#"<RibbonNumericField Width="64" Value="1"/>"#.to_string(),
        "RibbonGallery" => r#"<RibbonGallery Columns="3"><RibbonGalleryItem Label="Normal"/><RibbonGalleryItem Label="Titre 1"/><RibbonGalleryItem Label="Titre 2"/></RibbonGallery>"#.to_string(),
        "RibbonGalleryCategory" => r#"<RibbonGalleryCategory Header="Catégorie"><RibbonGalleryItem Label="Élément"/></RibbonGalleryCategory>"#.to_string(),
        "RibbonGalleryItem" => r#"<RibbonGalleryItem Label="Élément"/>"#.to_string(),
        "RibbonLabel" => r#"<RibbonLabel Label="Texte"/>"#.to_string(),
        "Command" => r#"<Command Label="Commande"/>"#.to_string(),
        _ => return None,
    })
}

/// Every component this family declares.
pub const ALL: &[ComponentMeta] = &[
    ribbon::META,
    ribbon_tab::META,
    ribbon_contextual_tab_group::META,
    ribbon_group::META,
    ribbon_control_group::META,
    ribbon_box::META,
    ribbon_qat::META,
    ribbon_backstage::META,
    backstage_tab::META,
    backstage_button::META,
    backstage_separator::META,
    ribbon_button::META,
    ribbon_toggle_button::META,
    ribbon_radio_button::META,
    ribbon_menu_button::META,
    ribbon_split_button::META,
    ribbon_color_picker::META,
    ribbon_menu_item::META,
    ribbon_split_menu_item::META,
    ribbon_check_box::META,
    ribbon_combo_box::META,
    ribbon_text_box::META,
    ribbon_numeric_field::META,
    ribbon_gallery::META,
    ribbon_gallery_category::META,
    ribbon_gallery_item::META,
    ribbon_label::META,
    ribbon_separator::META,
    command::META,
];

// ═════════════════════════════════════════════════════════════════════════════
// The model read from the element tree
// ═════════════════════════════════════════════════════════════════════════════

/// The properties of one sub-element, read once (bindings resolved every frame).
struct Attrs {
    label: PropSource<String>,
    small_icon: String,
    large_icon: String,
    tip_title: PropSource<String>,
    tip_text: PropSource<String>,
    /// `ItemsSource` of a combo box: the choices from a bound list (fields `Value` and `Text`).
    items_source: Option<crate::binding::BindingSpec>,
    key_tip: String,
    command: String,
    show_label: bool,
    qat: bool,
    visible: PropSource<bool>,
    enabled: PropSource<bool>,
    checked: PropSource<bool>,
    /// `Checked` is written on the element (else a command's state is used).
    has_checked: bool,
    large: bool,
    width: Option<f32>,
    /// `SelectedValue` / `Text` / `Value` / `SelectedColor`.
    value: PropSource<String>,
    editable: bool,
    range: (f32, f32, f32),
    gallery: Option<GallerySpec>,
    color: String,
    group_name: String,
    item_value: String,
    vertical: bool,
    launcher: bool,
    size_definition: String,
}

fn literal(p: &Props<'_>, name: &str, default: &str) -> String {
    match p.str(name, default) {
        Ok(PropSource::Literal(s)) => s,
        _ => default.to_string(),
    }
}

fn literal_f32(p: &Props<'_>, name: &str, default: f32) -> f32 {
    match p.f32(name, default) {
        Ok(PropSource::Literal(v)) => v,
        _ => default,
    }
}

fn literal_bool(p: &Props<'_>, name: &str, default: bool) -> bool {
    match p.bool(name, default) {
        Ok(PropSource::Literal(v)) => v,
        _ => default,
    }
}

impl Attrs {
    fn read(el: &Element, meta: &'static ComponentMeta) -> Result<Self, BuildError> {
        let p = Props::new(el, meta);
        let label_name = if meta.property("Header").is_some() { "Header" } else { "Label" };
        let value_name = ["SelectedValue", "Text", "SelectedColor"].into_iter().find(|n| meta.property(n).is_some());
        let value = match (value_name, meta.property("Value").is_some() && meta.name == "RibbonNumericField") {
            (_, true) => match p.f32("Value", 0.0)? {
                PropSource::Literal(v) => PropSource::Literal(engine::format_number(v)),
                PropSource::Bound { spec, fallback } => PropSource::Bound { spec, fallback: engine::format_number(fallback) },
            },
            (Some(n), _) => p.str(n, "")?,
            (None, _) => PropSource::Literal(String::new()),
        };
        let display = literal(&p, "Display", "InRibbon");
        // `Inline`: the web's single row of labelled chips, each as wide as its text (no grid spec).
        let gallery = (meta.name == "RibbonGallery" && display != "Inline").then(|| GallerySpec {
            display: match display.as_str() {
                "DropDown" => GalleryDisplay::DropDown,
                "InMenu" => GalleryDisplay::InMenu,
                _ => GalleryDisplay::InRibbon,
            },
            rows: literal_f32(&p, "Rows", 1.0).max(1.0) as usize,
            columns: literal_f32(&p, "Columns", 4.0).max(1.0) as usize,
            item_w: literal_f32(&p, "ItemWidth", 72.0),
            item_h: literal_f32(&p, "ItemHeight", 56.0),
            first_row: 0,
        });
        Ok(Self {
            label: p.str(label_name, if meta.name == "RibbonBackstage" { "Fichier" } else { "" })?,
            small_icon: literal(&p, if meta.property("Icon").is_some() && el.attribute("SmallIcon").is_none() { "Icon" } else { "SmallIcon" }, ""),
            large_icon: literal(&p, "LargeIcon", ""),
            tip_title: p.str("ScreenTipTitle", "")?,
            tip_text: p.str("ScreenTipText", "")?,
            items_source: if meta.property("ItemsSource").is_some() { p.str("ItemsSource", "")?.binding().cloned() } else { None },
            key_tip: literal(&p, "KeyTip", ""),
            command: literal(&p, if meta.name == "RibbonGroup" { "DialogLauncherCommand" } else { "Command" }, ""),
            show_label: literal_bool(&p, "ShowLabel", true),
            qat: literal_bool(&p, "CanAddToQat", true),
            visible: p.bool("Visible", true)?,
            enabled: p.bool("Enabled", true)?,
            checked: if meta.property("Checked").is_some() { p.bool("Checked", false)? } else { PropSource::Literal(false) },
            has_checked: el.attribute("Checked").is_some(),
            large: literal(&p, "Size", "Small") == "Large",
            width: el.attribute("Width").and_then(|a| a.value()).and_then(|v| v.trim().parse().ok()),
            value,
            editable: literal_bool(&p, "IsEditable", false),
            range: (literal_f32(&p, "Minimum", 0.0), literal_f32(&p, "Maximum", 100.0), literal_f32(&p, "Increment", 1.0)),
            gallery,
            color: literal(&p, "Color", ""),
            group_name: literal(&p, "GroupName", ""),
            item_value: literal(&p, "Value", ""),
            vertical: literal(&p, "Orientation", "Horizontal") == "Vertical",
            launcher: literal_bool(&p, "ShowDialogLauncher", false),
            size_definition: literal(&p, "SizeDefinition", "Auto"),
        })
    }
}

/// A sub-element: its virtual element (index in [`RibbonNode::elements`]), attributes, children.
struct Node {
    el: usize,
    attrs: Attrs,
    children: Vec<Node>,
    /// `<Option>` children of a combo box: `(value, label)`.
    options: Vec<(String, String)>,
    /// A Backstage tab's view.
    view: Option<Box<dyn ViewNode>>,
    /// A tab's scaling policy.
    scaling: ScalingPolicy,
}

/// A `<Command>` of the view.
struct Cmd {
    name: String,
    el: VirtualElement,
    attrs: Attrs,
    shortcut: String,
    checkable: bool,
}

/// `<Ribbon>`'s live node — see the module doc.
pub struct RibbonNode {
    engine: Rc<RefCell<Ribbon>>,
    elements: Vec<VirtualElement>,
    /// The ribbon's children, in document order.
    roots: Vec<Node>,
    commands: Vec<Cmd>,
    tone: String,
    owns_caption: bool,
    /// The window caption follows the ribbon: the view sets no `TitleBarBackground` and does not
    /// opt out (`TitleBarFollowsRibbon="false"`).
    follows: bool,
    selected: PropSource<String>,
    minimized: PropSource<bool>,
    qat_below: bool,
    simplified: bool,
    key_tips: bool,
    on_tab: Option<String>,
    on_minimized: Option<String>,
    on_qat: Option<String>,
    focus_id: Option<kubuno_desktop_ui::FocusId>,
    backstage: RefCell<Backstage>,
    /// The ribbon's height at the last paint (what `measure` answers; the whole height while the
    /// Backstage is open, so it covers the page).
    height: Cell<f32>,
    /// Values the user changed that no binding holds (a literal `Checked`, a typed text).
    local_checked: HashMap<usize, bool>,
    local_value: HashMap<usize, String>,
    /// The checked state of checkable commands with no binding, switched by their runs.
    cmd_checked: HashMap<String, bool>,
    /// The tab shown last (to report it when it changes).
    last_tab: Option<String>,
    /// Painting on the design surface (no view model behind the bindings: fields show sample values).
    design: Cell<bool>,
    /// The `<Ribbon>` element's own stable id (its smart tag, its « + » for a new tab).
    own_id: String,
    /// `PreviewWidth`: the width the designer lays the ribbon out at (0: its bounds).
    preview_width: f32,
    /// `QatSettingsKey`: where the user's quick access toolbar is saved (empty: nowhere).
    qat_key: String,
    /// The `<Ribbon>`'s x:Name: the target merged fragments name.
    own_name: Option<String>,
    /// The saved quick access toolbar was restored (once, at the first live frame).
    qat_loaded: bool,
    /// What assistive technology is told of each element drawn this frame: its name, role, checked
    /// state and whether it is disabled (`publish_access`).
    access_names: HashMap<String, (String, kubuno_desktop_controls::host::access::AccessRole, Option<bool>, bool)>,
    /// Elements an accessibility client pressed (a Narrator / UI Automation Invoke), clicked this frame.
    activated: Vec<String>,
}

fn build_node_tree(el: &Element, cx: &mut BuildCx, elements: &mut Vec<VirtualElement>) -> Result<Node, BuildError> {
    let ve = VirtualElement::build(el, cx)?;
    let meta = ve.meta;
    let attrs = Attrs::read(el, meta)?;
    let index = elements.len();
    elements.push(ve);
    let mut children = Vec::new();
    let mut options = Vec::new();
    let mut view = None;
    let mut scaling = ScalingPolicy::default();
    for child in el.children() {
        let name = child.name().unwrap_or_default();
        match name.as_str() {
            "Option" => {
                let value = child.attribute("Value").and_then(|a| a.value()).unwrap_or_default();
                let text = child.attribute("Label").or_else(|| child.attribute("Text")).and_then(|a| a.value()).filter(|t| !t.is_empty()).unwrap_or_else(|| value.clone());
                options.push((value, text));
            }
            "RibbonTab.ScalingPolicy" => {
                for s in child.children() {
                    let get = |n: &str| s.attribute(n).and_then(|a| a.value()).unwrap_or_default();
                    let group = get("Group");
                    // A step names a group by x:Name: the engine knows it by element id, mapped below.
                    let size = kubuno_desktop_ui::ribbon::GroupSize::parse(&get("Size")).unwrap_or(kubuno_desktop_ui::ribbon::GroupSize::Medium);
                    let step = ScaleStep { group, size };
                    if get("Ideal") == "true" {
                        scaling.ideal.push(step);
                    } else {
                        scaling.steps.push(step);
                    }
                }
            }
            _ if meta.name == "BackstageTab" => {
                view = Some(crate::compile::build_node(&child, cx, crate::registry::LayoutKind::None)?);
            }
            _ => children.push(build_node_tree(&child, cx, elements)?),
        }
    }
    Ok(Node { el: index, attrs, children, options, view, scaling })
}

/// Reads `<Ribbon>`.
pub(crate) fn build_ribbon(props: &Props<'_>, cx: &mut BuildCx) -> Result<Box<dyn ViewNode>, BuildError> {
    let mut elements = Vec::new();
    let mut roots = Vec::new();
    for child in props.element().children() {
        roots.push(build_node_tree(&child, cx, &mut elements)?);
    }
    // The scaling steps name groups by x:Name; the engine knows them by element id.
    let name_to_id: HashMap<String, String> = elements.iter().filter_map(|e| e.name.clone().map(|n| (n, e.id.clone()))).collect();
    fn remap(nodes: &mut [Node], map: &HashMap<String, String>) {
        for n in nodes {
            for s in n.scaling.ideal.iter_mut().chain(n.scaling.steps.iter_mut()) {
                if let Some(id) = map.get(&s.group) {
                    s.group = id.clone();
                }
            }
            remap(&mut n.children, map);
        }
    }
    remap(&mut roots, &name_to_id);
    // The view's commands (anywhere in the document).
    let mut commands = Vec::new();
    if let Some(root) = props.element().syntax().ancestors().last() {
        for el in root.descendants().filter_map(Element::cast) {
            if el.name().as_deref() != Some("Command") {
                continue;
            }
            let Some(name) = el.attribute("x:Name").and_then(|a| a.value()).filter(|n| !n.is_empty()) else { continue };
            let ve = VirtualElement::build_with(&el, cx, false)?;
            let attrs = Attrs::read(&el, ve.meta)?;
            let p = Props::new(&el, ve.meta);
            commands.push(Cmd { name, attrs, shortcut: literal(&p, "Shortcut", ""), checkable: literal_bool(&p, "IsCheckable", false), el: ve });
        }
    }
    let theme = theme_of(&literal(props, "Tone", "Documents"));
    let mut engine = Ribbon::new(Vec::new(), theme);
    engine.set_collapsed(literal_bool(props, "IsMinimized", false));
    Ok(Box::new(RibbonNode {
        engine: Rc::new(RefCell::new(engine)),
        elements,
        roots,
        commands,
        tone: literal(props, "Tone", "Documents"),
        owns_caption: literal_bool(props, "OwnsCaption", true),
        follows: title_follows_ribbon(props.element()),
        selected: props.str("SelectedTab", "")?,
        minimized: props.bool("IsMinimized", false)?,
        qat_below: literal(props, "QatPosition", "InTabStrip") == "BelowRibbon",
        simplified: literal(props, "DisplayMode", "Classic") == "Simplified",
        key_tips: literal_bool(props, "ShowKeyTips", true),
        on_tab: props.event("OnSelectedTabChanged"),
        on_minimized: props.event("OnIsMinimizedChanged"),
        on_qat: props.event("OnQatChanged"),
        focus_id: props.focus_id(),
        backstage: RefCell::new(Backstage::new(Vec::new())),
        height: Cell::new(engine::metrics::TAB_H + engine::metrics::CONTENT_H),
        local_checked: HashMap::new(),
        local_value: HashMap::new(),
        cmd_checked: HashMap::new(),
        last_tab: None,
        design: Cell::new(false),
        own_id: props.element().stable_id(),
        preview_width: literal_f32(props, "PreviewWidth", 0.0),
        qat_key: literal(props, "QatSettingsKey", ""),
        own_name: props.element().attribute("x:Name").and_then(|a| a.value()).map(|v| v.trim().to_string()).filter(|v| !v.is_empty()),
        qat_loaded: false,
        access_names: HashMap::new(),
        activated: Vec::new(),
    }))
}

/// Where the quick access toolbar of `key` is saved: `%LOCALAPPDATA%\Kubuno\settings\<exe>\<key>.qat`
/// (per user, per application). `None` for an empty key or a key that is not a plain name.
fn qat_settings_path(key: &str) -> Option<std::path::PathBuf> {
    let key = key.trim();
    if key.is_empty() || !key.chars().all(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | '.')) || key.starts_with('.') {
        return None;
    }
    let base = std::env::var_os("LOCALAPPDATA")?;
    let exe = std::env::current_exe().ok()?.file_stem()?.to_string_lossy().into_owned();
    Some(std::path::PathBuf::from(base).join("Kubuno").join("settings").join(exe).join(format!("{key}.qat")))
}

/// The element names saved in `path`, one per line (none when it does not exist).
fn load_qat(path: &std::path::Path) -> Vec<String> {
    std::fs::read_to_string(path).map(|t| t.lines().map(str::trim).filter(|l| !l.is_empty()).map(str::to_string).collect()).unwrap_or_default()
}

/// Saves `names` to `path` (best effort: a toolbar that cannot be saved stays as it is this session).
fn save_qat(path: &std::path::Path, names: &[String]) {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Err(e) = std::fs::write(path, names.join("\n")) {
        eprintln!("[ribbon] could not save the quick access toolbar to {}: {e}", path.display());
    }
}

/// The language server's warnings on ribbons (`RIBBON.md` §5): a `<Scale>` naming no group of its
/// tab, a group a policy makes larger again, two KeyTips that collide (among a ribbon's tabs, or
/// within one tab), and a `Command` naming no `<Command>` of the view. Non-blocking: the view runs.
pub fn warnings(parse: &crate::syntax::Parse, root: &Element) -> Vec<crate::syntax::Diagnostic> {
    use crate::syntax::Diagnostic;
    let mut out = Vec::new();
    let mut warn = |range: Option<rowan::TextRange>, message: String| {
        if let Some(range) = range {
            let lc = parse.line_col(range.start());
            out.push(Diagnostic { range, line: lc.line, column: lc.column, message });
        }
    };
    let attr = |e: &Element, n: &str| e.attribute(n).and_then(|a| a.value()).map(|v| v.trim().to_string()).filter(|v| !v.is_empty());
    let value_range = |e: &Element, n: &str| e.attribute(n).and_then(|a| a.value_range());
    let children = |e: &Element| e.syntax().children().filter_map(Element::cast).collect::<Vec<_>>();
    let all: Vec<Element> = root.syntax().descendants().filter_map(Element::cast).collect();
    let commands: Vec<String> = all.iter().filter(|e| e.name().as_deref() == Some("Command")).filter_map(|e| attr(e, "x:Name")).collect();
    for e in &all {
        let Some(name) = e.name() else { continue };
        if name.starts_with("Ribbon") || name.starts_with("Backstage") {
            if let Some(cmd) = attr(e, "Command").filter(|c| !c.starts_with('{')) {
                if !commands.contains(&cmd) {
                    warn(value_range(e, "Command"), format!("attribute `Command`: no `<Command x:Name=\"{cmd}\">` in this view"));
                }
            }
        }
        match name.as_str() {
            "Ribbon" => {
                let tabs: Vec<Element> = children(e)
                    .into_iter()
                    .flat_map(|c| if c.name().as_deref() == Some("RibbonContextualTabGroup") { children(&c) } else { vec![c] })
                    .filter(|c| c.name().as_deref() == Some("RibbonTab"))
                    .collect();
                key_tip_collisions(&tabs, &attr, &value_range, &mut warn, "tab");
            }
            "RibbonTab" => {
                // Every control of the tab (its groups and what they hold), menus aside.
                let controls: Vec<Element> = e
                    .syntax()
                    .descendants()
                    .filter_map(Element::cast)
                    .filter(|c| c.syntax() != e.syntax() && c.name().is_some_and(|n| n != "RibbonMenuItem" && n != "RibbonSplitMenuItem"))
                    .collect();
                key_tip_collisions(&controls, &attr, &value_range, &mut warn, "control of this tab");
                let groups: Vec<String> = children(e).iter().filter(|c| c.name().as_deref() == Some("RibbonGroup")).filter_map(|g| attr(g, "x:Name")).collect();
                let Some(policy) = children(e).into_iter().find(|c| c.name().as_deref() == Some("RibbonTab.ScalingPolicy")) else { continue };
                let rank = |s: &str| ["Large", "Medium", "Small", "Collapsed"].iter().position(|x| *x == s);
                let mut reached: Vec<(String, usize)> = Vec::new();
                for step in children(&policy) {
                    let group = attr(&step, "Group").unwrap_or_default();
                    if !groups.contains(&group) {
                        warn(value_range(&step, "Group").or_else(|| step.name_range()), format!("`<Scale>`: no group of this tab is named `{group}`"));
                        continue;
                    }
                    let size = attr(&step, "Size").unwrap_or_else(|| "Collapsed".to_string());
                    let Some(r) = rank(&size) else {
                        warn(value_range(&step, "Size"), format!("`<Scale>`: `{size}` is not a size (Medium, Small or Collapsed)"));
                        continue;
                    };
                    match reached.iter_mut().find(|(g, _)| *g == group) {
                        Some((_, before)) if r < *before => {
                            warn(value_range(&step, "Size"), format!("`<Scale>`: `{group}` grows back to {size}; a step never makes a group larger"));
                        }
                        Some((_, before)) => *before = r,
                        None => reached.push((group, r)),
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// Warns on every explicit KeyTip of `elements` already used by an earlier one.
fn key_tip_collisions(
    elements: &[Element],
    attr: &dyn Fn(&Element, &str) -> Option<String>,
    value_range: &dyn Fn(&Element, &str) -> Option<rowan::TextRange>,
    warn: &mut dyn FnMut(Option<rowan::TextRange>, String),
    what: &str,
) {
    let mut seen: Vec<String> = Vec::new();
    for e in elements {
        let Some(tip) = attr(e, "KeyTip").map(|t| t.to_uppercase()) else { continue };
        if seen.contains(&tip) {
            warn(value_range(e, "KeyTip"), format!("attribute `KeyTip`: `{tip}` is already the KeyTip of another {what}"));
        } else {
            seen.push(tip);
        }
    }
}

/// Whether the window's caption follows the ribbon: the view's root sets no `TitleBarBackground`
/// and does not opt out with `TitleBarFollowsRibbon="false"`.
fn title_follows_ribbon(element: &Element) -> bool {
    let Some(root) = element.syntax().ancestors().filter_map(Element::cast).last() else { return true };
    let attr = |n: &str| root.attribute(n).and_then(|a| a.value()).map(|v| v.trim().to_string()).unwrap_or_default();
    attr("TitleBarBackground").is_empty() && attr("TitleBarFollowsRibbon") != "false"
}

fn theme_of(tone: &str) -> RibbonTheme {
    use kubuno_desktop_ui::ribbon::tone as t;
    match tone {
        "Plain" => RibbonTheme::plain(),
        "Spreadsheet" => RibbonTheme::office(t::SPREADSHEET),
        "Presentation" => RibbonTheme::office(t::PRESENTATION),
        "Projects" => RibbonTheme::office(t::PROJECTS),
        "Diagrams" => RibbonTheme::office(t::DIAGRAMS),
        "Data" => RibbonTheme::office(t::DATA),
        "Maths" => RibbonTheme::office(t::MATHS),
        "Whiteboard" => RibbonTheme::office(t::WHITEBOARD),
        _ => RibbonTheme::office(t::DOCUMENTS),
    }
}

fn icon(name: &str) -> Option<Icon> {
    if name.is_empty() {
        return None;
    }
    Some(match crate::icon::resolve(name) {
        Some(s) => Icon::Borrowed(s),
        None => Icon::Owned(name.to_string()),
    })
}

fn parse_color(text: &str) -> Option<windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F> {
    kubuno_desktop_ui::color::parse(text.trim()).map(|c| c.to_d2d())
}

/// `"Ctrl+Shift+B"` → the key and its modifiers.
fn parse_shortcut(text: &str) -> Option<(u16, Modifiers)> {
    let mut mods = Modifiers::NONE;
    let mut key = None;
    for part in text.split('+').map(str::trim) {
        match part.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => mods.ctrl = true,
            "shift" => mods.shift = true,
            "alt" => mods.alt = true,
            p if p.len() == 1 => {
                let c = p.chars().next()?;
                key = Some(if c.is_ascii_digit() { host::vk::digit(c as u8 - b'0') } else { host::vk::letter(c) });
            }
            p if p.starts_with('f') => key = p[1..].parse::<u16>().ok().filter(|n| (1..=12).contains(n)).map(|n| host::vk::F1 + n - 1),
            "delete" | "del" => key = Some(host::vk::DELETE),
            "enter" => key = Some(host::vk::ENTER),
            _ => return None,
        }
    }
    key.filter(|k| *k != 0).map(|k| (k, mods))
}

/// The text of an `<Option>`: its `Text`/`Value` as written, or the resource a `{Res key}` names
/// (in the current UI culture).
fn option_text(raw: &str, vm: &dyn ViewModel) -> String {
    if !kubuno_desktop_views_syntax::res::is_res_expr(raw) {
        return raw.to_string();
    }
    let inner = raw.trim().trim_start_matches('{').trim_end_matches('}');
    match crate::resources::parse_res(inner).and_then(|spec| crate::resources::get(vm, &spec)) {
        Some(Value::Str(text)) => text,
        _ => raw.to_string(),
    }
}

/// A combo box's choices from its bound `ItemsSource`: one per row, its `Value` field (else its
/// `Text`) and its `Text` field (else its value). `None` when the binding holds no list.
fn bound_options(vm: &dyn ViewModel, spec: &crate::binding::BindingSpec) -> Option<Vec<RibbonOption>> {
    let Some(Value::List(rows)) = vm.get(&spec.path) else { return None };
    Some(
        rows.iter()
            .map(|r| {
                let text = r.text("Text");
                let value = r.text("Value");
                let value = if value.is_empty() { text.clone() } else { value };
                let text = if text.is_empty() { value.clone() } else { text };
                RibbonOption::new(value, text)
            })
            .collect(),
    )
}

impl RibbonNode {
    fn cmd(&self, name: &str) -> Option<&Cmd> {
        (!name.is_empty()).then(|| self.commands.iter().find(|c| c.name == name)).flatten()
    }

    /// A command's checked state: switched by a run when it is checkable and not bound, else its
    /// `Checked` (literal or binding).
    fn command_checked(&self, c: &Cmd, vm: &dyn ViewModel) -> bool {
        self.cmd_checked.get(&c.name).copied().unwrap_or_else(|| c.attrs.checked.resolve(vm))
    }

    fn id(&self, n: &Node) -> String {
        self.elements[n.el].id.clone()
    }

    /// Finds the node of element `id` (and whether it lies in the Backstage).
    fn find<'a>(nodes: &'a [Node], elements: &[VirtualElement], id: &str) -> Option<&'a Node> {
        for n in nodes {
            if elements[n.el].id == id {
                return Some(n);
            }
            if let Some(f) = Self::find(&n.children, elements, id) {
                return Some(f);
            }
        }
        None
    }

    fn checked_of(&self, n: &Node, vm: &dyn ViewModel) -> bool {
        if let Some(v) = self.local_checked.get(&n.el) {
            return *v;
        }
        // An element running a command, with no `Checked` of its own, shows the command's state
        // (`RIBBON.md` §4: the state lives in the command, every surface repaints from it).
        if !n.attrs.has_checked {
            if let Some(c) = self.cmd(&n.attrs.command) {
                return self.command_checked(c, vm);
            }
        }
        n.attrs.checked.resolve(vm)
    }

    fn value_of(&self, n: &Node, vm: &dyn ViewModel) -> String {
        let value = self.local_value.get(&n.el).cloned().unwrap_or_else(|| n.attrs.value.resolve(vm));
        // On the design surface a bound field shows its first choice (no view model holds the value).
        if value.is_empty() && self.design.get() {
            if let Some((v, _)) = n.options.first() {
                return v.clone();
            }
        }
        value
    }

    /// The engine item of one element, the command's label, icons and state applied.
    fn item(&self, n: &Node, vm: &dyn ViewModel) -> RibbonItem {
        let ve = &self.elements[n.el];
        let a = &n.attrs;
        let cmd = self.cmd(&a.command);
        let kind = match ve.meta.name {
            "RibbonToggleButton" | "RibbonRadioButton" => ItemKind::Toggle,
            "RibbonMenuButton" => ItemKind::Menu,
            "RibbonSplitButton" | "RibbonSplitMenuItem" => ItemKind::Split,
            "RibbonColorPicker" => ItemKind::ColorPicker,
            "RibbonCheckBox" => ItemKind::CheckBox,
            "RibbonComboBox" => ItemKind::ComboBox,
            "RibbonTextBox" => ItemKind::TextBox,
            "RibbonNumericField" => ItemKind::NumericField,
            "RibbonGallery" => ItemKind::Gallery,
            "RibbonLabel" => ItemKind::Label,
            "RibbonSeparator" => ItemKind::Separator,
            "RibbonControlGroup" => ItemKind::ControlGroup,
            "RibbonBox" => ItemKind::Box,
            _ => ItemKind::Button,
        };
        let mut it = RibbonItem::of_kind(ve.id.clone(), kind);
        let own = a.label.resolve(vm);
        let label = if own.is_empty() { cmd.map(|c| c.attrs.label.resolve(vm)).unwrap_or_default() } else { own };
        let small = if a.small_icon.is_empty() { cmd.map(|c| c.attrs.small_icon.clone()).unwrap_or_default() } else { a.small_icon.clone() };
        let large_icon = if a.large_icon.is_empty() { cmd.map(|c| c.attrs.large_icon.clone()).unwrap_or_default() } else { a.large_icon.clone() };
        it.size = if a.large { ItemSize::Large } else { ItemSize::Small };
        let use_large = (it.size == ItemSize::Large && !large_icon.is_empty()) || small.is_empty();
        it.icon = icon(if use_large { &large_icon } else { &small });
        // ScreenTips resolve every frame: a `{Res key}` follows the UI culture, a binding its source.
        let own_title = a.tip_title.resolve(vm);
        let own_text = a.tip_text.resolve(vm);
        let title = if own_title.is_empty() { cmd.map(|c| c.attrs.tip_title.resolve(vm)).unwrap_or_default() } else { own_title };
        let text = if own_text.is_empty() { cmd.map(|c| c.attrs.tip_text.resolve(vm)).unwrap_or_default() } else { own_text };
        let tip = [title, text].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" — ");
        if a.show_label {
            it = it.label(label.clone());
            if !tip.is_empty() {
                it.tooltip = Some(tip);
            }
        } else {
            it.tooltip = Some(if tip.is_empty() { label.clone() } else { tip });
        }
        it.shortcut = cmd.map(|c| c.shortcut.clone()).filter(|s| !s.is_empty());
        it.key_tip = Some(a.key_tip.clone()).filter(|k| !k.is_empty()).or_else(|| cmd.map(|c| c.attrs.key_tip.clone()).filter(|k| !k.is_empty()));
        it.visible = a.visible.resolve(vm);
        it.disabled = !a.enabled.resolve(vm) || cmd.is_some_and(|c| !c.attrs.enabled.resolve(vm));
        it.qat = a.qat;
        it.width = a.width;
        match kind {
            ItemKind::Toggle | ItemKind::CheckBox | ItemKind::Button | ItemKind::Split => it.active = self.checked_of(n, vm) && matches!(kind, ItemKind::Toggle | ItemKind::CheckBox | ItemKind::Split | ItemKind::Button),
            _ => {}
        }
        if ve.meta.name == "RibbonButton" && !a.has_checked && cmd.is_none_or(|c| !c.checkable) {
            it.active = false;
        }
        match kind {
            ItemKind::ComboBox => {
                it.editable = a.editable;
                it.width = Some(a.width.unwrap_or(120.0));
                it.options = match a.items_source.as_ref().and_then(|spec| bound_options(vm, spec)) {
                    Some(bound) => bound,
                    None => n.options.iter().map(|(v, l)| RibbonOption::new(v.clone(), option_text(l, vm))).collect(),
                };
                it.value = Some(self.value_of(n, vm));
            }
            ItemKind::TextBox => {
                it.width = Some(a.width.unwrap_or(120.0));
                it.value = Some(self.value_of(n, vm));
            }
            ItemKind::NumericField => {
                it.width = Some(a.width.unwrap_or(64.0));
                it.range = Some(a.range);
                it.value = Some(self.value_of(n, vm));
            }
            ItemKind::ColorPicker => {
                it.color = parse_color(&self.value_of(n, vm));
                it.split_items = n.children.iter().map(|c| self.item(c, vm)).collect();
            }
            ItemKind::Gallery => {
                // `Display="Inline"` has no grid: the engine's single row of chips.
                it.gallery = a.gallery.clone().map(|mut spec| {
                    spec.first_row = 0;
                    spec
                });
                it.value = Some(self.value_of(n, vm)).filter(|v| !v.is_empty());
                let mut opts = Vec::new();
                for c in &n.children {
                    let cve = &self.elements[c.el];
                    if cve.meta.name == "RibbonGalleryCategory" {
                        let header = c.attrs.label.resolve(vm);
                        for g in &c.children {
                            opts.push(self.gallery_option(g, vm).category(header.clone()));
                        }
                    } else {
                        opts.push(self.gallery_option(c, vm));
                    }
                }
                it.options = opts;
            }
            ItemKind::ControlGroup | ItemKind::Box => {
                it.vertical = a.vertical;
                it.children = n.children.iter().map(|c| self.item(c, vm)).collect();
            }
            ItemKind::Menu | ItemKind::Split => {
                it.split_items = n.children.iter().map(|c| self.item(c, vm)).collect();
                if ve.meta.name == "RibbonMenuButton" {
                    it.has_action = false;
                }
                // A menu item whose children make a sub-menu.
                if ve.meta.name == "RibbonSplitMenuItem" && n.children.is_empty() {
                    it.kind = ItemKind::Button;
                }
            }
            _ => {}
        }
        if ve.meta.name == "RibbonMenuItem" {
            it.split_items = n.children.iter().map(|c| self.item(c, vm)).collect();
            it.active = self.checked_of(n, vm);
        }
        it
    }

    fn gallery_option(&self, n: &Node, vm: &dyn ViewModel) -> RibbonOption {
        let label = n.attrs.label.resolve(vm);
        let value = if n.attrs.item_value.is_empty() { label.clone() } else { n.attrs.item_value.clone() };
        let mut o = RibbonOption::new(value, label);
        if let Some(i) = icon(&n.attrs.small_icon) {
            o = o.icon(i);
        }
        if let Some(c) = parse_color(&n.attrs.color) {
            o = o.color(c);
        }
        o
    }

    fn tab(&self, n: &Node, vm: &dyn ViewModel, ctx: Option<&Node>) -> RibbonTab {
        let mut t = RibbonTab::new(self.id(n), n.attrs.label.resolve(vm), Vec::new());
        t.visible = n.attrs.visible.resolve(vm);
        t.key_tip = Some(n.attrs.key_tip.clone()).filter(|k| !k.is_empty());
        t.scaling = n.scaling.clone();
        for g in n.children.iter().filter(|g| self.elements[g.el].meta.name == "RibbonGroup") {
            let mut rg = RibbonGroup::new(self.id(g), g.attrs.label.resolve(vm), g.children.iter().map(|c| self.item(c, vm)).collect());
            rg.icon = icon(&g.attrs.small_icon);
            rg.visible = g.attrs.visible.resolve(vm);
            rg.key_tip = Some(g.attrs.key_tip.clone()).filter(|k| !k.is_empty());
            rg.launcher = g.attrs.launcher;
            rg.size = kubuno_desktop_ui::ribbon::SizeDefinition::parse(&g.attrs.size_definition).unwrap_or_default();
            t.groups.push(rg);
        }
        if let Some(c) = ctx {
            let color = parse_color(&c.attrs.color).unwrap_or(kubuno_desktop_ui::ribbon::hex(0x107c41));
            let visible = t.visible && c.attrs.visible.resolve(vm);
            t = t.contextual_group(color, visible, self.id(c), c.attrs.label.resolve(vm));
        }
        t
    }

    /// The engine's tabs, quick actions and Backstage sections for this frame.
    fn model(&self, vm: &dyn ViewModel) -> (Vec<RibbonTab>, Vec<RibbonItem>, Vec<BackstageSection>) {
        let mut tabs = Vec::new();
        let mut qat = Vec::new();
        let mut sections = Vec::new();
        for n in &self.roots {
            match self.elements[n.el].meta.name {
                "RibbonTab" => tabs.push(self.tab(n, vm, None)),
                "RibbonContextualTabGroup" => {
                    for t in &n.children {
                        tabs.push(self.tab(t, vm, Some(n)));
                    }
                }
                "RibbonQuickAccessToolbar" => qat = n.children.iter().map(|c| self.item(c, vm)).collect(),
                "RibbonBackstage" => {
                    tabs.insert(0, RibbonTab::file(self.id(n), n.attrs.label.resolve(vm)));
                    let mut separated = false;
                    for e in &n.children {
                        let name = self.elements[e.el].meta.name;
                        if name == "BackstageSeparator" {
                            separated = true;
                            continue;
                        }
                        let label = e.attrs.label.resolve(vm);
                        let ic = icon(&e.attrs.small_icon).unwrap_or(Icon::Borrowed("FileText"));
                        let mut s = if name == "BackstageTab" { BackstageSection::view(self.id(e), label, ic) } else { BackstageSection::action(self.id(e), label, ic) };
                        s.separated = separated;
                        s.disabled = !e.attrs.enabled.resolve(vm);
                        separated = false;
                        sections.push(s);
                    }
                }
                _ => {}
            }
        }
        (tabs, qat, sections)
    }

    /// The design state from the designer's selection: the tab (or Backstage) holding a selected
    /// element is shown, a selected menu control (or entry) shows its drop-down.
    fn design_state(&self) -> RibbonDesign {
        let selection = crate::virtual_regions::design_selection();
        let under = |id: &str| selection.iter().any(|s| s == id || s.starts_with(&format!("{id}.")));
        let mut d = RibbonDesign { add_glyphs: true, ..RibbonDesign::default() };
        let mut tab_ids = Vec::new();
        for n in &self.roots {
            match self.elements[n.el].meta.name {
                "RibbonTab" => tab_ids.push(self.id(n)),
                "RibbonContextualTabGroup" => tab_ids.extend(n.children.iter().map(|t| self.id(t))),
                "RibbonBackstage" => d.backstage |= under(&self.id(n)),
                _ => {}
            }
        }
        d.active_tab = tab_ids.into_iter().find(|t| under(t));
        // The deepest selected control that has a menu.
        fn menu_owner(nodes: &[Node], elements: &[VirtualElement], under: &dyn Fn(&str) -> bool) -> Option<String> {
            for n in nodes {
                let ve = &elements[n.el];
                if under(&ve.id) {
                    if let Some(inner) = menu_owner(&n.children, elements, under) {
                        return Some(inner);
                    }
                    let menu = matches!(ve.meta.name, "RibbonMenuButton" | "RibbonSplitButton" | "RibbonColorPicker") || (ve.meta.name == "RibbonGallery" && n.attrs.gallery.as_ref().is_some_and(|g| g.display == GalleryDisplay::DropDown));
                    if menu {
                        return Some(ve.id.clone());
                    }
                }
            }
            None
        }
        d.open_menu = menu_owner(&self.roots, &self.elements, &under);
        d
    }

    /// Runs `n`'s command (after its own event): `Execute`, and a checkable command switches.
    fn execute(&mut self, cx: &mut PaintCx<'_>, n_el: usize, command: &str, rect: Rect) {
        let _ = n_el;
        let Some(i) = self.commands.iter().position(|c| c.name == command) else { return };
        if !self.commands[i].attrs.enabled.resolve(&*cx.vm) {
            return;
        }
        if self.commands[i].checkable {
            let now = self.command_checked(&self.commands[i], &*cx.vm);
            match self.commands[i].attrs.checked.binding() {
                Some(spec) => spec.update_source(cx.vm, Value::Bool(!now)),
                None => {
                    let name = self.commands[i].name.clone();
                    self.cmd_checked.insert(name, !now);
                }
            }
        }
        let el = &self.commands[i].el;
        el.fire(cx, rect, "OnExecute", ViewEventKind::Clicked, &mut EmptyEventArgs);
    }

    /// Where element `id` was drawn this frame.
    fn rect_of(regions: &[kubuno_desktop_ui::ribbon::RibbonRegion], id: &str) -> Rect {
        regions.iter().find(|r| r.id == id).map(|r| r.rect).unwrap_or_default()
    }

    fn index_of(&self, id: &str) -> Option<usize> {
        self.elements.iter().position(|e| e.id == id)
    }

    /// The node of element index `el`.
    fn node(&self, el: usize) -> Option<&Node> {
        Self::find(&self.roots, &self.elements, &self.elements[el].id)
    }

    fn set_checked(&mut self, cx: &mut PaintCx<'_>, el: usize, on: bool) {
        let bound = self.node(el).and_then(|n| n.attrs.checked.binding().cloned());
        match bound {
            Some(spec) => {
                cx.vm.set_bound(&spec, Value::Bool(on));
                self.local_checked.remove(&el);
            }
            None => {
                self.local_checked.insert(el, on);
            }
        }
    }

    fn set_value(&mut self, cx: &mut PaintCx<'_>, el: usize, value: &str) {
        let bound = self.node(el).and_then(|n| n.attrs.value.binding().cloned());
        match bound {
            Some(spec) => {
                let v = if self.elements[el].meta.name == "RibbonNumericField" { value.parse::<f32>().map(Value::F32).unwrap_or(Value::Str(value.to_string())) } else { Value::Str(value.to_string()) };
                cx.vm.set_bound(&spec, v);
                self.local_value.remove(&el);
            }
            None => {
                self.local_value.insert(el, value.to_string());
            }
        }
    }

    /// A click on a button-like element (a ribbon button, a menu entry, a check box…).
    fn clicked(&mut self, cx: &mut PaintCx<'_>, el: usize, rect: Rect) {
        let Some(n) = self.node(el) else { return };
        let name = self.elements[el].meta.name;
        let command = n.attrs.command.clone();
        let group_name = n.attrs.group_name.clone();
        let checked = self.checked_of(n, &*cx.vm);
        let toggles = matches!(name, "RibbonToggleButton" | "RibbonCheckBox" | "RibbonRadioButton" | "RibbonMenuItem" | "RibbonSplitMenuItem") && (n.attrs.has_checked || name != "RibbonMenuItem");
        // An element running a command and with no `Checked` of its own keeps no state: the command's
        // is shown (a checkable command switches when it runs; another one is switched by its code).
        let owned_by_command = !n.attrs.has_checked && self.cmd(&command).is_some();
        let mut args = crate::node::click_args(cx.frame, rect);
        if name != "RibbonCheckBox" {
            self.elements[el].fire(cx, rect, "OnClick", ViewEventKind::Clicked, &mut args);
        }
        if toggles && !owned_by_command {
            let new = if name == "RibbonRadioButton" { true } else { !checked };
            if name == "RibbonRadioButton" && !group_name.is_empty() {
                let others: Vec<usize> = (0..self.elements.len()).filter(|&i| i != el && self.elements[i].meta.name == "RibbonRadioButton" && self.node(i).is_some_and(|o| o.attrs.group_name == group_name)).collect();
                for o in others {
                    if self.node(o).is_some_and(|on| self.checked_of(on, &*cx.vm)) {
                        self.set_checked(cx, o, false);
                        let r = rect;
                        self.elements[o].fire(cx, r, "OnCheckedChanged", ViewEventKind::Toggled(false), &mut CheckedChangedEventArgs::new(true, false, ChangeSource::User));
                    }
                }
            }
            if new != checked {
                self.set_checked(cx, el, new);
                self.elements[el].fire(cx, rect, "OnCheckedChanged", ViewEventKind::Toggled(new), &mut CheckedChangedEventArgs::new(checked, new, ChangeSource::User));
            }
        }
        if !command.is_empty() {
            self.execute(cx, el, &command, rect);
        }
    }

    /// A value picked or typed (combo box, text box, numeric field, gallery, colour).
    fn changed(&mut self, cx: &mut PaintCx<'_>, el: usize, value: &str, rect: Rect) {
        let Some(n) = self.node(el) else { return };
        let name = self.elements[el].meta.name;
        let old = self.value_of(n, &*cx.vm);
        let command = n.attrs.command.clone();
        match name {
            "RibbonGallery" => {
                // The item clicked raises its own Click too.
                let item = n.children.iter().flat_map(|c| if c.children.is_empty() { vec![c] } else { c.children.iter().collect() }).find(|c| {
                    let label = c.attrs.label.resolve(&*cx.vm);
                    (if c.attrs.item_value.is_empty() { label } else { c.attrs.item_value.clone() }) == value
                });
                if let Some(i) = item.map(|c| c.el) {
                    let mut args = crate::node::click_args(cx.frame, rect);
                    self.elements[i].fire(cx, rect, "OnClick", ViewEventKind::Clicked, &mut args);
                }
                self.set_value(cx, el, value);
                self.elements[el].fire(cx, rect, "OnItemClick", ViewEventKind::Changed(value.to_string()), &mut TextChangedEventArgs::new(old, value.to_string(), ChangeSource::User));
            }
            "RibbonNumericField" => {
                let o = old.parse::<f32>().unwrap_or(0.0);
                let v = value.parse::<f32>().unwrap_or(o);
                self.set_value(cx, el, value);
                self.elements[el].fire(cx, rect, "OnValueChanged", ViewEventKind::Changed(value.to_string()), &mut NumericValueChangedEventArgs::new(o, v, ChangeSource::User));
            }
            _ => {
                let event = match name {
                    "RibbonColorPicker" => "OnSelectedColorChanged",
                    "RibbonTextBox" => "OnTextChanged",
                    _ => "OnSelectedValueChanged",
                };
                self.set_value(cx, el, value);
                self.elements[el].fire(cx, rect, event, ViewEventKind::Changed(value.to_string()), &mut TextChangedEventArgs::new(old, value.to_string(), ChangeSource::User));
            }
        }
        if !command.is_empty() {
            self.execute(cx, el, &command, rect);
        }
    }

    /// Maps the engine's events to the elements.
    fn dispatch(&mut self, cx: &mut PaintCx<'_>, events: Vec<RibbonEvent>, regions: &[kubuno_desktop_ui::ribbon::RibbonRegion]) {
        for e in events {
            match e {
                RibbonEvent::TabChanged(id) => {
                    let name = self.index_of(&id).map(|i| self.elements[i].name.clone().unwrap_or_else(|| id.clone())).unwrap_or(id.clone());
                    let old = self.last_tab.clone().unwrap_or_default();
                    self.last_tab = Some(name.clone());
                    if let Some(spec) = self.selected.binding().cloned() {
                        cx.vm.set_bound(&spec, Value::Str(name.clone()));
                    }
                    let handler = self.on_tab.clone();
                    cx.fire("OnSelectedTabChanged", self.focus_id, handler.as_deref(), ViewEventKind::Changed(name.clone()), &mut TextChangedEventArgs::new(old, name, ChangeSource::User));
                }
                RibbonEvent::Clicked(id) => {
                    if let Some(el) = self.index_of(&id) {
                        // A Backstage entry, a quick action or a control.
                        let rect = Self::rect_of(regions, &id);
                        self.clicked(cx, el, rect);
                    } else if let Some(name) = self.own_name.as_deref() {
                        // A control a merged fragment added: its own handler.
                        kubuno_desktop_ui::ribbon::merge::dispatch(name, &id);
                    }
                }
                RibbonEvent::DoubleClicked(_) => {}
                RibbonEvent::Chosen { item, entry } => {
                    let rect = Self::rect_of(regions, &item);
                    if let Some(el) = self.index_of(&entry) {
                        self.clicked(cx, el, rect);
                    } else if entry == "more-colors" {
                        let _ = item;
                    }
                }
                RibbonEvent::Changed { item, value } => {
                    if let Some(el) = self.index_of(&item) {
                        let rect = Self::rect_of(regions, &item);
                        self.changed(cx, el, &value, rect);
                    }
                }
                RibbonEvent::Launcher(group) => {
                    if let Some(el) = self.index_of(&group) {
                        let rect = Self::rect_of(regions, &group);
                        let command = self.node(el).map(|n| n.attrs.command.clone()).unwrap_or_default();
                        let mut args = crate::node::click_args(cx.frame, rect);
                        self.elements[el].fire(cx, rect, "OnDialogLauncherClick", ViewEventKind::Clicked, &mut args);
                        if !command.is_empty() {
                            self.execute(cx, el, &command, rect);
                        }
                    }
                }
                RibbonEvent::Collapsed(on) => {
                    if let Some(spec) = self.minimized.binding().cloned() {
                        cx.vm.set_bound(&spec, Value::Bool(on));
                    }
                    let handler = self.on_minimized.clone();
                    cx.fire("OnIsMinimizedChanged", self.focus_id, handler.as_deref(), ViewEventKind::Toggled(on), &mut CheckedChangedEventArgs::new(!on, on, ChangeSource::User));
                }
                RibbonEvent::QatChanged(ids) => {
                    let names: Vec<String> = ids.iter().map(|id| self.index_of(id).and_then(|i| self.elements[i].name.clone()).unwrap_or_else(|| id.clone())).collect();
                    let handler = self.on_qat.clone();
                    let joined = names.join(",");
                    if let Some(path) = qat_settings_path(&self.qat_key) {
                        save_qat(&path, &names);
                    }
                    cx.fire("OnQatChanged", self.focus_id, handler.as_deref(), ViewEventKind::Changed(joined.clone()), &mut TextChangedEventArgs::new(String::new(), joined, ChangeSource::User));
                }
            }
        }
    }

    /// Declares every region to the designer and the router, in paint order (a container before
    /// what it holds).
    fn report(&self, cx: &mut PaintCx<'_>, regions: &[kubuno_desktop_ui::ribbon::RibbonRegion], vm_enabled: &dyn Fn(usize) -> bool) {
        let mut done: Vec<&str> = Vec::new();
        for r in regions {
            let id = match r.kind {
                RegionKind::GalleryItem(n) => {
                    // Cell n of a gallery: its n-th item (categories flattened).
                    let Some(g) = self.index_of(&r.id).and_then(|i| self.node(i)) else { continue };
                    let items: Vec<&Node> = g.children.iter().flat_map(|c| if self.elements[c.el].meta.name == "RibbonGalleryCategory" { c.children.iter().collect() } else { vec![c] }).collect();
                    match items.get(n) {
                        Some(item) => self.elements[item.el].id.as_str(),
                        None => continue,
                    }
                }
                RegionKind::Launcher | RegionKind::Collapse | RegionKind::AddTab | RegionKind::AddGroup | RegionKind::AddItem | RegionKind::MenuEntry => continue,
                _ => r.id.as_str(),
            };
            if done.contains(&id) {
                continue;
            }
            let Some(i) = self.index_of(id) else { continue };
            done.push(&self.elements[i].id);
            self.elements[i].report(cx, r.rect, vm_enabled(i));
        }
    }

    /// Publishes the tabs, the quick access toolbar and the controls drawn this frame to assistive
    /// technology, under the ribbon's own node: each with its name (its label, else its tooltip — an
    /// icon-only button's only words), role and state, and pressable (UI Automation's Invoke clicks
    /// it, `RibbonNode::paint`).
    fn publish_access(&self, cx: &mut PaintCx<'_>, regions: &[kubuno_desktop_ui::ribbon::RibbonRegion]) {
        let Some(services) = cx.services.as_deref_mut() else { return };
        let parent = crate::common::access_id(&self.own_id);
        let mut done: Vec<&str> = Vec::new();
        for r in regions {
            if !matches!(r.kind, RegionKind::FileTab | RegionKind::Tab | RegionKind::QuickAction | RegionKind::Item | RegionKind::MenuEntry) || done.contains(&r.id.as_str()) {
                continue;
            }
            let (Some((name, role, checked, disabled)), Some(i)) = (self.access_names.get(&r.id), self.index_of(&r.id)) else { continue };
            done.push(&r.id);
            let client = crate::clip::visible_client(crate::common::to_client(r.rect));
            let id = crate::common::access_id(&r.id);
            services.access_ids.push((id, r.id.clone(), self.elements[i].focus_id()));
            services.access.push(kubuno_desktop_controls::host::access::AccessNode {
                id,
                parent: Some(parent),
                role: *role,
                name: name.clone(),
                description: String::new(),
                value: None,
                bounds: (client.left, client.top, client.right, client.bottom),
                focusable: false,
                disabled: *disabled,
                checked: *checked,
                clickable: !disabled,
                read_only: true,
                access_key: None,
                expanded: None,
            });
        }
    }
}

/// What assistive technology is told of each tab and control of `engine`'s model: its name (label,
/// else tooltip), role, checked state (toggles and check boxes) and whether it is disabled.
fn access_names(engine: &Ribbon) -> HashMap<String, (String, kubuno_desktop_controls::host::access::AccessRole, Option<bool>, bool)> {
    use kubuno_desktop_controls::host::access::AccessRole;
    fn walk(items: &[RibbonItem], out: &mut HashMap<String, (String, AccessRole, Option<bool>, bool)>) {
        for it in items {
            let name = it.label.clone().or_else(|| it.tooltip.clone()).unwrap_or_default();
            let (role, checked) = match it.kind {
                ItemKind::Toggle | ItemKind::CheckBox => (AccessRole::CheckBox, Some(it.active)),
                ItemKind::ComboBox | ItemKind::Dropdown => (AccessRole::ComboBox, None),
                ItemKind::Separator | ItemKind::ControlGroup | ItemKind::Box | ItemKind::Label => (AccessRole::Group, None),
                _ => (AccessRole::Button, None),
            };
            if !name.is_empty() {
                out.insert(it.id.clone(), (name, role, checked, it.disabled));
            }
            walk(&it.children, out);
            walk(&it.split_items, out);
        }
    }
    let mut out = HashMap::new();
    for t in &engine.tabs {
        out.insert(t.id.clone(), (t.label.clone(), AccessRole::Tab, None, false));
        for g in &t.groups {
            walk(&g.items, &mut out);
        }
    }
    walk(&engine.strip_actions, &mut out);
    out
}

impl ViewNode for RibbonNode {
    fn measure(&self, _c: &dyn Canvas, _vm: &dyn ViewModel) -> Size {
        Size::new(400.0, self.height.get())
    }

    fn paint(&mut self, cx: &mut PaintCx<'_>, bounds: Rect) {
        let design = cx.design.is_some();
        self.design.set(design);
        let (mut tabs, qat, sections) = self.model(&*cx.vm);
        // The fragments other modules merged into this ribbon (`RIBBON.md` §8), by its x:Name; they
        // name tabs and groups by x:Name too.
        if let Some(name) = self.own_name.as_deref() {
            let elements = &self.elements;
            let id_of = |n: &str| elements.iter().find(|e| e.name.as_deref() == Some(n)).map(|e| e.id.clone()).unwrap_or_else(|| n.to_string());
            kubuno_desktop_ui::ribbon::merge::apply_named(name, &mut tabs, &id_of);
        }
        let engine_rc = self.engine.clone();
        let mut engine = engine_rc.borrow_mut();
        engine.tabs = tabs;
        engine.strip_actions = qat;
        engine.theme = theme_of(&self.tone);
        engine.owns_caption = self.owns_caption && self.follows;
        engine.key_tips = self.key_tips;
        if !design && !self.qat_loaded {
            self.qat_loaded = true;
            if let Some(path) = qat_settings_path(&self.qat_key) {
                let ids: Vec<String> = load_qat(&path).iter().filter_map(|n| self.elements.iter().find(|e| e.name.as_deref() == Some(n.as_str())).map(|e| e.id.clone())).collect();
                if !ids.is_empty() {
                    engine.qat_custom = ids;
                }
            }
        }
        engine.qat_position = if self.qat_below { kubuno_desktop_ui::ribbon::QatPosition::BelowRibbon } else { kubuno_desktop_ui::ribbon::QatPosition::InTabStrip };
        engine.display_mode = if self.simplified { kubuno_desktop_ui::ribbon::DisplayMode::Simplified } else { kubuno_desktop_ui::ribbon::DisplayMode::Classic };
        engine.design = design.then(|| self.design_state());
        // `SelectedTab` / `IsMinimized` bound: the model wins when it changes.
        let wanted = self.selected.resolve(&*cx.vm);
        if !wanted.is_empty() && self.last_tab.as_deref() != Some(wanted.as_str()) {
            if let Some(i) = self.elements.iter().position(|e| e.name.as_deref() == Some(wanted.as_str())) {
                let id = self.elements[i].id.clone();
                engine.select(&id);
            }
            self.last_tab = Some(wanted);
        }
        // A bound `IsMinimized`: the model wins when it changes.
        let minimized = self.minimized.resolve(&*cx.vm);
        if self.minimized.binding().is_some() && minimized != engine.is_collapsed() && !cx.frame.mouse_down {
            engine.set_collapsed(minimized);
        }
        // Keyboard shortcuts of the commands (before the focused control reads its keys).
        let mut shortcut_hits = Vec::new();
        if !design {
            for (i, c) in self.commands.iter().enumerate() {
                if let Some((key, mods)) = parse_shortcut(&c.shortcut) {
                    if host::take_key(key, mods) > 0 {
                        shortcut_hits.push(i);
                    }
                }
            }
        }
        // An accessibility client's Invoke on one of the elements (published by `publish_access`):
        // a tab is selected, anything else is clicked.
        self.activated.clear();
        if !design {
            if let Some(services) = cx.services.as_deref_mut() {
                for e in &self.elements {
                    if services.take_activation(&e.id) {
                        if matches!(e.meta.name, "RibbonTab" | "RibbonBackstage") {
                            engine.select(&e.id);
                        }
                        self.activated.push(e.id.clone());
                    }
                }
            }
        }
        let canvas: &dyn Canvas = cx.canvas;
        // The designer's width preview: the ribbon laid out narrower, the rest of its band shaded.
        let full = bounds;
        let preview = design && self.preview_width > 0.0 && bounds.left + self.preview_width < bounds.right;
        let bounds = if preview { Rect::new(bounds.left, bounds.top, bounds.left + self.preview_width, bounds.bottom) } else { bounds };
        let run = engine.frame(canvas, bounds, cx.frame);
        if preview {
            let shade = Rect::new(bounds.right, full.top, full.right, full.top + run.height);
            let label = format!("{} px", self.preview_width.round());
            crate::virtual_regions::defer_late(
                Vec::new(),
                Box::new(move |c| {
                    c.fill_rect(&shade, &windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F { r: 0.5, g: 0.5, b: 0.5, a: 0.35 });
                    c.fill_rect(&Rect::new(shade.left, shade.top, shade.left + 2.0, shade.bottom), &kubuno_desktop_ui::ribbon::hex(0xD13438));
                    let tag = Rect::new(shade.left + 6.0, shade.top + 4.0, shade.left + 90.0, shade.top + 22.0);
                    c.text(&label, &tag, &c.formats().caption, &kubuno_desktop_ui::ribbon::hex(0xD13438), false);
                }),
            );
        }
        if design && self.owns_caption && self.follows {
            crate::virtual_regions::set_design_caption(engine.caption_colors(canvas));
        }
        let backstage_open = run.backstage;
        let mut regions = run.regions.clone();
        // The Backstage over the page: its rail and the active tab's view.
        let mut bs_rows = Vec::new();
        let mut bs_content = None;
        if backstage_open {
            let mut bs = self.backstage.borrow_mut();
            let active_before = bs.active().to_string();
            bs.sections = sections;
            bs.design = design;
            if design {
                let selection = crate::virtual_regions::design_selection();
                if let Some(s) = bs.sections.iter().find(|s| s.view && selection.iter().any(|x| *x == s.id || x.starts_with(&format!("{}.", s.id)))).map(|s| s.id.clone()) {
                    bs.set_active(&s);
                }
            }
            if bs.active().is_empty() || !bs.sections.iter().any(|s| s.id == bs.active()) {
                if let Some(first) = bs.sections.iter().find(|s| s.view).map(|s| s.id.clone()) {
                    bs.set_active(&first);
                }
            }
            let _ = active_before;
            let theme = engine.theme;
            let b = bs.frame(canvas, run.content, cx.frame, &theme);
            if b.back {
                engine.close_backstage();
            }
            bs_rows = b.rows.clone();
            bs_content = Some((b.active.clone(), b.content));
            if let Some(action) = b.action {
                drop(bs);
                regions.push(kubuno_desktop_ui::ribbon::RibbonRegion { id: action.clone(), kind: RegionKind::Item, rect: Rect::default(), parent: None });
                let mut events = vec![RibbonEvent::Clicked(action)];
                events.extend(run.events.clone());
                let height = bounds.bottom - bounds.top;
                self.height.set(height);
                drop(engine);
                self.after_frame(cx, events, &regions, &bs_rows, bs_content, &shortcut_hits);
                return;
            }
        }
        let height = if backstage_open { (bounds.bottom - bounds.top).max(run.height) + 10_000.0 } else { run.height };
        if (self.height.get() - height).abs() > 0.5 {
            self.height.set(height);
            host::request_repaint_after(0);
        }
        if !design {
            self.access_names = access_names(&engine);
        }
        let overlay = engine.design_overlay_rect();
        drop(engine);
        if let (true, Some(_)) = (design, overlay) {
            let eng = self.engine.clone();
            // Entries of the inline drop-down: recorded late, above the page.
            let entries: Vec<crate::design::LayoutEntry> = regions
                .iter()
                .filter(|r| matches!(r.kind, RegionKind::MenuEntry))
                .filter_map(|r| {
                    let i = self.index_of(&r.id)?;
                    let e = &self.elements[i];
                    Some(crate::design::LayoutEntry { id: e.id.clone(), parent_id: crate::design::parent_id_of(&e.id), bounds: r.rect, layout: crate::registry::LayoutKind::None, container: e.container, locked: false, clip: None })
                })
                .collect();
            crate::virtual_regions::defer_late(entries, Box::new(move |c| eng.borrow().paint_design_overlay(c)));
        }
        if design {
            self.design_glyphs(bounds, &regions);
        }
        let mut events = run.events;
        for id in std::mem::take(&mut self.activated) {
            match self.index_of(&id).map(|i| self.elements[i].meta.name) {
                Some("RibbonTab") => events.push(RibbonEvent::TabChanged(id)),
                Some("RibbonBackstage") => {}
                _ => events.push(RibbonEvent::Clicked(id)),
            }
        }
        self.after_frame(cx, events, &regions, &bs_rows, bs_content, &shortcut_hits);
    }
}

impl RibbonNode {
    /// The designer's glyphs (`RIBBON.md` §9): each « + » the engine drew asks for the « Ajouter ▾ »
    /// menu of its container (the ribbon, a tab, a group), and the selected ribbon element gets a
    /// smart tag (its tasks).
    fn design_glyphs(&self, bounds: Rect, regions: &[kubuno_desktop_ui::ribbon::RibbonRegion]) {
        use crate::virtual_regions::{push_design_glyph, DesignGlyph};
        for r in regions {
            let element_id = match r.kind {
                RegionKind::AddTab => self.own_id.clone(),
                RegionKind::AddGroup | RegionKind::AddItem => r.id.clone(),
                _ => continue,
            };
            push_design_glyph(DesignGlyph { rect: r.rect, element_id, menu: "add" });
        }
        let selection = crate::virtual_regions::design_selection();
        let Some(primary) = selection.first() else { return };
        let target = if *primary == self.own_id {
            Some(Rect::new(bounds.right - 18.0, bounds.top + 4.0, bounds.right - 4.0, bounds.top + 18.0))
        } else {
            let tasks = self.index_of(primary).is_some_and(|i| SMART_TAGGED.contains(&self.elements[i].meta.name));
            regions
                .iter()
                .filter(|_| tasks)
                .find(|r| r.id == *primary && matches!(r.kind, RegionKind::Tab | RegionKind::Group | RegionKind::Item))
                .map(|r| Rect::new(r.rect.right + 2.0, r.rect.top, r.rect.right + 16.0, r.rect.top + 14.0))
        };
        if let Some(rect) = target {
            push_design_glyph(DesignGlyph { rect, element_id: primary.clone(), menu: "tasks" });
            crate::virtual_regions::defer_late(Vec::new(), Box::new(move |c| crate::virtual_regions::paint_smart_tag(c, rect)));
        }
    }

    /// Reports the regions, paints the Backstage tab's view, dispatches the events and runs the
    /// shortcuts.
    #[allow(clippy::too_many_arguments)]
    fn after_frame(&mut self, cx: &mut PaintCx<'_>, events: Vec<RibbonEvent>, regions: &[kubuno_desktop_ui::ribbon::RibbonRegion], bs_rows: &[(String, Rect)], bs_content: Option<(String, Rect)>, shortcuts: &[usize]) {
        let vm_enabled: Vec<bool> = (0..self.elements.len()).map(|i| self.node(i).is_none_or(|n| n.attrs.enabled.resolve(&*cx.vm))).collect();
        self.report(cx, regions, &|i| vm_enabled.get(i).copied().unwrap_or(true));
        if cx.design.is_none() {
            self.publish_access(cx, regions);
        }
        for (id, rect) in bs_rows {
            if let Some(i) = self.index_of(id) {
                self.elements[i].report(cx, *rect, vm_enabled[i]);
            }
        }
        if let Some((active, content)) = bs_content {
            if let Some(i) = self.index_of(&active) {
                let id = self.elements[i].id.clone();
                fn find_mut<'a>(nodes: &'a mut [Node], elements: &[VirtualElement], id: &str) -> Option<&'a mut Node> {
                    for n in nodes {
                        if elements[n.el].id == id {
                            return Some(n);
                        }
                        if let Some(f) = find_mut(&mut n.children, elements, id) {
                            return Some(f);
                        }
                    }
                    None
                }
                if let Some(view) = find_mut(&mut self.roots, &self.elements, &id).and_then(|n| n.view.as_mut()) {
                    let pad = 32.0;
                    let r = Rect::new(content.left + pad, content.top + 24.0, content.right - pad, content.bottom - 16.0);
                    let mut inner = cx.reborrow();
                    view.paint(&mut inner, r);
                }
            }
        }
        if cx.design.is_none() {
            self.dispatch(cx, events, regions);
            for &i in shortcuts {
                let name = self.commands[i].name.clone();
                self.execute(cx, usize::MAX, &name, Rect::default());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::Runtime;

    const VIEW: &str = r#"<Panel DesignWidth="1000" DesignHeight="400">
  <Command x:Name="cmd_bold" Label="Gras" SmallIcon="Bold" Shortcut="Ctrl+B" IsCheckable="true" Checked="{Binding Bold, Mode=TwoWay}" OnExecute="bold"/>
  <Ribbon x:Name="ribbon" Dock="Top" OnSelectedTabChanged="tab_changed">
    <RibbonQuickAccessToolbar><RibbonButton x:Name="save" SmallIcon="Save" Label="Enregistrer"/></RibbonQuickAccessToolbar>
    <RibbonBackstage Header="Fichier"><BackstageTab Header="Informations" Icon="Info"><Label Text="Infos"/></BackstageTab><BackstageSeparator/><BackstageButton Label="Fermer" SmallIcon="X" OnClick="close"/></RibbonBackstage>
    <RibbonTab x:Name="home" Header="Accueil">
      <RibbonGroup x:Name="clip" Header="Presse-papiers">
        <RibbonButton x:Name="paste" Label="Coller" LargeIcon="ClipboardPaste" Size="Large" OnClick="paste"/>
        <RibbonButton x:Name="cut" Label="Couper" SmallIcon="Scissors"/>
      </RibbonGroup>
      <RibbonGroup x:Name="font" Header="Police">
        <RibbonControlGroup><RibbonToggleButton x:Name="bold" Command="cmd_bold" ShowLabel="false"/></RibbonControlGroup>
      </RibbonGroup>
    </RibbonTab>
    <RibbonContextualTabGroup x:Name="ctx" Header="Outils de tableau" Visible="{Binding Table}">
      <RibbonTab x:Name="table" Header="Disposition"><RibbonGroup Header="Lignes"><RibbonButton Label="Insérer"/></RibbonGroup></RibbonTab>
    </RibbonContextualTabGroup>
  </Ribbon>
</Panel>"#;

    #[test]
    fn size_definitions_are_auto_custom_and_the_engine_templates() {
        let mut expected = vec!["Auto", "Custom"];
        expected.extend(kubuno_desktop_ui::ribbon::scaling::Template::names());
        assert_eq!(SIZE_DEFINITIONS, expected.as_slice());
    }

    #[test]
    fn a_ribbon_view_validates_and_compiles() {
        let mut rt = Runtime::new();
        assert!(rt.reload_from_text(VIEW), "{:?}", rt.diagnostics());
    }

    #[test]
    fn ribbon_elements_are_controls_of_the_ribbon_levels() {
        let chain = |n: &str| crate::registry::lookup(n).map(|m| m.base_chain().to_vec()).unwrap_or_default();
        assert_eq!(chain("RibbonButton"), ["RibbonButton", "RibbonItem", "RibbonControl", "Control", "Component"]);
        assert_eq!(chain("RibbonColorPicker"), ["RibbonColorPicker", "RibbonSplitButton", "RibbonMenuButton", "RibbonButton", "RibbonItem", "RibbonControl", "Control", "Component"]);
        assert_eq!(chain("RibbonTab"), ["RibbonTab", "RibbonControl", "Control", "Component"]);
        assert_eq!(chain("Command"), ["Command", "Component"]);
        assert!(crate::registry::is_non_visual("Command"));
        let tab = crate::registry::lookup("RibbonTab").unwrap();
        assert!(tab.property("Dock").is_some_and(|p| !p.browsable), "the layout properties are hidden");
        assert!(tab.event("OnMouseDown").is_some(), "every element raises the common events");
    }

    #[test]
    fn misplaced_ribbon_elements_are_diagnosed() {
        let mut rt = Runtime::new();
        assert!(!rt.reload_from_text(r#"<Panel><RibbonButton Label="x"/></Panel>"#));
        assert!(!rt.reload_from_text(r#"<Panel><Ribbon><RibbonButton Label="x"/></Ribbon></Panel>"#));
    }

    #[test]
    fn the_quick_access_toolbar_round_trips_through_its_settings_file() {
        assert!(qat_settings_path("").is_none());
        assert!(qat_settings_path("../evil").is_none());
        let path = qat_settings_path("ribbon-test").expect("a path");
        assert!(path.ends_with("ribbon-test.qat"));
        let dir = std::env::temp_dir().join(format!("kubuno-qat-{}", std::process::id()));
        let file = dir.join("main.qat");
        save_qat(&file, &["cmd_save".to_string(), "paste".to_string()]);
        assert_eq!(load_qat(&file), vec!["cmd_save", "paste"]);
        let _ = std::fs::remove_dir_all(&dir);
        assert!(load_qat(&file).is_empty());
    }

    #[test]
    fn ribbon_warnings_cover_policies_key_tips_and_commands() {
        let text = r#"<Panel>
  <Command x:Name="cmd_ok" Label="Ok"/>
  <Ribbon>
    <RibbonTab Header="A" KeyTip="H">
      <RibbonTab.ScalingPolicy>
        <Scale Group="font" Size="Small"/>
        <Scale Group="font" Size="Medium"/>
        <Scale Group="nope" Size="Small"/>
      </RibbonTab.ScalingPolicy>
      <RibbonGroup x:Name="font" Header="Police">
        <RibbonButton Label="X" KeyTip="B" Command="cmd_ok"/>
        <RibbonButton Label="Y" KeyTip="b" Command="cmd_missing"/>
      </RibbonGroup>
    </RibbonTab>
    <RibbonTab Header="B" KeyTip="H"/>
  </Ribbon>
</Panel>"#;
        let parse = crate::syntax::parse(text);
        let messages: Vec<String> = crate::validate::warnings(&parse).into_iter().map(|d| d.message).collect();
        let has = |s: &str| messages.iter().any(|m| m.contains(s));
        assert!(has("grows back"), "{messages:?}");
        assert!(has("`nope`"), "{messages:?}");
        assert!(has("`B` is already the KeyTip of another control"), "{messages:?}");
        assert!(has("`H` is already the KeyTip of another tab"), "{messages:?}");
        assert!(has("cmd_missing") && !has("\"cmd_ok\""), "{messages:?}");
        assert_eq!(messages.len(), 5, "{messages:?}");
    }

    /// IconColor / IconSize / IconScaling travel with a ribbon element's icon (`docs/ICONS.md`).
    #[test]
    fn icon_options_reach_the_ribbon_item() {
        let parsed = crate::syntax::parse(r##"<RibbonButton Label="Gras" SmallIcon="Bold" IconColor="#FF0000" IconSize="24"/>"##);
        let el = crate::ast::Document::cast(parsed.syntax()).and_then(|d| d.root_element()).expect("a root element");
        let meta = crate::registry::lookup("RibbonButton").expect("RibbonButton");
        let attrs = Attrs::read(&el, meta).expect("attributes");
        assert_ne!(attrs.small_icon, "Bold", "the options are composed into the icon");
        let painted = icon(&attrs.small_icon).expect("resolves");
        assert!(painted.as_ref().contains("Bold"), "{painted}");
        assert!(painted.as_ref().to_ascii_lowercase().contains("ff0000"), "{painted}");
    }

    /// A Backstage tab's and a group's `Icon` is their icon (they inherit `SmallIcon` too, unset).
    #[test]
    fn the_icon_property_of_tabs_and_groups_is_read() {
        for (xml, name) in [(r#"<BackstageTab Header="Informations" Icon="Info"/>"#, "BackstageTab"), (r#"<RibbonGroup Header="Police" Icon="Bold"/>"#, "RibbonGroup")] {
            let parsed = crate::syntax::parse(xml);
            let el = crate::ast::Document::cast(parsed.syntax()).and_then(|d| d.root_element()).expect("a root element");
            let attrs = Attrs::read(&el, crate::registry::lookup(name).expect(name)).expect("attributes");
            assert!(icon(&attrs.small_icon).is_some_and(|i| i.as_ref().contains(if name == "BackstageTab" { "Info" } else { "Bold" })), "{name}: {}", attrs.small_icon);
        }
    }

    #[test]
    fn shortcuts_parse() {
        let (k, m) = parse_shortcut("Ctrl+Shift+B").unwrap();
        assert_eq!(k, host::vk::letter('B'));
        assert!(m.ctrl && m.shift && !m.alt);
        assert_eq!(parse_shortcut("F5").map(|x| x.0), Some(host::vk::F5));
        assert!(parse_shortcut("Ctrl+Nope").is_none());
    }
}
