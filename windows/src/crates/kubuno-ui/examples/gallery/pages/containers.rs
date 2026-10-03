//! Gallery page — **containers**.
//!
//! Every rectangle a container owns on this page is placed by the family, not by
//! the page: the only `Rect::new` calls below carve the page itself into demo
//! areas, and each demo then hands its area to a container and paints what comes
//! back. That is the whole exercise — if a demo had to compute a child's
//! rectangle to make the container look right, the container would not be doing
//! its job.
//!
//! What to look at:
//!
//! * **Dock** — a Top band, a Left column and a Fill body, resolved in the
//!   toolkit's reverse z-order (the child added *last* takes the outer edge).
//! * **Anchors** — resize the window and watch the three children react: one
//!   stretches, one rides the right edge, one stays put.
//! * **Card** (`@ui/Card.tsx`) — the surface-0 ground, an icon, a subtitle, a
//!   header actions cluster, `dense`, `flush` (rows bleeding to the rounded
//!   edges), a footer on surface-1, raised and the console's layer look. Every
//!   body is painted through `Card::paint_body`, so the check boxes inside read
//!   the card's ground: no square block of the window colour behind them.
//! * **GroupBox** — the caption sits in a GAP of the frame (no erased patch),
//!   a long caption ends in an ellipsis, and the frame keeps its corners.
//! * **Splitter** — the desktop hairline and the web's `ResizeHandle` grip.
//! * **ScrollView** — content clipped to a rounded viewport inside the
//!   hairline, and the `range` family's scroll bar in the view's track.

use std::cell::RefCell;

use drive_app_controls::themes::shape::{radius, space};
use kubuno_controls::enums::{AnchorStyles, BorderStyle, CheckState, Padding, Size};
use kubuno_controls::host::{self, vk, Cursor, Frame, Modifiers};
use kubuno_controls::toolstrip::StripItem;
use kubuno_ui::buttons::{CheckBox, IconButton};
use kubuno_ui::containers::{Axis, Card, GroupBox, Panel, ScrollView, Splitter, Stack, Surface};
use kubuno_ui::lists::{separator, Menu, MenuEntry};
use kubuno_ui::metrics::control;
use kubuno_ui::range::ScrollPart;
use kubuno_ui::{Canvas, FocusId, Rect, Widget, WidgetState};

use super::interact::{self, Live};
use super::sheet::{Page, MARGIN};

/// A stand-in child: a tinted box with its name in it, so a placed rectangle is
/// visible without the other families' primitives being ready.
fn slot(c: &dyn Canvas, r: Rect, label: &str, tint: u8) {
    let t = c.theme();
    let f = c.formats();
    let fill = match tint {
        0 => t.accent_light,
        1 => t.surface_2,
        _ => t.warning_light,
    };
    c.fill_rounded(&r, radius::SM, &fill);
    c.stroke_rounded(&r, radius::SM, &t.card_stroke);
    c.text_ellipsis_center(label, &r, &f.caption, &t.text_secondary);
}

/// A list row that bleeds to its container's edges — what a `flush` card or a
/// scroll view holds. Separated by a hairline, selected rows on the accent tint.
fn list_row(c: &dyn Canvas, r: Rect, label: &str, selected: bool, hot: bool) {
    let t = c.theme();
    if selected {
        c.fill_rounded(&r, 0.0, &t.accent_light);
    } else if hot {
        c.fill_rounded(&r, 0.0, &t.row_hover);
    }
    let text = Rect::new(r.left + space::LG, r.top, r.right - space::LG, r.bottom);
    let colour = if selected { t.text_nav_active } else { t.text_primary };
    c.text_ellipsis(label, &text, &c.formats().body, &colour);
    let rule = Rect::new(r.left, r.bottom - 1.0, r.right, r.bottom);
    c.fill_rounded(&rule, 0.0, &t.divider);
}

/// The demo area for a row: the page width, `h` tall, inset by the margin.
fn area(page: &Page, top: f32, h: f32) -> Rect {
    Rect::new(MARGIN, top, page.area.right - MARGIN, top + h)
}

/// Splits a row in two, with a gutter between.
fn halves(row: Rect) -> (Rect, Rect) {
    let mid = (row.left + row.right) * 0.5;
    (
        Rect::new(row.left, row.top, mid - space::MD, row.bottom),
        Rect::new(mid + space::MD, row.top, row.right, row.bottom),
    )
}

