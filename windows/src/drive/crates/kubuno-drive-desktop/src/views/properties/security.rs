//! "Security" tab (READ-ONLY) of the properties sheet — mirror of
//! `Views/Properties/SecurityPage.xaml` (the DRAW). The view-model
//! (`SecurityModel`/`gather`, `Ace` = `AccessControlEntry`, SID resolution)
//! now lives in `crate::view_models::properties::security_view_model` and is
//! re-exported below to preserve the API
//! `properties::security::{SecurityModel, Ace}`.
//!
//! We read the DACL and owner of the file object via `GetNamedSecurityInfo`,
//! decode the selected ACE's access mask into permissions (Full control,
//! Modify, Read and execute, Read, Write, List folder contents for a
//! folder), exactly like `AccessControlEntry`'s `AllowedXAccess` properties.
//!
//! READ-ONLY: adding/removing an ACE (`AddAce`/`DeleteAce`), editable Deny
//! checkboxes and the "Advanced permissions" page are TODOs.

use crate::styles::theme::Theme;
use crate::ui::{Painter, Rect};

// Re-export of the view-model (mirror of `SecurityViewModel`), moved to
// `view_models/properties/security_view_model.rs`. Preserves the
// `crate::views::properties::security::{SecurityModel, Ace}` paths used by
// `window.rs` (and by `permission_rows`/`draw` below).
pub use crate::view_models::properties::security_view_model::{Ace, SecurityModel};

// --- `AccessMaskFlags` values (Data/Enums/AccessMaskFlags.cs) --------------
const MASK_LIST_DIRECTORY: u32 = 1;
const MASK_WRITE: u32 = 278;
const MASK_READ: u32 = 131209;
const MASK_READ_AND_EXECUTE: u32 = 131241;
const MASK_MODIFY: u32 = 197055;
const MASK_FULL_CONTROL: u32 = 2032127;

fn has_flag(mask: u32, flag: u32) -> bool {
    mask & flag == flag
}

