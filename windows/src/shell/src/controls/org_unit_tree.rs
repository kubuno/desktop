//! `OrgUnitTree` — the organisational units as an indented tree (the web's `OrgUnitsPanel`): root
//! first, each unit's children under it sorted by name, every unit shown (as on the web). A row: a
//! building glyph indented by its depth, the name over the description, the accounts of the unit
//! and its descendants, then « + » (add a child unit) and the pencil (edit), both in the web console.
//!
//! No primitive holds it: `TreeView` paints one line per node with no per-node hook, and a
//! `Repeater` item cannot move its controls by depth. What is drawn is the design system's
//! (`IconButton`s, the `Separator`, the theme's row hover).

use kubuno::controls::host::access::AccessRole;
use kubuno::ui::buttons::IconButton;
use kubuno::ui::display::Separator;
use kubuno::ui::metrics::{radius, space};
use kubuno::ui::{Canvas, Rect, Size, Widget, WidgetState};
use kubuno::views::component::{AccessiblePart, Component as _, Control, ControlCore, EventCx, PaintEventCx, Shared};
use kubuno::views::events::{EmptyEventArgs, Event, MouseEventArgs};

use crate::Resources;

/// A row's height.
pub const ROW_H: f32 = 60.0;
/// The indentation of one level.
const INDENT: f32 = 28.0;
const ICON: f32 = 20.0;
/// The count and the two buttons, at the end of a row.
const RIGHT_W: f32 = 190.0;
const BTN: f32 = 28.0;
const BTN_GLYPH: f32 = 15.0;
/// How deep the tree is followed (a loop in the data stops there).
const MAX_DEPTH: u8 = 8;

/// One unit.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Unit {
    pub id: String,
    pub name: String,
    pub description: String,
    pub parent: Option<String>,
    /// Its own accounts (its descendants' are added when shown).
    pub accounts: i64,
}

/// The units in display order, `(index, depth)`: depth-first, root first, children by name.
pub fn order(units: &[Unit]) -> Vec<(usize, u8)> {
    fn walk(units: &[Unit], parent: Option<&str>, depth: u8, out: &mut Vec<(usize, u8)>) {
        let mut children: Vec<usize> = units.iter().enumerate().filter(|(_, u)| u.parent.as_deref() == parent).map(|(i, _)| i).collect();
        children.sort_by(|&a, &b| units[a].name.to_lowercase().cmp(&units[b].name.to_lowercase()));
        for i in children {
            out.push((i, depth));
            if depth < MAX_DEPTH {
                walk(units, Some(&units[i].id), depth + 1, out);
            }
        }
    }
    let mut out = Vec::new();
    walk(units, None, 0, &mut out);
    out
}

/// The accounts of unit `id` and its descendants.
pub fn subtree_accounts(units: &[Unit], id: &str) -> i64 {
    fn total(units: &[Unit], id: &str, depth: u8) -> i64 {
        if depth > MAX_DEPTH {
            return 0;
        }
        let own = units.iter().find(|u| u.id == id).map_or(0, |u| u.accounts);
        own + units.iter().filter(|u| u.parent.as_deref() == Some(id)).map(|u| total(units, &u.id, depth + 1)).sum::<i64>()
    }
    total(units, id, 0)
}

/// « 1 compte », « 101 comptes ».
pub fn accounts_text(n: i64) -> String {
    if n == 1 { Resources::units_account() } else { Resources::units_accounts() }.replace("{0}", &n.to_string())
}

/// What the pointer is on, in a row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Part {
    Row(usize),
    Add(usize),
    Edit(usize),
}

fn add_rect(row: Rect) -> Rect {
    let cy = (row.top + row.bottom) / 2.0;
    let right = row.right - space::SM - BTN - space::XS;
    Rect::new(right - BTN, cy - BTN / 2.0, right, cy + BTN / 2.0)
}

fn edit_rect(row: Rect) -> Rect {
    let cy = (row.top + row.bottom) / 2.0;
    Rect::new(row.right - space::SM - BTN, cy - BTN / 2.0, row.right - space::SM, cy + BTN / 2.0)
}

/// Raised by « + » and the pencil: the unit.
#[derive(kubuno::views::events::EventArgs, Debug, Clone, Default, PartialEq)]
pub struct UnitEventArgs {
    pub id: String,
}

