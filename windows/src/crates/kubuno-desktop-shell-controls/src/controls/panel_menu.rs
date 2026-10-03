//! The two pieces of the account panel (the web's `UserPanel`) no primitive draws:
//!
//! * [`PanelMenu`] — a white card of clickable rows (`overflow-hidden rounded-[20px]`): a tinted
//!   circle carrying a glyph, or an account's initials, then a label and an optional second line;
//!   a row lights up under the pointer (a plain rectangle the card's corners cut) and the rows may
//!   be divided (`divide-y`, inset by their padding).
//! * [`AccentPill`] — « Gérer votre compte »: an outlined accent pill, not a `Button` (the web
//!   writes it as a link with its own `rounded-full`).
//!
//! A `PanelMenu`'s rows are its `Items` property (a binding to a `Shared<Vec<MenuRow>>`, as
//! [`crate::AccountMenu`] does), or [`PanelMenu::set_rows`] from code. Its preferred height is its rows'
//! (`AutoSize="true"` sizes it to them).

use kubuno_desktop::controls::host::access::AccessRole;
use kubuno_desktop::ui::display::Separator;
use kubuno_desktop::ui::metrics::{pill, space};
use kubuno_desktop::ui::{Canvas, Rect, Size, Widget, WidgetState};
use kubuno_desktop::views::component::{AccessiblePart, Component as _, Control, ControlCore, EventCx, HasControlCore, PaintEventCx, Shared};
use kubuno_desktop::views::events::{EmptyEventArgs, Event, MouseEventArgs};

/// A row: `px-4 py-3` around a 32 circle — 56.6 measured.
pub const ROW_H: f32 = 56.6;
const ROW_PAD_X: f32 = 16.0;
const CARD_RADIUS: f32 = 20.0;
/// A glyph row's circle, and its glyph.
const ROW_ICON: f32 = 32.0;
const ROW_GLYPH: f32 = 16.0;
/// An account row's avatar (`w-10 h-10`).
const ROW_AVATAR: f32 = 40.0;
/// `gap-3` between the circle and the text.
const ROW_GAP: f32 = 12.0;
/// `text-sm` over `text-xs`: a 20 line over a 16 one.
const LABEL_LINE: f32 = 20.0;
const SUB_LINE: f32 = 16.0;

/// One row of a [`PanelMenu`].
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MenuRow {
    /// What [`MenuItemEventArgs::id`] reports.
    pub id: String,
    /// A glyph in a tinted circle, or (empty) the initials of `label` on the accent.
    pub icon: &'static str,
    pub label: String,
    /// A second line under the label (an account's address); empty for none.
    pub sub: String,
}

/// Raised when a row is clicked.
#[derive(kubuno_desktop::views::events::EventArgs, Debug, Clone, Default, PartialEq)]
pub struct MenuItemEventArgs {
    pub id: String,
}

/// A card of clickable rows (see the module doc).
#[derive(kubuno_desktop::views::component::Component, Default)]
#[kubuno(extends = Control, overrides(Control))]
#[category("Kubuno")]
#[toolbox(icon = "list")]
#[default_event("ItemClicked")]
pub struct PanelMenu {
    base: ControlCore,
    /// Draws a rule between the rows.
    #[property]
    #[category("Appearance")]
    pub divided: bool,
    /// The rows (a binding to a `Shared<Vec<MenuRow>>`; or `set_rows` from code).
    #[property(bindable, on_change = "items_changed")]
    #[category("Data")]
    pub items: Shared<Vec<MenuRow>>,
    /// Occurs when a row is clicked.
    #[event]
    #[category("Action")]
    pub item_clicked: Event<MenuItemEventArgs>,
    rows: Vec<MenuRow>,
    hot: Option<usize>,
    pressed: Option<usize>,
}

impl PanelMenu {
    fn items_changed(&mut self) {
        let rows = (*self.items).clone();
        self.set_rows(rows);
    }

    /// Shows `rows`.
    pub fn set_rows(&mut self, rows: Vec<MenuRow>) {
        self.rows = rows;
        self.hot = None;
        self.invalidate();
    }

    /// The rows shown.
    pub fn rows(&self) -> &[MenuRow] {
        &self.rows
    }

