//! IconCacheService (mirror of IconCacheService.cs)
//!
//! Port of `Files.App/Services/Storage/IconCacheService.cs`: cache of real
//! shell icons (IShellItemImageFactory) as Direct2D bitmaps. The public API
//! is exposed via `crate::services::storage::IconCache`.

use std::collections::HashMap;

use windows::Win32::Graphics::Direct2D::{ID2D1Bitmap1, ID2D1DeviceContext};

use crate::utils::thumbnails::file_thumbnail_helper::{fetch_icon, spawn_worker, upload};

pub(crate) type Key = (String, i32, bool);
/// Decoded pixels handed from the thumbnail worker to the UI thread.
pub(crate) type ThumbResults = std::sync::Arc<std::sync::Mutex<Vec<(Key, Option<kubuno_drive_desktop_app_storage::ShellBitmap>)>>>;

#[derive(Default)]
pub struct IconCache {
    map: HashMap<Key, Option<ID2D1Bitmap1>>,
    /// Thumbnail requests already queued to the worker.
    pending: std::collections::HashSet<Key>,
    /// Decoded pixels delivered by the worker, awaiting D2D upload.
    results: ThumbResults,
    worker: Option<std::sync::mpsc::Sender<Key>>,
}

impl IconCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// Drops all cached bitmaps (device loss).
    pub fn clear(&mut self) {
        self.map.clear();
    }

    pub fn get(&self, path: &str, size_px: i32) -> Option<&ID2D1Bitmap1> {
        self.map
            .get(&(path.to_string(), size_px, false))
            .and_then(|b| b.as_ref())
    }

    /// Content thumbnail (image/PDF preview) with icon fallback.
    pub fn get_thumbnail(&self, path: &str, size_px: i32) -> Option<&ID2D1Bitmap1> {
        self.map
            .get(&(path.to_string(), size_px, true))
            .and_then(|b| b.as_ref())
            .or_else(|| self.get(path, size_px))
    }

    /// Fetches and caches the shell icon for `path` if not already present.
    /// Failures are cached as `None` so they are not retried every frame.
    pub fn ensure(&mut self, ctx: &ID2D1DeviceContext, path: &str, size_px: i32) {
        self.ensure_kind(ctx, path, size_px, false);
    }

    /// Preloads an `imageres.dll` icon by index (`Constants.ImageRes`:
    /// the sidebar's Drives/Network sections). Key "imageres:{index}".
    pub fn ensure_imageres(&mut self, ctx: &ID2D1DeviceContext, index: i32, size_px: i32) {
        let key = (format!("imageres:{index}"), size_px, false);
        self.map.entry(key).or_insert_with(|| {
            crate::utils::shell::imageres_icon(index, size_px).and_then(|b| upload(ctx, &b))
        });
    }

    /// Same, but preferring the content thumbnail — fetched ASYNCHRONOUSLY on
    /// a dedicated STA worker (thumbnail providers can take seconds; the C#
    /// original also loads them off the UI thread).
    pub fn ensure_thumbnail(&mut self, ctx: &ID2D1DeviceContext, path: &str, size_px: i32, hwnd_raw: isize) {
        // Fast path: the plain icon shows instantly while the thumbnail loads.
        self.ensure_kind(ctx, path, size_px, false);

        let key = (path.to_string(), size_px, true);
        if self.map.contains_key(&key) || self.pending.contains(&key) {
            return;
        }
        self.pending.insert(key.clone());
        let sender = self.worker.get_or_insert_with(|| spawn_worker(std::sync::Arc::clone(&self.results), hwnd_raw));
        if sender.send(key.clone()).is_err() {
            // Worker died: retry with a fresh one next frame.
            self.worker = None;
            self.pending.remove(&key);
        }
    }

    /// Uploads worker results into D2D bitmaps (call on WM_APP_ICON_READY).
    pub fn drain_results(&mut self, ctx: &ID2D1DeviceContext) {
        let results: Vec<_> = std::mem::take(&mut *self.results.lock().unwrap());
        for (key, shell_bitmap) in results {
            self.pending.remove(&key);
            let bitmap = shell_bitmap.and_then(|b| upload(ctx, &b));
            self.map.insert(key, bitmap);
        }
    }

    fn ensure_kind(&mut self, ctx: &ID2D1DeviceContext, path: &str, size_px: i32, thumbnail: bool) {
        let key = (path.to_string(), size_px, thumbnail);
        if self.map.contains_key(&key) {
            return;
        }
        let bitmap = fetch_icon(ctx, path, size_px, thumbnail);
        self.map.insert(key, bitmap);
    }
}