/// The organisational units (see the module doc).
#[derive(kubuno::views::component::Component, Default)]
#[kubuno(extends = Control, overrides(Control))]
#[category("Kubuno")]
#[toolbox(icon = "building-2")]
#[default_event("EditRequested")]
pub struct OrgUnitTree {
    base: ControlCore,
    /// The units (a binding to a `Shared<Vec<Unit>>`; or set from code).
    #[property(bindable, on_change = "units_changed")]
    #[category("Data")]
    pub units_source: Shared<Vec<Unit>>,
    /// Occurs when a unit's pencil is clicked.
    #[event]
    #[category("Action")]
    pub edit_requested: Event<UnitEventArgs>,
    /// Occurs when a unit's « + » is clicked (a child unit).
    #[event]
    #[category("Action")]
    pub add_child_requested: Event<UnitEventArgs>,
    units: Vec<Unit>,
    hot: Option<Part>,
    pressed: Option<Part>,
    width: f32,
}

impl OrgUnitTree {
    fn units_changed(&mut self) {
        let units = (*self.units_source).clone();
        self.set_units(units);
    }

    /// Shows `units`.
    pub fn set_units(&mut self, units: Vec<Unit>) {
        self.units = units;
        self.hot = None;
        self.invalidate();
    }

    /// The height the tree needs.
    pub fn content_height(&self) -> f32 {
        self.units.len() as f32 * ROW_H
    }

    fn row(&self, i: usize, width: f32) -> Rect {
        Rect::new(0.0, i as f32 * ROW_H, width, (i + 1) as f32 * ROW_H)
    }

    fn part_at(&self, x: f32, y: f32) -> Option<Part> {
        let i = (y / ROW_H).floor();
        if i < 0.0 || i as usize >= self.units.len() {
            return None;
        }
        let i = i as usize;
        let r = self.row(i, self.width);
        if edit_rect(r).contains(x, y) {
            Some(Part::Edit(i))
        } else if add_rect(r).contains(x, y) {
            Some(Part::Add(i))
        } else {
            Some(Part::Row(i))
        }
    }

    fn unit_at(&self, row: usize) -> Option<&Unit> {
        order(&self.units).get(row).and_then(|&(i, _)| self.units.get(i))
    }
}

impl Control for OrgUnitTree {
    fn get_preferred_size(&self, _canvas: &dyn Canvas, _proposed: Size) -> Size {
        Size { width: 936.0, height: self.content_height().max(ROW_H) }
    }

    fn accessible_parts(&self) -> Vec<AccessiblePart> {
        order(&self.units)
            .iter()
            .enumerate()
            .map(|(row, &(i, _))| {
                let u = &self.units[i];
                AccessiblePart { name: format!("{}, {}", u.name, accounts_text(subtree_accounts(&self.units, &u.id))), role: AccessRole::TreeItem, bounds: self.row(row, self.width) }
            })
            .collect()
    }

    fn on_paint(&mut self, e: &mut PaintEventCx<'_>) {
        let b = e.clip_rectangle;
        self.width = b.right - b.left;
        if self.units.is_empty() && self.design_mode() {
            self.units = design_units();
        }
        let c: &dyn Canvas = e.graphics;
        let t = c.theme().clone();
        let f = c.formats();
        for (row, &(i, depth)) in order(&self.units).iter().enumerate() {
            let u = &self.units[i];
            let local = self.row(row, self.width);
            let r = Rect::new(b.left + local.left, b.top + local.top, b.left + local.right, b.top + local.bottom);
            let hot = |p: Part| self.hot == Some(p);
            if matches!(self.hot, Some(Part::Row(j) | Part::Add(j) | Part::Edit(j)) if j == row) {
                c.fill_rounded(&r, radius::LG, &t.row_hover);
            }
            let cy = (r.top + r.bottom) / 2.0;
            let ix = r.left + space::MD + f32::from(depth) * INDENT;
            c.vector_icon("Building2", &Rect::new(ix, cy - ICON / 2.0, ix + ICON, cy + ICON / 2.0), ICON, &t.text_tertiary);
            let text_left = ix + ICON + space::SM;
            let text_right = r.right - RIGHT_W;
            c.text_ellipsis(&u.name, &Rect::new(text_left, r.top + 12.0, text_right, r.top + 34.0), &f.body_strong, &t.text_primary);
            if !u.description.trim().is_empty() {
                c.text_ellipsis(&u.description, &Rect::new(text_left, r.top + 34.0, text_right, r.bottom - 8.0), &f.caption, &t.text_tertiary);
            }
            let add = add_rect(r);
            c.text(&accounts_text(subtree_accounts(&self.units, &u.id)), &Rect::new(text_right, cy - 10.0, add.left - space::SM, cy + 10.0), &f.caption, &t.text_tertiary, true);
            IconButton::plain("Plus", BTN, BTN_GLYPH).paint(c, add, WidgetState::REST.hot(hot(Part::Add(row))));
            IconButton::plain("PenLine", BTN, BTN_GLYPH).paint(c, edit_rect(r), WidgetState::REST.hot(hot(Part::Edit(row))));
            Separator::horizontal().paint(c, Rect::new(r.left, r.bottom - Separator::THICKNESS, r.right, r.bottom), WidgetState::REST);
        }
        e.raise(self, "OnPaint");
    }