    /// The card's height for `n` rows.
    pub fn height_for(n: usize) -> f32 {
        n as f32 * ROW_H
    }

    fn row_at(&self, y: f32) -> Option<usize> {
        let i = (y / ROW_H).floor();
        (i >= 0.0 && (i as usize) < self.rows.len()).then_some(i as usize)
    }
}

/// The first letter of `label`, upper-cased: an account row's avatar.
fn initial(label: &str) -> String {
    label.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_default()
}

impl Control for PanelMenu {
    fn get_preferred_size(&self, _canvas: &dyn Canvas, _proposed: Size) -> Size {
        Size { width: 302.8, height: Self::height_for(self.rows.len().max(1)) }
    }

    /// Every row, by its label, for a screen reader.
    fn accessible_parts(&self) -> Vec<AccessiblePart> {
        let width = self.control_core().props.size().width;
        self.rows
            .iter()
            .enumerate()
            .map(|(i, row)| AccessiblePart {
                name: if row.sub.is_empty() { row.label.clone() } else { format!("{}, {}", row.label, row.sub) },
                role: AccessRole::ListItem,
                bounds: Rect::new(0.0, i as f32 * ROW_H, width, (i + 1) as f32 * ROW_H),
            })
            .collect()
    }

    fn on_paint(&mut self, e: &mut PaintEventCx<'_>) {
        let r = e.clip_rectangle;
        // The designer shows sample rows: actions for a divided menu, an account for the others.
        if self.rows.is_empty() && self.design_mode() {
            self.rows = if self.divided {
                vec![
                    MenuRow { id: "add".into(), icon: "UserPlus", label: "Ajouter un compte".into(), sub: String::new() },
                    MenuRow { id: "labels".into(), icon: "Tags", label: "Étiquettes".into(), sub: String::new() },
                    MenuRow { id: "admin".into(), icon: "Shield", label: "Administration".into(), sub: String::new() },
                    MenuRow { id: "logout".into(), icon: "LogOut", label: "Se déconnecter".into(), sub: String::new() },
                ]
            } else {
                crate::controls::account_menu::account_rows(&crate::controls::account_menu::design_data().1)
            };
        }
        let c: &dyn Canvas = e.graphics;
        let t = c.theme().clone();
        let f = c.formats();
        // `bg-white`: the opaque card over the translucent panel — the application theme's surface,
        // not the ambient colour of the panel (its tint, which a free `BackColor` hands its children).
        c.fill_rounded(&r, CARD_RADIUS, &kubuno_desktop::Application::theme().layer_background);
        // `overflow-hidden`: a row's hover is a plain rectangle the card's corners cut.
        c.push_clip_rounded(&r, CARD_RADIUS);
        for (i, row) in self.rows.iter().enumerate() {
            let band = Rect::new(r.left, r.top + i as f32 * ROW_H, r.right, r.top + (i + 1) as f32 * ROW_H);
            if self.hot == Some(i) {
                // The row under the pointer: the text colour at 5 %, readable in both themes.
                let mut hover = t.text_primary;
                hover.a = 0.05;
                c.fill_rounded(&band, 0.0, &hover);
            }
            let cy = (band.top + band.bottom) / 2.0;
            let text_left = if row.icon.is_empty() {
                let circle = Rect::new(band.left + ROW_PAD_X, cy - ROW_AVATAR / 2.0, band.left + ROW_PAD_X + ROW_AVATAR, cy + ROW_AVATAR / 2.0);
                c.fill_rounded(&circle, pill(ROW_AVATAR), &t.accent);
                c.text(&initial(&row.label), &circle, &f.heading_strong, &t.accent_foreground, true);
                circle.right + ROW_GAP
            } else {
                let circle = Rect::new(band.left + ROW_PAD_X, cy - ROW_ICON / 2.0, band.left + ROW_PAD_X + ROW_ICON, cy + ROW_ICON / 2.0);
                c.fill_rounded(&circle, pill(ROW_ICON), &t.surface_2);
                c.vector_icon(row.icon, &circle, ROW_GLYPH, &t.text_secondary);
                circle.right + ROW_GAP
            };
            if row.sub.is_empty() {
                c.text(&row.label, &Rect::new(text_left, band.top, band.right - ROW_PAD_X, band.bottom), &f.body, &t.text_primary, false);
            } else {
                // The two lines are centred as a block, so the gap between them stays the web's.
                let top = cy - (LABEL_LINE + SUB_LINE) / 2.0;
                c.text_ellipsis(&row.label, &Rect::new(text_left, top, band.right - ROW_PAD_X, top + LABEL_LINE), &f.body, &t.text_primary);
                c.text_ellipsis(&row.sub, &Rect::new(text_left, top + LABEL_LINE, band.right - ROW_PAD_X, top + LABEL_LINE + SUB_LINE), &f.caption, &t.text_tertiary);
            }
            if self.divided && i > 0 {
                Separator::horizontal().paint(c, Rect::new(band.left + ROW_PAD_X, band.top, band.right - ROW_PAD_X, band.top + Separator::THICKNESS), WidgetState::REST);
            }
        }
        c.pop_clip_rounded();
        e.raise(self, "OnPaint");
    }

