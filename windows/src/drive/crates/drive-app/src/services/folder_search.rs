//! Port of the recursive Win32 fallback from `Files.App/Utils/Storage/Search/
//! FolderSearch.cs` — specifically `SearchWithWin32Async` (NOT the AQS
//! indexer branch `ToQueryOptions`, nor tags, nor resolved shortcuts).
//!
//! This is the engine that works everywhere: a recursive `FindFirstFileExW` at
//! unlimited depth on `{folder}\*{pattern}` to find
//! matches, plus a second pass `{folder}\*` to descend into
//! subfolders. The threading mirrors `size_provider.rs`: a background
//! worker, results accumulated in a shared `Mutex`, and a
//! `PostMessageW(WM_APP_SEARCH_RESULT)` to wake the UI thread in batches.
//!
//! Cooperative cancellation via "generation" (the equivalent of the C#'s
//! `CancellationToken`): each search carries a number; starting
//! a new search replaces the current generation, so the
//! previous worker, seeing its generation become stale, stops on its own.

use std::ffi::c_void;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

use windows::core::PCWSTR;
use windows::Win32::Foundation::{FILETIME, HWND, INVALID_HANDLE_VALUE, LPARAM, WPARAM};
use windows::Win32::Storage::FileSystem::{
    FindClose, FindExInfoBasic, FindExSearchNameMatch, FindFirstFileExW, FindNextFileW,
    FIND_FIRST_EX_LARGE_FETCH, WIN32_FIND_DATAW,
};
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;

use crate::data::items::DirEntryItem;

/// A batch of search results is ready to be drained (WM_APP registry).
///
/// # Value uniqueness
/// This port's application messages are numbered `WM_APP (0x8000) + n`,
/// each unique (a `0x8000+3` collision once broke thumbnails, see
/// `utils/thumbnails.rs`). Registry as of 2026-07-23:
/// - `+1` `WM_APP_DIR_CHANGED`   (utils/folder_watcher.rs)
/// - `+2` `WM_APP_OPS_PROGRESS`  (utils/storage.rs)
/// - `+3` `WM_APP_SHELL_MENU`    (main_window.rs)
/// - `+4` `WM_APP_GIT_DONE`      (main_window.rs)
/// - `+5` `WM_APP_TAB_PREVIEW`   (main_window.rs)
/// - `+6` `WM_APP_ICON_READY`    (utils/thumbnails.rs)
/// - `+7` `WM_APP_FOLDER_SIZE`   (services/size_provider.rs)
/// - `+8` `WM_APP_SEARCH_RESULT` (THIS module) ← first free value.
///
/// `WPARAM` carries the search generation; `LPARAM` is 1 on the
/// final message (search complete), 0 otherwise.
pub const WM_APP_SEARCH_RESULT: u32 = 0x8000 + 8;

/// The search request: the typed text and the root folder to explore.
/// `max_results` caps the number of results (`FolderSearch.MaxItemCount`,
/// `0`/`None` = no limit).
#[derive(Debug, Clone)]
pub struct SearchQuery {
    pub text: String,
    pub root: String,
    pub max_results: Option<usize>,
}

impl SearchQuery {
    /// Search with no result limit (the full-screen tab case;
    /// `MaxItemCount = 0` in the C#).
    pub fn new(text: impl Into<String>, root: impl Into<String>) -> Self {
        Self { text: text.into(), root: root.into(), max_results: None }
    }
}

