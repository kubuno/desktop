//! Gallery page — **display**: the five type roles, the nine content
//! alignments on one rectangle, links in their states, every badge variant, a
//! grid of icons (and the missing-glyph box), text overflow in narrow cards,
//! tooltips placed on the four sides and wrapped at their max width, and both
//! separators.
//!
//! There is no predecessor to pair against here (nothing in
//! `drive-app-controls` draws a badge, a tooltip or a rule), so the page is a
//! variant × state matrix rather than an old/new diff. The alignment block is
//! the one worth staring at: the same rectangle, nine times, is what makes a
//! mistake in the vertical thirds impossible to miss.
//!
//! The static exposition is laid out in two columns so it fits the pane left
//! of the interactive column without scrolling — the host has no scroll view,
//! so a section that overflows is a section nobody checks.

use std::cell::RefCell;

use kubuno_controls::enums::{BorderStyle, Padding};
use kubuno_controls::host::{self, vk, Cursor, Frame, Modifiers};
use kubuno_controls::labels::Link;
use kubuno_ui::display::{place, BadgeSize, BadgeVariant, Role, Side, TextOverflow, BADGE_VARIANTS, ROLES, SIDES};
use kubuno_ui::display::{Badge, Icon, Label, LinkLabel, Separator, Tooltip, TooltipTrigger};
use kubuno_ui::metrics::space;
use kubuno_ui::{Canvas, ContentAlignment, Rect, Size, Widget, WidgetState};

use super::interact::{self, Live};
use super::sheet::{Page, MARGIN};

/// The nine cells, in the toolkit's own order — three rows of three.
const NINE: [(ContentAlignment, &str); 9] = [
    (ContentAlignment::TopLeft, "TopLeft"),
    (ContentAlignment::TopCenter, "TopCenter"),
    (ContentAlignment::TopRight, "TopRight"),
    (ContentAlignment::MiddleLeft, "MiddleLeft"),
    (ContentAlignment::MiddleCenter, "MiddleCenter"),
    (ContentAlignment::MiddleRight, "MiddleRight"),
    (ContentAlignment::BottomLeft, "BottomLeft"),
    (ContentAlignment::BottomCenter, "BottomCenter"),
    (ContentAlignment::BottomRight, "BottomRight"),
];

/// Names that exist in `lucide-icons.txt` / `themed-icons.txt`, then one that
/// does not (`Save`) to show the missing-glyph box, and one lucide rename
/// (`CircleAlert` → `AlertCircle`) that resolves through the alias table.
const ICONS: [&str; 14] = [
    "Check",
    "ChevronDown",
    "ChevronRight",
    "Plus",
    "Minus",
    "X",
    "Star",
    "Search",
    "Bell",
    "Trash2",
    "FolderOpen",
    "Users",
    "CircleAlert",
    "Save",
];

/// A long, unbreakable-looking sentence the overflow demos share.
const LONG: &str = "Partage de liens publics — les destinataires ouvrent le dossier sans compte Kubuno";

/// The multi-line tooltip text: long enough to wrap at `maxWidth: 280`, with an
/// explicit break (`pre-line`).
const LONG_TIP: &str = "Renommer l'élément sélectionné (F2).\nLe nouveau nom est vérifié avant d'être appliqué : \
                        les caractères interdits par le système de fichiers sont refusés.";

// ── Page-local layout metrics ────────────────────────────────────────────────

