//! Diagnostics of a GUI application: where its logs, its `println!`s and its panics go when
//! there is no console — the Windows Forms model (`OutputType=WinExe`: no console window, even
//! in Debug; `Debug.WriteLine`/trace output in Visual Studio's Output window).
//!
//! Kubuno desktop applications are built with `#![windows_subsystem = "windows"]`, so they
//! never open a console. [`install`] (called by the host when a window opens, unless
//! [`super::HostOptions::diagnostics`] is `false`; an application with its own window loop
//! calls it itself) then routes, **per line**:
//!
//! - to the debugger's output (`OutputDebugStringW`, Visual Studio's *Output > Debug* pane)
//!   while a debugger is attached (`IsDebuggerPresent`, checked at every line, so attaching
//!   later works);
//! - otherwise to a log file, `%LOCALAPPDATA%\Kubuno\logs\<app>.log`, rotated at 1 MiB (the
//!   previous one is kept as `<app>.1.log`).
//!
//! What is routed:
//!
//! - **`tracing`** events (a small built-in subscriber, installed as the global default unless
//!   the application already installed its own; the level is `INFO` — `KUBUNO_LOG` =
//!   `error|warn|info|debug|trace` or [`set_max_level`] changes it; the per-event traces of the
//!   views (every dispatched event) are `DEBUG`, so they only appear when asked for);
//! - **`log`** records (a logger, unless one is already set);
//! - **`println!`/`eprintln!`**: when the process has no standard output/error (no console, no
//!   pipe from a parent — the usual case of a GUI exe), they are redirected through a pipe to
//!   the same destination, line by line; a process started with its own pipes (a terminal, a test
//!   runner) keeps them;
//! - **panics** (message, location, backtrace). A panic of the UI thread (`main`) of a GUI
//!   application then shows the Kubuno crash window ([`super::crash`]: the app's name, the message,
//!   the details, *Open log*, *Copy*, *Close*) and closes the process — never a silent exit, and
//!   never a panic unwinding out of the window procedure. The window runs on its own thread, so the
//!   panicking UI thread pumps no message meanwhile; Windows' "not responding" ghosting is turned
//!   off so the application's window keeps its look. A system message box is the fallback. A
//!   background thread's panic is only logged. **Under a debugger** the debugger sees the panic
//!   first, like a .NET exception: a panic in the application's frame (its handlers) unwinds as the
//!   `rust_panic` C++ exception Visual Studio breaks on, and the crash window only opens if the
//!   developer continues ([`run_frame`]); elsewhere the hook breaks into the debugger
//!   ([`debug_break`]; `KUBUNO_BREAK_ON_PANIC=0` turns that off) before reporting.
//!
//! Nothing here allocates or does I/O until [`install`] is called.

use std::fmt::Write as _;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

/// The size at which the log file is rotated.
const MAX_LOG_BYTES: u64 = 1024 * 1024;

static DISABLED: AtomicBool = AtomicBool::new(false);
static SINK: OnceLock<Sink> = OnceLock::new();
/// A panic was already reported with a crash window (the process is closing).
static REPORTED: AtomicBool = AtomicBool::new(false);

struct Sink {
    app: String,
    /// The name shown to the user ("EvtApp"), see [`set_display_name`].
    display: Mutex<Option<String>>,
    /// Show an error dialog on panic (GUI applications).
    dialog: bool,
    /// The log file, opened on the first line written while no debugger is attached.
    file: Mutex<Option<File>>,
}

/// Turns diagnostics off for this process: a later [`install`] does nothing (the host's
/// automatic one included). For an application that routes its output itself.
pub fn disable() {
    DISABLED.store(true, Ordering::Relaxed);
}

