//! Gallery page — **composition**: the families assembled the way real screens
//! assemble them.
//!
//! Every other page shows one family alone, on the window ground, in rectangles
//! the page chose for it. That is exactly the situation in which a primitive
//! cannot go wrong — and the user reported that the renders turn « weird » as
//! soon as components are *combined*. This page is the regression bench for
//! that: nothing here is a demo cell, everything is a screen.
//!
//! The exposition is split into **scenes**, picked from a strip of tabs at its
//! top (or pre-selected with `--scene <1..6|name>` on the command line or
//! `KUBUNO_COMPOSITION_SCENE=<1..6|name>`, so a capture script can land on
//! one), and it scrolls with the wheel when a scene is
//! taller than the window:
//!
//! 1. **Réglages** — a settings screen: `Panel` > `Card` > `GroupBox` >
//!    labelled fields (`OutlinedField`, `TextField`, `HelpButton`), switches,
//!    check boxes, radios, `ComboBox` beside `Dropdown`, `NumericField` beside
//!    `Slider`, a `DatePicker`, and a footer of buttons.
//! 2. **Données** — a toolbar row (`SearchField` + `Dropdown` + buttons) above
//!    a `DataTable` whose role column carries `Badge`s, with a row menu open.
//! 3. **Explorateur** — a file manager: `Toolbar`, `Breadcrumb`, a `Splitter`
//!    between a `TreeView` and `Tabs` holding a scrolled `ListView`, and a
//!    `StatusBar`.
//! 4. **États** — `Callout`, `ProgressBar`, `Spinner`, `EmptyState`,
//!    `Accordion` and `Stepper`, each inside a `Card`.
//! 5. **Dialogue** — a modal `FloatingWindow` whose body is a form, over a page,
//!    and a `ConfirmDialog` carrying a very long file name.
//! 6. **Débordements** — long labels in narrow containers, very long words,
//!    large numbers, disabled and focused states flush against a clip, and a
//!    filter bar that puts every field family on ONE row so their heights and
//!    baselines can be compared.
//!
//! The live column is a settings form inside a clipped, wheel-scrolled
//! container: its `ComboBox`, `Dropdown`, `DatePicker`, menu, help bubble and
//! tooltip must all ESCAPE that container (they are hosted in
//! [`host::popup`] / [`host::overlay`] windows), and Tab / typing / Escape go
//! through the page's focus ring.

use std::cell::RefCell;

use kubuno_controls::datetime::Date;
use kubuno_controls::enums::{CheckState, HorizontalAlignment, Padding};
use kubuno_controls::host::{self, vk, Cursor, Frame, Modifiers};
use kubuno_controls::toolstrip::StripItem;
use kubuno_ui::buttons::{Button, CheckBox, IconButton, RadioButton, Size as ButtonSize, Switch, Variant};
use kubuno_ui::containers::{band, Axis, Card, GroupBox, Panel, ScrollView, Splitter, Surface};
use kubuno_ui::datetime::{DatePicker, HeaderPart};
use kubuno_ui::dialogs::{place, Actions, ConfirmDialog, FloatingWindow};
use kubuno_ui::display::{Badge, BadgeSize, BadgeVariant, Label, LinkLabel, Role, Side, Tooltip, wrap_lines};
use kubuno_ui::editors::{Dropdown, DropdownVariant};
use kubuno_ui::feedback::{
    Accordion, AccordionSection, AccordionSize, Callout, CalloutAction, CalloutVariant, EmptyState,
    EmptyStateVariant, Spinner, SpinnerSize, Step, StepStatus, Stepper,
};
use kubuno_ui::fields::OutlinedField;
use kubuno_ui::focus::FocusOpts;
use kubuno_ui::help::{HelpBubble, HelpButton, HelpPart, HelpPlacement};
use kubuno_ui::lists::{separator, ComboBox, ListBox, Menu, MenuEntry, FLOAT_SHADOW_MARGIN};
use kubuno_ui::navigation::{
    icon_item, icon_label_item, menu_item, separator_item, status_item, toggle_item, Breadcrumb,
    StatusBar, Tabs, Toolbar,
};
use kubuno_ui::range::{NumericField, ProgressBar, ProgressSize, Slider};
use kubuno_ui::tables::{aligned, column, flags, with_flag, BulkAction, Cell, DataTable, Layout, ListViewItem};
use kubuno_ui::text::{SearchField, TextField};
use kubuno_ui::views::{ColumnHeader, ListView, TreeNode, TreeView, View};
use kubuno_ui::{Canvas, DockStyle, FocusState, Rect, Size, Widget, WidgetState};

use super::interact::{self, Live};
use super::sheet::{Page, MARGIN};

// ─────────────────────────────────────────────────────────────────────────────
// Metrics — the page's own layout rhythm (the web's gaps, named once)
// ─────────────────────────────────────────────────────────────────────────────

mod m {
    /// `gap-4` between two cards / columns.
    pub const GAP: f32 = 16.0;
    /// `gap-3` between two stacked form rows.
    pub const ROW_GAP: f32 = 12.0;
    /// `gap-1` between a form label and its control (`Input.tsx`: `flex-col gap-1`).
    pub const LABEL_GAP: f32 = 4.0;
    /// The label line (`text-sm font-medium` at the desktop's 12 px body).
    pub const LABEL_H: f32 = 16.0;
    /// A setting row: title + description on the left, control on the right.
    pub const SETTING_H: f32 = 44.0;
    /// The scene strip's gap under it.
    pub const STRIP_GAP: f32 = 12.0;
    /// A caption line announcing what a block exercises.
    pub const NOTE_H: f32 = 16.0;
    /// Shadow margin a popup reserves around its surface (`SHADOW_MENU` ~7 DIP).
    pub const SHADOW: f32 = 10.0;
    /// Distance a menu / list opens from its anchor.
    pub const ANCHOR_GAP: f32 = 4.0;
    /// The monitor edge a floating surface keeps (`MenuDropdown`: 8 px).
    pub const EDGE: f32 = 8.0;
    /// The vertical filter bar the alignment audit lays its controls on.
    pub const BAR_H: f32 = 48.0;
    /// Height of the live column's scroll container — shorter than the form.
    pub const LIVE_VIEW_H: f32 = 400.0;
    /// How far a focus ring reaches outside its control (`ring-2` +
    /// `ring-offset-1`), rounded up: the inset a clipped container keeps.
    pub const RING_INSET: f32 = 4.0;
}

/// The date every picker is pinned to, so each capture is reproducible.
const TODAY: Date = Date::new(2026, 9, 23);

/// The six scenes, in strip order.
const SCENES: [&str; 6] = ["Réglages", "Données", "Explorateur", "États", "Dialogue", "Débordements"];

// ─────────────────────────────────────────────────────────────────────────────
// Exposition state: the chosen scene and the wheel scroll
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Default)]
struct Expo {
    /// The scene shown, an index into [`SCENES`].
    scene: usize,
    /// Whether `KUBUNO_COMPOSITION_SCENE` was read already.
    seeded: bool,
    /// Vertical scroll of the scene area, in DIP.
    scroll: f32,
    /// Last frame's content height, to clamp the scroll.
    content_h: f32,
    prev_down: bool,
}

thread_local! {
    static EXPO: RefCell<Expo> = RefCell::new(Expo::default());
}

/// Resolves a scene given as a 1-based number or a name (case, accents and
/// prefixes tolerated: `6`, `debordements`, `Débord`, `etats`).
fn scene_from(arg: &str) -> Option<usize> {
    let arg = arg.trim();
    if let Ok(n) = arg.parse::<usize>() {
        return (1..=SCENES.len()).contains(&n).then(|| n - 1);
    }
    fn fold(s: &str) -> String {
        s.chars()
            .map(|ch| match ch {
                'é' | 'è' | 'ê' | 'É' | 'È' => 'e',
                'à' | 'â' => 'a',
                'ô' => 'o',
                other => other.to_ascii_lowercase(),
            })
            .collect()
    }
    let want = fold(arg);
    if want.is_empty() {
        return None;
    }
    SCENES.iter().position(|s| fold(s).starts_with(&want))
}

/// The scene to open first: `--scene <n|name>` on the command line, else the
/// `KUBUNO_COMPOSITION_SCENE` environment variable — so a capture script can
/// land on any scene without clicking.
fn initial_scene() -> Option<usize> {
    let args: Vec<String> = std::env::args().collect();
    let value = args
        .iter()
        .position(|a| a == "--scene")
        .and_then(|i| args.get(i + 1))
        .map(String::as_str)
        .or_else(|| args.iter().find_map(|a| a.strip_prefix("--scene=")));
    let from_args = value.and_then(scene_from);
    from_args.or_else(|| std::env::var("KUBUNO_COMPOSITION_SCENE").ok().and_then(|s| scene_from(&s)))
}

pub fn draw(c: &dyn Canvas, f: &Frame) {
    EXPO.with(|e| {
        let mut e = e.borrow_mut();
        if !e.seeded {
            e.seeded = true;
            if let Some(i) = initial_scene() {
                e.scene = i;
            }
        }
        let clicked = f.mouse_down && !e.prev_down;
        e.prev_down = f.mouse_down;

        let page = Page::new(c, f.size.0 - interact::PANEL_W(), f.size.1);
        let area = Rect::new(MARGIN, page.y, page.area.right - MARGIN, page.area.bottom);

        // ── The scene strip — itself a composition: Tabs on the page ground ──
        let mut strip = Tabs::new();
        for s in SCENES {
            strip = strip.with(s);
        }
        strip.selected_index = e.scene as i32;
        let strip_h = strip.measure(c).height;
        let strip_rect = Rect::new(area.left, area.top, area.right, area.top + strip_h);
        if clicked {
            if let Some(i) = strip.item_at(c, strip_rect, f.mouse.0, f.mouse.1) {
                if i != e.scene {
                    e.scene = i;
                    e.scroll = 0.0;
                }
            }
        }
        strip.selected_index = e.scene as i32;
        let hot = strip.item_at(c, strip_rect, f.mouse.0, f.mouse.1);
        strip.paint_tabs(c, strip_rect, hot);
        if hot.is_some() {
            host::set_cursor(Cursor::Hand);
        }

        // ── The scene, scrolled by the wheel and clipped under the strip ──
        let view = Rect::new(0.0, strip_rect.bottom + 1.0, page.area.right, page.area.bottom);
        if view.contains(f.mouse.0, f.mouse.1) {
            e.scroll += f.wheel.1 * host::WHEEL_NOTCH_DIP;
            if f.wheel.1 != 0.0 {
                host::claim_wheel();
            }
        }
        let reach = (e.content_h - (view.bottom - view.top)).max(0.0);
        e.scroll = e.scroll.clamp(0.0, reach);
        let top = view.top + m::STRIP_GAP - e.scroll;
        let scene = Rect::new(area.left, top, area.right, top + 10_000.0);

        c.push_clip(&view);
        let used = match e.scene {
            0 => scene_settings(c, f, scene),
            1 => scene_data(c, f, scene),
            2 => scene_explorer(c, f, scene),
            3 => scene_states(c, f, scene),
            4 => scene_dialog(c, f, scene),
            _ => scene_overflow(c, f, scene),
        };
        c.pop_clip();
        e.content_h = used + m::STRIP_GAP + MARGIN;

        // A thin scroll indicator, so a capture says whether it is scrolled.
        if reach > 0.0 {
            let t = c.theme();
            let track_h = view.bottom - view.top;
            let thumb_h = (track_h * track_h / e.content_h).max(24.0);
            let y = view.top + (track_h - thumb_h) * (e.scroll / reach);
            let r = Rect::new(page.area.right - 5.0, y, page.area.right - 2.0, y + thumb_h);
            c.fill_rounded(&r, 1.5, &t.border_strong);
        }
    });
}

