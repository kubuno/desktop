//! The properties each level of the control hierarchy declares (`vskubuno/docs/EVENTS.md` §16,
//! "WinForms-rich property sets"): every control inherits [`CONTROL_PROPERTIES`], a button the
//! [`BUTTON_BASE_PROPERTIES`] too, and so on — like `System.Windows.Forms.Control` and its bases. The
//! view's root element also takes the [`VIEW_PROPERTIES`] (the `Form`'s). Categories are WinForms'
//! (`Accessibility`, `Appearance`, `Behavior`, `Data`, `Design`, `Focus`, `Layout`, `Window Style`,
//! `Misc`); a component's own property of the same name wins (its own default or kind).
//!
//! The runtime honours them in `crate::common` (every element), in the nodes of the families
//! (the control-specific ones) and in the host window (the view's).

use super::{PropKind, PropertyMeta};

/// The .NET `AccessibleRole` values.
pub const ACCESSIBLE_ROLES: &[&str] = &[
    "Default", "None", "TitleBar", "MenuBar", "ScrollBar", "Grip", "Sound", "Cursor", "Caret", "Alert", "Window", "Client", "MenuPopup",
    "MenuItem", "ToolTip", "Application", "Document", "Pane", "Chart", "Dialog", "Border", "Grouping", "Separator", "ToolBar", "StatusBar",
    "Table", "ColumnHeader", "RowHeader", "Column", "Row", "Cell", "Link", "HelpBalloon", "Character", "List", "ListItem", "Outline",
    "OutlineItem", "PageTab", "PropertyPage", "Indicator", "Graphic", "StaticText", "Text", "PushButton", "CheckButton", "RadioButton",
    "ComboBox", "DropList", "ProgressBar", "Dial", "HotkeyField", "Slider", "SpinButton", "Diagram", "Animation", "Equation",
    "ButtonDropDown", "ButtonMenu", "ButtonDropDownGrid", "WhiteSpace", "PageTabList", "Clock", "SplitButton", "IpAddress", "OutlineButton",
];

/// The nine-cell alignment (`ContentAlignment`).
pub const CONTENT_ALIGNMENTS: &[&str] =
    &["TopLeft", "TopCenter", "TopRight", "MiddleLeft", "MiddleCenter", "MiddleRight", "BottomLeft", "BottomCenter", "BottomRight"];

const ACCESSIBILITY: &str = "Accessibility";
const APPEARANCE: &str = "Appearance";
const BEHAVIOR: &str = "Behavior";
const DATA: &str = "Data";
const DESIGN: &str = "Design";
const FOCUS: &str = "Focus";
const LAYOUT: &str = "Layout";
const WINDOW_STYLE: &str = "Window Style";
const MISC: &str = "Misc";
const TITLE_BAR: &str = "Title Bar";
/// The Properties window group of an icon and how it is drawn (« Icône »).
pub const ICON: &str = "Icon";

const fn p(name: &'static str, kind: PropKind, default: &'static str, doc: &'static str, category: &'static str) -> PropertyMeta {
    PropertyMeta::new(name, kind, default, doc).category(category)
}

