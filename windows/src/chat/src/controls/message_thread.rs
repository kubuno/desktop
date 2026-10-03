//! `MessageThread` — the custom control that shows a conversation's messages.
//!
//! Variable-height bubbles (own messages on the right in the accent colour, the other side's
//! on the left on a card, the web's `MessageBubble`), date separators, delivery ticks
//! (✓ sent, ✓✓ delivered/read), virtualised painting (only the bubbles in view are drawn),
//! the wheel and a scroll bar, and "stick to the bottom": a thread opens on its newest
//! message and follows new ones while the reader is at the bottom.
//!
//! The text is wrapped on words with DirectWrite measurements of each candidate line
//! (`kubuno_desktop_ui::display::wrap_lines`) and painted line by line, so what is laid out is
//! exactly what is drawn — the former window estimated the line count from one long line.
//! The geometry is the pure function [`layout`], unit-tested with a fake measure.
//!
//! The form feeds it through typed access (`#[control] thread: Custom<MessageThread>`,
//! `self.thread.with(|t| t.set_thread(data))`), or a binding of `Messages` to a
//! `Shared<ThreadData>`. In the designer it shows sample bubbles.

use kubuno_desktop::prelude::*;
use kubuno_desktop::ui::graphics::Color;
use kubuno_desktop::ui::{Canvas, Rect, Size, Widget, WidgetState};
use kubuno_desktop::views::component::{Component, Control, ControlCore, EventCx, PaintEventCx};
use kubuno_desktop::views::events::Event;
use kubuno_desktop::views::events::EmptyEventArgs;

use crate::model::{Message, Status};

// ── Geometry (DIP), the former window's numbers ─────────────────────────────────────────────

/// Side margin of the bubbles.
pub const PAD: f32 = 12.0;
/// Space above the first item and below the last.
pub const TOP: f32 = 10.0;
/// A bubble's horizontal and vertical inner padding.
pub const BUBBLE_PAD_X: f32 = 12.0;
pub const BUBBLE_PAD_Y: f32 = 8.0;
/// One wrapped line of text.
pub const LINE_H: f32 = 20.0;
/// The meta line (time, ticks) under the text.
pub const META_H: f32 = 15.0;
/// The gap between two bubbles.
pub const GAP: f32 = 6.0;
/// A date separator pill, and the space it takes with its gap.
pub const SEPARATOR_H: f32 = 22.0;
pub const SEPARATOR_MIN_W: f32 = 96.0;
pub const SEPARATOR_STEP: f32 = 34.0;
/// The widest a bubble gets, and its share of the thread's width.
pub const MAX_BUBBLE: f32 = 520.0;
pub const MAX_BUBBLE_SHARE: f32 = 0.62;
/// The bubbles' corner radius, and the tucked corner's.
pub const RADIUS: f32 = 14.0;
pub const TUCK_RADIUS: f32 = 3.0;
/// How far one wheel notch scrolls.
pub const WHEEL_STEP: f32 = 60.0;

/// What the thread shows: one conversation's messages (oldest first), and an optional
/// search text that keeps only the messages containing it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ThreadData {
    /// Which conversation this is: a change of key scrolls to the bottom.
    pub key: String,
    pub messages: Vec<Message>,
    /// Shows only the messages containing it (case- and accent-insensitive); empty for all.
    pub filter: String,
}

/// The text style a measure is asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextKind {
    /// A message's text (the body face).
    Body,
    /// A bubble's meta line and a date separator (the caption face).
    Caption,
}

/// One laid-out element of the thread, in thread coordinates (y from the top of the content).
#[derive(Debug, Clone, PartialEq)]
pub enum Item {
    /// A date separator pill, centred.
    Separator { label: String, rect: Rect },
    /// A message bubble: which message (index into the data's messages), its box, its lines.
    Bubble { message: usize, rect: Rect, lines: Vec<String>, meta: String, mine: bool },
}

impl Item {
    pub fn rect(&self) -> Rect {
        match self {
            Item::Separator { rect, .. } | Item::Bubble { rect, .. } => *rect,
        }
    }
}

