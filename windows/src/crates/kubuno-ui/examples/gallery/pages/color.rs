//! Gallery page — the colour family.
//!
//! None of these primitives has a hand-written predecessor in
//! `drive-app-controls`, so there is no [`super::sheet::pair`] here. The page
//! shows every state the family can be in — a field closed, open, dead,
//! translucent and focused from the keyboard, and the gradient field; then
//! the three panels in exactly the state the web's own component gallery
//! (`core/admin/theme/groups/PickersGroup.tsx`) and the reference captures
//! show them: the full picker on `#4a90d9` with twelve recent colours, the
//! quick picker on `#3c78d8` with three custom colours, a four-stop gradient
//! at 120°; and, last, the family nested in a narrow `Panel`.
//!
//! The pointer drives the hover through each panel's own hit-testing.
//!
//! The live column is the real thing: each `ColorField` opens its panel in a
//! host POPUP placed by `ColorField::popover_rect` (to the LEFT of the swatch,
//! as the web prefers, clamped to the monitor). The picker's own interaction
//! loop (`ColorPicker::pointer` / `ColorPicker::keyboard`) runs it: ring, SV
//! area in its three shapes, harmony schemes and swatches, hex, the five
//! channel models, chips, recent colours and the screen eyedropper. A press
//! outside closes the popover and is swallowed, as the web's backdrop
//! swallows it; Escape closes it and gives the focus back to the field.

use std::cell::RefCell;

use kubuno_controls::host::{self, vk, Cursor, Frame, Modifiers};
use kubuno_ui::color::{
    m, parse, picker_swatches, take_color_keys, Color, ColorField, ColorPicker, DraftOutcome, Gradient,
    GradientField, GradientKind, GradientPart, GradientPicker, GradientStop, PickerEvent, PickerPointer, SwatchPicker,
};
use kubuno_ui::containers::{Panel, Surface};
use kubuno_ui::focus::{caret_visible, FocusOpts};
use kubuno_ui::{Canvas, Rect, Widget, WidgetState};

use super::interact::{self, Live};
use super::sheet::{cells, Page, CAPTION_H, MARGIN};

/// The gap between two cells on this page.
const GAP: f32 = 16.0;
/// The six states the field row shows.
const FIELD_STATES: usize = 6;
/// Narrower than this and a panel stops being readable, so the row breaks
/// instead of squeezing.
const MIN_PANEL: f32 = 180.0;
/// The narrow panel the last section nests the family in.
const NARROW_W: f32 = 180.0;

