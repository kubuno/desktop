//! The view state of an open document: the document being edited (the core's [`Editor`]), its
//! title, its zoom, how far it has scrolled, and where each page sits in the view.
//!
//! # Where the pages go
//!
//! As on the web (`DocumentEditorPage.tsx`, the page container): the pages are a wrapping row —
//! `flex-wrap: wrap`, `justify-content: safe center`, a column gap of 24 px and a row gap of
//! `PAGE_GAP` (10 px), `padding-top: CANVAS_PAD_Y` (32 px) — in **screen** units (the gaps do not
//! zoom, the pages do). So as many pages as fit the view's width share a row: one at 100 % on a
//! common screen, two or more at 50 %, a whole grid at 25 %. A row is as tall as its tallest page
//! and its pages are top-aligned; a page wider than the view starts at its left edge (`safe`).
//!
//! `PageCanvas` (`crate::controls::page_canvas`) owns one of these and paints from it.

use std::path::Path;

use kubuno_docs_core::editor::{Editor, PageSetup};

use crate::doc::DocPx;

/// Zoom bounds, matching Word's status-bar slider.
pub const ZOOM_MIN: f32 = 0.10;
pub const ZOOM_MAX: f32 = 5.00;
/// `CANVAS_PAD_Y` (screen px above the first row and below the last).
pub const CANVAS_PAD_Y: f32 = 32.0;
/// `PAGE_GAP` (screen px between rows).
pub const ROW_GAP: f32 = 10.0;
/// `columnGap: 24` (screen px between pages of a row).
pub const COLUMN_GAP: f32 = 24.0;

/// Where one page is drawn, in **view units** (DIP): `y` from the top of the scrolled content.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PagePlace {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// The pages' positions for a view `width` DIP wide at `zoom`, and the content's total height.
pub fn place_pages(sizes: &[(DocPx, DocPx)], zoom: f32, width: f32) -> (Vec<PagePlace>, f32) {
    let mut out = Vec::with_capacity(sizes.len());
    let mut y = CANVAS_PAD_Y;
    let mut i = 0;
    while i < sizes.len() {
        // Greedy rows: at least one page; another joins while the row still fits.
        let mut row_w = sizes[i].0 * zoom;
        let mut j = i + 1;
        while j < sizes.len() && row_w + COLUMN_GAP + sizes[j].0 * zoom <= width {
            row_w += COLUMN_GAP + sizes[j].0 * zoom;
            j += 1;
        }
        // `safe center`: centred when the row fits, at the start when it overflows.
        let mut x = if row_w <= width { ((width - row_w) / 2.0).floor() } else { 0.0 };
        let mut row_h = 0.0f32;
        for &(w, h) in &sizes[i..j] {
            let (pw, ph) = ((w * zoom).round(), (h * zoom).round());
            out.push(PagePlace { x, y, w: pw, h: ph });
            x += pw + COLUMN_GAP;
            row_h = row_h.max(ph);
        }
        y += row_h + ROW_GAP;
        i = j;
    }
    let total = if out.is_empty() { 0.0 } else { y - ROW_GAP + CANVAS_PAD_Y };
    (out, total)
}

pub struct App {
    pub editor: Editor,
    pub title: String,
    /// The pointer is in the scroll gutter, so the bar unfolds into a full track.
    pub scroll_hot: bool,
    /// While dragging the thumb: how far down the thumb the pointer grabbed it.
    pub scroll_grab: Option<f32>,
    pub zoom: f32,
    /// How far the page area has scrolled down, in DIP.
    pub scroll: f32,
    /// How far it has scrolled sideways, in DIP (a page wider than the view).
    pub scroll_x: f32,
    /// Where the pages were placed at the last layout, and the content's height.
    pub places: Vec<PagePlace>,
    pub content_h: f32,
    pub content_w: f32,
    pub page_count: usize,
    pub current_page: usize,
}

impl Default for App {
    fn default() -> Self {
        Self::new(Editor::open(SAMPLE.as_bytes()).unwrap_or_else(|_| Editor::open(EMPTY.as_bytes()).expect("the empty document parses")), "Document sans titre".into())
    }
}

/// An empty document: one paragraph.
pub const EMPTY: &str = r#"{"content":[{"type":"paragraph"}],"type":"doc"}"#;

/// The document shown when none is opened.
const SAMPLE: &str = r#"{"content":[{"attrs":{"level":1},"content":[{"text":"Kubuno Documents","type":"text"}],"type":"heading"},{"content":[{"text":"Ce document est éditable : cliquez pour placer le curseur, tapez, sélectionnez, mettez en forme depuis le ruban. La mise en page est celle de l’éditeur web, portée en Rust.","type":"text"}],"type":"paragraph"}],"type":"doc"}"#;

