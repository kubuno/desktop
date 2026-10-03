//! The printing stack without a printer: settings, the print loop over recording pages, the preview
//! layout, the registry, and a view whose `.kbview` handlers draw the pages.

use std::cell::RefCell;
use std::rc::Rc;

use kubuno_desktop_print::{hundredths_to_dip, Margins, PageSettings, PaperSize, PreviewPagesButton, PrintAction, PrintDocument, PrintPageEventArgs, PrintPreviewControl};
use kubuno_desktop_ui::graphics::{Color, RectExt};
use kubuno_desktop_ui::Rect;
use kubuno_desktop_views::binding::{MapViewModel, Value, ViewModel};
use kubuno_desktop_views::events::{ElementRef, EventArgs};
use kubuno_desktop_views::registry;
use kubuno_desktop_views::runtime::Runtime;

/// The printer drivers (winspool, the Print to PDF driver) are not meant to be driven from several
/// threads at once, and Direct2D's imaging needs COM: the tests that reach them take this lock and
/// an apartment, as a UI thread has.
fn printer_thread() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    // SAFETY: initialises COM for this thread (a second call only returns S_FALSE).
    let _ = unsafe { windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED) };
    LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[test]
fn margins_and_papers_read_the_view_spellings() {
    assert_eq!(Margins::parse("50, 60, 70, 80"), Some(Margins::new(50, 60, 70, 80)));
    assert_eq!(Margins::parse(" 25 "), Some(Margins::all(25)));
    assert_eq!(Margins::parse("1,2,3"), None);
    assert_eq!(Margins::parse("-1"), None);
    assert_eq!(Margins::default().to_string(), "100, 100, 100, 100");
    assert_eq!(PaperSize::by_name("a4").map(|p| (p.width, p.height, p.raw_kind)), Some((827, 1169, 9)));
    assert!(PaperSize::by_name("Tabloid 2").is_none());
    assert!((hundredths_to_dip(100.0) - 96.0).abs() < 1e-4);
}

#[test]
fn page_settings_turn_with_the_orientation() {
    let mut p = PageSettings { paper_size: Some(PaperSize::letter()), ..PageSettings::default() };
    assert_eq!(p.size(), (850.0, 1100.0));
    p.landscape = true;
    assert_eq!(p.bounds(), Rect::new(0.0, 0.0, 1100.0, 850.0));
    p.margins = Margins::new(50, 100, 25, 75);
    assert_eq!(p.margin_bounds(), Rect::new(50.0, 25.0, 1000.0, 775.0));
}

/// A document whose handler draws one rectangle per page and records the events it saw.
fn three_pages(log: Rc<RefCell<Vec<String>>>) -> PrintDocument {
    let mut doc = PrintDocument::named("test");
    let l = log.clone();
    doc.on_begin_print(move |_, e| l.borrow_mut().push(format!("begin {:?}", e.print_action)));
    let l = log.clone();
    doc.on_query_page_settings(move |_, e| {
        e.page_settings.landscape = e.page_number == 2;
        l.borrow_mut().push(format!("query {}", e.page_number));
    });
    let l = log.clone();
    let mut n = 0;
    doc.on_print_page(move |_, e| {
        n += 1;
        e.graphics().fill_rectangle(Color::BLACK, e.margin_bounds);
        l.borrow_mut().push(format!("page {} landscape={} wider={}", e.page_number, e.page_settings.landscape, e.page_bounds.right > e.page_bounds.bottom));
        e.has_more_pages = n < 3;
    });
    let l = log;
    doc.on_end_print(move |_, e| l.borrow_mut().push(format!("end {:?}", e.print_action)));
    doc
}

#[test]
fn the_print_loop_raises_the_events_in_order_until_has_more_pages_is_cleared() {
    let _printer = printer_thread();
    let log = Rc::new(RefCell::new(Vec::new()));
    let mut doc = three_pages(log.clone());
    let pages = doc.render_recorded().expect("recorded");
    assert_eq!(pages.len(), 3);
    assert!(pages.iter().all(|(_, list)| list.describe() == ["FillRect"]), "each page drew its rectangle");
    assert_eq!(
        *log.borrow(),
        [
            "begin PrintToPreview",
            "query 1",
            "page 1 landscape=false wider=false",
            "query 2",
            "page 2 landscape=true wider=true",
            "query 3",
            "page 3 landscape=false wider=false",
            "end PrintToPreview"
        ]
    );
    assert!(pages[1].0.landscape && !pages[0].0.landscape, "QueryPageSettings changed page 2 only");
}

