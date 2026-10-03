//! The non-visual components (EVT-7b of `vskubuno/docs/EVENTS.md`, WinForms' component tray):
//! elements a view declares that paint nothing — the designer lists them under its surface.

#[allow(unused_imports)] // Used by the `component!` invocation below.
use crate::registry::macros::component;
use crate::registry::ComponentMeta;
#[allow(unused_imports)]
use crate::node::ViewNode;

component! {
    mod_name: timer,
    name: "Timer",
    // Note: `crate::node::custom::TimerNode`: ticks on the view's frames, the host woken for each one.
    doc: "A component that raises an event at regular intervals.",
    ctor: (),
    children: ChildrenModel::None,
    default_event: "OnTick",
    props: [
        PropertyMeta::new("Interval", PropKind::F32, "100", "The time between two ticks, in milliseconds.").category("Behavior"),
        PropertyMeta::new("Enabled", PropKind::Bool, "false", "Whether the timer is running.").category("Behavior"),
    ],
    events: [
        EventMeta::new("OnTick", "Occurs when the interval has elapsed while the timer is enabled.").category(crate::registry::EventCategory::Behavior),
    ],
    smoke: |t| { t },
    build: |props, _cx| {
        Ok(Box::new(crate::node::custom::TimerNode::new(
            props.f32("Interval", 100.0)?,
            props.bool("Enabled", false)?,
            props.focus_id(),
            props.event("OnTick"),
        )) as Box<dyn ViewNode>)
    },
}

component! {
    mod_name: tool_tip,
    name: "ToolTip",
    // Note: the extender of the `ToolTip` property every control has (WinForms' ToolTip component):
    // its settings apply to the tooltips of the view (`crate::window`).
    doc: "A component that sets how the tooltips of the view appear. Each control's tooltip text is its ToolTip property.",
    ctor: (),
    children: ChildrenModel::None,
    props: [
        PropertyMeta::new("InitialDelay", PropKind::F32, "500", "How long the mouse must rest on a control before its tooltip appears, in milliseconds.").category("Behavior"),
        PropertyMeta::new("AutoPopDelay", PropKind::F32, "5000", "How long a tooltip stays visible while the mouse rests on its control, in milliseconds.").category("Behavior"),
        PropertyMeta::new("ReshowDelay", PropKind::F32, "100", "How long it takes for the next tooltip to appear when the mouse moves from one control to another, in milliseconds.").category("Behavior"),
        PropertyMeta::new("ShowAlways", PropKind::Bool, "false", "Shows the tooltips even when the window is not active.").category("Behavior"),
        PropertyMeta::new("Active", PropKind::Bool, "true", "Whether the tooltips are shown at all.").category("Behavior"),
    ],
    events: [],
    smoke: |t| { t },
    build: |_props, _cx| {
        Ok(Box::new(crate::node::custom::NonVisualNode) as Box<dyn ViewNode>)
    },
}

component! {
    mod_name: context_menu,
    name: "ContextMenu",
    // Note: opened by the runtime (`crate::window`) on a right click (or Shift+F10, the context-menu key) on a
    // control whose `ContextMenu` names it (WinForms' ContextMenuStrip); shown open in the designer while selected
    // (`crate::menus::ContextMenuNode`).
    doc: "A menu shown when a control that names it in its ContextMenu property is right-clicked (or gets Shift+F10 or the context-menu key), when a button names it in its DropDownMenu property, or from code (show). Add its commands as MenuItem elements, with MenuSeparator and MenuHeader between them; MenuItem children make a sub-menu.",
    ctor: (),
    children: ChildrenModel::List(&["MenuItem", "MenuSeparator", "MenuHeader"]),
    default_event: "OnOpening",
    props: [
        crate::owner_draw::OWNER_DRAW,
        PropertyMeta::new("ItemsSource", PropKind::String, "", "Commands added from a list when the menu opens (fields Text, Key, Icon, ShortcutKeys, Checked, Enabled, Danger, ToolTip, and Kind = Separator or Header), or made from its ItemTemplate; choosing one raises OnItemClicked with its key.").category("Data").bindable().editor("list"),
    ],
    events: [
        EventMeta::new("OnOpening", "Occurs when the menu is about to open.").category(crate::registry::EventCategory::Behavior),
        EventMeta::new("OnClosed", "Occurs when the menu has closed.").category(crate::registry::EventCategory::Behavior),
        EventMeta::new("OnItemClicked", "Occurs when a command made from ItemsSource is chosen: its key is the new text.").category(crate::registry::EventCategory::Action).args::<crate::events::TextChangedEventArgs>(),
        crate::owner_draw::ON_DRAW_ITEM,
    ],
    smoke: |m| { m },
    build: |props, _cx| {
        Ok(Box::new(crate::menus::ContextMenuNode::build(props.element())) as Box<dyn ViewNode>)
    },
}