// ─────────────────────────────────────────────────────────────────────────────
// Small composition helpers — what an application writes around the primitives
// ─────────────────────────────────────────────────────────────────────────────

/// A grey note stating what the block below exercises; returns the next top.
fn note(c: &dyn Canvas, x: f32, y: f32, right: f32, text: &str) -> f32 {
    let t = c.theme();
    c.text_ellipsis(text, &Rect::new(x, y, right, y + m::NOTE_H), &c.formats().caption, &t.text_tertiary);
    y + m::NOTE_H + 4.0
}

/// A form label above a control (`Input.tsx`: `text-sm font-medium`, `gap-1`);
/// returns the control's top.
fn form_label(c: &dyn Canvas, x: f32, y: f32, right: f32, text: &str) -> f32 {
    let t = c.theme();
    c.text_ellipsis(text, &Rect::new(x, y, right, y + m::LABEL_H), &c.formats().body, &t.text_primary);
    y + m::LABEL_H + m::LABEL_GAP
}

/// A form label followed by its « ? »; returns `(control_top, help_rect)`.
fn form_label_help(c: &dyn Canvas, x: f32, y: f32, text: &str, button: &HelpButton, state: WidgetState) -> (f32, Rect) {
    let t = c.theme();
    let f = c.formats();
    let lw = c.measure(text, &f.body).ceil();
    c.text(text, &Rect::new(x, y, x + lw, y + m::LABEL_H), &f.body, &t.text_primary, false);
    let d = button.measure(c);
    let bx = x + lw + 4.0;
    let by = y + (m::LABEL_H - d.height) / 2.0;
    let r = Rect::new(bx, by, bx + d.width, by + d.height);
    button.paint(c, r, state);
    (y + m::LABEL_H + m::LABEL_GAP, r)
}

/// A settings row: title + description on the left, `Switch` on the right,
/// closed by a hairline — the shape every settings screen of the web uses.
fn setting_row(c: &dyn Canvas, r: Rect, title: &str, desc: &str, on: bool, state: WidgetState) -> Rect {
    let t = c.theme();
    let f = c.formats();
    let sw_w = switch_size(c).width;
    let text_right = r.right - sw_w - m::ROW_GAP;
    c.text_ellipsis(title, &Rect::new(r.left, r.top + 4.0, text_right, r.top + 20.0), &f.body, &t.text_primary);
    c.text_ellipsis(desc, &Rect::new(r.left, r.top + 21.0, text_right, r.top + 37.0), &f.caption, &t.text_secondary);
    let cy = (r.top + r.bottom) / 2.0;
    let sw = Rect::new(r.right - sw_w, cy - switch_size(c).height / 2.0, r.right, cy + switch_size(c).height / 2.0);
    Switch::new().on(on).paint(c, sw, state);
    c.fill_rounded(&Rect::new(r.left, r.bottom - 1.0, r.right, r.bottom), 0.0, &t.divider);
    sw
}

/// The switch's own size — its metrics are private to the family.
fn switch_size(c: &dyn Canvas) -> Size {
    Switch::new().measure(c)
}

/// A rectangle `h` tall centred on the row `r`.
fn centred(r: Rect, x0: f32, x1: f32, h: f32) -> Rect {
    let cy = (r.top + r.bottom) / 2.0;
    Rect::new(x0, cy - h / 2.0, x1, cy + h / 2.0)
}

/// The body width a card `w` DIP wide leaves its content.
fn card_body_width(card: &Card, w: f32) -> f32 {
    let b = card.body_rect(Rect::new(0.0, 0.0, w, 1000.0));
    b.right - b.left
}

/// The height a card `w` DIP wide needs to show `content_h` DIP of body —
/// its header and paddings asked of the card itself, never guessed.
fn card_height(card: &Card, w: f32, content_h: f32) -> f32 {
    let probe = Rect::new(0.0, 0.0, w, 1000.0);
    let b = card.body_rect(probe);
    b.top + content_h + (probe.bottom - b.bottom)
}

fn filled_combo(items: &[&str], sel: usize) -> ComboBox {
    let mut cb = ComboBox::new();
    for it in items {
        cb.add_item(*it);
    }
    cb.max_drop_down_items = 5;
    cb.set_selected_index(sel as i32);
    cb
}

fn filled_dropdown(items: &[(&str, Option<&str>)], sel: usize) -> Dropdown {
    let mut d = Dropdown::new();
    for (label, icon) in items {
        d.add_option(*label, *icon);
    }
    d.set_selected_index(sel as i32);
    d
}

const LANGUAGES: [&str; 6] = ["Français", "English", "Deutsch", "Español", "Italiano", "Português"];
const TIMEZONES: [(&str, Option<&str>); 4] = [
    ("Europe/Paris (UTC+2)", Some("Globe")),
    ("Africa/Douala (UTC+1)", Some("Globe")),
    ("America/Montreal (UTC−4)", Some("Globe")),
    ("Asia/Tokyo (UTC+9)", Some("Globe")),
];
const ROLES: [(&str, Option<&str>); 3] =
    [("Membre", Some("User")), ("Administrateur", Some("Shield")), ("Invité", Some("UserPlus"))];

// ─────────────────────────────────────────────────────────────────────────────
// Scene 1 — a settings screen: Panel > Card > GroupBox > labelled controls
// ─────────────────────────────────────────────────────────────────────────────

fn scene_settings(c: &dyn Canvas, _f: &Frame, a: Rect) -> f32 {
    let t = c.theme();
    let y = note(
        c,
        a.left,
        a.top,
        a.right,
        "Panel (layer) > Card > GroupBox > champs libellés — les cartes sont peintes PAR L'APPELANT sur le Panel, comme dans une app",
    );

    // The page ground of a settings screen: a layer panel with the LG padding.
    let panel_h = 612.0;
    let panel = Rect::new(a.left, y, a.right, y + panel_h);
    Panel::new().with_surface(Surface::Layer).with_padding(Padding::all(m::GAP)).paint(c, panel, WidgetState::REST);
    let inner = Rect::new(panel.left + m::GAP, panel.top + m::GAP, panel.right - m::GAP, panel.bottom - m::GAP);
    let col_w = (inner.right - inner.left - m::GAP) / 2.0;
    let col_a = Rect::new(inner.left, inner.top, inner.left + col_w, inner.bottom - 52.0);
    let col_b = Rect::new(col_a.right + m::GAP, inner.top, inner.right, col_a.bottom);

    // ── Column A: the profile card ───────────────────────────────────────────
    let card = Card::titled("Profil").with_subtitle("Informations visibles par les membres de l'espace");
    card.paint(c, col_a, WidgetState::REST);
    let body = card.body_rect(col_a);
    let group = GroupBox::titled("Identité").with_padding(Padding::all(12.0));
    let group_rect = Rect::new(body.left, body.top, body.right, body.bottom);
    group.paint(c, group_rect, WidgetState::REST);
    let g = group.display_rect(group_rect);
    // `display_rect` is LOCAL space: rebase it on the box.
    let g = Rect::new(group_rect.left + g.left, group_rect.top + g.top, group_rect.left + g.right, group_rect.top + g.bottom);
    let mut gy = g.top;

    let mut name = OutlinedField::new("Nom complet").required(true);
    name.text = "Amélie Rousseau".into();
    name.paint(c, Rect::new(g.left, gy, g.right, gy + 48.0), WidgetState::REST);
    gy += 48.0 + m::ROW_GAP;

    let (ctl, _) = form_label_help(c, g.left, gy, "Adresse de récupération", &HelpButton::new(), WidgetState::REST);
    let mut mail = OutlinedField::new("Adresse e-mail").with_leading("AtSign");
    mail.text = "amelie.rousseau@kubuno.com".into();
    mail.paint(c, Rect::new(g.left, ctl, g.right, ctl + 48.0), WidgetState::REST);
    gy = ctl + 48.0 + m::ROW_GAP;

    let ctl = form_label(c, g.left, gy, g.right, "Téléphone");
    let mut phone = TextField::new();
    phone.placeholder_text = "+33 6 12 34 56 78".into();
    phone.paint(c, Rect::new(g.left, ctl, g.right, ctl + 36.0), WidgetState::REST);
    gy = ctl + 36.0 + m::ROW_GAP;

    // ComboBox and Dropdown on ONE row: two select-like controls a form puts
    // side by side. Each gets its own height from the family.
    let half = (g.right - g.left - m::ROW_GAP) / 2.0;
    let ctl_l = form_label(c, g.left, gy, g.left + half, "Langue");
    form_label(c, g.left + half + m::ROW_GAP, gy, g.right, "Fuseau horaire");
    let lang = filled_combo(&LANGUAGES, 0);
    let lang_h = lang.field_height();
    lang.paint(c, Rect::new(g.left, ctl_l, g.left + half, ctl_l + lang_h), WidgetState::REST);
    let tz = filled_dropdown(&TIMEZONES, 0);
    tz.paint(c, Rect::new(g.left + half + m::ROW_GAP, ctl_l, g.right, ctl_l + tz.height), WidgetState::REST);
    gy = ctl_l + lang_h.max(tz.height) + m::ROW_GAP;

    let ctl = form_label(c, g.left, gy, g.right, "Biographie (désactivé)");
    let mut bio = TextField::new();
    bio.set_text("Responsable du pôle documentaire depuis 2019.");
    bio.paint(c, Rect::new(g.left, ctl, g.right, ctl + 36.0), WidgetState::REST.disabled(true));

    // ── Column B: the preferences card ───────────────────────────────────────
    let card = Card::titled("Préférences");
    card.paint(c, col_b, WidgetState::REST);
    let body = card.body_rect(col_b);
    let group = GroupBox::titled("Notifications").with_padding(Padding::all(12.0));
    // The caption band + top padding, asked of the box itself, then the rows.
    let band_top = group.display_rect(Rect::new(0.0, 0.0, 100.0, 1000.0)).top;
    let group_h = band_top + 3.0 * m::SETTING_H + 6.0 + 24.0 + 20.0 + 12.0;
    let group_rect = Rect::new(body.left, body.top, body.right, body.top + group_h);
    group.paint(c, group_rect, WidgetState::REST);
    let g = group.display_rect(group_rect);
    let g = Rect::new(group_rect.left + g.left, group_rect.top + g.top, group_rect.left + g.right, group_rect.top + g.bottom);
    let mut gy = g.top;
    setting_row(c, Rect::new(g.left, gy, g.right, gy + m::SETTING_H), "Résumé par e-mail", "Un récapitulatif quotidien des activités de l'espace", true, WidgetState::REST);
    gy += m::SETTING_H;
    setting_row(c, Rect::new(g.left, gy, g.right, gy + m::SETTING_H), "Notifications push", "Sur les appareils connectés", false, WidgetState::REST);
    gy += m::SETTING_H;
    setting_row(c, Rect::new(g.left, gy, g.right, gy + m::SETTING_H), "Alertes de sécurité", "Imposé par l'administrateur", true, WidgetState::REST.disabled(true));
    gy += m::SETTING_H + 6.0;
    CheckBox::new("Me notifier des mentions").check(CheckState::Checked).paint(c, Rect::new(g.left, gy, g.right, gy + 20.0), WidgetState::REST);
    gy += 24.0;
    CheckBox::new("Inclure les dossiers partagés").tri_state().check(CheckState::Indeterminate).paint(c, Rect::new(g.left, gy, g.right, gy + 20.0), WidgetState::REST);

    let mut by = group_rect.bottom + m::ROW_GAP;
    // A horizontal radio group — three options on one line, each at its own width.
    let ctl = form_label(c, body.left, by, body.right, "Thème");
    let mut x = body.left;
    for (i, name) in ["Clair", "Sombre", "Système"].into_iter().enumerate() {
        let rb = RadioButton::new(name).selected(i == 2);
        let w = rb.measure(c).width;
        rb.paint(c, Rect::new(x, ctl, x + w, ctl + 20.0), WidgetState::REST);
        x += w + m::GAP;
    }
    by = ctl + 20.0 + m::ROW_GAP;

    // NumericField beside a Slider — the value and its handle on one line.
    let ctl = form_label(c, body.left, by, body.right, "Quota de stockage (Go)");
    let q = NumericField::ranged(0.0, 500.0).with_value(120.0).unwrap_or_default();
    let qh = q.measure(c).height;
    q.paint(c, Rect::new(body.left, ctl, body.left + 110.0, ctl + qh), WidgetState::REST);
    let mut s = Slider::new();
    s.set_maximum(500);
    let _ = s.set_value(120);
    let sh = s.measure(c).height;
    s.paint(c, centred(Rect::new(0.0, ctl, 0.0, ctl + qh), body.left + 110.0 + m::ROW_GAP, body.right, sh), WidgetState::REST);
    by = ctl + qh + m::ROW_GAP;

    let ctl = form_label(c, body.left, by, body.right, "Expiration du compte");
    let dp = DatePicker::short().on(TODAY);
    let dw = dp.measure(c).width;
    dp.paint(c, Rect::new(body.left, ctl, body.left + dw, ctl + dp.field_height()), WidgetState::REST);

    // ── The footer: a link on the left, the two actions on the right ─────────
    let foot = Rect::new(inner.left, inner.bottom - 36.0, inner.right, inner.bottom);
    c.fill_rounded(&Rect::new(inner.left, foot.top - m::ROW_GAP, inner.right, foot.top - m::ROW_GAP + 1.0), 0.0, &t.divider);
    let link = LinkLabel::new("Réinitialiser les préférences");
    let lw = link.measure(c);
    link.paint(c, centred(foot, foot.left, foot.left + lw.width, lw.height), WidgetState::REST);
    let save = Button::new("Enregistrer").variant(Variant::Primary);
    let sr = save.rect_ending_at(c, foot.right, foot.top);
    save.paint(c, sr, WidgetState::REST);
    let cancel = Button::new("Annuler").variant(Variant::Secondary);
    let cr = cancel.rect_ending_at(c, sr.left - 8.0, foot.top);
    cancel.paint(c, cr, WidgetState::REST);

    panel.bottom - a.top
}