/// What every control inherits from `Control`.
pub const CONTROL_PROPERTIES: &[PropertyMeta] = &[
    // Accessibility
    p("AccessibleName", PropKind::String, "", "Name that screen readers announce for the control. Leave empty to use its text.", ACCESSIBILITY).localizable(),
    p("AccessibleDescription", PropKind::String, "", "Description that screen readers announce for the control.", ACCESSIBILITY).localizable(),
    p("AccessibleRole", PropKind::Enum(ACCESSIBLE_ROLES), "Default", "Kind of element that screen readers announce. Default uses the control's own kind.", ACCESSIBILITY),
    // Appearance
    p("BackColor", PropKind::String, "", "Background colour: a theme colour (it follows the light and dark themes) or a colour of your choice. Leave empty to use the parent's.", APPEARANCE).editor("color").type_converter("Color"),
    p("ForeColor", PropKind::String, "", "Text colour: a theme colour (it follows the light and dark themes) or a colour of your choice. Leave empty to use the parent's.", APPEARANCE).editor("color").type_converter("Color"),
    p("Font", PropKind::String, "", "Font of the text: family, size and style. Leave empty to use the parent's.", APPEARANCE).editor("font").type_converter("Font"),
    p("Cursor", PropKind::Enum(crate::style::CURSORS), "Default", "Mouse pointer shown over the control.", APPEARANCE).editor("cursor"),
    p("RightToLeft", PropKind::Enum(&["No", "Yes", "Inherit"]), "Inherit", "Shows the text from right to left, for languages such as Arabic or Hebrew. Inherit uses the parent's setting.", APPEARANCE),
    p("BackgroundImage", PropKind::String, "", "Image painted behind the control's content: the path of an image file, relative to the view.", APPEARANCE).editor("image"),
    p("BackgroundImageLayout", PropKind::Enum(&["None", "Tile", "Center", "Stretch", "Zoom"]), "Tile", "How the background image fills the control.", APPEARANCE),
    // Behavior
    p("Enabled", PropKind::Bool, "true", "Whether the control reacts to the mouse and the keyboard. A disabled control is shown greyed, with everything inside it.", BEHAVIOR).bindable(),
    p("Visible", PropKind::Bool, "true", "Whether the control is shown when the application runs. The designer still shows it.", BEHAVIOR).bindable(),
    p("TabIndex", PropKind::F32, "0", "Position of the control in the order the Tab key follows, among the controls of the same container.", BEHAVIOR),
    p("TabStop", PropKind::Bool, "true", "Whether the Tab key stops on the control.", BEHAVIOR),
    p("ContextMenu", PropKind::String, "", "Menu shown when the control is right-clicked: the name of a ContextMenu of the view.", BEHAVIOR).editor("reference:ContextMenu"),
    p("AllowDrop", PropKind::Bool, "false", "Whether data (files, text…) can be dragged onto the control: it then raises DragEnter, DragOver, DragLeave and DragDrop.", BEHAVIOR),
    p("UseWaitCursor", PropKind::Bool, "false", "Shows the busy pointer over the control and everything inside it.", BEHAVIOR).bindable(),
    // Misc (the ToolTip extender, like WinForms' "ToolTip on toolTip1")
    p("ToolTip", PropKind::String, "", "Text of the tooltip shown when the mouse rests on the control.", MISC).localizable().bindable(),
    // Data
    p("Tag", PropKind::String, "", "Any text you want to keep with the control, for your own code.", DATA),
    // Design (read by the designer only)
    p("Locked", PropKind::Bool, "false", "Prevents the control from being moved or resized in the designer.", DESIGN).design_time(),
    p("GenerateMember", PropKind::Bool, "true", "Whether your code can find the control by its name.", DESIGN).design_time(),
    p("Modifiers", PropKind::Enum(&["Private", "Protected", "Internal", "ProtectedInternal", "Public"]), "Private", "Who may use the control from code outside the view; Protected or Public also lets the views inheriting this one change it.", DESIGN).design_time(),
    // Focus
    p("CausesValidation", PropKind::Bool, "true", "Whether moving the focus to this control first validates the control that had it.", FOCUS),
    // Layout
    p("X", PropKind::F32, "0", "Distance from the left edge of the parent panel, in pixels.", LAYOUT),
    p("Y", PropKind::F32, "0", "Distance from the top edge of the parent panel, in pixels.", LAYOUT),
    p("Width", PropKind::F32, "", "Width of the element, in pixels.", LAYOUT),
    p("Height", PropKind::F32, "", "Height of the element, in pixels.", LAYOUT),
    p("Dock", PropKind::Enum(&["None", "Top", "Bottom", "Left", "Right", "Fill"]), "None", "Edge of the parent panel the element is docked to, or Fill to take the remaining space.", LAYOUT),
    p("Anchor", PropKind::String, "Top, Left", "Edges of the parent panel the element stays attached to when it is resized, for example Top, Left.", LAYOUT),
    p("Margin", PropKind::String, "0, 0, 0, 0", "Space kept around the control by the container that lines it up with others: left, top, right, bottom, in pixels.", LAYOUT).type_converter("Padding"),
    p("Padding", PropKind::String, "0, 0, 0, 0", "Space inside the control, around its content: left, top, right, bottom, in pixels.", LAYOUT).type_converter("Padding"),
    p("MinimumSize", PropKind::String, "0, 0", "Smallest size of the control: width, height in pixels (0 means no limit).", LAYOUT).type_converter("Size"),
    p("MaximumSize", PropKind::String, "0, 0", "Largest size of the control: width, height in pixels (0 means no limit).", LAYOUT).type_converter("Size"),
    p("AutoSize", PropKind::Bool, "false", "Sizes the control to its content instead of its Width and Height.", LAYOUT),
    p("AutoSizeMode", PropKind::Enum(&["GrowOnly", "GrowAndShrink"]), "GrowOnly", "With AutoSize, whether the control may also become smaller than its Width and Height.", LAYOUT),
    // Attached to a child of the view's root: placed in the window's title bar or action bar.
    p("TitleBar.Region", PropKind::Enum(TITLE_BAR_REGIONS), "None", "On a control of the view's top level: puts it in the window's title bar, at its left, centre or right (tabs, a search field, an avatar, a menu button).", LAYOUT),
    p("TitleBar.Drag", PropKind::Bool, "false", "Makes the control an area that moves the window when dragged: in the title bar, or anywhere in a window without border.", LAYOUT),
    p("ActionBar.Region", PropKind::Enum(ACTION_BAR_REGIONS), "None", "On a control of the view's top level: puts it in the window's action bar, the footer of a dialog (buttons on the right, a check box or a link on the left).", LAYOUT),
    // Attached to a child of a `<Stack>`: it fills the room the other children leave.
    p("Stack.Fill", PropKind::Bool, "false", "On a child of a Stack: it takes the room the other children leave along the flow.", LAYOUT),
    // Attached to a child of a `<TableLayoutPanel>`: its cell.
    p("TableLayoutPanel.Row", PropKind::F32, "", "On a child of a TableLayoutPanel: its row (0 is the first); empty for the next free cell.", LAYOUT),
    p("TableLayoutPanel.Column", PropKind::F32, "", "On a child of a TableLayoutPanel: its column (0 is the first); empty for the next free cell.", LAYOUT),
    p("TableLayoutPanel.RowSpan", PropKind::F32, "1", "On a child of a TableLayoutPanel: how many rows it spans.", LAYOUT),
    p("TableLayoutPanel.ColumnSpan", PropKind::F32, "1", "On a child of a TableLayoutPanel: how many columns it spans.", LAYOUT),
];

