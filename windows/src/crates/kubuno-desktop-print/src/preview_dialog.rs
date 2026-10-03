//! `<PrintPreviewDialog>` (WinForms `PrintPreviewDialog`): a window showing a document's pages in a
//! [`PrintPreviewControl`] under a Kubuno tool bar — Print, Zoom (Auto, 500 % … 10 %), one, two,
//! three, four or six pages, the page number, Close.
//!
//! ```xml
//! <PrintPreviewDialog x:Name="print_preview_dialog1" Document="print_document1"/>
//! ```
//!
//! [`PrintPreviewDialog::show_dialog`] renders the pages (the document's `PrintPage` handler, with
//! `print_action` = `PrintToPreview`) and opens the window, modal to the application's window. A
//! dialog of a view shown from one of the view's handlers opens when that handler returns (the
//! handlers of the document must run: see `crate::document`).

use std::cell::RefCell;
use std::rc::Rc;

use kubuno_desktop_controls::buttons::DialogResult;
use kubuno_desktop_controls::host::{self, HostOptions, StartPosition};
use kubuno_desktop_views::events::CloseReason;
use kubuno_desktop_ui::graphics::{Color, Pen, RectExt};
use kubuno_desktop_ui::Rect;
use kubuno_desktop_views::binding::{Value, ViewModel};
use kubuno_desktop_views::events::{ElementRef, EventArgs};
use kubuno_desktop_views::prelude::*;
use kubuno_desktop_views::runtime::Runtime;

use crate::dialogs::Target;
use crate::document::PrintDocument;
use crate::preview::PrintPreviewControl;
use crate::{text, PreviewDocument, PrintError};

/// The zoom choices of the tool bar: `(value, percent)`; `auto` fits the pages.
const ZOOMS: &[(&str, f32)] = &[("auto", 0.0), ("5", 5.0), ("2", 2.0), ("1.5", 1.5), ("1", 1.0), ("0.75", 0.75), ("0.5", 0.5), ("0.25", 0.25), ("0.1", 0.1)];

/// The page layouts of the tool bar: pages, columns, rows.
const LAYOUTS: &[(u32, u32, u32)] = &[(1, 1, 1), (2, 2, 1), (3, 3, 1), (4, 2, 2), (6, 3, 2)];

/// `<PrintPreviewDialog>` (see the module doc).
#[derive(Component)]
#[kubuno(extends = Component)]
#[toolbox(icon = "scan-eye", category = "Printing")]
#[default_property("Document")]
pub struct PrintPreviewDialog {
    base: ComponentCore,
    /// The PrintDocument to preview.
    #[property]
    #[category("Behavior")]
    #[editor("reference:PrintDocument")]
    pub document: String,
    /// The title of the window; empty for "Print preview".
    #[property]
    #[category("Appearance")]
    pub text: String,
    /// The width of the window, in pixels.
    #[property]
    #[category("Layout")]
    #[default_value(960)]
    pub width: f32,
    /// The height of the window, in pixels.
    #[property]
    #[category("Layout")]
    #[default_value(720)]
    pub height: f32,
    target: Target,
    result: DialogResult,
}

impl Default for PrintPreviewDialog {
    fn default() -> Self {
        Self { base: ComponentCore::default(), document: String::new(), text: String::new(), width: 960.0, height: 720.0, target: Target::Own, result: DialogResult::None }
    }
}

impl std::fmt::Debug for PrintPreviewDialog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PrintPreviewDialog").field("document", &self.document).field("target", &self.target).finish()
    }
}

impl PrintPreviewDialog {
    pub fn new() -> Self {
        Self::default()
    }

    /// Previews a document of code (`Document = doc`).
    pub fn with_document(mut self, document: Rc<RefCell<PrintDocument>>) -> Self {
        self.target = Target::Code(document);
        self
    }

    /// How the last dialog closed.
    pub fn dialog_result(&self) -> DialogResult {
        self.result
    }