// ─────────────────────────────────────────────────────────────────────────────
// Scene 2 — a data screen: Card > toolbar row > DataTable with badges + menu
// ─────────────────────────────────────────────────────────────────────────────

const PEOPLE: [(&str, &str, &str, &str); 9] = [
    ("Amélie Rousseau", "Administratrice", "Aujourd'hui, 09:12", "4,2 Go"),
    ("Bastien Laurent", "Membre", "Hier, 18:40", "812 Mo"),
    ("Camille Fontaine-Delacroix de Saint-Exupéry", "Membre", "16 sept. 2026", "1,9 Go"),
    ("Damien Perrot", "Invité", "15 sept. 2026", "42 Mo"),
    ("Élise Marchand", "Suspendu", "02 août 2026", "6,4 Go"),
    ("Farid Benali", "Administrateur", "14 sept. 2026", "980 Mo"),
    ("Gaëlle Nguyen", "Membre", "14 sept. 2026", "2,7 Go"),
    ("Hugo Delacroix", "Membre", "13 sept. 2026", "158 Mo"),
    ("Inès Chevalier", "Invitée", "13 sept. 2026", "12 Mo"),
];

fn role_badge(role: &str) -> Badge {
    let v = match role {
        "Administratrice" | "Administrateur" => BadgeVariant::Primary,
        "Invité" | "Invitée" => BadgeVariant::Warning,
        "Suspendu" => BadgeVariant::Danger,
        _ => BadgeVariant::Default,
    };
    Badge::new(role).variant(v).dot(true)
}

fn people_table(width: f32) -> DataTable {
    let mut t = DataTable::new();
    t.selectable = true;
    t.row_actions = true;
    t.layout = Layout::Table;
    let name = (width - 40.0 - 48.0 - 150.0 - 150.0 - 120.0).max(140.0) as i32;
    t.columns = vec![
        with_flag(with_flag(column("name", "Nom", name), flags::SORTABLE), flags::PRIMARY),
        column("role", "Rôle", 150),
        with_flag(column("seen", "Dernière connexion", 150), flags::SORTABLE),
        aligned(column("quota", "Quota", 120), HorizontalAlignment::Right),
    ];
    // The role cell carries its text (the card layout and sorting read it);
    // the table's per-cell renderer — the web column's `cell` — draws it as a
    // badge capped to the cell, so it never spills into the next column.
    t.items = PEOPLE
        .iter()
        .map(|(n, r, s, q)| ListViewItem::new(*n).with_sub(*r).with_sub(*s).with_sub(*q))
        .collect();
    t.cell_painter = Some(Box::new(|c: &dyn Canvas, cell: &Cell<'_>| {
        if cell.column != 1 {
            return false;
        }
        let r = cell.rect;
        let b = role_badge(cell.text).max_width(r.right - r.left);
        let s = b.measure(c);
        b.paint(c, centred(r, r.left, r.left + s.width, s.height), WidgetState::REST.disabled(cell.disabled));
        true
    }));
    t.page_size = 6;
    t.page_size_options = vec![6, 12, 24];
    t.bulk_actions = vec![
        BulkAction::new("export", "Exporter").icon("Upload"),
        BulkAction::new("delete", "Supprimer").icon("Trash2").danger(true),
    ];
    t
}

fn row_menu() -> Menu {
    Menu::with_items(vec![
        MenuEntry::new("Modifier le profil").icon("PenLine").build(),
        MenuEntry::new("Changer de rôle").icon("Users").build(),
        MenuEntry::new("Réinitialiser le mot de passe").icon("KeyRound").build(),
        separator(),
        MenuEntry::new("Suspendre le compte").icon("Ban").build(),
        MenuEntry::new("Supprimer définitivement").icon("Trash2").danger().build(),
    ])
}

fn scene_data(c: &dyn Canvas, f: &Frame, a: Rect) -> f32 {
    let y = note(
        c,
        a.left,
        a.top,
        a.right,
        "Card > barre (SearchField · Dropdown ghost · boutons) > DataTable ; badges dans la colonne Rôle ; menu de ligne ouvert (ligne 3)",
    );
    let card = Card::titled("Membres de l'espace").with_subtitle("9 comptes · 2 invitations en attente");
    let card_rect = Rect::new(a.left, y, a.right, y + 520.0);
    card.paint(c, card_rect, WidgetState::REST);
    let body = card.body_rect(card_rect);

    // The toolbar row: search takes the slack, the rest sits at its own width.
    let row = Rect::new(body.left, body.top, body.right, body.top + SearchField::HEIGHT);
    let invite = Button::new("Inviter").icon("Plus").variant(Variant::Primary);
    let ir = invite.rect_ending_at(c, row.right, 0.0);
    let ir = centred(row, ir.left, ir.right, ButtonSize::Md.height());
    invite.paint(c, ir, WidgetState::REST);
    let filt = Button::new("Filtres").icon("Filter").variant(Variant::Secondary);
    let fw = filt.width(c);
    let fr = centred(row, ir.left - 8.0 - fw, ir.left - 8.0, ButtonSize::Md.height());
    filt.paint(c, fr, WidgetState::REST);
    let mut role = filled_dropdown(&[("Rôle : tous", None), ("Administrateurs", None), ("Membres", None)], 0);
    role.variant = DropdownVariant::Ghost;
    let rw = role.measure(c).width;
    let rr = centred(row, fr.left - 8.0 - rw, fr.left - 8.0, role.height);
    role.paint(c, rr, WidgetState::REST);
    let mut search = SearchField::new();
    search.set_text("");
    search.paint(c, Rect::new(row.left, row.top, rr.left - 12.0, row.bottom), WidgetState::REST);

    // The table under it, a partial selection so the bulk bar is up.
    let table_rect = Rect::new(body.left, row.bottom + m::ROW_GAP, body.right, body.bottom);
    let mut table = people_table(table_rect.right - table_rect.left);
    table.toggle_row(1);
    table.hot_index = table.row_at(table_rect, f.mouse.0, f.mouse.1);
    table.hot_chrome = table.chrome_at(c, table_rect, f.mouse.0, f.mouse.1);
    table.paint(c, table_rect, WidgetState::REST);

    // The row menu of row 3, open, anchored on the row's trailing edge — it is
    // taller than the rows left under it, so it must hang past the card.
    let anchor = table.row_rect(table_rect, 2);
    let menu = row_menu();
    let want = menu.measure(c);
    let mx = anchor.right - want.width - 8.0;
    let my = anchor.bottom - 4.0;
    menu.paint(c, Rect::new(mx, my, mx + want.width, my + want.height), WidgetState::REST);

    // A breadcrumb + status bar under the card: two strips an admin page ends on.
    let mut yy = card_rect.bottom + m::GAP;
    let trail = Breadcrumb::new().with("Administration").with("Espaces").with("Kubuno Cloud — équipe documentation").with("Membres");
    let th = trail.measure(c).height;
    trail.paint_segments(c, Rect::new(a.left, yy, a.right, yy + th), None, false);
    yy += th + 8.0;
    let bar = StatusBar::new()
        .with(status_item("9 membres", false))
        .with(separator_item())
        .with(status_item("1 sélectionné", true))
        .with(icon_label_item("HardDrive", "16,9 Go / 50 Go"));
    let bh = bar.measure(c).height;
    bar.paint_items(c, Rect::new(a.left, yy, a.right, yy + bh), None);
    yy + bh - a.top
}

// ─────────────────────────────────────────────────────────────────────────────
// Scene 3 — a file manager: Toolbar, Breadcrumb, Splitter(TreeView | Tabs >
// scrolled ListView), StatusBar — all inside one Card
// ─────────────────────────────────────────────────────────────────────────────

fn explorer_tree() -> TreeView {
    let mut t = TreeView::new();
    let docs = TreeNode::new("Documents")
        .expanded()
        .child(TreeNode::new("Contrats et avenants signés par les deux parties").expanded().child(TreeNode::new("2025")).child(TreeNode::new("2026")))
        .child(TreeNode::new("Factures"))
        .child(TreeNode::new("Notes de réunion"));
    let media = TreeNode::new("Médias").expanded().child(TreeNode::new("Photos")).child(TreeNode::new("Vidéos"));
    t.nodes = vec![docs, media, TreeNode::new("Partagés avec moi"), TreeNode::new("Corbeille")];
    fn ico(nodes: &mut [TreeNode]) {
        for n in nodes.iter_mut() {
            n.image_index = 0;
            ico(&mut n.children);
        }
    }
    ico(&mut t.nodes);
    t.image_list = vec!["Folder"];
    t.selected_path = Some(vec![0, 0, 1]);
    t
}

fn explorer_list(width: f32) -> ListView {
    const FILES: [(&str, &str, &str, i32); 12] = [
        ("Avenant n°3 — contrat de maintenance.pdf", "17 sept. 2026", "1,2 Mo", 1),
        ("Budget prévisionnel 2027.xlsx", "16 sept. 2026", "48 Ko", 1),
        ("Compte-rendu-comité-de-pilotage-trimestriel-septembre-2026-v4-final.docx", "15 sept. 2026", "212 Ko", 1),
        ("Contrat cadre.pdf", "14 sept. 2026", "3,4 Mo", 1),
        ("logo-kubuno.png", "11 sept. 2026", "212 Ko", 2),
        ("notes.md", "10 sept. 2026", "3 Ko", 1),
        ("Présentation client.pptx", "09 sept. 2026", "18,7 Mo", 1),
        ("Relevé 2026-08.pdf", "02 sept. 2026", "96 Ko", 1),
        ("Signature.png", "01 sept. 2026", "40 Ko", 2),
        ("Planning.ods", "30 août 2026", "22 Ko", 1),
        ("Archive.zip", "28 août 2026", "1,1 Go", 1),
        ("README.txt", "20 août 2026", "1 Ko", 1),
    ];
    let mut v = ListView::new();
    v.view = View::Details;
    v.full_row_select = true;
    v.multi_select = true;
    let name = (width - 120.0 - 80.0).max(140.0) as i32;
    v.columns = vec![ColumnHeader::new("Nom", name), ColumnHeader::new("Modifié", 120), ColumnHeader::new("Taille", 80)];
    v.items = FILES
        .iter()
        .map(|(n, d, s, i)| {
            let mut it = ListViewItem::new(*n).with_sub(*d).with_sub(*s);
            it.image_index = *i;
            it
        })
        .collect();
    v.image_list = vec!["Folder", "File", "Image"];
    v.click(3, false, false);
    v
}