/// `FolderSearch.QueryWithWildcard` — turns the typed text into a
/// `FindFirstFile` pattern. If the text contains a `.`, the extension is
/// isolated ("name.ext" → "name*.ext*"); otherwise it's simply suffixed ("name*").
/// The pattern is then prefixed with a `*` by the caller (`{folder}\*{pattern}`),
/// so "abc" searches for `*abc*` and "name.ext" searches for `*name*.ext*`, like
/// the original. Case is ignored by `FindFirstFile` itself (NTFS).
pub fn query_with_wildcard(query: &str) -> String {
    // Cf. C#: `if (!string.IsNullOrEmpty(Query) && Query.Contains('.'))`.
    if !query.is_empty() && query.contains('.') {
        // `var split = Query.Split('.');`
        // `var leading = string.Join('.', split.SkipLast(1));`
        // `var query = $"{leading}*.{split.Last()}";`  then  `return $"{query}*";`
        let parts: Vec<&str> = query.split('.').collect();
        let leading = parts[..parts.len() - 1].join(".");
        let last = parts[parts.len() - 1];
        format!("{leading}*.{last}*")
    } else {
        format!("{query}*")
    }
}

// ─────────────────────────── engine (shared state) ───────────────────────────

static ENGINE: OnceLock<Engine> = OnceLock::new();

struct Engine {
    /// The current search generation. Each started search overwrites
    /// this value; a worker whose generation no longer matches stops
    /// (the C#'s `token.IsCancellationRequested`).
    current: AtomicU64,
    /// Results accumulated for the current generation, drained in batches by the
    /// UI thread (like `SizeProvider.results`).
    batch: Mutex<Batch>,
}

#[derive(Default)]
struct Batch {
    generation: u64,
    items: Vec<DirEntryItem>,
}

fn engine() -> &'static Engine {
    ENGINE.get_or_init(|| Engine {
        current: AtomicU64::new(0),
        batch: Mutex::new(Batch::default()),
    })
}

/// Starts the `query` search in a worker. `generation` identifies this
/// search (the caller keeps a monotonic counter): declaring it current
/// cancels any older worker. Batches are posted to `hwnd_raw` via
/// `WM_APP_SEARCH_RESULT`.
pub fn start_search(query: SearchQuery, hwnd_raw: isize, generation: u64) {
    let engine = engine();
    engine.current.store(generation, Ordering::SeqCst);
    {
        let mut batch = engine.batch.lock().unwrap();
        batch.generation = generation;
        batch.items.clear();
    }
    std::thread::spawn(move || run_search(&query, hwnd_raw, generation));
}

/// Drains the results accumulated for `generation` (empty if a more recent
/// search has taken over). Called on every `WM_APP_SEARCH_RESULT` to
/// enrich the "search results" tab, just like the C#'s `SearchTick`
/// refreshes its bound list.
pub fn take_results(generation: u64) -> Vec<DirEntryItem> {
    let mut batch = engine().batch.lock().unwrap();
    if batch.generation != generation {
        return Vec::new();
    }
    std::mem::take(&mut batch.items)
}

/// Cancels the current search WITHOUT starting a new one (the tab leaves
/// search mode). Bumps the current generation: any active worker sees it
/// go stale and stops, and the undrained leftovers are discarded.
pub fn cancel() {
    let engine = engine();
    engine.current.fetch_add(1, Ordering::SeqCst);
    engine.batch.lock().unwrap().items.clear();
}

/// True if `generation` is no longer the current search.
fn is_cancelled(generation: u64) -> bool {
    engine().current.load(Ordering::SeqCst) != generation
}

// ─────────────────────────── worker (Win32 walk) ───────────────────────────

/// Display settings frozen at launch (the C# re-reads them for each item;
/// here we avoid a full `AppSettings` clone per entry).
struct Filter {
    show_hidden: bool,
    show_system: bool,
    show_dot: bool,
}

impl Filter {
    /// C#'s `shouldBeListed` (`hiddenOnly == false` branch):
    /// `(!isHidden || (ShowHiddenItems && (!isSystem || ShowProtectedSystemFiles)))
    ///  && (!startWithDot || ShowDotFiles)`.
    fn should_list(&self, attrs: u32, name: &str) -> bool {
        const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
        const FILE_ATTRIBUTE_SYSTEM: u32 = 0x4;
        let is_hidden = attrs & FILE_ATTRIBUTE_HIDDEN != 0;
        let is_system = attrs & FILE_ATTRIBUTE_SYSTEM != 0;
        let starts_with_dot = name.starts_with('.');
        (!is_hidden || (self.show_hidden && (!is_system || self.show_system)))
            && (!starts_with_dot || self.show_dot)
    }
}