/// Installs the diagnostics sink for the application `app` (the log file's name) — see the
/// module doc. Idempotent: the first call wins, later ones return `false`, as does a call after
/// [`disable`]. `dialog`: show an error dialog when the application panics.
pub fn install(app: &str, dialog: bool) -> bool {
    if DISABLED.load(Ordering::Relaxed) {
        return false;
    }
    let mut installed = false;
    SINK.get_or_init(|| {
        installed = true;
        Sink { app: sanitize(app), display: Mutex::new(None), dialog, file: Mutex::new(None) }
    });
    if !installed {
        return false;
    }
    let max = max_level();
    MAX_LEVEL.store(level_rank(max), Ordering::Relaxed);
    // Both fail harmlessly when the application already installed its own.
    let _ = tracing::subscriber::set_global_default(Subscriber { next_span: AtomicU64::new(1) });
    if log::set_logger(&LOGGER).is_ok() {
        log::set_max_level(log_filter(max));
    }
    std::panic::set_hook(Box::new(panic_hook));
    redirect_std_handles();
    true
}

/// The application's name as the user knows it ("EvtApp", "Kubuno Drive"), shown by the crash window
/// (the log file keeps the executable's name). The host sets it to its window title; the first call
/// wins. Without it, the executable's name is shown.
pub fn set_display_name(name: &str) {
    if let Some(sink) = SINK.get() {
        if let Ok(mut display) = sink.display.lock() {
            if display.is_none() && !name.trim().is_empty() {
                *display = Some(name.trim().to_string());
            }
        }
    }
}

/// The name of the running executable without its extension (`dsurfapp`), what the host
/// passes to [`install`].
pub fn exe_name() -> String {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.file_stem().map(|s| s.to_string_lossy().into_owned()))
        .unwrap_or_else(|| "kubuno-app".to_string())
}

/// `%LOCALAPPDATA%\Kubuno\logs\<app>.log` — where the lines go while no debugger is attached.
pub fn log_file_path(app: &str) -> Option<PathBuf> {
    let base = std::env::var_os("LOCALAPPDATA")?;
    Some(PathBuf::from(base).join("Kubuno").join("logs").join(format!("{}.log", sanitize(app))))
}

/// Writes one diagnostic line (`level` like `"INFO"`, `target` the module or source) to the
/// current destination — the debugger's output or the log file. Does nothing before [`install`].
pub fn write_line(level: &str, target: &str, message: &str) {
    let Some(sink) = SINK.get() else { return };
    let line = format!("{} [{level}] {target}: {message}\r\n", timestamp());
    if debugger_present() {
        output_debug_string(&line);
    } else {
        sink.write_file(&line);
    }
}

impl Sink {
    fn write_file(&self, line: &str) {
        let Ok(mut guard) = self.file.lock() else { return };
        if guard.is_none() {
            *guard = open_log(&self.app);
        }
        let too_big = match guard.as_mut() {
            Some(file) => {
                let _ = file.write_all(line.as_bytes());
                file.metadata().map(|m| m.len() > MAX_LOG_BYTES).unwrap_or(false)
            }
            None => false,
        };
        if too_big {
            *guard = None;
            if let Some(path) = log_file_path(&self.app) {
                let _ = std::fs::rename(&path, path.with_extension("1.log"));
            }
            *guard = open_log(&self.app);
        }
    }
}

fn open_log(app: &str) -> Option<File> {
    let path = log_file_path(app)?;
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if std::fs::metadata(&path).map(|m| m.len() > MAX_LOG_BYTES).unwrap_or(false) {
        let _ = std::fs::rename(&path, path.with_extension("1.log"));
    }
    OpenOptions::new().create(true).append(true).open(path).ok()
}

/// A file-name-safe version of `app`.
fn sanitize(app: &str) -> String {
    let s: String = app.chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '_' || c == '.' { c } else { '_' }).collect();
    if s.is_empty() {
        "kubuno-app".to_string()
    } else {
        s
    }
}

/// The level [`install`] starts with: `KUBUNO_LOG`, else `INFO` (debug builds included — the
/// per-event traces would flood the output otherwise).
fn max_level() -> tracing::Level {
    match std::env::var("KUBUNO_LOG").map(|v| v.to_ascii_lowercase()).as_deref() {
        Ok("error") => tracing::Level::ERROR,
        Ok("warn") => tracing::Level::WARN,
        Ok("info") => tracing::Level::INFO,
        Ok("debug") => tracing::Level::DEBUG,
        Ok("trace") => tracing::Level::TRACE,
        _ => tracing::Level::INFO,
    }
}

