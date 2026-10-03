#![allow(unused_imports)]
use windows::core::Result;
use windows::Win32::Graphics::Direct2D::Common::{D2D1_COLOR_F, D2D_RECT_F};
use windows::Win32::Graphics::Direct2D::{
    ID2D1Bitmap1, ID2D1DeviceContext, ID2D1SolidColorBrush, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
    D2D1_INTERPOLATION_MODE_LINEAR, D2D1_ROUNDED_RECT,
};
use windows::Win32::Graphics::DirectWrite::{
    IDWriteTextFormat, DWRITE_MEASURING_MODE_NATURAL, DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
    DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_LEADING,
};

use crate::graphics::Renderer;
use crate::services::storage::IconCache;
use crate::data::items::{HomeModel, QuickAccessKind};
use crate::view_models::shell_view_model::{Location, Tab, TabGroup};
use crate::styles::theme::Theme;
use super::*;

pub struct UiState {
    pub sidebar_visible: bool,
    pub settings_section: usize,
    /// Appearance page expanders: [BgColor, BgImage, AddressBar, Toolbar].
    /// (BackgroundColor is IsExpanded="True" in the XAML.)
    pub appearance_expanded: [bool; 4],
    /// SettingsExpander state of every other settings section
    /// (indexed by section, then by the page's EXP_* constants).
    pub settings_expanded: [[bool; 4]; 9],
    pub flyout: Option<Flyout>,
    pub edit: Option<EditState>,
    /// Omnibar path-mode suggestions: (display label, full path).
    pub path_suggestions: Vec<(String, String)>,
    pub tabs: Vec<TabGroup>,
    pub active_tab: usize,
    pub hot: Option<Hot>,
    /// The pane splitter currently being dragged, if any — the resize rail
    /// stays lit (and turns accent) for the whole drag, even once the pointer
    /// has run past it.
    pub pane_dragging: Option<Hot>,
    pub maximized: bool,
    /// Background operations in flight (drives the StatusCenter button).
    pub ops_active: bool,
    /// Number of operations in progress (`OngoingTasksViewModel.InfoBadgeValue`).
    pub ops_count: usize,
    /// StatusCenter flyout visibility (toggled by its button).
    pub status_center_open: bool,
    /// Reorder animation, one entry per tab (see `TabSlide`).
    pub tab_slides: Vec<TabSlide>,
    /// Tab being dragged and the DIP offset from its slot to the cursor.
    pub tab_drag_offset: Option<(usize, f32)>,
    /// A tab from ANOTHER window is hovering our strip at this index: the
    /// layout reserves an empty slot (insertion preview, like the TabView
    /// during a TabStripDragOver).
    pub tab_preview_insert: Option<usize>,
    /// The pane's « Modifier les étiquettes » clickable zone, captured BY THE
    /// DRAWING (its position depends on measured text heights) — hence the
    /// `Cell`: drawing only has `&UiState`.
    pub info_edit_tags_rect: std::cell::Cell<Option<(f32, f32, f32, f32)>>,
    /// Info pane scroll (the original `RootPropertiesScrollViewer`: name,
    /// rows, tags AND the Properties button all scroll).
    pub info_pane_scroll: f32,
    /// Total height of the scrolling content, captured by the drawing
    /// (bounds the wheel).
    pub info_pane_extent: std::cell::Cell<f32>,
    /// Rect of the Properties button, captured by the drawing (it's part of
    /// the scrolling FLOW — its position varies with the content).
    pub info_properties_rect: std::cell::Cell<Option<(f32, f32, f32, f32)>>,
    /// Last scroll: WinUI's `ScrollBar` only shows its indicator while
    /// scrolling, then it fades out.
    pub scrolled_at: Option<std::time::Instant>,
    /// The pointer is in the gutter: the bar expands.
    pub scrollbar_expanded: bool,
    /// Thumb drag: the offset grabbed inside it.
    pub scroll_drag: Option<f32>,
    /// The sidebar tree: EXPANDED paths (chevron) and the cache of their
    /// child folders (name, path), filled in on expand.
    pub sidebar_expanded: std::collections::BTreeSet<String>,
    pub sidebar_children: std::collections::HashMap<String, Vec<(String, String)>>,
    /// The sidebar's scroll (its ScrollViewer), and its ScrollBar: last
    /// scroll (indicator fades), gutter hover, thumb drag.
    pub sidebar_scroll: f32,
    pub sidebar_scrolled_at: Option<std::time::Instant>,
    pub sidebar_scrollbar_expanded: bool,
    pub sidebar_scroll_drag: Option<f32>,
    /// The sidebar's HORIZONTAL scroll (`HorizontalScrollMode=Enabled`):
    /// current offset and horizontal bar thumb drag.
    pub sidebar_hscroll: f32,
    pub sidebar_hscroll_drag: Option<f32>,
    /// Minimal mode (`IsPaneOpen`): whether the floating pane is expanded.
    /// And its `TranslateX` animation: (start, target, begin) over 350 ms,
    /// plus the current position kept between frames.
    pub sidebar_pane_open: bool,
    pub sidebar_pane_tx: f32,
    pub sidebar_pane_anim: Option<(f32, f32, std::time::Instant)>,
    /// The open `ContentDialog`, if any (modal: it intercepts everything).
    pub dialog: Option<crate::dialogs::DialogState>,
    /// Does the window have focus? False when another window is active:
    /// the title bar then switches to the INACTIVE tint (like the original —
    /// the Mica backdrop desaturates and the accent color leaves the title bar).
    pub window_active: bool,
    /// The file conflict dialog (`FilesystemOperationDialog`), modal over
    /// everything just like `dialog`.
    pub conflict: Option<crate::dialogs::ConflictDialog>,
    /// Scroll position of the conflict dialog's list.
    pub conflict_scroll: f32,
    /// The branches listed for the branch selector flyout (computed when it
    /// opens; the flyout's clicks index this list).
    pub git_branches: Vec<crate::data::items::BranchItem>,
    /// Snapshot of the Shelf pane (`ShelfViewModel.Items`): (name, path,
    /// is folder?), rebuilt every frame from `MainWindow.shelf`.
    pub shelf: Vec<(String, String, bool)>,
    /// Items being dragged: enough to paint the ghost at the pointer and
    /// highlight the target (`DragUIOverride` + `Item_DragOver` highlight).
    pub drag: Option<DragVisual>,
}