/// The laid-out thread: its items top to bottom, and the content's total height.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Layout {
    pub items: Vec<Item>,
    pub height: f32,
    /// The width it was laid out for.
    pub width: f32,
}

impl Layout {
    /// The items intersecting the band `[top, bottom)` of content y (virtualisation):
    /// a binary search on the sorted tops, then a forward scan.
    pub fn visible(&self, top: f32, bottom: f32) -> std::ops::Range<usize> {
        let start = self.items.partition_point(|i| i.rect().bottom <= top);
        let end = start + self.items[start..].iter().take_while(|i| i.rect().top < bottom).count();
        start..end
    }

    /// The bubble under content point `(x, y)`, if any: the message's index.
    pub fn message_at(&self, x: f32, y: f32) -> Option<usize> {
        let range = self.visible(y, y + 0.5);
        self.items[range].iter().find_map(|i| match i {
            Item::Bubble { message, rect, .. } if rect.contains(x, y) => Some(*message),
            _ => None,
        })
    }
}

/// The tick marks of one of our own messages.
pub fn ticks(status: Status) -> &'static str {
    match status {
        Status::None => "",
        Status::Sent => "✓",
        Status::Delivered | Status::Read => "✓✓",
    }
}

/// A bubble's meta line: the time, and the ticks for our own messages.
pub fn meta_of(m: &Message) -> String {
    if m.mine && m.status != Status::None {
        format!("{}  {}", m.time, ticks(m.status))
    } else {
        m.time.clone()
    }
}

/// Folds case and the common Latin accents, for the search.
pub fn fold(text: &str) -> String {
    text.chars()
        .flat_map(char::to_lowercase)
        .map(|c| match c {
            'à' | 'â' | 'ä' | 'á' | 'ã' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'î' | 'ï' | 'í' | 'ì' => 'i',
            'ô' | 'ö' | 'ó' | 'ò' | 'õ' => 'o',
            'û' | 'ü' | 'ú' | 'ù' => 'u',
            'ç' => 'c',
            'ÿ' => 'y',
            'ñ' => 'n',
            other => other,
        })
        .collect()
}

/// Whether `text` contains `filter` (folded); an empty filter matches everything.
pub fn matches(text: &str, filter: &str) -> bool {
    let filter = filter.trim();
    filter.is_empty() || fold(text).contains(&fold(filter))
}

/// Lays the thread out for a content `width`: a separator before the first message of each
/// day, then one bubble per message, `GAP` apart. `measure` gives a one-line text's width in
/// a face (DirectWrite in the control, a fixed advance in the tests).
pub fn layout(data: &ThreadData, width: f32, measure: &mut dyn FnMut(&str, TextKind) -> f32) -> Layout {
    let max_bubble = (width * MAX_BUBBLE_SHARE).clamp(2.0 * BUBBLE_PAD_X + 24.0, MAX_BUBBLE);
    let inner_max = max_bubble - 2.0 * BUBBLE_PAD_X;
    let mut items = Vec::new();
    let mut y = TOP;
    let mut day: Option<&str> = None;
    for (index, m) in data.messages.iter().enumerate() {
        if !matches(&m.text, &data.filter) {
            continue;
        }
        if !m.day.is_empty() && day != Some(m.day.as_str()) {
            if day.is_some() {
                // A little more air above a separator that follows a bubble.
                y += GAP;
            }
            let w = (measure(&m.day, TextKind::Caption) + 28.0).max(SEPARATOR_MIN_W).min(width.max(SEPARATOR_MIN_W));
            let left = (width - w) / 2.0;
            items.push(Item::Separator { label: m.day.clone(), rect: Rect::new(left, y, left + w, y + SEPARATOR_H) });
            y += SEPARATOR_STEP;
            day = Some(m.day.as_str());
        }
        let lines = kubuno_desktop::ui::display::wrap_lines(&m.text, inner_max, &mut |s| measure(s, TextKind::Body));
        let lines = if lines.is_empty() { vec![String::new()] } else { lines };
        let text_w = lines.iter().map(|l| measure(l, TextKind::Body)).fold(0.0f32, f32::max);
        let meta = meta_of(m);
        let meta_w = measure(&meta, TextKind::Caption);
        let w = (text_w.max(meta_w) + 2.0 * BUBBLE_PAD_X).min(max_bubble);
        let h = 2.0 * BUBBLE_PAD_Y + lines.len() as f32 * LINE_H + META_H;
        let (left, right) = if m.mine { (width - PAD - w, width - PAD) } else { (PAD, PAD + w) };
        items.push(Item::Bubble { message: index, rect: Rect::new(left, y, right, y + h), lines, meta, mine: m.mine });
        y += h + GAP;
    }
    let height = if items.is_empty() { 0.0 } else { y - GAP + TOP };
    Layout { items, height, width }
}

