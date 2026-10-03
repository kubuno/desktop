//! The menu family (`vskubuno/docs/MENUS.md`): the menu bar, the drop-down and split buttons, and the
//! separators and headers of every menu. `<ContextMenu>` and `<MenuItem>` are components
//! ([`super::components`]); [`crate::menus`] holds what they all share at run time and in the
//! designer.

#[allow(unused_imports)] // Used by the `component!` invocations below.
use crate::registry::macros::component;
use crate::registry::ComponentMeta;
#[allow(unused_imports)]
use crate::node::ViewNode;

/// What a menu holds (`<ContextMenu>`, `<MenuItem>`, `<DropDownButton>`, `<SplitButton>`).
pub const MENU_CHILDREN: &[&str] = crate::window::MENU_CHILDREN;

component! {
    mod_name: menu_bar,
    name: "MenuBar",
    // Note: `crate::menus::MenuBarNode`, the web's `WorkspaceMenuBar` (28 DIP) or PaintSharp's compact bar (24 DIP).
    doc: "The menu bar of a window (WinForms MenuStrip): a row of menus, each a MenuItem whose MenuItem children are its commands. Alt or F10 moves to it from the keyboard; Alt + the underlined letter opens a menu.",
    ctor: kubuno_desktop_ui::workspace::MenuBar::new(Vec::new(), kubuno_desktop_ui::workspace::MenuBarStyle::Workspace),
    children: ChildrenModel::List(&["MenuItem"]),
    default_event: "OnMenuActivate",
    props: [
        PropertyMeta::new("Style", PropKind::Enum(&["Workspace", "Compact"]), "Workspace", "Workspace: the editors' bar, 28 pixels high. Compact: a 24-pixel bar in the theme's header colour.").category("Appearance"),
    ],
    events: [
        EventMeta::new("OnMenuActivate", "Occurs when the bar takes the keyboard or the pointer opens one of its menus.").category(crate::registry::EventCategory::Behavior),
        EventMeta::new("OnMenuDeactivate", "Occurs when the bar gives the keyboard back and its menus are closed.").category(crate::registry::EventCategory::Behavior),
    ],
    smoke: |b| { b },
    build: |props, _cx| {
        let compact = crate::common::literal_attr(props, "Style").as_deref() == Some("Compact");
        Ok(Box::new(crate::menus::MenuBarNode::build(props.element(), compact, props.focus_id())) as Box<dyn ViewNode>)
    },
}

component! {
    mod_name: menu_separator,
    name: "MenuSeparator",
    // Note: read by `crate::window::read_item` (a separator row).
    doc: "A line between the commands of a menu.",
    ctor: (),
    children: ChildrenModel::None,
    props: [
        PropertyMeta::new("Visible", PropKind::Bool, "true", "Whether the line is shown.").category("Behavior").bindable(),
    ],
    events: [],
    smoke: |s| { s },
    build: |_props, _cx| {
        Ok(Box::new(crate::node::custom::NonVisualNode) as Box<dyn ViewNode>)
    },
}

component! {
    mod_name: menu_header,
    name: "MenuHeader",
    // Note: read by `crate::window::read_item` (the web's `{ type: 'label' }` row).
    doc: "A section title in a menu: a short line of text above a group of commands, never chosen.",
    ctor: (),
    children: ChildrenModel::None,
    props: [
        PropertyMeta::new("Text", PropKind::String, "", "Text of the title (shown in capitals).").category("Appearance").localizable().bindable(),
        PropertyMeta::new("Visible", PropKind::Bool, "true", "Whether the title is shown.").category("Behavior").bindable(),
    ],
    events: [],
    smoke: |h| { h },
    build: |_props, _cx| {
        Ok(Box::new(crate::node::custom::NonVisualNode) as Box<dyn ViewNode>)
    },
}

component! {
    mod_name: drop_down_button,
    name: "DropDownButton",
    // Note: `crate::menus::DropDownButtonNode` (split: false).
    doc: "A button that opens a menu below it (WinForms ToolStripDropDownButton): its MenuItem children, or the ContextMenu its DropDownMenu names.",
    ctor: kubuno_desktop_ui::buttons::Button::new("Menu"),
    children: ChildrenModel::List(crate::registry::families::menus::MENU_CHILDREN),
    default_event: "OnDropDownOpening",
    props: [
        PropertyMeta::new("Text", PropKind::String, "", "Text displayed on the button. An ampersand before a letter makes it its keyboard shortcut.").localizable().bindable(),
        PropertyMeta::new("Variant", PropKind::Enum(&["Primary", "Secondary", "Ghost", "Text", "Danger", "TextDanger"]), "Secondary", "Visual style: filled, outlined, ghost, text only or danger."),
        PropertyMeta::new("Size", PropKind::Enum(&["Sm", "Md", "Lg"]), "Md", "Height and horizontal padding of the button."),
        PropertyMeta::new("Icon", PropKind::String, "", "Icon shown before the text: a name of the Kubuno icon set, or an image file (SVG, PNG…) relative to the view.").editor("icon").category("Icon"),
        PropertyMeta::new("DropDownMenu", PropKind::String, "", "A ContextMenu of the view the button opens, when it holds no MenuItem of its own.").category("Behavior").editor("reference:ContextMenu"),
        PropertyMeta::new("ShowDropDownArrow", PropKind::Bool, "true", "Shows the arrow after the text.").category("Appearance"),
    ],
    events: [
        EventMeta::new("OnDropDownOpening", "Occurs when the menu is about to open: fill it now.").category(crate::registry::EventCategory::Behavior).args::<crate::events::CancelEventArgs>(),
        EventMeta::new("OnDropDownClosed", "Occurs when the menu has closed.").category(crate::registry::EventCategory::Behavior),
    ],
    smoke: |b| { b.variant(kubuno_desktop_ui::buttons::Variant::Secondary) },
    build: |props, cx| {
        Ok(Box::new(crate::registry::families::menus::drop_down(props, cx, false)?) as Box<dyn ViewNode>)
    },
}

