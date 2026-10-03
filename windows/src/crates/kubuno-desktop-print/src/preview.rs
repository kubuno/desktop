//! `<PrintPreviewControl>` (WinForms `PrintPreviewControl`): the pages of a document as they will be
//! printed, laid out `Columns` × `Rows` from `StartPage`, at `Zoom` (1.0 = actual size) or fitted
//! (`AutoZoom`).
//!
//! ```xml
//! <PrintPreviewControl x:Name="preview" Document="print_document1" Columns="2" Dock="Fill"/>
//! ```
//!
//! The pages come from the document's own print loop (`BeginPrint`, `QueryPageSettings`, `PrintPage`,
//! `EndPrint`, with `print_action` = `PrintToPreview`), rendered once and kept until the document
//! changes ([`crate::PrintDocument::invalidate`], a property, [`PrintPreviewControl::invalidate_preview`]).
//! A document of the view is rendered by the runtime between two frames (its `.kbview` handlers
//! run): name the control (`x:Name`) so the runtime reaches it. The page bitmaps are drawn at the
//! window's resolution.

use std::cell::RefCell;
use std::rc::Rc;

use kubuno_desktop_ui::graphics::{Color, Font, FontStyle, Image, RectExt, StringFormat};
use kubuno_desktop_views::binding::{BindingFormat, Value};
use kubuno_desktop_views::format::ValueKind;
use kubuno_desktop_views::prelude::*;
use kubuno_desktop_views::scope::{BindingProvider, ComponentScope};
use windows::Win32::Graphics::Direct2D::ID2D1Bitmap1;

use crate::document::PrintDocument;
use crate::engine::{self, Surface};
use crate::{text, PreviewDocument, PrintError};

/// The space around and between the pages, DIP (WinForms' 10-pixel border).
const GAP: f32 = 10.0;

/// Where the pages go in a view of the control (see [`PrintPreviewControl::layout`]).
#[derive(Debug, Clone, PartialEq)]
pub struct PreviewLayout {
    /// The zoom used (1.0 = actual size).
    pub zoom: f32,
    /// The pages shown, `(index, rectangle)`, in the view's coordinates (scroll applied).
    pub pages: Vec<(usize, Rect)>,
    /// The size of everything laid out, DIP (larger than the view: it scrolls).
    pub content: (f32, f32),
}

/// What the control shows when it has no pages.
#[derive(Debug, Clone, PartialEq)]
enum State {
    Empty,
    Ready,
    Failed(String),
}

/// A page bitmap uploaded to the window's device.
struct Uploaded {
    device: usize,
    index: usize,
    size: (u32, u32),
    revision: u64,
    bitmap: ID2D1Bitmap1,
}

/// `<PrintPreviewControl>` (see the module doc).
#[derive(Component)]
#[kubuno(extends = Control, overrides(Control, Component))]
#[toolbox(icon = "file-search", category = "Printing")]
#[default_property("Document")]
#[default_event("StartPageChanged")]
pub struct PrintPreviewControl {
    base: ControlCore,
    /// The PrintDocument whose pages are shown.
    #[property]
    #[category("Behavior")]
    #[editor("reference:PrintDocument")]
    pub document: String,
    /// Whether the zoom is chosen so that the pages fit the control.
    #[property]
    #[category("Behavior")]
    #[default_value(true)]
    pub auto_zoom: bool,
    /// The size of the pages when AutoZoom is off: 1.0 is the actual size, 0.5 half of it.
    #[property]
    #[category("Behavior")]
    #[default_value(0.3)]
    pub zoom: f32,
    /// How many pages are shown side by side.
    #[property]
    #[category("Layout")]
    #[default_value(1)]
    pub columns: u32,
    /// How many rows of pages are shown.
    #[property]
    #[category("Layout")]
    #[default_value(1)]
    pub rows: u32,
    /// The page shown first (top left), from 0.
    #[property]
    #[category("Behavior")]
    #[default_value(0)]
    pub start_page: u32,
    /// Occurs when the first page shown changes.
    #[event]
    #[category("Property Changed")]
    pub start_page_changed: Event<EmptyEventArgs>,
    preview: Option<PreviewDocument>,
    source: Option<Rc<RefCell<PrintDocument>>>,
    state: State,
    stale: bool,
    scroll: (f32, f32),
    /// The last view painted (the control's bounds) and the content's size in it.
    last_view: Rect,
    last_content: (f32, f32),
    uploaded: RefCell<Vec<Uploaded>>,
    surface: Option<Rc<Surface>>,
}

