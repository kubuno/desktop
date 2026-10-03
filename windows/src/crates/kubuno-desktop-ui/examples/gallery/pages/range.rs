//! Gallery page — **range**.
//!
//! The top of the page is the one that matters: [`super::sheet::pair`] hands the
//! *same* rectangle to `kubuno_drive_desktop_app_controls::scrollbar` (what the shell paints
//! today) and to [`kubuno_desktop_ui::range::ScrollBar`], driven from the same
//! content/viewport/scroll description. The two must be indistinguishable —
//! `scrollbar_geometry_matches_the_predecessor` asserts it, this page shows it.
//!
//! Below that come the three primitives with no predecessor, in every state the
//! design system distinguishes — focus-visible ring, disabled, invalid, a
//! hovered or spent step button, a field being typed into, text too long for
//! its box, a slider on a card (no ground of its own) — and the interactive
//! column drives all four with the real pointer, the keyboard (ARIA keys,
//! typing, clipboard) and the wheel, the slider's value bubble floating in a
//! `host::overlay` above everything.

use std::cell::RefCell;

use kubuno_drive_desktop_app_controls::scrollbar::{self as old_scrollbar, Scrollbar as OldScrollbar};
use kubuno_desktop_controls::host::{self, vk, Cursor, Frame, InputEvent, Modifiers};
use kubuno_desktop_controls::range::{Orientation, TickStyle};
use kubuno_desktop_ui::focus::{caret_visible, FocusOpts};
use kubuno_desktop_ui::range::{
    DomainField, EditOutcome, NumericEdit, NumericField, ProgressBar, ProgressSize, ProgressVariant,
    RangeKey, ScrollBar, ScrollPart, Slider, SpinPart, WheelSteps, SCROLL_REPEAT_DELAY_MS,
    SCROLL_REPEAT_INTERVAL_MS, VALUE_BUBBLE_MARGIN,
};
use kubuno_desktop_controls::host::WHEEL_NOTCH_DIP;
use kubuno_desktop_ui::metrics::control::SCROLLBAR;
use kubuno_desktop_ui::{Canvas, Rect, Widget, WidgetState};

use super::interact::{self, Live};
use super::sheet::{cells, Page, MARGIN, NAV_H};

/// The panel a scroll bar overlays, so the translucent gutter reads.
fn content_backdrop(c: &dyn Canvas, r: Rect) {
    let t = c.theme();
    c.fill_rounded(&r, 4.0, &t.card_background);
    c.stroke_rounded(&r, 4.0, &t.card_stroke);
}

/// One old/new scroll-bar pair, from a single content description.
#[allow(clippy::too_many_arguments)]
fn scrollbar_pair(
    c: &dyn Canvas,
    left: f32,
    top: f32,
    w: f32,
    h: f32,
    horizontal: bool,
    extent: f32,
    scroll: f32,
    expanded: bool,
    hot: bool,
) -> f32 {
    super::sheet::pair(
        c,
        left,
        top,
        w,
        h,
        |a| {
            content_backdrop(c, a);
            if let Some(bar) = OldScrollbar::new(&a, extent, scroll, horizontal, expanded) {
                old_scrollbar::draw(c, &bar, 1.0, hot);
            }
        },
        |b| {
            content_backdrop(c, b);
            // The viewport is the content rectangle's own length on the bar's
            // axis — exactly what the predecessor derives internally.
            let viewport = if horizontal { b.right - b.left } else { b.bottom - b.top };
            if let Some(mut bar) = ScrollBar::from_content(horizontal, extent, viewport, scroll) {
                bar.expanded = expanded;
                bar.paint(c, bar.rail(&b), WidgetState::REST.hot(hot));
            }
        },
    )
}

/// A labelled slider cell. The slider gets exactly what it measures, centred
/// in what is left under the caption.
fn slider_cell(c: &dyn Canvas, r: Rect, s: &Slider, state: WidgetState, label: &str) {
    let t = c.theme();
    let f = c.formats();
    let caption = Rect::new(r.left, r.top, r.right, r.top + 14.0);
    c.text_ellipsis(label, &caption, &f.caption, &t.text_tertiary);
    let h = s.measure(c).height;
    let cy = (caption.bottom + r.bottom) / 2.0;
    s.paint(c, Rect::new(r.left, cy - h / 2.0, r.right, cy + h / 2.0), state);
}

/// A keyboard-focused state: `:focus-visible`, the ring shows.
fn keyboard_focus() -> WidgetState {
    WidgetState::REST.focused(true).focus_visible(true)
}

