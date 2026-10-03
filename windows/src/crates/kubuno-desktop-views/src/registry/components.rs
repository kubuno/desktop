//! The five example components phase 2a declares, read straight off
//! `kubuno-desktop-ui`'s real builder API (`src/crates/kubuno-desktop-ui/src/buttons.rs`,
//! `containers.rs`, `text.rs`). Each `component!` call's `smoke` block is a
//! real, compiled call into `kubuno-desktop-ui`; see `registry/mod.rs`'s module doc
//! for why that is what stands in for the design note's runtime-erased
//! setter table in this phase.
//!
//! Property names are `PascalCase`, matching `XML_VIEWS.md` §1 ("attributes
//! are the builder's fluent setters" as XML sees them, e.g. `Dense="true"`,
//! `Text="Ok"`) — not the Rust setters' own `snake_case` spelling.

use super::macros::component;
use super::ComponentMeta;

component! {
    mod_name: button,
    name: "Button",
    // Note: A push button (`kubuno_desktop_controls::buttons::Button` replica).
    doc: "A push button.",
    ctor: kubuno_desktop_ui::buttons::Button::new("Envoyer"),
    children: ChildrenModel::None,
    props: [
        // Note: The label (the replica's `text` field, set directly through `Deref`).
        PropertyMeta::new("Text", PropKind::String, "",
            "Text displayed on the button.",
        ),
        PropertyMeta::new("Variant",
            PropKind::Enum(&["Primary", "Secondary", "Ghost", "Text", "Danger", "TextDanger"]),
            "Primary",
            "Visual style: filled, outlined, ghost, text only or danger.",
        ),
        PropertyMeta::new("Size", PropKind::Enum(&["Sm", "Md", "Lg"]), "Md",
            "Height and horizontal padding of the button.",
        ),
        PropertyMeta::new("Icon", PropKind::String, "",
            "Icon shown before the text: a name of the Kubuno icon set, or an image file (SVG, PNG…) relative to the view.",
        ).editor("icon").category("Icon"),
        PropertyMeta::new("Loading", PropKind::Bool, "false",
            "Shows a spinner instead of the content and disables the button.",
        ),
        PropertyMeta::new("DropDownMenu", PropKind::String, "", "A ContextMenu of the view that a click opens below the button (a drop-down button).").category("Behavior").editor("reference:ContextMenu"),
    ],
    events: [
        EventMeta::new("OnClick", "Occurs when the button is clicked or activated with Space or Enter.").args::<crate::events::MouseEventArgs>(),
    ],
    smoke: |mut b| {
        b.text = "Go".into();
        let b = b.variant(kubuno_desktop_ui::buttons::Variant::Secondary);
        let b = b.size(kubuno_desktop_ui::buttons::Size::Lg);
        let b = b.icon("check");
        b.loading(true)
    },
    build: |props, cx| {
        let text = props.str("Text", "")?;
        let variant = props.enum_("Variant", "Primary")?;
        let size = props.enum_("Size", "Md")?;
        let icon = props.str("Icon", "")?;
        let loading = props.bool("Loading", false)?;
        let focus_id = props.focus_id();
        let on_click = props.event("OnClick");
        let base = crate::common::ButtonBaseProps::read(props, cx.base_dir.as_deref());
        let drop_down = match props.str("DropDownMenu", "")? {
            crate::binding::PropSource::Literal(m) => Some(m),
            crate::binding::PropSource::Bound { .. } => None,
        };
        Ok(Box::new(ButtonNode::new(text, variant, size, icon, loading, focus_id, on_click).with_base(base).with_drop_down(drop_down)) as Box<dyn ViewNode>)
    },
}

