//! Port of `Files.App/Dialogs/`, re-skinned on the Kubuno web `FloatingWindow`
//! (`.kb-window`): scrim, centered card with the `--kb-window-radius`, a FULL
//! ACCENT title bar carrying the title and its ✕, a body, and a footer of TEXT
//! buttons (confirm LEFT, cancel RIGHT). Each concrete box is a pre-filled
//! `DialogState`, like the `DynamicDialogFactory` factories.

use crate::ui::{Painter, Rect};
use drive_app_controls::themes::shape;

/// What "OK" triggers — the ViewModel's `PrimaryButtonAction`.
#[derive(Clone, PartialEq)]
pub enum DialogAction {
    /// `DeleteItemsDialog`: delete the selection (paths frozen at open time).
    DeleteItems(Vec<String>),
    /// `CreateArchiveDialog`: compress `sources` under the field's name,
    /// in the chosen format (zip/7z).
    CompressInto(Vec<String>),
    /// `DecompressArchiveDialog`: extract `archive` to the field's path;
    /// the "Ouvrir le dossier de destination une fois terminé" checkbox.
    DecompressTo(String),
    /// `AddItemDialog`: the "Créer un nouvel élément" list — clicking an
    /// item chains into the corresponding naming dialog.
    AddItem,
    /// `GetFor_CreateItemDialog`: create a folder (`true`) or an empty file
    /// with the typed name in the current folder.
    CreateItem(bool),
    /// `CreateShortcutDialog`: create a shortcut to the field's path in the
    /// current folder (`.lnk` if the target exists, `.url` otherwise).
    CreateShortcutTo,
    /// `GitCreateBranchDialog`: create a branch with the typed name, based
    /// on the current branch, and switch to it.
    GitCreateBranch,
    /// `FileTagsHelper.RemoveTagsAsync`: "Supprimer les étiquettes"
    /// confirmation, then clears the tags of the paths frozen at open time.
    RemoveTags(Vec<String>),
    /// `EmptyRecycleBinAction`: confirmation then `SHEmptyRecycleBin`.
    EmptyRecycleBin,
    /// `RestoreAllRecycleBinAction`: restore everything.
    RestoreAllTrashes,
    /// `RestoreRecycleBinAction`: restore the selection (`$R…` paths).
    RestoreTrashItems(Vec<String>),
}

/// A row of a dialog's list (`AddItemDialogListItemViewModel`): icon,
/// title, grey subtitle.
#[derive(Clone, PartialEq)]
pub struct DialogListItem {
    pub glyph: String,
    pub header: String,
    pub sub_header: String,
}

/// The state of an open `ContentDialog`.
#[derive(Clone, PartialEq)]
pub struct DialogState {
    pub title: String,
    pub subtitle: String,
    /// Empty = no primary button (AddItemDialog only has Cancel).
    pub primary_text: String,
    pub close_text: String,
    pub action: DialogAction,
    /// A text field ("Nom" + TextBox), edited via `EditState`/`EDIT_DIALOG`.
    pub field_label: Option<String>,
    /// A choice ("Format" + zip/7z): label, options, selection.
    pub choices_label: Option<String>,
    pub choices: Vec<String>,
    pub choice: usize,
    /// A checkbox ("Supprimer définitivement", "Ouvrir une fois terminé"):
    /// label and state.
    pub checkbox_label: Option<String>,
    pub checkbox: bool,
    /// A clickable list (the AddItemDialog's ListView).
    pub list: Vec<DialogListItem>,
}

impl DialogState {
    /// A two-button dialog with no rich content.
    pub fn simple(
        title: String,
        subtitle: String,
        primary_text: String,
        close_text: String,
        action: DialogAction,
    ) -> Self {
        Self {
            title,
            subtitle,
            primary_text,
            close_text,
            action,
            field_label: None,
            choices_label: None,
            choices: Vec::new(),
            choice: 0,
            checkbox_label: None,
            checkbox: false,
            list: Vec::new(),
        }
    }