pub fn draw(c: &dyn Canvas, f: &Frame) {
    // The static exposition takes the window minus the interactive column, so it
    // lays out to the left of it rather than under it.
    let mut p = Page::new(c, f.size.0 - interact::PANEL_W(), f.size.1);
    let t = c.theme();
    // The exposition is taller than the window: it scrolls, through this
    // family's own ScrollBar (see `expo_scroll_begin`).
    let scroll = expo_scroll_begin(f, p.area);
    p.y -= scroll;
    let avail = p.area.right - 2.0 * MARGIN - SCROLLBAR;

    // ── ScrollBar — the non-regression pairs ────────────────────────────────
    p.section("ScrollBar — actuel / reconstruit");

    let top = p.caption("Vertical · 1000 DIP de contenu · repos, déplié, déplié survolé");
    let cell_w = 56.0;
    let cell_h = 110.0;
    let pitch = 2.0 * cell_w + 40.0 + 28.0;
    // (scroll, expanded, hot) — the resting indicator, then the unfolded gutter
    // at both ends of the travel.
    let vertical: [(f32, bool, bool); 4] =
        [(0.0, false, false), (0.0, true, false), (440.0, true, true), (872.0, true, false)];
    let mut used = 0.0_f32;
    for (i, &(scroll, expanded, hot)) in vertical.iter().enumerate() {
        used = scrollbar_pair(
            c,
            MARGIN + i as f32 * pitch,
            top,
            cell_w,
            cell_h,
            false,
            1000.0,
            scroll,
            expanded,
            hot,
        );
    }
    p.advance(used);

    let top = p.caption("Horizontal · même modèle, axe tourné");
    let hcell_w = 150.0;
    let hcell_h = 44.0;
    let hpitch = 2.0 * hcell_w + 40.0 + 28.0;
    let horizontal: [(f32, bool); 2] = [(0.0, false), (600.0, true)];
    let mut used = 0.0_f32;
    for (i, &(scroll, expanded)) in horizontal.iter().enumerate() {
        used = scrollbar_pair(
            c,
            MARGIN + i as f32 * hpitch,
            top,
            hcell_w,
            hcell_h,
            true,
            1000.0,
            scroll,
            expanded,
            false,
        );
    }
    p.advance(used);

    // ── Slider ──────────────────────────────────────────────────────────────
    p.section("Slider — @ui/RangeSlider sur TrackBar");

    let top = p.caption("Valeurs et états — focus clavier = anneau hors du halo · désactivé = opacity-60");
    let row_h = 38.0;
    let row = fit(avail, top, 170.0, row_h, 24.0, 5);

    let mut zero = Slider::new();
    let _ = zero.set_value(0);
    slider_cell(c, row[0], &zero, WidgetState::REST, "value = minimum");

    let mut mid = Slider::new();
    let _ = mid.set_value(4);
    slider_cell(c, row[1], &mid, WidgetState::REST.hot(true), "4/10 · survolé (inchangé, comme le web)");

    let mut full = Slider::new();
    let _ = full.set_value(10);
    slider_cell(c, row[2], &full, keyboard_focus(), "maximum · focus clavier");

    let mut clicked = Slider::new();
    let _ = clicked.set_value(6);
    slider_cell(c, row[3], &clicked, WidgetState::REST.focused(true), "focus souris (pas d'anneau)");

    let mut off = Slider::new();
    let _ = off.set_value(3);
    slider_cell(c, row[4], &off, WidgetState::REST.disabled(true), "désactivé");
    p.advance(row_h);

    // The bubble variant with `showValue`, painted in place (the live column
    // floats the same bubble in a `host::overlay`), and a slider on a card —
    // the card's surface must show all around the rail.
    let top = p.caption("showValue (bulle au-dessus du curseur) · sur une carte : aucun fond propre");
    let row = fit(avail, top + 24.0, 220.0, 44.0, 28.0, 2);
    let mut bubbled = Slider::new();
    bubbled.set_maximum(100);
    let _ = bubbled.set_value(62);
    let sh = bubbled.measure(c).height;
    let sr = Rect::new(row[0].left, row[0].top + 12.0, row[0].right, row[0].top + 12.0 + sh);
    bubbled.paint(c, sr, WidgetState::REST);
    let text = format!("{} %", bubbled.value());
    Slider::paint_value_bubble(c, bubbled.value_bubble_rect(c, sr, &text), &text);

    let card = row[1];
    c.fill_rounded(&card, 8.0, &t.surface_2);
    c.stroke_rounded(&card, 8.0, &t.card_stroke);
    let mut on_card = Slider::new();
    let _ = on_card.set_value(3);
    let inner = Rect::new(card.left + 12.0, card.top, card.right - 12.0, card.bottom);
    let cy = (inner.top + inner.bottom) / 2.0;
    on_card.paint(c, Rect::new(inner.left, cy - sh / 2.0, inner.right, cy + sh / 2.0), keyboard_focus());
    p.advance(24.0 + 44.0);

    let top = p.caption("TickStyle — les crans viennent de TrackBar::tick_values (dernier écart plus court)");
    let tick_h = 44.0;
    let tick_row = fit(avail - 3.0 * 36.0 - 24.0, top, 200.0, tick_h, 24.0, 3);
    for (i, style) in [TickStyle::BottomRight, TickStyle::TopLeft, TickStyle::Both]
        .into_iter()
        .enumerate()
    {
        let mut s = Slider::new();
        s.set_maximum(20);
        s.set_tick_frequency(3);
        s.set_tick_style(style);
        let _ = s.set_value(12);
        slider_cell(c, tick_row[i], &s, WidgetState::REST, &format!("{style:?}"));
    }

    // Vertical sliders, beside the tick row: the value grows UPWARD.
    let vx = tick_row[2].right + 24.0;
    let vtop = top;
    let vh = 100.0;
    for (i, value) in [0, 5, 10].into_iter().enumerate() {
        let mut s = Slider::new();
        s.set_orientation(Orientation::Vertical);
        let _ = s.set_value(value);
        let w = s.measure(c).width;
        let x = vx + i as f32 * 36.0;
        let state = if value == 5 { keyboard_focus() } else { WidgetState::REST };
        s.paint(c, Rect::new(x, vtop, x + w, vtop + vh), state);
    }
    p.advance(vh.max(tick_h));

    // ── ProgressBar ─────────────────────────────────────────────────────────
    p.section("ProgressBar — @ui/ProgressBar sur la réplique labels::ProgressBar");

    let top = p.caption("Variante « auto » : primaire → avertissement (75 %) → danger (90 %)");
    let bar_h = 18.0;
    let bars = fit(avail, top, 150.0, bar_h, 20.0, 6);
    for (i, value) in [0, 40, 75, 89, 90, 100].into_iter().enumerate() {
        let mut b = ProgressBar::new();
        b.set_value(value);
        b.paint(c, bars[i], WidgetState::REST);
    }
    p.advance(bar_h);

    let top = p.caption("Variante explicite, tailles md/sm, RTL, désactivé, indéterminé (3 phases)");
    let bars = fit(avail, top, 150.0, bar_h, 20.0, 6);

    let mut success = ProgressBar::new().with_variant(ProgressVariant::Success);
    success.set_value(96);
    success.paint(c, bars[0], WidgetState::REST);

    // The two sizes `@ui/ProgressBar` offers, at the same value and with the
    // variant pinned so the auto thresholds cannot recolour one of them.
    let mut medium = ProgressBar::new()
        .with_size(ProgressSize::Md)
        .with_variant(ProgressVariant::Primary);
    medium.set_value(60);
    medium.paint(c, bars[1], WidgetState::REST);

    let mut small = ProgressBar::new()
        .with_size(ProgressSize::Sm)
        .with_variant(ProgressVariant::Primary);
    small.set_value(60);
    small.paint(c, bars[2], WidgetState::REST);

    let mut rtl = ProgressBar::new().with_variant(ProgressVariant::Primary);
    rtl.set_value(35);
    rtl.right_to_left_layout = true;
    rtl.paint(c, bars[3], WidgetState::REST);

    let mut dead = ProgressBar::new();
    dead.set_value(45);
    dead.paint(c, bars[4], WidgetState::REST.disabled(true));

    // The indeterminate sliver, sampled at three points of its keyframe — the
    // control owns no timer, the host advances `phase`. Stacked in one cell, so
    // the `sm` track is the one that fits three bands in the row's height.
    let mut spinner = ProgressBar::new().with_size(ProgressSize::Sm);
    spinner.set_indeterminate(true);
    let strip = bars[5];
    let third = (strip.bottom - strip.top) / 3.0;
    for (k, phase) in [0.25_f32, 0.5, 0.75].into_iter().enumerate() {
        spinner.phase = phase;
        let y = strip.top + k as f32 * third;
        spinner.paint(c, Rect::new(strip.left, y, strip.right, y + third), WidgetState::REST);
    }
    p.advance(bar_h);

    let top = p.caption("label + showValue : le libellé se tronque (…), la valeur garde sa place");
    let bars = fit(avail, top, 150.0, 30.0, 20.0, 3);
    let headed = [
        ("Stockage", 42),
        ("Quota de l'équipe documentation partagée", 81),
        ("Anticonstitutionnellement-RapportFinal", 95),
    ];
    for (i, (label, value)) in headed.into_iter().enumerate() {
        let mut b = ProgressBar::new().with_label(label).with_value_shown(true);
        b.set_value(value);
        let h = b.measure(c).height;
        b.paint(c, Rect::new(bars[i].left, bars[i].top, bars[i].right, bars[i].top + h), WidgetState::REST);
    }
    p.advance(30.0);

    // ── Spinners ────────────────────────────────────────────────────────────
    p.section("NumericField / DomainField — @ui/NumberInput sur UpDownBase");

    let top = p.caption("Repos, bouton survolé, focus, désactivé (opacity-50), invalide, invalide + focus");
    let field_h = 36.0;
    let fields = fit(avail, top, 130.0, field_h, 16.0, 6);

    let plain = NumericField::ranged(0.0, 100.0).with_value(42.0).unwrap_or_default();
    plain.paint(c, fields[0], WidgetState::REST);
    let mut hot = NumericField::ranged(0.0, 100.0).with_value(42.0).unwrap_or_default();
    hot.spin_hot = Some(SpinPart::Up);
    hot.paint(c, fields[1], WidgetState::REST.hot(true));
    plain.paint(c, fields[2], WidgetState::REST.focused(true));
    plain.paint(c, fields[3], WidgetState::REST.disabled(true));
    let bad = NumericField::ranged(0.0, 100.0).with_value(42.0).unwrap_or_default().with_invalid(true);
    bad.paint(c, fields[4], WidgetState::REST);
    bad.paint(c, fields[5], WidgetState::REST.focused(true));
    p.advance(field_h);

    let top = p.caption("En saisie (sélection + caret), au maximum (▲ éteint), décimales, milliers trop long, hexadécimal");
    let fields = fit(avail, top, 130.0, field_h, 16.0, 5);

    let editing = NumericField::ranged(0.0, 500.0).with_value(128.0).unwrap_or_default();
    let mut edit = editing.begin_edit();
    edit.set_caret(1, false);
    edit.set_caret(3, true);
    editing.paint_editing(c, fields[0], WidgetState::REST.focused(true), &edit, true);

    let at_max = NumericField::ranged(0.0, 100.0).with_value(100.0).unwrap_or_default();
    at_max.paint(c, fields[1], WidgetState::REST);

    let mut money = NumericField::ranged(0.0, 20_000.0);
    let _ = money.set_decimal_places(2);
    money.set_thousands_separator(true);
    let _ = money.set_value(10_000.0);
    money.paint(c, fields[2], WidgetState::REST);

    // Too long for its box: clipped at the padding, never over the buttons.
    let mut big = NumericField::ranged(0.0, 10_000_000_000.0);
    big.set_thousands_separator(true);
    let _ = big.set_value(1_234_567_890.0);
    big.paint(c, Rect::new(fields[3].left, fields[3].top, fields[3].left + 96.0, fields[3].bottom), WidgetState::REST);

    // `Hexadecimal` wins over the decimal options, as the toolkit formats it.
    let mut hex = NumericField::ranged(0.0, 65_535.0);
    hex.set_hexadecimal(true);
    let _ = hex.set_value(48_879.0);
    hex.paint(c, fields[4], WidgetState::REST);
    p.advance(field_h);

    let top = p.caption("UpDownAlign = Left, TextAlign = Right, et un DomainUpDown (repos, focus, premier élément ▲ éteint)");
    let fields = fit(avail, top, 130.0, field_h, 16.0, 4);

    let mut left = NumericField::ranged(0.0, 100.0);
    let _ = left.set_value(7.0);
    left.set_up_down_align(kubuno_desktop_controls::enums::LeftRightAlignment::Left);
    left.paint(c, fields[0], WidgetState::REST);

    let mut right_aligned = NumericField::ranged(0.0, 100.0);
    let _ = right_aligned.set_value(88.0);
    right_aligned.set_text_align(kubuno_desktop_controls::enums::HorizontalAlignment::Right);
    right_aligned.paint(c, fields[1], WidgetState::REST);

    let domain = DomainField::with_items(["Alpha", "Bravo", "Charlie"]);
    domain.paint(c, fields[2], WidgetState::REST);
    domain.paint(c, fields[3], WidgetState::REST.focused(true));
    p.advance(field_h);

    expo_scroll_end(c, f, p.area, p.y + scroll);
}