component! {
    mod_name: switch,
    name: "Switch",
    // Note: A toggle switch (`kubuno_desktop_controls::buttons::CheckBox`-backed replica).
    doc: "An on/off switch.",
    ctor: kubuno_desktop_ui::buttons::Switch::new(),
    children: ChildrenModel::None,
    default_event: "OnCheckedChanged",
    props: [
        PropertyMeta::new("On", PropKind::Bool, "false", "Whether the switch is on."),
        PropertyMeta::new("Label", PropKind::String, "", "Text displayed next to the switch."),
        // Note: A secondary line under the label, `text-xs text-text-secondary`.
        PropertyMeta::new("Description", PropKind::String, "",
            "Secondary text displayed under the label.",
        ),
        PropertyMeta::new("Size", PropKind::Enum(&["Sm", "Md"]), "Md", "Size of the switch."),
        PropertyMeta::new("CheckAlign", PropKind::Enum(crate::registry::common::CONTENT_ALIGNMENTS), "MiddleLeft", "Where the switch sits against its label: MiddleLeft before it, MiddleRight at the right end of the control after it (a settings row).").category("Appearance"),
    ],
    events: [
        EventMeta::new("OnCheckedChanged", "Occurs when the switch is turned on or off.").category(crate::registry::EventCategory::PropertyChanged).args::<crate::events::CheckedChangedEventArgs>().aliases(&["OnToggled"]),
    ],
    smoke: |s| {
        let s = s.on(true);
        let s = s.label("Notifications");
        let s = s.description("Reçoit un e-mail par nouvelle activité.");
        s.with_size(kubuno_desktop_ui::buttons::SwitchSize::Sm)
    },
    build: |props, _cx| {
        let on = props.bool("On", false)?;
        let label = props.str("Label", "")?;
        let description = props.str("Description", "")?;
        let size = props.enum_("Size", "Md")?;
        let focus_id = props.focus_id();
        let check_align = props.enum_("CheckAlign", "MiddleLeft")?;
        let on_toggled = props.event("OnToggled");
        Ok(Box::new(SwitchNode::new(on, label, description, size, focus_id, on_toggled).with_check_align(check_align)) as Box<dyn ViewNode>)
    },
}

component! {
    mod_name: text_field,
    name: "TextField",
    // Note: A single-line text input (`kubuno_desktop_controls::text::TextBox` replica).
    doc: "A single-line text box.",
    ctor: kubuno_desktop_ui::text::TextField::new(),
    children: ChildrenModel::None,
    default_event: "OnTextChanged",
    props: [
        PropertyMeta::new("Text", PropKind::String, "", "Text in the field."),
        PropertyMeta::new("Placeholder", PropKind::String, "", "Hint shown while the field is empty."),
        PropertyMeta::new("Invalid", PropKind::Bool, "false",
            "Shows the field in the error colour.",
        ),
    ],
    events: [
        EventMeta::new("OnTextChanged", "Occurs when the text changes.").category(crate::registry::EventCategory::PropertyChanged).args::<crate::events::TextChangedEventArgs>().aliases(&["OnChanged"]),
    ],
    smoke: |mut f| {
        f.set_text("hello");
        f.placeholder_text = "Serveur".into();
        f.invalid = true;
        f
    },
    build: |props, _cx| {
        let text = props.str("Text", "")?;
        let placeholder = props.str("Placeholder", "")?;
        let invalid = props.bool("Invalid", false)?;
        let focus_id = props.focus_id();
        let on_changed = props.event("OnChanged");
        let text_box = crate::common::TextBoxProps::read(props)?;
        Ok(Box::new(TextFieldNode::new(text, placeholder, invalid, focus_id, on_changed).with_text_box(text_box)) as Box<dyn ViewNode>)
    },
}

component! {
    mod_name: card,
    name: "Card",
    // Note: A titled surface (`kubuno_desktop_ui::containers::Card`, wraps a `Panel`).
    doc: "A card with an optional title that holds one child.",
    ctor: kubuno_desktop_ui::containers::Card::new(),
    children: ChildrenModel::SingleWidget,
    props: [
        PropertyMeta::new("Title", PropKind::String, "", "Title shown in the card header. Leave empty to hide the header."),
        PropertyMeta::new("Subtitle", PropKind::String, "", "Secondary text shown under the title."),
        PropertyMeta::new("Dense", PropKind::Bool, "false",
            "Uses tighter spacing and a smaller title.",
        ),
        PropertyMeta::new("Flush", PropKind::Bool, "false",
            "Removes the body padding, so a table or list reaches the card edges.",
        ),
        // Note: The card's ground: the Kubuno card, the console's current `Layer` look (`on_layer`), or the card lifted off the page with a shadow (`raised`).
        PropertyMeta::new("Surface",
            PropKind::Enum(&["Card", "Layer", "Raised"]),
            "Card",
            "Background of the card: standard, layer, or raised with a shadow.",
        ),
    ],
    events: [],
    smoke: |mut c| {
        c.set_title("Réglages");
        let c = c.with_subtitle("Compte et sécurité");
        let c = c.dense();
        let c = c.flush();
        c.on_layer()
    },
    build: |props, cx| {
        let title = props.str("Title", "")?;
        let subtitle = props.str("Subtitle", "")?;
        let dense = props.bool("Dense", false)?;
        let flush = props.bool("Flush", false)?;
        let surface = props.enum_("Surface", "Card")?;
        let child = props.build_single_child(cx)?;
        Ok(Box::new(CardNode { title, subtitle, dense, flush, surface, child }) as Box<dyn ViewNode>)
    },
}