fn scene_explorer(c: &dyn Canvas, f: &Frame, a: Rect) -> f32 {
    let y = note(
        c,
        a.left,
        a.top,
        a.right,
        "Card > Toolbar · Breadcrumb · Splitter( TreeView | Tabs > ListView défilée de 50 DIP ) · StatusBar",
    );
    let card = Card::new();
    let card_rect = Rect::new(a.left, y, a.right, y + 470.0);
    card.paint(c, card_rect, WidgetState::REST);
    let body = card.body_rect(card_rect);

    let bar = Toolbar::new()
        .with(menu_item("Plus", "Nouveau"))
        .with(separator_item())
        .with(icon_item("Cut"))
        .with(icon_item("Copy"))
        .with(icon_item("Paste"))
        .with(icon_label_item("Share2", "Partager"))
        .with(toggle_item("PanelRight", true));
    let bh = bar.row_height;
    let bar_rect = Rect::new(body.left, body.top, body.right, body.top + bh);
    let hot = bar.item_at(c, bar_rect, f.mouse.0, f.mouse.1);
    bar.paint_items(c, bar_rect, hot, false);

    let mut trail = Breadcrumb::new().with("Ce PC").with("Documents").with("Contrats et avenants signés par les deux parties").with("2026");
    trail.root_chevron = true;
    let th = trail.measure(c).height;
    let trail_rect = Rect::new(body.left, bar_rect.bottom + 8.0, body.right, bar_rect.bottom + 8.0 + th);
    trail.paint_segments(c, trail_rect, None, false);

    let status = StatusBar::new()
        .with(status_item("12 éléments", false))
        .with(separator_item())
        .with(status_item("1 sélectionné · 3,4 Mo", true))
        .with(icon_label_item("Cloud", "Synchronisé"));
    let sh = status.measure(c).height;
    let status_rect = Rect::new(body.left, body.bottom - sh, body.right, body.bottom);

    let split_rect = Rect::new(body.left, trail_rect.bottom + 8.0, body.right, status_rect.top - 8.0);
    let split = Splitter::vertical().with_distance(190.0).with_minimums(120.0, 200.0);
    let panes = split.arrange(split_rect);

    // Pane 1: the tree, clipped to its pane.
    let mut tree = explorer_tree();
    tree.hot_row = tree.node_at(panes.panel1, f.mouse.0, f.mouse.1);
    c.push_clip(&panes.panel1);
    tree.paint(c, panes.panel1, WidgetState::REST);
    c.pop_clip();

    // Pane 2: tabs whose page is a scrolled details list.
    let mut tabs = Tabs::new().with("Fichiers").with("Partages").with("Activité récente").with("Versions");
    tabs.selected_index = 0;
    tabs.paint_tabs(c, panes.panel2, tabs.item_at(c, panes.panel2, f.mouse.0, f.mouse.1));
    let page = tabs.page_rect(c, panes.panel2);
    let mut list = explorer_list(page.right - page.left);
    list.scroll = 50.0;
    list.hot_index = list.row_at(page, f.mouse.0, f.mouse.1);
    list.paint(c, page, WidgetState::REST.focused(true));

    let grip = split.hit_test_grip(split_rect, f.mouse.0, f.mouse.1);
    split.paint(c, split_rect, WidgetState::REST.hot(grip));

    status.paint_items(c, status_rect, None);
    card_rect.bottom - a.top
}

// ─────────────────────────────────────────────────────────────────────────────
// Scene 4 — feedback surfaces inside cards
// ─────────────────────────────────────────────────────────────────────────────

fn scene_states(c: &dyn Canvas, f: &Frame, a: Rect) -> f32 {
    let t = c.theme();
    let fm = c.formats();
    let y = note(
        c,
        a.left,
        a.top,
        a.right,
        "Card > Callout + ProgressBar + Spinner en ligne · Card > EmptyState · Card > Accordion > contrôles · Card > Stepper",
    );
    let col_w = (a.right - a.left - m::GAP) / 2.0;
    let left = Rect::new(a.left, y, a.left + col_w, y);
    let right = Rect::new(left.right + m::GAP, y, a.right, y);

    // ── Synchronisation card ─────────────────────────────────────────────────
    let callout = Callout::new("Il reste 1,2 Go sur les 50 Go du quota. Les nouveaux envois seront refusés au-delà.")
        .with_variant(CalloutVariant::Warning)
        .with_title("Espace bientôt plein")
        .with_action(CalloutAction::new("Libérer de l'espace").icon("Trash2"))
        .with_dismiss(true);
    let body_w = col_w - 2.0 * 16.0;
    let ch = callout.height_at(c, body_w);
    let card = Card::titled("Synchronisation").with_subtitle("Poste « BUREAU-AMELIE »");
    let card_h = card.header_height() + 16.0 + ch + 12.0 + 20.0 + 8.0 + 18.0 + 12.0 + 24.0 + 16.0;
    let r1 = Rect::new(left.left, y, left.right, y + card_h);
    card.paint(c, r1, WidgetState::REST);
    let b = card.body_rect(r1);
    callout.paint(c, Rect::new(b.left, b.top, b.right, b.top + ch), WidgetState::REST);
    let mut yy = b.top + ch + 12.0;
    // Label + percentage on one line, the bar under it.
    c.text("Envoi en cours", &Rect::new(b.left, yy, b.right, yy + 20.0), &fm.body, &t.text_primary, false);
    c.text_aligned("78 %", &Rect::new(b.left, yy, b.right, yy + 20.0), &fm.body, &t.text_secondary, windows::Win32::Graphics::DirectWrite::DWRITE_TEXT_ALIGNMENT_TRAILING);
    yy += 20.0 + 8.0;
    let mut bar = ProgressBar::new().with_size(ProgressSize::Md);
    bar.set_value(78);
    bar.paint(c, Rect::new(b.left, yy, b.right, yy + 18.0), WidgetState::REST);
    yy += 18.0 + 12.0;
    // Spinner inline with a label: its ring must sit on the text's centre line.
    let sp = Spinner::new().with_size(SpinnerSize::Sm).with_phase(0.3);
    let d = SpinnerSize::Sm.box_size();
    let row = Rect::new(b.left, yy, b.right, yy + 24.0);
    sp.paint(c, centred(row, b.left, b.left + d, d), WidgetState::REST);
    c.text("Envoi de 3 fichiers vers « Documents »…", &Rect::new(b.left + d + 8.0, row.top, b.right, row.bottom), &fm.body, &t.text_secondary, false);
    let mut bottom_l = r1.bottom;

    // ── Empty card ───────────────────────────────────────────────────────────
    let empty = EmptyState::new("Inbox", "Aucun fichier partagé avec vous")
        .with_variant(EmptyStateVariant::FirstUse)
        .with_description("Quand un membre partagera un dossier ou un document, il apparaîtra ici avec ses droits d'accès.")
        .with_action(Button::new("Demander un accès").size(ButtonSize::Sm).icon("Send"))
        .with_secondary_action(Button::new("En savoir plus").size(ButtonSize::Sm).variant(Variant::Ghost));
    let card = Card::titled("Partagés avec moi");
    let cw = right.right - right.left;
    // Height-for-width: the text wraps into the card's real body column.
    let eh = empty.height_at(c, card_body_width(&card, cw));
    let r2 = Rect::new(right.left, y, right.right, y + card_height(&card, cw, eh));
    card.paint(c, r2, WidgetState::REST);
    let b = card.body_rect(r2);
    empty.paint(c, Rect::new(b.left, b.top, b.right, b.top + eh), WidgetState::REST);
    let mut bottom_r = r2.bottom;

    // ── Accordion card: panels holding real controls ─────────────────────────
    let y2 = bottom_l.max(bottom_r) + m::GAP;
    let mut acc = Accordion::new()
        .with_size(AccordionSize::Sm)
        .section(AccordionSection::new("Partage de liens publics", 76.0).icon("Share2").badge("3").open(true))
        .section(AccordionSection::new("Rétention et corbeille", 30.0).icon("Clock"))
        .section(AccordionSection::new("Chiffrement de bout en bout (expérimental, réservé aux administrateurs)", 44.0).icon("Lock").open(true));
    let card = Card::titled("Paramètres avancés");
    let r3 = Rect::new(left.left, y2, left.right, y2 + card.header_height() + acc.total_height() + 32.0);
    card.paint(c, r3, WidgetState::REST);
    let b = card.body_rect(r3);
    let acc_rect = Rect::new(b.left, b.top, b.right, b.top + acc.total_height());
    acc.track_pointer(acc_rect, f.mouse.0, f.mouse.1);
    acc.paint(c, acc_rect, WidgetState::REST);
    let sections = acc.section_rects(acc_rect);
    if let Some(&s0) = sections.first() {
        let p = acc.panel_rect(s0, 0);
        CheckBox::new("Autoriser les liens sans mot de passe").paint(c, Rect::new(p.left, p.top + 4.0, p.right, p.top + 24.0), WidgetState::REST);
        CheckBox::new("Expiration automatique après 30 jours").check(CheckState::Checked).paint(c, Rect::new(p.left, p.top + 28.0, p.right, p.top + 48.0), WidgetState::REST);
        let sw = Rect::new(p.right - switch_size(c).width, p.top + 54.0, p.right, p.top + 54.0 + switch_size(c).height);
        c.text("Journaliser les accès", &Rect::new(p.left, p.top + 52.0, sw.left - 8.0, p.top + 76.0), &fm.body, &t.text_primary, false);
        Switch::new().on(true).paint(c, sw, WidgetState::REST);
    }
    if let Some(&s2) = sections.get(2) {
        let p = acc.panel_rect(s2, 2);
        let mut key = TextField::new();
        key.set_text("kb_pub_7f3a9c21d84e0b5f6a2c9e1d3b7f4a8c");
        key.leading_icon = Some("KeyRound");
        key.paint(c, Rect::new(p.left, p.top + 4.0, p.right, p.top + 40.0), WidgetState::REST);
    }
    bottom_l = r3.bottom;

    // ── Stepper card ─────────────────────────────────────────────────────────
    let mut stepper = Stepper::new()
        .step(Step::new("Source").description("dossier local"))
        .step(Step::new("Correspondance").description("colonnes"))
        .step(Step::new("Validation").description("3 erreurs"))
        .step(Step::new("Import"))
        .at(2);
    stepper.steps[2].status = Some(StepStatus::Error);
    let card = Card::titled("Assistant d'import");
    let cw = right.right - right.left;
    // The stepper picks its trail or compact view from the width it gets.
    let sh = stepper.height_at(card_body_width(&card, cw));
    let r4 = Rect::new(right.left, y2, right.right, y2 + card_height(&card, cw, sh + m::GAP + ButtonSize::Md.height()));
    card.paint(c, r4, WidgetState::REST);
    let b = card.body_rect(r4);
    stepper.paint(c, Rect::new(b.left, b.top, b.right, b.top + sh), WidgetState::REST);
    let next = Button::new("Continuer").variant(Variant::Primary);
    let nr = next.rect_ending_at(c, b.right, b.bottom - ButtonSize::Md.height());
    next.paint(c, nr, WidgetState::REST.disabled(true));
    let prev = Button::new("Retour").variant(Variant::Secondary);
    prev.paint(c, prev.rect_ending_at(c, nr.left - 8.0, nr.top), WidgetState::REST);
    bottom_r = r4.bottom;

    bottom_l.max(bottom_r) - a.top
}