/// The visual state of an item drag (painted over everything else).
#[derive(Clone)]
pub struct DragVisual {
    /// Current pointer position in DIP (ghost anchor).
    pub cursor: (f32, f32),
    /// Number of dragged items (ghost badge).
    pub count: usize,
    /// Caption: « Déplacer vers X » / « Copier vers X » over a valid target,
    /// otherwise just the dragged count.
    pub caption: String,
    /// The target rect to highlight, if there's one under the pointer.
    pub target_rect: Option<Rect>,
}

/// A tab sliding to a new slot after a reorder. WinUI animates this through
/// the ListView's reorder transition; `TabBarStyles.xaml` pins the duration
/// with `<VisualTransition GeneratedDuration="0:0:0.2" To="NoReorderHint" />`.
/// `from` is where the tab was, relative to the slot it now owns; it decays to
/// zero, so the tab appears to glide into its new place.
#[derive(Clone, Copy)]
pub struct TabSlide {
    pub from: f32,
    pub t0: std::time::Instant,
}

/// ListViewItemDragThemeOpacity — the dragged tab is drawn translucent.
pub const TAB_DRAG_OPACITY: f32 = 0.80;

/// The `ScrollBar`'s opacity.
///
/// The indicator only appears while scrolling then fades out; the expanded
/// bar (pointer in the gutter) or a thumb being dragged stay fully
/// visible.
pub fn scrollbar_alpha(state: &UiState) -> f32 {
    use crate::user_controls::scrollbar::{FADE_AFTER_MS, FADE_MS};
    if state.scrollbar_expanded || state.scroll_drag.is_some() {
        return 1.0;
    }
    let Some(since) = state.scrolled_at else { return 0.0 };
    let ms = since.elapsed().as_secs_f32() * 1000.0;
    if ms <= FADE_AFTER_MS {
        1.0
    } else {
        (1.0 - (ms - FADE_AFTER_MS) / FADE_MS).clamp(0.0, 1.0)
    }
}

/// Same color at a fraction of its alpha (used to fade the dragged tab).
pub(crate) fn fade(color: &D2D1_COLOR_F, alpha: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F { a: color.a * alpha, ..*color }
}
/// GeneratedDuration of the reorder VisualTransition.
const TAB_SLIDE_SECS: f32 = 0.2;

impl TabSlide {
    pub fn new(from: f32) -> Self {
        Self { from, t0: std::time::Instant::now() }
    }

    /// Remaining offset, on a decelerating curve (Fluent's ease-out).
    pub fn offset(&self) -> f32 {
        let t = (self.t0.elapsed().as_secs_f32() / TAB_SLIDE_SECS).clamp(0.0, 1.0);
        let eased = 1.0 - (1.0 - t).powi(3);
        self.from * (1.0 - eased)
    }

