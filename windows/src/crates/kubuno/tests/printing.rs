//! `kubuno::printing`: the printing components of a `#[kubuno::view]` (typed fields, handlers that
//! draw the pages) and the handles created in code.

use kubuno::prelude::*;
use kubuno::printing::components;
use kubuno::ui::graphics::{Color, RectExt};
use kubuno::ui::Rect;
use kubuno::views::runtime::Runtime;

#[kubuno::view(xml = r#"
<Panel DesignWidth="400" DesignHeight="200" Title="Report">
  <PrintDocument x:Name="print_document1" DocumentName="Report" OnBeginPrint="print_document1_begin_print" OnPrintPage="print_document1_print_page"/>
  <PrintPreviewDialog x:Name="print_preview_dialog1" Document="print_document1"/>
  <PrintDialog x:Name="print_dialog1" Document="print_document1"/>
  <PageSetupDialog x:Name="page_setup_dialog1" Document="print_document1"/>
  <PrintPreviewControl x:Name="preview" Document="print_document1" X="0" Y="40" Width="400" Height="160"/>
  <Button x:Name="print" Text="Print" X="8" Y="8" Width="80" Height="28"/>
</Panel>"#)]
#[derive(Default)]
pub struct Report {
    lines: Vec<String>,
    page: usize,
}

impl Report {
    fn print_document1_begin_print(&mut self, _sender: &Control, _e: &mut PrintEventArgs) {
        self.page = 0;
    }

    fn print_document1_print_page(&mut self, _sender: &Control, e: &mut PrintPageEventArgs) {
        self.page += 1;
        e.graphics().fill_rectangle(Color::BLACK, Rect::from_xywh(e.margin_bounds.left, e.margin_bounds.top, 10.0, 10.0));
        self.lines.push(format!("page {}", self.page));
        e.has_more_pages = self.page < 3;
    }
}

/// A UI thread has a COM apartment (the host initialises OLE): Direct2D's imaging, which the pages are
/// rendered with, needs it.
fn ui_thread() {
    #[link(name = "ole32")]
    extern "system" {
        fn CoInitializeEx(reserved: *const std::ffi::c_void, coinit: u32) -> i32;
    }
    // SAFETY: initialises COM for this thread (apartment-threaded); a second call returns S_FALSE.
    let _ = unsafe { CoInitializeEx(std::ptr::null(), 2) };
}

fn type_of<T>(_: &T) -> &'static str {
    std::any::type_name::<T>()
}

#[test]
fn the_view_has_typed_printing_fields() {
    let mut view = Report::default();
    view.initialize_component();
    assert_eq!(type_of(&view.print_document1), "kubuno::printing::PrintDocument");
    assert_eq!(type_of(&view.print_preview_dialog1), "kubuno::printing::PrintPreviewDialog");
    assert_eq!(type_of(&view.print_dialog1), "kubuno::printing::PrintDialog");
    assert_eq!(type_of(&view.page_setup_dialog1), "kubuno::printing::PageSetupDialog");
    assert_eq!(type_of(&view.preview), "kubuno::printing::PrintPreviewControl");
    assert_eq!(view.print_document1.as_control().get_name(), "print_document1");
    // Linked to the view: out of its window, the component cannot be reached (nothing panics).
    assert_eq!(view.print_document1.get_document_name(), "");
}

#[test]
fn the_views_handler_draws_the_pages_through_the_runtime() {
    ui_thread();
    let mut view = Report::default();
    view.initialize_component();
    let text = kubuno::__private::compose_text(view.form());
    let mut runtime = Runtime::new();
    assert!(runtime.reload_from_text(&text), "{:?}\n{text}", runtime.diagnostics());
    assert_eq!(runtime.with_component::<components::PrintDocument, _>("print_document1", |d| d.document_name.clone()).as_deref(), Some("Report"));
    let scope = runtime.components();
    let pages = scope
        .with_dispatch::<components::PrintDocument, _>(&mut view, "print_document1", |d| d.render_recorded())
        .expect("the document of the view")
        .expect("rendered");
    assert_eq!(pages.len(), 3);
    assert!(pages.iter().all(|(_, list)| list.describe() == ["FillRect"]), "each page drawn by the view's method");
    // BeginPrint reset the counter; the preview control of the view rendered its pages in the same
    // sync, through the same handlers.
    assert_eq!(view.lines, ["page 1", "page 2", "page 3", "page 1", "page 2", "page 3"]);
}

#[test]
fn a_document_created_in_code_prints_through_its_closures() {
    ui_thread();
    let doc = PrintDocument::new().document_name("Code");
    let mut n = 0;
    doc.on_print_page(move |_, e| {
        n += 1;
        e.graphics().fill_rectangle(Color::BLACK, e.margin_bounds);
        e.has_more_pages = n < 2;
    });
    assert_eq!(doc.get_document_name(), "Code");
    let pages = doc.with(|d| d.render_recorded()).expect("own document").expect("rendered");
    assert_eq!(pages.len(), 2);
    let mut settings = doc.default_page_settings();
    settings.landscape = true;
    doc.set_default_page_settings(settings);
    assert!(doc.default_page_settings().landscape);
    // The dialogs take it (code-first ergonomics; showing them needs a window, not tested here).
    let _preview = PrintPreviewDialog::new().document(&doc);
    let _setup = PageSetupDialog::new().document(&doc);
    let _print = PrintDialog::new().document(&doc);
}