/// What the buttons (`Button`, `CheckBox`, `RadioButton`, `IconButton`, `Switch`) inherit from `ButtonBase`.
pub const BUTTON_BASE_PROPERTIES: &[PropertyMeta] = &[
    p("TextAlign", PropKind::Enum(CONTENT_ALIGNMENTS), "MiddleCenter", "Position of the text in the control.", APPEARANCE),
    p("Image", PropKind::String, "", "Image shown on the control: the path of an image file, relative to the view.", APPEARANCE).editor("image"),
    p("ImageAlign", PropKind::Enum(CONTENT_ALIGNMENTS), "MiddleCenter", "Position of the image or the icon in the control (with TextImageRelation Overlay; otherwise TextAlign places the icon and the text together).", ICON),
    p("TextImageRelation", PropKind::Enum(&["Overlay", "ImageAboveText", "TextAboveImage", "ImageBeforeText", "TextBeforeImage"]), "Overlay", "Where the image or the icon goes relative to the text. An icon goes before the text when this is not set.", ICON),
    p("IconSpacing", PropKind::F32, "", "Space between the icon (or the image) and the text, in pixels. Leave empty for the button's own spacing.", ICON),
    p("UseMnemonic", PropKind::Bool, "true", "Treats a letter after an ampersand (&Save) as the control's keyboard shortcut: Alt and that letter activate it, and the letter is underlined while Alt is held.", APPEARANCE),
    p("UseVisualStyleBackColor", PropKind::Bool, "true", "Paints the face of the button in the theme's colours, ignoring BackColor. Set to false to use BackColor.", APPEARANCE),
];

/// What the text displays (`Label`, `LinkLabel`, `Badge`) inherit from `LabelBase`.
pub const LABEL_BASE_PROPERTIES: &[PropertyMeta] = &[
    p("TextAlign", PropKind::Enum(CONTENT_ALIGNMENTS), "TopLeft", "Position of the text in the control.", APPEARANCE),
    p("UseMnemonic", PropKind::Bool, "true", "Treats a letter after an ampersand (&Name) as a keyboard shortcut: Alt and that letter move to the next control in the Tab order.", APPEARANCE),
    p("BorderStyle", PropKind::Enum(&["None", "FixedSingle", "Fixed3D"]), "None", "Border drawn around the control.", APPEARANCE),
    p("Image", PropKind::String, "", "Image shown in the control: the path of an image file, relative to the view.", APPEARANCE).editor("image"),
    p("ImageAlign", PropKind::Enum(CONTENT_ALIGNMENTS), "MiddleCenter", "Position of the image in the control.", APPEARANCE),
];