    /// `true` when confirming DESTROYS something — the web renders such a
    /// confirm button as a `danger` text button rather than an accent one.
    pub fn is_destructive(&self) -> bool {
        matches!(
            self.action,
            DialogAction::DeleteItems(_) | DialogAction::EmptyRecycleBin | DialogAction::RemoveTags(_)
        )
    }
}

/// `FileNameConflictResolveOptionType`: the resolution chosen for a conflict.
#[derive(Clone, Copy, PartialEq)]
pub enum ConflictResolve {
    /// Generate a unique name "(2)" (`GenerateUniqueName`).
    GenerateNewName,
    /// Replace the existing one (`ReplaceExisting`).
    ReplaceExisting,
    /// Skip (`Skip` → `FailIfExists`, the item is not copied).
    Skip,
}

impl ConflictResolve {
    /// The option's `.resw` label key (like the ComboBox items).
    pub fn label(self) -> &'static str {
        match self {
            ConflictResolve::GenerateNewName => "GenerateNewName",
            ConflictResolve::ReplaceExisting => "ReplaceExisting",
            ConflictResolve::Skip => "Skip",
        }
    }
    /// Cycles to the next option (clicking an item's pill).
    pub fn cycled(self) -> Self {
        match self {
            ConflictResolve::GenerateNewName => ConflictResolve::ReplaceExisting,
            ConflictResolve::ReplaceExisting => ConflictResolve::Skip,
            ConflictResolve::Skip => ConflictResolve::GenerateNewName,
        }
    }
}

/// An item of the `FilesystemOperationDialog`: a source→destination pair,
/// conflicting or not (`FileSystemDialogConflictItemViewModel`).
#[derive(Clone, PartialEq)]
pub struct ConflictItem {
    /// Full source path.
    pub source: String,
    /// Displayed source name (basename).
    pub source_name: String,
    /// Destination name (basename in the target folder).
    pub dest_name: String,
    /// `true` if the destination already exists (item to resolve).
    pub is_conflict: bool,
    /// The chosen option (ignored if `!is_conflict`).
    pub resolve: ConflictResolve,
}

/// The `FilesystemOperationDialog` in conflict mode: the list of items, the
/// "apply to all" resolution, and what's needed to relaunch the operation
/// on "Continuer".
#[derive(Clone, PartialEq)]
pub struct ConflictDialog {
    pub title: String,
    pub description: String,
    pub items: Vec<ConflictItem>,
    /// Move (`true`) or copy — the operation to relaunch once resolved.
    pub is_move: bool,
    /// Destination folder.
    pub dest_dir: String,
    /// "Appliquer à tous": `Some(option)` = all the same, `None` = "Personnalisé".
    pub aggregated: Option<ConflictResolve>,
}

impl ConflictDialog {
    /// Recomputes `aggregated`: `Some` if all conflicts share the option,
    /// `None` (Personnalisé) otherwise — mirrors `Receive(...OptionChanged)`.
    pub fn sync_aggregated(&mut self) {
        let mut it = self.items.iter().filter(|i| i.is_conflict).map(|i| i.resolve);
        self.aggregated = match it.next() {
            Some(first) if it.all(|r| r == first) => Some(first),
            _ => None,
        };
    }
    /// "Appliquer à tous": forces the option on every conflict.
    pub fn apply_to_all(&mut self, option: ConflictResolve) {
        for item in self.items.iter_mut().filter(|i| i.is_conflict) {
            item.resolve = option;
        }
        self.aggregated = Some(option);
    }
    /// The primary button's label (`ConflictingItemsDialogPrimaryButtonText`
    /// = "Continuer").
    pub fn primary_button(&self) -> &'static str {
        drive_localization::tr("ConflictingItemsDialogPrimaryButtonText")
    }
}

