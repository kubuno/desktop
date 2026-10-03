//! Diagnostic tool for the shell-icon extraction pipeline.
//!
//! Extracts the icons of `C:\` and of a temporary `.txt` file at 96 px and
//! 256 px, saves them as PNGs under `%TEMP%\icon_dump\`, and prints alpha
//! statistics that prove (or disprove) that the pixels are anti-aliased and
//! premultiplied:
//! - "intermediate alpha" pixels (0 < a < 255) must exist on glyph edges,
//!   otherwise the source has hard (jagged) edges;
//! - no color channel may exceed its alpha (PARGB invariant), otherwise the
//!   buffer is straight-alpha and Direct2D (which expects PREMULTIPLIED)
//!   would render bright fringes.
//!
//! Run with: `cargo run -p kubuno-drive-desktop-app-storage --example icon_dump`

use std::io::Write as _;

use kubuno_drive_desktop_app_storage::icons::ShellBitmap;
use kubuno_drive_desktop_app_storage::windows_storage::WindowsStorable;
use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
use windows::Win32::UI::Shell::{SIIGBF_BIGGERSIZEOK, SIIGBF_ICONONLY};

fn main() {
    // SAFETY: single-threaded example; COM stays initialized for the process
    // lifetime.
    unsafe {
        CoInitializeEx(None, COINIT_APARTMENTTHREADED)
            .ok()
            .expect("COM init");
    }

    let out_dir = std::env::temp_dir().join("icon_dump");
    std::fs::create_dir_all(&out_dir).expect("create output dir");

    // A throwaway .txt so the generic text-file icon is exercised.
    let txt_path = std::env::temp_dir().join("icon_dump_probe.txt");
    std::fs::write(&txt_path, b"probe").expect("write probe txt");

    let targets: [(&str, String); 2] = [
        ("drive_c", "C:\\".to_string()),
        ("txt", txt_path.to_string_lossy().into_owned()),
    ];

    for (name, path) in &targets {
        for &size in &[96i32, 256i32] {
            let item = match WindowsStorable::try_parse(path) {
                Some(item) => item,
                None => {
                    println!("{name}: failed to parse {path}");
                    continue;
                }
            };
            let bitmap = match item
                .storable()
                .try_get_thumbnail(size, SIIGBF_ICONONLY | SIIGBF_BIGGERSIZEOK)
            {
                Ok(b) => b,
                Err(e) => {
                    println!("{name} @ {size}px: GetImage failed: {e}");
                    continue;
                }
            };

            println!(
                "=== {name} requested {size}px -> got {}x{} ===",
                bitmap.width, bitmap.height
            );
            print_alpha_report(&bitmap);

            let file = out_dir.join(format!("{name}_{size}.png"));
            save_png(&file, &bitmap);
            println!("  saved {}", file.display());

            // 4x nearest-neighbor magnification of the top-left quadrant so
            // shell-upscaling blur (48 -> 96) is visible to the naked eye.
            if size == 96 {
                let zoom = magnify_top_left(&bitmap, 4);
                let zoom_file = out_dir.join(format!("{name}_{size}_zoom4x.png"));
                save_png(&zoom_file, &zoom);
                println!("  saved {}", zoom_file.display());
            }
            // Reference: 256px source cleanly box-downscaled to 96 shows what
            // a properly downscaled icon looks like at the same size.
            if size == 256 {
                let down = box_downscale(&bitmap, 96, 96);
                let zoom = magnify_top_left(&down, 4);
                let zoom_file = out_dir.join(format!("{name}_from256_zoom4x.png"));
                save_png(&zoom_file, &zoom);
                println!("  saved {}", zoom_file.display());
            }
        }
    }

    println!("\nPNGs written to {}", out_dir.display());
}

/// Prints anti-aliasing / premultiplication evidence for a BGRA bitmap.
fn print_alpha_report(bitmap: &ShellBitmap) {
    let (mut a0, mut a255, mut a_mid) = (0usize, 0usize, 0usize);
    let mut straight_violations = 0usize; // color channel > alpha
    for px in bitmap.bgra.as_chunks::<4>().0 {
        let (b, g, r, a) = (px[0], px[1], px[2], px[3]);
        match a {
            0 => a0 += 1,
            255 => a255 += 1,
            _ => a_mid += 1,
        }
        if b > a || g > a || r > a {
            straight_violations += 1;
        }
    }
    let total = (bitmap.width * bitmap.height) as usize;
    println!(
        "  alpha histogram: a=0: {a0}, a=255: {a255}, 0<a<255 (anti-aliased): {a_mid} \
         ({:.1}% of {total})",
        a_mid as f64 * 100.0 / total as f64
    );
    println!(
        "  premultiplied invariant (channel<=alpha): {} ({} violating pixels)",
        if straight_violations == 0 { "OK" } else { "VIOLATED -> straight alpha" },
        straight_violations
    );

    // Sample a few edge pixels: walk the middle row and print the pixels
    // around the first opaque->transparent transition.
    let w = bitmap.width as usize;
    let row = bitmap.height as usize / 2;
    let row_px: Vec<[u8; 4]> = bitmap.bgra[row * w * 4..(row + 1) * w * 4]
        .as_chunks::<4>().0.iter()
        .map(|c| [c[0], c[1], c[2], c[3]])
        .collect();
    if let Some(first_visible) = row_px.iter().position(|p| p[3] > 0) {
        let from = first_visible.saturating_sub(1);
        let to = (first_visible + 4).min(w);
        let samples: Vec<String> = (from..to)
            .map(|x| {
                let [b, g, r, a] = row_px[x];
                format!("x={x} bgra=({b},{g},{r},{a})")
            })
            .collect();
        println!("  middle-row left edge: {}", samples.join("  "));
    } else {
        println!("  middle row fully transparent");
    }
}