/// What the text fields (`TextField`, `TextArea`, `MaskedField`, `SearchField`) inherit from `TextBoxBase`.
pub const TEXT_BOX_BASE_PROPERTIES: &[PropertyMeta] = &[
    p("ReadOnly", PropKind::Bool, "false", "Lets the text be selected and copied but not changed.", BEHAVIOR).bindable(),
    p("MaxLength", PropKind::F32, "32767", "Largest number of characters that can be typed.", BEHAVIOR),
    p("AcceptsTab", PropKind::Bool, "false", "Types a tab character when Tab is pressed, instead of moving to the next control.", BEHAVIOR),
    p("PasswordChar", PropKind::String, "", "Character shown instead of each typed character, for a password. Leave empty to show the text.", BEHAVIOR),
    p("CharacterCasing", PropKind::Enum(&["Normal", "Upper", "Lower"]), "Normal", "Changes the typed letters to capitals or small letters.", BEHAVIOR),
    p("HideSelection", PropKind::Bool, "true", "Hides the selected text while the field does not have the focus.", BEHAVIOR),
    p("TextAlign", PropKind::Enum(&["Left", "Right", "Center"]), "Left", "Alignment of the text in the field.", APPEARANCE),
];

/// What the lists (`ListBox`, `CheckedListBox`, `ComboBox`, `Dropdown`) inherit from `ListControl`.
pub const LIST_CONTROL_PROPERTIES: &[PropertyMeta] = &[
    p("Sorted", PropKind::Bool, "false", "Shows the items in alphabetical order.", BEHAVIOR),
];

/// What the value-in-a-range controls (`Slider`, `ProgressBar`, `NumericField`) inherit from `RangeBase`.
pub const RANGE_BASE_PROPERTIES: &[PropertyMeta] = &[];

/// What the scrolling controls inherit from `ScrollableControl`.
pub const SCROLLABLE_PROPERTIES: &[PropertyMeta] = &[
    p("AutoScroll", PropKind::Bool, "false", "Shows scroll bars when the content is larger than the control.", LAYOUT),
];

/// What the layout containers (`Panel`, `GroupBox`, `Card`, `Stack`…) inherit from `ContainerBase`.
pub const CONTAINER_BASE_PROPERTIES: &[PropertyMeta] = &[
    p("BorderStyle", PropKind::Enum(&["None", "FixedSingle", "Fixed3D"]), "None", "Border drawn around the container.", APPEARANCE),
];