/// The scrim behind a `FloatingWindow` (`rgba(0,0,0,.30)`).
pub const SMOKE: f32 = 0.30;
/// `.kb-window` width.
pub const DIALOG_WIDTH: f32 = 448.0;
/// Body padding (`--kb-space-lg`).
pub const PAD: f32 = shape::space::LG;
/// `.kb-window-titlebar`: the full-accent band (`min-h-11`).
const TITLEBAR_H: f32 = shape::height::DIALOG_TITLEBAR;
/// The title bar's ✕ button.
const TITLE_CLOSE: f32 = 30.0;
const TITLE_CLOSE_RADIUS: f32 = 5.0;
/// Inset of the ✕ from the card's right edge.
const TITLE_CLOSE_INSET: f32 = shape::space::SM;
/// One line of `--kb-text-body` message text.
const SUBTITLE_H: f32 = 22.0;
/// `.kb-window-footer`: 12px vertical padding around 36-tall text buttons.
const FOOTER_PAD_Y: f32 = shape::space::MD;
const BUTTON_H: f32 = shape::height::BUTTON_MD;
const FOOTER_H: f32 = 2.0 * FOOTER_PAD_Y + BUTTON_H;
/// A footer text button never goes below this (`min-width: 96px`).
const BUTTON_MIN_W: f32 = 96.0;

/// A labelled control row (a 36-tall control with 4 DIP of breathing room).
const ROW_H: f32 = 44.0;

/// Width of a footer text button. The geometry runs without a device context,
/// so the label is measured by glyph count (14px Segoe averages ~7.6 DIP per
/// glyph) and clamped to the web's minimum width.
fn button_width(label: &str) -> f32 {
    (label.chars().count() as f32 * 7.6 + 2.0 * shape::space::LG).max(BUTTON_MIN_W)
}

/// The pair of footer buttons, shrunk proportionally if they would collide
/// (confirm is left-aligned, cancel right-aligned).
fn button_widths(primary: &str, close: &str, card_width: f32) -> (f32, f32) {
    let (mut pw, mut cw) = (button_width(primary), button_width(close));
    let usable = card_width - 2.0 * PAD - shape::space::SM;
    if pw + cw > usable && pw + cw > 0.0 {
        let k = usable / (pw + cw);
        pw *= k;
        cw *= k;
    }
    (pw, cw)
}

/// The ✕ of the title bar, right-aligned in the accent band.
fn titlebar_close_rect(card: &Rect) -> Rect {
    let cy = card.top + TITLEBAR_H / 2.0;
    Rect::new(
        card.right - TITLE_CLOSE_INSET - TITLE_CLOSE,
        cy - TITLE_CLOSE / 2.0,
        card.right - TITLE_CLOSE_INSET,
        cy + TITLE_CLOSE / 2.0,
    )
}

/// A dialog's computed rects.
pub struct DialogRects {
    pub card: Rect,
    /// The ✕ of the accent title bar — same action as `close`.
    pub titlebar_close: Rect,
    pub primary: Rect,
    pub close: Rect,
    /// The "Nom" field's TextBox.
    pub field: Option<Rect>,
    /// The "Format" choice's pills.
    pub choices: Vec<Rect>,
    /// The checkbox (its clickable zone includes the label).
    pub checkbox: Option<Rect>,
    /// The list's rows (50 DIP each, the 400-tall ListView).
    pub list: Vec<Rect>,
}

/// A list row's height (the template's `Grid Height="50"`).
const LIST_ROW_H: f32 = 50.0;