component! {
    mod_name: menu_item,
    name: "MenuItem",
    // Note: a row of a menu (`<ContextMenu>`, `<MenuBar>`, drop-down button); read by `crate::window::read_item`.
    doc: "A command of a menu. MenuItem children make it a sub-menu. Its Command runs a Command of the view and takes its text, icon, shortcut and state from it; its ShortcutKeys run it while the menu is closed.",
    ctor: (),
    children: ChildrenModel::List(&["MenuItem", "MenuSeparator", "MenuHeader"]),
    default_event: "OnClick",
    props: [
        PropertyMeta::new("Text", PropKind::String, "", "Text of the command. An ampersand before a letter makes it its keyboard shortcut in the menu.").category("Appearance").localizable().bindable(),
        PropertyMeta::new("Icon", PropKind::String, "", "Icon shown before the text: a name of the Kubuno icon set, or an image file (SVG, PNG…) relative to the view.").editor("icon").category("Icon"),
        PropertyMeta::new("Kind", PropKind::Enum(&["Command", "Separator", "Header"]), "Command", "A command, a separator line, or a section title (MenuSeparator and MenuHeader say it shorter).").category("Appearance"),
        PropertyMeta::new("Danger", PropKind::Bool, "false", "A destructive command, shown in the danger colour.").category("Appearance"),
        PropertyMeta::new("Enabled", PropKind::Bool, "true", "Whether the command can be chosen.").category("Behavior").bindable(),
        PropertyMeta::new("Visible", PropKind::Bool, "true", "Whether the command is shown.").category("Behavior").bindable(),
        PropertyMeta::new("Checked", PropKind::Bool, "false", "Shows a check mark before the command.").category("Appearance").bindable(),
        PropertyMeta::new("CheckOnClick", PropKind::Bool, "false", "Choosing the command toggles its check mark.").category("Behavior"),
        PropertyMeta::new("RadioGroup", PropKind::String, "", "Commands with the same group are exclusive: choosing one checks it and unchecks the others.").category("Behavior"),
        PropertyMeta::new("ShortcutKeys", PropKind::String, "", "Keyboard shortcut that runs the command while its menu is closed, shown next to it: modifiers then a key, such as Ctrl+S, Ctrl+Shift+N, Alt+F4 or F5.").category("Misc").editor("shortcut"),
        PropertyMeta::new("ShortcutKeyDisplayString", PropKind::String, "", "Text shown in place of the shortcut (the shortcut itself still works).").category("Misc").localizable(),
        PropertyMeta::new("ShowShortcutKeys", PropKind::Bool, "true", "Shows the shortcut next to the command.").category("Misc"),
        PropertyMeta::new("ToolTip", PropKind::String, "", "Text shown when the pointer rests on the command.").category("Behavior").localizable().bindable(),
        PropertyMeta::new("Command", PropKind::String, "", "The Command of the view the item runs: its text, icon, shortcut, enabled and checked state come from it, and choosing the item raises its OnExecute.").category("Behavior").editor("reference:Command"),
        PropertyMeta::new("ItemsSource", PropKind::String, "", "Sub-menu commands added from a list when it opens (see the ContextMenu's ItemsSource), or made from its ItemTemplate.").category("Data").bindable().editor("list"),
    ],
    events: [
        EventMeta::new("OnClick", "Occurs when the command is chosen.").args::<crate::events::MouseEventArgs>(),
        EventMeta::new("OnCheckedChanged", "Occurs when choosing the command changed its check mark (CheckOnClick, RadioGroup).").category(crate::registry::EventCategory::PropertyChanged).args::<crate::events::CheckedChangedEventArgs>(),
        EventMeta::new("OnDropDownOpening", "Occurs when its sub-menu is about to open: fill it now (its ItemsSource is read after).").category(crate::registry::EventCategory::Behavior).args::<crate::events::CancelEventArgs>(),
    ],
    smoke: |i| { i },
    build: |_props, _cx| {
        Ok(Box::new(crate::node::custom::NonVisualNode) as Box<dyn ViewNode>)
    },
}


/// Every non-visual component (and the menu's items).
pub const ALL: &[ComponentMeta] = &[timer::META, tool_tip::META, context_menu::META, menu_item::META];
