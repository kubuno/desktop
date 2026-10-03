//! Gallery page — the rich text family (`kubuno_ui::richtext`, web
//! `core/frontend/src/ui/RichText.tsx`).
//!
//! The exposition shows the whole `@ui/RichText` box — the toolbar
//! ([`kubuno_ui::editors::RichTextToolbar`], lit from the caret) over a
//! word-wrapped document with two heading levels, every inline mark, both
//! list kinds, a link, inline code and a hard line break — then the box in
//! each state the field family distinguishes (resting with its placeholder,
//! focused with a selection and the `:focus-visible` outline, disabled,
//! invalid, read-only, the link row open), and the overflow cases: content
//! taller than the box (it scrolls, with the Kubuno scroll bar), an unbroken
//! word in a narrow box (`overflow-wrap: break-word`), the bare editing area,
//! and an editor nested in a `Card`.
//!
//! The right-hand column is a live editor: typing (and IME commits), the
//! mouse (click, drag, double / triple click, Shift+click, drag past the edge
//! to auto-scroll, the wheel, the scroll bar), the keyboard (arrows,
//! Ctrl+arrows, Home / End, Ctrl+Home / End, Page Up / Down, Shift to extend,
//! Enter / Shift+Enter, Backspace / Delete and their Ctrl forms), Ctrl+B / I /
//! U, Ctrl+K (the link row), Ctrl+Shift+7 / 8 (lists), Ctrl+A / C / X / V,
//! Ctrl+Z / Y, and the toolbar with its tooltips in a floating overlay that
//! may hang past the window. Under it, the live state (marks, block,
//! selection, undo, the HTML value) and the last input events — the page
//! doubles as the keyboard harness `C:/kubuno-build/keys-at.ps1` checks.

use std::cell::RefCell;

use kubuno_controls::enums::Size;
use kubuno_controls::host::{self, vk, Cursor, Frame, InputEvent, Modifiers};
use kubuno_ui::containers::Card;
use kubuno_ui::display::{place, Placement, Side, Tooltip};
use kubuno_ui::editors::RichTextCommand;
use kubuno_ui::focus::FocusOpts;
use kubuno_ui::metrics::{height, radius, space};
use kubuno_ui::richtext::{Block, BlockKind, Document, Mark, Marks, PointerInput, Pos, RichTextBox};
use kubuno_ui::{Canvas, Rect, Widget, WidgetState};

use super::interact::{self, Live};
use super::sheet::{Page, CAPTION_H, MARGIN};

/// Page-local metrics.
mod pm {
    /// Widest the showcase editor gets (the web box is as wide as its form).
    pub const SHOWCASE_MAX_W: f32 = 620.0;
    /// The state grid: three columns.
    pub const COLS: usize = 3;
    pub const COL_GAP: f32 = 24.0;
    /// The scrolling example is shorter than its content on purpose.
    pub const SCROLL_H: f32 = 118.0;
    /// The state cells keep the toolbar but shrink the editing area, so the
    /// six states fit on the page.
    pub const STATE_MIN_H: f32 = 44.0;
    /// How far that example is pre-scrolled.
    pub const SCROLL_Y: f32 = 36.0;
    /// The narrow box of the unbroken word.
    pub const NARROW_W: f32 = 180.0;
    /// The live editor's height: short enough for typing to make it scroll.
    pub const LIVE_H: f32 = 280.0;
    /// A status line in the live column.
    pub const LINE_H: f32 = 18.0;
    /// How many input events the harness lists.
    pub const LOG_LEN: usize = 5;
    /// A native `title` tooltip appears after a short hover.
    pub const TOOLTIP_DELAY_MS: u64 = 500;
    /// Room around the tooltip bubble in its overlay (arrow + shadow).
    pub const TOOLTIP_MARGIN: f32 = 10.0;
    /// The reset button.
    pub const BUTTON_W: f32 = 180.0;
}

// ── Sample content ──────────────────────────────────────────────────────────