/// The view's own properties (the `Form`'s), accepted on its ROOT element only and applied to the
/// window that shows it.
pub const VIEW_PROPERTIES: &[PropertyMeta] = &[
    p("Title", PropKind::String, "", "Text of the window's title bar, also shown in the task bar.", APPEARANCE).localizable().bindable(),
    p("Icon", PropKind::String, "", "Icon of the window, in its title bar, the task bar and Alt+Tab: a name of the Kubuno icon set, or an image file (.ico, .svg, .png…) relative to the view.", WINDOW_STYLE).editor("icon"),
    p("StartPosition", PropKind::Enum(&["Manual", "CenterScreen", "WindowsDefaultLocation", "WindowsDefaultBounds", "CenterParent"]), "WindowsDefaultLocation", "Where the window opens. Manual uses the view's X and Y.", LAYOUT),
    p("FormBorderStyle", PropKind::Enum(&["None", "FixedSingle", "Fixed3D", "FixedDialog", "Sizable", "FixedToolWindow", "SizableToolWindow"]), "Sizable", "Border and title bar of the window, and whether it can be resized.", APPEARANCE),
    p("ControlBox", PropKind::Bool, "true", "Shows the buttons of the title bar (minimize, maximize, close) and the window menu.", WINDOW_STYLE),
    p("MinimizeBox", PropKind::Bool, "true", "Shows the minimize button of the title bar.", WINDOW_STYLE),
    p("MaximizeBox", PropKind::Bool, "true", "Shows the maximize button of the title bar.", WINDOW_STYLE),
    p("ShowInTaskbar", PropKind::Bool, "true", "Shows the window in the Windows task bar.", WINDOW_STYLE),
    p("TopMost", PropKind::Bool, "false", "Keeps the window above the other windows.", WINDOW_STYLE).bindable(),
    p("Opacity", PropKind::F32, "100", "Opacity of the window, in percent: 100 is opaque, lower values let what is behind show through.", WINDOW_STYLE).type_converter("Opacity").bindable(),
    p("WindowState", PropKind::Enum(&["Normal", "Minimized", "Maximized"]), "Normal", "Whether the window opens normal, minimized or maximized.", LAYOUT).bindable(),
    p("AcceptButton", PropKind::String, "", "Button clicked when Enter is pressed in the window: the name of a button of the view.", MISC).editor("reference:ButtonBase"),
    p("CancelButton", PropKind::String, "", "Button clicked when Escape is pressed in the window: the name of a button of the view.", MISC).editor("reference:ButtonBase"),
    p("KeyPreview", PropKind::Bool, "false", "Lets the view receive the key events before the control that has the focus.", MISC),
    p("AutoScroll", PropKind::Bool, "false", "Shows scroll bars when the view is larger than the window.", LAYOUT),
    // Window kinds and WinForms' remaining Form properties.
    p("WindowKind", PropKind::Enum(WINDOW_KINDS), "Form", "Kind of window: a main window, a modal dialog, a tool window, a splash screen, a flyout or an MDI document. It presets the border, the title bar buttons and where the window opens; a property set on the view wins.", WINDOW_STYLE),
    p("ShowIcon", PropKind::Bool, "true", "Shows the window's icon in its title bar.", WINDOW_STYLE),
    p("HelpButton", PropKind::Bool, "false", "Shows a help button in the title bar; clicking it raises HelpButtonClicked.", WINDOW_STYLE),
    p("SizeGripStyle", PropKind::Enum(&["Auto", "Show", "Hide"]), "Auto", "Resize grip at the bottom-right corner of the window. Auto shows it when the window can be resized.", WINDOW_STYLE),
    p("TransparencyKey", PropKind::String, "", "Colour that is transparent in the window: what is behind shows through and receives the clicks.", WINDOW_STYLE).editor("color").type_converter("Color"),
    p("ResizeBorder", PropKind::Bool, "false", "Lets a window without border (FormBorderStyle None) still be resized by its edges.", WINDOW_STYLE),
    p("RightToLeftLayout", PropKind::Bool, "false", "Mirrors the window for right-to-left languages: the title bar buttons go to the left.", APPEARANCE),
    p("IsMdiContainer", PropKind::Bool, "false", "Makes the window the parent of MDI documents: child windows open inside its client area.", WINDOW_STYLE),
    p("SplashDuration", PropKind::F32, "3000", "For a splash screen: how long it stays, in milliseconds, before it closes itself (0 keeps it open).", BEHAVIOR),
    // The Kubuno window chrome.
    p("Chrome", PropKind::Enum(&["Default", "Kubuno", "System", "None"]), "Default", "Who draws the title bar: Kubuno's, Windows' own, or nobody (the view draws its own). Default uses the application's setting.", WINDOW_STYLE),
    p("Backdrop", PropKind::Enum(&["Default", "None", "Mica", "MicaAlt", "Acrylic"]), "Default", "Windows 11 material behind the window: Mica, Mica Alt or Acrylic. It shows where the view leaves the window transparent.", WINDOW_STYLE),
    p("CornerPreference", PropKind::Enum(&["Default", "Round", "RoundSmall", "DoNotRound"]), "Default", "Corners of the window as a preset: Round (8 pixels, Windows 11's), RoundSmall (4) or DoNotRound (square). Default rounds a window with a title bar at 8 and leaves a borderless one square (a splash screen is rounded). CornerRadius, when set, wins.", APPEARANCE),
    p("CornerRadius", PropKind::F32, "8", "Radius of the window's corners, in pixels (8 by default, like Windows 11; 0 for square corners). A maximized, snapped or full-screen window is always square. Without it, CornerPreference decides (a borderless window is square). On a flyout, a radius above 0 shows it as a floating panel over a blur of what is behind it, with a drop shadow (the web's app launcher), tinted by the view's translucent BackColor.", APPEARANCE).type_converter("CornerRadius"),
    p("BorderColor", PropKind::String, "", "Colour of the window's thin border. Leave empty to use the title bar's colour.", WINDOW_STYLE).editor("color").type_converter("Color"),
    p("AccentColor", PropKind::String, "", "Accent colour of this window: its title bar and its accent-coloured controls. Leave empty to use the theme's.", APPEARANCE).editor("color").type_converter("Color"),
    p("TitleBarHeight", PropKind::F32, "50", "Height of the title bar, in pixels (50 by default, 32 for a tool window).", TITLE_BAR),
    p("TitleBarPadding", PropKind::F32, "16", "Space between the window's left and right edges and what the title bar holds at its ends (its icon or its left controls, its own buttons), in pixels (16 by default, 8 for a tool window).", TITLE_BAR),
    p("TitleBarBackground", PropKind::String, "", "Colour of the title bar. Leave empty to use the accent colour.", TITLE_BAR).editor("color").type_converter("Color"),
    p("TitleBarFollowsRibbon", PropKind::Bool, "true", "When the view has a ribbon and no title bar colour of its own, the title bar takes the colour of the ribbon's tab strip, so the two read as one band.", TITLE_BAR),
    p("TitleBarForeground", PropKind::String, "", "Colour of the title, the icon and the buttons of the title bar. Leave empty for white on the accent.", TITLE_BAR).editor("color").type_converter("Color"),
    p("Subtitle", PropKind::String, "", "Second, smaller text shown after the title in the title bar.", TITLE_BAR).localizable().bindable(),
    p("TitleAlignment", PropKind::Enum(&["Left", "Center"]), "Left", "Where the title sits in the title bar.", TITLE_BAR),
    p("ShowTitle", PropKind::Bool, "true", "Shows the title in the title bar (hide it when the title bar holds tabs or a search field).", TITLE_BAR),
    p("CaptionButtonStyle", PropKind::Enum(&["Kubuno", "Windows"]), "Kubuno", "Look of the title bar buttons: Kubuno's rounded buttons, or Windows' wide ones with a red close button.", TITLE_BAR),
    p("CaptionButtons", PropKind::String, "", "Buttons of your own in the title bar, next to minimize: id:Icon:Tooltip, separated by semicolons (pin:Pin:Keep on top; settings:Settings2:Settings). Clicking one raises CaptionButtonClick.", TITLE_BAR),
    p("ExtendContentIntoTitleBar", PropKind::Bool, "false", "Lets the view draw under the title bar: the title and the buttons are drawn over it.", TITLE_BAR),
    // The header's standard items (the web header's search button and HeaderActions), placed at the end of the
    // title bar's right region. Off unless set, for every kind of window.
    p("ShowSearch", PropKind::Bool, "false", "Shows the search button (a magnifier) before the header's other buttons in the title bar; clicking it raises SearchClicked.", TITLE_BAR).bindable(),
    p("ShowNotifications", PropKind::Bool, "false", "Shows the notifications bell in the title bar, with the UnreadCount counter; clicking it raises NotificationsClicked.", TITLE_BAR).bindable(),
    p("ShowSettings", PropKind::Bool, "false", "Shows the settings button in the title bar; clicking it raises SettingsClicked.", TITLE_BAR).bindable(),
    p("ShowHelp", PropKind::Bool, "false", "Shows the header's help button in the title bar; clicking it raises HelpClicked.", TITLE_BAR).bindable(),
    p("ShowWaffle", PropKind::Bool, "false", "Shows the apps launcher (the waffle) in the title bar; it opens the app launcher.", TITLE_BAR).bindable(),
    p("ShowAccount", PropKind::Bool, "false", "Shows the account avatar in the title bar; it opens the account panel.", TITLE_BAR).bindable(),
    p("UnreadCount", PropKind::F32, "0", "Number of unread notifications shown on the bell (0 shows no counter).", TITLE_BAR).bindable(),
];