pub fn draw(c: &dyn Canvas, f: &Frame) {
    let w = f.size.0 - interact::PANEL_W();
    let mut page = Page::new(c, w, f.size.1);
    let (mx, my) = f.mouse;
    let right = w - MARGIN;
    let avail = right - MARGIN;

    // ── ColorField · GradientField ───────────────────────────────────────────
    page.section("ColorField · GradientField");
    let top = page.caption("fermé · ouvert (bordure accent) · désactivé · semi-transparent · focus clavier · dégradé");
    // Fixed steps, as the web gallery lays its fields out (`flex gap-…`),
    // rather than stretched over the page.
    let field_w = (m::FIELD_W + 3.0 * GAP).min(((avail - (FIELD_STATES as f32 - 1.0) * GAP) / FIELD_STATES as f32).max(m::FIELD_W));
    let row = cells(MARGIN, top, field_w, m::FIELD_H, GAP, FIELD_STATES);
    let at = |cell: Rect, w: f32, h: f32| Rect::new(cell.left, cell.top, cell.left + w, cell.top + h);

    let closed = ColorField::new(sample());
    closed.paint(c, at(row[0], closed.width, closed.height), WidgetState::REST);
    let opened = ColorField::new(sample()).open(true);
    opened.paint(c, at(row[1], opened.width, opened.height), WidgetState::REST);
    let mut dead = ColorField::new(sample());
    dead.enabled = false;
    dead.paint(c, at(row[2], dead.width, dead.height), WidgetState::REST);
    let translucent = ColorField::new(Color::new(sample().rgb, 45.0));
    translucent.paint(c, at(row[3], translucent.width, translucent.height), WidgetState::REST);
    let keyed = ColorField::new(parse("#f9ab00").unwrap_or_default());
    keyed.paint(c, at(row[4], keyed.width, keyed.height), WidgetState::REST.focused(true).focus_visible(true));
    let gfield = GradientField::new(reference_gradient());
    gfield.paint(c, at(row[5], gfield.width, gfield.height), WidgetState::REST);
    page.advance(m::FIELD_H);

    // ── The three panels ─────────────────────────────────────────────────────
    page.section("ColorPicker · SwatchPicker · GradientPicker");
    let top = page.caption("états de référence de la galerie web : #4a90d9 + 12 récentes · #3c78d8 + 3 personnalisées · 4 arrêts à 120°");

    let picker = reference_picker();
    let swatches = reference_swatches();
    let gradient = GradientPicker::new(reference_gradient());

    let wanted = [picker.measure(c).width, swatches.measure(c).width, gradient.measure(c).width];
    let column = (avail - 2.0 * GAP) / 3.0;
    let side_by_side = column >= MIN_PANEL && wanted.iter().sum::<f32>() + 2.0 * GAP <= avail;
    let widths: Vec<f32> = wanted.iter().map(|w| w.min(avail)).collect();
    let heights = [
        picker.height_for_width(widths[0]),
        swatches.height_for_width(widths[1]),
        gradient.height_for_width(widths[2]),
    ];
    let mut x = MARGIN;
    let mut y = top;
    let mut rects = Vec::with_capacity(3);
    for i in 0..3 {
        if !side_by_side && i > 0 {
            // Two columns: the picker on the left, the other two stacked.
            if i == 1 {
                x = MARGIN + widths[0] + GAP;
                y = top;
            } else {
                y += heights[i - 1] + GAP;
            }
            if x + widths[i] > right {
                x = MARGIN;
                y = rects.iter().map(|r: &Rect| r.bottom).fold(top, f32::max) + GAP;
            }
        }
        rects.push(Rect::new(x, y, x + widths[i], y + heights[i]));
        if side_by_side {
            x += widths[i] + GAP;
        }
    }

    let mut picker = picker;
    picker.hot = picker.part_at(rects[0], mx, my);
    picker.hot_recent = match picker.hot {
        Some(kubuno_ui::color::PickerPart::Recent(i)) => Some(i),
        _ => None,
    };
    picker.paint(c, rects[0], WidgetState::REST);

    let mut swatches = swatches;
    if let Some(i) = swatches.cell_at(rects[1], mx, my) {
        if i < swatches.colors.len() {
            swatches.hot = Some(i);
        } else {
            swatches.hot_custom = Some(i - swatches.colors.len());
        }
    }
    swatches.paint(c, rects[1], WidgetState::REST);

    let mut gradient = gradient;
    gradient.hot_stop = gradient.stop_at(rects[2], mx, my);
    gradient.paint(c, rects[2], WidgetState::REST);

    // The value the gradient names, printed under it: `gradientToCss`,
    // character for character.
    let css = gradient.gradient.to_css();
    let strip = Rect::new(rects[2].left, rects[2].bottom + super::sheet::ROW_GAP, w - MARGIN, rects[2].bottom + super::sheet::ROW_GAP + CAPTION_H);
    c.text_ellipsis(&css, &strip, &c.formats().caption, &c.theme().text_tertiary);

    let bottom = rects.iter().map(|r| r.bottom).fold(top, f32::max);
    page.advance(bottom - top + super::sheet::ROW_GAP + CAPTION_H);

    // ── Nesting ──────────────────────────────────────────────────────────────
    if page.y + 120.0 > f.size.1 {
        return;
    }
    page.section("Imbrication — dans un Panel étroit");
    let top = page.caption("le panneau rapide réduit à 180 DIP : la grille se resserre, rien ne déborde du Panel");
    let mut narrow = SwatchPicker::new();
    narrow.colors = picker_swatches();
    narrow.custom = ["#4a90d9", "#9b59b6"].iter().filter_map(|h| parse(h)).collect();
    narrow.select(sample());
    let inner_h = narrow.height_for_width(NARROW_W);
    let pad = kubuno_ui::metrics::space::MD;
    let host_rect = Rect::new(MARGIN, top, MARGIN + NARROW_W + 2.0 * pad + m::FIELD_W + pad, top + inner_h + 2.0 * pad);
    Panel::new().with_surface(Surface::Layer).paint(c, host_rect, WidgetState::REST);
    let field_rect = Rect::new(host_rect.left + pad, top + pad, host_rect.left + pad + m::FIELD_W, top + pad + m::FIELD_H);
    ColorField::new(sample()).paint(c, field_rect, WidgetState::REST);
    let sw_rect = Rect::new(field_rect.right + pad, top + pad, field_rect.right + pad + NARROW_W, top + pad + inner_h);
    narrow.paint(c, sw_rect, WidgetState::REST);
}

// ═════════════════════════════════════════════════════════════════════════════
// The live column
// ═════════════════════════════════════════════════════════════════════════════