/// The scroll offset after the content changed: a new conversation, or a reader that was at
/// the bottom, goes to the (new) bottom; otherwise the offset stays, clamped.
pub fn follow_scroll(scroll: f32, was_at_bottom: bool, new_key: bool, content: f32, viewport: f32) -> f32 {
    let max = (content - viewport).max(0.0);
    if new_key || was_at_bottom { max } else { scroll.clamp(0.0, max) }
}

/// Raised when a message is activated (double-clicked): which one.
#[derive(kubuno_desktop::views::events::EventArgs, Debug, Clone, Default, PartialEq)]
pub struct MessageActivatedEventArgs {
    /// The message's index in the thread's data.
    pub index: usize,
    /// The server's id (empty for an unsent or sample message).
    pub id: String,
    /// Its text.
    pub text: String,
}

/// A conversation's messages, as bubbles (see the module doc).
#[derive(kubuno_desktop::views::component::Component, Default)]
#[kubuno(extends = Control, overrides(Control))]
#[category("Chat")]
#[toolbox(icon = "messages-square")]
#[default_event("MessageActivated")]
pub struct MessageThread {
    base: ControlCore,
    /// The conversation shown (a binding to a `Shared<ThreadData>`; or set from code).
    #[property]
    #[category("Data")]
    pub messages: Shared<ThreadData>,
    /// Occurs when a message is double-clicked.
    #[event]
    #[category("Action")]
    pub message_activated: Event<MessageActivatedEventArgs>,
    /// Occurs when the reader scrolls to the top of the thread (the place to load older messages).
    #[event]
    #[category("Action")]
    pub reached_top: Event<EmptyEventArgs>,
    /// The scroll offset (content y at the top of the view).
    scroll: f32,
    /// The laid-out data, and what it was laid out from.
    cache: Layout,
    cached_from: Option<Shared<ThreadData>>,
    cached_key: Option<String>,
    /// The bounds of the last paint (the mouse arrives in local coordinates).
    bounds: Rect,
    /// The reader is at the bottom (the thread follows new messages).
    at_bottom: bool,
    /// Dragging the scroll bar's thumb: where it was grabbed (from the thumb's top).
    drag: Option<f32>,
    /// `ReachedTop` was raised for this visit of the top (raised again after leaving it).
    top_reported: bool,
    /// The pointer is over the scroll bar (it widens, as the web's does).
    over_bar: bool,
}

impl MessageThread {
    /// Shows `data` (what the form calls through `Custom<MessageThread>::with`).
    pub fn set_thread(&mut self, data: ThreadData) {
        self.messages = Shared::from(std::sync::Arc::new(data));
        self.invalidate();
    }

    /// The data shown.
    pub fn thread(&self) -> &ThreadData {
        &self.messages
    }

    /// Scrolls to the newest message.
    pub fn scroll_to_bottom(&mut self) {
        self.at_bottom = true;
        self.scroll = f32::MAX;
        self.invalidate();
    }

    /// The scroll offset, in DIP from the top.
    pub fn scroll_offset(&self) -> f32 {
        self.scroll
    }

    fn viewport(&self) -> f32 {
        (self.bounds.bottom - self.bounds.top).max(0.0)
    }