/// Lays `n` cells of at most `w` across `avail`, shrinking them to fit.
fn fit(avail: f32, top: f32, w: f32, h: f32, gap: f32, n: usize) -> Vec<Rect> {
    let w = w.min((avail - gap * (n.saturating_sub(1)) as f32) / n.max(1) as f32).max(1.0);
    cells(MARGIN, top, w, h, gap, n)
}

/// The exposition's own scroll state: the offset, the content height measured
/// last frame, and a thumb drag.
#[derive(Default)]
struct Expo {
    scroll: f32,
    content: f32,
    drag: Option<f32>,
    prev_down: bool,
}

thread_local! {
    static EXPO: RefCell<Expo> = RefCell::new(Expo::default());
}

/// The viewport the exposition scrolls in: below the nav strip, to the left
/// of the interactive column.
fn expo_viewport(area: Rect) -> Rect {
    Rect::new(area.left, NAV_H, area.right, area.bottom)
}

/// Applies this frame's wheel and thumb drag to the exposition's scroll
/// offset (clamped against last frame's content height) and returns it.
fn expo_scroll_begin(f: &Frame, area: Rect) -> f32 {
    EXPO.with(|e| {
        let mut e = e.borrow_mut();
        let view = expo_viewport(area);
        let viewport = view.bottom - view.top;
        let (mx, my) = f.mouse;
        let clicked = f.mouse_down && !e.prev_down;
        e.prev_down = f.mouse_down;
        if let Some(mut bar) = ScrollBar::from_content(false, e.content, viewport, e.scroll) {
            bar.expanded = true;
            let rail = bar.rail(&view);
            if clicked && rail.contains(mx, my) {
                match bar.part_at(rail, mx, my) {
                    Some(ScrollPart::Thumb) => e.drag = Some(my - bar.thumb_rect(rail).top),
                    Some(part) => {
                        bar.apply_part(part);
                    }
                    None => {}
                }
            }
            if !f.mouse_down {
                e.drag = None;
            }
            if let Some(grab) = e.drag {
                bar.drag_to(rail, my, grab);
            } else if view.contains(mx, my) && !rail.contains(mx, my) {
                // 100 DIP a notch, the web's wheel step; scroll_by rounds.
                bar.scroll_by(f.wheel.1 * WHEEL_NOTCH_DIP);
                if f.wheel.1 != 0.0 {
                    host::claim_wheel();
                }
            }
            e.scroll = bar.value() as f32;
        } else {
            e.scroll = 0.0;
        }
        e.scroll
    })
}

