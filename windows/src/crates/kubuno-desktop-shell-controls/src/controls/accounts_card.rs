//! `AccountsCard` — the account panel's card of the OTHER accounts, as the web's `AccountMenu` draws it:
//! a toggle row (« Masquer plus de comptes » / « Afficher plus de comptes », the first accounts' letters
//! while folded, a chevron), then, unfolded, one row per account:
//!
//! - a live session: its avatar (40, initials on `surface-3`), its name over its address, its unread
//!   counter; one click raises `OpenAccount`;
//! - a dead session: the same, a « Déconnecté » badge, then « Connexion » (`OpenAccount`) and
//!   « Supprimer » (`RemoveAccount`);
//! - an account of another instance: its server pill, then « Ouvrir » (`OpenAccount`) and
//!   « Supprimer » (`RemoveAccount`).
//!
//! Every metric is the web's (`mx-2 rounded-[20px]`, `px-4 py-3`, `gap-3`, `pl-[52px]`, `mt-2.5`); the
//! card's height follows its rows (`AutoSize`), [`card_height`] says it for the host.

use kubuno_desktop::controls::host::access::AccessRole;
use kubuno_desktop::ui::buttons::{Button, Size as ButtonSize, Variant};
use kubuno_desktop::ui::display::Separator;
use kubuno_desktop::ui::metrics::pill;
use kubuno_desktop::ui::{Canvas, Rect, Size, Widget, WidgetState};
use kubuno_desktop::views::component::{AccessiblePart, Component as _, Control, ControlCore, EventCx, PaintEventCx, Shared};
use kubuno_desktop::views::events::{EmptyEventArgs, Event, MouseEventArgs};

use crate::controls::account_menu::AccountEventArgs;
use crate::model::account::AccountEntry;
use crate::ShellControlsResources;

const CARD_RADIUS: f32 = 20.0;
const PAD_X: f32 = 16.0;
const PAD_Y: f32 = 12.0;
/// The toggle row: `py-3` around a 20 line.
pub const TOGGLE_H: f32 = 44.0;
const AVATAR: f32 = 40.0;
const GAP: f32 = 12.0;
/// A live session's row: `py-3` around the 40 avatar.
pub const ROW_H: f32 = 64.0;
/// The buttons under a dead session or a remote account: `mt-2.5`, a small button.
const BUTTONS_TOP: f32 = 10.0;
const BUTTON_H: f32 = 32.0;
/// A row with buttons.
pub const ACTION_ROW_H: f32 = PAD_Y + AVATAR + BUTTONS_TOP + BUTTON_H + PAD_Y;
/// `pl-[52px]`: the buttons line up with the name.
const BUTTONS_LEFT: f32 = AVATAR + GAP;
const MINI: f32 = 24.0;
const CHEVRON: f32 = 16.0;

/// A row's height.
pub fn row_height(a: &AccountEntry) -> f32 {
    if a.connected && !a.remote { ROW_H } else { ACTION_ROW_H }
}

/// The card's height for `accounts`, unfolded or not (none without accounts).
pub fn card_height(accounts: &[AccountEntry], expanded: bool) -> f32 {
    if accounts.is_empty() {
        return 0.0;
    }
    TOGGLE_H + if expanded { accounts.iter().map(row_height).sum::<f32>() } else { 0.0 }
}

/// What the pointer is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Part {
    Toggle,
    Row(usize),
    Primary(usize),
    Remove(usize),
}

/// Raised when the card folds or unfolds.
#[derive(kubuno_desktop::views::events::EventArgs, Debug, Clone, Default, PartialEq)]
pub struct ExpandedEventArgs {
    pub expanded: bool,
}