    fn max_scroll(&self) -> f32 {
        (self.cache.height - self.viewport()).max(0.0)
    }

    /// Moves the view to `scroll` (clamped) and updates the bottom/top state.
    fn scroll_to(&mut self, scroll: f32) {
        let before = self.scroll;
        self.scroll = scroll.clamp(0.0, self.max_scroll());
        self.at_bottom = self.scroll >= self.max_scroll() - 1.0;
        if self.scroll != before {
            self.invalidate();
        }
        if self.scroll <= 0.5 && self.max_scroll() > 0.0 {
            if !self.top_reported {
                self.top_reported = true;
                self.raise_reached_top(EmptyEventArgs);
            }
        } else if self.scroll > 40.0 {
            self.top_reported = false;
        }
    }

    /// The data to show: the bound/assigned one, or sample bubbles in the designer.
    fn data(&self) -> Shared<ThreadData> {
        if self.messages.messages.is_empty() && self.design_mode() {
            let sample = crate::model::sample().into_iter().next().map(|c| c.messages).unwrap_or_default();
            return Shared::from(std::sync::Arc::new(ThreadData { key: "design".into(), messages: sample, filter: String::new() }));
        }
        self.messages.clone()
    }

    /// Lays the data out again when it or the width changed, then follows the bottom.
    fn ensure_layout(&mut self, canvas: &dyn Canvas, width: f32) {
        let data = self.data();
        let same_data = self.cached_from.as_ref().is_some_and(|d| d.ptr_eq(&data) || **d == *data);
        if same_data && (self.cache.width - width).abs() < 0.5 {
            return;
        }
        let f = canvas.formats();
        let (body, caption) = (&f.body, &f.caption);
        let mut measure = |s: &str, kind: TextKind| canvas.measure(s, if kind == TextKind::Body { body } else { caption });
        let was_at_bottom = self.at_bottom || self.cached_from.is_none();
        self.cache = layout(&data, width, &mut measure);
        let new_key = self.cached_key.as_deref() != Some(data.key.as_str()) || !same_data && self.cache.items.is_empty();
        self.scroll = follow_scroll(self.scroll, was_at_bottom, new_key, self.cache.height, self.viewport());
        self.at_bottom = self.scroll >= self.max_scroll() - 1.0;
        self.cached_key = Some(data.key.clone());
        self.cached_from = Some(data);
    }

    fn scrollbar(&self) -> Option<kubuno_desktop::ui::range::ScrollBar> {
        kubuno_desktop::ui::range::ScrollBar::from_content(false, self.cache.height, self.viewport(), self.scroll)
    }

    fn paint_item(&self, c: &dyn Canvas, item: &Item, origin: (f32, f32)) {
        let t = c.theme();
        let f = c.formats();
        let at = |r: Rect| Rect::new(r.left + origin.0, r.top + origin.1, r.right + origin.0, r.bottom + origin.1);
        match item {
            Item::Separator { label, rect } => {
                let r = at(*rect);
                let dark = t.mode == kubuno_desktop::ui::ThemeMode::Dark;
                let wash = if dark { Color::rgba_f(1.0, 1.0, 1.0, 0.08) } else { Color::rgba_f(0.0, 0.0, 0.0, 0.06) }.to_d2d();
                c.fill_rounded(&r, SEPARATOR_H / 2.0, &wash);
                c.text(label, &r, &f.caption, &t.text_secondary, true);
            }
            Item::Bubble { rect, lines, meta, mine, .. } => {
                let r = at(*rect);
                let (bg, fg, meta_color) = if *mine {
                    let mut meta = t.accent_foreground;
                    meta.a = 0.75;
                    (t.accent, t.accent_foreground, meta)
                } else {
                    (t.card_background, t.text_primary, t.text_tertiary)
                };
                c.fill_rounded(&r, RADIUS, &bg);
                if !*mine {
                    c.stroke_rounded(&r, RADIUS, &t.card_stroke);
                }
                // The near-bottom corner tucked in, the web's `MessageBubble`.
                let corner = if *mine {
                    Rect::new(r.right - RADIUS, r.bottom - RADIUS, r.right, r.bottom)
                } else {
                    Rect::new(r.left, r.bottom - RADIUS, r.left + RADIUS, r.bottom)
                };
                c.fill_rounded(&corner, TUCK_RADIUS, &bg);
                for (i, line) in lines.iter().enumerate() {
                    let top = r.top + BUBBLE_PAD_Y + i as f32 * LINE_H;
                    let lr = Rect::new(r.left + BUBBLE_PAD_X, top, r.right - BUBBLE_PAD_X + 4.0, top + LINE_H);
                    c.text(line, &lr, &f.body, &fg, false);
                }
                let mr = Rect::new(r.left + BUBBLE_PAD_X, r.bottom - META_H - 3.0, r.right - BUBBLE_PAD_X + 4.0, r.bottom - 3.0);
                c.text(meta, &mr, &f.caption, &meta_color, false);
            }
        }
    }
}