/// Which floating panel is open — one at a time, as the web's backdrop allows.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Open {
    None,
    /// The « Couleur » field's full `ColorPicker`.
    Colour,
    /// The « Surlignage » field's quick `SwatchPicker` (or, after `+`, its
    /// custom `ColorPicker` with the Cancel / Add footer).
    Swatches,
    /// The gradient's selected stop, in a `ColorPicker`.
    Stop,
    /// The « Dégradé » `GradientField`'s `GradientPicker`.
    Gradient,
}

/// What the gradient panel is being dragged by.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum GradDrag {
    Stop(usize),
    Angle,
    Opacity,
}

/// What the interactive column remembers between frames.
struct Ui {
    prev_down: bool,
    open: Open,
    colour: Color,
    picker: ColorPicker,
    highlight: Color,
    swatches: SwatchPicker,
    /// `customOpen` in `ColorSwatchPicker`.
    custom_open: bool,
    custom: ColorPicker,
    gradient: GradientPicker,
    stop_picker: ColorPicker,
    grad_drag: Option<GradDrag>,
    /// The « Dégradé » field's value, and its popover picker.
    field_gradient: Gradient,
    field_picker: GradientPicker,
    field_drag: Option<GradDrag>,
}

impl Default for Ui {
    fn default() -> Self {
        let colour = parse("#4a90d9").unwrap_or_default();
        let mut picker = ColorPicker::new(colour);
        picker.recent = ["#d93025", "#f9ab00", "#1e8e3e", "#1a73e8", "#9b51e0"].iter().filter_map(|h| parse(h)).collect();
        let highlight = parse("#ffe599").unwrap_or_default();
        let mut swatches = SwatchPicker::new();
        swatches.select(highlight);
        Ui {
            prev_down: false,
            open: Open::None,
            colour,
            picker,
            highlight,
            swatches,
            custom_open: false,
            custom: ColorPicker::new(highlight).with_footer("Annuler", "Ajouter"),
            gradient: GradientPicker::new(Gradient::default()),
            stop_picker: ColorPicker::new(Color::default()),
            grad_drag: None,
            field_gradient: reference_gradient(),
            field_picker: GradientPicker::new(reference_gradient()),
            field_drag: None,
        }
    }
}

thread_local! {
    static UI: RefCell<Ui> = RefCell::new(Ui::default());
}

const ID_COLOUR: &str = "color-field";
const ID_HIGHLIGHT: &str = "highlight-field";
const ID_GRADIENT: &str = "gradient-field";