fn run_search(query: &SearchQuery, hwnd_raw: isize, generation: u64) {
    const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x10;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;

    let settings = crate::services::settings::get();
    let filter = Filter {
        show_hidden: settings.show_hidden_items,
        show_system: settings.show_protected_system_files,
        show_dot: settings.show_dot_files,
    };
    let wildcard = query_with_wildcard(&query.text);
    let max = query.max_results;

    // Total counter (the C#'s `results.Count`: the 32 and 300 thresholds
    // apply to the cumulative total, not to the current batch which may have been drained).
    let mut total = 0usize;

    // Iterative DFS: a stack of folders, rather than the C#'s recursion, to
    // avoid overflowing the stack on a deep tree. Unlimited depth,
    // like the original.
    let mut stack: Vec<String> = vec![query.root.clone()];

    while let Some(folder) = stack.pop() {
        if is_cancelled(generation) {
            return;
        }
        if max.is_some_and(|m| total >= m) {
            break;
        }

        // ── Pass 1: matches within `folder` (`{folder}\*{wildcard}`).
        let pattern = join(&folder, &format!("*{wildcard}"));
        for_each_find(&pattern, |find| {
            if is_cancelled(generation) || max.is_some_and(|m| total >= m) {
                return false; // stop
            }
            let name = file_name(find);
            if name == "." || name == ".." {
                return true;
            }
            if filter.should_list(find.dwFileAttributes, &name) {
                let item = make_item(&folder, &name, find);
                push(generation, item);
                total += 1;
                // `if (results.Count == 32 || results.Count % 300 == 0)` → SearchTick.
                if total == 32 || total.is_multiple_of(300) {
                    post(hwnd_raw, generation, false);
                }
            }
            true // continue
        });

        if is_cancelled(generation) {
            return;
        }
        if max.is_some_and(|m| total >= m) {
            break;
        }

        // ── Pass 2: subfolders to explore (`{folder}\*`).
        let all = join(&folder, "*");
        let mut subdirs: Vec<String> = Vec::new();
        for_each_find(&all, |find| {
            if is_cancelled(generation) {
                return false;
            }
            let is_dir = find.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY != 0;
            // Junctions / links ignored, like `size_provider` (avoids
            // cycles and double counting).
            let is_reparse = find.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0;
            if is_dir && !is_reparse {
                let name = file_name(find);
                if name != "." && name != ".." {
                    subdirs.push(join(&folder, &name));
                }
            }
            true
        });
        // Push in reverse order so exploration follows enumeration order.
        for sub in subdirs.into_iter().rev() {
            stack.push(sub);
        }
    }

    // Final message: the UI thread drains the leftovers and knows it's done.
    post(hwnd_raw, generation, true);
}

/// Adds a result to the shared batch (respecting the generation: a stale
/// batch is ignored).
fn push(generation: u64, item: DirEntryItem) {
    let mut batch = engine().batch.lock().unwrap();
    if batch.generation == generation {
        batch.items.push(item);
    }
}

/// Wakes the UI thread (`SearchTick`). `final_msg` → `LPARAM(1)`.
fn post(hwnd_raw: isize, generation: u64, final_msg: bool) {
    unsafe {
        let _ = PostMessageW(
            Some(HWND(hwnd_raw as *mut _)),
            WM_APP_SEARCH_RESULT,
            WPARAM(generation as usize),
            LPARAM(final_msg as isize),
        );
    }
}

