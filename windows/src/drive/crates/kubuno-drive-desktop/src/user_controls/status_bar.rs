//! Port of `Files.App/UserControls/StatusBar.xaml`: item count + selection
//! line at the bottom of folder views.
//!
//! The strip itself is [`kubuno_desktop_ui::navigation::StatusBar`], the design
//! system's own: its cells are `ToolStripStatusLabel`s / `ToolStripButton`s,
//! and the leading cell is the one that `Spring`s — the `StatusStrip` flag
//! that hands the leftover width to a cell, which is exactly what the count +
//! selection line does between the left edge and the git widgets. Neither the
//! sharing nor the cell paint is written here any more; what stays is what the
//! primitive cannot know: the ICU plural strings, the size rollup, and which
//! cells the git repo contributes.

use kubuno_desktop_controls::toolstrip::{StripItem, ToolStripItemAlignment};
use kubuno_desktop_ui::navigation::{icon_label_item, status_item, StatusBar};

use crate::ui::{Hot, Layout, Painter, Rect, UiState, STATUSBAR_HEIGHT};
use crate::view_models::shell_view_model::{Location, Tab};

/// `StatusBar.xaml`'s right `Padding` — the gap the strip keeps between its
/// last cell and the window edge.
const EDGE_PADDING: f32 = 8.0;

thread_local! {
    /// The width each cell asked for on the last frame, straight from
    /// [`StatusBar::item_widths`].
    ///
    /// The primitive measures its captions through DirectWrite, and
    /// `Layout::compute` — which needs the git widgets' rectangles to hit-test
    /// them — is static and has no drawing surface. So the widths are taken
    /// once per frame in [`premeasure`], while a `Canvas` exists, and the
    /// layout pass reuses them: one measurement, one arrangement, shared by
    /// the hit test and the paint.
    static CELL_WIDTHS: std::cell::RefCell<Vec<f32>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// Evaluates an ICU plural (`{0, plural, one {…} other {…}}`) — the format of the
/// `SelectedItems`/`DeleteItemsDialogTitle` resources. The text AROUND the
/// block is preserved ("Supprimer {0, plural, …}" keeps its "Supprimer").
pub(crate) fn icu_plural(pattern: &str, count: usize) -> String {
    let Some(block_start) = pattern.find("{0, plural,") else {
        return pattern.replace('#', &count.to_string());
    };
    // The end of the block: the closing brace of `{0, plural, …}`.
    let mut depth = 0usize;
    let mut block_end = pattern.len();
    for (i, c) in pattern[block_start..].char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    block_end = block_start + i + 1;
                    break;
                }
            }
            _ => {}
        }
    }
    let block = &pattern[block_start..block_end];
    let branch_tag = if count == 1 { "one {" } else { "other {" };
    let replacement = block
        .find(branch_tag)
        .map(|p| p + branch_tag.len())
        .and_then(|s| block[s..].find('}').map(|e| &block[s..s + e]))
        .unwrap_or("");
    format!(
        "{}{}{}",
        &pattern[..block_start],
        replacement.replace('#', &count.to_string()),
        &pattern[block_end..]
    )
}

/// Whether the bar shows at all — a folder view, with the setting on.
pub(crate) fn is_visible(tab: &Tab) -> bool {
    matches!(tab.location, Location::Dir(_)) && crate::services::settings::get().show_status_bar
}