// ─────────────────────────────────────────────────────────────────────────────
// Scene 5 — a modal whose body is a form, and a confirm with a long name
// ─────────────────────────────────────────────────────────────────────────────

fn scene_dialog(c: &dyn Canvas, f: &Frame, a: Rect) -> f32 {
    let y = note(
        c,
        a.left,
        a.top,
        a.right,
        "FloatingWindow modale : corps = Panel de widgets (OutlinedField · Dropdown · CheckBox · Callout) · puis ConfirmDialog avec un nom très long",
    );
    // The page behind the modal: a table in a card, so the veil has something to veil.
    let host = Rect::new(a.left, y, a.right, y + 470.0);
    let card = Card::titled("Membres de l'espace");
    card.paint(c, host, WidgetState::REST);
    let tb = card.body_rect(host);
    let table = people_table(tb.right - tb.left);
    table.paint(c, tb, WidgetState::REST);

    let mut w = FloatingWindow::new("Inviter un membre").modal().with_icon("UserPlus").with_actions(Actions::pair("Envoyer l'invitation", "Annuler"));
    {
        let body = w.body_mut();
        body.padding = Padding::all(20.0);
        // Reverse reading order: the engine gives the LAST child the top band.
        let info = Callout::new("L'invité recevra un lien valable 7 jours.").with_variant(CalloutVariant::Info);
        body.push_widget(band(DockStyle::Top, 40.0), Box::new(info));
        body.push_widget(band(DockStyle::Top, 12.0), Box::new(Label::new("")));
        body.push_widget(band(DockStyle::Top, 20.0), Box::new(CheckBox::new("Envoyer une copie à mon adresse").check(CheckState::Checked)));
        body.push_widget(band(DockStyle::Top, 12.0), Box::new(Label::new("")));
        body.push_widget(band(DockStyle::Top, 36.0), Box::new(filled_dropdown(&ROLES, 0)));
        body.push_widget(band(DockStyle::Top, 20.0), Box::new(Label::new("Rôle").role(Role::Body)));
        body.push_widget(band(DockStyle::Top, 12.0), Box::new(Label::new("")));
        let mut mail = OutlinedField::new("Adresse e-mail").with_leading("AtSign").required(true);
        mail.text = "nouveau.membre@exemple.fr".into();
        body.push_widget(band(DockStyle::Top, 48.0), Box::new(mail));
    }
    w.content_height = 40.0 + 12.0 + 20.0 + 12.0 + 36.0 + 20.0 + 12.0 + 48.0 + 40.0;
    let size = w.measure_at(c, host.right - host.left);
    let rect = place(host, size);
    w.close_hot = w.close_hit(rect, f.mouse.0, f.mouse.1);
    w.paint_modal(c, host, rect, WidgetState::REST);

    // A danger confirm naming a file whose name has no break opportunity.
    let host2 = Rect::new(a.left, host.bottom + m::GAP, a.right, host.bottom + m::GAP + 300.0);
    c.stroke_rounded(&host2, 6.0, &c.theme().border_strong);
    let d = ConfirmDialog::danger(
        "Supprimer « Compte-rendu-comité-de-pilotage-trimestriel-septembre-2026-v4-final.docx » ?",
        "Le fichier « Compte-rendu-comité-de-pilotage-trimestriel-septembre-2026-v4-final.docx » sera déplacé dans la corbeille.",
    )
    .labels("Supprimer", "Annuler");
    let s = d.measure_at(c, host2.right - host2.left);
    let r = place(host2, s);
    d.paint_modal(c, host2, r, WidgetState::REST);
    host2.bottom - a.top
}

// ─────────────────────────────────────────────────────────────────────────────
// Scene 6 — overflow, narrow containers, focus flush against clips, and one
// filter bar holding every field family
// ─────────────────────────────────────────────────────────────────────────────

const LONG_WORD: &str = "Anticonstitutionnellement-RapportFinalConsolidé2026";