/// Enumerates a `FindFirstFileExW` pattern (FindExInfoBasic / FindExSearchNameMatch
/// / FIND_FIRST_EX_LARGE_FETCH, exactly the C#'s call) and applies `f` to
/// each entry. `f` returns `false` to stop early.
fn for_each_find(pattern: &str, mut f: impl FnMut(&WIN32_FIND_DATAW) -> bool) {
    let wide: Vec<u16> = pattern.encode_utf16().chain(std::iter::once(0)).collect();
    let mut data = WIN32_FIND_DATAW::default();
    let handle = unsafe {
        FindFirstFileExW(
            PCWSTR(wide.as_ptr()),
            FindExInfoBasic,
            &mut data as *mut _ as *mut c_void,
            FindExSearchNameMatch,
            None,
            FIND_FIRST_EX_LARGE_FETCH,
        )
    };
    let Ok(handle) = handle else { return };
    if handle == INVALID_HANDLE_VALUE {
        return;
    }
    loop {
        if !f(&data) {
            break;
        }
        if unsafe { FindNextFileW(handle, &mut data) }.is_err() {
            break; // ERROR_NO_MORE_FILES
        }
    }
    unsafe {
        let _ = FindClose(handle);
    }
}

/// The `WIN32_FIND_DATAW` struct's `cFileName` as a `String` (up to the NUL).
fn file_name(find: &WIN32_FIND_DATAW) -> String {
    let len = find.cFileName.iter().position(|&c| c == 0).unwrap_or(find.cFileName.len());
    String::from_utf16_lossy(&find.cFileName[..len])
}

/// Builds a `DirEntryItem` from the `FindFirstFile` data (the C#'s
/// `GetListedItemAsync(itemPath, findData)`, without icons or shortcuts).
fn make_item(folder: &str, name: &str, find: &WIN32_FIND_DATAW) -> DirEntryItem {
    const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x10;
    let is_dir = find.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY != 0;
    let size = ((find.nFileSizeHigh as u64) << 32) | find.nFileSizeLow as u64;
    DirEntryItem {
        name: name.to_string(),
        path: join(folder, name),
        is_dir,
        size,
        // Like `load_directory`: size known for a file, to be computed
        // for a folder (via the `SizeProvider`).
        size_known: !is_dir,
        modified: Some(filetime_to_systemtime(find.ftLastWriteTime)),
        original_path: None,
    }
}

/// `Path.Combine(folder, tail)`: avoids a double separator when `folder`
/// already ends with `\` (root "C:\").
fn join(folder: &str, tail: &str) -> String {
    if folder.ends_with('\\') || folder.ends_with('/') {
        format!("{folder}{tail}")
    } else {
        format!("{folder}\\{tail}")
    }
}

/// `FILETIME` (100 ns since 1601) → `SystemTime` (Unix epoch). Copied from
/// `utils/recycle_bin.rs::filetime_to_system`.
fn filetime_to_systemtime(ft: FILETIME) -> std::time::SystemTime {
    let intervals = ((ft.dwHighDateTime as u64) << 32) | ft.dwLowDateTime as u64;
    let unix_ns = intervals.saturating_sub(116_444_736_000_000_000) * 100;
    std::time::UNIX_EPOCH + std::time::Duration::from_nanos(unix_ns)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wildcard_sans_point() {
        // "abc" → "abc*" (prefixed "*abc*" when calling FindFirstFile).
        assert_eq!(query_with_wildcard("abc"), "abc*");
    }

    #[test]
    fn wildcard_avec_extension() {
        // "readme.md" → "readme*.md*".
        assert_eq!(query_with_wildcard("readme.md"), "readme*.md*");
        // Several dots: only the last segment is the extension.
        assert_eq!(query_with_wildcard("archive.tar.gz"), "archive.tar*.gz*");
    }

    #[test]
    fn wildcard_vide() {
        assert_eq!(query_with_wildcard(""), "*");
    }

    #[test]
    fn join_gere_la_racine() {
        assert_eq!(join("C:\\", "*"), "C:\\*");
        assert_eq!(join("C:\\Users", "*"), "C:\\Users\\*");
    }
}