/// `n` equal cells across `row`, `gap` apart.
fn cells(row: Rect, n: usize, gap: f32) -> Vec<Rect> {
    let w = ((row.right - row.left) - gap * (n as f32 - 1.0)) / n as f32;
    (0..n)
        .map(|i| {
            let x = row.left + i as f32 * (w + gap);
            Rect::new(x, row.top, x + w, row.bottom)
        })
        .collect()
}

/// A small caption under a demo — the live values a demo wants to show.
fn note(c: &dyn Canvas, r: Rect, s: &str) {
    let t = c.theme();
    let f = c.formats();
    c.text_ellipsis(s, &Rect::new(r.left, r.bottom + 2.0, r.right, r.bottom + 18.0), &f.caption, &t.text_tertiary);
}

/// One check box row — a control with an opaque-ground preamble, which is what
/// shows whether the container pushed its surface.
fn check_row(c: &dyn Canvas, r: Rect, label: &str, on: bool, state: WidgetState) {
    let st = if on { CheckState::Checked } else { CheckState::Unchecked };
    CheckBox::new(label).check(st).paint(c, r, state);
}

/// The CHECK row's height used by the demos.
const CHECK_ROW: f32 = 22.0;

pub fn draw(c: &dyn Canvas, f: &Frame) {
    // Leave the right column for the live controls, so the static exposition lays
    // out to its left rather than underneath it.
    let mut p = Page::new(c, f.size.0 - interact::PANEL_W(), f.size.1);

    // ── Dock and anchors, side by side ───────────────────────────────────
    p.section("Panel — le conteneur qui manquait");
    let top = p.caption("À gauche : Top · Left · Fill.   À droite : ancres — redimensionne la fenêtre.");
    let (dock_box, anchor_box) = halves(area(&p, top, 84.0));

    let docked = Panel::new()
        .with_surface(Surface::Layer)
        .with_padding(Padding::all(space::SM))
        .fill() // added FIRST, so it receives the remainder
        .top(24.0)
        .left(110.0);
    docked.paint_with(c, dock_box, WidgetState::REST, |c, _| {
        for (r, name) in docked.layout_children(dock_box).into_iter().zip(["Fill", "Top", "Left"]) {
            slot(c, r, name, 1);
        }
    });

    let anchored = Panel::new()
        .with_surface(Surface::Layer)
        .with_border(BorderStyle::FixedSingle)
        .with_padding(Padding::all(space::SM))
        .with_design_size(Size::new(520.0, 84.0))
        .anchored(
            Rect::new(8.0, 8.0, 504.0, 36.0),
            AnchorStyles::LEFT.union(AnchorStyles::RIGHT).union(AnchorStyles::TOP),
        )
        .anchored(Rect::new(374.0, 44.0, 504.0, 72.0), AnchorStyles::RIGHT.union(AnchorStyles::TOP))
        .fixed(Rect::new(8.0, 44.0, 138.0, 72.0));
    anchored.paint_with(c, anchor_box, WidgetState::REST, |c, _| {
        let names = ["gauche + droite → s'étire", "droite → suit le bord", "fixe"];
        for (r, name) in anchored.layout_children(anchor_box).into_iter().zip(names) {
            slot(c, r, name, 0);
        }
    });
    p.advance(84.0);

    // ── Cards ────────────────────────────────────────────────────────────
    p.section("Card — @ui/Card.tsx");
    let top = p.caption(
        "surface-0 · icône · sous-titre + actions · dense · flush · pied de carte · surélevée · look console",
    );
    let row1 = area(&p, top, 112.0);
    let r1 = cells(row1, 4, space::MD);

    // Plain, with an icon — its body holds a real control.
    let plain = Card::titled("Aperçu").with_icon("FolderOpen");
    plain.paint_body(c, r1[0], WidgetState::REST, |c, body| {
        check_row(c, Rect::new(body.left, body.top, body.right, body.top + CHECK_ROW), "Notifications", true, WidgetState::REST);
    });
    // Subtitle + an actions cluster (the « … » button is the caller's).
    let with_actions = Card::titled("Comptes et accès partagés")
        .with_subtitle("42 actifs, 3 suspendus, 7 invitations")
        .with_actions(32.0, 32.0);
    with_actions.paint_body(c, r1[1], WidgetState::REST, |c, body| {
        slot(c, body, "body_rect", 1);
    });
    if let Some(a) = with_actions.actions_rect(r1[1]) {
        let _bg = with_actions.push_surface(c);
        IconButton::plain("MoreHorizontal", 32.0, 16.0).paint(c, a, WidgetState::REST);
    }
    // Dense.
    let dense = Card::titled("Réglages denses").dense();
    dense.paint_body(c, r1[2], WidgetState::REST, |c, body| {
        check_row(c, Rect::new(body.left, body.top, body.right, body.top + CHECK_ROW), "Compacte", false, WidgetState::REST);
    });
    // Flush — rows bleed to the rounded edges, the frame stays on top.
    let flush = Card::titled("Membres").flush();
    flush.paint_body(c, r1[3], WidgetState::REST, |c, body| {
        let names = ["Amélie Durand", "Bastien Leroy", "Chloé Martin", "David Petit"];
        for (i, n) in names.iter().enumerate() {
            let y = body.top + i as f32 * 32.0;
            list_row(c, Rect::new(body.left, y, body.right, y + 32.0), n, i == 1, false);
        }
    });
    p.advance(112.0);

    let row2 = area(&p, p.y, 100.0);
    let r2 = cells(row2, 4, space::MD);
    // No title: a body and a footer only.
    let footer = Card::new().with_footer(16.0);
    footer.paint_body(c, r2[0], WidgetState::REST, |c, body| {
        c.text_ellipsis("62 Go sur 100 Go", &body, &c.formats().body, &c.theme().text_primary);
    });
    if let Some(fb) = footer.footer_body_rect(r2[0]) {
        let _bg = footer.push_footer(c, r2[0]);
        c.text_ellipsis("Total : 62 %", &fb, &c.formats().caption, &c.theme().text_secondary);
    }
    for (card, r) in [
        (Card::titled("Volume").raised(), r2[1]),
        (Card::titled("Catégories").on_layer(), r2[2]),
        (Card::new(), r2[3]),
    ] {
        card.paint_body(c, r, WidgetState::REST, |c, body| {
            check_row(c, Rect::new(body.left, body.top, body.right, body.top + CHECK_ROW), "Sur la carte", true, WidgetState::REST);
        });
    }
    p.advance(100.0);

    // ── GroupBox and Splitter, side by side ──────────────────────────────
    p.section("GroupBox · Splitter");
    let top = p.caption("Légende dans une brèche du cadre, légende longue tronquée · filet et poignée ResizeHandle");
    let (group_row, split_row) = halves(area(&p, top, 84.0));

    let g = cells(group_row, 3, space::MD);
    let group = GroupBox::titled("Quotas par unité").with_padding(Padding::all(space::SM)).fill();
    group.paint_with(c, g[0], WidgetState::REST, |c, _| {
        slot(c, group.layout_children(g[0])[0], "Fill", 1);
    });
    GroupBox::titled("Désactivé")
        .with_padding(Padding::all(space::SM))
        .paint(c, g[1], WidgetState::REST.disabled(true));
    let long = GroupBox::titled("Autorisations d'accès partagées aux invités externes").with_padding(Padding::all(space::SM));
    long.paint_with(c, g[2], WidgetState::REST, |c, d| {
        check_row(c, Rect::new(d.left, d.top, d.right, d.top + CHECK_ROW), "Autoriser", true, WidgetState::REST);
    });

    let (hair_box, handle_box) = halves(split_row);
    for (mut split, sbox, style) in [
        (Splitter::vertical(), hair_box, "filet"),
        (Splitter::vertical().with_handle(), handle_box, "ResizeHandle"),
    ] {
        split = split
            .with_distance((sbox.right - sbox.left) * 0.5)
            .with_minimums(40.0, 40.0);
        // Stateless on purpose: the bar follows the pointer while it is over the
        // demo — the same `drag_to` a real drag calls.
        if sbox.contains(f.mouse.0, f.mouse.1) {
            split.drag_to(sbox, f.mouse.0, f.mouse.1);
            host::set_cursor(split.cursor());
        }
        let rects = split.arrange(sbox);
        slot(c, rects.panel1, "panel1", 1);
        slot(c, rects.panel2, "panel2", 1);
        let hot = sbox.contains(f.mouse.0, f.mouse.1);
        split.paint(c, sbox, WidgetState::REST.hot(hot).pressed(hot && f.mouse_down));
        note(c, sbox, &format!("{style} · distance {:.0}", split.splitter_distance));
    }
    p.advance(84.0 + 18.0);

    // ── ScrollView + Stack ───────────────────────────────────────────────
    p.section("ScrollView + Stack");
    let top = p.caption("Contenu rogné à la fenêtre arrondie, sous le filet ; la barre vient de la famille « range »");
    let box_ = area(&p, top, 110.0);
    let view_box = Rect::new(box_.left, box_.top, (box_.left + 420.0).min(box_.right), box_.bottom);

    let heights = [48.0_f32, 36.0, 64.0, 44.0];
    let mut stack = Stack::column(space::MD).with_padding(Padding::all(space::SM));
    for h in heights {
        stack.push(h);
    }
    let mut view =
        ScrollView::new().with_surface(Surface::Layer).fixed(Rect::new(0.0, 0.0, 1.0, stack.content_extent()));
    let reach = view.max_offset(view_box, Axis::Vertical);
    // Stateless: scrolled to the bottom so the last block meets the rounded
    // bottom corners (the overflow the clip must handle), or following the
    // pointer while it hovers.
    let ratio = if view_box.contains(f.mouse.0, f.mouse.1) {
        ((f.mouse.1 - view_box.top) / (view_box.bottom - view_box.top)).clamp(0.0, 1.0)
    } else {
        0.6
    };
    view.scroll_to(view_box, 0.0, reach * ratio);
    view.paint_content(c, view_box, WidgetState::REST, |c, viewport| {
        let content = Rect::new(
            viewport.left,
            viewport.top - view.offset(Axis::Vertical),
            viewport.right - control::SCROLLBAR,
            viewport.bottom,
        );
        for (i, r) in stack.layout_children(content).into_iter().enumerate() {
            // Wider than the lane on purpose: the clip, not the page, keeps it in.
            slot(c, Rect::new(r.left - space::LG, r.top, r.right + space::XL, r.bottom), &format!("bloc {}", i + 1), 2);
        }
    });
    if let Some(bar) = view.scroll_bar(view_box, Axis::Vertical) {
        bar.paint(c, view.track(view_box, Axis::Vertical), WidgetState::REST);
    }
    let range = view.range(view_box, Axis::Vertical);
    let side = Rect::new(view_box.right + space::XL, view_box.top, p.area.right - MARGIN, view_box.top + 18.0);
    c.text_ellipsis(
        &format!(
            "range(Vertical) → max {:.0} · large_change {:.0} · value {:.0} · visible {}",
            range.maximum, range.large_change, range.value, range.visible
        ),
        &side,
        &c.formats().caption,
        &c.theme().text_secondary,
    );
    c.text_ellipsis(
        "Survole la zone pour faire défiler. Les blocs débordent volontairement : le filet et les coins restent intacts.",
        &Rect::new(side.left, side.bottom + 4.0, side.right, side.bottom + 22.0),
        &c.formats().caption,
        &c.theme().text_tertiary,
    );
    p.advance(110.0);
}