/// The right-hand column: two `ColorField`s and a `GradientField` whose
/// panels open in popups, and a live `GradientPicker` whose stop field opens
/// a fourth.
pub fn interactive_column(c: &dyn Canvas, f: &Frame) {
    UI.with(|ui| {
        let mut ui = ui.borrow_mut();
        let live = Live::new(f, ui.prev_down);
        ui.prev_down = f.mouse_down;

        let (left, mut y, right) = interact::panel(c, interact::panel_rect(f.size));

        let fields_caption = y;
        y += 20.0;
        let colour_rect = Rect::new(left, y, left + m::FIELD_W, y + m::FIELD_H);
        let label_w = 96.0;
        let hl_left = (left + m::FIELD_W + space_md() + label_w).min(right - m::FIELD_W);
        let highlight_rect = Rect::new(hl_left, y, hl_left + m::FIELD_W, y + m::FIELD_H);
        let gf_left = (hl_left + m::FIELD_W + space_md() + label_w).min(right - m::FIELD_W);
        let gfield_rect = Rect::new(gf_left, y, gf_left + m::FIELD_W, y + m::FIELD_H);
        y += m::FIELD_H + 20.0;
        let grad_caption = y;
        y += 20.0;
        let gw = ui.gradient.measure(c).width.min(right - left);
        let gh = ui.gradient.height_for_width(gw);
        let grect = Rect::new(left, y, left + gw, y + gh);

        // The eyedropper runs whatever the pointer does; while it is armed,
        // or until the press that sampled is released, clicks go nowhere.
        let picking = poll_eyedroppers(&mut ui);

        if f.dismiss && !picking {
            close(&mut ui, false);
        }
        let panel = open_panel(c, f, &ui, colour_rect, highlight_rect, gfield_rect, grect);

        // A press while a panel is open: inside, it is the panel's; outside,
        // it closes the panel and goes nowhere else.
        let mut routed_click = false;
        if let (Some(p), true, false) = (panel, live.clicked, picking) {
            if p.contains(live.mouse.0, live.mouse.1) {
                routed_click = true;
            } else {
                close(&mut ui, false);
            }
        }
        let mut under = live;
        if panel.is_some() || picking {
            under.clicked = false;
            if panel.is_some_and(|p| p.contains(live.mouse.0, live.mouse.1)) {
                under.mouse = (host::POINTER_AWAY, host::POINTER_AWAY);
            }
        }
        let panel = open_panel(c, f, &ui, colour_rect, highlight_rect, gfield_rect, grect);

        // ── The fields ───────────────────────────────────────────────────────
        caption(c, left, right, fields_caption, "ColorField · GradientField — clic, Entrée ou Espace : panneau flottant");

        let colour = ui.colour;
        if field(c, &under, ID_COLOUR, colour_rect, colour, ui.open == Open::Colour) {
            toggle(&mut ui, Open::Colour);
        }
        text_beside(c, colour_rect, "Couleur");
        if let (Open::Colour, Some(p)) = (ui.open, panel) {
            let ev = run_picker(c, &live, f, PickerSlot::Colour, &mut ui, p, routed_click);
            ui.colour = ui.picker.color();
            if matches!(ev, PickerEvent::Close) {
                close(&mut ui, true);
            }
        }

        let highlight = ui.highlight;
        if field(c, &under, ID_HIGHLIGHT, highlight_rect, highlight, ui.open == Open::Swatches) {
            toggle(&mut ui, Open::Swatches);
        }
        text_beside(c, highlight_rect, "Surlignage");
        if let (Open::Swatches, Some(p)) = (ui.open, panel) {
            if ui.custom_open {
                match run_picker(c, &live, f, PickerSlot::Custom, &mut ui, p, routed_click) {
                    PickerEvent::Confirm => {
                        // `addCustom(hex); onChange(hex); setCustomOpen(false)`.
                        let chosen = ui.custom.color();
                        ui.swatches.add_custom(chosen);
                        ui.highlight = chosen;
                        ui.swatches.select(chosen);
                        back_to_grid(&mut ui);
                    }
                    PickerEvent::Cancel | PickerEvent::Close => back_to_grid(&mut ui),
                    _ => {}
                }
            } else {
                run_swatches(c, &live, &mut ui, p, routed_click);
            }
        }

        let gvalue = ui.field_gradient.clone();
        let ws = under.focus_state(ID_GRADIENT, gfield_rect);
        if under.hover(gfield_rect) {
            host::set_cursor(Cursor::Hand);
        }
        GradientField::new(gvalue).open(ui.open == Open::Gradient).paint(c, gfield_rect, ws);
        if under.hit(gfield_rect)
            || (ws.focused && (under.take_key(vk::ENTER, Modifiers::NONE) || under.take_key(vk::SPACE, Modifiers::NONE)))
        {
            toggle(&mut ui, Open::Gradient);
        }
        text_beside(c, gfield_rect, "Dégradé");
        if let (Open::Gradient, Some(p)) = (ui.open, panel) {
            let escaped = live.take_escape();
            let Ui { field_picker, field_drag, .. } = &mut *ui;
            let closed = run_gradient(c, &live, f.pointer_outside(), field_picker, field_drag, p, routed_click, "gf", false);
            ui.field_gradient = ui.field_picker.gradient.clone();
            if closed || escaped {
                close(&mut ui, true);
            }
        }

        // ── The gradient builder ─────────────────────────────────────────────
        caption(c, left, right, grad_caption, "GradientPicker — arrêts (glisser, ←/→, ↑/↓), barre, curseurs, cases, couleur");
        {
            let Ui { gradient, grad_drag, .. } = &mut *ui;
            let clicked = under.clicked;
            run_gradient(c, &under, f.pointer_outside(), gradient, grad_drag, grect, clicked, "grad", true);
        }
        if ui.gradient.hot == Some(GradientPart::Field) && under.clicked {
            toggle(&mut ui, Open::Stop);
        }
        if let (Open::Stop, Some(p)) = (ui.open, panel) {
            let ev = run_picker(c, &live, f, PickerSlot::Stop, &mut ui, p, routed_click);
            let chosen = ui.stop_picker.color();
            let sel = ui.gradient.selected_index();
            if let Some(stop) = ui.gradient.gradient.stops.get_mut(sel) {
                stop.color = chosen.rgb;
            }
            if matches!(ev, PickerEvent::Close) {
                close(&mut ui, true);
            }
        }
        ui.gradient.field_open = ui.open == Open::Stop;
        ui.gradient.paint(c, grect, WidgetState::REST);

        let css = ui.gradient.gradient.to_css();
        let css_top = grect.bottom + 8.0;
        c.text_ellipsis(&css, &Rect::new(left, css_top, right, css_top + CAPTION_H), &c.formats().caption, &c.theme().text_tertiary);

        if let Some(p) = panel {
            emit_popup(&ui, p);
        }
    });
}