/// Records the content height and paints the exposition's scroll bar.
fn expo_scroll_end(c: &dyn Canvas, f: &Frame, area: Rect, bottom: f32) {
    EXPO.with(|e| {
        let mut e = e.borrow_mut();
        let view = expo_viewport(area);
        e.content = (bottom - NAV_H + MARGIN).max(0.0);
        if let Some(mut bar) =
            ScrollBar::from_content(false, e.content, view.bottom - view.top, e.scroll)
        {
            let rail = bar.rail(&view);
            let (mx, my) = f.mouse;
            bar.expanded = rail.contains(mx, my) || e.drag.is_some();
            let hot = bar.thumb_rect(rail).contains(mx, my) || e.drag.is_some();
            bar.paint(c, rail, WidgetState::REST.hot(hot).pressed(e.drag.is_some()));
        }
    })
}

// ═════════════════════════════════════════════════════════════════════════════
// Interactive column — the four primitives, driven by pointer, keyboard, wheel.
// ═════════════════════════════════════════════════════════════════════════════

/// Rows of the live scrolled list.
const LIST_ROWS: usize = 40;
/// One list row — also the scroll bar's `SmallChange`, so an arrow key or an
/// arrow button moves exactly one row.
const LIST_ROW_H: f32 = 24.0;
/// The live list's viewport height.
const LIST_H: f32 = 168.0;
/// The live numeric field's width and the domain field's.
const FIELD_W: f32 = 150.0;
/// Gap between a control and the next caption.
const BLOCK_GAP: f32 = 16.0;