/// The dialog's geometry — dynamic based on the content. The title is NOT in
/// the body any more: it lives in the accent title bar, so the body starts
/// under it.
pub fn dialog_rects(dialog: &DialogState, width: f32, height: f32) -> DialogRects {
    let has_content = dialog.field_label.is_some()
        || !dialog.choices.is_empty()
        || dialog.checkbox_label.is_some()
        || !dialog.list.is_empty();
    // The gap under the message only exists when something follows it.
    let subtitle_h = if dialog.subtitle.is_empty() {
        0.0
    } else {
        SUBTITLE_H + if has_content { shape::space::MD } else { 0.0 }
    };
    let field_h = if dialog.field_label.is_some() { ROW_H + shape::space::SM } else { 0.0 };
    let choices_h = if dialog.choices.is_empty() { 0.0 } else { ROW_H + shape::space::SM };
    let checkbox_h = if dialog.checkbox_label.is_some() { ROW_H } else { 0.0 };
    let list_h = dialog.list.len() as f32 * LIST_ROW_H;
    let card_h = TITLEBAR_H
        + PAD
        + subtitle_h
        + field_h
        + choices_h
        + checkbox_h
        + list_h
        + PAD
        + FOOTER_H;
    let left = (width - DIALOG_WIDTH) / 2.0;
    let top = (height - card_h) / 2.0;
    let card = Rect::new(left, top, left + DIALOG_WIDTH, top + card_h);
    let titlebar_close = titlebar_close_rect(&card);

    // The content, below the subtitle: "label | control" (column 140).
    let mut cy = card.top + TITLEBAR_H + PAD + subtitle_h;
    let control_left = card.left + PAD + 140.0;
    let field = dialog.field_label.is_some().then(|| {
        let r = Rect::new(control_left, cy + 4.0, card.right - PAD, cy + ROW_H - 4.0);
        cy += ROW_H + 8.0;
        r
    });
    let mut choices = Vec::new();
    if !dialog.choices.is_empty() {
        let mut x = control_left;
        for _ in &dialog.choices {
            choices.push(Rect::new(x, cy + 4.0, x + 76.0, cy + ROW_H - 4.0));
            x += 84.0;
        }
        cy += ROW_H + 8.0;
    }
    // The CheckBox, full width below the content (as in the XAML).
    let checkbox = dialog
        .checkbox_label
        .is_some()
        .then(|| Rect::new(card.left + PAD, cy + 4.0, card.right - PAD, cy + ROW_H - 4.0));
    if checkbox.is_some() {
        cy += ROW_H;
    }
    // The AddItemDialog's ListView: full-width rows of 50.
    let mut list = Vec::new();
    for _ in &dialog.list {
        list.push(Rect::new(card.left + PAD, cy, card.right - PAD, cy + LIST_ROW_H));
        cy += LIST_ROW_H;
    }

    // Footer: CONFIRM on the LEFT, CANCEL on the RIGHT (Kubuno order, the
    // reverse of Windows). A dialog without a primary keeps only its cancel,
    // still right-aligned.
    let by = card.bottom - FOOTER_H + FOOTER_PAD_Y;
    let (primary, close) = if dialog.primary_text.is_empty() {
        let cw = button_width(&dialog.close_text);
        (
            Rect::new(0.0, 0.0, 0.0, 0.0),
            Rect::new(card.right - PAD - cw, by, card.right - PAD, by + BUTTON_H),
        )
    } else {
        let (pw, cw) = button_widths(&dialog.primary_text, &dialog.close_text, DIALOG_WIDTH);
        (
            Rect::new(card.left + PAD, by, card.left + PAD + pw, by + BUTTON_H),
            Rect::new(card.right - PAD - cw, by, card.right - PAD, by + BUTTON_H),
        )
    };
    DialogRects { card, titlebar_close, primary, close, field, choices, checkbox, list }
}

/// Width of the `FilesystemOperationDialog` in conflict mode (the original
/// goes up to 650; we stay a bit narrower for the port's window).
pub const CONFLICT_WIDTH: f32 = 580.0;
const CONFLICT_ROW_H: f32 = 56.0;
/// Max list height (`ListView MaxHeight=200`, a bit more generous).
const CONFLICT_LIST_MAX: f32 = 280.0;
/// Width of the option pill (the original's 200-wide ComboBox).
const OPTION_PILL_W: f32 = 190.0;

/// A conflict dialog's computed rects.
pub struct ConflictRects {
    pub card: Rect,
    /// The ✕ of the accent title bar — same action as `close`.
    pub titlebar_close: Rect,
    /// The "Appliquer à tous" pill.
    pub apply_all: Rect,
    /// The list zone (clipped + scrollable as needed).
    pub list: Rect,
    /// One row per item (in the list's coordinates, including scroll).
    pub rows: Vec<Rect>,
    /// Each item's option pill (null rect if the item is not conflicting).
    pub options: Vec<Rect>,
    pub primary: Rect,
    pub close: Rect,
}