/// The card of the other accounts (see the module doc).
#[derive(kubuno_desktop::views::component::Component)]
#[kubuno(extends = Control, overrides(Control))]
#[category("Kubuno")]
#[toolbox(icon = "users")]
#[default_event("OpenAccount")]
pub struct AccountsCard {
    base: ControlCore,
    /// The other accounts (a binding to a `Shared<Vec<AccountEntry>>`).
    #[property(bindable, on_change = "accounts_changed")]
    #[category("Data")]
    pub accounts: Shared<Vec<AccountEntry>>,
    /// Unfolded (the web's default).
    #[property(bindable, on_change = "accounts_changed")]
    #[default_value(true)]
    #[category("Behavior")]
    pub expanded: bool,
    /// A switch is under way: the rows take no click.
    #[property(bindable)]
    #[category("Behavior")]
    pub busy: bool,
    /// Occurs when a live account's row, « Connexion » or « Ouvrir » is clicked.
    #[event]
    #[category("Action")]
    pub open_account: Event<AccountEventArgs>,
    /// Occurs when « Supprimer » is clicked.
    #[event]
    #[category("Action")]
    pub remove_account: Event<AccountEventArgs>,
    /// Occurs when the toggle row folds or unfolds the card.
    #[event]
    #[category("Action")]
    pub expanded_changed: Event<ExpandedEventArgs>,
    hot: Option<Part>,
    pressed: Option<Part>,
    width: f32,
    /// The buttons of each row as last painted (measured in the real font), local coordinates.
    button_rects: Vec<Option<(Rect, Rect)>>,
}

impl Default for AccountsCard {
    fn default() -> Self {
        Self {
            base: ControlCore::default(),
            accounts: Shared::default(),
            expanded: true,
            busy: false,
            open_account: Event::default(),
            remove_account: Event::default(),
            expanded_changed: Event::default(),
            hot: None,
            pressed: None,
            width: 302.8,
            button_rects: Vec::new(),
        }
    }
}

/// The first letter of `name`, upper-cased (the folded card's mini avatars).
fn letter(name: &str) -> String {
    name.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_default()
}

/// The initials of `name` (two at most), as the web's `initialsOf`.
fn initials_of(name: &str) -> String {
    name.split_whitespace().filter_map(|w| w.chars().find(|c| c.is_alphanumeric())).take(2).collect::<String>().to_uppercase()
}

fn primary_label(a: &AccountEntry) -> &'static str {
    if a.remote { ShellControlsResources::account_open() } else { ShellControlsResources::account_reconnect() }
}

impl AccountsCard {
    fn accounts_changed(&mut self) {
        self.hot = None;
        self.invalidate();
    }

    /// Its height now.
    pub fn height(&self) -> f32 {
        card_height(&self.accounts, self.expanded)
    }

    /// The rows' tops, under the toggle row.
    fn row_tops(&self) -> Vec<f32> {
        let mut y = TOGGLE_H;
        self.accounts
            .iter()
            .map(|a| {
                let top = y;
                y += row_height(a);
                top
            })
            .collect()
    }

    /// The two buttons of a row with buttons (local coordinates of a row at `top`), measured on `c`
    /// when given (else at the web's sizes).
    fn buttons(&self, c: Option<&dyn Canvas>, a: &AccountEntry, top: f32) -> (Rect, Rect) {
        let y = top + PAD_Y + AVATAR + BUTTONS_TOP;
        let w = |label: &str, icon: bool| match c {
            Some(c) => {
                let mut b = Button::new(label).variant(Variant::Primary).size(ButtonSize::Sm);
                if icon {
                    b = b.icon("ExternalLink");
                }
                b.measure(c).width
            }
            None => 90.0,
        };
        let left = PAD_X + BUTTONS_LEFT;
        let pw = w(primary_label(a), a.remote);
        let rw = w(ShellControlsResources::account_remove(), false);
        let primary = Rect::new(left, y, left + pw, y + BUTTON_H);
        let remove = Rect::new(primary.right + 8.0, y, primary.right + 8.0 + rw, y + BUTTON_H);
        (primary, remove)
    }