fn scene_overflow(c: &dyn Canvas, _f: &Frame, a: Rect) -> f32 {
    let t = c.theme();
    let fm = c.formats();
    let mut y = note(c, a.left, a.top, a.right, "Cellules étroites (170 DIP) : libellés longs, mots insécables, grands nombres, désactivés");
    let n = 4usize;
    let w = (a.right - a.left - (n as f32 - 1.0) * m::ROW_GAP) / n as f32;
    let cell = |i: usize, top: f32, h: f32| {
        let x = a.left + i as f32 * (w + m::ROW_GAP);
        Rect::new(x, top, x + w, top + h)
    };

    // Row 1 — titles and labels that do not fit. The group's switch row wraps
    // its label into the width the switch leaves, so the row is as tall as
    // the wrapped label needs.
    let g = GroupBox::titled("Autorisations d'accès partagées aux invités externes").with_padding(Padding::all(8.0));
    let probe = Rect::new(0.0, 0.0, w, 1000.0);
    let gd = g.display_rect(probe);
    let sw_s = switch_size(c);
    let notify_lines = wrap_lines(
        "Notifier le propriétaire à chaque ouverture",
        (gd.right - gd.left) - sw_s.width - m::ROW_GAP,
        &mut |s| c.measure(s, &fm.body),
    );
    let notify_h = (notify_lines.len() as f32 * m::LABEL_H).max(sw_s.height);
    let h = (gd.top + 52.0 + notify_h + (probe.bottom - gd.bottom)).max(150.0);
    let r = cell(0, y, h);
    let card = Card::titled("Paramètres de synchronisation avancés du poste").with_subtitle("Dernière modification par l'administrateur principal");
    card.paint(c, r, WidgetState::REST);
    let b = card.body_rect(r);
    let btn = Button::new("Enregistrer les modifications").icon("Save");
    btn.paint(c, Rect::new(b.left, b.top, b.right, b.top + ButtonSize::Md.height()), WidgetState::REST);

    let r = cell(1, y, h);
    g.paint(c, r, WidgetState::REST);
    let d = g.display_rect(r);
    let d = Rect::new(r.left + d.left, r.top + d.top, r.left + d.right, r.top + d.bottom);
    CheckBox::new("Autoriser la modification des documents par les invités externes").check(CheckState::Checked).paint(c, Rect::new(d.left, d.top, d.right, d.top + 20.0), WidgetState::REST);
    RadioButton::new("Lecture seule pour tous les liens publics").selected(true).paint(c, Rect::new(d.left, d.top + 26.0, d.right, d.top + 46.0), WidgetState::REST);
    // Label on the left (wrapped), switch on the right, centred on the
    // label's first line: the two never share a pixel.
    let row_top = d.top + 52.0;
    let sw = Rect::new(d.right - sw_s.width, row_top + (m::LABEL_H - sw_s.height) / 2.0, d.right, row_top + (m::LABEL_H + sw_s.height) / 2.0);
    let label_right = sw.left - m::ROW_GAP;
    for (i, line) in notify_lines.iter().enumerate() {
        let ly = row_top + i as f32 * m::LABEL_H;
        c.text_ellipsis(line, &Rect::new(d.left, ly, label_right, ly + m::LABEL_H), &fm.body, &t.text_primary);
    }
    Switch::new().on(true).paint(c, sw, WidgetState::REST);

    let r = cell(2, y, h);
    let card = Card::new();
    card.paint(c, r, WidgetState::REST);
    let b = card.body_rect(r);
    let mut url = TextField::new();
    url.set_text("https://drive.kubuno.com/partage/0f9c2b7e4d1a8c3e5b6f7a9d0e1c2b3a");
    url.paint(c, Rect::new(b.left, b.top, b.right, b.top + 36.0), WidgetState::REST);
    OutlinedField::new("Adresse de facturation complète (rue, code postal, ville)").paint(c, Rect::new(b.left, b.top + 46.0, b.right, b.top + 94.0), WidgetState::REST);

    let r = cell(3, y, h);
    let card = Card::titled("Badges");
    card.paint(c, r, WidgetState::REST);
    let b = card.body_rect(r);
    // The short count keeps its width; the long pill is capped to what the
    // card has left, so it ellipsizes INSIDE its pill instead of running out.
    let count = Badge::new("99+").variant(BadgeVariant::Danger);
    let cs = count.measure(c);
    let long = Badge::new("Administratrice principale de l'espace").variant(BadgeVariant::Primary).max_width(b.right - b.left - cs.width - 6.0);
    let ls = long.measure(c);
    long.paint(c, Rect::new(b.left, b.top, b.left + ls.width, b.top + ls.height), WidgetState::REST);
    let x = b.left + ls.width + 6.0;
    count.paint(c, Rect::new(x, b.top, x + cs.width, b.top + cs.height), WidgetState::REST);
    Label::new(LONG_WORD).paint(c, Rect::new(b.left, b.top + 28.0, b.right, b.top + 48.0), WidgetState::REST);
    Badge::new("Hors ligne").size(BadgeSize::Sm).paint(c, Rect::new(b.left, b.top + 54.0, b.left + 70.0, b.top + 70.0), WidgetState::REST.disabled(true));
    y += h + m::GAP;

    // Row 2 — selects, numbers, tabs and a trail in narrow cells.
    // Two stacked fields per card: the cards are sized from the fields'
    // own heights, so nothing hangs below their borders.
    let cb = filled_combo(&["Téléchargements partagés de l'équipe documentation", "Documents"], 0);
    let dd = filled_dropdown(&[("Trier par date de dernière modification", Some("Clock"))], 0);
    let mut big = NumericField::ranged(0.0, 10_000_000_000.0);
    big.set_thousands_separator(true);
    let _ = big.set_value(1_234_567_890.0);
    let big_h = big.measure(c).height;
    let dp = DatePicker::short().on(TODAY);
    let card_a = Card::titled("Dossier cible");
    let card_b = Card::titled("Grands nombres");
    let h = card_height(&card_a, w, cb.field_height() + 8.0 + dd.height)
        .max(card_height(&card_b, w, big_h + 8.0 + dp.field_height()))
        .max(120.0);

    let r = cell(0, y, h);
    card_a.paint(c, r, WidgetState::REST);
    let b = card_a.body_rect(r);
    cb.paint_trigger(c, Rect::new(b.left, b.top, b.right, b.top + cb.field_height()), WidgetState::REST);
    let dy = b.top + cb.field_height() + 8.0;
    dd.paint(c, Rect::new(b.left, dy, b.right, dy + dd.height), WidgetState::REST);

    let r = cell(1, y, h);
    card_b.paint(c, r, WidgetState::REST);
    let b = card_b.body_rect(r);
    big.paint(c, Rect::new(b.left, b.top, b.right, b.top + big_h), WidgetState::REST);
    let dy = b.top + big_h + 8.0;
    dp.paint_field_only(c, Rect::new(b.left, dy, b.right, dy + dp.field_height()), WidgetState::REST.disabled(true));

    let r = cell(2, y, h);
    let card = Card::new();
    card.paint(c, r, WidgetState::REST);
    let b = card.body_rect(r);
    let mut tabs = Tabs::new().with("Général").with("Sécurité").with("Partage").with("Versions précédentes");
    tabs.selected_index = 3;
    let th = tabs.measure(c).height;
    tabs.paint_tabs(c, Rect::new(b.left, b.top, b.right, b.top + th), None);
    let trail = Breadcrumb::new().with("Documents").with("Contrats").with("2026").with("Septembre");
    let bh = trail.measure(c).height;
    trail.paint_segments(c, Rect::new(b.left, b.top + th + 8.0, b.right, b.top + th + 8.0 + bh), None, true);

    let r = cell(3, y, h);
    let callout = Callout::new(format!("Le fichier {LONG_WORD}.pdf est verrouillé."))
        .with_variant(CalloutVariant::Danger)
        .with_title("Conflit de version");
    let chh = callout.height_at(c, w);
    callout.paint(c, Rect::new(r.left, r.top, r.right, r.top + chh), WidgetState::REST);
    y += h.max(chh) + m::GAP;

    // Row 3 — list, menu row, toolbar and empty state in narrow cells. The
    // empty state's title wraps (its unbreakable word breaks between
    // characters) and the row grows to the height it needs at that width.
    let empty = EmptyState::new("Search", "Aucun résultat pour « Anticonstitutionnellement »").with_compact(true).with_description("Essayez un autre terme.");
    let empty_card = Card::new();
    let empty_h = card_height(&empty_card, w, empty.height_at(c, card_body_width(&empty_card, w)));
    let h = empty_h.max(150.0);
    let r = cell(0, y, h);
    let mut lb = ListBox::new();
    for s in ["Téléchargements partagés de l'équipe", "Documents", LONG_WORD, "Vidéos"] {
        lb.add_item(s);
    }
    lb.set_selected_index(2);
    lb.paint(c, Rect::new(r.left, r.top, r.right, r.top + lb.height_for_rows(4)), WidgetState::REST);

    let r = cell(1, y, h);
    let menu = Menu::with_items(vec![
        MenuEntry::new("Ouvrir dans une nouvelle fenêtre de l'explorateur").icon("ExternalLink").shortcut(true, false, true, "N").build(),
        MenuEntry::new("Renommer").icon("FileEdit").shortcut_text("F2").build(),
        separator(),
        MenuEntry::new(LONG_WORD).icon("Trash2").danger().build(),
    ]);
    let want = menu.measure(c);
    menu.paint(c, Rect::new(r.left, r.top, r.left + want.width.min(w), r.top + want.height), WidgetState::REST);

    let r = cell(2, y, h);
    let card = Card::new();
    card.paint(c, r, WidgetState::REST);
    let b = card.body_rect(r);
    let bar = Toolbar::new().with(menu_item("Plus", "Nouveau")).with(icon_item("Cut")).with(icon_item("Copy")).with(icon_label_item("Share2", "Partager"));
    bar.paint_items(c, Rect::new(b.left, b.top, b.right, b.top + bar.row_height), None, true);

    let r = cell(3, y, h);
    empty_card.paint(c, r, WidgetState::REST);
    let b = empty_card.body_rect(r);
    empty.paint(c, b, WidgetState::REST);
    y += h + m::GAP;

    // Row 4 — focus rings flush against a clipped container's edge.
    y = note(c, a.left, y, a.right, "Focus (clavier) au bord d'un conteneur découpé : l'anneau doit rester visible entier (web : ring hors de la boîte)");
    // The web's container keeps its rings whole with a `p-1` inset: the
    // content starts one ring reach inside the clip, never on it.
    let probe = Rect::new(0.0, 0.0, 1000.0, 1000.0);
    let sv = ScrollView::new().with_surface(Surface::Layer);
    let pv = sv.viewport(probe);
    let h = 36.0 + 2.0 * m::RING_INSET +(pv.top - probe.top) + (probe.bottom - pv.bottom);
    let frame = Rect::new(a.left, y, a.right, y + h);
    sv.paint(c, frame, WidgetState::REST);
    let vp = sv.viewport(frame);
    c.push_clip(&vp);
    let inner = Rect::new(vp.left + m::RING_INSET, vp.top + m::RING_INSET, vp.right - m::RING_INSET, vp.bottom - m::RING_INSET);
    let kb = WidgetState::REST.focused(true).focus_visible(true);
    let mut tf = TextField::new();
    tf.set_text("Champ au bord gauche");
    tf.paint(c, Rect::new(inner.left, inner.top, inner.left + 170.0, inner.top + 36.0), kb);
    let bt = Button::new("Bouton focus").variant(Variant::Secondary);
    let bw = bt.width(c);
    bt.paint(c, Rect::new(inner.left + 190.0, inner.top, inner.left + 190.0 + bw, inner.top + 36.0), kb);
    let chk = CheckBox::new("Case focus");
    let chk_w = chk.measure(c).width;
    let chk_x = inner.left + 190.0 + bw + 20.0;
    chk.paint(c, Rect::new(chk_x, inner.top, chk_x + chk_w, inner.top + 20.0), kb);
    let sw_x = chk_x + chk_w + 20.0;
    Switch::new().on(true).paint(c, Rect::new(sw_x, inner.top, sw_x + switch_size(c).width, inner.top + switch_size(c).height), kb);
    let sl_r = Rect::new(inner.left + 500.0, inner.bottom - 24.0, inner.right, inner.bottom);
    let mut sl = Slider::new();
    let _ = sl.set_value(10);
    sl.paint(c, sl_r, kb);
    c.pop_clip();
    y += h + m::GAP;

    // Row 5 — ONE filter bar holding every field family, each at its own
    // height, centred on the bar: the heights and the text baselines must agree.
    y = note(c, a.left, y, a.right, "Barre de filtres : chaque champ à la hauteur qu'il annonce, centré — hauteurs et lignes de base doivent coïncider");
    let bar = Rect::new(a.left, y, a.right, y + m::BAR_H);
    c.fill_rounded(&bar, 6.0, &t.surface_2);
    let mid = (bar.top + bar.bottom) / 2.0;
    let mut x = bar.left + 8.0;
    let mut heights: Vec<(&str, f32)> = Vec::new();
    let mut put = |c: &dyn Canvas, name: &'static str, wdt: f32, h: f32, paint: &dyn Fn(&dyn Canvas, Rect)| {
        let r = Rect::new(x, mid - h / 2.0, x + wdt, mid + h / 2.0);
        paint(c, r);
        heights.push((name, h));
        x += wdt + 6.0;
    };
    let mut tfield = TextField::new();
    tfield.set_text("Texte");
    put(c, "TextField", 70.0, tfield.measure(c).height, &|c, r| tfield.paint(c, r, WidgetState::REST));
    let mut sfield = SearchField::new();
    sfield.set_text("Rech.");
    put(c, "SearchField", 80.0, SearchField::HEIGHT, &|c, r| sfield.paint(c, r, WidgetState::REST));
    let combo = filled_combo(&["Combo"], 0);
    put(c, "ComboBox", 80.0, combo.field_height(), &|c, r| combo.paint(c, r, WidgetState::REST));
    let drop = filled_dropdown(&[("Dropdown", None)], 0);
    put(c, "Dropdown", 92.0, drop.height, &|c, r| drop.paint(c, r, WidgetState::REST));
    let num = NumericField::ranged(0.0, 100.0).with_value(42.0).unwrap_or_default();
    put(c, "NumericField", 60.0, num.measure(c).height, &|c, r| num.paint(c, r, WidgetState::REST));
    let date = DatePicker::short().on(TODAY);
    put(c, "DatePicker", date.measure(c).width, date.field_height(), &|c, r| date.paint(c, r, WidgetState::REST));
    let btn = Button::new("Bouton").variant(Variant::Secondary);
    put(c, "Button md", btn.width(c), ButtonSize::Md.height(), &|c, r| btn.paint(c, r, WidgetState::REST));
    // The bar's centre line and the body-text baseline band, drawn OVER the
    // controls: a control whose text sits off it shows at a glance.
    c.fill_rounded(&Rect::new(bar.left, mid, bar.right, mid + 1.0), 0.0, &t.danger);
    y += m::BAR_H + 6.0;
    let summary: Vec<String> = heights.iter().map(|(n, h)| format!("{n} {h:.0}")).collect();
    let line = format!("hauteurs annoncées (DIP) : {}", summary.join(" · "));
    c.text_ellipsis(&line, &Rect::new(a.left, y, a.right, y + 16.0), &fm.caption, &t.text_secondary);
    y += 16.0 + 4.0;
    let sizes = format!(
        "measure() : TextField {:.0}×{:.0} · ComboBox {:.0}×{:.0} · Dropdown {:.0}×{:.0} · NumericField {:.0}×{:.0} · DatePicker {:.0}×{:.0} · OutlinedField {:.0}×{:.0}",
        tfield.measure(c).width, tfield.measure(c).height,
        combo.measure(c).width, combo.measure(c).height,
        drop.measure(c).width, drop.measure(c).height,
        num.measure(c).width, num.measure(c).height,
        date.measure(c).width, date.measure(c).height,
        OutlinedField::new("x").measure(c).width, OutlinedField::new("x").measure(c).height,
    );
    c.text_ellipsis(&sizes, &Rect::new(a.left, y, a.right, y + 16.0), &fm.caption, &t.text_secondary);
    y += 16.0;
    y - a.top
}

// ═════════════════════════════════════════════════════════════════════════════
// The live column — a form inside a clipped, scrolled container, whose
// floating parts must escape it
// ═════════════════════════════════════════════════════════════════════════════

/// Which floating surface is open. One at a time, as on the web.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
enum Open {
    #[default]
    None,
    Combo,
    Role,
    Date,
    Menu,
    Help,
}

struct Ui {
    seeded: bool,
    prev_down: bool,
    /// Wheel scroll of the form container.
    scroll: f32,
    content_h: f32,
    /// The typed workspace name.
    name: String,
    lang: usize,
    role: usize,
    picker: DatePicker,
    quota: f64,
    notify: bool,
    guests: bool,
    open: Open,
    /// Last frame's geometry (client DIP) of each trigger, for click routing
    /// at the top of the next frame.
    combo_bounds: Rect,
    /// Where the open combo list / role list were placed last frame (client
    /// DIP), flipped or shifted to stay on the monitor.
    combo_panel: Rect,
    role_panel: Rect,
    role_bounds: Rect,
    date_bounds: Rect,
    menu_panel: Option<Rect>,
    menu_at: (f32, f32),
    help_place: Option<HelpPlacement>,
    last: String,
}

impl Default for Ui {
    fn default() -> Self {
        Ui {
            seeded: false,
            prev_down: false,
            scroll: 0.0,
            content_h: 0.0,
            name: "Équipe documentation".into(),
            lang: 0,
            role: 0,
            picker: DatePicker::short(),
            quota: 120.0,
            notify: true,
            guests: false,
            open: Open::None,
            combo_bounds: Rect::default(),
            combo_panel: Rect::default(),
            role_panel: Rect::default(),
            role_bounds: Rect::default(),
            date_bounds: Rect::default(),
            menu_panel: None,
            menu_at: (0.0, 0.0),
            help_place: None,
            last: String::new(),
        }
    }
}

thread_local! {
    static UI: RefCell<Ui> = RefCell::new(Ui::default());
}

fn space_menu() -> Menu {
    Menu::with_items(vec![
        MenuEntry::new("Dupliquer l'espace").icon("Copy").build(),
        MenuEntry::new("Exporter les réglages").icon("Download").shortcut(true, false, true, "E").build(),
        MenuEntry::new("Transférer la propriété…").icon("Users").build(),
        separator(),
        MenuEntry::new("Archiver l'espace").icon("Archive").build(),
        MenuEntry::new("Supprimer l'espace").icon("Trash2").danger().build(),
    ])
}

fn live_bubble() -> HelpBubble {
    HelpBubble::new("Le rôle attribué aux nouveaux membres invités par lien. Un administrateur peut le changer ensuite, membre par membre.")
        .title("Rôle par défaut")
}

