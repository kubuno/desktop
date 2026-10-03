//! Port of `Files.App/UserControls/Toolbar.xaml`: the command bar under the
//! navigation toolbar.
//!
//! The bar is [`kubuno_desktop_ui::navigation::Toolbar`], the design system's own: the
//! commands are `ToolStripItem`s (so `Enabled`, `Checked`, the separators and
//! the drop-downs are the replica's fields, stored once), and their
//! arrangement is `kubuno_drive_desktop_app_controls::toolbar::arrange`, which the primitive
//! wraps. What is written here is the binding: which commands the current
//! context shows, which `Hot` each of them answers to, and the one thing the
//! primitive has no notion of — that Drive's bar is two blocks, a primary one
//! against the left edge and a `BaseCommandBar` packed against the right.
//!
//! Overflow is turned OFF on every item ([`ToolStripItemOverflow::Never`]):
//! the primitive would happily fold the tail behind a « More » button, but the
//! application has no `Hot` for that button and no menu to open, so it would
//! draw a control nothing can click. It becomes a two-line change here the day
//! that menu exists.

use kubuno_desktop_controls::toolstrip::{StripItem, ToolStripItemOverflow};
use kubuno_desktop_ui::navigation::{icon_item, icon_label_item, menu_item, separator_item, toggle_item, Toolbar};

use crate::ui::{Hot, Layout, Painter, Rect, UiState, CMDBAR_BUTTON_HEIGHT};

/// One block's measured widths, as [`Toolbar::item_widths`] reports them:
/// `(index into that block's items, desired width)`.
type BlockWidths = Vec<(usize, f32)>;
/// The left block's widths and the right block's.
type CmdWidths = (BlockWidths, BlockWidths);

/// How far the button row is inset from the card's own left edge, and how far
/// the right block stops short of its right edge (`Toolbar.xaml`'s
/// `ToolbarInnerPadding` as the shipping bar applies it).
const CARD_INSET_LEFT: f32 = 12.0;
const CARD_INSET_RIGHT: f32 = 8.0;
/// The card is 48 DIP tall and encloses centred 36 DIP `AppBarButton`s, so it
/// overflows the row by 6 above and below.
const ROW_INSET_TOP: f32 = 6.0;
/// Breathing room kept between the two blocks when the window gets narrow —
/// the left block never grows into the right one's box.
const BLOCK_GAP: f32 = 8.0;

thread_local! {
    /// The width each command asked for on the last frame, straight from
    /// [`Toolbar::item_widths`], for the left block and the right one.
    ///
    /// A command's width is a DirectWrite extent as soon as it carries a
    /// caption (« Nouveau », the Recycle Bin's three), and `Layout::compute` —
    /// which needs every button's rectangle to hit-test it — is static and has
    /// no drawing surface. So the widths are taken once per frame in
    /// [`premeasure`], while a `Canvas` exists, and the layout pass reuses
    /// them: the primitive measures and arranges once, and the hit test and
    /// the paint share the result.
    static CMD_WIDTHS: std::cell::RefCell<CmdWidths> =
        const { std::cell::RefCell::new((Vec::new(), Vec::new())) };
}

/// Drive's command bar: two [`Toolbar`]s, each with the `Hot` its items answer
/// to, index-aligned with `Toolbar::items`.
pub(crate) struct CmdBar {
    pub(crate) left: Toolbar,
    pub(crate) left_hots: Vec<Hot>,
    pub(crate) right: Toolbar,
    pub(crate) right_hots: Vec<Hot>,
}

/// A block of commands, built from `(item, Hot)` pairs.
fn block(items: Vec<(StripItem, Hot)>) -> (Toolbar, Vec<Hot>) {
    let mut bar = Toolbar::new();
    bar.row_height = CMDBAR_BUTTON_HEIGHT;
    let mut hots = Vec::with_capacity(items.len());
    for (mut item, hot) in items {
        // See the module header: no command may fold into an overflow menu
        // that the application cannot open.
        match &mut item {
            StripItem::Button(b) => b.item.overflow = ToolStripItemOverflow::Never,
            StripItem::MenuItem(m) => m.base.item.overflow = ToolStripItemOverflow::Never,
            StripItem::Separator(s) => s.item.overflow = ToolStripItemOverflow::Never,
            _ => {}
        }
        bar = bar.with(item);
        hots.push(hot);
    }
    (bar, hots)
}