impl Control for MessageThread {
    fn get_preferred_size(&self, _canvas: &dyn Canvas, _proposed: Size) -> Size {
        Size { width: 640.0, height: 480.0 }
    }

    fn on_paint(&mut self, e: &mut PaintEventCx<'_>) {
        let bounds = e.clip_rectangle;
        self.bounds = bounds;
        let g = e.graphics;
        let canvas: &dyn Canvas = g;
        self.ensure_layout(canvas, bounds.right - bounds.left);
        self.scroll = self.scroll.clamp(0.0, self.max_scroll());

        canvas.push_clip(&bounds);
        let range = self.cache.visible(self.scroll, self.scroll + self.viewport());
        let origin = (bounds.left, bounds.top - self.scroll);
        for item in &self.cache.items[range] {
            self.paint_item(canvas, item, origin);
        }
        if let Some(mut bar) = self.scrollbar() {
            bar.expanded = self.drag.is_some() || self.over_bar;
            let rail = bar.rail(&bounds);
            bar.paint(canvas, rail, WidgetState::REST);
        }
        canvas.pop_clip();
        e.raise(self, "OnPaint");
    }

    fn on_mouse_wheel(&mut self, e: &mut EventCx<'_, MouseEventArgs>) {
        let delta = e.args().delta;
        self.scroll_to(self.scroll + delta * WHEEL_STEP);
        e.raise(&*self, "OnMouseWheel");
    }

    fn on_mouse_down(&mut self, e: &mut EventCx<'_, MouseEventArgs>) {
        let (x, y, clicks) = (e.args().x, e.args().y, e.args().clicks);
        let local = Rect::new(0.0, 0.0, self.bounds.right - self.bounds.left, self.viewport());
        if let Some(bar) = self.scrollbar() {
            let rail = bar.rail(&local);
            if rail.contains(x, y) {
                let thumb = bar.thumb_rect(rail);
                if y >= thumb.top && y < thumb.bottom {
                    self.drag = Some(y - thumb.top);
                } else {
                    // A click on the track pages towards it.
                    let page = self.viewport() * 0.9;
                    self.scroll_to(if y < thumb.top { self.scroll - page } else { self.scroll + page });
                }
                self.invalidate();
                e.raise(&*self, "OnMouseDown");
                return;
            }
        }
        if clicks >= 2 {
            if let Some(index) = self.cache.message_at(x, y + self.scroll) {
                let m = self.messages.messages.get(index).cloned().unwrap_or_default();
                self.raise_message_activated(MessageActivatedEventArgs { index, id: m.id, text: m.text });
            }
        }
        e.raise(&*self, "OnMouseDown");
    }