/// The current level, as [`level_rank`] (set by [`install`] and [`set_max_level`]).
static MAX_LEVEL: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(2);

fn level_rank(level: tracing::Level) -> u8 {
    match level {
        tracing::Level::ERROR => 0,
        tracing::Level::WARN => 1,
        tracing::Level::INFO => 2,
        tracing::Level::DEBUG => 3,
        _ => 4,
    }
}

fn rank_level(rank: u8) -> tracing::Level {
    match rank {
        0 => tracing::Level::ERROR,
        1 => tracing::Level::WARN,
        2 => tracing::Level::INFO,
        3 => tracing::Level::DEBUG,
        _ => tracing::Level::TRACE,
    }
}

fn log_filter(level: tracing::Level) -> log::LevelFilter {
    match level {
        tracing::Level::ERROR => log::LevelFilter::Error,
        tracing::Level::WARN => log::LevelFilter::Warn,
        tracing::Level::INFO => log::LevelFilter::Info,
        tracing::Level::DEBUG => log::LevelFilter::Debug,
        _ => log::LevelFilter::Trace,
    }
}

/// Changes the diagnostics level at run time (`DEBUG` shows every event the views dispatch) —
/// the programmatic twin of `KUBUNO_LOG`. Without effect when the application installed its own
/// `tracing`/`log` subscriber.
pub fn set_max_level(level: tracing::Level) {
    MAX_LEVEL.store(level_rank(level), Ordering::Relaxed);
    if SINK.get().is_some() {
        log::set_max_level(log_filter(level));
    }
    tracing::callsite::rebuild_interest_cache();
}

/// The current diagnostics level.
pub fn max_level_now() -> tracing::Level {
    rank_level(MAX_LEVEL.load(Ordering::Relaxed))
}

// ── tracing ──────────────────────────────────────────────────────────────────

/// A minimal `tracing` subscriber: every enabled event becomes one [`write_line`]. Spans are
/// accepted (ids handed out) but not recorded.
struct Subscriber {
    next_span: AtomicU64,
}

impl tracing::Subscriber for Subscriber {
    fn enabled(&self, metadata: &tracing::Metadata<'_>) -> bool {
        *metadata.level() <= max_level_now()
    }

    fn max_level_hint(&self) -> Option<tracing::level_filters::LevelFilter> {
        Some(tracing::level_filters::LevelFilter::from_level(max_level_now()))
    }

    fn new_span(&self, _span: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(self.next_span.fetch_add(1, Ordering::Relaxed).max(1))
    }

    fn record(&self, _span: &tracing::span::Id, _values: &tracing::span::Record<'_>) {}

    fn record_follows_from(&self, _span: &tracing::span::Id, _follows: &tracing::span::Id) {}

    fn event(&self, event: &tracing::Event<'_>) {
        let mut fields = Fields(String::new());
        event.record(&mut fields);
        let meta = event.metadata();
        write_line(meta.level().as_str(), meta.target(), fields.0.trim_start());
    }

    fn enter(&self, _span: &tracing::span::Id) {}

    fn exit(&self, _span: &tracing::span::Id) {}
}

/// Formats an event's fields: the `message` as is, the others as ` name=value`.
struct Fields(String);

impl tracing::field::Visit for Fields {
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "message" {
            let _ = write!(self.0, " {value}");
        } else {
            let _ = write!(self.0, " {}={value}", field.name());
        }
    }

    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            let _ = write!(self.0, " {value:?}");
        } else {
            let _ = write!(self.0, " {}={value:?}", field.name());
        }
    }
}

// ── log ──────────────────────────────────────────────────────────────────────

struct Logger;

static LOGGER: Logger = Logger;

impl log::Log for Logger {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.level() <= log::max_level()
    }

    fn log(&self, record: &log::Record<'_>) {
        if self.enabled(record.metadata()) {
            write_line(record.level().as_str(), record.target(), &record.args().to_string());
        }
    }

    fn flush(&self) {}
}

// ── panics ───────────────────────────────────────────────────────────────────