/// The label a click on an enabled leaf of `menu` chooses.
fn menu_label(menu: &Menu, i: usize) -> Option<String> {
    match menu.items().get(i) {
        Some(StripItem::MenuItem(mi)) if mi.base.item.enabled && !mi.has_drop_down_items() => Some(mi.base.item.text.clone()),
        _ => None,
    }
}

/// Passes a control's focus state through, remembering the control's
/// rectangle when it has just gained focus (so the form can scroll to it).
fn track(reveal: &mut Option<Rect>, r: Rect, s: FocusState) -> FocusState {
    if s.gained {
        *reveal = Some(r);
    }
    s
}

/// `r` grown by `by` on every side.
fn inflate(r: Rect, by: f32) -> Rect {
    Rect::new(r.left - by, r.top - by, r.right + by, r.bottom + by)
}

fn rebase(r: Rect, o: Rect) -> Rect {
    Rect::new(r.left - o.left, r.top - o.top, r.right - o.left, r.bottom - o.top)
}

/// Routes a click made while a surface is open, against last frame's geometry.
/// The open surface swallows it, as the web's backdrop does.
fn route_open_click(ui: &mut Ui, c: &dyn Canvas, x: f32, y: f32) {
    match ui.open {
        Open::Combo => {
            let mut cb = filled_combo(&LANGUAGES, ui.lang);
            cb.open();
            if let Some(i) = cb.item_at_panel(ui.combo_panel, x, y) {
                ui.lang = i;
                ui.last = format!("Langue : {}", LANGUAGES[i]);
            }
            ui.open = Open::None;
        }
        Open::Role => {
            let mut d = filled_dropdown(&ROLES, ui.role);
            d.open = true;
            if let Some(i) = d.item_at_in(ui.role_panel, x, y) {
                ui.role = i;
                ui.last = format!("Rôle : {}", ROLES[i].0);
            }
            ui.open = Open::None;
        }
        Open::Date => {
            let panel = ui.picker.drop_down_rect(ui.date_bounds);
            if panel.contains(x, y) {
                match ui.picker.calendar.header_at(panel, x, y) {
                    Some(HeaderPart::Prev) => ui.picker.calendar.prev_month(),
                    Some(HeaderPart::Next) => ui.picker.calendar.next_month(),
                    Some(HeaderPart::Title) => {}
                    None => {
                        if let Some(d) = ui.picker.day_at(ui.date_bounds, x, y) {
                            ui.picker.pick(d);
                            ui.last = format!("Expiration : {:02}/{:02}/{}", d.day, d.month, d.year);
                            ui.open = Open::None;
                        }
                    }
                }
            } else {
                ui.picker.close_panel();
                ui.open = Open::None;
            }
        }
        Open::Menu => {
            let menu = space_menu();
            if let Some(p) = ui.menu_panel {
                if p.contains(x, y) {
                    if let Some(label) = menu.item_at(p, x, y).and_then(|i| menu_label(&menu, i)) {
                        ui.last = format!("Menu : {label}");
                        ui.open = Open::None;
                    }
                    return;
                }
            }
            ui.open = Open::None;
        }
        Open::Help => {
            if let Some(p) = ui.help_place {
                match live_bubble().part_at(c, &p, x, y) {
                    HelpPart::Bubble | HelpPart::Action => {}
                    _ => ui.open = Open::None,
                }
            } else {
                ui.open = Open::None;
            }
        }
        Open::None => {}
    }
    if ui.open != Open::Date {
        ui.picker.close_panel();
    }
}