fn sample_doc() -> Document {
    let bold = Marks::NONE.with(Mark::Bold);
    let italic = Marks::NONE.with(Mark::Italic);
    Document::from_blocks(vec![
        Block::new(BlockKind::Heading1).plain("Compte rendu"),
        Block::new(BlockKind::Paragraph)
            .plain("Le texte peut être ")
            .with("gras", bold.clone())
            .plain(", ")
            .with("italique", italic.clone())
            .plain(", ")
            .with("souligné", Marks::NONE.with(Mark::Underline))
            .plain(", ")
            .with("barré", Marks::NONE.with(Mark::Strike))
            .plain(", en ")
            .with("code", Marks::NONE.with(Mark::Code))
            .plain(" ou un ")
            .with("lien", Marks::NONE.linked("https://kubuno.com"))
            .plain(" ; les lignes trop longues passent à la ligne au mot près."),
        Block::new(BlockKind::Heading2).plain("Points"),
        Block::new(BlockKind::BulletItem).plain("Une puce ").with("en gras et italique", bold.with(Mark::Italic)),
        Block::new(BlockKind::BulletItem).plain("Une seconde puce, assez longue pour revenir à la ligne sous son propre texte."),
        Block::new(BlockKind::NumberedItem).plain("Première étape"),
        Block::new(BlockKind::NumberedItem).plain("Deuxième étape\navec un saut de ligne (Maj+Entrée)"),
    ])
}

fn long_doc() -> Document {
    let mut blocks = Vec::new();
    for i in 1..=8 {
        blocks.push(Block::new(BlockKind::Paragraph).plain(format!(
            "Paragraphe {i} — un contenu plus haut que la boîte défile dans la zone d'édition."
        )));
    }
    Document::from_blocks(blocks)
}

// ── Exposition ──────────────────────────────────────────────────────────────