impl Default for PrintPreviewControl {
    fn default() -> Self {
        // Painted every frame from its bitmaps (the paint buffer would keep a stale page).
        let mut base = ControlCore::default();
        base.styles.set(ControlStyles::OPTIMIZED_DOUBLE_BUFFER, false);
        Self {
            base,
            document: String::new(),
            auto_zoom: true,
            zoom: 0.3,
            columns: 1,
            rows: 1,
            start_page: 0,
            start_page_changed: Event::default(),
            preview: None,
            source: None,
            state: State::Empty,
            stale: true,
            scroll: (0.0, 0.0),
            last_view: Rect::new(0.0, 0.0, 0.0, 0.0),
            last_content: (0.0, 0.0),
            uploaded: RefCell::new(Vec::new()),
            surface: None,
        }
    }
}

impl std::fmt::Debug for PrintPreviewControl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PrintPreviewControl").field("document", &self.document).field("pages", &self.page_count()).field("state", &self.state).finish()
    }
}

impl PrintPreviewControl {
    pub fn new() -> Self {
        Self::default()
    }

    /// Shows the pages of a document of code (`Document = doc`).
    pub fn set_document(&mut self, document: Rc<RefCell<PrintDocument>>) {
        self.source = Some(document);
        self.invalidate_preview();
    }

    /// Shows pages rendered elsewhere (what the preview dialog does).
    pub fn set_preview(&mut self, preview: PreviewDocument) {
        self.surface = Some(preview.surface.clone());
        self.state = State::Ready;
        self.preview = Some(preview);
        self.stale = false;
        self.uploaded.borrow_mut().clear();
        let count = self.page_count() as u32;
        if self.start_page >= count && count > 0 {
            self.start_page = count - 1;
        }
        self.invalidate();
    }

    /// The pages shown.
    pub fn preview(&self) -> Option<&PreviewDocument> {
        self.preview.as_ref()
    }

    /// How many pages the document has.
    pub fn page_count(&self) -> usize {
        self.preview.as_ref().map_or(0, |p| p.pages.len())
    }

    /// Renders the pages again at the next opportunity (`InvalidatePreview()`).
    pub fn invalidate_preview(&mut self) {
        self.stale = true;
        self.invalidate();
        kubuno_desktop_controls::host::request_repaint_after(1);
    }

    /// Moves to `page` (0-based, clamped), raising `StartPageChanged` (`StartPage = …`).
    pub fn set_start_page(&mut self, page: u32) {
        let count = self.page_count() as u32;
        let page = if count == 0 { 0 } else { page.min(count - 1) };
        if page != self.start_page {
            self.start_page = page;
            self.scroll = (0.0, 0.0);
            self.invalidate();
            let event = self.start_page_changed.clone();
            kubuno_desktop_views::component::raise_declared_event(self, "OnStartPageChanged", &event, EmptyEventArgs);
        }
    }

    pub fn set_zoom(&mut self, zoom: f32) {
        self.zoom = zoom.clamp(0.01, 10.0);
        self.auto_zoom = false;
        self.invalidate();
    }

    pub fn set_auto_zoom(&mut self, on: bool) {
        self.auto_zoom = on;
        self.invalidate();
    }

    /// Shows `columns` × `rows` pages at once.
    pub fn set_layout(&mut self, columns: u32, rows: u32) {
        self.columns = columns.clamp(1, 10);
        self.rows = rows.clamp(1, 10);
        self.scroll = (0.0, 0.0);
        self.invalidate();
    }