    /// Renders the pages and shows the dialog (`ShowDialog()`); returns how it was closed (`Cancel`
    /// when closed from its caption or Close, `OK` after printing). A preview of a document of a
    /// view asked from one of the view's handlers opens when that handler returns: `None` is
    /// returned then.
    pub fn show_dialog(&mut self) -> DialogResult {
        let title = Some(self.text.trim().to_string()).filter(|t| !t.is_empty()).unwrap_or_else(text::preview_title);
        let size = (self.width, self.height);
        self.result = match &self.target {
            Target::Code(doc) => match doc.try_borrow_mut() {
                Ok(mut d) => d.show_preview(&title, size),
                Err(_) => {
                    tracing::error!(target: "kubuno_desktop_print", "PrintPreviewDialog: the PrintDocument is busy (reached from one of its own events)");
                    DialogResult::Abort
                }
            },
            Target::Own => {
                let name = self.document.trim().to_string();
                let shown = kubuno_desktop_views::scope::current().and_then(|scope| scope.with::<PrintDocument, _>(&name, |d| d.show_preview(&title, size)));
                shown.unwrap_or_else(|| {
                    tracing::error!(target: "kubuno_desktop_print", document = %name, "PrintPreviewDialog: no PrintDocument of that name can be reached in the view");
                    DialogResult::Abort
                })
            }
        };
        self.result
    }
}

// ── The window ───────────────────────────────────────────────────────────────────────────────────

/// The dialog's view model: the tool bar's state; the Print and Close requests are read by the
/// window's paint closure after each frame.
#[derive(Default)]
struct DialogVm {
    zoom: String,
    layout: usize,
    print_requested: bool,
    close_requested: bool,
    /// The tool bar changed something the preview binds: the next frame shows it.
    changed: bool,
}

impl DialogVm {
    fn zoom_value(&self) -> (bool, f32) {
        match ZOOMS.iter().find(|(v, _)| *v == self.zoom) {
            Some((_, z)) if *z > 0.0 => (false, *z),
            _ => (true, 1.0),
        }
    }
}

impl ViewModel for DialogVm {
    fn get(&self, path: &str) -> Option<Value> {
        let (auto, zoom) = self.zoom_value();
        let (_, cols, rows) = LAYOUTS[self.layout.min(LAYOUTS.len() - 1)];
        Some(match path {
            "Zoom" => Value::Str(self.zoom.clone()),
            "AutoZoom" => Value::Bool(auto),
            "ZoomValue" => Value::F32(zoom),
            "Columns" => Value::F32(cols as f32),
            "Rows" => Value::F32(rows as f32),
            _ => {
                let n = path.strip_prefix("Layout")?.parse::<usize>().ok()?;
                Value::Bool(LAYOUTS.get(self.layout).is_some_and(|l| l.0 as usize == n))
            }
        })
    }

    fn set(&mut self, path: &str, value: Value) {
        if path == "Zoom" {
            if let Value::Str(s) = value {
                self.changed |= self.zoom != s;
                self.zoom = s;
            }
        }
    }

    fn dispatch_event(&mut self, handler: &str, _sender: &ElementRef<'_>, _args: &mut dyn EventArgs) -> bool {
        self.changed = true;
        match handler {
            "print_click" => self.print_requested = true,
            "close_click" => self.close_requested = true,
            _ => match handler.strip_prefix("pages").and_then(|n| n.strip_suffix("_click")).and_then(|n| n.parse::<u32>().ok()) {
                Some(n) => {
                    if let Some(i) = LAYOUTS.iter().position(|l| l.0 == n) {
                        self.layout = i;
                    }
                }
                None => return false,
            },
        }
        true
    }
}

/// Escapes `text` for an XML attribute.
fn xml(text: &str) -> String {
    text.replace('&', "&amp;").replace('"', "&quot;").replace('<', "&lt;").replace('>', "&gt;")
}