fn panic_hook(info: &std::panic::PanicHookInfo<'_>) {
    let message = if let Some(s) = info.payload().downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = info.payload().downcast_ref::<String>() {
        s.clone()
    } else {
        "(no message)".to_string()
    };
    let location = info.location().map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column())).unwrap_or_default();
    let thread = std::thread::current().name().unwrap_or("<unnamed>").to_string();
    let backtrace = std::backtrace::Backtrace::force_capture();
    write_line("PANIC", &thread, &format!("{message} at {location}\r\n{backtrace}"));
    // Only the UI thread's panic closes a GUI application (a background thread's panic is logged
    // and left to the code that joins it), and only once: a panic unwinding out of the window
    // procedure would abort with a second "panic in a function that cannot unwind".
    let Some(sink) = SINK.get().filter(|s| s.dialog) else { return };
    if thread != "main" || REPORTED.swap(true, Ordering::SeqCst) {
        return;
    }
    let display = sink.display.lock().ok().and_then(|d| d.clone()).unwrap_or_else(|| sink.app.clone());
    let log = if debugger_present() { None } else { log_file_path(&sink.app) };
    let details = format!("thread '{thread}' panicked at {location}:\r\n{message}\r\n\r\n{backtrace}");
    let report = super::crash::CrashReport { app: display, message, location, details, log };
    if debugger_present() {
        // Under a debugger, the debugger sees the panic first, like a .NET exception: the crash window
        // must not pre-empt the break (vskubuno docs/DEBUGGING.md). Inside the application's frame -
        // where its event handlers run - the panic is left to unwind: Visual Studio stops on the
        // `rust_panic` C++ exception at the line that panicked (Exception Settings > C++ Exceptions >
        // rust_panic), and if the developer continues, the host catches the unwind at the end of the
        // frame and shows the crash window then ([`run_frame`]). Anywhere else nothing could catch it
        // (the window procedure cannot unwind): break into the debugger here, then report.
        if IN_FRAME.with(|f| f.get()) {
            if let Ok(mut pending) = PENDING.lock() {
                *pending = Some(report);
            }
            return;
        }
        if std::env::var_os("KUBUNO_BREAK_ON_PANIC").is_none_or(|v| v != "0") {
            debug_break();
        }
    }
    report_and_exit(report);
}

/// Shows the crash window for `report` (a system message box when it cannot open), then closes
/// the process with exit code 101, before the panic unwinds into the window procedure.
fn report_and_exit(report: super::crash::CrashReport) -> ! {
    // The UI thread stops pumping messages while the crash window is up: keep its window as it
    // is (Kubuno caption included) instead of Windows' "not responding" ghost.
    // SAFETY: no arguments, no preconditions.
    unsafe { DisableProcessWindowsGhosting() };
    let (display, message, location) = (report.app.clone(), report.message.clone(), report.location.clone());
    let log = report.log.clone();
    if !super::crash::show(report) {
        show_panic_dialog(&display, &message, &location, log.map(|p| p.display().to_string()).as_deref());
    }
    std::process::exit(101);
}

thread_local! {
    /// The UI thread is inside [`run_frame`] (the application's frame, where handlers run).
    static IN_FRAME: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// The crash report of a panic left to unwind under a debugger (see [`panic_hook`]), shown by
/// [`run_frame`] once the unwind reaches it.
static PENDING: Mutex<Option<super::crash::CrashReport>> = Mutex::new(None);

/// Runs one frame of the application (`on_paint`: its view, its event handlers) on the UI thread.
/// A panic under a debugger unwinds to here once the developer continues past the debugger's break,
/// and the crash window is shown then; any other panic keeps unwinding as before (the hook has
/// already reported it, or the application asked for no dialog).
pub(crate) fn run_frame<R>(frame: impl FnOnce() -> R) -> R {
    let outer = IN_FRAME.with(|f| f.replace(true));
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(frame));
    IN_FRAME.with(|f| f.set(outer));
    match result {
        Ok(value) => value,
        Err(payload) => {
            let pending = PENDING.lock().ok().and_then(|mut p| p.take());
            match pending {
                Some(report) if !outer => report_and_exit(report),
                Some(report) => {
                    // A nested frame: let the outermost one report.
                    if let Ok(mut p) = PENDING.lock() {
                        *p = Some(report);
                    }
                    std::panic::resume_unwind(payload)
                }
                None => std::panic::resume_unwind(payload),
            }
        }
    }
}