    fn part_at(&self, x: f32, y: f32) -> Option<Part> {
        if self.accounts.is_empty() {
            return None;
        }
        if y < TOGGLE_H {
            return Some(Part::Toggle);
        }
        if !self.expanded {
            return None;
        }
        for (i, (a, top)) in self.accounts.iter().zip(self.row_tops()).enumerate() {
            if y >= top && y < top + row_height(a) {
                if a.connected && !a.remote {
                    return Some(Part::Row(i));
                }
                let (primary, remove) = self.button_rects.get(i).copied().flatten().unwrap_or_else(|| self.buttons(None, a, top));
                if primary.contains(x, y) {
                    return Some(Part::Primary(i));
                }
                if remove.contains(x, y) {
                    return Some(Part::Remove(i));
                }
                return None;
            }
        }
        None
    }

    fn activate(&mut self, part: Part) {
        match part {
            Part::Toggle => {
                self.expanded = !self.expanded;
                let expanded = self.expanded;
                self.raise_expanded_changed(ExpandedEventArgs { expanded });
            }
            Part::Row(i) | Part::Primary(i) if !self.busy => {
                let id = self.accounts[i].id.clone();
                self.raise_open_account(AccountEventArgs { id });
            }
            Part::Remove(i) => {
                let id = self.accounts[i].id.clone();
                self.raise_remove_account(AccountEventArgs { id });
            }
            _ => {}
        }
        self.invalidate();
    }

    fn paint_avatar(c: &dyn Canvas, r: Rect, a: &AccountEntry) {
        let t = c.theme();
        c.fill_rounded(&r, pill(r.right - r.left), &t.surface_3);
        let text = a.initials.clone().unwrap_or_else(|| initials_of(&a.name));
        c.text(&text, &r, &c.formats().body_strong, &t.text_secondary, true);
    }

    fn paint_pill(c: &dyn Canvas, right: f32, cy: f32, text: &str, round: bool) -> f32 {
        let t = c.theme();
        let f = &c.formats().micro;
        let w = c.measure(text, f) + 16.0;
        let r = Rect::new(right - w, cy - 9.5, right, cy + 9.5);
        c.fill_rounded(&r, if round { pill(19.0) } else { 6.0 }, &t.surface_2);
        if round {
            c.stroke_rounded(&r, pill(19.0), &t.card_stroke);
        }
        c.text(text, &r, f, if round { &t.text_tertiary } else { &t.text_secondary }, true);
        r.left
    }
}

impl Control for AccountsCard {
    fn get_preferred_size(&self, _canvas: &dyn Canvas, _proposed: Size) -> Size {
        Size { width: 302.8, height: self.height().max(1.0) }
    }

    fn accessible_parts(&self) -> Vec<AccessiblePart> {
        let mut parts = vec![AccessiblePart {
            name: if self.expanded { ShellControlsResources::account_hide_more() } else { ShellControlsResources::account_show_more() }.to_string(),
            role: AccessRole::Button,
            bounds: Rect::new(0.0, 0.0, self.width, TOGGLE_H),
        }];
        if self.expanded {
            for (a, top) in self.accounts.iter().zip(self.row_tops()) {
                parts.push(AccessiblePart {
                    name: format!("{}, {}", a.name, if a.remote { &a.server } else { &a.email }),
                    role: AccessRole::ListItem,
                    bounds: Rect::new(0.0, top, self.width, top + row_height(a)),
                });
                if !(a.connected && !a.remote) {
                    let (primary, remove) = self.buttons(None, a, top);
                    parts.push(AccessiblePart { name: primary_label(a).to_string(), role: AccessRole::Button, bounds: primary });
                    parts.push(AccessiblePart { name: ShellControlsResources::account_remove().to_string(), role: AccessRole::Button, bounds: remove });
                }
            }
        }
        parts
    }

