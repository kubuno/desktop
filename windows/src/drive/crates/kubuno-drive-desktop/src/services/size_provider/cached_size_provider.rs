//! CachedSizeProvider (mirror of `CachedSizeProvider.cs`).
//!
//! The single cache of computed sizes (`sizes`), the `TryGetSize`
//! (`get`) / `drain` / `ClearAsync` (`clear`) accessors, and the recursive
//! `Calculate` (`walk`) which ignores reparse points and caches subfolders
//! close to the root (≤ 3 levels).

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use super::size_changed_event_args::SizeResult;
use super::user_size_provider::push;

#[derive(Default)]
pub struct SizeProvider {
    /// `CachedSizeProvider.sizes` (a single cache: paths are absolute,
    /// `DrivesSizeProvider`'s per-drive split is unnecessary here).
    pub(super) cache: HashMap<String, u64>,
    /// Folders already submitted to the worker (avoids re-requests per frame).
    pub(super) pending: HashSet<String>,
    pub(super) results: Arc<Mutex<Vec<SizeResult>>>,
    pub(super) worker: Option<std::sync::mpsc::Sender<String>>,
}

impl SizeProvider {
    /// `TryGetSize`.
    pub fn get(&self, path: &str) -> Option<u64> {
        self.cache.get(path).copied()
    }

    /// Drains the worker's results into the cache and returns them to the caller
    /// (which updates the displayed entries).
    pub fn drain(&mut self) -> Vec<SizeResult> {
        let results: Vec<SizeResult> = std::mem::take(&mut *self.results.lock().unwrap());
        for (path, size, done) in &results {
            self.cache.insert(path.clone(), *size);
            if *done {
                self.pending.remove(path);
            }
        }
        results
    }

    /// `ClearAsync` — when the setting is toggled.
    pub fn clear(&mut self) {
        self.cache.clear();
        self.pending.clear();
        self.results.lock().unwrap().clear();
    }
}

pub(super) struct WalkContext<'a> {
    pub(super) results: &'a Arc<Mutex<Vec<SizeResult>>>,
    pub(super) hwnd_raw: isize,
    pub(super) last_post: std::time::Instant,
    pub(super) root: String,
}

/// `CachedSizeProvider`'s recursive `Calculate`.
pub(super) fn walk(path: &str, level: u32, ctx: &mut WalkContext) -> u64 {
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    use std::os::windows::fs::MetadataExt;

    let Ok(read) = std::fs::read_dir(path) else { return 0 };
    let mut size = 0u64;
    for entry in read.flatten() {
        let Ok(meta) = entry.metadata() else { continue };
        // Symbolic links and junctions ignored, like the original.
        if meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            continue;
        }
        if meta.is_dir() {
            let child = entry.path().to_string_lossy().into_owned();
            let child_size = walk(&child, level + 1, ctx);
            size += child_size;
            // Subfolders close to the root enrich the cache.
            if level <= 3 {
                push(ctx.results, ctx.hwnd_raw, vec![(child, child_size, true)]);
            }
        } else {
            size += meta.file_size();
        }
        // Intermediate result for the top-level folder, at most once per 500 ms.
        if level == 0 && ctx.last_post.elapsed().as_millis() > 500 {
            ctx.last_post = std::time::Instant::now();
            push(ctx.results, ctx.hwnd_raw, vec![(ctx.root.clone(), size, false)]);
        }
    }
    size
}