/// Runs every eyedropper that may be armed; returns whether pointer presses
/// must be swallowed this frame.
fn poll_eyedroppers(ui: &mut Ui) -> bool {
    let mut swallow = false;
    for p in [&mut ui.picker, &mut ui.custom, &mut ui.stop_picker] {
        p.poll_eyedropper();
        swallow |= p.swallows_pointer();
    }
    if let Some(rgb) = ui.swatches.eye.poll() {
        // `addCustom(r.sRGBHex); onChange(r.sRGBHex); onClose()`.
        let chosen = Color::opaque(rgb);
        ui.swatches.add_custom(chosen);
        ui.highlight = chosen;
        ui.swatches.select(chosen);
        ui.open = Open::None;
    }
    swallow || ui.swatches.eye.swallows_pointer()
}

fn caption(c: &dyn Canvas, left: f32, right: f32, y: f32, label: &str) {
    c.text_ellipsis(label, &Rect::new(left, y, right, y + 16.0), &c.formats().caption, &c.theme().text_secondary);
}

fn space_md() -> f32 {
    kubuno_ui::metrics::space::MD
}

fn text_beside(c: &dyn Canvas, field: Rect, label: &str) {
    let r = Rect::new(field.right + space_md(), field.top, field.right + space_md() + 90.0, field.bottom);
    c.text_ellipsis(label, &r, &c.formats().body, &c.theme().text_secondary);
}

/// One `ColorField`: registers its focus, paints it, and says whether it was
/// activated this frame — a click, or Enter / Space while it has the focus.
fn field(c: &dyn Canvas, live: &Live, id: &str, rect: Rect, colour: Color, open: bool) -> bool {
    let ws = live.focus_state(id, rect);
    if live.hover(rect) {
        host::set_cursor(Cursor::Hand);
    }
    ColorField::new(colour).open(open).paint(c, rect, ws);
    live.hit(rect) || (ws.focused && (live.take_key(vk::ENTER, Modifiers::NONE) || live.take_key(vk::SPACE, Modifiers::NONE)))
}

/// Toggles `which` (`setOpen(v => !v)`), closing whatever else was open.
fn toggle(ui: &mut Ui, which: Open) {
    if ui.open == which {
        close(ui, false);
        return;
    }
    ui.open = which;
    match which {
        Open::Colour => {
            let c = ui.colour;
            ui.picker.set_color(c);
        }
        Open::Swatches => {
            ui.custom_open = false;
            let h = ui.highlight;
            ui.swatches.select(h);
        }
        Open::Stop => {
            if let Some(stop) = ui.gradient.selected_stop() {
                ui.stop_picker = ColorPicker::new(Color::opaque(stop.color));
            }
        }
        Open::Gradient => {
            let mut p = GradientPicker::new(ui.field_gradient.clone());
            p.closable = true;
            ui.field_picker = p;
        }
        Open::None => {}
    }
}

/// Closes the open panel. `keyboard` gives the focus back to the field that
/// opened it, visibly.
fn close(ui: &mut Ui, keyboard: bool) {
    let owner = ui.open;
    if ui.open == Open::Colour {
        // The web's `history` is the caller's: this page keeps the last
        // colours chosen, newest first, as a module would.
        let chosen = ui.colour;
        ui.picker.recent.retain(|c| !c.same_swatch(chosen));
        ui.picker.recent.insert(0, chosen);
        ui.picker.recent.truncate(m::RECENT_MAX);
    }
    ui.open = Open::None;
    ui.custom_open = false;
    ui.picker.end_edit();
    ui.custom.end_edit();
    ui.stop_picker.end_edit();
    if keyboard {
        match owner {
            Open::Colour => interact::with_focus(|r| r.focus_visibly(ID_COLOUR)),
            Open::Swatches => interact::with_focus(|r| r.focus_visibly(ID_HIGHLIGHT)),
            Open::Gradient => interact::with_focus(|r| r.focus_visibly(ID_GRADIENT)),
            Open::Stop => interact::with_focus(|r| r.focus_visibly(("grad", GradientPart::Field.focus_slot()))),
            Open::None => {}
        }
    }
}

fn back_to_grid(ui: &mut Ui) {
    ui.custom_open = false;
    ui.custom.end_edit();
}