    /// Where the pages go in `view` (the control's bounds): the zoom (fitted with `AutoZoom`), the
    /// page rectangles from `StartPage`, centred when they fit, scrolled when they do not.
    pub fn layout(&self, view: Rect) -> PreviewLayout {
        let Some(preview) = &self.preview else { return PreviewLayout { zoom: self.zoom, pages: Vec::new(), content: (0.0, 0.0) } };
        let (cols, rows) = (self.columns.max(1) as usize, self.rows.max(1) as usize);
        let first = (self.start_page as usize).min(preview.pages.len().saturating_sub(1));
        let shown: Vec<usize> = (first..preview.pages.len()).take(cols * rows).collect();
        // Every cell is as large as the largest page shown (a landscape page among portrait ones).
        let (cell_w, cell_h) = shown.iter().fold((0.0f32, 0.0f32), |(w, h), i| (w.max(preview.pages[*i].size.0), h.max(preview.pages[*i].size.1)));
        let (cell_w, cell_h) = (cell_w.max(1.0), cell_h.max(1.0));
        let (vw, vh) = (view.right - view.left, view.bottom - view.top);
        let zoom = if self.auto_zoom {
            let zx = (vw - GAP * (cols as f32 + 1.0)) / (cell_w * cols as f32);
            let zy = (vh - GAP * (rows as f32 + 1.0)) / (cell_h * rows as f32);
            zx.min(zy).max(0.02)
        } else {
            self.zoom.max(0.01)
        };
        let (cw, ch) = (cell_w * zoom, cell_h * zoom);
        let content = (GAP + cols as f32 * (cw + GAP), GAP + rows as f32 * (ch + GAP));
        let ox = if content.0 <= vw { view.left + (vw - content.0) / 2.0 } else { view.left - self.scroll.0.clamp(0.0, content.0 - vw) };
        let oy = if content.1 <= vh { view.top + (vh - content.1) / 2.0 } else { view.top - self.scroll.1.clamp(0.0, content.1 - vh) };
        let pages = shown
            .iter()
            .enumerate()
            .map(|(n, i)| {
                let (col, row) = (n % cols, n / cols);
                let (pw, ph) = (preview.pages[*i].size.0 * zoom, preview.pages[*i].size.1 * zoom);
                // Centred in its cell.
                let x = ox + GAP + col as f32 * (cw + GAP) + (cw - pw) / 2.0;
                let y = oy + GAP + row as f32 * (ch + GAP) + (ch - ph) / 2.0;
                (*i, Rect::from_xywh(x, y, pw, ph))
            })
            .collect();
        PreviewLayout { zoom, pages, content }
    }

    /// Renders the pages when they are stale and the document can be reached: a document of code
    /// always, a document of the view when its handlers can run (`scope`).
    fn refresh(&mut self, scope: Option<&ComponentScope>) -> bool {
        if !self.stale && !self.document_changed(scope) {
            return false;
        }
        if self.site().is_some_and(|s| s.design_mode) {
            return false;
        }
        let surface = match &self.surface {
            Some(s) => s.clone(),
            None => match Surface::new() {
                Ok(s) => {
                    let s = Rc::new(s);
                    self.surface = Some(s.clone());
                    s
                }
                Err(e) => {
                    self.fail(e);
                    return true;
                }
            },
        };
        let result: Option<Result<PreviewDocument, PrintError>> = if let Some(source) = &self.source {
            match source.try_borrow_mut() {
                Ok(mut doc) => Some(doc.render_preview_on(surface)),
                Err(_) => Some(Err(PrintError::Busy("PrintDocument"))),
            }
        } else {
            let name = self.document.trim().to_string();
            match scope {
                _ if name.is_empty() => None,
                Some(scope) if kubuno_desktop_views::scope::can_raise_now() => scope.get(&name).map(|cell| match cell.try_borrow_mut() {
                    Ok(mut c) => match c.find_base_mut::<PrintDocument>() {
                        Some(doc) => doc.render_preview_on(surface),
                        None => Err(PrintError::Driver(format!("`{name}` is not a PrintDocument"))),
                    },
                    Err(_) => Err(PrintError::Busy("PrintDocument")),
                }),
                _ => return false,
            }
        };
        match result {
            Some(Ok(preview)) => self.set_preview(preview),
            Some(Err(e)) => self.fail(e),
            None => {
                self.preview = None;
                self.state = State::Empty;
                self.stale = false;
            }
        }
        self.invalidate();
        true
    }

    /// Whether the document's revision moved past the pages shown.
    fn document_changed(&self, scope: Option<&ComponentScope>) -> bool {
        let Some(shown) = self.preview.as_ref().map(|p| p.revision) else { return false };
        let now = match &self.source {
            Some(doc) => doc.try_borrow().ok().map(|d| d.revision()),
            None => scope.and_then(|s| s.with_ref::<PrintDocument, _>(self.document.trim(), |d| d.revision())),
        };
        now.is_some_and(|r| r != shown)
    }