/// The dialog's view.
fn dialog_view(title: &str, width: f32, height: f32) -> String {
    let title = xml(title);
    let bar = 48.0;
    let mut x = 12.0;
    let mut items = String::new();
    items.push_str(&format!(
        "  <IconButton x:Name=\"print\" Icon=\"Printer\" Diameter=\"36\" Glyph=\"18\" ToolTip=\"{0}\" AccessibleName=\"{0}\" OnClick=\"print_click\" X=\"{x}\" Y=\"6\" Width=\"36\" Height=\"36\"/>\n",
        xml(&text::print())
    ));
    x += 48.0;
    let mut options = String::new();
    for (value, zoom) in ZOOMS {
        let label = if *zoom > 0.0 { format!("{} %", (zoom * 100.0).round()) } else { text::zoom_auto() };
        options.push_str(&format!("    <Option Value=\"{value}\" Label=\"{}\"/>\n", xml(&label)));
    }
    items.push_str(&format!(
        "  <Dropdown x:Name=\"zoom\" SelectedValue=\"{{Binding Zoom, Mode=TwoWay}}\" ToolTip=\"{0}\" AccessibleName=\"{0}\" X=\"{x}\" Y=\"8\" Width=\"150\" Height=\"32\">\n{options}  </Dropdown>\n",
        xml(&text::zoom_tooltip())
    ));
    x += 162.0;
    items.push_str(&format!("  <Separator Orientation=\"Vertical\" X=\"{x}\" Y=\"12\" Width=\"1\" Height=\"24\"/>\n"));
    x += 12.0;
    for (pages, _, _) in LAYOUTS {
        items.push_str(&format!(
            "  <PreviewPagesButton x:Name=\"pages{pages}\" Pages=\"{pages}\" Checked=\"{{Binding Layout{pages}}}\" ToolTip=\"{0}\" AccessibleName=\"{0}\" AccessibleRole=\"PushButton\" OnClick=\"pages{pages}_click\" X=\"{x}\" Y=\"6\" Width=\"36\" Height=\"36\"/>\n",
            xml(&text::pages_tooltip(*pages))
        ));
        x += 40.0;
    }
    x += 4.0;
    items.push_str(&format!("  <Separator Orientation=\"Vertical\" X=\"{x}\" Y=\"12\" Width=\"1\" Height=\"24\"/>\n"));
    x += 14.0;
    items.push_str(&format!("  <Label x:Name=\"page_label\" Text=\"{}\" X=\"{x}\" Y=\"14\" Width=\"44\" Height=\"20\"/>\n", xml(&text::page())));
    x += 46.0;
    items.push_str(&format!(
        "  <NumericField x:Name=\"page\" AccessibleName=\"{}\" Minimum=\"1\" Maximum=\"{{Binding preview.PageCount}}\" Value=\"{{Binding preview.StartPage, Mode=TwoWay}}\" X=\"{x}\" Y=\"8\" Width=\"96\" Height=\"32\"/>\n",
        xml(&text::page())
    ));
    x += 104.0;
    items.push_str(&format!("  <Label x:Name=\"page_count\" Text=\"{{Binding preview.PageText}}\" X=\"{x}\" Y=\"14\" Width=\"80\" Height=\"20\"/>\n"));
    items.push_str(&format!(
        "  <Button x:Name=\"close\" Text=\"{}\" OnClick=\"close_click\" X=\"{}\" Y=\"8\" Width=\"96\" Height=\"32\" Anchor=\"Top, Right\"/>\n",
        xml(&text::close()),
        width - 108.0
    ));
    format!(
        "<Panel DesignWidth=\"{width}\" DesignHeight=\"{height}\">\n{items}  <PrintPreviewControl x:Name=\"preview\" AccessibleName=\"{title}\" AutoZoom=\"{{Binding AutoZoom}}\" Zoom=\"{{Binding ZoomValue}}\" Columns=\"{{Binding Columns}}\" Rows=\"{{Binding Rows}}\" X=\"0\" Y=\"{bar}\" Width=\"{width}\" Height=\"{}\" Anchor=\"Top, Bottom, Left, Right\"/>\n</Panel>\n",
        height - bar
    )
}