#[test]
fn cancelling_stops_the_job() {
    let _printer = printer_thread();
    // In BeginPrint: nothing, not even EndPrint (WinForms).
    let log = Rc::new(RefCell::new(Vec::new()));
    let mut doc = three_pages(log.clone());
    doc.on_begin_print(|_, e| e.cancel = true);
    assert_eq!(doc.render_recorded().map(|p| p.len()), Ok(0));
    assert_eq!(*log.borrow(), ["begin PrintToPreview"]);
    // In PrintPage: the loop ends, EndPrint is raised.
    let log = Rc::new(RefCell::new(Vec::new()));
    let mut doc = three_pages(log.clone());
    doc.on_print_page(|_, e| e.cancel = e.page_number == 2);
    let pages = doc.render_recorded().expect("recorded");
    assert_eq!(pages.len(), 2, "page 2 was drawn, then the job stopped");
    assert_eq!(log.borrow().last().map(String::as_str), Some("end PrintToPreview"));
}

#[test]
fn origin_at_margins_moves_the_page_graphics() {
    let _printer = printer_thread();
    let mut doc = PrintDocument::new();
    doc.origin_at_margins = true;
    doc.margins = "50, 50, 25, 25".to_string();
    doc.on_print_page(|_, e| e.graphics().fill_rectangle(Color::BLACK, Rect::from_xywh(0.0, 0.0, 10.0, 10.0)));
    let pages = doc.render_recorded().expect("recorded");
    let list = &pages[0].1;
    let op = list.ops.first().expect("one op");
    let t = op.state().map(|s| s.transform).unwrap_or_default();
    assert!((t.dx - 48.0).abs() < 0.01 && (t.dy - 24.0).abs() < 0.01, "translated to the margins: {t:?}");
    // The XML properties reach the settings.
    assert_eq!(doc.default_page_settings().margins, Margins::new(50, 50, 25, 25));
}

#[test]
fn the_properties_reach_the_settings_but_code_wins_until_they_change() {
    let _printer = printer_thread();
    let mut doc = PrintDocument::new();
    doc.landscape = true;
    doc.paper_size = "A5".to_string();
    doc.copies = 3;
    assert!(doc.default_page_settings().landscape);
    assert_eq!(doc.default_page_settings().paper_size.as_ref().map(|p| p.raw_kind), Some(11));
    assert_eq!(doc.printer_settings().copies, 3);
    doc.default_page_settings_mut().landscape = false;
    assert!(!doc.default_page_settings().landscape, "code changed it; the property did not change since");
    let before = doc.revision();
    doc.landscape = false;
    doc.landscape = true;
    assert_eq!(doc.revision(), before, "same value: nothing to apply");
}

#[test]
fn the_preview_layout_fits_and_scrolls() {
    let _printer = printer_thread();
    let mut c = PrintPreviewControl::new();
    assert!(c.layout(Rect::new(0.0, 0.0, 800.0, 600.0)).pages.is_empty(), "no pages yet");
    let mut doc = PrintDocument::new();
    let mut n = 0;
    doc.on_print_page(move |_, e| {
        n += 1;
        e.has_more_pages = n < 5;
    });
    let Ok(preview) = doc.render_preview() else {
        // No Direct2D device (a session without a GPU driver nor WARP): nothing more to check.
        return;
    };
    assert_eq!(preview.pages.len(), 5);
    c.set_preview(preview);
    let view = Rect::new(0.0, 0.0, 800.0, 600.0);
    let one = c.layout(view);
    assert_eq!(one.pages.len(), 1);
    let (_, r) = one.pages[0];
    assert!((r.bottom - r.top) <= 600.0 - 20.0 + 0.5 && r.top >= 9.5, "fitted with its margins: {r:?}");
    c.set_layout(3, 2);
    let six = c.layout(view);
    assert_eq!(six.pages.iter().map(|p| p.0).collect::<Vec<_>>(), [0, 1, 2, 3, 4], "5 pages in a 3 × 2 grid");
    c.set_start_page(3);
    assert_eq!(c.layout(view).pages.iter().map(|p| p.0).collect::<Vec<_>>(), [3, 4]);
    c.set_start_page(99);
    assert_eq!(c.start_page, 4, "clamped to the last page");
    c.set_layout(1, 1);
    c.set_zoom(1.0);
    let actual = c.layout(view);
    assert!(actual.content.1 > 600.0 && (actual.zoom - 1.0).abs() < 1e-6, "actual size scrolls");
    let bitmap = c.preview().expect("pages").rasterize(0, 40, 50).expect("rasterised");
    assert_eq!((bitmap.width, bitmap.height, bitmap.pixels.len()), (40, 50, 40 * 50 * 4));
    assert!(bitmap.pixels.chunks(4).all(|p| p == [255, 255, 255, 255]), "a blank page is white paper");
}

#[test]
fn the_layout_glyphs_have_one_rectangle_per_page() {
    for (pages, count) in [(1, 1), (2, 2), (3, 3), (4, 4), (6, 6)] {
        assert_eq!(PreviewPagesButton::glyph(pages, 0.0, 0.0).len(), count);
    }
}