    fn on_mouse_move(&mut self, e: &mut EventCx<'_, MouseEventArgs>) {
        let hot = self.part_at(e.args().x, e.args().y);
        if hot != self.hot {
            self.hot = hot;
            self.invalidate();
        }
        e.raise(&*self, "OnMouseMove");
    }

    fn on_mouse_down(&mut self, e: &mut EventCx<'_, MouseEventArgs>) {
        self.pressed = self.part_at(e.args().x, e.args().y);
        e.raise(&*self, "OnMouseDown");
    }

    fn on_mouse_up(&mut self, e: &mut EventCx<'_, MouseEventArgs>) {
        let at = self.part_at(e.args().x, e.args().y);
        if at.is_some() && at == self.pressed.take() {
            match at {
                Some(Part::Edit(row)) => {
                    if let Some(id) = self.unit_at(row).map(|u| u.id.clone()) {
                        self.raise_edit_requested(UnitEventArgs { id });
                    }
                }
                Some(Part::Add(row)) => {
                    if let Some(id) = self.unit_at(row).map(|u| u.id.clone()) {
                        self.raise_add_child_requested(UnitEventArgs { id });
                    }
                }
                _ => {}
            }
        }
        e.raise(&*self, "OnMouseUp");
    }

    fn on_mouse_leave(&mut self, e: &mut EventCx<'_, EmptyEventArgs>) {
        if self.hot.take().is_some() {
            self.invalidate();
        }
        e.raise(&*self, "OnMouseLeave");
    }
}

/// What the designer shows: the sample instance's units.
pub fn design_units() -> Vec<Unit> {
    let u = |id: &str, name: &str, description: &str, parent: Option<&str>, accounts: i64| Unit {
        id: id.into(),
        name: name.into(),
        description: description.into(),
        parent: parent.map(str::to_string),
        accounts,
    };
    vec![
        u("com", "Commercial", "", None, 101),
        u("dir", "Direction", "Comité de direction", None, 8),
        u("rh", "Ressources humaines", "", Some("dir"), 14),
        u("it", "Informatique", "Systèmes et réseaux", None, 42),
        u("sup", "Support", "", Some("it"), 61),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tree_is_depth_first_with_children_by_name() {
        let units = design_units();
        let names: Vec<(&str, u8)> = order(&units).iter().map(|&(i, d)| (units[i].name.as_str(), d)).collect();
        assert_eq!(names, [("Commercial", 0), ("Direction", 0), ("Ressources humaines", 1), ("Informatique", 0), ("Support", 1)]);
    }

    #[test]
    fn a_unit_counts_its_descendants_accounts() {
        let units = design_units();
        assert_eq!(subtree_accounts(&units, "dir"), 22);
        assert_eq!(subtree_accounts(&units, "it"), 103);
        assert_eq!(subtree_accounts(&units, "sup"), 61);
    }

    #[test]
    fn the_pointer_finds_the_buttons_of_a_row() {
        let mut t = OrgUnitTree { width: 936.0, ..OrgUnitTree::default() };
        t.set_units(design_units());
        let r = t.row(1, 936.0);
        let e = edit_rect(r);
        assert_eq!(t.part_at((e.left + e.right) / 2.0, (e.top + e.bottom) / 2.0), Some(Part::Edit(1)));
        let a = add_rect(r);
        assert_eq!(t.part_at((a.left + a.right) / 2.0, (a.top + a.bottom) / 2.0), Some(Part::Add(1)));
        assert_eq!(t.part_at(10.0, ROW_H * 1.5), Some(Part::Row(1)));
        assert_eq!(t.unit_at(2).map(|u| u.id.as_str()), Some("rh"));
    }
}