/// Opens the preview window (modal to the active window) over `preview`; `print` prints the
/// document (the Print button).
pub(crate) fn run_preview_window(title: &str, size: (f32, f32), preview: Result<PreviewDocument, PrintError>, print: &mut dyn FnMut() -> Result<usize, PrintError>) -> DialogResult {
    let (width, height) = (size.0.max(480.0), size.1.max(360.0));
    let mut runtime = Runtime::new();
    if !runtime.reload_from_text(&dialog_view(title, width, height)) {
        for d in runtime.diagnostics() {
            tracing::error!(target: "kubuno_desktop_print", "print preview view, line {}: {}", d.line, d.message);
        }
        return DialogResult::Abort;
    }
    match preview {
        Ok(p) => {
            runtime.with_component::<PrintPreviewControl, _>("preview", |c| c.set_preview(p));
        }
        Err(e) => {
            tracing::error!(target: "kubuno_desktop_print", "print preview: {e}");
        }
    }
    let mut vm = DialogVm { zoom: "auto".to_string(), ..DialogVm::default() };
    let mut result = DialogResult::Cancel;
    let owner = host::main_window().map(|h| h.0 as isize).or_else(|| {
        let h = crate::native::owner_window();
        (!h.is_null()).then_some(h as isize)
    });
    let mut options = HostOptions::new(title, width.round() as u32, height.round() as u32, crate::dialog_theme());
    options.chrome = kubuno_desktop_controls::host::Chrome::Kubuno;
    options.client_size = true;
    options.fit_work_area = true;
    options.diagnostics = false;
    options.owner = owner;
    options.modal = owner.is_some();
    options.form.start_position = if owner.is_some() { StartPosition::CenterParent } else { StartPosition::CenterScreen };
    options.form.show_in_taskbar = owner.is_none();
    let outcome = host::run_scoped(options, |canvas, frame| {
        let body = Rect::new(0.0, frame.chrome_top, frame.size.0, frame.size.1.max(frame.chrome_top));
        runtime.frame_model(canvas, frame, &mut vm, body);
        if std::mem::take(&mut vm.changed) {
            host::request_repaint_after(1);
        }
        if std::mem::take(&mut vm.print_requested) {
            match print() {
                Ok(_) => result = DialogResult::OK,
                Err(e) => tracing::error!(target: "kubuno_desktop_print", "printing from the preview failed: {e}"),
            }
        }
        if std::mem::take(&mut vm.close_requested) {
            runtime.close(CloseReason::UserClosing);
            host::request_repaint_after(1);
        }
    });
    if let Err(e) = outcome {
        tracing::error!(target: "kubuno_desktop_print", "the preview window could not be opened: {e}");
        return DialogResult::Abort;
    }
    result
}

// ── The page layout buttons of the tool bar ─────────────────────────────────────────────────────

/// A tool bar button showing a page layout glyph (1, 2, 3, 4 or 6 pages) — the preview dialog's.
#[derive(Component)]
#[kubuno(extends = ButtonBase, overrides(Control))]
#[toolbox(hidden, icon = "file-search", category = "Printing")]
pub struct PreviewPagesButton {
    base: ButtonBaseCore,
    /// How many pages the glyph shows: 1, 2, 3, 4 or 6.
    #[property]
    #[category("Appearance")]
    #[default_value(1)]
    pub pages: u32,
    /// Whether the layout is the current one.
    #[property]
    #[category("Appearance")]
    #[default_value(false)]
    pub checked: bool,
}

impl Default for PreviewPagesButton {
    fn default() -> Self {
        let mut base = ButtonBaseCore::default();
        base.control.styles.set(ControlStyles::OPTIMIZED_DOUBLE_BUFFER, false);
        Self { base, pages: 1, checked: false }
    }
}