// ─────────────────────────────────────────────────────────────────────────────
// The live column — the same containers, driven by the real pointer and the
// keyboard.
// ─────────────────────────────────────────────────────────────────────────────

/// The card's « … » menu entries (a separator at index 2).
fn card_menu() -> Menu {
    Menu::with_items(vec![
        MenuEntry::new("Renommer").icon("PenLine").build(),
        MenuEntry::new("Partager").icon("Share2").build(),
        separator(),
        MenuEntry::new("Supprimer").icon("Trash2").danger().build(),
    ])
}

/// The rows of [`card_menu`] a keyboard or a click can choose.
const MENU_CHOICES: [usize; 3] = [0, 1, 3];

fn menu_label(menu: &Menu, i: usize) -> Option<String> {
    match menu.items().get(i) {
        Some(StripItem::MenuItem(m)) if m.base.item.enabled => Some(m.base.item.text.clone()),
        _ => None,
    }
}

/// What the live column remembers between frames.
#[derive(Default)]
struct Ui {
    seeded: bool,
    /// The live splitter itself (distance + collapse memory), and its drag.
    split: Splitter,
    split_dragging: bool,
    split_grab: f32,
    /// The scroll view's offset and its thumb drag.
    scroll_y: f32,
    scroll_dragging: bool,
    scroll_grab: f32,
    row_selected: usize,
    card_collapsed: bool,
    /// The card's « … » menu: open, the hovered/keyboard row, where it was
    /// painted last frame (click routing), and the last choice.
    menu_open: bool,
    menu_hot: Option<usize>,
    menu_panel: Option<Rect>,
    menu_choice: Option<String>,
    group_enabled: bool,
    prev_down: bool,
}