/// The count + selection line, the cell that springs.
///
/// `Strings.Items` (ICU): "1 élément" / "19 éléments", like
/// `directoryItemCountLocalization` in `BaseShellPage`; then, if there is a
/// selection, "N éléments sélectionnés" (the `SelectedItems` resource, also an
/// ICU plural, like in `StatusBarViewModel`); then `ItemSize`
/// (`StatusBar.xaml` via `UpdateSelectionSize`) — the cumulative size of the
/// selection, or of ALL items if nothing is selected, shown only if every size
/// is known (`isSizeKnown`): an uncomputed folder hides the block.
fn summary(tab: &Tab) -> String {
    let mut text = format!(
        "{} {}",
        tab.entries.len(),
        icu_plural(kubuno_drive_desktop_localization::tr("Items"), tab.entries.len())
    );
    if !tab.selected.is_empty() {
        let plural = icu_plural(kubuno_drive_desktop_localization::tr("SelectedItems"), tab.selected.len());
        text.push_str(&format!("    |    {plural}"));
    }
    let sized: Vec<&crate::data::items::DirEntryItem> = if tab.selected.is_empty() {
        tab.entries.iter().collect()
    } else {
        tab.selected.iter().filter_map(|&i| tab.entries.get(i)).collect()
    };
    if !sized.is_empty() && sized.iter().all(|e| e.size_known) {
        let total: u64 = sized.iter().map(|e| e.size).sum();
        text.push_str(&format!("    |    {}", crate::data::items::format_bytes_fr(total)));
    }
    text
}

/// The bar's cells: the summary, then — inside a repo only — the two git
/// widgets of `StatusBar.xaml`'s columns 1 and 2, packed against the trailing
/// edge by the `Alignment` the replica already carries.
///
/// The returned `Hot`s are index-aligned with the cells, so one lookup answers
/// both « which cell is under the pointer » and « which cell is hot ».
fn build(tab: &Tab) -> (StatusBar, Vec<Option<Hot>>) {
    let mut bar = StatusBar::new().with(status_item(&summary(tab), true));
    let mut hots = vec![None];
    if let Some(branch) = tab.git_branch.as_deref() {
        let counter = format!("{} / {}", tab.git_ahead, tab.git_behind);
        for (mut item, hot) in [
            (icon_label_item("Git", &counter), Hot::StatusGitActions),
            (icon_label_item("Git.Branch", branch), Hot::StatusGitBranch),
        ] {
            if let StripItem::Button(b) = &mut item {
                b.item.alignment = ToolStripItemAlignment::Right;
            }
            bar = bar.with(item);
            hots.push(Some(hot));
        }
    }
    (bar, hots)
}

/// The strip's own rectangle: the content region's left edge, the window's
/// bottom band, minus the trailing padding.
pub(crate) fn bounds(content_left: f32, width: f32, height: f32) -> Rect {
    Rect::new(content_left, height - STATUSBAR_HEIGHT, width - EDGE_PADDING, height)
}

/// Measures the bar's cells while a drawing surface exists, for the layout
/// pass that follows. See [`CELL_WIDTHS`].
pub(crate) fn premeasure(c: &dyn kubuno_desktop_ui::Canvas, state: &UiState) {
    let tab = state.active();
    if !is_visible(tab) {
        return;
    }
    let (bar, _) = build(tab);
    let widths = bar.item_widths(c);
    CELL_WIDTHS.with(|w| *w.borrow_mut() = widths);
}

/// The git widgets' rectangles (network actions, branch selector), for the
/// layout pass to hit-test — arranged by the primitive itself, so the box the
/// pointer hits is the box that was drawn.
pub(crate) fn git_rects(tab: &Tab, bounds: Rect) -> (Option<Rect>, Option<Rect>) {
    if tab.git_branch.is_none() {
        return (None, None);
    }
    let (bar, _) = build(tab);
    let layout = CELL_WIDTHS.with(|w| bar.item_rects_with(&w.borrow(), bounds));
    let cell = |i: usize| layout.rects.get(i).copied().filter(|r| r.right > r.left);
    (cell(1), cell(2))
}

impl Painter<'_> {
    /// Status bar: item count + selection (port of StatusBar.xaml).
    pub(crate) fn draw_statusbar(&self, layout: &Layout, state: &UiState) {
        let tab = state.active();
        if !is_visible(tab) {
            return;
        }
        let (bar, hots) = build(tab);
        let hot = hots.iter().position(|h| h.is_some() && *h == state.hot);
        bar.paint_items(self, bounds(layout.content.left, layout.width, layout.height), hot);
    }
}