impl PreviewPagesButton {
    /// The glyph's page rectangles in a 16-DIP box at `(x, y)`: columns × rows of small pages.
    pub fn glyph(pages: u32, x: f32, y: f32) -> Vec<Rect> {
        let (cols, rows) = LAYOUTS.iter().find(|l| l.0 == pages).map_or((1, 1), |l| (l.1, l.2));
        let (w, h) = (16.0f32, 16.0f32);
        let gap = 2.0;
        let (pw, ph) = ((w - gap * (cols as f32 - 1.0)) / cols as f32, (h - gap * (rows as f32 - 1.0)) / rows as f32);
        let mut out = Vec::new();
        for r in 0..rows {
            for c in 0..cols {
                out.push(Rect::from_xywh(x + c as f32 * (pw + gap), y + r as f32 * (ph + gap), pw, ph));
            }
        }
        out
    }
}

impl Control for PreviewPagesButton {
    fn get_preferred_size(&self, _canvas: &dyn Canvas, _proposed: Size) -> Size {
        Size { width: 36.0, height: 36.0 }
    }

    fn on_paint(&mut self, e: &mut PaintEventCx<'_>) {
        let g = e.graphics;
        let theme = g.theme_colors();
        let b = e.clip_rectangle;
        let r = Rect::from_xywh(b.left + ((b.right - b.left) - 32.0).max(0.0) / 2.0, b.top + ((b.bottom - b.top) - 32.0).max(0.0) / 2.0, 32.0, 32.0);
        if e.state.pressed {
            g.fill_rounded(&r, 6.0, &theme.control_fill_pressed);
        } else if e.state.hot {
            g.fill_rounded(&r, 6.0, &theme.control_fill_hover);
        } else if self.checked {
            g.fill_rounded(&r, 6.0, &theme.accent_light);
        }
        let ink = if e.state.disabled {
            theme.text_tertiary
        } else if self.checked {
            theme.accent
        } else {
            theme.text_primary
        };
        let pen = Pen::new(Color::from(ink), 1.25);
        for page in Self::glyph(self.pages, r.left + 8.0, r.top + 8.0) {
            g.draw_rounded_rectangle(&pen, Rect::new(page.left + 0.5, page.top + 0.5, page.right - 0.5, page.bottom - 0.5), 1.0);
        }
        e.raise(self, "OnPaint");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_dialog_view_compiles_and_owns_its_preview_control() {
        let mut runtime = Runtime::new();
        assert!(runtime.reload_from_text(&dialog_view("Print preview", 960.0, 720.0)), "{:?}", runtime.diagnostics());
        assert!(runtime.diagnostics().is_empty(), "{:?}", runtime.diagnostics());
        assert_eq!(runtime.with_component::<PrintPreviewControl, _>("preview", |c| c.page_count()), Some(0));
        assert_eq!(runtime.with_component::<PreviewPagesButton, _>("pages6", |b| b.pages), Some(6));
    }

    #[test]
    fn the_tool_bar_state_is_what_the_preview_binds() {
        let mut vm = DialogVm { zoom: "auto".into(), ..DialogVm::default() };
        assert_eq!(vm.get("AutoZoom"), Some(Value::Bool(true)));
        vm.set("Zoom", Value::Str("0.5".into()));
        assert_eq!((vm.get("AutoZoom"), vm.get("ZoomValue")), (Some(Value::Bool(false)), Some(Value::F32(0.5))));
        let sender = ElementRef::detached("pages4");
        assert!(vm.dispatch_event("pages4_click", &sender, &mut kubuno_desktop_views::events::EmptyEventArgs));
        assert_eq!((vm.get("Columns"), vm.get("Rows")), (Some(Value::F32(2.0)), Some(Value::F32(2.0))));
        assert_eq!(vm.get("Layout4"), Some(Value::Bool(true)));
        assert!(vm.dispatch_event("print_click", &sender, &mut kubuno_desktop_views::events::EmptyEventArgs) && vm.print_requested);
        assert!(!vm.dispatch_event("unknown", &sender, &mut kubuno_desktop_views::events::EmptyEventArgs));
    }
}
