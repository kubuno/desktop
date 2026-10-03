//! `09-toolstrip` — MenuStrip, ToolStrip, StatusStrip.

use kubuno_controls::toolstrip::{
    MenuStrip, Shortcut, StatusStrip, StripItem, ToolStrip, ToolStripButton, ToolStripComboBox,
    ToolStripGripStyle, ToolStripLabel, ToolStripMenuItem, ToolStripProgressBar,
    ToolStripSeparator, ToolStripStatusLabel,
};
use kubuno_controls::{ContentAlignment, Control, DockStyle};

use crate::sheet::{group, kid, Group, Sheet};

pub fn build() -> Sheet {
    Sheet::new(vec![menu(), tools(), status()])
}

/// A strip docks to the top by default; the reference frees it (`Dock = None`)
/// so it can be laid inside a group at a designed width.
fn undocked(strip: &mut ToolStrip) {
    strip.control_mut().dock = DockStyle::None;
}

fn menu() -> Group {
    let mut m = MenuStrip::new();
    undocked(&mut m.base);

    let mut file = ToolStripMenuItem::new("Fichier");
    file.base.drop_down_items.push(StripItem::MenuItem(ToolStripMenuItem::new("Nouveau")));
    let mut open = ToolStripMenuItem::new("Ouvrir");
    open.shortcut_keys = Some(Shortcut::new(true, false, false, "O"));
    file.base.drop_down_items.push(StripItem::MenuItem(open));
    file.base.drop_down_items.push(StripItem::Separator(ToolStripSeparator::default()));
    file.base.drop_down_items.push(StripItem::MenuItem(ToolStripMenuItem::new("Quitter")));

    m.items.push(StripItem::MenuItem(file));
    m.items.push(StripItem::MenuItem(ToolStripMenuItem::new("Édition")));
    m.items.push(StripItem::MenuItem(ToolStripMenuItem::new("Aide")));

    group("MenuStrip", 460.0, vec![kid(m).w(420.0)])
}

fn tools() -> Group {
    let mut t = ToolStrip::new();
    undocked(&mut t);
    t.grip_style = ToolStripGripStyle::Visible;

    t.items.push(StripItem::Button(ToolStripButton::new("Enregistrer")));
    t.items.push(StripItem::Separator(ToolStripSeparator::default()));
    t.items.push(StripItem::Label(ToolStripLabel::new("Étiquette")));
    t.items.push(StripItem::ComboBox(ToolStripComboBox {
        items: vec!["100 %".to_string(), "125 %".to_string(), "150 %".to_string()],
        selected_index: 0,
        ..Default::default()
    }));

    let mut toggled = ToolStripButton::new("Activé");
    toggled.checked = true;
    toggled.check_on_click = true;
    t.items.push(StripItem::Button(toggled));

    group("ToolStrip — button / separator / combo", 460.0, vec![kid(t).w(420.0)])
}

fn status() -> Group {
    let mut s = StatusStrip::new();
    undocked(&mut s.base);
    s.sizing_grip = true;

    // `Spring` makes the first label absorb the free space, pushing the rest to
    // the trailing edge.
    let mut ready = ToolStripStatusLabel::new("Prêt");
    ready.spring = true;
    ready.text_align = ContentAlignment::MiddleLeft;
    s.items.push(StripItem::StatusLabel(ready));
    s.items.push(StripItem::StatusLabel(ToolStripStatusLabel::new("26 comptes")));

    let mut bar = ToolStripProgressBar::default();
    bar.set_value(40);
    s.items.push(StripItem::ProgressBar(bar));

    group("StatusStrip", 460.0, vec![kid(s).w(420.0)])
}