    pub fn done(&self) -> bool {
        self.t0.elapsed().as_secs_f32() >= TAB_SLIDE_SECS
    }
}

impl UiState {
    /// Current `TranslateX` position of the Minimal pane (interpolates
    /// toward the target over 350 ms via the `sidebar_ease` spline).
    pub fn sidebar_pane_tx_value(&self) -> f32 {
        match self.sidebar_pane_anim {
            Some((from, to, started)) => {
                let ms = started.elapsed().as_secs_f32() * 1000.0;
                let p = sidebar_ease(ms / SIDEBAR_ANIM_MS);
                from + (to - from) * p
            }
            None => self.sidebar_pane_tx,
        }
    }

    /// Is the pane animation running (drives the 16 ms timer).
    pub fn sidebar_pane_animating(&self) -> bool {
        self.sidebar_pane_anim
            .is_some_and(|(_, _, s)| s.elapsed().as_secs_f32() * 1000.0 < SIDEBAR_ANIM_MS)
    }

    /// Starts sliding the pane toward `target` (0 = expanded, -300 = stowed),
    /// from its current position. `sidebar_pane_tx` holds the resting value.
    pub fn sidebar_pane_animate_to(&mut self, target: f32) {
        let from = self.sidebar_pane_tx_value();
        self.sidebar_pane_tx = target;
        if (from - target).abs() < 0.5 {
            self.sidebar_pane_anim = None;
        } else {
            self.sidebar_pane_anim = Some((from, target, std::time::Instant::now()));
        }
    }

    pub fn new() -> Self {
        Self {
            sidebar_visible: true,
            pane_dragging: None,
            settings_section: 1, // Appearance, the most useful today
            appearance_expanded: [true, false, false, false],
            settings_expanded: crate::views::settings::default_expanded(),
            flyout: None,
            edit: None,
            path_suggestions: Vec::new(),
            tabs: vec![TabGroup::new_home()],
            active_tab: 0,
            hot: None,
            maximized: false,
            ops_active: false,
            ops_count: 0,
            status_center_open: false,
            tab_slides: Vec::new(),
            tab_drag_offset: None,
            tab_preview_insert: None,
            info_edit_tags_rect: std::cell::Cell::new(None),
            info_pane_scroll: 0.0,
            info_pane_extent: std::cell::Cell::new(0.0),
            info_properties_rect: std::cell::Cell::new(None),
            scrolled_at: None,
            scrollbar_expanded: false,
            scroll_drag: None,
            sidebar_expanded: Default::default(),
            sidebar_children: Default::default(),
            sidebar_scroll: 0.0,
            sidebar_scrolled_at: None,
            sidebar_scrollbar_expanded: false,
            sidebar_scroll_drag: None,
            sidebar_hscroll: 0.0,
            sidebar_hscroll_drag: None,
            sidebar_pane_open: false,
            sidebar_pane_tx: -SIDEBAR_OPEN_PANE_LENGTH,
            sidebar_pane_anim: None,
            dialog: None,
            window_active: true,
            conflict: None,
            conflict_scroll: 0.0,
            git_branches: Vec::new(),
            shelf: Vec::new(),
            drag: None,
        }
    }

    /// Horizontal offset to paint tab `i` at: the cursor for the tab being
    /// dragged, the remaining slide for tabs gliding to a new slot.
    pub fn tab_offset(&self, i: usize) -> f32 {
        if let Some((dragged, offset)) = self.tab_drag_offset {
            if dragged == i {
                return offset;
            }
        }
        self.tab_slides.get(i).map_or(0.0, TabSlide::offset)
    }

    /// True while any tab is still gliding — drives the animation timer.
    pub fn tabs_sliding(&self) -> bool {
        self.tab_slides.iter().any(|s| !s.done())
    }

    pub fn group(&self) -> &TabGroup {
        &self.tabs[self.active_tab]
    }

    pub fn group_mut(&mut self) -> &mut TabGroup {
        &mut self.tabs[self.active_tab]
    }

    /// The focused pane of the active tab.
    pub fn active(&self) -> &Tab {
        self.group().active()
    }

    pub fn active_mut(&mut self) -> &mut Tab {
        self.group_mut().active_mut()
    }
}

/// Rough text width estimate for layout (proper DirectWrite measurement
/// comes with the controls crate).
pub(crate) fn approx_text_width(text: &str, font_size: f32) -> f32 {
    text.chars().count() as f32 * font_size * 0.52
}