component! {
    mod_name: split_button,
    name: "SplitButton",
    // Note: `crate::menus::DropDownButtonNode` (split: true).
    doc: "A button with its own action and an arrow that opens a menu (WinForms ToolStripSplitButton): its MenuItem children, or the ContextMenu its DropDownMenu names.",
    ctor: kubuno_desktop_ui::buttons::Button::new("Action"),
    children: ChildrenModel::List(crate::registry::families::menus::MENU_CHILDREN),
    default_event: "OnClick",
    props: [
        PropertyMeta::new("Text", PropKind::String, "", "Text displayed on the button. An ampersand before a letter makes it its keyboard shortcut.").localizable().bindable(),
        PropertyMeta::new("Variant", PropKind::Enum(&["Primary", "Secondary", "Ghost", "Text", "Danger", "TextDanger"]), "Secondary", "Visual style: filled, outlined, ghost, text only or danger."),
        PropertyMeta::new("Size", PropKind::Enum(&["Sm", "Md", "Lg"]), "Md", "Height and horizontal padding of the button."),
        PropertyMeta::new("Icon", PropKind::String, "", "Icon shown before the text: a name of the Kubuno icon set, or an image file (SVG, PNG…) relative to the view.").editor("icon").category("Icon"),
        PropertyMeta::new("DropDownMenu", PropKind::String, "", "A ContextMenu of the view the arrow opens, when the button holds no MenuItem of its own.").category("Behavior").editor("reference:ContextMenu"),
        PropertyMeta::new("DefaultItem", PropKind::String, "", "The x:Name of the item a click on the button runs when it has no OnClick handler.").category("Behavior"),
    ],
    events: [
        EventMeta::new("OnClick", "Occurs when the button (not its arrow) is clicked or activated with Space or Enter.").args::<crate::events::MouseEventArgs>(),
        EventMeta::new("OnDropDownOpening", "Occurs when the menu is about to open: fill it now.").category(crate::registry::EventCategory::Behavior).args::<crate::events::CancelEventArgs>(),
        EventMeta::new("OnDropDownClosed", "Occurs when the menu has closed.").category(crate::registry::EventCategory::Behavior),
    ],
    smoke: |b| { b.variant(kubuno_desktop_ui::buttons::Variant::Secondary) },
    build: |props, cx| {
        Ok(Box::new(crate::registry::families::menus::drop_down(props, cx, true)?) as Box<dyn ViewNode>)
    },
}

/// Reads a `<DropDownButton>` or a `<SplitButton>`.
pub(crate) fn drop_down(props: &crate::props::Props<'_>, cx: &mut crate::props::BuildCx, split: bool) -> Result<crate::menus::DropDownButtonNode, crate::props::BuildError> {
    let base = crate::common::ButtonBaseProps::read(props, cx.base_dir.as_deref());
    let drop_down = crate::common::literal_attr(props, "DropDownMenu");
    Ok(crate::menus::DropDownButtonNode::build(
        props.element(),
        split,
        props.str("Text", "")?,
        props.enum_("Variant", "Secondary")?,
        props.enum_("Size", "Md")?,
        props.str("Icon", "")?,
        base,
        props.focus_id(),
        drop_down,
    ))
}

/// Every element of the family.
pub const ALL: &[ComponentMeta] = &[menu_bar::META, menu_separator::META, menu_header::META, drop_down_button::META, split_button::META];