/// A command that is greyed out when its precondition does not hold.
fn enabled(mut item: StripItem, on: bool) -> StripItem {
    if let StripItem::Button(b) = &mut item {
        b.item.enabled = on;
    }
    item
}

/// The commands the current context shows.
///
/// Recycle Bin gets the « RecycleBin » context (`ToolbarSections.cs:51-56`,
/// mapping in `ToolbarItemDescriptor.cs:184`): ONLY three buttons, all
/// labelled (`showLabel: true`), and no right block. Every other folder gets
/// the « AlwaysVisible » context plus the `BaseCommandBar` on the right.
pub(crate) fn build_cmdbar(state: &UiState) -> CmdBar {
    use kubuno_drive_desktop_localization::tr;

    let tab = state.active();
    let has_selection = !tab.selected.is_empty();
    let has_items = !tab.entries.is_empty();
    let settings = crate::services::settings::get();

    if tab.location == crate::view_models::shell_view_model::Location::RecycleBin {
        let (left, left_hots) = block(vec![
            (
                enabled(icon_label_item("Delete", tr("EmptyRecycleBin")), has_items),
                Hot::CmdEmptyRecycleBin,
            ),
            (
                enabled(icon_label_item("RestoreDeleted", tr("RestoreAllItems")), has_items),
                Hot::CmdRestoreAllRecycleBin,
            ),
            (
                enabled(icon_label_item("RestoreDeleted", tr("Restore")), has_selection),
                Hot::CmdRestoreRecycleBin,
            ),
        ]);
        let (right, right_hots) = block(Vec::new());
        return CmdBar { left, left_hots, right, right_hots };
    }

    // LEFT block — primary commands from the « AlwaysVisible » context
    // (`ToolbarSections.DefaultItemsByContext`), in the exact order:
    //   Nouveau ▾ | (separator) | Couper | Copier | Coller | Renommer
    //   | Partager | Supprimer | Propriétés.
    let mut items = vec![
        (menu_item("Plus", tr("BaseLayoutContextFlyoutNew.Label")), Hot::CmdNew),
        (separator_item(), Hot::CmdSeparator),
    ];
    for (icon, hot) in [
        ("Cut", Hot::CmdCut),
        ("Copy", Hot::CmdCopy),
        ("Paste", Hot::CmdPaste),
        ("Rename", Hot::CmdRename),
        ("Share", Hot::CmdShare),
        ("Delete", Hot::CmdDelete),
        ("Properties", Hot::CmdProperties),
    ] {
        let on = matches!(hot, Hot::CmdPaste | Hot::CmdProperties) || has_selection;
        items.push((enabled(icon_item(icon), on), hot));
    }
    let (left, left_hots) = block(items);

    // RIGHT block — `BaseCommandBar`, in the XAML's exact order:
    //   Filtre | Options de sélection ▾ | Trier ▾ | Disposition ▾ | volet de
    //   détails | étagère. No « Grouper » button: grouping lives in the Trier
    //   flyout (MenuFlyoutSubItem).
    let (right, right_hots) = block(vec![
        (icon_item("Filter"), Hot::CmdFilter),
        (menu_item("SelectMode", ""), Hot::CmdSelOptions),
        (menu_item("Sorting", ""), Hot::CmdSort),
        (menu_item(tab.view_mode.icon(), ""), Hot::CmdLayout),
        (toggle_item("PanelRight", settings.show_info_pane), Hot::CmdInfoPane),
        (toggle_item("Shelf", settings.show_shelf_pane), Hot::CmdShelf),
    ]);

    CmdBar { left, left_hots, right, right_hots }
}