    fn fail(&mut self, e: PrintError) {
        tracing::error!(target: "kubuno_desktop_print", "print preview: {e}");
        self.state = State::Failed(e.to_string());
        self.preview = None;
        self.stale = false;
    }

    /// The bitmap of page `index` at `size` pixels for `renderer`'s device (cached).
    fn page_bitmap(&self, renderer: &kubuno_drive_desktop_app_controls::Renderer, index: usize, size: (u32, u32)) -> Option<ID2D1Bitmap1> {
        let preview = self.preview.as_ref()?;
        let device = windows::core::Interface::as_raw(&renderer.d2d_context) as usize;
        if let Some(u) = self.uploaded.borrow().iter().find(|u| u.device == device && u.index == index && u.size == size && u.revision == preview.revision) {
            return Some(u.bitmap.clone());
        }
        let page = preview.rasterize(index, size.0, size.1).map_err(|e| tracing::warn!(target: "kubuno_desktop_print", "{e}")).ok()?;
        let bitmap = engine::upload(renderer, &page)?;
        let mut cache = self.uploaded.borrow_mut();
        cache.retain(|u| !(u.index == index && u.device == device));
        // A handful of pages at a handful of sizes: keep it small.
        if cache.len() > 24 {
            cache.remove(0);
        }
        cache.push(Uploaded { device, index, size, revision: preview.revision, bitmap: bitmap.clone() });
        Some(bitmap)
    }

    fn message(g: &Graphics<'_>, bounds: Rect, text: &str) {
        let theme = g.theme_colors();
        let format = StringFormat::centered();
        g.draw_string(text, &Font::new("Segoe UI", 10.0, FontStyle::REGULAR), Color::from(theme.text_secondary), bounds, &format);
    }

    /// Scrolls by `(dx, dy)` DIP within the content; returns whether it moved.
    fn scroll_by(&mut self, dx: f32, dy: f32) -> bool {
        let (vw, vh) = (self.last_view.right - self.last_view.left, self.last_view.bottom - self.last_view.top);
        let max = ((self.last_content.0 - vw).max(0.0), (self.last_content.1 - vh).max(0.0));
        let next = ((self.scroll.0 + dx).clamp(0.0, max.0), (self.scroll.1 + dy).clamp(0.0, max.1));
        let moved = next != self.scroll;
        self.scroll = next;
        moved
    }

    fn page_step(&self) -> u32 {
        (self.columns.max(1) * self.rows.max(1)).max(1)
    }
}

impl Control for PrintPreviewControl {
    fn get_preferred_size(&self, _canvas: &dyn Canvas, _proposed: Size) -> Size {
        Size { width: 320.0, height: 240.0 }
    }

    fn on_paint(&mut self, e: &mut PaintEventCx<'_>) {
        // A document of code is rendered here; one of the view by the runtime (`binding_sync`).
        if self.source.is_some() {
            self.refresh(None);
        }
        let g = e.graphics;
        let bounds = e.clip_rectangle;
        let theme = g.theme_colors();
        g.fill_rectangle(Color::from(theme.surface_2), bounds);
        g.with_saved(|g| {
            g.set_clip(bounds);
            if self.site().is_some_and(|s| s.design_mode) || (self.preview.is_none() && self.document.trim().is_empty() && self.source.is_none()) {
                // The designer: an empty sheet of Letter paper, fitted.
                let (pw, ph) = (816.0f32, 1056.0f32);
                let zoom = ((bounds.right - bounds.left - 2.0 * GAP) / pw).min((bounds.bottom - bounds.top - 2.0 * GAP) / ph).max(0.02);
                let r = Rect::from_xywh(bounds.left + (bounds.right - bounds.left - pw * zoom) / 2.0, bounds.top + (bounds.bottom - bounds.top - ph * zoom) / 2.0, pw * zoom, ph * zoom);
                g.draw_card_shadow(&r, 2.0);
                g.fill_rectangle(Color::WHITE, r);
                return;
            }
            match &self.state {
                State::Failed(message) => return Self::message(g, bounds, &text::preview_failed(message)),
                State::Empty if self.stale => return Self::message(g, bounds, &text::generating()),
                State::Empty => return Self::message(g, bounds, &text::no_pages()),
                State::Ready if self.page_count() == 0 => return Self::message(g, bounds, &text::no_pages()),
                State::Ready => {}
            }
            let layout = self.layout(bounds);
            self.last_view = bounds;
            self.last_content = layout.content;
            let renderer = g.renderer();
            let scale = g.dpi_scale().max(1.0);
            for (index, rect) in &layout.pages {
                g.draw_card_shadow(rect, 2.0);
                g.fill_rectangle(Color::WHITE, *rect);
                let size = (((rect.right - rect.left) * scale).round().max(1.0) as u32, ((rect.bottom - rect.top) * scale).round().max(1.0) as u32);
                if let Some(bitmap) = renderer.and_then(|r| self.page_bitmap(r, *index, size)) {
                    g.draw_image(&Image::from_bitmap(bitmap), *rect);
                }
            }
        });
        e.raise(self, "OnPaint");
    }