/// How an icon is drawn: what every element with an icon property (`editor("icon")`) accepts too —
/// honoured by every control that paints an icon, carried with the icon value as its drawing options
/// (`crate::icon::with_options`).
pub const ICON_OPTION_PROPERTIES: &[PropertyMeta] = &[
    p("IconSize", PropKind::String, "", "Size of the icon: Small (16), Medium (20), Large (24), XLarge (32), a number of pixels, or width, height. Leave empty for the control's own size.", ICON).type_converter("IconSize"),
    p("IconScaling", PropKind::Enum(&["Fit", "Fill", "Stretch", "None"]), "Fit", "How an image that is not square fills the icon's box: Fit shows all of it, Fill covers the box, Stretch fits it to the box exactly, None keeps its own size.", ICON),
    p("IconColor", PropKind::String, "", "Colour of the icon, every pixel recoloured (for a one-colour icon). Leave empty for the control's colour: a glyph and an SVG drawn in currentColor follow it and the theme, another image keeps its colours.", ICON).editor("color").type_converter("Color"),
];

/// The kinds of window a view can be (`WindowKind`).
pub const WINDOW_KINDS: &[&str] = &["Form", "Dialog", "ToolWindow", "Splash", "Flyout", "MdiChild"];

/// The title-bar and action-bar regions a child of the view's root can be placed in.
pub const TITLE_BAR_REGIONS: &[&str] = &["None", "Left", "Center", "Right"];
pub const ACTION_BAR_REGIONS: &[&str] = &["None", "Left", "Right"];