/// Draws the tab in `content`, offset by `scroll`, with the `selected`
/// principal highlighted. Returns `(total height, clickable rects of the ACEs)`.
pub fn draw(
    p: &Painter,
    theme: &Theme,
    content: &Rect,
    model: &SecurityModel,
    selected: usize,
    scroll: f32,
) -> (f32, Vec<Rect>) {
    let f = &p.renderer.formats;
    let mut ace_hits = Vec::new();
    let pad = 12.0;
    let left = content.left + pad;
    let right = content.right - pad;
    let top0 = content.top + pad - scroll;
    let mut y = top0;

    if !model.display {
        // "Unable to display permissions" InfoBar (multi-line).
        let box_h = 72.0;
        let bar = Rect::new(left, y, right, y + box_h);
        p.fill_rounded(&bar, 4.0, &theme.layer_fill);
        p.stroke_rounded(&bar, 4.0, &theme.card_stroke);
        // Message split into lines (\r\n).
        let mut ty = bar.top + 10.0;
        for line in model.error.split("\r\n") {
            p.text_ellipsis(line, &Rect::new(bar.left + 12.0, ty, bar.right - 12.0, ty + 20.0), &f.body, &theme.text_primary);
            ty += 22.0;
        }
        return (box_h + pad * 2.0, ace_hits);
    }

    // Owner.
    let owner_row = Rect::new(left, y, right, y + 24.0);
    p.text(
        kubuno_drive_desktop_localization::tr("PropertyFileOwner"),
        &Rect::new(owner_row.left, owner_row.top, owner_row.left + 120.0, owner_row.bottom),
        &f.body_strong,
        &theme.text_secondary,
        false,
    );
    p.text_ellipsis(
        &model.owner,
        &Rect::new(owner_row.left + 128.0, owner_row.top, owner_row.right, owner_row.bottom),
        &f.body,
        &theme.text_primary,
    );
    y = owner_row.bottom + 8.0;

    // Principal list card (`PrincipalListGrid`).
    let list_header_h = 32.0;
    let ace_row_h = 32.0;
    let list_h = list_header_h + 1.0 + model.aces.len().max(1) as f32 * ace_row_h + 8.0;
    let card = Rect::new(left, y, right, y + list_h);
    p.fill_rounded(&card, 4.0, &theme.layer_fill);
    p.stroke_rounded(&card, 4.0, &theme.card_stroke);
    // "User name or group" header.
    p.text(
        kubuno_drive_desktop_localization::tr("SecurityUsersGroupLabel.Text"),
        &Rect::new(card.left + 16.0, card.top + 4.0, card.right - 12.0, card.top + list_header_h),
        &f.body,
        &theme.text_primary,
        false,
    );
    let sep = Rect::new(card.left, card.top + list_header_h, card.right, card.top + list_header_h + 1.0);
    p.fill_rounded(&sep, 0.0, &theme.divider);

    let mut ry = card.top + list_header_h + 1.0;
    if model.aces.is_empty() {
        p.text(
            kubuno_drive_desktop_localization::tr("SecurityNoAccessControlEntriesText"),
            &Rect::new(card.left + 16.0, ry, card.right - 16.0, ry + ace_row_h),
            &f.body,
            &theme.text_secondary,
            false,
        );
    } else {
        for (i, ace) in model.aces.iter().enumerate() {
            let row = Rect::new(card.left + 4.0, ry, card.right - 4.0, ry + ace_row_h);
            if i == selected {
                p.fill_rounded(&row.inflate(-2.0, -2.0), 4.0, &theme.control_fill_hover);
            }
            // Principal's glyph.
            p.text(
                ace.glyph,
                &Rect::new(row.left + 8.0, row.top, row.left + 32.0, row.bottom),
                &f.icon_small,
                &theme.text_primary,
                true,
            );
            // Name + (domain\name) in secondary.
            let name_rect = Rect::new(row.left + 40.0, row.top, row.right - 8.0, row.bottom);
            let label = if ace.full_name.is_empty() {
                ace.display_name.clone()
            } else {
                format!("{} {}", ace.display_name, ace.full_name)
            };
            p.text_ellipsis(&label, &name_rect, &f.body, &theme.text_primary);
            ace_hits.push(row);
            ry = row.bottom;
        }
    }
    y = card.bottom + 8.0;

    // Permissions card of the selected ACE (`PermissionsForSelected...`).
    let selected_ace = model.aces.get(selected);
    let perms: &[(&str, bool)] = &permission_rows(selected_ace, model.is_folder);
    let perm_header_h = 32.0;
    let perm_row_h = 28.0;
    let perm_h = perm_header_h + 1.0 + perms.len() as f32 * perm_row_h + 16.0;
    let pcard = Rect::new(left, y, right, y + perm_h);
    p.fill_rounded(&pcard, 4.0, &theme.layer_fill);
    p.stroke_rounded(&pcard, 4.0, &theme.card_stroke);
    // Header: "Permissions" or "Permissions for X".
    let header_text = match selected_ace {
        Some(ace) => format!("{} {}", kubuno_drive_desktop_localization::tr("Permissions"), ace.display_name),
        None => kubuno_drive_desktop_localization::tr("Permissions").to_string(),
    };
    p.text(
        &header_text,
        &Rect::new(pcard.left + 12.0, pcard.top + 4.0, pcard.right - 100.0, pcard.top + perm_header_h),
        &f.body,
        &theme.text_primary,
        false,
    );
    // "Allow" column (header).
    p.text(
        kubuno_drive_desktop_localization::tr("Allow"),
        &Rect::new(pcard.right - 90.0, pcard.top + 4.0, pcard.right - 12.0, pcard.top + perm_header_h),
        &f.body,
        &theme.text_secondary,
        true,
    );
    let psep = Rect::new(pcard.left, pcard.top + perm_header_h, pcard.right, pcard.top + perm_header_h + 1.0);
    p.fill_rounded(&psep, 0.0, &theme.divider);

    let mut py = pcard.top + perm_header_h + 1.0 + 6.0;
    for (label, checked) in perms {
        let row = Rect::new(pcard.left + 12.0, py, pcard.right - 12.0, py + perm_row_h);
        p.text(
            label,
            &Rect::new(row.left, row.top, row.right - 90.0, row.bottom),
            &f.body,
            &theme.text_primary,
            false,
        );
        // Allow checkbox (read-only) centered in the column.
        let cx = pcard.right - 51.0;
        let box_rect = Rect::new(cx - 10.0, row.top + perm_row_h / 2.0 - 10.0, cx + 10.0, row.top + perm_row_h / 2.0 + 10.0);
        let fill = if *checked { theme.accent } else { theme.toolbar_background };
        p.fill_rounded(&box_rect, 4.0, &fill);
        if *checked {
            p.text("\u{E73E}", &box_rect, &f.icon_small, &theme.accent_foreground, true);
        } else {
            p.stroke_rounded(&box_rect, 4.0, &theme.card_stroke);
        }
        py = row.bottom;
    }

    // TODO: editable Deny checkboxes + inheritance, adding/removing an ACE
    // (`AddAce`/`DeleteAce`), "Advanced permissions" page.
    (pcard.bottom - top0 + pad, ace_hits)
}

/// The permission rows (label, checked) of the selected ACE, decoded from
/// the allowed mask — mirror of `AccessControlEntry`'s `AllowedXAccess`.
fn permission_rows(ace: Option<&Ace>, is_folder: bool) -> Vec<(&'static str, bool)> {
    let mask = ace.map(|a| a.allow_mask).unwrap_or(0);
    let mut rows = vec![
        (tr("SecurityFullControlLabel.Text"), has_flag(mask, MASK_FULL_CONTROL)),
        (tr("Modify"), has_flag(mask, MASK_MODIFY)),
        (tr("SecurityReadAndExecuteLabel.Text"), has_flag(mask, MASK_READ_AND_EXECUTE)),
    ];
    if is_folder {
        rows.push((tr("SecurityListDirectoryLabel.Text"), has_flag(mask, MASK_LIST_DIRECTORY)));
    }
    rows.push((tr("SecurityReadLabel.Text"), has_flag(mask, MASK_READ)));
    rows.push((tr("Write"), has_flag(mask, MASK_WRITE)));
    rows
}

fn tr(key: &'static str) -> &'static str {
    kubuno_drive_desktop_localization::tr(key)
}