/// The open panel's rectangle this frame, in client DIP — the web's
/// `reposition()`, against the monitor's work area.
fn open_panel(c: &dyn Canvas, f: &Frame, ui: &Ui, colour: Rect, highlight: Rect, gfield: Rect, grect: Rect) -> Option<Rect> {
    let area = f.screen_area();
    let (anchor, size) = match ui.open {
        Open::None => return None,
        Open::Colour => (colour, panel_size(c, &ui.picker)),
        Open::Stop => (ui.gradient.layout(grect).field, panel_size(c, &ui.stop_picker)),
        Open::Swatches if ui.custom_open => (highlight, panel_size(c, &ui.custom)),
        Open::Swatches => {
            let w = ui.swatches.measure(c).width;
            (highlight, (w, ui.swatches.height_for_width(w)))
        }
        Open::Gradient => {
            let w = ui.field_picker.measure(c).width;
            (gfield, (w, ui.field_picker.height_for_width(w)))
        }
    };
    Some(ColorField::popover_rect(anchor, size, area))
}

fn panel_size(c: &dyn Canvas, p: &ColorPicker) -> (f32, f32) {
    let w = p.measure(c).width;
    (w, p.height_for_width(w))
}

/// Paints the open panel in a host popup, with room for its shadow.
fn emit_popup(ui: &Ui, panel: Rect) {
    let (l, t, r, b) = kubuno_ui::datetime::shadow_outset(&kubuno_ui::datetime::SHADOW_2XL);
    let bounds = Rect::new(panel.left - l, panel.top - t, panel.right + r, panel.bottom + b);
    let local = Rect::new(l, t, l + (panel.right - panel.left), t + (panel.bottom - panel.top));
    match ui.open {
        Open::Colour => {
            let p = ui.picker.clone();
            host::popup(bounds, move |canvas| p.paint(canvas, local, WidgetState::REST));
        }
        Open::Stop => {
            let p = ui.stop_picker.clone();
            host::popup(bounds, move |canvas| p.paint(canvas, local, WidgetState::REST));
        }
        Open::Swatches if ui.custom_open => {
            let p = ui.custom.clone();
            host::popup(bounds, move |canvas| p.paint(canvas, local, WidgetState::REST));
        }
        Open::Swatches => {
            let s = ui.swatches.clone();
            host::popup(bounds, move |canvas| s.paint(canvas, local, WidgetState::REST));
        }
        Open::Gradient => {
            let g = ui.field_picker.clone();
            host::popup(bounds, move |canvas| g.paint(canvas, local, WidgetState::REST));
        }
        Open::None => {}
    }
    interact::with_focus(|r| r.keep_focus_in(panel));
}

#[derive(Clone, Copy)]
enum PickerSlot {
    Colour,
    Custom,
    Stop,
}

impl PickerSlot {
    fn id(self) -> &'static str {
        match self {
            Self::Colour => "cp-colour",
            Self::Custom => "cp-custom",
            Self::Stop => "cp-stop",
        }
    }
}

fn picker_of(ui: &mut Ui, slot: PickerSlot) -> &mut ColorPicker {
    match slot {
        PickerSlot::Colour => &mut ui.picker,
        PickerSlot::Custom => &mut ui.custom,
        PickerSlot::Stop => &mut ui.stop_picker,
    }
}

/// One frame of a `ColorPicker` in a popup, through the picker's own
/// interaction loop: the pointer, the focus, the keyboard.
fn run_picker(c: &dyn Canvas, live: &Live, f: &Frame, slot: PickerSlot, ui: &mut Ui, panel: Rect, click: bool) -> PickerEvent {
    let p = picker_of(ui, slot);
    // Escape closes the popover — unless it cancels an armed eyedropper.
    if !p.is_picking() && live.take_escape() {
        return PickerEvent::Close;
    }
    let pointer = PickerPointer { x: live.mouse.0, y: live.mouse.1, down: live.down, pressed: click, away: f.pointer_outside() };
    let ev = p.pointer(c, panel, pointer);

    p.focus = None;
    p.focus_visible = false;
    for (part, rect) in p.tab_stops(panel) {
        let opts = if part.is_text() { FocusOpts::TEXT } else { FocusOpts::default() };
        let st = live.focus_with((slot.id(), part.focus_slot()), rect, opts);
        if st.focused {
            p.focus = Some(part);
            p.focus_visible = st.visible;
        }
    }
    let kev = p.keyboard(panel, live.window_focused);
    if ev != PickerEvent::None {
        ev
    } else {
        kev
    }
}

