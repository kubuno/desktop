//! "Customization" tab of the properties sheet — mirror of
//! `Views/Properties/CustomizationPage.xaml` (the DRAW). The view-model
//! (`CustomizationModel`/`gather`, `desktop.ini`/`.lnk` reading,
//! `CustomizationViewModel`) now lives in
//! `crate::view_models::properties::customization_view_model` and is
//! re-exported below to preserve the API
//! `properties::customization::CustomizationModel`.
//!
//! The tab is only offered for a FOLDER (non-archive) or a SHORTCUT. The
//! icon grid (`ExtractIconsFromDLL`) and APPLYING the choice (`UpdateIcon`)
//! are TODOs: the "Browse" / "Restore default" buttons are UI only.

use crate::styles::theme::Theme;
use crate::ui::{Painter, Rect};

// Re-export of the view-model (mirror of `CustomizationViewModel`), moved to
// `view_models/properties/customization_view_model.rs`. Preserves the
// `crate::views::properties::customization::CustomizationModel` path used by
// `window.rs`.
pub use crate::view_models::properties::customization_view_model::CustomizationModel;

/// Clickable areas of the tab (UI-only buttons).
#[derive(Default, Clone, Copy)]
pub struct CustomizationHits {
    pub browse: Option<Rect>,
    pub restore_default: Option<Rect>,
}

/// Draws the tab in `content`, offset by `scroll`. Returns
/// `(total height, clickable areas)`. Clipped by the caller.
pub fn draw(
    p: &Painter,
    theme: &Theme,
    content: &Rect,
    model: &CustomizationModel,
    scroll: f32,
) -> (f32, CustomizationHits) {
    let f = &p.renderer.formats;
    let mut hits = CustomizationHits::default();
    let pad = 12.0;
    let left = content.left + pad;
    let right = content.right - pad;
    let top0 = content.top + pad - scroll;

    if !model.loaded {
        return (0.0, hits);
    }

    // Single card (bordered `Grid`) containing header, separator, path + buttons.
    let card = Rect::new(left, top0, right, top0 + 132.0);
    p.fill_rounded(&card, 4.0, &theme.card_background);
    p.stroke_rounded(&card, 4.0, &theme.card_stroke);
    let ix = card.left + 12.0;
    let iright = card.right - 12.0;

    // Header: "Choose a custom icon" + "Restore default" button.
    let header_y = card.top + 12.0;
    let btn_w = 140.0;
    let restore = Rect::new(iright - btn_w, header_y, iright, header_y + 32.0);
    p.text(
        drive_localization::tr("ChooseCustomIcon"),
        &Rect::new(ix, header_y, restore.left - 8.0, header_y + 32.0),
        &f.body,
        &theme.text_primary,
        false,
    );
    draw_button(p, theme, &restore, drive_localization::tr("RestoreDefault"));
    hits.restore_default = Some(restore);

    // Separator (`DividerStrokeColorDefaultBrush`).
    let sep_y = header_y + 32.0 + 12.0;
    let sep = Rect::new(card.left, sep_y, card.right, sep_y + 1.0);
    p.fill_rounded(&sep, 0.0, &theme.divider);

    // DLL resource path (`TextBox`) + "Browse" button.
    let row_y = sep_y + 1.0 + 12.0;
    let browse = Rect::new(iright - btn_w, row_y, iright, row_y + 32.0);
    let tb = Rect::new(ix, row_y, browse.left - 8.0, row_y + 32.0);
    p.fill_rounded(&tb, 4.0, &theme.toolbar_background);
    p.stroke_rounded(&tb, 4.0, &theme.card_stroke);
    p.text_ellipsis(
        &model.icon_resource_path,
        &Rect::new(tb.left + 10.0, tb.top, tb.right - 10.0, tb.bottom),
        &f.body,
        &theme.text_primary,
    );
    draw_button(p, theme, &browse, drive_localization::tr("Browse"));
    hits.browse = Some(browse);

    // TODO: DLL icon grid (`ExtractIconsFromDLL` / `GridView`) and applying
    // the choice on Save (`UpdateIcon` → `SetCustomDirectoryIcon` /
    // `SetCustomFileIcon`). The buttons above are UI only.
    (card.bottom - top0 + pad, hits)
}

fn draw_button(p: &Painter, theme: &Theme, rect: &Rect, label: &str) {
    let f = &p.renderer.formats;
    p.fill_rounded(rect, 4.0, &theme.card_background);
    p.stroke_rounded(rect, 4.0, &theme.card_stroke);
    p.text(label, rect, &f.body, &theme.text_primary, true);
}