pub fn interactive_column(c: &dyn Canvas, f: &Frame) {
    UI.with(|ui| {
        let mut ui = ui.borrow_mut();
        if !ui.seeded {
            ui.picker = DatePicker::short().on(TODAY);
            ui.seeded = true;
        }
        // The calendar places itself on the monitor (flips above the field
        // when there is no room below), for painting and hit-testing alike.
        ui.picker.viewport = Some(f.screen_area());
        let mut live = Live::new(f, ui.prev_down);
        ui.prev_down = f.mouse_down;
        let t = c.theme();
        let fm = c.formats();

        // ── An open surface takes the input first ─────────────────────────────
        if f.dismiss {
            ui.open = Open::None;
            ui.picker.close_panel();
        }
        if ui.open != Open::None {
            if live.take_escape() {
                ui.open = Open::None;
                ui.picker.close_panel();
            } else if live.clicked {
                route_open_click(&mut ui, c, f.mouse.0, f.mouse.1);
                live.clicked = false;
            }
            // Keyboard inside the open list: arrows move, Enter closes.
            match ui.open {
                Open::Combo => {
                    if live.take_key(vk::DOWN, Modifiers::NONE) {
                        ui.lang = (ui.lang + 1).min(LANGUAGES.len() - 1);
                    }
                    if live.take_key(vk::UP, Modifiers::NONE) {
                        ui.lang = ui.lang.saturating_sub(1);
                    }
                    if live.take_key(vk::ENTER, Modifiers::NONE) {
                        ui.open = Open::None;
                    }
                }
                Open::Role => {
                    if live.take_key(vk::DOWN, Modifiers::NONE) {
                        ui.role = (ui.role + 1).min(ROLES.len() - 1);
                    }
                    if live.take_key(vk::UP, Modifiers::NONE) {
                        ui.role = ui.role.saturating_sub(1);
                    }
                    if live.take_key(vk::ENTER, Modifiers::NONE) {
                        ui.open = Open::None;
                    }
                }
                _ => {}
            }
            if ui.open != Open::None {
                live.mouse = (host::POINTER_AWAY, host::POINTER_AWAY);
            }
        }
        let (mx, my) = live.mouse;

        let (left, y0, right) = interact::panel(c, interact::panel_rect(f.size));
        let col_bottom = interact::panel_rect(f.size).bottom - 16.0;
        let y = interact::caption(c, left, right, y0, "Formulaire dans une ScrollView découpée (molette) — les listes doivent en sortir");

        // ── The scroll container ──────────────────────────────────────────────
        let status_h = 40.0;
        // Shorter than its content on purpose, so it scrolls and its floating
        // parts open near (and past) its clipped edges.
        let frame = Rect::new(left, y, right, (col_bottom - status_h).min(y + m::LIVE_VIEW_H).max(y + 200.0));
        let mut view = ScrollView::new().with_surface(Surface::Layer).fixed(Rect::new(0.0, 0.0, 1.0, ui.content_h.max(1.0)));
        let (_, dy) = live.wheel_over(frame);
        ui.scroll += dy;
        let reach = view.max_offset(frame, Axis::Vertical);
        ui.scroll = ui.scroll.clamp(0.0, reach);
        view.scroll_to(frame, 0.0, ui.scroll);
        view.paint(c, frame, WidgetState::REST);
        let vp = view.viewport(frame);
        let inside = |x: f32, y: f32| vp.contains(x, y);
        // Hit-testing inside the container only counts where it is visible.
        let hm = if inside(mx, my) { (mx, my) } else { (host::POINTER_AWAY, host::POINTER_AWAY) };
        let hit = |r: Rect| live.clicked && r.contains(hm.0, hm.1);
        let hover = |r: Rect| r.contains(hm.0, hm.1);
        let st = |r: Rect| WidgetState::REST.hot(hover(r)).pressed(hover(r) && live.down);
        let clip_to_vp = |r: Rect| Rect::new(r.left.max(vp.left), r.top.max(vp.top), r.right.min(vp.right), r.bottom.min(vp.bottom));
        // Every control registers with the focus ring, scrolled out or not
        // (its hit rectangle is clipped to the viewport), so Tab walks the
        // whole form; the one that just gained focus is scrolled into view.
        let mut reveal: Option<Rect> = None;

        c.push_clip(&vp);
        let content_top = vp.top - ui.scroll;
        let card = Card::titled("Espace de travail").with_subtitle("Réglages partagés par tous les membres");
        let card_rect = Rect::new(vp.left + 8.0, content_top + 8.0, vp.right - 14.0, content_top + 8.0 + ui.content_h.max(200.0) - 16.0);
        card.paint(c, card_rect, WidgetState::REST);
        let b = card.body_rect(card_rect);
        let mut yy = b.top;

        // Name — a real text field: click or Tab to focus, type, Backspace, Ctrl+V.
        let ctl = form_label(c, b.left, yy, b.right, "Nom de l'espace");
        let name_r = Rect::new(b.left, ctl, b.right, ctl + 36.0);
        let fs = track(&mut reveal, name_r, live.focus_with("c-name", clip_to_vp(name_r), FocusOpts::TEXT));
        if hover(name_r) {
            host::set_cursor(Cursor::IBeam);
        }
        if fs.focused {
            let typed = live.take_text();
            ui.name.push_str(&typed);
            if live.take_key(vk::BACK, Modifiers::NONE) {
                ui.name.pop();
            }
            if live.take_key(vk::letter('V'), Modifiers::CTRL) {
                if let Some(s) = host::clipboard_text() {
                    ui.name.push_str(&s.replace(['\r', '\n'], " "));
                }
            }
        }
        let mut name = TextField::new();
        name.set_text(&ui.name);
        name.placeholder_text = "Sans titre".into();
        if fs.focused {
            let n = name.text().chars().count() as i32;
            name.select(n, 0);
        }
        name.paint(c, name_r, fs.apply(st(name_r)));
        yy = name_r.bottom + m::ROW_GAP;

        // Language — ComboBox: its list must leave the container.
        let ctl = form_label(c, b.left, yy, b.right, "Langue de l'interface");
        let combo = filled_combo(&LANGUAGES, ui.lang);
        let combo_r = Rect::new(b.left, ctl, b.right, ctl + combo.field_height());
        let cf = track(&mut reveal, combo_r, live.focus("c-lang", clip_to_vp(combo_r)));
        if ui.open == Open::None && (hit(combo_r) || (cf.focused && (live.take_key(vk::SPACE, Modifiers::NONE) || live.take_key(vk::DOWN, Modifiers::ALT)))) {
            ui.open = Open::Combo;
        }
        combo.paint_trigger(c, combo_r, cf.apply(st(combo_r)).focused(cf.focused || ui.open == Open::Combo));
        ui.combo_bounds = combo_r;
        yy = combo_r.bottom + m::ROW_GAP;

        // Default role — label + « ? » (bubble) + Dropdown.
        let q = HelpButton::new().open(ui.open == Open::Help);
        let (ctl, qr) = form_label_help(c, b.left, yy, "Rôle par défaut", &q, WidgetState::REST);
        if ui.open == Open::None && hit(qr) {
            ui.open = Open::Help;
        }
        q.paint(c, qr, WidgetState::REST.hot(hover(qr)));
        let role = filled_dropdown(&ROLES, ui.role);
        let role_r = Rect::new(b.left, ctl, b.right, ctl + role.height);
        let rf = track(&mut reveal, role_r, live.focus("c-role", clip_to_vp(role_r)));
        if ui.open == Open::None && (hit(role.trigger_rect(role_r)) || (rf.focused && live.take_key(vk::SPACE, Modifiers::NONE))) {
            ui.open = Open::Role;
        }
        role.paint(c, role_r, rf.apply(st(role_r)));
        ui.role_bounds = role_r;
        yy = role_r.bottom + m::ROW_GAP;

        // Expiry — DatePicker: its calendar must leave the container.
        let ctl = form_label(c, b.left, yy, b.right, "Expiration des invitations");
        let dw = ui.picker.measure(c).width;
        let date_r = Rect::new(b.left, ctl, b.left + dw, ctl + ui.picker.field_height());
        let df = track(&mut reveal, date_r, live.focus("c-date", clip_to_vp(date_r)));
        if ui.open == Open::None && (hit(date_r) || (df.focused && live.take_key(vk::SPACE, Modifiers::NONE))) {
            ui.open = Open::Date;
            ui.picker.open_panel();
        }
        ui.picker.paint_field_only(c, date_r, df.apply(st(date_r)).focused(df.focused || ui.open == Open::Date));
        ui.date_bounds = date_r;
        yy = date_r.bottom + m::ROW_GAP;

        // Quota — NumericField + Slider on one line; arrows step when focused.
        let ctl = form_label(c, b.left, yy, b.right, "Quota par membre (Go)");
        let mut qf = NumericField::ranged(0.0, 500.0);
        let _ = qf.set_value(ui.quota);
        let qh = qf.measure(c).height;
        let num_r = Rect::new(b.left, ctl, b.left + 100.0, ctl + qh);
        let nf = track(&mut reveal, num_r, live.focus_with("c-quota", clip_to_vp(num_r), FocusOpts::TEXT));
        if nf.focused {
            if live.take_key(vk::UP, Modifiers::NONE) {
                ui.quota = (ui.quota + 10.0).min(500.0);
            }
            if live.take_key(vk::DOWN, Modifiers::NONE) {
                ui.quota = (ui.quota - 10.0).max(0.0);
            }
        }
        let _ = qf.set_value(ui.quota);
        qf.paint(c, num_r, nf.apply(st(num_r)));
        let mut sl = Slider::new();
        sl.set_maximum(500);
        let _ = sl.set_value(ui.quota as i32);
        let slh = sl.measure(c).height;
        let sl_r = centred(num_r, num_r.right + m::ROW_GAP, b.right, slh);
        if live.down && hover(sl_r) {
            ui.quota = sl.value_at(sl_r, hm.0, hm.1) as f64;
            let _ = sl.set_value(ui.quota as i32);
        }
        let sf = track(&mut reveal, sl_r, live.focus("c-slider", clip_to_vp(sl_r)));
        sl.paint(c, sl_r, sf.apply(st(sl_r)));
        yy = num_r.bottom + m::ROW_GAP;

        // A switch row and a check box.
        let row = Rect::new(b.left, yy, b.right, yy + m::SETTING_H);
        let sw_r = Rect::new(row.right - switch_size(c).width, (row.top + row.bottom - switch_size(c).height) / 2.0, row.right, (row.top + row.bottom + switch_size(c).height) / 2.0);
        let swf = track(&mut reveal, sw_r, live.focus("c-notify", clip_to_vp(sw_r)));
        if hit(row) || (swf.focused && live.take_key(vk::SPACE, Modifiers::NONE)) {
            ui.notify = !ui.notify;
        }
        setting_row(c, row, "Notifications", "Prévenir les membres des changements", ui.notify, swf.apply(st(sw_r)));
        yy = row.bottom + 8.0;
        let cb_r = Rect::new(b.left, yy, b.right, yy + 20.0);
        let cbf = track(&mut reveal, cb_r, live.focus("c-guests", clip_to_vp(cb_r)));
        if hit(cb_r) || (cbf.focused && live.take_key(vk::SPACE, Modifiers::NONE)) {
            ui.guests = !ui.guests;
        }
        let chk = if ui.guests { CheckState::Checked } else { CheckState::Unchecked };
        CheckBox::new("Autoriser les invités externes à commenter").check(chk).paint(c, cb_r, cbf.apply(st(cb_r)));
        yy = cb_r.bottom + m::ROW_GAP;

        // An actions row: an icon button opening a menu (and a tooltip).
        let more = IconButton::plain("MoreHorizontal", 32.0, 16.0);
        let more_r = Rect::new(b.left, yy, b.left + 32.0, yy + 32.0);
        let mf = track(&mut reveal, more_r, live.focus("c-more", clip_to_vp(more_r)));
        if ui.open == Open::None && (hit(more_r) || (mf.focused && live.take_key(vk::ENTER, Modifiers::NONE))) {
            ui.open = Open::Menu;
            ui.menu_at = (more_r.left, more_r.bottom + m::ANCHOR_GAP);
        }
        more.paint(c, more_r, mf.apply(st(more_r)).pressed(ui.open == Open::Menu));
        c.text("Actions sur l'espace", &Rect::new(more_r.right + 8.0, more_r.top, b.right, more_r.bottom), &fm.body, &t.text_secondary, false);
        let more_hovered = hover(more_r);
        yy = more_r.bottom + m::GAP;

        // Footer.
        // Both rectangles first, then registered in READING order (Annuler,
        // then Enregistrer), so Tab walks them left to right like the web.
        let save = Button::new("Enregistrer").variant(Variant::Primary);
        let save_r = save.rect_ending_at(c, b.right, yy);
        let cancel = Button::new("Annuler").variant(Variant::Secondary);
        let cancel_r = cancel.rect_ending_at(c, save_r.left - 8.0, yy);
        let cvf = track(&mut reveal, cancel_r, live.focus("c-cancel", clip_to_vp(cancel_r))).apply(WidgetState::REST);
        cancel.paint(c, cancel_r, cvf.hot(hover(cancel_r)).pressed(hover(cancel_r) && live.down));
        let svf = track(&mut reveal, save_r, live.focus("c-save", clip_to_vp(save_r))).apply(WidgetState::REST);
        if hit(save_r) || (svf.focused && live.take_key(vk::ENTER, Modifiers::NONE)) {
            ui.last = "Enregistré".into();
        }
        save.paint(c, save_r, svf.hot(hover(save_r)).pressed(hover(save_r) && live.down));
        yy = save_r.bottom + 16.0 + 8.0;
        c.pop_clip();
        let new_h = yy - content_top;
        if (new_h - ui.content_h).abs() > 0.5 {
            // The card is sized from this height: paint once more with it.
            host::request_repaint_after(1);
        }
        ui.content_h = new_h;
        // `scrollIntoView({ block: 'nearest' })` for a control Tab reached
        // outside the viewport, keeping its focus ring clear of the clip.
        if let Some(r) = reveal {
            let margin = m::RING_INSET + 4.0;
            let before = ui.scroll;
            if r.top < vp.top + margin {
                ui.scroll -= vp.top + margin - r.top;
            } else if r.bottom > vp.bottom - margin {
                ui.scroll += r.bottom - (vp.bottom - margin);
            }
            ui.scroll = ui.scroll.clamp(0.0, reach);
            if (ui.scroll - before).abs() > 0.5 {
                host::request_repaint_after(1);
            }
        }

        // The container's own scroll bar: a thin thumb in its track.
        let range = view.range(frame, Axis::Vertical);
        if range.visible {
            let track = view.track(frame, Axis::Vertical);
            let th = (track.bottom - track.top) * (range.large_change / (range.maximum - range.minimum + 1.0).max(1.0));
            let th = th.clamp(20.0, track.bottom - track.top);
            let ty = track.top + (track.bottom - track.top - th) * if reach > 0.0 { ui.scroll / reach } else { 0.0 };
            c.fill_rounded(&Rect::new(track.right - 4.0, ty, track.right - 1.0, ty + th), 1.5, &t.border_strong);
        }

        // ── The status line under the container ───────────────────────────────
        let status = if ui.last.is_empty() { format!("Ouvert : {:?}", ui.open) } else { format!("{} · ouvert : {:?}", ui.last, ui.open) };
        c.text_ellipsis(&status, &Rect::new(left, frame.bottom + 8.0, right, frame.bottom + 24.0), &fm.caption, &t.text_secondary);

        // ── The floating surfaces, each in a popup of its own ─────────────────
        let area = f.screen_area();
        let (px, py) = f.mouse;
        match ui.open {
            Open::Combo => {
                // Only the LIST floats: the trigger stays painted in the page
                // (`paint_trigger` above). The list is placed on the monitor
                // (flipped above / pulled in when it would leave it).
                let mut cb = filled_combo(&LANGUAGES, ui.lang);
                cb.open();
                let panel = cb.drop_down_rect_in(ui.combo_bounds, area);
                cb.hot_index = cb.item_at_panel(panel, px, py);
                ui.combo_panel = panel;
                let pb = inflate(panel, FLOAT_SHADOW_MARGIN);
                let local = rebase(panel, pb);
                interact::with_focus(|r| r.keep_focus_in(pb));
                host::popup(pb, move |canvas| cb.paint_drop_down_at(canvas, local));
            }
            Open::Role => {
                let mut d = filled_dropdown(&ROLES, ui.role);
                d.open = true;
                let bounds = ui.role_bounds;
                d.place_drop_down(c, bounds, area);
                let panel = d.drop_down_rect(bounds);
                d.hot_index = d.item_at_in(panel, px, py);
                ui.role_panel = panel;
                let pb = d.drop_down_paint_bounds(bounds);
                let local = rebase(panel, pb);
                interact::with_focus(|r| r.keep_focus_in(pb));
                host::popup(pb, move |canvas| d.paint_drop_down_at(canvas, local));
            }
            Open::Date => {
                // The field is painted in the page (`paint_field_only`); the
                // calendar floats in its own popup, shadow included.
                let mut p = ui.picker.clone();
                let bounds = ui.date_bounds;
                let panel = p.drop_down_rect(bounds);
                p.calendar.hot_day = p.day_at(bounds, px, py).filter(|d| p.calendar.is_selectable(*d));
                p.calendar.hot_header = p.calendar.header_at(panel, px, py);
                if let Some(pb) = p.popup_drop_down(bounds) {
                    interact::with_focus(|r| r.keep_focus_in(pb));
                }
            }
            Open::Menu => {
                let mut menu = space_menu();
                let want = menu.measure(c);
                let (ax, ay) = ui.menu_at;
                let x = ax.min(area.right - m::EDGE - want.width).max(area.left + m::EDGE);
                let top = if ay + want.height > area.bottom - m::EDGE { (ay - m::ANCHOR_GAP - 32.0 - m::ANCHOR_GAP - want.height).max(area.top + m::EDGE) } else { ay };
                let panel = Rect::new(x, top, x + want.width, top + want.height);
                menu.hot_index = menu.item_at(panel, px, py);
                ui.menu_panel = Some(panel);
                // The menu says what it paints (panel, open submenu, shadow).
                let pb = menu.paint_bounds(c, panel);
                let local = rebase(panel, pb);
                interact::with_focus(|r| r.keep_focus_in(pb));
                host::popup(pb, move |canvas| menu.paint(canvas, local, WidgetState::REST));
            }
            Open::Help => {
                let b = live_bubble();
                let anchor = qr;
                let p = b.place(c, anchor, area);
                ui.help_place = Some(p);
                let hot = b.part_at(c, &p, px, py);
                let pb = p.paint_bounds();
                let local = p.offset(-pb.left, -pb.top);
                host::popup(pb, move |canvas| b.paint_placed(canvas, &local, Some(hot)));
            }
            Open::None => {
                ui.menu_panel = None;
                ui.help_place = None;
                // The tooltip: pointer passes through, may overhang the window.
                if more_hovered {
                    let tip = Tooltip::new("Plus d'actions").side(Side::Bottom);
                    let size = tip.measure(c);
                    let rel = Rect::new(more_r.left - area.left, more_r.top - area.top, more_r.right - area.left, more_r.bottom - area.top);
                    let placed = kubuno_ui::display::place(rel, size, Side::Bottom, Size::new(area.right - area.left, area.bottom - area.top));
                    let rect = Rect::new(placed.rect.left + area.left, placed.rect.top + area.top, placed.rect.right + area.left, placed.rect.bottom + area.top);
                    let pb = Rect::new(rect.left - m::SHADOW, rect.top - m::SHADOW, rect.right + m::SHADOW, rect.bottom + m::SHADOW);
                    let local = kubuno_ui::display::Placement {
                        rect: rebase(rect, pb),
                        side: placed.side,
                        tip: (placed.tip.0 + area.left - pb.left, placed.tip.1 + area.top - pb.top),
                    };
                    host::overlay(pb, move |canvas| tip.paint_placed(canvas, &local, WidgetState::REST));
                }
            }
        }
        if ui.open != Open::None {
            host::request_repaint_after(250);
        }
    });
}