/// The exposition's editors, kept between frames so their layout caches live.
struct Exposition {
    showcase: RichTextBox,
    states: Vec<(&'static str, RichTextBox, WidgetState)>,
    scrolling: RichTextBox,
    narrow: RichTextBox,
    bare: RichTextBox,
}

fn exposition() -> Exposition {
    let mut showcase = RichTextBox::new().with_document(sample_doc());
    showcase.doc.move_to(Pos::new(1, 22), false);
    showcase.sync_toolbar();

    let rest = RichTextBox::new().with_placeholder("Écrivez votre message…");
    let mut focused = RichTextBox::new().with_document(Document::from_plain("Une sélection surligne le texte choisi."));
    focused.doc.set_selection(Pos::new(0, 4), Pos::new(0, 14));
    focused.toggle_mark(Mark::Bold);
    focused.sync_toolbar();
    let mut disabled = RichTextBox::new().with_document(Document::from_plain("Éditeur désactivé."));
    disabled.enabled = false;
    let mut invalid = RichTextBox::new().with_placeholder("Champ obligatoire");
    invalid.invalid = true;
    let mut read_only = RichTextBox::new().with_document(Document::from_plain("Lecture seule : sélectionnable, non modifiable."));
    read_only.read_only = true;
    let mut linking = RichTextBox::new().with_document(Document::from_plain("Le mot site recevra un lien."));
    linking.doc.set_selection(Pos::new(0, 7), Pos::new(0, 11));
    linking.link_draft = Some("kubuno.com".to_string());
    linking.sync_toolbar();

    let focus = WidgetState::REST.focused(true).focus_visible(true);
    let mut states = vec![
        ("Au repos — placeholder", rest, WidgetState::REST),
        ("Focus — sélection, anneau :focus-visible", focused, focus),
        ("Désactivé", disabled, WidgetState::REST.disabled(true)),
        ("Invalide (error) — focus", invalid, focus),
        ("Lecture seule", read_only, WidgetState::REST),
        ("Barre de lien ouverte (Ctrl+K)", linking, focus),
    ];
    for (_, rt, _) in &mut states {
        rt.min_height = pm::STATE_MIN_H;
    }

    let mut scrolling = RichTextBox::new().with_document(long_doc());
    scrolling.scroll_y = pm::SCROLL_Y;
    let narrow = RichTextBox::bare()
        .with_document(Document::from_plain("https://exemple.kubuno.com/un/chemin/sans/aucune/espace"));
    let mut bare = RichTextBox::bare().with_placeholder("Zone d'édition seule, dans une Card");
    bare.min_height = pm::STATE_MIN_H;
    Exposition { showcase, states, scrolling, narrow, bare }
}

thread_local! {
    static EXPO: RefCell<Option<Exposition>> = const { RefCell::new(None) };
}

fn caption(c: &dyn Canvas, r: Rect, label: &str) {
    let t = c.theme();
    c.text_ellipsis(label, &r, &c.formats().caption, &t.text_secondary);
}

pub fn draw(c: &dyn Canvas, f: &Frame) {
    let (w, h) = f.size;
    let mut p = Page::new(c, w - interact::PANEL_W(), h);
    let right = p.area.right - MARGIN;
    EXPO.with(|cell| {
        let mut cell = cell.borrow_mut();
        let e = cell.get_or_insert_with(exposition);

        // ── The whole box ──
        p.section("Texte enrichi — RichText");
        let top = p.caption("Éditeur complet : barre d'outils (allumée depuis le curseur), titres, marques, listes, lien, code");
        let width = (right - MARGIN).clamp(1.0, pm::SHOWCASE_MAX_W);
        let hh = e.showcase.height_for_width(c, width);
        let r = Rect::new(MARGIN, top, MARGIN + width, top + hh);
        e.showcase.paint(c, r, WidgetState::REST);
        p.advance(hh);

        // ── States ──
        p.section("États");
        let col_w = ((right - MARGIN - pm::COL_GAP * (pm::COLS as f32 - 1.0)) / pm::COLS as f32).max(1.0);
        let mut row_top = p.y;
        let mut row_h: f32 = 0.0;
        for (i, (label, rt, st)) in e.states.iter().enumerate() {
            let col = i % pm::COLS;
            if col == 0 && i > 0 {
                row_top += row_h + space::LG;
                row_h = 0.0;
            }
            let x = MARGIN + col as f32 * (col_w + pm::COL_GAP);
            caption(c, Rect::new(x, row_top, x + col_w, row_top + CAPTION_H), label);
            let size = rt.measure(c);
            let cell = Rect::new(x, row_top + CAPTION_H + space::XS, x + col_w, row_top + CAPTION_H + space::XS + size.height);
            rt.paint(c, cell, *st);
            row_h = row_h.max(CAPTION_H + space::XS + size.height);
        }
        p.y = row_top + row_h + space::LG;

        // ── Overflow and nesting ──
        p.section("Débordement et imbrication");
        let cols = [
            "Contenu plus haut que la boîte : défilement",
            "Mot sans espace : coupure (break-word)",
            "Sans barre, imbriquée dans une Card",
        ];
        let top = p.y;
        for (i, label) in cols.iter().enumerate() {
            let x = MARGIN + i as f32 * (col_w + pm::COL_GAP);
            caption(c, Rect::new(x, top, x + col_w, top + CAPTION_H), label);
        }
        let y0 = top + CAPTION_H + space::XS;
        let r0 = Rect::new(MARGIN, y0, MARGIN + col_w, y0 + pm::SCROLL_H);
        e.scrolling.paint(c, r0, WidgetState::REST);
        let x1 = MARGIN + col_w + pm::COL_GAP;
        let nw = pm::NARROW_W.min(col_w);
        let nh = e.narrow.height_for_width(c, nw).min(pm::SCROLL_H);
        e.narrow.paint(c, Rect::new(x1, y0, x1 + nw, y0 + nh), WidgetState::REST);
        // The bare editing area nested in a Card: the parent's ground and
        // radius are respected, and the editor stays inside the body.
        let x2 = MARGIN + 2.0 * (col_w + pm::COL_GAP);
        let card = Card::titled("Commentaire");
        let inner_h = e.bare.measure(c).height;
        let cb = Rect::new(x2, y0, x2 + col_w, y0 + pm::SCROLL_H);
        card.paint(c, cb, WidgetState::REST);
        let body = card.body_rect(cb);
        {
            let _scope = card.push_body(c, cb);
            let er = Rect::new(body.left, body.top, body.right, (body.top + inner_h).min(body.bottom));
            e.bare.paint(c, er, WidgetState::REST);
        }
        p.y = y0 + pm::SCROLL_H + space::LG;
    });
}

// ── Live column ─────────────────────────────────────────────────────────────

struct LiveUi {
    prev_down: bool,
    rt: RichTextBox,
    hover: Option<(RichTextCommand, u64)>,
    log: Vec<String>,
    resets: u32,
}

thread_local! {
    static UI: RefCell<Option<LiveUi>> = const { RefCell::new(None) };
}

fn live_editor() -> RichTextBox {
    RichTextBox::new().with_document(sample_doc()).with_placeholder("Écrivez ici…")
}

fn describe(e: &InputEvent) -> Option<String> {
    match e {
        InputEvent::Key { vk, down: true, repeat, mods } => {
            let mut s = String::new();
            if mods.ctrl {
                s.push_str("Ctrl+");
            }
            if mods.alt {
                s.push_str("Alt+");
            }
            if mods.shift {
                s.push_str("Maj+");
            }
            s.push_str(&format!("0x{vk:02X}"));
            if *repeat {
                s.push_str(" (rép.)");
            }
            Some(s)
        }
        InputEvent::Text(t) => Some(format!("texte « {t} »")),
        InputEvent::WindowFocus(on) => Some(format!("fenêtre {}", if *on { "active" } else { "inactive" })),
        _ => None,
    }
}

fn marks_label(m: &Marks) -> String {
    let mut out = Vec::new();
    for (on, name) in [
        (m.bold, "gras"),
        (m.italic, "italique"),
        (m.underline, "souligné"),
        (m.strike, "barré"),
        (m.code, "code"),
        (m.link.is_some(), "lien"),
    ] {
        if on {
            out.push(name);
        }
    }
    if out.is_empty() {
        "aucune".to_string()
    } else {
        out.join(", ")
    }
}

fn block_label(k: BlockKind) -> &'static str {
    match k {
        BlockKind::Paragraph => "paragraphe",
        BlockKind::Heading1 => "titre 1",
        BlockKind::Heading2 => "titre 2",
        BlockKind::BulletItem => "liste à puces",
        BlockKind::NumberedItem => "liste numérotée",
    }
}