/// What the interactive column remembers between frames.
struct Ui {
    /// Left button state last frame, for the click edge.
    prev_down: bool,

    /// Slider value, `0..=100`; also drives the progress bar under it.
    slider: i32,
    /// The slider is being dragged (the press started on it).
    slider_drag: bool,
    slider_wheel: WheelSteps,

    /// The numeric field's committed value.
    field: f64,
    /// The text being typed while the field is focused.
    edit: Option<NumericEdit>,
    /// A drag-selection started in the field's text.
    edit_drag: bool,
    /// When the field was last edited or focused — the caret's blink phase.
    last_input: u64,
    field_wheel: WheelSteps,

    /// The domain field's selected index.
    domain: i32,

    /// Scroll position of the live list, in DIP.
    scroll: f32,
    /// The scroll thumb is being dragged, and where it was grabbed.
    scroll_drag: bool,
    scroll_grab: f32,
    /// An arrow / track press being auto-repeated: the part, and when the next
    /// repeat is due.
    scroll_hold: Option<(ScrollPart, u64)>,
}

impl Default for Ui {
    fn default() -> Self {
        Self {
            prev_down: false,
            slider: 40,
            slider_drag: false,
            slider_wheel: WheelSteps::default(),
            field: 42.5,
            edit: None,
            edit_drag: false,
            last_input: 0,
            field_wheel: WheelSteps::default(),
            domain: 0,
            scroll: 0.0,
            scroll_drag: false,
            scroll_grab: 0.0,
            scroll_hold: None,
        }
    }
}

thread_local! {
    static UI: RefCell<Ui> = RefCell::new(Ui::default());
}

/// A small secondary line under a live control.
fn note(c: &dyn Canvas, left: f32, right: f32, y: f32, text: &str) -> f32 {
    c.text_ellipsis(text, &Rect::new(left, y, right, y + 16.0), &c.formats().caption, &c.theme().text_secondary);
    y + 16.0
}

/// The right-hand column: the same primitives as the page, but live.
pub fn interactive_column(c: &dyn Canvas, f: &Frame) {
    UI.with(|ui| {
        let mut ui = ui.borrow_mut();
        let live = Live::new(f, ui.prev_down);
        ui.prev_down = f.mouse_down;

        let (left, mut y, right) = interact::panel(c, interact::panel_rect(f.size));

        y = live_slider(c, f, &live, &mut ui, left, right, y);
        y = live_progress(c, &mut ui, left, right, y);
        y = live_numeric(c, &live, &mut ui, left, right, y);
        y = live_domain(c, &live, &mut ui, left, right, y);
        live_scroll(c, f, &live, &mut ui, left, right, y);
    });
}

