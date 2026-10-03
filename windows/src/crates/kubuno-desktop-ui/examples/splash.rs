//! The splash screens of the Kubuno desktop apps.
//!
//! * `cargo run -p kubuno-desktop-ui --example splash -- drive` shows Drive's splash over a simulated
//!   start-up, then a stand-in main window it fades into (`kubuno`, `drive`, `chat`,
//!   `documents`); `--exit-after <ms>` closes the stand-in window by itself.
//! * `cargo run -p kubuno-desktop-ui --example splash -- --still <dir>` renders every artwork at 100, 125,
//!   150, 175 and 200 % into `<dir>\<artwork>-<percent>.bmp`, over a neutral desktop.

use std::time::{Duration, Instant};

use kubuno_desktop_ui::splash::{render_still, Artwork, SplashContent, SplashScreen};
use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{BeginPaint, CreateSolidBrush, DeleteObject, EndPaint, FillRect, HGDIOBJ, PAINTSTRUCT};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW, LoadCursorW, PostQuitMessage, RegisterClassExW, SetTimer, ShowWindow,
    TranslateMessage, CW_USEDEFAULT, IDC_ARROW, MSG, SW_SHOWNORMAL, WM_DESTROY, WM_PAINT, WM_TIMER, WNDCLASSEXW, WS_EX_APPWINDOW, WS_OVERLAPPEDWINDOW,
};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Some(i) = args.iter().position(|a| a == "--still") {
        let dir = args.get(i + 1).map(std::path::PathBuf::from).unwrap_or_else(|| std::path::PathBuf::from("."));
        if let Err(e) = stills(&dir) {
            eprintln!("{e}");
            std::process::exit(1);
        }
        return;
    }
    let art = args.iter().find_map(|a| Artwork::from_name(a)).unwrap_or(Artwork::Kubuno);
    let exit_after = args.iter().position(|a| a == "--exit-after").and_then(|i| args.get(i + 1)).and_then(|s| s.parse::<u32>().ok());
    let started = Instant::now();
    let splash = SplashScreen::new().artwork(art).version(env!("CARGO_PKG_VERSION")).license(env!("CARGO_PKG_LICENSE")).show();
    println!("shown after {:?}", started.elapsed());
    for (i, step) in ["Initialisation des modules…", "Chargement des polices…", "Lecture des réglages…", "Connexion au serveur…", "Préparation de l'espace de travail…"]
        .into_iter()
        .enumerate()
    {
        std::thread::sleep(Duration::from_millis(420));
        splash.step(step, (i + 1) as f32 / 6.0);
    }
    std::thread::sleep(Duration::from_millis(420));
    if let Some(t) = splash.time_to_first_paint() {
        println!("first frame {} ms after the process started", t.as_millis());
    }
    // SAFETY: a plain Win32 window on this thread, its loop run below.
    unsafe {
        let Ok(instance) = GetModuleHandleW(None) else { return };
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(stand_in),
            hInstance: instance.into(),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            lpszClassName: w!("KubunoSplashStandIn"),
            ..Default::default()
        };
        RegisterClassExW(&wc);
        let Ok(hwnd) = CreateWindowExW(
            WS_EX_APPWINDOW,
            w!("KubunoSplashStandIn"),
            w!("Fenêtre principale (démonstration)"),
            WS_OVERLAPPEDWINDOW,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            1280,
            800,
            None,
            None,
            Some(instance.into()),
            None,
        ) else {
            return;
        };
        let _ = ShowWindow(hwnd, SW_SHOWNORMAL);
        if let Some(ms) = exit_after {
            SetTimer(Some(hwnd), 1, ms, None);
        }
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    splash.wait_closed(Duration::from_secs(5));
}

unsafe extern "system" fn stand_in(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // SAFETY: the window procedure of the stand-in window.
    unsafe {
        match msg {
            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                let dc = BeginPaint(hwnd, &mut ps);
                let brush = CreateSolidBrush(windows::Win32::Foundation::COLORREF(0x00F7F3F0));
                FillRect(dc, &ps.rcPaint as *const RECT, brush);
                let _ = DeleteObject(HGDIOBJ(brush.0));
                let _ = EndPaint(hwnd, &ps);
                LRESULT(0)
            }
            WM_TIMER => {
                let _ = DestroyWindow(hwnd);
                LRESULT(0)
            }
            WM_DESTROY => {
                PostQuitMessage(0);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

/// Every artwork at every scale, composited over a neutral desktop, as 24-bit BMP files.
fn stills(dir: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    for art in Artwork::ALL {
        let mut content = SplashContent::new(art);
        content.version = kubuno_desktop_ui::splash::format_version(env!("CARGO_PKG_VERSION"), None);
        content.status = "Initialisation des modules…".into();
        content.progress = Some(0.62);
        for percent in [100u32, 125, 150, 175, 200] {
            let still = render_still(&content, percent as f32 / 100.0).map_err(std::io::Error::other)?;
            let path = dir.join(format!("{}-{percent}.bmp", art.name().to_ascii_lowercase()));
            write_bmp(&path, &still)?;
            println!("{}", path.display());
        }
    }
    Ok(())
}

fn write_bmp(path: &std::path::Path, still: &kubuno_desktop_ui::splash::Still) -> std::io::Result<()> {
    let (w, h) = (still.width as usize, still.height as usize);
    let row = (w * 3).div_ceil(4) * 4;
    let mut data = Vec::with_capacity(54 + row * h);
    let file_size = (54 + row * h) as u32;
    data.extend_from_slice(b"BM");
    data.extend_from_slice(&file_size.to_le_bytes());
    data.extend_from_slice(&0u32.to_le_bytes());
    data.extend_from_slice(&54u32.to_le_bytes());
    data.extend_from_slice(&40u32.to_le_bytes());
    data.extend_from_slice(&(w as i32).to_le_bytes());
    data.extend_from_slice(&(h as i32).to_le_bytes());
    data.extend_from_slice(&1u16.to_le_bytes());
    data.extend_from_slice(&24u16.to_le_bytes());
    data.extend_from_slice(&[0u8; 24]);
    for y in (0..h).rev() {
        for x in 0..w {
            let i = (y * w + x) * 4;
            let a = still.pixels[i + 3] as f32 / 255.0;
            // A neutral desktop: a soft light gradient.
            let t = y as f32 / h as f32;
            let back = [218.0 - 30.0 * t, 212.0 - 26.0 * t, 206.0 - 20.0 * t];
            for (c, b) in back.iter().enumerate() {
                // Premultiplied: colour + background × (1 − alpha).
                let v = still.pixels[i + c] as f32 + b * (1.0 - a);
                data.push(v.round().clamp(0.0, 255.0) as u8);
            }
        }
        data.resize(data.len() + (row - w * 3), 0);
    }
    std::fs::write(path, data)
}