/// The French documentation of the family and of the menu components (`registry::docs_fr`), keyed
/// `Element` or `Element.Member`; it wins over the general table.
pub fn french(key: &str) -> Option<&'static str> {
    Some(match key {
        "MenuBar" => "Barre de menus d'une fenêtre (MenuStrip de WinForms) : une rangée de menus, chacun un MenuItem dont les MenuItem enfants sont les commandes. Alt ou F10 y mène au clavier ; Alt + la lettre soulignée ouvre un menu.",
        "MenuBar.Style" => "Workspace : la barre des éditeurs, haute de 28 pixels. Compact : une barre de 24 pixels dans la couleur d'en-tête du thème.",
        "MenuBar.OnMenuActivate" => "Se produit quand la barre prend le clavier ou que la souris ouvre l'un de ses menus.",
        "MenuBar.OnMenuDeactivate" => "Se produit quand la barre rend le clavier et que ses menus sont fermés.",
        "MenuSeparator" => "Ligne entre les commandes d'un menu.",
        "MenuSeparator.Visible" => "Indique si la ligne est affichée.",
        "MenuHeader" => "Titre de section dans un menu : une courte ligne de texte au-dessus d'un groupe de commandes, jamais choisie.",
        "MenuHeader.Text" => "Texte du titre (affiché en capitales).",
        "MenuHeader.Visible" => "Indique si le titre est affiché.",
        "DropDownButton" => "Bouton qui ouvre un menu sous lui (ToolStripDropDownButton de WinForms) : ses MenuItem enfants, ou le ContextMenu que nomme sa propriété DropDownMenu.",
        "SplitButton" => "Bouton avec sa propre action et une flèche qui ouvre un menu (ToolStripSplitButton de WinForms) : ses MenuItem enfants, ou le ContextMenu que nomme sa propriété DropDownMenu.",
        "DropDownButton.Text" | "SplitButton.Text" => "Texte affiché sur le bouton. Une esperluette devant une lettre en fait le raccourci clavier.",
        "DropDownButton.Variant" | "SplitButton.Variant" => "Style visuel : plein, contour, fantôme, texte seul ou danger.",
        "DropDownButton.Size" | "SplitButton.Size" => "Hauteur et marge horizontale du bouton.",
        "DropDownButton.Icon" | "SplitButton.Icon" => "Icône affichée avant le texte : un nom du jeu d'icônes Kubuno, ou un fichier image (SVG, PNG…) relatif à la vue.",
        "DropDownButton.DropDownMenu" => "ContextMenu de la vue qu'ouvre le bouton, quand il ne contient pas ses propres MenuItem.",
        "SplitButton.DropDownMenu" => "ContextMenu de la vue qu'ouvre la flèche, quand le bouton ne contient pas ses propres MenuItem.",
        "DropDownButton.ShowDropDownArrow" => "Affiche la flèche après le texte.",
        "SplitButton.DefaultItem" => "Le x:Name de la commande qu'exécute un clic sur le bouton quand il n'a pas de gestionnaire OnClick.",
        "SplitButton.OnClick" => "Se produit quand on clique sur le bouton (pas sur sa flèche) ou qu'on l'active avec Espace ou Entrée.",
        "DropDownButton.OnDropDownOpening" | "SplitButton.OnDropDownOpening" => "Se produit quand le menu va s'ouvrir : remplissez-le maintenant.",
        "DropDownButton.OnDropDownClosed" | "SplitButton.OnDropDownClosed" => "Se produit quand le menu s'est fermé.",
        "ContextMenu" => "Menu affiché quand on clique avec le bouton droit sur un contrôle qui le nomme dans sa propriété ContextMenu (ou qu'il reçoit Maj+F10 ou la touche Menu), quand un bouton le nomme dans sa propriété DropDownMenu, ou depuis le code (show). Ajoutez ses commandes comme éléments MenuItem, avec des MenuSeparator et des MenuHeader entre elles ; des MenuItem enfants d'un MenuItem forment un sous-menu.",
        "ContextMenu.OnClosed" => "Se produit quand le menu s'est fermé.",
        "ContextMenu.ItemsSource" => "Commandes ajoutées depuis une liste à l'ouverture du menu (champs Text, Key, Icon, ShortcutKeys, Checked, Enabled, Danger, ToolTip, et Kind = Separator ou Header), ou faites à partir de son ItemTemplate ; en choisir une déclenche OnItemClicked avec sa clé.",
        "MenuItem" => "Commande d'un menu. Des MenuItem enfants en font un sous-menu. Sa propriété Command exécute une Command de la vue et en prend le texte, l'icône, le raccourci et l'état ; ses ShortcutKeys l'exécutent quand le menu est fermé.",
        "MenuItem.Kind" => "Une commande, une ligne de séparation ou un titre de section (MenuSeparator et MenuHeader le disent plus court).",
        "MenuItem.ShortcutKeys" => "Raccourci clavier qui exécute la commande quand son menu est fermé, affiché à côté d'elle : des touches de modification puis une touche, comme Ctrl+S, Ctrl+Maj+N, Alt+F4 ou F5.",
        "MenuItem.ShortcutKeyDisplayString" => "Texte affiché à la place du raccourci (le raccourci lui-même fonctionne toujours).",
        "MenuItem.ShowShortcutKeys" => "Affiche le raccourci à côté de la commande.",
        "MenuItem.ToolTip" => "Texte affiché quand la souris s'arrête sur la commande.",
        "MenuItem.Command" => "La Command de la vue qu'exécute la commande : son texte, son icône, son raccourci, son état activé et coché viennent d'elle, et choisir la commande déclenche son OnExecute.",
        "MenuItem.ItemsSource" => "Commandes du sous-menu ajoutées depuis une liste à son ouverture (voir l'ItemsSource du ContextMenu), ou faites à partir de son ItemTemplate.",
        _ => return None,
    })
}