mod page_metrics {
    /// Where the right-hand column of the exposition starts.
    pub const COL2: f32 = 430.0;
    /// One alignment cell.
    pub const ALIGN_W: f32 = 118.0;
    pub const ALIGN_H: f32 = 32.0;
    /// One link-state cell.
    pub const LINK_W: f32 = 104.0;
    /// One overflow card.
    pub const CARD_W: f32 = 150.0;
    pub const CARD_H: f32 = 64.0;
    /// The icon grid's cell.
    pub const ICON_CELL: f32 = 28.0;
    /// The interactive overflow card's width range, driven by the wheel.
    pub const LIVE_CARD_MIN: f32 = 90.0;
    pub const LIVE_CARD_MAX: f32 = 380.0;
    pub const LIVE_CARD_START: f32 = 190.0;
    /// Width change per wheel notch on the live card (a notch is 100 DIP of
    /// wheel travel; a fifth of it is a comfortable resize step).
    pub const LIVE_CARD_STEP: f32 = 0.2;
    /// The popup margin around a floating bubble: covers its shadow
    /// (`SHADOW_MENU` reaches ~7 DIP) and the arrow.
    pub const TIP_MARGIN: f32 = 10.0;
}
use page_metrics as pm;

pub fn draw(c: &dyn Canvas, f: &Frame) {
    // The static exposition takes the window minus the interactive column, so it
    // lays out to the left of it rather than under it.
    let mut p = Page::new(c, f.size.0 - interact::PANEL_W(), f.size.1);

    let top = p.y;
    roles(&mut p);
    let after_roles = p.y;
    // The overflow cards sit beside the roles, in the second column.
    p.y = top;
    overflow(&mut p);
    p.y = p.y.max(after_roles);

    let top = p.y;
    alignments(&mut p);
    let after_align = p.y;
    p.y = top;
    tooltips(&mut p);
    p.y = p.y.max(after_align);

    links(&mut p);
    badges(&mut p);
    icons_and_separators(&mut p);
}

// ── Interactive column ───────────────────────────────────────────────────────

