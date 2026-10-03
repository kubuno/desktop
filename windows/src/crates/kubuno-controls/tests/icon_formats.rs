//! Every image format an icon can name is decoded and drawn at the size asked for
//! (`kubuno_controls::icon_image`): the fixtures are a red disc (PNG, JPEG, BMP, GIF, TIFF, WebP),
//! an icon file with three sizes (16 and 32 red, 48 blue), an SVG drawn in `currentColor` and a
//! 2:1 green SVG.

use kubuno_controls::icon_image::{rasterize, Raster};
use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;

fn fixture(name: &str) -> String {
    format!("{}/tests/fixtures/icons/{name}", env!("CARGO_MANIFEST_DIR"))
}

fn com() {
    // SAFETY: COM for WIC on the test's thread; a second call is harmless.
    unsafe {
        let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
    }
}

const INK: D2D1_COLOR_F = D2D1_COLOR_F { r: 0.0, g: 0.0, b: 1.0, a: 1.0 };

/// The pixel at (x, y) as (r, g, b, a), premultiplied.
fn px(r: &Raster, x: u32, y: u32) -> (u8, u8, u8, u8) {
    let i = ((y * r.width + x) * 4) as usize;
    (r.pixels[i + 2], r.pixels[i + 1], r.pixels[i], r.pixels[i + 3])
}

fn render(name: &str, size: u32) -> Raster {
    com();
    rasterize(&fixture(name), size, size, INK, None).unwrap_or_else(|| panic!("{name} could not be drawn"))
}

#[test]
fn every_raster_format_is_decoded_and_scaled() {
    for name in ["red.png", "red.jpg", "red.bmp", "red.gif", "red.tif"] {
        for size in [16, 24, 64] {
            let r = render(name, size);
            assert_eq!((r.width, r.height), (size, size), "{name}");
            let (red, green, blue, alpha) = px(&r, size / 2, size / 2);
            assert!(red > 200 && green < 60 && blue < 60 && alpha > 200, "{name} at {size}: centre is {:?}", (red, green, blue, alpha));
        }
    }
}

#[test]
fn webp_is_decoded_when_windows_has_its_codec() {
    com();
    match rasterize(&fixture("red.webp"), 8, 8, INK, None) {
        Some(r) => {
            assert_eq!((r.width, r.height), (8, 8));
        }
        // The system's WebP codec (Windows 10 1809+, "WebP Image Extensions") is missing here.
        None => eprintln!("warning: no WebP codec on this machine: WebP icons cannot be drawn"),
    }
}

#[test]
fn an_icon_file_gives_its_closest_size() {
    // 48 and more: the blue 48 px frame; 32 and less: a red frame.
    let (r, g, b, _) = px(&render("multi.ico", 48), 24, 24);
    assert!(b > 200 && r < 60 && g < 60, "48 px: {:?}", (r, g, b));
    let (r, g, b, _) = px(&render("multi.ico", 20), 10, 10);
    assert!(r > 200 && b < 60 && g < 60, "20 px: {:?}", (r, g, b));
}

#[test]
fn an_svg_follows_the_colour_and_its_scaling() {
    // `currentColor` is the colour asked for: the square's outline is blue.
    let r = render("currentcolor.svg", 48);
    let edge = (0..48).map(|y| px(&r, 6, y)).find(|p| p.3 > 200).expect("an outline pixel on the left edge");
    assert!(edge.2 > 200 && edge.0 < 40, "{edge:?}");
    // A 2:1 SVG fits a square box (transparent bands above and below)…
    let fit = render("wide.svg", 32);
    assert_eq!(px(&fit, 16, 2).3, 0);
    assert!(px(&fit, 16, 16).1 > 200);
    // …and covers it with `scaling=Fill`.
    com();
    let fill = rasterize(&format!("{}\u{1}scaling=Fill", fixture("wide.svg")), 32, 32, INK, None).expect("fill");
    assert!(px(&fill, 16, 2).1 > 200);
}

#[test]
fn a_tint_recolours_and_a_mirror_flips() {
    com();
    let tinted = rasterize(&format!("{}\u{1}tint=#00ff00", fixture("red.png")), 32, 32, INK, None).expect("tint");
    let (r, g, b, _) = px(&tinted, 16, 16);
    assert!(g > 200 && r < 30 && b < 30, "{:?}", (r, g, b));
    // A glyph of the set is drawn too; mirrored, its left and right swap.
    let glyph = rasterize("ChevronRight", 24, 24, INK, None).expect("glyph");
    let mirrored = rasterize("ChevronRight\u{1}mirror", 24, 24, INK, None).expect("mirrored glyph");
    // The tip of the chevron, on its middle row: right of the centre, mirrored left of it.
    let ink_x = |r: &Raster| (0..24).filter(|x| px(r, *x, 12).3 > 128).max().unwrap_or(0);
    assert!(ink_x(&glyph) > ink_x(&mirrored), "the chevron points right, mirrored left");
}

#[test]
fn a_one_pixel_png_is_enlarged() {
    let r = render("one.png", 8);
    assert!(px(&r, 4, 4).3 > 200, "{:?}", px(&r, 4, 4));
}

#[test]
fn a_missing_file_draws_nothing() {
    com();
    assert!(rasterize(&fixture("nope.png"), 16, 16, INK, None).is_none());
}

/// A real WebP image (not shipped: a local file, when present), decoded and drawn.
#[test]
fn a_real_webp_when_present() {
    let path = r"C:\kubuno-build\icons-live\icons-desktop\src\resources\photo.webp";
    if !std::path::Path::new(path).exists() {
        return;
    }
    com();
    let r = rasterize(path, 64, 64, INK, None).expect("a WebP image");
    let opaque = r.pixels.chunks(4).filter(|p| p[3] > 0).count();
    assert!(opaque > 100, "{opaque} opaque pixels");
}
