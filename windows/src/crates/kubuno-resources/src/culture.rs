//! The process-wide UI culture (.NET's `CultureInfo.CurrentUICulture`): the culture resources are
//! looked up in. It follows Windows' display language until the application sets one, and every
//! change is announced to the subscribers (the view runtime repaints its windows), so a running
//! application switches language live.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};

static EXPLICIT: RwLock<Option<String>> = RwLock::new(None);
static SYSTEM: RwLock<Option<String>> = RwLock::new(None);
static GENERATION: AtomicU64 = AtomicU64::new(1);

type Callback = Arc<dyn Fn(&str) + Send + Sync>;
static SUBSCRIBERS: Mutex<Vec<(u64, Callback)>> = Mutex::new(Vec::new());
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// The current UI culture (`fr-FR`, `en-US`…): the one set by [`set_culture`], else Windows'
/// display language.
pub fn culture() -> String {
    if let Some(c) = EXPLICIT.read().ok().and_then(|g| g.clone()) {
        return c;
    }
    system_culture()
}

/// Windows' display language (the first of the user's preferred UI languages), `en-US` when it
/// cannot be read. Read once, then cached.
pub fn system_culture() -> String {
    if let Some(c) = SYSTEM.read().ok().and_then(|g| g.clone()) {
        return c;
    }
    let c = read_system_culture().unwrap_or_else(|| "en-US".to_string());
    if let Ok(mut g) = SYSTEM.write() {
        *g = Some(c.clone());
    }
    c
}

/// Sets the UI culture of the process (`"fr"`, `"de-DE"`; `""` or `"invariant"` shows the neutral
/// values) and notifies the subscribers when it changed. Views bound with `{Res …}` repaint.
pub fn set_culture(tag: &str) {
    let tag = if tag.trim().is_empty() || tag.eq_ignore_ascii_case("invariant") { "invariant".to_string() } else { kubuno_resources_model::culture::canonical(tag) };
    let changed = match EXPLICIT.write() {
        Ok(mut g) => {
            let before = g.clone().unwrap_or_else(system_culture);
            *g = Some(tag.clone());
            before != tag
        }
        Err(_) => false,
    };
    if changed {
        notify(&tag);
    }
}

/// Goes back to following Windows' display language.
pub fn follow_system_culture() {
    let before = culture();
    if let Ok(mut g) = EXPLICIT.write() {
        *g = None;
    }
    let now = culture();
    if before != now {
        notify(&now);
    }
}

/// A number that changes whenever what `{Res …}` resolves to may have changed (a culture switch, a
/// resource set registered or replaced): caches keyed by it stay correct.
pub fn generation() -> u64 {
    GENERATION.load(Ordering::Acquire)
}

pub(crate) fn bump() {
    GENERATION.fetch_add(1, Ordering::AcqRel);
}

/// A subscription made by [`on_culture_changed`]; [`Subscription::cancel`] (or dropping it after
/// [`Subscription::forget`] was not called) ends it.
#[must_use = "dropping the subscription cancels it; call `forget` to keep it for the process's life"]
pub struct Subscription(u64);

impl Subscription {
    /// Ends the subscription.
    pub fn cancel(self) {}

    /// Keeps the subscription for the rest of the process.
    pub fn forget(self) {
        std::mem::forget(self);
    }
}

impl Drop for Subscription {
    fn drop(&mut self) {
        if let Ok(mut subs) = SUBSCRIBERS.lock() {
            subs.retain(|(id, _)| *id != self.0);
        }
    }
}

/// Calls `f` with the new culture after every culture change (on the thread that changed it).
pub fn on_culture_changed(f: impl Fn(&str) + Send + Sync + 'static) -> Subscription {
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    if let Ok(mut subs) = SUBSCRIBERS.lock() {
        subs.push((id, Arc::new(f)));
    }
    Subscription(id)
}

pub(crate) fn notify(culture: &str) {
    bump();
    // Called outside the lock, so a subscriber may subscribe or change the culture again.
    let subs: Vec<Callback> = SUBSCRIBERS.lock().map(|s| s.iter().map(|(_, f)| f.clone()).collect()).unwrap_or_default();
    for f in subs {
        f(culture);
    }
}

fn read_system_culture() -> Option<String> {
    use windows::Win32::Globalization::{GetUserPreferredUILanguages, MUI_LANGUAGE_NAME};
    let mut count = 0u32;
    let mut len = 0u32;
    // SAFETY: the first call only reads the sizes; the second fills a buffer of exactly `len` u16s.
    unsafe {
        GetUserPreferredUILanguages(MUI_LANGUAGE_NAME, &mut count, None, &mut len).ok()?;
        let mut buf = vec![0u16; len as usize];
        GetUserPreferredUILanguages(MUI_LANGUAGE_NAME, &mut count, Some(windows::core::PWSTR(buf.as_mut_ptr())), &mut len).ok()?;
        let first: Vec<u16> = buf.iter().copied().take_while(|&c| c != 0).collect();
        let s = String::from_utf16(&first).ok()?;
        (!s.is_empty()).then(|| kubuno_resources_model::culture::canonical(&s))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_system_culture() {
        let c = system_culture();
        assert!(kubuno_resources_model::culture::is_culture(&c), "{c}");
    }
}