/// Slider — drag (the host captures the mouse, so the drag follows outside the
/// window), click on the rail, ARIA keys, wheel; the value bubble floats in a
/// `host::overlay` while dragging, escaping the column and the window.
fn live_slider(c: &dyn Canvas, f: &Frame, live: &Live, ui: &mut Ui, left: f32, right: f32, y: f32) -> f32 {
    let (mx, my) = live.mouse;
    let y = interact::caption(c, left, right, y, "Slider — glisser, clic sur le rail, ←→↑↓ PgUp/PgDn Début/Fin, molette");
    let mut slider = Slider::new();
    slider.set_maximum(100);
    let _ = slider.set_large_change(10);
    let _ = slider.set_value(ui.slider);
    let h = slider.measure(c).height;
    // Room above for the bubble, which the web hangs over the track.
    let sl = Rect::new(left, y + 22.0, right, y + 22.0 + h);

    let hover = slider.hit_test(sl, mx, my);
    let focus = live.focus("range-slider", slider.hit_rect(sl));
    if live.clicked && hover {
        ui.slider_drag = true;
    }
    if !live.down || f.dismiss {
        ui.slider_drag = false;
    }
    if ui.slider_drag {
        slider.drag_to(sl, mx, my);
    }
    if focus.focused {
        for k in RangeKey::KEYS {
            let n = host::take_key(k, Modifiers::NONE);
            if let Some(key) = RangeKey::for_slider(k) {
                for _ in 0..n {
                    slider.apply_key(key);
                }
            }
        }
    }
    if hover {
        if live.wheel.1 != 0.0 {
            host::claim_wheel();
        }
        let steps = ui.slider_wheel.take(live.wheel.1);
        slider.apply_wheel(steps);
        host::set_cursor(Cursor::Hand);
    } else {
        ui.slider_wheel.reset();
    }
    if ui.slider_drag {
        host::set_cursor(Cursor::Hand);
    }
    ui.slider = slider.value();

    let state = focus.apply(WidgetState::REST.hot(hover || ui.slider_drag).pressed(ui.slider_drag));
    slider.paint(c, sl, state);

    // `showBubble = showValue || dragging` — a floating surface, so it is drawn
    // in its own top-level window and may hang over the window's edge.
    if ui.slider_drag {
        let text = format!("{} %", ui.slider);
        let b = slider.value_bubble_rect(c, sl, &text);
        let bounds = b.inflate(VALUE_BUBBLE_MARGIN, VALUE_BUBBLE_MARGIN);
        let local = Rect::new(
            VALUE_BUBBLE_MARGIN,
            VALUE_BUBBLE_MARGIN,
            VALUE_BUBBLE_MARGIN + (b.right - b.left),
            VALUE_BUBBLE_MARGIN + (b.bottom - b.top),
        );
        host::overlay(bounds, move |canvas| Slider::paint_value_bubble(canvas, local, &text));
    }
    let y = note(
        c,
        left,
        right,
        sl.bottom + 4.0,
        &format!("value = {} / {} · Tab pour le focus clavier", ui.slider, slider.maximum()),
    );
    y + BLOCK_GAP
}

/// ProgressBar — driven by the slider, with a truncating label and the value;
/// plus the indeterminate sliver animated at the web's 1.3 s ease-in-out.
fn live_progress(c: &dyn Canvas, ui: &mut Ui, left: f32, right: f32, y: f32) -> f32 {
    let y = interact::caption(c, left, right, y, "ProgressBar — pilotée par le slider (auto : ambre 75 %, rouge 90 %)");
    let slider_value = ui.slider;
    let mut bar = ProgressBar::new()
        .with_label("Espace utilisé par l'équipe documentation partagée")
        .with_value_shown(true);
    bar.set_value(slider_value);
    let h = bar.measure(c).height;
    bar.paint(c, Rect::new(left, y, right, y + h), WidgetState::REST);
    let y = y + h + 8.0;

    let mut busy = ProgressBar::new().with_size(ProgressSize::Sm);
    busy.set_indeterminate(true);
    busy.phase = ProgressBar::phase_at(host::now_ms());
    busy.paint(c, Rect::new(left, y, right, y + ProgressSize::Sm.track()), WidgetState::REST);
    // One frame of the animation at a time: the request is one-shot.
    host::request_repaint_after(16);
    y + ProgressSize::Sm.track() + BLOCK_GAP
}