    fn on_paint(&mut self, e: &mut PaintEventCx<'_>) {
        if self.accounts.is_empty() && self.design_mode() {
            self.accounts = Shared::new(crate::controls::account_menu::design_data().1);
        }
        let r = e.clip_rectangle;
        self.width = r.right - r.left;
        let c: &dyn Canvas = e.graphics;
        let t = c.theme().clone();
        let f = c.formats();
        if self.accounts.is_empty() {
            e.raise(self, "OnPaint");
            return;
        }
        let card = Rect::new(r.left, r.top, r.right, r.top + self.height());
        c.fill_rounded(&card, CARD_RADIUS, &kubuno_desktop::Application::theme().layer_background);
        c.push_clip_rounded(&card, CARD_RADIUS);
        let hover = |c: &dyn Canvas, band: Rect| {
            // `hover:bg-surface-1`: the text colour at 4 %, readable in both themes.
            let mut h = t.text_primary;
            h.a = 0.04;
            c.fill_rounded(&band, 0.0, &h);
        };
        // The toggle row.
        let toggle = Rect::new(r.left, r.top, r.right, r.top + TOGGLE_H);
        if self.hot == Some(Part::Toggle) {
            hover(c, toggle);
        }
        let label = if self.expanded { ShellControlsResources::account_hide_more() } else { ShellControlsResources::account_show_more() };
        c.text_ellipsis(label, &Rect::new(toggle.left + PAD_X, toggle.top, toggle.right - 100.0, toggle.bottom), &f.body_strong, &t.text_primary);
        let cy = (toggle.top + toggle.bottom) / 2.0;
        let chevron = Rect::new(toggle.right - PAD_X - CHEVRON, cy - CHEVRON / 2.0, toggle.right - PAD_X, cy + CHEVRON / 2.0);
        c.vector_icon(if self.expanded { "ChevronUp" } else { "ChevronDown" }, &chevron, CHEVRON, &t.text_tertiary);
        if !self.expanded {
            // The first two accounts' letters, overlapping (`-space-x-1`), then « +N ».
            let shown: Vec<String> = self.accounts.iter().take(2).map(|a| letter(&a.name)).collect();
            let more = self.accounts.len().saturating_sub(2);
            let mut items = shown;
            if more > 0 {
                items.push(format!("+{more}"));
            }
            let n = items.len() as f32;
            let mut x = chevron.left - 8.0 - (n * MINI - (n - 1.0) * 4.0);
            let ground = kubuno_desktop::Application::theme().layer_background;
            for text in items {
                let m = Rect::new(x, cy - MINI / 2.0, x + MINI, cy + MINI / 2.0);
                // `border-2 border-white`: the card's own ground around each disc.
                c.fill_rounded(&Rect::new(m.left - 2.0, m.top - 2.0, m.right + 2.0, m.bottom + 2.0), pill(MINI + 4.0), &ground);
                c.fill_rounded(&m, pill(MINI), &t.surface_3);
                c.text(&text, &m, &f.micro, &t.text_secondary, true);
                x += MINI - 4.0;
            }
        }
        if self.expanded {
            self.button_rects = self.accounts.iter().zip(self.row_tops()).map(|(a, top)| (!(a.connected && !a.remote)).then(|| self.buttons(Some(c), a, top))).collect();
            for (i, (a, top)) in self.accounts.iter().zip(self.row_tops()).enumerate() {
                let band = Rect::new(r.left, r.top + top, r.right, r.top + top + row_height(a));
                let simple = a.connected && !a.remote;
                if simple && self.hot == Some(Part::Row(i)) && !self.busy {
                    hover(c, band);
                }
                if i > 0 {
                    // `divide-y divide-border/50`.
                    Separator::horizontal().paint(c, Rect::new(band.left, band.top, band.right, band.top + Separator::THICKNESS), WidgetState::REST);
                }
                let line_cy = band.top + PAD_Y + AVATAR / 2.0;
                let avatar = Rect::new(band.left + PAD_X, line_cy - AVATAR / 2.0, band.left + PAD_X + AVATAR, line_cy + AVATAR / 2.0);
                Self::paint_avatar(c, avatar, a);
                let mut right = band.right - PAD_X;
                if a.remote {
                    right = Self::paint_pill(c, right, line_cy, &a.server, true) - GAP;
                } else if !a.connected {
                    right = Self::paint_pill(c, right, line_cy, ShellControlsResources::account_disconnected(), false) - GAP;
                } else if a.unread > 0 {
                    let text = if a.unread > 9 { "9+".to_string() } else { a.unread.to_string() };
                    let w = (c.measure(&text, &f.micro) + 8.0).max(18.0);
                    let badge = Rect::new(right - w, line_cy - 9.0, right, line_cy + 9.0);
                    c.fill_rounded(&badge, pill(18.0), &t.danger);
                    c.text(&text, &badge, &f.micro, &t.accent_foreground, true);
                    right = badge.left - GAP;
                }
                let text_left = avatar.right + GAP;
                let top_line = line_cy - 18.0;
                c.text_ellipsis(&a.name, &Rect::new(text_left, top_line, right, top_line + 20.0), &f.body_strong, &t.text_primary);
                c.text_ellipsis(&a.email, &Rect::new(text_left, top_line + 20.0, right, top_line + 36.0), &f.caption, &t.text_tertiary);
                if !simple {
                    let Some((p, rm)) = self.button_rects[i] else { continue };
                    let at = |b: Rect| Rect::new(r.left + b.left, r.top + b.top, r.left + b.right, r.top + b.bottom);
                    let mut primary = Button::new(primary_label(a)).variant(Variant::Primary).size(ButtonSize::Sm);
                    if a.remote {
                        primary = primary.icon("ExternalLink");
                    }
                    primary.paint(c, at(p), WidgetState::REST.hot(self.hot == Some(Part::Primary(i))).pressed(self.pressed == Some(Part::Primary(i))));
                    Button::new(ShellControlsResources::account_remove())
                        .variant(Variant::Secondary)
                        .size(ButtonSize::Sm)
                        .paint(c, at(rm), WidgetState::REST.hot(self.hot == Some(Part::Remove(i))).pressed(self.pressed == Some(Part::Remove(i))));
                }
            }
        }
        c.pop_clip_rounded();
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
        self.invalidate();
        e.raise(&*self, "OnMouseDown");
    }