/// Conflict dialog geometry.
pub fn conflict_rects(dialog: &ConflictDialog, width: f32, height: f32, scroll: f32) -> ConflictRects {
    let list_h = (dialog.items.len() as f32 * CONFLICT_ROW_H).min(CONFLICT_LIST_MAX);
    let apply_h = 40.0;
    let desc_h = if dialog.description.is_empty() { 0.0 } else { 44.0 };
    let card_h =
        TITLEBAR_H + PAD + apply_h + 8.0 + desc_h + 8.0 + 13.0 + list_h + PAD + FOOTER_H;
    let left = (width - CONFLICT_WIDTH) / 2.0;
    let top = ((height - card_h) / 2.0).max(8.0);
    let card = Rect::new(left, top, left + CONFLICT_WIDTH, top + card_h);
    let titlebar_close = titlebar_close_rect(&card);

    // "Appliquer à tous": label on the left, option pill on the right.
    let ay = card.top + TITLEBAR_H + PAD;
    let apply_all = Rect::new(card.right - PAD - OPTION_PILL_W, ay + 4.0, card.right - PAD, ay + apply_h - 4.0);

    // The separator, then the list.
    let list_top = ay + apply_h + 8.0 + desc_h + 8.0 + 13.0;
    let list = Rect::new(card.left + PAD, list_top, card.right - PAD, list_top + list_h);

    let mut rows = Vec::new();
    let mut options = Vec::new();
    let mut y = list.top - scroll;
    for item in &dialog.items {
        let row = Rect::new(list.left, y, list.right, y + CONFLICT_ROW_H);
        let opt = if item.is_conflict {
            Rect::new(row.right - OPTION_PILL_W, row.top + 12.0, row.right, row.bottom - 12.0)
        } else {
            Rect::new(0.0, 0.0, 0.0, 0.0)
        };
        rows.push(row);
        options.push(opt);
        y += CONFLICT_ROW_H;
    }

    // Footer: "Continuer" left, "Annuler" right (Kubuno order).
    let by = card.bottom - FOOTER_H + FOOTER_PAD_Y;
    let (pw, cw) = button_widths(
        dialog.primary_button(),
        drive_localization::tr("Cancel"),
        CONFLICT_WIDTH,
    );
    let primary = Rect::new(card.left + PAD, by, card.left + PAD + pw, by + BUTTON_H);
    let close = Rect::new(card.right - PAD - cw, by, card.right - PAD, by + BUTTON_H);
    ConflictRects { card, titlebar_close, apply_all, list, rows, options, primary, close }
}