/// NumericField — typing (digits, sign, decimal comma), caret and selection by
/// click / Shift+click / drag / double-click, arrows, Home/End, Backspace /
/// Delete, Ctrl+A/C/X/V, ↑↓ step, Enter commits, Escape reverts, focus loss
/// commits; spin buttons with hover, spent at the bounds; wheel while focused.
fn live_numeric(c: &dyn Canvas, live: &Live, ui: &mut Ui, left: f32, right: f32, y: f32) -> f32 {
    let (mx, my) = live.mouse;
    let y = interact::caption(c, left, right, y, "NumericField — saisie, ↑↓, Entrée, Échap, Ctrl+A/C/X/V, molette");
    let mut field = NumericField::ranged(0.0, 500.0);
    let _ = field.set_decimal_places(1);
    let _ = field.set_increment(0.5);
    field.clamped(ui.field);
    let r = Rect::new(left, y, left + FIELD_W, y + field.measure(c).height.max(36.0));
    let text_rect = field.text_rect(r);

    let st = live.focus_with("range-number", r, FocusOpts::TEXT);
    let spin = field.spin_part_at(r, mx, my);
    field.spin_hot = spin;
    if text_rect.contains(mx, my) || ui.edit_drag {
        host::set_cursor(Cursor::IBeam);
    }

    if st.focused {
        if ui.edit.is_none() || st.gained {
            ui.edit = Some(field.begin_edit());
            ui.last_input = host::now_ms();
        }
    } else if let Some(edit) = ui.edit.take() {
        // Focus left: validate what was typed (WinForms' ValidateEditText).
        field.commit(&edit);
        ui.edit_drag = false;
    }

    // Spin buttons: act, keeping the field focused and the text in sync.
    if let Some(part) = spin {
        if live.clicked && field.can_step(part) {
            if let Some(edit) = &ui.edit {
                field.commit(edit);
            }
            field.step(part);
            if st.focused {
                ui.edit = Some(field.begin_edit());
            }
        }
    }

    if let Some(edit) = ui.edit.as_mut() {
        // Pointer: place the caret, extend it with Shift or a drag, double-click
        // selects the whole number.
        if live.clicked && text_rect.contains(mx, my) {
            if live.click_count >= 2 {
                edit.select_all();
            } else {
                let at = field.caret_at(c, r, edit, mx);
                edit.set_caret(at, live.mods.shift);
                ui.edit_drag = true;
            }
            ui.last_input = host::now_ms();
        }
        if ui.edit_drag {
            if live.down {
                let at = field.caret_at(c, r, edit, mx);
                edit.set_caret(at, true);
            } else {
                ui.edit_drag = false;
            }
        }

        // Keyboard: peek every key, take only the ones the edit used.
        let hex = field.hexadecimal();
        let mut restart = false;
        for e in live.events() {
            let InputEvent::Key { vk: key, down: true, mods, .. } = e else { continue };
            let outcome = edit.handle_key(key, mods);
            if outcome == EditOutcome::Ignored {
                continue;
            }
            host::take_key(key, mods);
            ui.last_input = host::now_ms();
            match outcome {
                EditOutcome::Copy(s) => {
                    host::set_clipboard_text(&s);
                }
                EditOutcome::Cut(s) => {
                    host::set_clipboard_text(&s);
                }
                EditOutcome::Paste => {
                    if let Some(s) = host::clipboard_text() {
                        edit.insert(&s, hex);
                    }
                }
                EditOutcome::Commit | EditOutcome::Revert => {
                    if outcome == EditOutcome::Commit {
                        field.commit(edit);
                    }
                    restart = true;
                }
                EditOutcome::Step(part) => {
                    field.commit(edit);
                    field.step(part);
                    restart = true;
                }
                _ => {}
            }
        }
        let typed = live.take_text();
        if !typed.is_empty() {
            edit.insert(&typed, hex);
            ui.last_input = host::now_ms();
        }
        if live.hover(r) {
            if live.wheel.1 != 0.0 {
                host::claim_wheel();
            }
            let steps = ui.field_wheel.take(live.wheel.1);
            if steps != 0 {
                field.commit(edit);
                field.apply_wheel(steps);
                restart = true;
            }
        } else {
            ui.field_wheel.reset();
        }
        if restart {
            let mut fresh = field.begin_edit();
            let end = fresh.text().len();
            fresh.set_caret(end, false);
            *edit = fresh;
        }
    }

    ui.field = field.value();
    let state = st.apply(live.state(r));
    match &ui.edit {
        Some(edit) if st.focused => {
            let caret = live.window_focused && caret_visible(ui.last_input);
            field.paint_editing(c, r, state, edit, caret);
        }
        _ => field.paint(c, r, state),
    }
    let shown = match &ui.edit {
        Some(e) => match field.parse_text(e.text()) {
            Some(v) => format!("valeur = {} · saisie « {} » → {v}", field.display_text(), e.text()),
            None => format!("valeur = {} · saisie « {} » : pas un nombre", field.display_text(), e.text()),
        },
        None => format!("valeur = {} (0 à 500, pas de 0,5)", field.display_text()),
    };
    let y = note(c, left, right, r.bottom + 4.0, &shown);
    y + BLOCK_GAP
}

/// DomainField — spin buttons, ↑↓ and Début/Fin when focused.
fn live_domain(c: &dyn Canvas, live: &Live, ui: &mut Ui, left: f32, right: f32, y: f32) -> f32 {
    let (mx, my) = live.mouse;
    let y = interact::caption(c, left, right, y, "DomainField — ▲▼, ↑↓, Début/Fin");
    let mut d = DomainField::with_items(["Brouillon", "En relecture", "Validé", "Publié", "Archivé"]);
    let _ = d.set_selected_index(ui.domain);
    let r = Rect::new(left, y, left + FIELD_W, y + d.measure(c).height);
    let st = live.focus("range-domain", r);
    d.spin_hot = d.spin_part_at(r, mx, my);
    if let Some(part) = d.spin_hot {
        if live.clicked {
            d.step(part);
        }
    }
    if st.focused {
        for k in [vk::UP, vk::DOWN, vk::HOME, vk::END] {
            for _ in 0..host::take_key(k, Modifiers::NONE) {
                d.apply_key(k);
            }
        }
    }
    ui.domain = d.selected_index();
    d.paint(c, r, st.apply(live.state(r)));
    r.bottom + BLOCK_GAP
}