/// The toolbar button's `title`, as a tooltip in a floating overlay — it may
/// hang past the window, clamped to the monitor's work area.
fn tooltip_overlay(c: &dyn Canvas, f: &Frame, anchor: Rect, label: &str) {
    let tip = Tooltip::new(label).side(Side::Bottom);
    let size = tip.measure(c);
    let area = f.screen_area();
    let rel = Rect::new(anchor.left - area.left, anchor.top - area.top, anchor.right - area.left, anchor.bottom - area.top);
    let placed = place(rel, size, tip.side, Size::new(area.right - area.left, area.bottom - area.top));
    let mg = pm::TOOLTIP_MARGIN;
    let (bl, bt) = (placed.rect.left - mg, placed.rect.top - mg);
    let bounds = Rect::new(bl + area.left, bt + area.top, placed.rect.right + mg + area.left, placed.rect.bottom + mg + area.top);
    let local = Placement {
        rect: Rect::new(placed.rect.left - bl, placed.rect.top - bt, placed.rect.right - bl, placed.rect.bottom - bt),
        side: placed.side,
        tip: (placed.tip.0 - bl, placed.tip.1 - bt),
    };
    host::overlay(bounds, move |canvas| tip.paint_placed(canvas, &local, WidgetState::REST));
}

pub fn interactive_column(c: &dyn Canvas, f: &Frame) {
    UI.with(|cell| {
        let mut cell = cell.borrow_mut();
        let ui = cell.get_or_insert_with(|| LiveUi {
            prev_down: false,
            rt: live_editor(),
            hover: None,
            log: Vec::new(),
            resets: 0,
        });
        let live = Live::new(f, ui.prev_down);
        ui.prev_down = f.mouse_down;
        let t = c.theme();
        let fm = c.formats();

        // Log what arrived before anyone consumes it.
        for e in live.events() {
            if let Some(s) = describe(&e) {
                ui.log.push(s);
            }
        }
        let excess = ui.log.len().saturating_sub(pm::LOG_LEN);
        ui.log.drain(..excess);

        let (left, mut y, right) = interact::panel(c, interact::panel_rect(f.size));
        y = interact::caption(c, left, right, y, "Éditeur — clavier, souris, presse-papiers, molette");
        let r = Rect::new(left, y, right, y + pm::LIVE_H);

        // Focus first (paint order), then the pointer, then the keys.
        let st = live.focus_with("richtext-editor", r, FocusOpts::TEXT);
        if st.gained {
            ui.rt.touch();
        }
        let rt = &mut ui.rt;
        rt.window_active = live.window_focused;
        rt.handle_pointer(
            c,
            r,
            PointerInput {
                x: live.mouse.0,
                y: live.mouse.1,
                down: live.down,
                pressed: live.clicked,
                click_count: live.click_count,
                shift: live.mods.shift,
                wheel_dy: live.wheel.1 * host::WHEEL_NOTCH_DIP,
            },
        );
        if st.focused {
            rt.handle_host_input(c, r);
        }
        if let Some(cur) = rt.cursor_at(c, r, live.mouse.0, live.mouse.1) {
            host::set_cursor(cur);
        }
        rt.sync_toolbar();
        rt.paint(c, r, st.apply(live.state(r)));

        // The toolbar's tooltips: after a short hover, like a native `title`.
        let now = host::now_ms();
        match rt.command_at(r, live.mouse.0, live.mouse.1) {
            Some(cmd) if !live.down => {
                let since = match ui.hover {
                    Some((prev, t0)) if prev == cmd => t0,
                    _ => now,
                };
                ui.hover = Some((cmd, since));
                let waited = now.saturating_sub(since);
                if waited >= pm::TOOLTIP_DELAY_MS {
                    if let Some(anchor) = ui.rt.command_rect(r, cmd) {
                        tooltip_overlay(c, f, anchor, cmd.label());
                    }
                } else {
                    host::request_repaint_after((pm::TOOLTIP_DELAY_MS - waited) as u32);
                }
            }
            _ => ui.hover = None,
        }
        y = r.bottom + space::MD;

        // ── Live state ──
        let doc = &ui.rt.doc;
        let sel = doc.selection();
        let lines = [
            format!("Marques actives : {}", marks_label(&doc.active_marks())),
            format!("Bloc : {}", block_label(doc.active_block())),
            format!(
                "Sélection : ({}, {}) → ({}, {})   Annuler : {}   Rétablir : {}",
                sel.anchor.block,
                sel.anchor.offset,
                sel.head.block,
                sel.head.offset,
                if doc.can_undo() { "oui" } else { "non" },
                if doc.can_redo() { "oui" } else { "non" }
            ),
            format!("HTML : {}", doc.to_html()),
        ];
        for line in lines.iter() {
            let lr = Rect::new(left, y, right, y + pm::LINE_H);
            c.text_ellipsis(line, &lr, &fm.caption, &t.text_secondary);
            y += pm::LINE_H;
        }
        y += space::SM;

        // ── A focusable button: Tab reaches it after the editor ──
        let b = Rect::new(left, y, left + pm::BUTTON_W, y + height::BUTTON_MD);
        let ws = live.focus_state("richtext-reset", b);
        if live.hover(b) {
            host::set_cursor(Cursor::Hand);
        }
        let activate = live.hit(b)
            || (ws.focused
                && (live.take_key(vk::ENTER, Modifiers::NONE) || live.take_key(vk::SPACE, Modifiers::NONE)));
        if activate {
            ui.rt = live_editor();
            ui.resets += 1;
        }
        c.fill_rounded(&b, radius::LG, if ws.pressed { &t.accent_hover } else { &t.accent });
        if ws.show_focus_ring() {
            let o = 2.0;
            let ring = Rect::new(b.left - o, b.top - o, b.right + o, b.bottom + o);
            c.stroke_rounded_w(&ring, radius::LG + o, &t.accent_light, o);
        }
        let label = format!("Réinitialiser ({})", ui.resets);
        c.text_ellipsis_center(&label, &b, &fm.body_strong, &t.accent_foreground);
        y = b.bottom + space::LG;

        // ── Harness: the raw input ──
        let status = format!(
            "Modificateurs : {}{}{}   clic ×{}   focus visible : {}",
            if live.mods.ctrl { "Ctrl " } else { "" },
            if live.mods.shift { "Maj " } else { "" },
            if live.mods.alt { "Alt " } else { "" },
            live.click_count,
            interact::with_focus(|ring| ring.focus_visible())
        );
        c.text_ellipsis(&status, &Rect::new(left, y, right, y + pm::LINE_H), &fm.caption, &t.text_tertiary);
        y += pm::LINE_H;
        y = interact::caption(c, left, right, y, "Derniers événements");
        for line in ui.log.iter() {
            c.text_ellipsis(line, &Rect::new(left, y, right, y + pm::LINE_H), &fm.caption, &t.text_tertiary);
            y += pm::LINE_H;
        }
    });
}