    fn on_mouse_move(&mut self, e: &mut EventCx<'_, MouseEventArgs>) {
        // A thumb drag lasts while the left button is held (a release outside the control may not come back).
        if e.args().button != MouseButton::Left {
            self.drag = None;
        }
        let local = Rect::new(0.0, 0.0, self.bounds.right - self.bounds.left, self.viewport());
        let over = self.scrollbar().is_some_and(|bar| bar.rail(&local).contains(e.args().x, e.args().y));
        if over != self.over_bar {
            self.over_bar = over;
            self.invalidate();
        }
        if let (Some(grab), Some(bar)) = (self.drag, self.scrollbar()) {
            let local = Rect::new(0.0, 0.0, self.bounds.right - self.bounds.left, self.viewport());
            let rail = bar.rail(&local);
            let value = bar.value_at_thumb_start(rail, e.args().y - grab);
            self.scroll_to(value as f32);
        }
        e.raise(&*self, "OnMouseMove");
    }

    fn on_mouse_up(&mut self, e: &mut EventCx<'_, MouseEventArgs>) {
        if self.drag.take().is_some() {
            self.invalidate();
        }
        e.raise(&*self, "OnMouseUp");
    }

    fn on_mouse_leave(&mut self, e: &mut EventCx<'_, EmptyEventArgs>) {
        if std::mem::take(&mut self.over_bar) {
            self.invalidate();
        }
        e.raise(&*self, "OnMouseLeave");
    }