#[test]
fn the_components_are_registered_as_printing_classes() {
    let mut ours: Vec<&str> = registry::all().iter().map(|c| c.name).filter(|n| registry::project_info(n).is_some_and(|i| i.crate_name == Some("kubuno_desktop_print"))).collect();
    ours.sort_unstable();
    let mut table = kubuno_desktop_views_meta::kbview::PRINT_ELEMENTS.to_vec();
    table.sort_unstable();
    assert_eq!(ours, table, "the view macro's table is what this crate registers");
    for name in ["PrintDocument", "PrintPreviewDialog", "PrintDialog", "PageSetupDialog"] {
        let info = registry::project_info(name).expect("registered");
        assert!(registry::is_non_visual(name), "{name} goes to the component tray");
        assert_eq!(info.toolbox_category, Some("Printing"), "{name}");
    }
    assert!(!registry::is_non_visual("PrintPreviewControl"), "the preview control is drawn on the form");
    let doc = registry::lookup("PrintDocument").expect("PrintDocument");
    for event in ["OnBeginPrint", "OnQueryPageSettings", "OnPrintPage", "OnEndPrint"] {
        assert!(doc.events.iter().any(|e| e.name == event), "{event}");
    }
    let print_page = doc.events.iter().find(|e| e.name == "OnPrintPage").expect("OnPrintPage");
    assert_eq!(print_page.args_type, "PrintPageEventArgs");
    assert!(print_page.args_chain.contains(&"CancelEventArgs"));
    let dialog = registry::lookup("PrintPreviewDialog").expect("PrintPreviewDialog");
    let document = dialog.properties.iter().find(|p| p.name == "Document").expect("Document");
    assert_eq!(document.editor, Some("reference:PrintDocument"), "a drop-down of the view's documents");
}

/// A view model that answers the document's `.kbview` handler.
#[derive(Default)]
struct Vm {
    inner: MapViewModel,
    pages: u32,
}

impl ViewModel for Vm {
    fn get(&self, path: &str) -> Option<Value> {
        self.inner.get(path)
    }
    fn set(&mut self, path: &str, value: Value) {
        self.inner.set(path, value)
    }
    fn dispatch_event(&mut self, handler: &str, _sender: &ElementRef<'_>, args: &mut dyn EventArgs) -> bool {
        if handler == "print_document1_print_page" {
            if let Some(e) = args.downcast_mut::<PrintPageEventArgs>() {
                self.pages += 1;
                e.graphics().fill_rectangle(Color::BLACK, e.margin_bounds);
                e.has_more_pages = self.pages < 2;
            }
            return true;
        }
        false
    }
}

#[test]
fn a_view_owns_its_documents_and_their_xml_handlers_draw_the_pages() {
    let _printer = printer_thread();
    let view = r#"<Panel DesignWidth="400" DesignHeight="300">
  <PrintDocument x:Name="print_document1" DocumentName="Invoice" Landscape="true" Margins="50, 50, 50, 50" OnPrintPage="print_document1_print_page"/>
  <PrintPreviewDialog x:Name="print_preview_dialog1" Document="print_document1"/>
  <PrintDialog x:Name="print_dialog1" Document="print_document1" AllowSomePages="true"/>
  <PageSetupDialog x:Name="page_setup_dialog1" Document="print_document1"/>
  <PrintPreviewControl x:Name="preview" Document="print_document1" Columns="2" X="0" Y="0" Width="400" Height="300"/>
</Panel>"#;
    let mut rt = Runtime::new();
    assert!(rt.reload_from_text(view), "{:?}", rt.diagnostics());
    assert_eq!(rt.with_component::<PrintDocument, _>("print_document1", |d| d.document_name.clone()).as_deref(), Some("Invoice"));
    assert_eq!(rt.with_component::<PrintDocument, _>("print_document1", |d| d.default_page_settings().landscape), Some(true));
    assert_eq!(rt.with_component::<kubuno_desktop_print::PrintDialog, _>("print_dialog1", |d| d.allow_some_pages), Some(true));
    assert_eq!(rt.with_component::<PrintPreviewControl, _>("preview", |c| c.columns), Some(2));
    // Through the scope, with the view model at hand: the XML handler draws each page.
    let scope = rt.components();
    let mut vm = Vm::default();
    let pages = scope.with_dispatch::<PrintDocument, _>(&mut vm, "print_document1", |d| d.render_recorded()).expect("reached").expect("rendered");
    assert_eq!(pages.len(), 2);
    // Then the view's preview control, in the same sync, rendered its pages through the same handler (whose
    // counter the test does not reset: one more page, the last).
    assert_eq!(vm.pages, 3);
    assert_eq!(rt.with_component::<PrintPreviewControl, _>("preview", |c| c.page_count()), Some(1));
    assert!(pages.iter().all(|(s, list)| s.landscape && list.describe() == ["FillRect"]));
    let _ = PrintAction::PrintToPreview;
}
