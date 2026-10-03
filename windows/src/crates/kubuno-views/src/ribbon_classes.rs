//! The classes of the ribbon family (`vskubuno/docs/RIBBON.md` §3): `<Ribbon>` (a `Control`)
//! and every element composing it, in the `RibbonControl` → `RibbonItem` levels. The ribbon's node
//! paints them (`crate::registry::families::ribbon`); each is still an object of its own — the
//! `Sender` of its events, a named component of its view, and a base to derive from:
//! `#[derive(Component)] #[kubuno(extends = RibbonButton)] struct MyButton { base: RibbonButton }`.

use crate::component::{Component, ComponentCore, ControlCore, RibbonControlCore, RibbonItemCore};

/// `<Ribbon>`: the Office ribbon.
#[derive(Component, Default)]
#[kubuno(extends = Control)]
pub struct Ribbon {
    base: ControlCore,
}

macro_rules! level_classes {
    ($($(#[$doc:meta])* $name:ident: $level:ident($core:ty);)*) => {$(
        $(#[$doc])*
        #[derive(Component, Default)]
        #[kubuno(extends = $level)]
        pub struct $name {
            base: $core,
        }
    )*};
}

level_classes! {
    /// `<RibbonTab>`: a tab of groups.
    RibbonTab: RibbonControl(RibbonControlCore);
    /// `<RibbonContextualTabGroup>`: tabs shown in a context, under a coloured header.
    RibbonContextualTabGroup: RibbonControl(RibbonControlCore);
    /// `<RibbonGroup>`: a group of commands of a tab.
    RibbonGroup: RibbonControl(RibbonControlCore);
    /// `<RibbonControlGroup>`: buttons joined in one row.
    RibbonControlGroup: RibbonControl(RibbonControlCore);
    /// `<RibbonBox>`: a row or a column of commands.
    RibbonBox: RibbonControl(RibbonControlCore);
    /// `<RibbonQuickAccessToolbar>`.
    RibbonQuickAccessToolbar: RibbonControl(RibbonControlCore);
    /// `<RibbonBackstage>`: the « Fichier » tab and its view.
    RibbonBackstage: RibbonControl(RibbonControlCore);
    /// `<BackstageTab>`: a tab of the Backstage showing a view.
    BackstageTab: RibbonControl(RibbonControlCore);
    /// `<BackstageButton>`: a command of the Backstage.
    BackstageButton: RibbonControl(RibbonControlCore);
    /// `<BackstageSeparator>`.
    BackstageSeparator: RibbonControl(RibbonControlCore);
    /// `<RibbonButton>`: a command button.
    RibbonButton: RibbonItem(RibbonItemCore);
    /// `<RibbonCheckBox>`.
    RibbonCheckBox: RibbonItem(RibbonItemCore);
    /// `<RibbonComboBox>`.
    RibbonComboBox: RibbonItem(RibbonItemCore);
    /// `<RibbonTextBox>`.
    RibbonTextBox: RibbonItem(RibbonItemCore);
    /// `<RibbonNumericField>`.
    RibbonNumericField: RibbonItem(RibbonItemCore);
    /// `<RibbonGallery>`.
    RibbonGallery: RibbonItem(RibbonItemCore);
    /// `<RibbonGalleryCategory>`.
    RibbonGalleryCategory: RibbonItem(RibbonItemCore);
    /// `<RibbonGalleryItem>`.
    RibbonGalleryItem: RibbonItem(RibbonItemCore);
    /// `<RibbonLabel>`.
    RibbonLabel: RibbonItem(RibbonItemCore);
    /// `<RibbonSeparator>`.
    RibbonSeparator: RibbonItem(RibbonItemCore);
    /// `<RibbonToggleButton>`: a button that stays pressed.
    RibbonToggleButton: RibbonButton(RibbonButton);
    /// `<RibbonRadioButton>`: a toggle of a set.
    RibbonRadioButton: RibbonButton(RibbonButton);
    /// `<RibbonMenuButton>`: a button opening a menu.
    RibbonMenuButton: RibbonButton(RibbonButton);
    /// `<RibbonMenuItem>`: an entry of a menu.
    RibbonMenuItem: RibbonButton(RibbonButton);
    /// `<RibbonSplitButton>`: an action and a menu.
    RibbonSplitButton: RibbonMenuButton(RibbonMenuButton);
    /// `<RibbonSplitMenuItem>`: a menu entry with a sub-menu.
    RibbonSplitMenuItem: RibbonMenuItem(RibbonMenuItem);
    /// `<RibbonColorPicker>`: a split button whose menu is a colour palette.
    RibbonColorPicker: RibbonSplitButton(RibbonSplitButton);
}

/// `<Command>`: a command of the view (component tray).
#[derive(Component, Default)]
#[kubuno(extends = Component)]
pub struct Command {
    base: ComponentCore,
}

#[allow(dead_code)]
fn _uses(_: &dyn Component) {}