/// Simple box-filter downscale (averages source pixels per target pixel).
/// Correct only for premultiplied sources, which is what the pipeline emits.
fn box_downscale(bitmap: &ShellBitmap, ow: u32, oh: u32) -> ShellBitmap {
    let mut out = vec![0u8; (ow * oh * 4) as usize];
    for y in 0..oh {
        let sy0 = (y * bitmap.height / oh) as usize;
        let sy1 = ((y + 1) * bitmap.height).div_ceil(oh) as usize;
        for x in 0..ow {
            let sx0 = (x * bitmap.width / ow) as usize;
            let sx1 = ((x + 1) * bitmap.width).div_ceil(ow) as usize;
            let mut acc = [0u32; 4];
            let n = ((sy1 - sy0) * (sx1 - sx0)) as u32;
            for sy in sy0..sy1 {
                for sx in sx0..sx1 {
                    let i = (sy * bitmap.width as usize + sx) * 4;
                    for (c, a) in acc.iter_mut().enumerate() {
                        *a += bitmap.bgra[i + c] as u32;
                    }
                }
            }
            let dst = ((y * ow + x) * 4) as usize;
            for c in 0..4 {
                out[dst + c] = (acc[c] / n) as u8;
            }
        }
    }
    ShellBitmap { width: ow, height: oh, bgra: out }
}

/// Nearest-neighbor magnification of the top-left quadrant (overlay corner).
fn magnify_top_left(bitmap: &ShellBitmap, factor: u32) -> ShellBitmap {
    let (qw, qh) = (bitmap.width / 2, bitmap.height / 2);
    let (ow, oh) = (qw * factor, qh * factor);
    let mut out = vec![0u8; (ow * oh * 4) as usize];
    for y in 0..oh {
        for x in 0..ow {
            let src = (((y / factor) * bitmap.width + (x / factor)) * 4) as usize;
            let dst = ((y * ow + x) * 4) as usize;
            out[dst..dst + 4].copy_from_slice(&bitmap.bgra[src..src + 4]);
        }
    }
    ShellBitmap { width: ow, height: oh, bgra: out }
}

/// Minimal dependency-free PNG writer (RGBA8, zlib stream with stored deflate
/// blocks). The premultiplied source is un-premultiplied first so viewers show
/// the icon as intended.
fn save_png(path: &std::path::Path, bitmap: &ShellBitmap) {
    let (w, h) = (bitmap.width as usize, bitmap.height as usize);

    // Raw scanlines: filter byte 0 + RGBA (straight alpha for viewing).
    let mut raw = Vec::with_capacity(h * (1 + w * 4));
    for y in 0..h {
        raw.push(0u8);
        for x in 0..w {
            let i = (y * w + x) * 4;
            let (b, g, r, a) = (
                bitmap.bgra[i],
                bitmap.bgra[i + 1],
                bitmap.bgra[i + 2],
                bitmap.bgra[i + 3],
            );
            let un = |c: u8| -> u8 {
                if a == 0 {
                    0
                } else {
                    ((c as u32 * 255 + a as u32 / 2) / a as u32).min(255) as u8
                }
            };
            raw.extend_from_slice(&[un(r), un(g), un(b), a]);
        }
    }

    // zlib: header + stored deflate blocks + adler32.
    let mut idat = vec![0x78u8, 0x01];
    for (i, chunk) in raw.chunks(65535).enumerate() {
        let last = (i + 1) * 65535 >= raw.len();
        idat.push(if last { 1 } else { 0 });
        let len = chunk.len() as u16;
        idat.extend_from_slice(&len.to_le_bytes());
        idat.extend_from_slice(&(!len).to_le_bytes());
        idat.extend_from_slice(chunk);
    }
    idat.extend_from_slice(&adler32(&raw).to_be_bytes());

    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&(w as u32).to_be_bytes());
    ihdr.extend_from_slice(&(h as u32).to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]); // 8-bit RGBA

    let mut file = std::fs::File::create(path).expect("create png");
    file.write_all(b"\x89PNG\r\n\x1a\n").unwrap();
    write_chunk(&mut file, b"IHDR", &ihdr);
    write_chunk(&mut file, b"IDAT", &idat);
    write_chunk(&mut file, b"IEND", &[]);
}

fn write_chunk(file: &mut std::fs::File, kind: &[u8; 4], data: &[u8]) {
    file.write_all(&(data.len() as u32).to_be_bytes()).unwrap();
    file.write_all(kind).unwrap();
    file.write_all(data).unwrap();
    let mut crc_input = kind.to_vec();
    crc_input.extend_from_slice(data);
    file.write_all(&crc32(&crc_input).to_be_bytes()).unwrap();
}

fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for &byte in data {
        a = (a + byte as u32) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &byte in data {
        crc ^= byte as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
        }
    }
    !crc
}
