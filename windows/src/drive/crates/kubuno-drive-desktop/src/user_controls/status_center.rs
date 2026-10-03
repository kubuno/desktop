//! Port of `Files.App/UserControls/StatusCenter/StatusCenter.xaml`: the
//! flyout (Width=400, MinHeight=120, MaxHeight=500) opened by the
//! StatusCenter button of the navigation toolbar, listing the file
//! operations reported by the IFileOperation progress sink.
//!
//! Despite the name this is NOT a status bar: it is a progress/notification
//! surface, so its design-system counterpart is `feedback`, not `navigation`.
//! The « no operation » state is [`kubuno_desktop_ui::feedback::EmptyState`] —
//! medallion, title, compact spacing, all of it the primitive's. The operation
//! rows stay hand-drawn: their web reference is `@ui/Toast.tsx`'s ToastCard,
//! and [`kubuno_desktop_ui::dialogs::Toast`] paints one WITH `--kb-shadow-float`,
//! which is right for a toast floating over the page and wrong here — these
//! are stacked inside a flyout that already carries that shadow, and a shadow
//! on a shadow reads as a smear. They will move to the primitive the day it
//! can be asked to drop its float. A determinate progress bar
//! (`MedianOperationProgressRing`) is likewise absent on purpose: `OpsMonitor`
//! only tracks (id, label), and `range::ProgressBar` at a made-up value would
//! betray fidelity.

use kubuno_drive_desktop_app_controls::themes::shape;
use kubuno_desktop_ui::feedback::EmptyState;
use kubuno_desktop_ui::{Widget, WidgetState};

use crate::ui::{Layout, Painter, Rect, UiState};

/// A card's height, and the stride between two cards — the web stacks its
/// toasts `gap-2` (8 DIP) apart (`Toast.tsx:180`).
const CARD_HEIGHT: f32 = 56.0;
const CARD_GAP: f32 = 8.0;

impl Painter<'_> {
    pub(crate) fn draw_status_center(&self, layout: &Layout, state: &UiState, ops: &[String]) {
        if !state.status_center_open {
            return;
        }
        let t = self.theme;
        let f = &self.renderer.formats;

        // Placement=BottomEdgeAlignedRight under the button.
        let anchor = layout
            .status_center_button
            .unwrap_or(Rect::new(layout.width - 52.0, 48.0, layout.width - 16.0, 80.0));
        let n = ops.len() as f32;
        let height = (16.0 + n * CARD_HEIGHT + (n - 1.0).max(0.0) * CARD_GAP).clamp(120.0, 500.0);
        let panel = Rect::new(
            anchor.right - 400.0,
            anchor.bottom + 4.0,
            anchor.right,
            anchor.bottom + 4.0 + height,
        );
        self.draw_flyout_panel(&panel);

        if ops.is_empty() {
            // The collection really is empty and nothing is filtered, so this
            // is the default `first-use` variant; `compact` because the panel
            // is only 120 tall. The mark is the very icon the button that
            // opened the flyout wears.
            let empty = EmptyState::new("StatusCenterIcon", kubuno_drive_desktop_localization::tr("NoFileOperations"))
                .with_compact(true);
            // The primitive lays its block out from the top of the box it is
            // given; the flyout centres it, so it gets a box of its own height.
            let h = empty.measure(self).height;
            let top = (panel.top + panel.bottom - h) / 2.0;
            empty.paint(self, Rect::new(panel.left, top, panel.right, top + h), WidgetState::REST);
            return;
        }
        // Operation cards — these ARE notifications, so they follow
        // `@ui/Toast.tsx`'s ToastCard (Toast.tsx:206): `rounded-lg` (6),
        // `border border-border`, `bg-surface-0`, `px-3 py-2.5`, a 16 DIP
        // variant glyph tinted `text-primary` (SKIN.info, Toast.tsx:48) and the
        // message at `--kb-text-body` `gap-2.5` (10 DIP) away.
        let mut y = panel.top + 8.0;
        for label in ops {
            let card = Rect::new(panel.left + 8.0, y, panel.right - 8.0, y + CARD_HEIGHT);
            self.fill_rounded(&card, shape::radius::LG, &t.layer_background);
            self.stroke_rounded(&card, shape::radius::LG, &t.card_stroke);
            let icon = Rect::new(card.left + 12.0, card.top, card.left + 28.0, card.bottom);
            self.text("\u{E895}", &icon, &f.icon, &t.accent, true);
            let label_rect = Rect::new(card.left + 38.0, card.top, card.right - 12.0, card.bottom);
            self.text_ellipsis(label, &label_rect, &f.body, &t.text_primary);
            y += CARD_HEIGHT + CARD_GAP;
        }
    }
}