/// One frame of the quick `SwatchPicker` in its popup.
fn run_swatches(_c: &dyn Canvas, live: &Live, ui: &mut Ui, panel: Rect, click: bool) {
    if !ui.swatches.eye.is_picking() && live.take_escape() {
        close(ui, true);
        return;
    }
    let (mx, my) = live.mouse;
    let n = ui.swatches.colors.len();
    let hovered = ui.swatches.cell_at(panel, mx, my);
    ui.swatches.hot = hovered.filter(|&i| i < n);
    ui.swatches.hot_custom = hovered.filter(|&i| i >= n).map(|i| i - n);

    let mut activate = if click { hovered } else { None };
    if let Some(i) = activate {
        ui.swatches.focus = Some(i);
    }
    let grid = ui.swatches.content(panel);
    let st = live.focus(("sw-grid", 0), grid);
    ui.swatches.focus_visible = st.visible;
    if st.focused {
        let cur = ui.swatches.focus.unwrap_or_else(|| ui.swatches.selected.unwrap_or(0));
        let mut cur = cur.min(ui.swatches.cell_count() - 1);
        for (k, _) in take_color_keys() {
            cur = ui.swatches.step_cursor(cur, k);
        }
        ui.swatches.focus = Some(cur);
        if live.take_key(vk::ENTER, Modifiers::NONE) || live.take_key(vk::SPACE, Modifiers::NONE) {
            activate = Some(cur);
        }
    } else if !click {
        ui.swatches.focus = None;
    }

    if let Some(i) = activate {
        if i == ui.swatches.add_index() {
            // `openCustom`: the full picker, on a draft of the current colour.
            ui.custom.set_color(ui.highlight);
            ui.custom_open = true;
        } else if Some(i) == ui.swatches.eyedropper_index() {
            ui.swatches.eye.arm();
        } else if let Some(col) = ui.swatches.colour_at(i) {
            // `onChange(c); onClose()`.
            ui.highlight = col;
            ui.swatches.select(col);
            close(ui, st.focused && !click);
        }
    }
}