thread_local! {
    static UI: RefCell<Ui> = RefCell::new(Ui::default());
}

const ID_SPLIT: FocusId = FocusId::of("containers.split");
const ID_SCROLL: FocusId = FocusId::of("containers.scroll");
const ID_CARD: FocusId = FocusId::of("containers.card");
const ID_MORE: FocusId = FocusId::of("containers.more");
const ID_GROUP: FocusId = FocusId::of("containers.group");

/// The right-hand column: the container family, live — mouse AND keyboard.
pub fn interactive_column(c: &dyn Canvas, f: &Frame) {
    UI.with(|ui| {
        let mut ui = ui.borrow_mut();
        let mut live = Live::new(f, ui.prev_down);
        ui.prev_down = f.mouse_down;

        // ── An open menu takes the pointer first (the web's backdrop) ─────
        if f.dismiss {
            ui.menu_open = false;
        }
        if ui.menu_open {
            let (px, py) = f.mouse;
            if let Some(panel) = ui.menu_panel {
                let menu = card_menu();
                ui.menu_hot = menu.item_at(panel, px, py).filter(|i| MENU_CHOICES.contains(i)).or(ui.menu_hot);
                if live.clicked {
                    if panel.contains(px, py) {
                        if let Some(label) = menu.item_at(panel, px, py).and_then(|i| menu_label(&menu, i)) {
                            ui.menu_choice = Some(label);
                            ui.menu_open = false;
                        }
                    } else {
                        ui.menu_open = false;
                    }
                    live.clicked = false;
                }
            }
            live.mouse = (host::POINTER_AWAY, host::POINTER_AWAY);
        }

        let (left, mut y, right) = interact::panel(c, interact::panel_rect(f.size));
        // The column is a Layer panel: announce its ground for everything below.
        let column_bg = Surface::Layer.ground(c).unwrap_or(c.theme().layer_background);
        let _column = kubuno_ui::containers::Scope::bg(c, column_bg);

        // ── Splitter — drag, double-click to reset, keys when focused ─────
        y = interact::caption(c, left, right, y, "Splitter — glisser · double-clic · ←/→ Début/Fin Entrée");
        let split_box = Rect::new(left, y, right, y + 84.0);
        let half = (right - left) * 0.5;
        if !ui.seeded {
            ui.split = Splitter::vertical().with_handle().with_distance(half).with_minimums(60.0, 60.0);
            ui.group_enabled = true;
            ui.seeded = true;
        }
        let grip = ui.split.grip_rect(split_box);
        let fs = live.focus(ID_SPLIT, grip);
        let grip_hot = ui.split.hit_test_grip(split_box, live.mouse.0, live.mouse.1);
        if live.double_hit(grip) {
            ui.split.splitter_distance = half;
            ui.split.restore = None;
        } else if live.clicked && grip_hot {
            ui.split_dragging = true;
            ui.split_grab = live.mouse.0 - ui.split.splitter_rect(split_box).left;
        }
        if !live.down {
            ui.split_dragging = false;
        }
        if ui.split_dragging {
            let x = live.mouse.0 - ui.split_grab;
            ui.split.drag_to(split_box, x, live.mouse.1);
        }
        if fs.focused {
            for key in [vk::LEFT, vk::RIGHT, vk::HOME, vk::END, vk::ENTER] {
                for mods in host::take_key_any(key) {
                    ui.split.handle_key(split_box, key, mods);
                }
            }
        }
        if grip_hot || ui.split_dragging {
            host::set_cursor(ui.split.cursor());
        }
        let rects = ui.split.arrange(split_box);
        slot(c, rects.panel1, "panel1", 1);
        slot(c, rects.panel2, "panel2", 1);
        let st = fs.apply(WidgetState::REST.hot(grip_hot || ui.split_dragging).pressed(ui.split_dragging));
        ui.split.paint(c, split_box, st);
        let (min, max) = ui.split.distance_range(split_box);
        y += 84.0 + 2.0;
        c.text(
            &format!("splitter_distance = {:.0} ∈ [{min:.0}, {max:.0}]", ui.split.splitter_distance),
            &Rect::new(left, y, right, y + 16.0),
            &c.formats().caption,
            &c.theme().text_tertiary,
            false,
        );
        y += 16.0 + 12.0;

        // ── ScrollView — wheel, thumb, keys when focused, click a row ─────
        y = interact::caption(c, left, right, y, "ScrollView — molette · pouce · ↑↓ PgPr/PgSv Début/Fin Espace");
        let view_box = Rect::new(left, y, right, y + 140.0);
        const ROW_H: f32 = 32.0;
        const ROWS: usize = 12;
        let mut view = ScrollView::new()
            .with_surface(Surface::Layer)
            .fixed(Rect::new(0.0, 0.0, 1.0, ROW_H * ROWS as f32));
        view.scroll_to(view_box, 0.0, ui.scroll_y);
        let vs = live.focus(ID_SCROLL, view_box);
        let (_, wheel_dy) = live.wheel_over(view_box);
        if wheel_dy != 0.0 {
            view.wheel(view_box, (0.0, wheel_dy), live.mods.shift);
        }
        if vs.focused {
            for key in [vk::UP, vk::DOWN, vk::PAGE_UP, vk::PAGE_DOWN, vk::HOME, vk::END, vk::SPACE] {
                for mods in host::take_key_any(key) {
                    view.handle_key(view_box, key, mods);
                }
            }
        }
        // The range family's bar, in the view's track: unfolds under the
        // pointer, drags by its thumb, pages on a track click.
        let track = view.track(view_box, Axis::Vertical);
        let over_track = live.hover(track) || ui.scroll_dragging;
        let mut on_thumb = false;
        if let Some(bar) = view.scroll_bar(view_box, Axis::Vertical).map(|b| b.with_expanded(over_track)) {
            let thumb = bar.thumb_rect(track);
            if live.clicked {
                match bar.part_at(track, live.mouse.0, live.mouse.1) {
                    Some(ScrollPart::Thumb) => {
                        ui.scroll_dragging = true;
                        ui.scroll_grab = live.mouse.1 - thumb.top;
                    }
                    Some(ScrollPart::PageLow) => {
                        view.handle_key(view_box, vk::PAGE_UP, Modifiers::NONE);
                    }
                    Some(ScrollPart::PageHigh) => {
                        view.handle_key(view_box, vk::PAGE_DOWN, Modifiers::NONE);
                    }
                    Some(ScrollPart::ArrowLow) => {
                        view.handle_key(view_box, vk::UP, Modifiers::NONE);
                    }
                    Some(ScrollPart::ArrowHigh) => {
                        view.handle_key(view_box, vk::DOWN, Modifiers::NONE);
                    }
                    None => {}
                }
            }
            if !live.down {
                ui.scroll_dragging = false;
            }
            if ui.scroll_dragging {
                let start = live.mouse.1 - ui.scroll_grab - track.top;
                let y_off = view.offset_for_thumb(view_box, Axis::Vertical, track.bottom - track.top, thumb.bottom - thumb.top, start);
                view.scroll_to(view_box, 0.0, y_off);
            }
            on_thumb = live.hover(track);
        }
        ui.scroll_y = view.offset(Axis::Vertical);

        let viewport = view.viewport(view_box);
        let lane = Rect::new(viewport.left, viewport.top, viewport.right - control::SCROLLBAR, viewport.bottom);
        let offset_now = ui.scroll_y;
        let row_at = |i: usize| {
            let top = lane.top - offset_now + i as f32 * ROW_H;
            Rect::new(lane.left, top, lane.right, top + ROW_H)
        };
        if live.clicked && live.hover(lane) && !on_thumb {
            if let Some(i) = (0..ROWS).find(|&i| live.hover(row_at(i))) {
                ui.row_selected = i;
                view.ensure_visible(view_box, row_at(i));
                ui.scroll_y = view.offset(Axis::Vertical);
            }
        }
        let selected = ui.row_selected;
        let scroll_y = ui.scroll_y;
        let mouse = live.mouse;
        view.paint_content(c, view_box, vs.apply(WidgetState::REST), |c, _| {
            for i in 0..ROWS {
                let top = lane.top - scroll_y + i as f32 * ROW_H;
                let r = Rect::new(lane.left, top, lane.right, top + ROW_H);
                if r.bottom < lane.top || r.top > lane.bottom {
                    continue;
                }
                let hot = r.contains(mouse.0, mouse.1) && lane.contains(mouse.0, mouse.1);
                list_row(c, r, &format!("Fichier {:02} — rapport trimestriel des ventes régionales", i + 1), i == selected, hot);
            }
        });
        if let Some(bar) = view.scroll_bar(view_box, Axis::Vertical).map(|b| b.with_expanded(over_track)) {
            bar.paint(c, track, WidgetState::REST.hot(live.hover(track)).pressed(ui.scroll_dragging));
        }
        y += 140.0 + 12.0;

        // ── Card — header toggles, « … » opens a menu (popup) ─────────────
        y = interact::caption(c, left, right, y, "Card — en-tête : clic/Entrée replie · « … » : menu clavier");
        let mut card = Card::titled("Détails du compte")
            .with_icon("Users")
            .with_subtitle("42 actifs · 3 suspendus")
            .with_actions(32.0, 32.0);
        if !ui.card_collapsed {
            card = card.with_footer(16.0);
        }
        let full_h = 150.0;
        // Collapsed: the header band plus the card's two border lines.
        let card_h = if ui.card_collapsed { card.header_height() + 2.0 } else { full_h };
        let card_rect = Rect::new(left, y, right, y + card_h);
        let header = card.header_rect(card_rect).unwrap_or(card_rect);
        let more = card.actions_rect(card_rect).unwrap_or(header);
        let title_zone = Rect::new(header.left, header.top, more.left, header.bottom);
        let hs = live.focus(ID_CARD, title_zone);
        let ms = live.focus(ID_MORE, more);
        if live.hit(title_zone)
            || (hs.focused && (live.take_key(vk::ENTER, Modifiers::NONE) || live.take_key(vk::SPACE, Modifiers::NONE)))
        {
            ui.card_collapsed = !ui.card_collapsed;
        }
        if live.hover(title_zone) {
            host::set_cursor(Cursor::Hand);
        }
        let open_by_key = ms.focused
            && !ui.menu_open
            && (live.take_key(vk::ENTER, Modifiers::NONE)
                || live.take_key(vk::SPACE, Modifiers::NONE)
                || live.take_key(vk::DOWN, Modifiers::NONE));
        if live.hit(more) || open_by_key {
            ui.menu_open = true;
            ui.menu_hot = if open_by_key { Some(MENU_CHOICES[0]) } else { None };
        }
        if ui.menu_open {
            // The menu reads its keys from the same queue.
            let pos = ui.menu_hot.and_then(|h| MENU_CHOICES.iter().position(|&i| i == h));
            if live.take_key(vk::DOWN, Modifiers::NONE) {
                ui.menu_hot = Some(MENU_CHOICES[pos.map_or(0, |p| (p + 1) % MENU_CHOICES.len())]);
            }
            if live.take_key(vk::UP, Modifiers::NONE) {
                let n = MENU_CHOICES.len();
                ui.menu_hot = Some(MENU_CHOICES[pos.map_or(n - 1, |p| (p + n - 1) % n)]);
            }
            if live.take_key(vk::ENTER, Modifiers::NONE) {
                if let Some(label) = ui.menu_hot.and_then(|h| menu_label(&card_menu(), h)) {
                    ui.menu_choice = Some(label);
                }
                ui.menu_open = false;
                interact::with_focus(|r| r.focus_visibly(ID_MORE));
            }
            if live.take_escape() {
                ui.menu_open = false;
                interact::with_focus(|r| r.focus_visibly(ID_MORE));
            }
        }
        let collapsed = ui.card_collapsed;
        let group_enabled = ui.group_enabled;
        if collapsed {
            // Only the header shows: paint a header-only card of the same look.
            card.paint(c, card_rect, WidgetState::REST);
        } else {
            card.paint_body(c, card_rect, WidgetState::REST, |c, body| {
                check_row(
                    c,
                    Rect::new(body.left, body.top, body.right, body.top + CHECK_ROW),
                    "Visible par l'équipe",
                    group_enabled,
                    WidgetState::REST,
                );
                c.text_ellipsis(
                    "Un contenu long se termine par des points de suspension au lieu de déborder.",
                    &Rect::new(body.left, body.top + CHECK_ROW + 4.0, body.right, body.top + CHECK_ROW + 22.0),
                    &c.formats().body,
                    &c.theme().text_secondary,
                );
            });
            if let Some(fb) = card.footer_body_rect(card_rect) {
                let _bg = card.push_footer(c, card_rect);
                let choice = ui.menu_choice.clone().unwrap_or_else(|| "Aucune action".to_string());
                c.text_ellipsis(&format!("Dernière action : {choice}"), &fb, &c.formats().caption, &c.theme().text_secondary);
            }
        }
        {
            // Header controls land on the card's surface-0 ground.
            let _bg = card.push_surface(c);
            let st = ms.apply(live.state(more)).selected(ui.menu_open);
            IconButton::plain("MoreHorizontal", 32.0, 16.0).paint(c, more, st);
            if hs.visible {
                c.stroke_rounded_w(&title_zone.inflate(-2.0, -2.0), radius::SM, &c.theme().accent, 2.0);
            }
        }
        y += card_h + 12.0;

        // ── GroupBox — Space / click toggles the group ────────────────────
        y = interact::caption(c, left, right, y, "GroupBox — cliquez ou Espace pour activer/désactiver");
        let gb_rect = Rect::new(left, y, right, y + 76.0);
        let title = if ui.group_enabled {
            "Quotas de stockage par unité organisationnelle — actif"
        } else {
            "Quotas de stockage par unité organisationnelle — désactivé"
        };
        let gb = GroupBox::titled(title).with_padding(Padding::all(space::SM));
        let inner = gb.inner_rect(gb_rect);
        let gs = live.focus(ID_GROUP, inner);
        if live.hit(inner) || (gs.focused && live.take_key(vk::SPACE, Modifiers::NONE)) {
            ui.group_enabled = !ui.group_enabled;
        }
        let dead = !ui.group_enabled;
        let gb_state = WidgetState::REST.disabled(dead);
        let check_state = gs.apply(live.state(inner)).disabled(dead);
        gb.paint_with(c, gb_rect, gb_state, |c, d| {
            check_row(c, Rect::new(d.left, d.top, d.right, d.top + CHECK_ROW), "Appliquer à toutes les unités", !dead, check_state);
        });
        y += 76.0 + 12.0;

        // ── Panel — static, on purpose ───────────────────────────────────────
        y = interact::caption(c, left, right, y, "Panel — statique : un conteneur ne réagit pas au pointeur");
        let panel_rect = Rect::new(left, y, right, y + 56.0);
        let panel = Panel::new()
            .with_surface(Surface::Card)
            .with_padding(Padding::all(space::SM))
            .fill()
            .top(22.0);
        panel.paint_with(c, panel_rect, WidgetState::REST, |c, _| {
            for (r, name) in panel.layout_children(panel_rect).into_iter().zip(["Fill", "Top"]) {
                slot(c, r, name, 1);
            }
        });

        // ── The menu, in an interactive popup placed against the SCREEN ──────
        if ui.menu_open {
            let mut menu = card_menu();
            let want = menu.measure(c);
            let area = f.screen_area();
            const EDGE: f32 = 8.0;
            const GAP: f32 = 4.0;
            // `MenuDropdown` `align="end"`: right edges aligned, below the
            // trigger; flipped above when it would leave the screen.
            let x = (more.right - want.width).clamp(area.left + EDGE, (area.right - EDGE - want.width).max(area.left + EDGE));
            let top = if more.bottom + GAP + want.height > area.bottom - EDGE {
                (more.top - GAP - want.height).max(area.top + EDGE)
            } else {
                more.bottom + GAP
            };
            let panel = Rect::new(x, top, x + want.width, top + want.height);
            menu.hot_index = ui.menu_hot;
            ui.menu_panel = Some(panel);
            interact::with_focus(|r| r.keep_focus_in(panel));
            const SHADOW: f32 = 10.0;
            let pb = panel.inflate(SHADOW, SHADOW);
            let local = Rect::new(SHADOW, SHADOW, SHADOW + want.width, SHADOW + want.height);
            host::popup(pb, move |canvas| menu.paint(canvas, local, WidgetState::REST));
        } else {
            ui.menu_panel = None;
        }
    });
}