/// Where each block is laid: the left one from the card's inner edge, the
/// right one packed against the opposite edge.
///
/// The packing is the one thing [`arrange`] cannot do — it walks left to right
/// from the origin it is given — so the right block's origin is computed here
/// from the width its items add up to, spacing included. Everything past that
/// origin is the primitive's.
///
/// [`arrange`]: kubuno_drive_desktop_app_controls::toolbar::arrange
fn block_bounds(bar: &CmdBar, card: Rect, widths: &CmdWidths) -> (Rect, Rect) {
    use kubuno_drive_desktop_app_controls::toolbar::ITEM_SPACING;

    let top = card.top + ROW_INSET_TOP;
    let bottom = top + bar.left.row_height;
    let right_edge = card.right - CARD_INSET_RIGHT;
    let total: f32 = widths.1.iter().map(|&(_, w)| w).sum::<f32>()
        + ITEM_SPACING * widths.1.len().saturating_sub(1) as f32;
    let right = Rect::new(right_edge - total, top, right_edge, bottom);
    let left_edge = card.left + CARD_INSET_LEFT;
    let left_right = if widths.1.is_empty() { right_edge } else { right.left - BLOCK_GAP };
    let left = Rect::new(left_edge, top, left_right.max(left_edge), bottom);
    (left, right)
}

/// Measures the bar's commands while a drawing surface exists, for the layout
/// pass that follows. See [`CMD_WIDTHS`].
pub(crate) fn premeasure(c: &dyn kubuno_desktop_ui::Canvas, state: &UiState) {
    let bar = build_cmdbar(state);
    let widths = (bar.left.item_widths(c), bar.right.item_widths(c));
    CMD_WIDTHS.with(|w| *w.borrow_mut() = widths);
}

/// Every command's rectangle and the `Hot` it answers to, for the layout pass
/// to hit-test — arranged by the primitive itself, so the box the pointer hits
/// is the box that was drawn.
pub(crate) fn cmdbar_arrangement(state: &UiState, card: Rect) -> Vec<(Rect, Hot)> {
    let bar = build_cmdbar(state);
    CMD_WIDTHS.with(|w| {
        let widths = fitting(&bar, &w.borrow());
        let (lb, rb) = block_bounds(&bar, card, &widths);
        let mut out = Vec::with_capacity(bar.left_hots.len() + bar.right_hots.len());
        for (block, hots, measures, bounds) in [
            (&bar.left, &bar.left_hots, &widths.0, lb),
            (&bar.right, &bar.right_hots, &widths.1, rb),
        ] {
            for (i, rect) in block.arrange_widths(measures, bounds).visible {
                if let Some(hot) = hots.get(i) {
                    out.push((rect, *hot));
                }
            }
        }
        out
    })
}

/// The cached widths, but only if they still describe THIS bar.
///
/// `Layout::compute` also runs between two frames — on a mouse move, say — and
/// the command set changes with the location (the Recycle Bin shows three
/// buttons where a folder shows nine). A measurement taken for the other set
/// indexes items that are no longer there, so it is dropped: the bar has no
/// hit boxes until the repaint that follows the navigation measures it again,
/// which is the same frame that draws it.
fn fitting(bar: &CmdBar, widths: &CmdWidths) -> CmdWidths {
    let keep = |block: &Toolbar, w: &BlockWidths| {
        if w.iter().all(|&(i, _)| i < block.items.len()) {
            w.clone()
        } else {
            Vec::new()
        }
    };
    (keep(&bar.left, &widths.0), keep(&bar.right, &widths.1))
}

impl Painter<'_> {
    /// Command bar under the toolbar (port of the original Toolbar row).
    pub(crate) fn draw_cmdbar(&self, layout: &Layout, state: &UiState) {
        let Some(card) = layout.cmdbar else { return };
        // The web has NO toolbar card: the command bar sits straight on the
        // module panel — no border, no shadow, no radius. The content card
        // right below carries the separation.
        self.fill_rounded(&card, 0.0, &self.theme.layer_background);

        let bar = build_cmdbar(state);
        let (lb, rb) = CMD_WIDTHS.with(|w| block_bounds(&bar, card, &fitting(&bar, &w.borrow())));
        let hot = |hots: &[Hot]| hots.iter().position(|h| state.hot == Some(*h));
        bar.left.paint_items(self, lb, hot(&bar.left_hots), false);
        if !bar.right_hots.is_empty() {
            bar.right.paint_items(self, rb, hot(&bar.right_hots), false);
        }
    }
}