/// One frame of a `GradientPicker`. Returns whether its ✕ was used.
#[allow(clippy::too_many_arguments)]
fn run_gradient(
    c: &dyn Canvas,
    live: &Live,
    away: bool,
    gp: &mut GradientPicker,
    drag: &mut Option<GradDrag>,
    grect: Rect,
    click: bool,
    id: &'static str,
    field_opens: bool,
) -> bool {
    let (mx, my) = live.mouse;
    let g = gp.layout(grect);
    let mut closed = false;
    let hovered = if drag.is_some() { None } else { gp.part_at(grect, mx, my) };
    gp.hot = hovered;
    match hovered {
        Some(GradientPart::Stop(_)) => host::set_cursor(Cursor::ResizeEW),
        Some(GradientPart::Field | GradientPart::Angle | GradientPart::Opacity) => host::set_cursor(Cursor::Hand),
        Some(part) if part.is_text() => host::set_cursor(Cursor::IBeam),
        _ => {}
    }
    if click {
        match hovered {
            Some(GradientPart::Stop(i)) => {
                gp.selected = i;
                *drag = Some(GradDrag::Stop(i));
            }
            Some(GradientPart::Bar) => {
                let p = gp.position_at(grect, mx);
                gp.selected = gp.gradient.add_stop(p);
            }
            Some(GradientPart::Add) => gp.selected = gp.gradient.add_stop(0.5),
            Some(GradientPart::Linear) => gp.gradient.kind = GradientKind::Linear,
            Some(GradientPart::Radial) => gp.gradient.kind = GradientKind::Radial,
            Some(GradientPart::Close) => closed = true,
            Some(GradientPart::Bin) => {
                gp.remove_selected();
            }
            Some(GradientPart::Angle) => {
                if g.angle.is_some_and(|a| GradientPicker::slider_track(a).contains(mx, my)) {
                    *drag = Some(GradDrag::Angle);
                }
            }
            Some(GradientPart::Opacity) => {
                if GradientPicker::slider_track(g.opacity).contains(mx, my) {
                    *drag = Some(GradDrag::Opacity);
                }
            }
            Some(part) if part.is_text() => {
                gp.begin_edit(part);
                let boxed = match part {
                    GradientPart::AngleBox => g.angle.map(GradientPicker::row_box).unwrap_or(g.position),
                    GradientPart::OpacityBox => GradientPicker::row_box(g.opacity),
                    _ => GradientPicker::position_box(g.position),
                };
                let text_left = boxed.left + m::NUM_PAD_X;
                if let Some((_, d)) = gp.edit.as_mut() {
                    let shown = d.text.clone();
                    let at = d.index_at(mx - text_left, |s| c.measure(&shown[..s.len()], &c.formats().caption));
                    d.move_to(at, false);
                }
            }
            _ => {}
        }
    }
    if !live.down {
        *drag = None;
    }
    if !away {
        match *drag {
            Some(GradDrag::Stop(i)) => {
                let p = gp.position_at(grect, mx);
                if let Some(st) = gp.gradient.stops.get_mut(i) {
                    st.position = p;
                }
            }
            Some(GradDrag::Angle) => {
                if let Some(a) = gp.angle_at(grect, mx) {
                    gp.gradient.angle = a;
                }
            }
            Some(GradDrag::Opacity) => {
                let v = gp.opacity_at(grect, mx);
                let sel = gp.selected_index();
                if let Some(st) = gp.gradient.stops.get_mut(sel) {
                    st.opacity = v;
                }
            }
            None => {}
        }
    }
    gp.hot_stop = match *drag {
        Some(GradDrag::Stop(i)) => Some(i),
        _ => gp.stop_at(grect, mx, my),
    };

    gp.focus = None;
    gp.focus_visible = false;
    for (part, rect) in gp.tab_stops(grect) {
        let opts = if part.is_text() { FocusOpts::TEXT } else { FocusOpts::default() };
        let st = live.focus_with((id, part.focus_slot()), rect, opts);
        if st.focused {
            gp.focus = Some(part);
            gp.focus_visible = st.visible;
        }
    }
    match gp.focus {
        Some(part) if part.is_text() => {
            if gp.draft(part).is_none() {
                gp.begin_edit(part);
            }
            let outcome = gp.edit.as_mut().map(|(_, d)| d.take_input()).unwrap_or(DraftOutcome::Idle);
            match outcome {
                DraftOutcome::Edited => {
                    gp.apply_edit();
                }
                DraftOutcome::Commit | DraftOutcome::Cancel => {
                    if outcome == DraftOutcome::Commit {
                        gp.apply_edit();
                    }
                    gp.begin_edit(part);
                }
                _ => {}
            }
            for (k, shift) in take_color_keys() {
                gp.key(part, k, shift);
            }
            let phase = gp.draft(part).map(|d| d.last_input_ms).unwrap_or(0);
            gp.caret_on = live.window_focused && caret_visible(phase);
        }
        Some(part) => {
            gp.end_edit();
            for (k, shift) in take_color_keys() {
                gp.key(part, k, shift);
            }
            if live.take_key(vk::ENTER, Modifiers::NONE) || live.take_key(vk::SPACE, Modifiers::NONE) {
                match part {
                    GradientPart::Linear => gp.gradient.kind = GradientKind::Linear,
                    GradientPart::Radial => gp.gradient.kind = GradientKind::Radial,
                    GradientPart::Close => closed = true,
                    GradientPart::Bin => {
                        gp.remove_selected();
                    }
                    GradientPart::Add => gp.selected = gp.gradient.add_stop(0.5),
                    GradientPart::Field if field_opens => {
                        // Handled by the caller (`Open::Stop`) through `hot`.
                        gp.hot = Some(GradientPart::Field);
                    }
                    _ => {}
                }
            }
        }
        None => gp.end_edit(),
    }
    closed
}

/// The colour most fields on the page show — the design system's accent.
fn sample() -> Color {
    parse("#1a73e8").unwrap_or_default()
}

/// The web gallery's full picker: `#4a90d9`, twelve recent colours.
fn reference_picker() -> ColorPicker {
    let mut p = ColorPicker::new(parse("#4a90d9").unwrap_or_default());
    p.recent = [
        "#d93025", "#f9ab00", "#1e8e3e", "#1a73e8", "#9b51e0", "#ff7eb6", "#16a085", "#2c3e50", "#f4d03f", "#7f8c8d",
        "#000000", "#ffffff",
    ]
    .iter()
    .filter_map(|h| parse(h))
    .collect();
    p
}

/// The web gallery's quick picker: `#3c78d8` selected, three custom colours.
fn reference_swatches() -> SwatchPicker {
    let mut s = SwatchPicker::new();
    s.select(parse("#3c78d8").unwrap_or_default());
    s.custom = ["#4a90d9", "#9b59b6", "#16a085"].iter().filter_map(|h| parse(h)).collect();
    s
}

/// A four-stop gradient at 120°, one stop translucent so the chequer shows.
fn reference_gradient() -> Gradient {
    let stop = |hex: &str, position: f64, opacity: f64| GradientStop::new(parse(hex).unwrap_or_default().rgb, position, opacity);
    Gradient {
        kind: GradientKind::Linear,
        angle: 120.0,
        stops: vec![stop("#4a90d9", 0.0, 100.0), stop("#16a085", 0.35, 100.0), stop("#f9ab00", 0.7, 45.0), stop("#9b59b6", 1.0, 100.0)],
    }
}