    fn on_mouse_up(&mut self, e: &mut EventCx<'_, MouseEventArgs>) {
        let at = self.part_at(e.args().x, e.args().y);
        if let (Some(part), Some(p)) = (at, self.pressed.take()) {
            if part == p {
                self.activate(part);
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

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: &str, connected: bool, remote: bool) -> AccountEntry {
        AccountEntry { id: id.into(), name: format!("Compte {id}"), email: format!("{id}@exemple.fr"), server: "cloud.exemple.fr".into(), connected, remote, ..AccountEntry::default() }
    }

    #[test]
    fn the_card_is_its_toggle_and_its_rows() {
        let accounts = vec![entry("a", true, false), entry("b", false, false), entry("c", true, true)];
        assert_eq!(card_height(&accounts, false), TOGGLE_H);
        assert_eq!(card_height(&accounts, true), TOGGLE_H + ROW_H + 2.0 * ACTION_ROW_H);
        assert_eq!(card_height(&[], true), 0.0);
        assert_eq!(ACTION_ROW_H, 106.0);
    }

    #[test]
    fn clicks_fold_open_and_remove() {
        let mut card = AccountsCard { accounts: Shared::new(vec![entry("a", true, false), entry("b", false, false)]), ..AccountsCard::default() };
        assert_eq!(card.part_at(10.0, 10.0), Some(Part::Toggle));
        assert_eq!(card.part_at(10.0, TOGGLE_H + 10.0), Some(Part::Row(0)));
        let top = TOGGLE_H + ROW_H;
        let (primary, remove) = card.buttons(None, &card.accounts[1].clone(), top);
        assert_eq!(card.part_at(primary.left + 2.0, primary.top + 2.0), Some(Part::Primary(1)));
        assert_eq!(card.part_at(remove.left + 2.0, remove.top + 2.0), Some(Part::Remove(1)));
        card.activate(Part::Toggle);
        assert!(!card.expanded);
        assert_eq!(card.part_at(10.0, TOGGLE_H + 10.0), None, "folded: no rows");
        assert_eq!(initials_of("Bob O'Brien"), "BO");
        assert_eq!(initials_of("Camille (Mairie)"), "CM");
    }
}