/// What the interactive column remembers between frames.
struct Ui {
    prev_down: bool,
    /// Per link of the live LinkLabel: followed yet.
    visited: [bool; 2],
    /// What the last activation was, for the status line.
    last: &'static str,
    /// The two tooltip triggers' timing state.
    tips: [TooltipTrigger; 2],
    /// The bubble to emit in `overlay` this frame: text and pointer anchor.
    pending_tip: Option<(&'static str, (f32, f32))>,
    /// The live overflow card's width.
    card_w: f32,
}

impl Default for Ui {
    fn default() -> Self {
        Self {
            prev_down: false,
            visited: [false; 2],
            last: "aucun",
            tips: [TooltipTrigger::new(); 2],
            pending_tip: None,
            card_w: pm::LIVE_CARD_START,
        }
    }
}

thread_local! {
    static UI: RefCell<Ui> = RefCell::new(Ui::default());
}

/// The right-hand column: a two-link [`LinkLabel`] (per-link hover, Hand
/// cursor, click or Tab + Enter to follow), two [`Tooltip`] triggers with the
/// web's 400 ms delay (a short one, and a long one that wraps at 280), and a
/// card whose width the wheel changes, to watch [`Label`] ellipsize and wrap
/// and a [`Badge`] truncate inside its pill.
pub fn interactive_column(c: &dyn Canvas, f: &Frame) {
    UI.with(|ui| {
        let mut ui = ui.borrow_mut();
        let live = Live::new(f, ui.prev_down);
        ui.prev_down = f.mouse_down;
        let t = c.theme();

        let (left, mut y, right) = interact::panel(c, interact::panel_rect(f.size));

        // ── LinkLabel — two links, each its own tab stop. ──
        y = interact::caption(c, left, right, y, "LinkLabel — survol par lien, clic ou Tab + Entrée");
        let text = "Lire les conditions d'utilisation et la confidentialité";
        let spans = [("conditions d'utilisation", "conditions"), ("confidentialité", "confidentialité")];
        let mut link = LinkLabel::new(text);
        link.links = spans
            .iter()
            .enumerate()
            .map(|(i, (needle, _))| {
                let start = char_index(text, needle);
                let mut l = Link::new(start as i32, needle.chars().count() as i32);
                l.visited = ui.visited[i];
                l
            })
            .collect();
        let want = link.measure(c);
        let link_rect = Rect::new(left, y, (left + want.width).min(right), y + want.height);
        let rects = link.link_rects(c, link_rect);
        let hot = link.link_at(c, link_rect, live.mouse.0, live.mouse.1);
        if hot.is_some() {
            host::set_cursor(Cursor::Hand);
        }
        let mut focused = None;
        let mut focus_visible = false;
        for (i, r) in rects.iter().enumerate() {
            let st = live.focus(("display-link", i), *r);
            if st.focused {
                focused = Some(i);
                focus_visible = st.visible;
            }
            // ARIA link: Enter follows it (Space does not — that is a button).
            let by_key = st.focused && live.take_key(vk::ENTER, Modifiers::NONE);
            if live.hit(*r) || by_key {
                ui.visited[i] = true;
                ui.last = spans[i].1;
            }
        }
        let state = live
            .state(link_rect)
            .focused(focused.is_some())
            .focus_visible(focus_visible);
        link.hot_link(hot).focused_link(focused).paint(c, link_rect, state);
        y += want.height + 2.0;
        let status = format!("dernier lien suivi : {}", ui.last);
        Label::new(status)
            .role(Role::Meta)
            .paint(c, Rect::new(left, y, right, y + 16.0), WidgetState::REST);
        y += 16.0 + space::MD;

        // ── Tooltips — the web's delay, pointer-anchored, multi-line. ──
        y = interact::caption(c, left, right, y, "Tooltip — 400 ms de délai, Échap ou clic le masque");
        let targets = [
            ("Info courte", "Renommer — F2"),
            ("Info longue", LONG_TIP),
        ];
        let now = host::now_ms();
        ui.pending_tip = None;
        let escape = ui.tips.iter().any(TooltipTrigger::is_visible) && live.take_escape();
        let mut x = left;
        for (i, (label, tip)) in targets.iter().enumerate() {
            let r = Rect::new(x, y, x + 130.0, y + 32.0);
            let hovering = live.hover(r);
            if escape || f.dismiss {
                ui.tips[i].dismiss();
            }
            let tick = ui.tips[i].update(hovering, live.mouse, f.mouse_down, now, Tooltip::DELAY_MS);
            if let Some(ms) = tick.repaint_in_ms {
                host::request_repaint_after(ms);
            }
            if let Some(at) = tick.show_at {
                ui.pending_tip = Some((*tip, at));
            }
            c.fill_rounded(&r, 6.0, &t.card_background);
            c.stroke_rounded(&r, 6.0, if hovering { &t.accent } else { &t.card_stroke });
            Label::new(*label)
                .align(ContentAlignment::MiddleCenter)
                .paint(c, r, WidgetState::REST);
            x += 130.0 + space::SM;
        }
        y += 32.0 + space::MD;

        // ── Overflow — the wheel over the card changes its width. ──
        y = interact::caption(c, left, right, y, "Débordement — molette sur la carte = largeur");
        let card_h = 20.0 + 40.0 + 20.0 + space::SM * 4.0;
        let probe = Rect::new(left, y, right, y + card_h);
        let (_, dy) = live.wheel_over(probe);
        ui.card_w = (ui.card_w - dy * pm::LIVE_CARD_STEP)
            .clamp(pm::LIVE_CARD_MIN, pm::LIVE_CARD_MAX.min(right - left));
        let card = Rect::new(left, y, left + ui.card_w, y + card_h);
        c.fill_rounded(&card, 8.0, &t.card_background);
        c.stroke_rounded(&card, 8.0, &t.card_stroke);
        let inner_l = card.left + space::SM;
        let inner_r = card.right - space::SM;
        let mut cy = card.top + space::SM;
        Label::new(LONG).paint(c, Rect::new(inner_l, cy, inner_r, cy + 20.0), WidgetState::REST);
        cy += 20.0 + space::SM;
        Label::new(LONG)
            .wrap()
            .paint(c, Rect::new(inner_l, cy, inner_r, cy + 40.0), WidgetState::REST);
        cy += 40.0 + space::SM;
        let badge = Badge::new("Partagé avec 12 personnes").variant(BadgeVariant::Primary).dot(true);
        let bw = badge.measure(c).width.min(inner_r - inner_l);
        badge.paint(c, Rect::new(inner_l, cy, inner_l + bw, cy + 20.0), WidgetState::REST);
        y = card.bottom + 4.0;
        Label::new(format!("largeur : {:.0} DIP", ui.card_w))
            .role(Role::Meta)
            .paint(c, Rect::new(left, y, right, y + 16.0), WidgetState::REST);
        y += 16.0 + space::MD;

        // ── Icons — a known glyph, a lucide rename, a missing one. ──
        y = interact::caption(c, left, right, y, "Icon — connue · alias lucide · absente (boîte)");
        let mut x = left;
        for (name, tint) in [("Star", Some(t.accent)), ("CircleAlert", None), ("Save", None)] {
            let icon = Icon::sized(name, 24.0).tint(tint);
            icon.paint(c, Rect::new(x, y, x + 24.0, y + 24.0), WidgetState::REST);
            x += 24.0 + space::MD;
        }
        Separator::horizontal().paint(
            c,
            Rect::new(x, y + 12.0, right, y + 12.0 + Separator::THICKNESS),
            WidgetState::REST,
        );
    });
}

/// The character index of `needle` in `hay` (0 when absent) — a link span is
/// counted in characters, not bytes.
fn char_index(hay: &str, needle: &str) -> usize {
    hay.find(needle).map(|b| hay[..b].chars().count()).unwrap_or(0)
}

/// The live tooltip, painted as a **floating surface** in its own top-level
/// window ([`host::overlay`]) so it sits above everything and can spill past
/// the window's edges. Placed the web way — below the pointer, left-aligned
/// with it — against the monitor's work area, not the window.
pub fn overlay(c: &dyn Canvas, f: &Frame) {
    let Some((text, at)) = UI.with(|ui| ui.borrow().pending_tip) else {
        return;
    };
    let tip = Tooltip::new(text);
    let placed = tip.place_at_pointer(c, at, f.screen_area());
    let m = pm::TIP_MARGIN;
    let bounds = Rect::new(
        placed.rect.left - m,
        placed.rect.top - m,
        placed.rect.right + m,
        placed.rect.bottom + m,
    );
    // The surface paints in its own space (origin = its top-left).
    let local = Rect::new(m, m, m + (placed.rect.right - placed.rect.left), m + (placed.rect.bottom - placed.rect.top));
    host::overlay(bounds, move |canvas| {
        tip.paint(canvas, local, WidgetState::REST);
    });
}

// ── The five type roles ──────────────────────────────────────────────────────

fn roles(p: &mut Page) {
    p.section("Label — les cinq rôles typographiques");
    let top = p.y;
    let mut y = top;

    for role in ROLES {
        let l = Label::new(format!("{role:?} — {} px, ligne {}", role.size(), role.line_height())).role(role);
        let want = l.measure(p.c);
        let r = Rect::new(MARGIN, y, MARGIN + want.width, y + want.height);
        l.paint(p.c, r, WidgetState::REST);
        // The measured box, outlined: an auto-size a pixel short shows up as a
        // glyph touching the frame rather than as nothing at all.
        p.c.stroke_rounded(&r, 0.0, &p.c.theme().card_stroke);
        y += want.height + space::XS;
    }

    // The three other knobs the replica carries: disabled, a border, padding.
    let mut inert = Label::new("Désactivé · FixedSingle · padding 8");
    inert.enabled = false;
    inert.border_style = BorderStyle::FixedSingle;
    inert.padding = Padding::all(space::SM);
    let want = inert.measure(p.c);
    inert.paint(p.c, Rect::new(MARGIN, y, MARGIN + want.width, y + want.height), WidgetState::REST);
    y += want.height;

    p.y = top;
    p.advance(y - top);
}

// ── Overflow, in narrow cards ────────────────────────────────────────────────

fn overflow(p: &mut Page) {
    let x0 = pm::COL2;
    let right = p.area.right - MARGIN;
    let t = p.c.theme();
    let f = p.c.formats();
    p.c.text(
        "Débordement — le texte ne sort jamais de sa boîte",
        &Rect::new(x0, p.y, right, p.y + 24.0),
        &f.heading,
        &t.text_primary,
        false,
    );
    let mut y = p.y + 28.0;
    p.c.fill_rounded(&Rect::new(x0, y, right, y + 1.0), 0.0, &t.card_stroke);
    y += space::MD;

    // Three cards side by side would not fit the column, so two rows of two.
    let modes = [
        ("Ellipsis (défaut)", TextOverflow::Ellipsis),
        ("Clip", TextOverflow::Clip),
        ("Wrap + line-clamp", TextOverflow::Wrap),
    ];
    let cells: Vec<(f32, f32)> = (0..4)
        .map(|i| {
            let col = (i % 2) as f32;
            let row = (i / 2) as f32;
            (x0 + col * (pm::CARD_W + space::MD), y + row * (pm::CARD_H + space::SM))
        })
        .collect();
    for ((caption, mode), (cx, cy)) in modes.iter().zip(cells.iter()) {
        let card = Rect::new(*cx, *cy, cx + pm::CARD_W, cy + pm::CARD_H);
        p.c.fill_rounded(&card, 6.0, &t.card_background);
        p.c.stroke_rounded(&card, 6.0, &t.card_stroke);
        Label::new(*caption)
            .role(Role::Meta)
            .paint(p.c, Rect::new(cx + 6.0, cy + 2.0, cx + pm::CARD_W - 6.0, cy + 18.0), WidgetState::REST);
        Label::new(LONG).overflow(*mode).paint(
            p.c,
            Rect::new(cx + 6.0, cy + 18.0, cx + pm::CARD_W - 6.0, cy + pm::CARD_H - 2.0),
            WidgetState::REST,
        );
    }
    // The fourth cell: a multi-line label (explicit break) and a capped badge.
    let (cx, cy) = cells[3];
    let card = Rect::new(cx, cy, cx + pm::CARD_W, cy + pm::CARD_H);
    p.c.fill_rounded(&card, 6.0, &t.card_background);
    p.c.stroke_rounded(&card, 6.0, &t.card_stroke);
    let two = Label::new("Deux lignes\nexplicites").role(Role::Meta);
    let want = two.measure(p.c);
    two.paint(p.c, Rect::new(cx + 6.0, cy + 2.0, cx + 6.0 + want.width, cy + 2.0 + want.height), WidgetState::REST);
    let b = Badge::new("Synchronisation en attente").variant(BadgeVariant::Warning).dot(true).max_width(pm::CARD_W - 12.0);
    let bw = b.measure(p.c);
    b.paint(p.c, Rect::new(cx + 6.0, cy + 36.0, cx + 6.0 + bw.width, cy + 36.0 + bw.height), WidgetState::REST);

    p.y = cells[3].1 + pm::CARD_H;
}

// ── The nine alignments, on one rectangle ────────────────────────────────────

fn alignments(p: &mut Page) {
    p.section("Label — les neuf ContentAlignment · Tooltip — quatre côtés, texte long");
    let top = p.caption("le même rectangle neuf fois : seule l'ancre change");

    let (w, h) = (pm::ALIGN_W, pm::ALIGN_H);
    for (row, chunk) in NINE.chunks(3).enumerate() {
        let y = top + row as f32 * (h + space::SM);
        for (col, (align, name)) in chunk.iter().enumerate() {
            let x = MARGIN + col as f32 * (w + space::SM);
            let r = Rect::new(x, y, x + w, y + h);
            // The box first, so the text's position inside it is readable.
            p.c.fill_rounded(&r, 4.0, &p.c.theme().card_background);
            p.c.stroke_rounded(&r, 4.0, &p.c.theme().card_stroke);
            Label::new(*name).role(Role::Meta).align(*align).paint(p.c, r, WidgetState::REST);
        }
    }

    p.y = top;
    p.advance(3.0 * h + 2.0 * space::SM);
}

// ── Tooltips ─────────────────────────────────────────────────────────────────

/// The four sides against one fixed anchor (so the flip logic is visible
/// without chasing the mouse), then one long bubble wrapped at `maxWidth: 280`.
/// Painted inline because they are specimens: the LIVE tooltip in the
/// interactive column is the floating surface.
fn tooltips(p: &mut Page) {
    // Below the section heading `alignments` drew.
    let top = p.y + 28.0 + space::MD + 22.0;
    let viewport = Size::new(p.area.right, p.area.bottom);
    let ax = pm::COL2 + 90.0;
    let anchor = Rect::new(ax, top + 36.0, ax + 60.0, top + 64.0);
    p.c.stroke_rounded(&anchor, 4.0, &p.c.theme().border_strong);
    for side in SIDES {
        let label = match side {
            Side::Top => "au-dessus",
            Side::Bottom => "au-dessous",
            Side::Left => "à gauche",
            Side::Right => "à droite",
        };
        let tip = Tooltip::new(label).side(side);
        let placement = place(anchor, tip.measure(p.c), side, viewport);
        tip.paint_placed(p.c, &placement, WidgetState::REST);
    }

    // The long one, under the four: grows DOWN, never wider than 280.
    let tip = Tooltip::new(LONG_TIP);
    let size = tip.measure(p.c);
    let y = anchor.bottom + 50.0;
    let r = Rect::new(pm::COL2, y, pm::COL2 + size.width, y + size.height);
    tip.paint(p.c, r, WidgetState::REST);
    p.y = r.bottom + super::sheet::ROW_GAP;
}

// ── Links ────────────────────────────────────────────────────────────────────

fn links(p: &mut Page) {
    p.section("LinkLabel — normal, survol, pressé, visité, désactivé, focus clavier");
    let top = p.caption("souligné au survol seulement (hover:underline), anneau 2 px au focus clavier");

    let states = [
        ("normal", WidgetState::REST, false, true),
        ("survol", WidgetState::REST.hot(true), false, true),
        ("pressé", WidgetState::REST.hot(true).pressed(true), false, true),
        ("visité", WidgetState::REST, true, true),
        ("désactivé", WidgetState::REST.disabled(true), false, false),
        ("focus", WidgetState::REST.focused(true).focus_visible(true), false, true),
    ];
    let mut x = MARGIN;
    for (name, state, visited, enabled) in states {
        let mut l = LinkLabel::new(format!("lien {name}")).visited(visited);
        l.enabled = enabled;
        let want = l.measure(p.c);
        // Two DIP of air on the left so the focus ring (drawn outside the
        // text, like CSS `ring-2`) is not cut by the pane.
        let r = Rect::new(x + 2.0, top, x + 2.0 + want.width.min(pm::LINK_W), top + want.height);
        l.paint(p.c, r, state);
        x += pm::LINK_W + space::SM;
    }

    p.y = top;
    p.advance(20.0);
}

// ── Badges ───────────────────────────────────────────────────────────────────

fn badges(p: &mut Page) {
    p.section("Badge — six variantes, pastille, deux tailles, largeur plafonnée");
    let top = p.caption("chaque pastille est dimensionnée par measure(), pas par sa cellule");

    let mut y = top;

    // Row one: every variant at `md`, plain for the first three and with its
    // dot for the last three (row two shows every variant plain).
    let mut x = MARGIN;
    for (i, variant) in BADGE_VARIANTS.into_iter().enumerate() {
        let b = Badge::new(format!("{variant:?}")).variant(variant).dot(i >= 3);
        let want = b.measure(p.c);
        b.paint(p.c, Rect::new(x, y, x + want.width, y + want.height), WidgetState::REST);
        x += want.width + space::SM;
    }
    y += 20.0 + space::SM;

    // Row two: the same at `sm` (narrower padding, same line box), then the
    // counters a badge is most often, an inert one and a capped one.
    let mut x = MARGIN;
    for variant in BADGE_VARIANTS {
        let b = Badge::new(format!("{variant:?}")).variant(variant).size(BadgeSize::Sm);
        let want = b.measure(p.c);
        b.paint(p.c, Rect::new(x, y, x + want.width, y + want.height), WidgetState::REST);
        x += want.width + space::XS;
    }
    x += space::MD;
    for n in ["1", "12", "99+"] {
        let b = Badge::new(n).variant(BadgeVariant::Danger);
        let want = b.measure(p.c);
        b.paint(p.c, Rect::new(x, y, x + want.width, y + want.height), WidgetState::REST);
        x += want.width + space::XS;
    }
    let mut off = Badge::new("Désactivé").variant(BadgeVariant::Primary).dot(true);
    off.enabled = false;
    let want = off.measure(p.c);
    off.paint(p.c, Rect::new(x, y, x + want.width, y + want.height), WidgetState::REST);
    x += want.width + space::XS;
    let capped = Badge::new("max_width 90 : texte tronqué").variant(BadgeVariant::Success).max_width(90.0);
    let want = capped.measure(p.c);
    capped.paint(p.c, Rect::new(x, y, x + want.width, y + want.height), WidgetState::REST);
    y += 20.0;

    p.y = top;
    p.advance(y - top);
}

// ── Icons, and the two rules ─────────────────────────────────────────────────

fn icons_and_separators(p: &mut Page) {
    p.section("Icon — géométrie, jamais un caractère · glyphe absent = boîte · Separator");
    let top = p.caption("16 et 24 DIP · CircleAlert via l'alias lucide · Save absent · accent et inerte");

    let cell = pm::ICON_CELL;
    let mut y = top;
    for px in [16.0, 24.0] {
        let mut x = MARGIN;
        let tinted = ICONS.iter().map(|n| (*n, None, WidgetState::REST)).chain([
            ("Star", Some(p.c.theme().accent), WidgetState::REST),
            ("Star", None, WidgetState::REST.disabled(true)),
        ]);
        for (name, tint, state) in tinted {
            let icon = Icon::sized(name, px).tint(tint);
            let want = icon.measure(p.c);
            // Centred in a fixed cell so the two rows line up whatever the size.
            let g = Rect::new(
                x + (cell - want.width) / 2.0,
                y + (cell - want.height) / 2.0,
                x + (cell + want.width) / 2.0,
                y + (cell + want.height) / 2.0,
            );
            icon.paint(p.c, g, state);
            x += cell + space::XXS;
        }
        y += cell;
    }

    // The rules: a horizontal one under the grid, then a status bar's three
    // cells divided by vertical rules.
    Separator::horizontal().paint(
        p.c,
        Rect::new(MARGIN, y + 4.0, p.area.right - MARGIN, y + 4.0 + Separator::THICKNESS),
        WidgetState::REST,
    );
    let band = y + 10.0;
    let v = Separator::vertical();
    let mut x = MARGIN;
    for text in ["12 éléments", "3,4 Go", "Synchronisé"] {
        let l = Label::new(text).role(Role::Meta).align(ContentAlignment::MiddleLeft);
        let want = l.measure(p.c);
        l.paint(p.c, Rect::new(x, band, x + want.width, band + 24.0), WidgetState::REST);
        x += want.width + space::MD;
        v.paint(p.c, Rect::new(x, band, x + Separator::THICKNESS, band + 24.0), WidgetState::REST);
        x += Separator::THICKNESS + space::MD;
    }

    p.y = top;
    p.advance(band + 24.0 - top);
}