    fn on_key_down(&mut self, e: &mut EventCx<'_, KeyEventArgs>) {
        use kubuno_desktop::controls::host::vk;
        let key = e.args().key.0;
        let page = self.viewport() * 0.9;
        let target = match key {
            k if k == vk::PAGE_UP => Some(self.scroll - page),
            k if k == vk::PAGE_DOWN => Some(self.scroll + page),
            k if k == vk::HOME => Some(0.0),
            k if k == vk::END => Some(f32::MAX),
            k if k == vk::UP => Some(self.scroll - WHEEL_STEP),
            k if k == vk::DOWN => Some(self.scroll + WHEEL_STEP),
            _ => None,
        };
        if let Some(target) = target {
            self.scroll_to(target);
            e.args_mut().handled = true;
        }
        e.raise(&*self, "OnKeyDown");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 7 DIP per character in the body face, 6 in the caption face.
    fn fixed(s: &str, kind: TextKind) -> f32 {
        s.chars().count() as f32 * if kind == TextKind::Body { 7.0 } else { 6.0 }
    }

    fn msg(text: &str, day: &str, mine: bool) -> Message {
        Message { id: String::new(), text: text.into(), time: "10:00".into(), day: day.into(), mine, status: if mine { Status::Read } else { Status::None } }
    }

    #[test]
    fn short_messages_are_one_line_bubbles_on_their_side() {
        let data = ThreadData { key: "a".into(), messages: vec![msg("Salut", "Aujourd'hui", false), msg("Oui", "Aujourd'hui", true)], filter: String::new() };
        let l = layout(&data, 800.0, &mut fixed);
        assert_eq!(l.items.len(), 3, "one separator, two bubbles");
        let Item::Separator { rect: sep, .. } = &l.items[0] else { panic!("separator first") };
        assert_eq!((sep.top, sep.bottom), (TOP, TOP + SEPARATOR_H));
        assert!((sep.left + sep.right - 800.0).abs() < 0.01, "centred");
        let Item::Bubble { rect: a, lines, mine: false, .. } = &l.items[1] else { panic!("other bubble") };
        assert_eq!(lines.len(), 1);
        assert_eq!(a.left, PAD);
        assert_eq!(a.top, TOP + SEPARATOR_STEP);
        // "10:00" = 30 DIP of caption, wider than "Salut" (35)? no: the text is wider.
        assert_eq!(a.right - a.left, 35.0 + 2.0 * BUBBLE_PAD_X);
        assert_eq!(a.bottom - a.top, 2.0 * BUBBLE_PAD_Y + LINE_H + META_H);
        let Item::Bubble { rect: b, meta, mine: true, .. } = &l.items[2] else { panic!("own bubble") };
        assert_eq!(b.right, 800.0 - PAD);
        assert_eq!(meta, "10:00  ✓✓");
        // The meta line is wider than "Oui": it sets the width.
        assert_eq!(b.right - b.left, fixed("10:00  ✓✓", TextKind::Caption) + 2.0 * BUBBLE_PAD_X);
        assert_eq!(b.top, a.bottom + GAP);
        assert_eq!(l.height, b.bottom + TOP);
    }

    #[test]
    fn long_messages_wrap_on_words_at_the_bubble_limit() {
        // 800 wide: max bubble 496, text 472 = 67 characters per line at most.
        let text = "mot ".repeat(40);
        let data = ThreadData { key: "a".into(), messages: vec![msg(text.trim(), "", false)], filter: String::new() };
        let l = layout(&data, 800.0, &mut fixed);
        let Item::Bubble { rect, lines, .. } = &l.items[0] else { panic!("bubble") };
        assert!(lines.len() > 1);
        assert!(lines.iter().all(|line| fixed(line, TextKind::Body) <= 800.0 * MAX_BUBBLE_SHARE - 2.0 * BUBBLE_PAD_X));
        assert!(lines.iter().all(|line| !line.starts_with(' ') && !line.ends_with(' ')), "broken between words");
        assert!(rect.right - rect.left <= 800.0 * MAX_BUBBLE_SHARE + 0.01);
        assert_eq!(rect.bottom - rect.top, 2.0 * BUBBLE_PAD_Y + lines.len() as f32 * LINE_H + META_H);
        // A wide thread caps bubbles at MAX_BUBBLE.
        let wide = layout(&data, 3000.0, &mut fixed);
        assert!(wide.items[0].rect().right - wide.items[0].rect().left <= MAX_BUBBLE + 0.01);
    }

    #[test]
    fn a_separator_starts_each_day_and_the_filter_keeps_matches() {
        let data = ThreadData {
            key: "a".into(),
            messages: vec![msg("un", "Hier", false), msg("deux", "Hier", true), msg("Élan", "Aujourd'hui", false)],
            filter: String::new(),
        };
        let l = layout(&data, 600.0, &mut fixed);
        let separators: Vec<_> = l.items.iter().filter(|i| matches!(i, Item::Separator { .. })).collect();
        assert_eq!(separators.len(), 2);
        let filtered = layout(&ThreadData { filter: "elan".into(), ..data }, 600.0, &mut fixed);
        assert_eq!(filtered.items.len(), 2, "the day of the match and the match");
        assert!(matches!(&filtered.items[1], Item::Bubble { message: 2, .. }));
    }

    #[test]
    fn virtualisation_and_hit_testing() {
        let messages = (0..1000).map(|i| msg(&format!("message {i}"), "", i % 2 == 0)).collect();
        let l = layout(&ThreadData { key: "a".into(), messages, filter: String::new() }, 600.0, &mut fixed);
        let step = l.items[1].rect().top - l.items[0].rect().top;
        let range = l.visible(10_000.0, 10_400.0);
        assert!(range.len() <= (400.0 / step).ceil() as usize + 1, "only what is in view");
        assert!(l.items[range.start].rect().bottom > 10_000.0);
        assert!(l.items[range.end - 1].rect().top < 10_400.0);
        let first = l.items[0].rect();
        assert_eq!(l.message_at(first.right - 1.0, first.top + 1.0), Some(0));
        assert_eq!(l.message_at(300.0, first.top - 5.0), None);
        assert_eq!(l.visible(-100.0, -1.0), 0..0);
    }

    #[test]
    fn the_thread_sticks_to_the_bottom() {
        assert_eq!(follow_scroll(0.0, false, true, 1000.0, 400.0), 600.0, "a new conversation opens at the bottom");
        assert_eq!(follow_scroll(600.0, true, false, 1100.0, 400.0), 700.0, "a reader at the bottom follows new messages");
        assert_eq!(follow_scroll(200.0, false, false, 1100.0, 400.0), 200.0, "a reader scrolled up stays");
        assert_eq!(follow_scroll(900.0, false, false, 300.0, 400.0), 0.0, "short content: no scroll");
        assert_eq!(fold("Élan ÇA"), "elan ca");
        assert!(matches("Réunion demain", "reunion"));
        assert!(!matches("Réunion", "salut"));
        assert_eq!(ticks(Status::Sent), "✓");
    }
}