/// Stops in the debugger when one is attached (`DebugBreak`), like .NET's `Debugger.Break()`; does
/// nothing otherwise, so it can stay in shipped code.
pub fn debug_break() {
    if debugger_present() {
        // SAFETY: no arguments; only raises a breakpoint exception the attached debugger handles.
        unsafe { DebugBreak() };
    }
}

/// Whether a debugger is attached to the process right now (`IsDebuggerPresent`).
pub fn is_debugger_attached() -> bool {
    debugger_present()
}

/// Names the process's main thread "Kubuno UI thread" for the debugger (`SetThreadDescription`,
/// what the Threads window and Parallel Stacks show); threads Rust started with a name already
/// carry it, and a host running on another thread leaves that thread's name alone.
pub(crate) fn name_ui_thread() {
    if std::thread::current().name() != Some("main") {
        return;
    }
    let name = wide("Kubuno UI thread");
    // SAFETY: the pseudo-handle of the current thread and a NUL-terminated UTF-16 buffer that
    // outlives the call; failure (older Windows) is harmless.
    unsafe { SetThreadDescription(GetCurrentThread(), name.as_ptr()) };
}

/// The fallback when the crash window cannot open (no Direct2D device...): a system message box.
fn show_panic_dialog(app: &str, message: &str, location: &str, log: Option<&str>) {
    let french = is_french_ui();
    let title = if french { format!("{app} - erreur") } else { format!("{app} - error") };
    let mut text = if french {
        format!("{app} a rencontré une erreur inattendue et doit se fermer.\n\n{message}\n({location})")
    } else {
        format!("{app} ran into an unexpected error and has to close.\n\n{message}\n({location})")
    };
    if let Some(log) = log {
        text.push_str(if french { "\n\nDétails : " } else { "\n\nDetails: " });
        text.push_str(log);
    }
    // On its own thread: the panicking (UI) thread must not pump messages meanwhile.
    let shown = std::thread::Builder::new().name("kubuno-panic-dialog".into()).spawn(move || {
        let title = wide(&title);
        let text = wide(&text);
        // SAFETY: both strings are NUL-terminated UTF-16 buffers that outlive the call; no owner.
        unsafe { MessageBoxW(0, text.as_ptr(), title.as_ptr(), MB_OK | MB_ICONERROR | MB_TASKMODAL | MB_SETFOREGROUND) };
    });
    if let Ok(handle) = shown {
        let _ = handle.join();
    }
}

// ── println!/eprintln! without a console ─────────────────────────────────────

/// When the process has no standard output or error (a GUI exe started by Explorer or the
/// debugger), points both at a pipe whose lines a background thread forwards to [`write_line`].
fn redirect_std_handles() {
    // SAFETY: plain Win32 calls on handles owned by this process; the pipe's handles are kept
    // for the process's lifetime (the write end becomes stdout/stderr, the read end is owned by
    // the reader thread's `File`).
    unsafe {
        let has = |id: u32| {
            let h = GetStdHandle(id);
            h != 0 && h != INVALID_HANDLE_VALUE
        };
        if has(STD_OUTPUT_HANDLE) || has(STD_ERROR_HANDLE) {
            return;
        }
        let (mut read, mut write) = (0isize, 0isize);
        if CreatePipe(&mut read, &mut write, std::ptr::null(), 0) == 0 {
            return;
        }
        SetStdHandle(STD_OUTPUT_HANDLE, write);
        SetStdHandle(STD_ERROR_HANDLE, write);
        use std::os::windows::io::FromRawHandle;
        let reader = File::from_raw_handle(read as *mut std::ffi::c_void);
        let _ = std::thread::Builder::new().name("kubuno-stdout".into()).spawn(move || {
            for line in BufReader::new(reader).lines() {
                match line {
                    Ok(line) => write_line("STDOUT", "println", line.trim_end()),
                    Err(_) => break,
                }
            }
        });
    }
}

// ── Win32 ────────────────────────────────────────────────────────────────────
//
// Declared directly (raw-dylib), like `input.rs`' clipboard, so the crate's `windows` feature
// set — and with it the `windows` crate every Kubuno program compiles — is unchanged.

