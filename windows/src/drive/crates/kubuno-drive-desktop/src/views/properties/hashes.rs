//! "Hashes" tab of the properties sheet — mirror of
//! `Views/Properties/HashesPage.xaml` (the DRAW). The view-model
//! (`HashRow`/`HashesState`/`compute`/`post`, `HashesViewModel` +
//! `Files.Shared.Helpers.ChecksumHelpers`) now lives in
//! `crate::view_models::properties::hashes_view_model` and is re-exported
//! below to preserve the API `properties::hashes::{HashesState,
//! WM_APP_HASH_READY, HashRow}`.

use crate::styles::theme::Theme;
use crate::ui::{Painter, Rect};

// Re-export of the view-model (mirror of `HashesViewModel`), moved to
// `view_models/properties/hashes_view_model.rs`. Preserves the
// `crate::views::properties::hashes::{HashesState, WM_APP_HASH_READY, HashRow}`
// paths used by `window.rs`.
pub use crate::view_models::properties::hashes_view_model::{
    HashesState, WM_APP_HASH_READY,
};

/// Draws the tab in `content`, offset by `scroll`, and returns
/// `(total height, Copy button rects per row)`. `anim` drives the
/// indeterminate ProgressBar.
pub fn draw(
    p: &Painter,
    theme: &Theme,
    content: &Rect,
    state: &HashesState,
    scroll: f32,
    anim: std::time::Duration,
) -> (f32, Vec<(Rect, usize)>) {
    let f = &p.renderer.formats;
    let mut copy_hits = Vec::new();
    let pad = 12.0;
    let left = content.left + pad;
    let right = content.right - pad;
    let top0 = content.top + pad - scroll;

    // Enclosing card (`HashesListGrid`: card background + border).
    let algo_w = 84.0;
    let row_h = 40.0;
    let header_h = 40.0;
    let card_top = top0;
    let card_h = header_h + state.rows.len() as f32 * row_h + 8.0;
    let card = Rect::new(left, card_top, right, card_top + card_h);
    p.fill_rounded(&card, 4.0, &theme.card_background);
    p.stroke_rounded(&card, 4.0, &theme.card_stroke);

    let ix = card.left + 16.0;
    let iright = card.right - 12.0;

    // Header: Algorithm | Value (`ListView.Header`).
    let hrow = Rect::new(ix, card.top + 8.0, iright, card.top + header_h);
    p.text(
        kubuno_drive_desktop_localization::tr("Algorithm"),
        &Rect::new(hrow.left, hrow.top, hrow.left + algo_w, hrow.bottom),
        &f.body_strong,
        &theme.text_primary,
        false,
    );
    p.text(
        kubuno_drive_desktop_localization::tr("HashValue"),
        &Rect::new(hrow.left + algo_w + 12.0, hrow.top, iright, hrow.bottom),
        &f.body_strong,
        &theme.text_primary,
        false,
    );
    // Bottom separator of the header.
    let sep = Rect::new(card.left + 8.0, card.top + header_h, card.right - 8.0, card.top + header_h + 1.0);
    p.fill_rounded(&sep, 0.0, &theme.divider);

    let mut y = card.top + header_h;
    for (i, row) in state.rows.iter().enumerate() {
        let r = Rect::new(ix, y, iright, y + row_h);
        // Algorithm name (`PropertyName`).
        p.text(
            row.algorithm,
            &Rect::new(r.left, r.top, r.left + algo_w, r.bottom),
            &f.body_strong,
            &theme.text_secondary,
            false,
        );
        let vx = r.left + algo_w + 12.0;
        if row.calculating {
            // "Calculating…" + indeterminate ProgressBar.
            p.text(
                kubuno_drive_desktop_localization::tr("Calculating"),
                &Rect::new(vx, r.top, vx + 90.0, r.bottom),
                &f.body,
                &theme.text_secondary,
                false,
            );
            let bar = Rect::new(vx + 100.0, r.top + row_h / 2.0 - 3.0, (vx + 200.0).min(iright), r.top + row_h / 2.0 + 3.0);
            draw_indeterminate_bar(p, theme, &bar, anim);
        } else if let Some(value) = &row.value {
            // Value truncated with "…" (Copy button on the right).
            let copy = Rect::new(iright - 28.0, r.top + row_h / 2.0 - 14.0, iright, r.top + row_h / 2.0 + 14.0);
            let vrect = Rect::new(vx, r.top, copy.left - 8.0, r.bottom);
            p.text_ellipsis(value, &vrect, &f.body, &theme.text_primary);
            // Copy button (`CopyHashButton`, glyph ).
            p.text("\u{E8C8}", &copy, &f.icon_small, &theme.text_secondary, true);
            copy_hits.push((copy, i));
        }
        y = r.bottom;
    }

    // TODO: `SelectAlgorithmsButton` (choice of displayed algorithms,
    // SHA384/SHA512), `HashInputTextBox` + `CompareFileButton` (comparison).
    (y - top0 + pad, copy_hits)
}

/// Indeterminate progress bar (`ProgressBar IsIndeterminate=True`) — same
/// visual as `general::draw_indeterminate_bar`.
fn draw_indeterminate_bar(p: &Painter, theme: &Theme, track: &Rect, anim: std::time::Duration) {
    p.fill_rounded(track, 2.0, &theme.drive_bar_track);
    let w = track.right - track.left;
    let seg = w * 0.35;
    let t = (anim.as_secs_f32() / 1.6).fract();
    let travel = (w + seg) * t - seg;
    let x0 = (track.left + travel).clamp(track.left, track.right);
    let x1 = (track.left + travel + seg).clamp(track.left, track.right);
    if x1 > x0 {
        p.fill_rounded(&Rect::new(x0, track.top, x1, track.bottom), 2.0, &theme.accent);
    }
}