/// ScrollBar over a real list: drag the thumb (captured), press an arrow or the
/// track (auto-repeat at Windows' timing), wheel over the list, and with the
/// list focused ↑↓ PgUp/PgDn Début/Fin. The gutter unfolds under the pointer.
fn live_scroll(c: &dyn Canvas, f: &Frame, live: &Live, ui: &mut Ui, left: f32, right: f32, y: f32) {
    let (mx, my) = live.mouse;
    let t = c.theme();
    let y = interact::caption(c, left, right, y, "ScrollBar — poignée, flèches, piste, molette, clavier");
    let content = Rect::new(left, y, left + (right - left).min(260.0), y + LIST_H);
    let extent = LIST_ROWS as f32 * LIST_ROW_H;
    let viewport = content.bottom - content.top;
    let Some(mut bar) = ScrollBar::from_content(false, extent, viewport, ui.scroll) else { return };
    let _ = bar.set_small_change(LIST_ROW_H as i32);
    // The panel is rounded (4 DIP): the gutter keeps clear of its corners.
    let rail = kubuno_desktop_ui::range::fit_rail(bar.rail(&content), content, 4.0);
    let over_gutter = rail.contains(mx, my);
    bar.expanded = over_gutter || ui.scroll_drag || ui.scroll_hold.is_some();

    let focus = live.focus("range-list", content);
    let now = host::now_ms();

    if !live.down || f.dismiss {
        ui.scroll_drag = false;
        ui.scroll_hold = None;
    }
    if live.clicked {
        match bar.part_at(rail, mx, my) {
            Some(ScrollPart::Thumb) => {
                ui.scroll_drag = true;
                let thumb = bar.thumb_rect(rail);
                ui.scroll_grab = bar.along(mx, my) - thumb.top;
            }
            Some(part) => {
                bar.apply_part(part);
                ui.scroll_hold = Some((part, now + SCROLL_REPEAT_DELAY_MS));
            }
            None => {}
        }
    }
    if ui.scroll_drag {
        bar.drag_to(rail, bar.along(mx, my), ui.scroll_grab);
    }
    if let Some((part, due)) = ui.scroll_hold {
        if now >= due {
            // A track press stops once the thumb has reached the pointer: the
            // part under the pointer is no longer the one held.
            let still = match part {
                ScrollPart::PageLow | ScrollPart::PageHigh => bar.part_at(rail, mx, my) == Some(part),
                _ => true,
            };
            if still && bar.apply_part(part) {
                ui.scroll_hold = Some((part, now + SCROLL_REPEAT_INTERVAL_MS));
            } else {
                ui.scroll_hold = None;
            }
        }
        if let Some((_, due)) = ui.scroll_hold {
            host::request_repaint_after(due.saturating_sub(now).clamp(1, u32::MAX as u64) as u32);
        }
    }
    if content.contains(mx, my) {
        let (_, dy) = live.wheel_over(content);
        bar.scroll_by(dy);
    }
    if focus.focused {
        for k in RangeKey::KEYS {
            let n = host::take_key(k, Modifiers::NONE);
            if let Some(key) = RangeKey::for_scroll(k) {
                for _ in 0..n {
                    bar.apply_key(key);
                }
            }
        }
    }
    ui.scroll = bar.value() as f32;

    // The list itself, scrolled and clipped to its panel.
    content_backdrop(c, content);
    c.push_clip_rounded(&content, 4.0);
    let first = (ui.scroll / LIST_ROW_H).floor() as usize;
    for i in first..LIST_ROWS {
        let top = content.top + i as f32 * LIST_ROW_H - ui.scroll;
        if top > content.bottom {
            break;
        }
        let row = Rect::new(content.left + 12.0, top, rail.left - 4.0, top + LIST_ROW_H);
        c.text_ellipsis(&format!("Ligne {} — document de l'équipe", i + 1), &row, &c.formats().body, &t.text_primary);
    }
    let thumb_hot = bar.thumb_rect(rail).contains(mx, my) || ui.scroll_drag;
    bar.paint(c, rail, WidgetState::REST.hot(thumb_hot).pressed(ui.scroll_drag));
    c.pop_clip_rounded();
    if focus.visible {
        c.stroke_rounded_w(&content.inflate(2.0, 2.0), 6.0, &t.accent, 2.0);
    }
    note(
        c,
        left,
        right,
        content.bottom + 6.0,
        &format!("scroll = {} / {} DIP", ui.scroll as i32, (extent - viewport) as i32),
    );
}