impl App {
    pub fn new(editor: Editor, title: String) -> Self {
        Self {
            editor,
            title,
            scroll_hot: false,
            scroll_grab: None,
            zoom: 1.0,
            scroll: 0.0,
            scroll_x: 0.0,
            places: Vec::new(),
            content_h: 0.0,
            content_w: 0.0,
            page_count: 1,
            current_page: 0,
        }
    }

    /// Opens a `content_json` file from disk (a `.kbdoc` or `.json` saved by the server, or one of
    /// the test fixtures).
    pub fn open_file(path: &Path) -> Result<Self, String> {
        let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let editor = Editor::open(&bytes).map_err(|e| format!("{}: {e}", path.display()))?;
        let title = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "Document".into());
        Ok(Self::new(editor, title))
    }

    /// The size of each page (its section's paper), in document px.
    pub fn page_sizes(&self) -> Vec<(DocPx, DocPx)> {
        self.editor.pages().iter().map(|p| {
            let s = self.editor.page_setup(p.sec_idx);
            (s.width, s.height)
        }).collect()
    }

    /// The setup of page `index`'s section.
    pub fn setup_of_page(&self, index: usize) -> PageSetup {
        self.editor.pages().get(index).map(|p| self.editor.page_setup(p.sec_idx)).unwrap_or_else(|| self.editor.page_setup(0))
    }

    /// Places the pages for a view `width` DIP wide (after the editor's layout).
    pub fn place(&mut self, width: f32) {
        let sizes = self.page_sizes();
        let (places, h) = place_pages(&sizes, self.zoom, width);
        self.content_w = places.iter().map(|p| p.x + p.w).fold(0.0, f32::max);
        self.places = places;
        self.content_h = h;
        self.page_count = sizes.len().max(1);
    }

    /// The furthest the view may scroll down.
    pub fn max_scroll(&self, viewport_height: f32) -> f32 {
        (self.content_h - viewport_height).max(0.0)
    }

    pub fn scroll_by(&mut self, delta: f32, viewport_height: f32) {
        self.scroll = (self.scroll + delta).clamp(0.0, self.max_scroll(viewport_height));
    }

    /// Changes the zoom, keeping the point at `anchor_y` DIP from the top of the view where it is.
    pub fn set_zoom(&mut self, zoom: f32, viewport_height: f32) {
        let zoom = zoom.clamp(ZOOM_MIN, ZOOM_MAX);
        let ratio = zoom / self.zoom.max(0.01);
        self.zoom = zoom;
        self.scroll = (self.scroll * ratio).clamp(0.0, (self.content_h * ratio - viewport_height).max(0.0));
    }

    /// The page covering most of the view band `[scroll, scroll + h)`.
    pub fn page_at_scroll(&self, viewport_height: f32) -> usize {
        let (top, bottom) = (self.scroll, self.scroll + viewport_height);
        let mut best = (0usize, -1.0f32);
        for (i, p) in self.places.iter().enumerate() {
            let seen = (bottom.min(p.y + p.h) - top.max(p.y)).max(0.0);
            if seen > best.1 {
                best = (i, seen);
            }
        }
        best.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A4: (f32, f32) = (794.0, 1123.0);

    #[test]
    fn at_100_percent_pages_stack_one_per_row_centred() {
        let (p, h) = place_pages(&[A4; 3], 1.0, 1200.0);
        assert_eq!(p[0].x, ((1200.0 - 794.0) / 2.0f32).floor());
        assert_eq!(p[0].y, CANVAS_PAD_Y);
        assert_eq!(p[1].y, CANVAS_PAD_Y + 1123.0 + ROW_GAP);
        assert_eq!(p[0].x, p[1].x);
        assert_eq!(h, CANVAS_PAD_Y * 2.0 + 3.0 * 1123.0 + 2.0 * ROW_GAP);
    }

    #[test]
    fn at_50_percent_two_pages_share_a_row_when_they_fit() {
        let (p, _) = place_pages(&[A4; 5], 0.5, 1200.0);
        // 397 + 24 + 397 = 818 ≤ 1200, a third would need 1239.
        assert_eq!(p[0].y, p[1].y);
        assert!(p[2].y > p[1].y);
        assert_eq!(p[1].x - p[0].x, 397.0 + COLUMN_GAP);
        // The row is centred.
        assert_eq!(p[0].x, ((1200.0 - 818.0) / 2.0f32).floor());
    }

    #[test]
    fn at_25_percent_a_wide_view_shows_a_grid() {
        let (p, _) = place_pages(&[A4; 10], 0.25, 1200.0);
        let per_row = p.iter().filter(|q| q.y == p[0].y).count();
        assert_eq!(per_row, 5, "199 × 5 + 24 × 4 = 1091 ≤ 1200");
    }

    #[test]
    fn a_page_wider_than_the_view_starts_at_its_left_edge() {
        let (p, _) = place_pages(&[A4], 2.0, 1000.0);
        assert_eq!(p[0].x, 0.0);
    }
}
