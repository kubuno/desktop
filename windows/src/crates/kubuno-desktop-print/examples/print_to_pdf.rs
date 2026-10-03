//! Prints a three-page document to "Microsoft Print to PDF" into a file, without any dialog, and
//! renders its preview pages into PNG-free raw checks (the live check of `vskubuno/docs/PRINTING.md`).
//!
//! ```text
//! cargo run -p kubuno-desktop-print --example print_to_pdf -- C:\kubuno-build\agent-print\out\three.pdf
//! ```

use kubuno_desktop_print::{PageSettings, PrintDocument, PrinterSettings};
use kubuno_desktop_ui::graphics::{Color, Font, FontStyle, Pen, RectExt, StringFormat};
use kubuno_desktop_ui::Rect;

fn main() {
    let out = std::env::args().nth(1).unwrap_or_else(|| r"C:\kubuno-build\agent-print\out\three.pdf".to_string());
    let printer = std::env::args().nth(2).unwrap_or_else(|| "Microsoft Print to PDF".to_string());
    // SAFETY: once, at the start of the thread (a Kubuno window's host does this itself).
    let hr = unsafe { windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED) };
    println!("COM: {hr:?}");
    println!("installed printers: {:?}", PrinterSettings::installed_printers());
    println!("default printer: {:?}", PrinterSettings::default_printer_name());
    let settings = PrinterSettings::for_printer(&printer);
    println!("valid: {}, paper sizes: {}, first: {:?}", settings.is_valid(), settings.paper_sizes().len(), settings.paper_sizes().first());
    println!("default page settings: {:?}", settings.default_page_settings());

    let mut doc = PrintDocument::named("Kubuno three pages");
    {
        let p = doc.printer_settings_mut();
        p.printer_name = printer.clone();
        p.print_to_file = true;
        p.print_file_name = out.clone();
    }
    let mut page = 0;
    doc.on_begin_print(|_, e| println!("BeginPrint {:?}", e.print_action));
    doc.on_query_page_settings(|_, e| {
        // The second page in landscape (a page ticket of its own).
        e.page_settings.landscape = e.page_number == 2;
    });
    doc.on_print_page(move |_, e| {
        page += 1;
        let g = e.graphics();
        let m = e.margin_bounds;
        g.draw_rectangle(&Pen::new(Color::rgb(0x1A, 0x73, 0xE8), 2.0), m);
        g.fill_rectangle(Color::rgb(0xE8, 0xF0, 0xFE), Rect::from_xywh(m.left, m.top, m.right - m.left, 60.0));
        let title = Font::new("Segoe UI", 24.0, FontStyle::BOLD);
        g.draw_string(&format!("Page {page} of 3"), &title, Color::BLACK, Rect::from_xywh(m.left + 12.0, m.top + 10.0, m.right - m.left - 24.0, 44.0), &StringFormat::generic_default());
        let body = Font::new("Segoe UI", 11.0, FontStyle::REGULAR);
        let text = format!("Printed by Kubuno with Direct2D over an XPS print job. Page bounds {:?}, margins {:?}, landscape {}.", e.page_bounds, e.margin_bounds, e.page_settings.landscape);
        g.draw_string(&text, &body, Color::rgb(0x33, 0x33, 0x33), Rect::from_xywh(m.left + 12.0, m.top + 80.0, m.right - m.left - 24.0, 200.0), &StringFormat::generic_default());
        g.fill_ellipse(Color::rgb(0x34, 0xA8, 0x53), Rect::from_xywh(m.left + 12.0, m.top + 300.0, 120.0, 120.0));
        e.has_more_pages = page < 3;
    });
    doc.on_end_print(|_, e| println!("EndPrint {:?}", e.print_action));
    let started = std::time::Instant::now();
    match doc.print() {
        Ok(pages) => println!("printed {pages} page(s) in {:?}", started.elapsed()),
        Err(e) => println!("print failed: {e}"),
    }
    match std::fs::metadata(&out) {
        Ok(m) => println!("{out}: {} bytes", m.len()),
        Err(e) => println!("{out}: {e}"),
    }

    // The preview of the same document (the handler's counter restarts with BeginPrint in real code;
    // here a second document).
    let mut preview_doc = PrintDocument::named("preview");
    let mut n = 0;
    preview_doc.on_print_page(move |_, e| {
        n += 1;
        e.graphics().fill_rectangle(Color::BLACK, e.margin_bounds);
        e.has_more_pages = n < 3;
    });
    let _ = PageSettings::default();
    match preview_doc.render_preview() {
        Ok(preview) => {
            println!("preview: {} page(s), sizes {:?}", preview.pages.len(), preview.pages.iter().map(|p| p.size).collect::<Vec<_>>());
            match preview.rasterize(0, 85, 110) {
                Ok(bmp) => {
                    let dark = bmp.pixels.chunks(4).filter(|p| p[0] < 128 && p[1] < 128 && p[2] < 128).count();
                    println!("page 1 rasterised {}x{}: {} dark pixels of {}", bmp.width, bmp.height, dark, bmp.width * bmp.height);
                }
                Err(e) => println!("rasterise failed: {e}"),
            }
        }
        Err(e) => println!("preview failed: {e}"),
    }
}