    fn on_mouse_move(&mut self, e: &mut EventCx<'_, MouseEventArgs>) {
        let hot = self.row_at(e.args().y);
        if hot != self.hot {
            self.hot = hot;
            self.invalidate();
        }
        e.raise(&*self, "OnMouseMove");
    }

    fn on_mouse_down(&mut self, e: &mut EventCx<'_, MouseEventArgs>) {
        self.pressed = self.row_at(e.args().y);
        e.raise(&*self, "OnMouseDown");
    }

    fn on_mouse_up(&mut self, e: &mut EventCx<'_, MouseEventArgs>) {
        let at = self.row_at(e.args().y);
        if let (Some(i), Some(p)) = (at, self.pressed.take()) {
            if i == p {
                let id = self.rows[i].id.clone();
                self.raise_item_clicked(MenuItemEventArgs { id });
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

/// « Gérer votre compte » (see the module doc). `Click` opens the account's settings.
#[derive(kubuno_desktop::views::component::Component, Default)]
#[kubuno(extends = Control, overrides(Control))]
#[category("Kubuno")]
#[toolbox(icon = "pill")]
#[default_event("Click")]
pub struct AccentPill {
    base: ControlCore,
    /// What it says.
    #[property(bindable)]
    #[category("Appearance")]
    pub text: String,
}

impl Control for AccentPill {
    fn get_preferred_size(&self, _canvas: &dyn Canvas, _proposed: Size) -> Size {
        // `px-5 py-1.5` around a 20 line, over a 1 border: 33.1 measured, 164.6 wide in French.
        Size { width: 164.6, height: 33.1 }
    }

    fn on_paint(&mut self, e: &mut PaintEventCx<'_>) {
        let r = e.clip_rectangle;
        let c: &dyn Canvas = e.graphics;
        let t = c.theme().clone();
        let h = r.bottom - r.top;
        if self.control_core().hot {
            c.fill_rounded(&r, pill(h), &t.accent_light);
        }
        c.stroke_rounded(&r, pill(h), &t.accent);
        c.text(&self.text, &Rect::new(r.left + space::LG, r.top, r.right - space::LG, r.bottom), &c.formats().body, &t.accent, true);
        e.raise(self, "OnPaint");
    }

    fn on_mouse_enter(&mut self, e: &mut EventCx<'_, EmptyEventArgs>) {
        self.invalidate();
        e.raise(&*self, "OnMouseEnter");
    }

    fn on_mouse_leave(&mut self, e: &mut EventCx<'_, EmptyEventArgs>) {
        self.invalidate();
        e.raise(&*self, "OnMouseLeave");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_row_is_found_by_its_band() {
        let mut m = PanelMenu::default();
        m.set_rows(vec![MenuRow { id: "a".into(), ..MenuRow::default() }, MenuRow { id: "b".into(), ..MenuRow::default() }]);
        assert_eq!(m.row_at(1.0), Some(0));
        assert_eq!(m.row_at(ROW_H + 1.0), Some(1));
        assert_eq!(m.row_at(ROW_H * 2.0 + 1.0), None);
        assert_eq!(m.row_at(-1.0), None);
        assert_eq!(PanelMenu::height_for(4), ROW_H * 4.0);
        assert_eq!(initial("association"), "A");
    }
}