component! {
    mod_name: stack,
    name: "Stack",
    // Note: A flow column/row of fixed-extent blocks (`kubuno_desktop_ui::containers::Stack`).
    doc: "Arranges its children one after another, in a row or a column.",
    ctor: kubuno_desktop_ui::containers::Stack::column(8.0),
    children: ChildrenModel::List(&[]),
    layout: LayoutKind::Flow,
    props: [
        // Note: The axis blocks run along — the real `FlowDirection` variant names (the design note's friendlier `Horizontal`/`Vertical` aliases are an interpreter-level naming convenience, not modelled yet; see the crate's top-level docs' deviation list).
        PropertyMeta::new("Direction",
            PropKind::Enum(&["LeftToRight", "TopDown", "RightToLeft", "BottomUp"]),
            "TopDown",
            "Direction in which the children are placed.",
        ),
        PropertyMeta::new("Gap", PropKind::F32, "8", "Space between two children, in pixels."),
        PropertyMeta::new("Padding", PropKind::F32, "0", "Space around the children, on all four sides, in pixels."),
        PropertyMeta::new("Surface",
            PropKind::Enum(&["None", "Layer", "Card", "Raised", "Well"]),
            "None",
            "Background painted behind the children.",
        ),
        PropertyMeta::new("WrapContents", PropKind::Bool, "false", "Wraps the children onto several rows (or columns) when they do not fit, like a FlowLayoutPanel.").category("Layout"),
        PropertyMeta::new("CrossAlign", PropKind::Enum(&["Stretch", "Start", "Center", "End"]), "Stretch", "How the children are placed across the flow: stretched to the row (or column), or at their own size at its start, centre or end.").category("Layout"),
    ],
    events: [],
    smoke: |mut s| {
        s.direction = kubuno_desktop_controls::layout_panels::FlowDirection::LeftToRight;
        s.gap = 12.0;
        let s = s.with_padding(kubuno_desktop_ui::Padding::all(16.0));
        s.with_surface(kubuno_desktop_ui::containers::Surface::Layer)
    },
    build: |props, cx| {
        let direction = props.enum_("Direction", "TopDown")?;
        let gap = props.f32("Gap", 8.0)?;
        let padding = props.f32("Padding", 0.0)?;
        let surface = props.enum_("Surface", "None")?;
        let wrap = props.bool("WrapContents", false)?;
        let align = props.enum_("CrossAlign", "Stretch")?;
        // `Stack.Fill="true"` on a child: it takes the room the others leave.
        let fills: Vec<bool> = props.element().children().map(|c| c.attribute("Stack.Fill").and_then(|a| a.value()).is_some_and(|v| v.trim() == "true")).collect();
        let children = props
            .build_children_sized(cx)?
            .into_iter()
            .zip(fills)
            .map(|((node, explicit_height, explicit_width), fill)| crate::node::StackChild { node, explicit_height, explicit_width, fill })
            .collect();
        Ok(Box::new(StackNode { direction, gap, padding, surface, wrap, align, children }) as Box<dyn ViewNode>)
    },
}

/// Every declared component, in declaration order — the source [`super::all`]
/// hands back.
pub const ALL: &[ComponentMeta] =
    &[button::META, switch::META, text_field::META, card::META, stack::META];