const STD_OUTPUT_HANDLE: u32 = -11i32 as u32;
const STD_ERROR_HANDLE: u32 = -12i32 as u32;
const INVALID_HANDLE_VALUE: isize = -1;
const MB_OK: u32 = 0x0000_0000;
const MB_ICONERROR: u32 = 0x0000_0010;
const MB_TASKMODAL: u32 = 0x0000_2000;
const MB_SETFOREGROUND: u32 = 0x0001_0000;

#[repr(C)]
struct SystemTime {
    year: u16,
    month: u16,
    day_of_week: u16,
    day: u16,
    hour: u16,
    minute: u16,
    second: u16,
    milliseconds: u16,
}

#[link(name = "kernel32", kind = "raw-dylib")]
extern "system" {
    fn IsDebuggerPresent() -> i32;
    fn DebugBreak();
    fn GetCurrentThread() -> isize;
    fn SetThreadDescription(thread: isize, description: *const u16) -> i32;
    fn OutputDebugStringW(text: *const u16);
    fn GetStdHandle(id: u32) -> isize;
    fn SetStdHandle(id: u32, handle: isize) -> i32;
    fn CreatePipe(read: *mut isize, write: *mut isize, attributes: *const std::ffi::c_void, size: u32) -> i32;
    fn GetLocalTime(time: *mut SystemTime);
    fn GetUserDefaultUILanguage() -> u16;
}

#[link(name = "user32", kind = "raw-dylib")]
extern "system" {
    fn MessageBoxW(owner: isize, text: *const u16, caption: *const u16, kind: u32) -> i32;
    fn DisableProcessWindowsGhosting();
}

fn debugger_present() -> bool {
    // SAFETY: no arguments, no preconditions.
    unsafe { IsDebuggerPresent() != 0 }
}

fn output_debug_string(line: &str) {
    let text = wide(line);
    // SAFETY: `text` is a NUL-terminated UTF-16 buffer alive for the call.
    unsafe { OutputDebugStringW(text.as_ptr()) };
}

pub(crate) fn is_french_ui() -> bool {
    // SAFETY: no arguments, no preconditions. The primary language is the low 10 bits.
    (unsafe { GetUserDefaultUILanguage() } & 0x3ff) == 0x0c
}

fn timestamp() -> String {
    let mut t = SystemTime { year: 0, month: 0, day_of_week: 0, day: 0, hour: 0, minute: 0, second: 0, milliseconds: 0 };
    // SAFETY: `t` is a valid, writable SYSTEMTIME-layout struct.
    unsafe { GetLocalTime(&mut t) };
    format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:03}", t.year, t.month, t.day, t.hour, t.minute, t.second, t.milliseconds)
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_file_safe_and_the_log_lives_under_local_app_data() {
        assert_eq!(sanitize("My App/1"), "My_App_1");
        assert_eq!(sanitize(""), "kubuno-app");
        if let Some(path) = log_file_path("dsurfapp") {
            assert!(path.ends_with("Kubuno\\logs\\dsurfapp.log"), "{}", path.display());
        }
    }

    #[test]
    fn event_fields_are_formatted_message_first() {
        use tracing::field::Visit;
        let mut f = Fields(String::new());
        let meta_fields = tracing::field::FieldSet::new(&["message", "count"], tracing::callsite::Identifier(&CALLSITE));
        let message = meta_fields.field("message").expect("message");
        let count = meta_fields.field("count").expect("count");
        f.record_str(&message, "hello");
        f.record_debug(&count, &3);
        assert_eq!(f.0.trim_start(), "hello count=3");
    }

    struct Callsite;
    static CALLSITE: Callsite = Callsite;
    impl tracing::callsite::Callsite for Callsite {
        fn set_interest(&self, _: tracing::subscriber::Interest) {}
        fn metadata(&self) -> &tracing::Metadata<'_> {
            unimplemented!("not used by the test")
        }
    }

    #[test]
    fn a_timestamp_is_local_date_and_time() {
        let t = timestamp();
        assert_eq!(t.len(), "2026-09-29 09:40:01.123".len(), "{t}");
    }
}