impl Painter<'_> {
    /// `--kb-shadow-window` (`0 6px 18px rgba(0,0,0,.24)`): approximated by
    /// stacked translucent fills, the same trick as `draw_card_shadow`.
    fn draw_window_shadow(&self, rect: &Rect, radius: f32) {
        use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;
        const STEPS: usize = 6;
        let Some(layer) = shape::SHADOW_WINDOW.first() else {
            return;
        };
        for i in 1..=STEPS {
            let spread = layer.blur * (i as f32 / STEPS as f32);
            let shadow = Rect::new(
                rect.left - spread,
                rect.top - spread + layer.dy,
                rect.right + spread,
                rect.bottom + spread + layer.dy,
            );
            let color =
                D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: layer.opacity / STEPS as f32 };
            self.fill_rounded(&shadow, radius + spread, &color);
        }
    }

    /// The `FloatingWindow` chassis shared by every dialog: scrim, shadowed
    /// card (`--kb-window-radius`), full-accent `.kb-window-titlebar` with its
    /// ✕, and the `.kb-window-footer` hairline. The body is the caller's.
    fn draw_window_chrome(
        &self,
        card: &Rect,
        close_button: &Rect,
        title: &str,
        hot_close: bool,
        width: f32,
        height: f32,
    ) {
        use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;
        let t = self.theme;
        let f = &self.renderer.formats;

        // The scrim covers the WHOLE window (`rgba(0,0,0,.30)`).
        self.fill_rounded(
            &Rect::new(0.0, 0.0, width, height),
            0.0,
            &D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: SMOKE },
        );
        // The card: opaque surface, radius 6, window shadow, no border.
        self.draw_window_shadow(card, shape::radius::LG);
        self.fill_rounded(card, shape::radius::LG, &t.layer_background);

        // `.kb-window-titlebar`: solid accent, only the TOP corners rounded.
        let bar = Rect::new(card.left, card.top, card.right, card.top + TITLEBAR_H);
        self.fill_top_rounded(&bar, shape::radius::LG, &t.accent);
        let label = Rect::new(
            bar.left + shape::space::LG,
            bar.top,
            close_button.left - shape::space::SM,
            bar.bottom,
        );
        self.text_ellipsis(title, &label, &f.heading, &t.accent_foreground);

        // The ✕: white glyph, white-20% wash on hover.
        if hot_close {
            self.fill_rounded(
                close_button,
                TITLE_CLOSE_RADIUS,
                &D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.20 },
            );
        }
        self.text("\u{E711}", close_button, &f.icon_small, &t.accent_foreground, true);

        // `.kb-window-footer`: same surface as the card, 1px rule on top.
        let footer_top = card.bottom - FOOTER_H;
        self.fill_rounded(
            &Rect::new(card.left, footer_top, card.right, footer_top + 1.0),
            0.0,
            &t.divider,
        );
    }

    /// A footer button — `Button variant="text"` in the web: NEVER filled at
    /// rest, a tinted wash on hover, label in the button's own colour.
    fn draw_text_button(
        &self,
        rect: &Rect,
        label: &str,
        color: &windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F,
        hover: &windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F,
        hot: bool,
    ) {
        if hot {
            self.fill_rounded(rect, shape::radius::SM, hover);
        }
        self.text(label, rect, &self.renderer.formats.body, color, true);
    }

    /// Paints the `FilesystemOperationDialog` in conflict mode: smoke layer,
    /// card, "apply to all", warning description, list of items with their
    /// option pill, Continuer / Annuler buttons.
    pub(crate) fn draw_conflict_dialog(
        &self,
        dialog: &ConflictDialog,
        width: f32,
        height: f32,
        scroll: f32,
        hot: Option<crate::ui::Hot>,
    ) {
        use crate::ui::Hot;
        let t = self.theme;
        let f = &self.renderer.formats;
        let tr = drive_localization::tr;

        let r = conflict_rects(dialog, width, height, scroll);
        // Scrim, card, accent title bar, footer rule — the shared chassis.
        self.draw_window_chrome(
            &r.card,
            &r.titlebar_close,
            &dialog.title,
            hot == Some(Hot::DialogClose),
            width,
            height,
        );

        // "Appliquer cette action à tous les éléments en conflit" + pill.
        let apply_label = Rect::new(r.card.left + PAD, r.apply_all.top, r.apply_all.left - 8.0, r.apply_all.bottom);
        self.text(tr("ApplyToAllConflictingItems"), &apply_label, &f.body, &t.text_primary, false);
        let agg_text = match dialog.aggregated {
            Some(o) => tr(o.label()),
            None => tr("Custom"),
        };
        self.draw_option_pill(&r.apply_all, agg_text, hot == Some(Hot::ConflictApplyAll));

        // Description (warning, caution color).
        if !dialog.description.is_empty() {
            let dy = r.apply_all.bottom + 8.0;
            let desc = Rect::new(r.card.left + PAD, dy, r.card.right - PAD, dy + 40.0);
            self.text(&dialog.description, &desc, &f.body, &t.caution, false);
        }

        // Separator line above the list.
        self.fill_rounded(&Rect::new(r.list.left, r.list.top - 8.0, r.list.right, r.list.top - 7.0), 0.0, &t.divider);

        // List of items (clipped).
        unsafe {
            self.ctx.PushAxisAlignedClip(&r.list.d2d(), windows::Win32::Graphics::Direct2D::D2D1_ANTIALIAS_MODE_ALIASED);
        }
        for (i, (item, row)) in dialog.items.iter().zip(&r.rows).enumerate() {
            if row.bottom < r.list.top || row.top > r.list.bottom {
                continue;
            }
            // Icon: folder or file (glyph — the dialog has no IconCache).
            let icon = Rect::new(row.left, row.top + 12.0, row.left + 32.0, row.bottom - 12.0);
            let is_dir = std::path::Path::new(&item.source).is_dir();
            let glyph = if is_dir { "\u{E8B7}" } else { "\u{E8A5}" };
            self.text(glyph, &icon, &f.icon, &t.text_secondary, true);
            let text_left = icon.right + 12.0;
            let text_right = if item.is_conflict { r.options[i].left - 12.0 } else { row.right - 8.0 };
            // Top line: "source → dest" (the original's E72A chevron).
            let mid = (row.top + row.bottom) / 2.0;
            let top_row = Rect::new(text_left, row.top + 8.0, text_right, mid);
            let arrow = if item.source_name == item.dest_name {
                item.source_name.clone()
            } else {
                format!("{}  \u{E72A}  {}", item.source_name, item.dest_name)
            };
            self.text_ellipsis(&arrow, &top_row, &f.body, &t.text_primary);
            // Bottom line: the full source path (grey).
            let sub = Rect::new(text_left, mid, text_right, row.bottom - 8.0);
            self.text_ellipsis(&item.source, &sub, &f.caption, &t.text_secondary);
            // Option pill (only for conflicts).
            if item.is_conflict {
                self.draw_option_pill(&r.options[i], tr(item.resolve.label()), hot == Some(Hot::ConflictOption(i)));
            }
        }
        unsafe {
            self.ctx.PopAxisAlignedClip();
        }

        // Footer text buttons: Continuer (accent) left, Annuler right.
        self.draw_text_button(
            &r.primary,
            dialog.primary_button(),
            &t.accent,
            &t.accent_light,
            hot == Some(Hot::DialogPrimary),
        );
        self.draw_text_button(
            &r.close,
            tr("Cancel"),
            &t.text_secondary,
            &t.control_fill_hover,
            hot == Some(Hot::DialogClose),
        );
    }

    /// An option pill (the dialog's ComboBox): fill, border, label, a "⌄"
    /// chevron on the right to signal it expands on click.
    fn draw_option_pill(&self, rect: &Rect, label: &str, hot: bool) {
        let t = self.theme;
        let f = &self.renderer.formats;
        // `Select`: card surface, `--color-border`, radius 4.
        let fill = if hot { t.control_fill_hover } else { t.layer_background };
        self.fill_rounded(rect, shape::radius::SM, &fill);
        self.stroke_rounded(rect, shape::radius::SM, &t.card_stroke);
        let lab = Rect::new(rect.left + 10.0, rect.top, rect.right - 24.0, rect.bottom);
        self.text(label, &lab, &f.body, &t.text_primary, false);
        let chev = Rect::new(rect.right - 22.0, rect.top, rect.right - 6.0, rect.bottom);
        self.text("\u{E70D}", &chev, &f.icon_tiny, &t.text_secondary, true);
    }

    /// Paints the smoke layer then the `ContentDialog`'s card.
    #[allow(clippy::too_many_arguments)] // a paint entry point: the frame's inputs, passed flat
    pub(crate) fn draw_dialog(
        &self,
        dialog: &DialogState,
        edit: Option<&crate::ui::EditState>,
        width: f32,
        height: f32,
        hot_primary: bool,
        hot_close: bool,
        hot_item: Option<usize>,
    ) {
        let t = self.theme;
        let f = &self.renderer.formats;

        let r = dialog_rects(dialog, width, height);
        let (card, primary, close) = (r.card, r.primary, r.close);
        // Scrim, card, accent title bar (the title lives THERE now), footer rule.
        self.draw_window_chrome(&card, &r.titlebar_close, &dialog.title, hot_close, width, height);

        // The message stays in the body: 14px, secondary.
        if !dialog.subtitle.is_empty() {
            let body_top = card.top + TITLEBAR_H + PAD;
            let subtitle =
                Rect::new(card.left + PAD, body_top, card.right - PAD, body_top + SUBTITLE_H);
            self.text_ellipsis(&dialog.subtitle, &subtitle, &f.body, &t.text_secondary);
        }

        // The "Nom" field (`Input`: 36 tall, radius 4, `--color-border`) and
        // the "Format" choice (pills).
        if let (Some(label), Some(field)) = (&dialog.field_label, &r.field) {
            let lab = Rect::new(card.left + PAD, field.top, field.left - shape::space::SM, field.bottom);
            self.text(label, &lab, &f.body, &t.text_primary, false);
            self.fill_rounded(field, shape::radius::SM, &t.layer_background);
            match edit {
                Some(e) => {
                    // A dialog field always owns the caret: accent focus ring.
                    self.stroke_rounded_w(field, shape::radius::SM, &t.accent, 2.0);
                    drive_app_controls::edit_box::draw_text(
                        self,
                        field,
                        &drive_app_controls::EditView { text: &e.text, caret: e.caret, anchor: e.anchor },
                    );
                }
                None => self.stroke_rounded(field, shape::radius::SM, &t.card_stroke),
            }
        }
        if let Some(label) = &dialog.choices_label {
            if let Some(first) = r.choices.first() {
                let lab = Rect::new(card.left + PAD, first.top, first.left - shape::space::SM, first.bottom);
                self.text(label, &lab, &f.body, &t.text_primary, false);
            }
            for (i, c) in r.choices.iter().enumerate() {
                if i == dialog.choice {
                    self.fill_rounded(c, shape::radius::SM, &t.accent);
                    self.text_center(&dialog.choices[i], c, &t.accent_foreground);
                } else {
                    self.fill_rounded(c, shape::radius::SM, &t.layer_background);
                    self.stroke_rounded(c, shape::radius::SM, &t.card_stroke);
                    self.text_center(&dialog.choices[i], c, &t.text_primary);
                }
            }
        }

        // Checkbox: 20 box, radius 4, check (E73E) on accent when checked.
        if let (Some(label), Some(row)) = (&dialog.checkbox_label, &r.checkbox) {
            let cy = (row.top + row.bottom) / 2.0;
            let bx = Rect::new(row.left, cy - 10.0, row.left + 20.0, cy + 10.0);
            if dialog.checkbox {
                self.fill_rounded(&bx, shape::radius::SM, &t.accent);
                self.text("\u{E73E}", &bx, &f.icon_tiny, &t.accent_foreground, true);
            } else {
                self.fill_rounded(&bx, shape::radius::SM, &t.layer_background);
                self.stroke_rounded(&bx, shape::radius::SM, &t.card_stroke);
            }
            let lab = Rect::new(bx.right + shape::space::SM, row.top, row.right, row.bottom);
            self.text(label, &lab, &f.body, &t.text_primary, false);
        }

        // The AddItemDialog's list: 24 icon, title, grey subtitle.
        for (i, (item, row)) in dialog.list.iter().zip(&r.list).enumerate() {
            if hot_item == Some(i) {
                self.fill_rounded(row, shape::radius::MENU_ITEM, &t.row_hover);
            }
            let icon = Rect::new(row.left + 8.0, row.top, row.left + 8.0 + 24.0, row.bottom);
            self.text(&item.glyph, &icon, &f.icon, &t.text_primary, true);
            let text_left = icon.right + 10.0;
            let mid = (row.top + row.bottom) / 2.0;
            let header = Rect::new(text_left, row.top + 4.0, row.right - 8.0, mid);
            self.text(&item.header, &header, &f.body, &t.text_primary, false);
            let sub = Rect::new(text_left, mid, row.right - 8.0, row.bottom - 4.0);
            self.text(&item.sub_header, &sub, &f.caption, &t.text_secondary, false);
        }

        // Footer TEXT buttons: confirm on the LEFT (danger tone when it
        // destroys), cancel on the RIGHT. Never filled at rest.
        if !dialog.primary_text.is_empty() {
            let (color, hover) = if dialog.is_destructive() {
                (t.danger, t.danger_light)
            } else {
                (t.accent, t.accent_light)
            };
            self.draw_text_button(&primary, &dialog.primary_text, &color, &hover, hot_primary);
        }
        self.draw_text_button(
            &close,
            &dialog.close_text,
            &t.text_secondary,
            &t.control_fill_hover,
            hot_close,
        );
    }

    fn text_center(&self, s: &str, rect: &Rect, color: &windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F) {
        self.text(s, rect, &self.renderer.formats.body, color, true);
    }
}