/// The view property (root element only) named `name`.
pub fn view_property(name: &str) -> Option<&'static PropertyMeta> {
    VIEW_PROPERTIES.iter().find(|p| p.matches(name))
}

/// The WinForms category of a component's own property that does not declare one (the built-in
/// families' tables): by name, then a switch is a behaviour, anything else is Misc.
pub fn default_category(name: &str, kind: PropKind) -> &'static str {
    match name {
        "Dock" | "Anchor" | "X" | "Y" | "Width" | "Height" | "MaxWidth" | "Padding" | "Gap" | "Direction" | "Orientation" | "Align" | "Distance"
        | "Band" | "Overflow" | "Dense" | "Compact" | "Flush" | "Diameter" => LAYOUT,
        "Text" | "Label" | "Title" | "Subtitle" | "Header" | "Description" | "Placeholder" | "Body" | "Icon" | "Glyph" | "Variant" | "Size"
        | "Surface" | "Color" | "Corner" | "Filled" | "Dot" | "Status" | "Role" | "ShowIcon" | "ShowValue" | "ActionLabel"
        | "SecondaryActionLabel" | "Format" | "Mask" | "CornerRadius" | "Caption" | "Name" => APPEARANCE,
        "ItemsSource" | "DisplayMember" | "ValueMember" | "Value" | "SelectedIndex" | "SelectedValue" | "SelectedPath" | "SelectedDate" | "Date"
        | "CurrentIndex" | "Binding" | "Minimum" | "Maximum" | "SmallChange" | "LargeChange" | "Today" | "Checked" | "On" | "CheckState" => DATA,
        _ if kind == PropKind::Bool => BEHAVIOR,
        _ => MISC,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_level_property_has_a_known_category_and_a_valid_default() {
        let known = [ACCESSIBILITY, APPEARANCE, BEHAVIOR, DATA, DESIGN, FOCUS, LAYOUT, WINDOW_STYLE, MISC, TITLE_BAR, ICON];
        for table in [ICON_OPTION_PROPERTIES, CONTROL_PROPERTIES, BUTTON_BASE_PROPERTIES, LABEL_BASE_PROPERTIES, TEXT_BOX_BASE_PROPERTIES, LIST_CONTROL_PROPERTIES, SCROLLABLE_PROPERTIES, CONTAINER_BASE_PROPERTIES, VIEW_PROPERTIES] {
            let mut seen = std::collections::HashSet::new();
            for prop in table {
                assert!(seen.insert(prop.name), "{} declared twice", prop.name);
                assert!(prop.category.is_some_and(|c| known.contains(&c)), "{}", prop.name);
                if let PropKind::Enum(values) = prop.kind {
                    assert!(values.contains(&prop.default), "{}: default {} not a value", prop.name, prop.default);
                }
                if prop.kind == PropKind::Bool {
                    assert!(prop.default == "true" || prop.default == "false", "{}", prop.name);
                }
            }
        }
        assert!(view_property("Opacity").is_some() && view_property("Text").is_none());
        assert_eq!(default_category("Placeholder", PropKind::String), APPEARANCE);
        assert_eq!(default_category("Loading", PropKind::Bool), BEHAVIOR);
    }
}