    fn on_mouse_wheel(&mut self, e: &mut EventCx<'_, MouseEventArgs>) {
        let delta = e.args().delta;
        let vh = self.last_view.bottom - self.last_view.top;
        if self.last_content.1 > vh + 0.5 {
            if self.scroll_by(0.0, delta * 60.0) {
                self.invalidate();
            }
        } else if delta > 0.0 {
            let next = self.start_page.saturating_add(self.page_step());
            self.set_start_page(next);
        } else if delta < 0.0 {
            let next = self.start_page.saturating_sub(self.page_step());
            self.set_start_page(next);
        }
        e.raise(self, "OnMouseWheel");
    }

    fn on_key_down(&mut self, e: &mut EventCx<'_, KeyEventArgs>) {
        const PRIOR: u16 = 0x21;
        const NEXT: u16 = 0x22;
        const END: u16 = 0x23;
        const HOME: u16 = 0x24;
        const UP: u16 = 0x26;
        const DOWN: u16 = 0x28;
        let step = self.page_step();
        let handled = match e.args().key.vk() {
            NEXT => {
                self.set_start_page(self.start_page.saturating_add(step));
                true
            }
            PRIOR => {
                self.set_start_page(self.start_page.saturating_sub(step));
                true
            }
            HOME => {
                self.set_start_page(0);
                true
            }
            END => {
                self.set_start_page(self.page_count().saturating_sub(1) as u32);
                true
            }
            UP => self.scroll_by(0.0, -40.0),
            DOWN => self.scroll_by(0.0, 40.0),
            _ => false,
        };
        if handled {
            e.args_mut().handled = true;
            self.invalidate();
        }
        e.raise(self, "OnKeyDown");
    }
}

impl Component for PrintPreviewControl {
    fn as_binding_provider(&self) -> Option<&dyn BindingProvider> {
        Some(self)
    }
    fn as_binding_provider_mut(&mut self) -> Option<&mut dyn BindingProvider> {
        Some(self)
    }
}

/// `preview.PageCount`, `preview.StartPage` (two-way, 1-based: a page number box binds it) and
/// `preview.PageText`; once per frame the pages are rendered again when stale (see the module doc).
impl BindingProvider for PrintPreviewControl {
    fn binding_get(&self, path: &str, want: ValueKind, format: &BindingFormat, _scope: &ComponentScope) -> Option<Value> {
        let value = match path {
            "PageCount" => Value::F32(self.page_count() as f32),
            "StartPage" => Value::F32(self.start_page as f32 + 1.0),
            "PageText" => Value::Str(text::of_pages(self.page_count())),
            _ => return None,
        };
        kubuno_desktop_views::format::to_target(value, want, format)
    }

    fn binding_set(&mut self, path: &str, value: Value, _format: &BindingFormat, _scope: &ComponentScope) -> bool {
        if path != "StartPage" {
            return false;
        }
        let n = match value {
            Value::F32(v) => v,
            Value::Str(s) => s.trim().parse().unwrap_or(1.0),
            _ => return true,
        };
        self.set_start_page((n.round().max(1.0) as u32) - 1);
        true
    }

    fn binding_sync(&mut self, scope: &ComponentScope) -> bool {
        self.refresh(Some(scope))
    }
}
