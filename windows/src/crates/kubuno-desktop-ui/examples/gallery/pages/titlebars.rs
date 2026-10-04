//! Gallery page — **title bars**: the Kubuno window band in its two heights (decision of
//! 2026-10-04), drawn by the one shared painter every window uses
//! ([`kubuno_desktop_controls::window_chrome`]):
//!
//! * `TitleBarStyle="Standard"`: 32 DIP, Windows 11's caption height — dialogs, tool windows,
//!   secondary forms (the default);
//! * `TitleBarStyle="Tall"`: 64 DIP, the web's module header — a main window showing the header's
//!   menus (waffle, account, `HeaderActions`) left of the caption buttons.
//!
//! Each band is shown with both caption button styles (Kubuno's rounded boxes, Windows' wide
//! columns), its close button hovered, and placeholders where a page's title-bar controls go. The
//! thin accent rule across each band is its middle: every caption button, glyph, icon, title and
//! slot item is centred on it.

use kubuno_desktop_controls::host::Frame;
use kubuno_desktop_controls::window_chrome as wc;
use kubuno_desktop_ui::{Canvas, Rect};

use super::interact;
use super::sheet::{Page, MARGIN};

/// The client area drawn under each band, so the sample reads as a window.
const BODY: f32 = 28.0;

pub fn draw(c: &dyn Canvas, f: &Frame) {
    let mut p = Page::new(c, f.size.0 - interact::PANEL_W(), f.size.1);
    let width = (p.area.right - 2.0 * MARGIN).clamp(360.0, 760.0);
    let t = c.theme().clone();

    p.section("Barres de titre — Standard (32 DIP) et Tall (64 DIP), boutons centrés");
    let samples = [
        ("Standard · boutons Kubuno · Fermer survolé (dialogue, fenêtre secondaire)", wc::TitleBarStyle::Standard, wc::ButtonStyle::Kubuno, false),
        ("Standard · boutons Windows · Fermer survolé", wc::TitleBarStyle::Standard, wc::ButtonStyle::Windows, true),
        ("Tall · boutons Kubuno · menus de l'en-tête à droite (fenêtre principale)", wc::TitleBarStyle::Tall, wc::ButtonStyle::Kubuno, false),
        ("Tall · boutons Windows · en-tête neutre (shell, chat)", wc::TitleBarStyle::Tall, wc::ButtonStyle::Windows, true),
    ];
    for (label, size, buttons, neutral) in samples {
        let top = p.caption(label);
        let mut style = wc::ChromeStyle { size, buttons, ..wc::ChromeStyle::default() };
        if neutral {
            // The web header's neutral look (`var(--body-bg)`, the text colour), as the shell wears it.
            style.background = Some(t.window_background);
            style.foreground = Some(t.text_primary);
        }
        let band_h = style.band_height();
        let outer = Rect::new(MARGIN, top, MARGIN + width, top + band_h + BODY);
        wc::paint_frame(c, outer, 8.0, Some(t.card_stroke));
        // The page's title-bar controls: a brand on the left, the header's buttons on the right.
        let tall = size == wc::TitleBarStyle::Tall;
        let side = if tall { 36.0 } else { wc::caption_button_size(band_h) };
        let count = if tall { 4.0 } else { 2.0 };
        let slots = wc::SlotWidths { left: if tall { 120.0 } else { 0.0 }, center: 0.0, right: count * side + (count - 1.0) * wc::BUTTON_GAP };
        let layout = wc::layout(&style, outer, true, wc::SystemButtons::default(), slots);
        wc::paint_band_rounded(c, &style, &layout, 8.0);
        let mut ghost = style.ink_color(&t);
        ghost.a *= 0.22;
        let cy = (layout.band.top + layout.band.bottom) / 2.0;
        let mut x = layout.right.left;
        for _ in 0..count as usize {
            c.fill_rounded(&Rect::new(x, cy - side / 2.0, x + side, cy + side / 2.0), side / 2.0, &ghost);
            x += side + wc::BUTTON_GAP;
        }
        if tall {
            c.fill_rounded(&Rect::new(layout.left.left, cy - 12.0, layout.left.right, cy + 12.0), 6.0, &ghost);
        }
        let state = wc::ChromeState { hot: Some(wc::Part::Close), ..wc::ChromeState::default() };
        let title = if tall { "" } else { "Propriétés — Rapport annuel.pdf" };
        wc::paint_caption(c, &style, &layout, title, wc::ChromeIcon::Glyph("FileText"), state);
        // The band's middle, across it: what everything in the band is centred on.
        // In the band's own ink (the accent would vanish on an accent band), dashed so the glyphs stay readable.
        let mut rule = style.ink_color(&t);
        rule.a = 0.55;
        let mut x = layout.band.left;
        while x < layout.band.right {
            c.fill_rounded(&Rect::new(x, cy - 0.5, (x + 6.0).min(layout.band.right), cy + 0.5), 0.0, &rule);
            x += 10.0;
        }
        p.advance(band_h + BODY);
    }
}
